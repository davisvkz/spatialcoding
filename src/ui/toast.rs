//! Short messages at the bottom of the canvas ("Shift+click to pin multiple
//! symbols…"): they fade after a few seconds.

use crate::state::AppState;

/// Seconds a toast stays.
pub const LIFETIME: f64 = 5.0;
/// Seconds a toast takes to fade out at the end.
const FADE: f64 = 0.6;
/// Most toasts kept at once; the oldest is dropped.
const MAX_TOASTS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub text: String,
    /// App time it was first drawn; `None` until then.
    pub born: Option<f64>,
}

/// Queues a message; the same text is not stacked twice.
pub fn push(state: &mut AppState, text: impl Into<String>) {
    let text = text.into();
    if state.toasts.iter().any(|t| t.text == text) {
        return;
    }
    state.toasts.push(Toast { text, born: None });
    if state.toasts.len() > MAX_TOASTS {
        state.toasts.remove(0);
    }
}

/// Drops the expired toasts and draws the rest. Keeps repainting while any
/// is on screen so it can expire.
pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let now = ctx.input(|i| i.time);
    for toast in &mut state.toasts {
        toast.born.get_or_insert(now);
    }
    state.toasts.retain(|t| now - t.born.unwrap_or(now) < LIFETIME);
    if state.toasts.is_empty() {
        return;
    }
    // Wake once when the first toast starts to fade, then often while any
    // fades; never a steady tick (it would keep an idle UI busy).
    let fade_in = state
        .toasts
        .iter()
        .map(|t| t.born.unwrap_or(now) + LIFETIME - FADE - now)
        .fold(f64::INFINITY, f64::min);
    let wake = if fade_in > 0.0 { fade_in.max(0.5) } else { 0.1 };
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(wake));
    let theme = &state.theme;
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -36.0])
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            for toast in &state.toasts {
                let left = LIFETIME - (now - toast.born.unwrap_or(now));
                let alpha = (left / FADE).clamp(0.0, 1.0) as f32;
                egui::Frame::NONE
                    .fill(theme.control_bg.gamma_multiply(alpha))
                    .stroke(egui::Stroke::new(1.0_f32, theme.node_border.gamma_multiply(alpha)))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(&toast.text).color(theme.text_secondary.gamma_multiply(alpha)));
                    });
                ui.add_space(4.0);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_is_not_queued_twice_and_the_oldest_makes_room() {
        let mut state = AppState::default();
        push(&mut state, "a");
        push(&mut state, "a");
        assert_eq!(state.toasts.len(), 1);
        for n in 0..6 {
            push(&mut state, format!("t{n}"));
        }
        assert_eq!(state.toasts.len(), MAX_TOASTS);
        assert_eq!(state.toasts.last().unwrap().text, "t5");
    }
}
