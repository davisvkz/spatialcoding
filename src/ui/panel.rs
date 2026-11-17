//! Shared chrome for the docked side panels: slot and bubble frames, the
//! standard header with its kebab menu, labeled sections, switches and
//! segmented controls.
//!
//! See v2.codecanvas.app for the visual reference: each rail entry opens a
//! card with an `[icon] [title] [switch?] [⋮] … [×]` header and labeled,
//! bordered groups below. Its kebab lists per-panel actions such as
//! "Reset <Panel> settings".

use egui::{WidgetInfo, WidgetType};

use crate::ui::icons;
use crate::ui::theme::Theme;

// A SidePanel clips its painting to `exact_width` but reserves whatever its
// frame actually measures, and it only subtracts `inner_margin` when sizing
// the content. Anything wider (content, stroke, outer margin) is reserved but
// never painted, so eframe's near-black clear color shows through. Keep every
// SidePanel frame here margin-only, with sizes that add up exactly.

/// Width of the always-visible icon rail: a 32 px button plus 4 px each side,
/// which also leaves room for the button's 1 px hover/selected expansion.
pub const RAIL_WIDTH: f32 = 40.0;
/// Width of the docked panel slot, gap included.
pub const PANEL_WIDTH: f32 = 300.0;

/// The rail's SidePanel frame: egui's default panel gray, content exactly 32 px.
pub fn rail_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin::symmetric(4, 6))
}

/// The panel slot's SidePanel frame: the gray gap around the bubble. No
/// stroke and no outer margin, so its size adds up to `PANEL_WIDTH`.
pub fn panel_slot_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin::same(6))
}

/// The visible rounded card, drawn inside [`panel_slot_frame`]. Never use it
/// as a SidePanel's own frame: its stroke would overflow the panel width.
pub fn bubble_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme.panel_bg)
        .stroke(egui::Stroke::new(1.0_f32, theme.panel_border))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
}

/// The white call-to-action button of v2 ("Open All 320", "Apply Layout"):
/// white fill, dark medium-weight text. `full_width` stretches it across the
/// available width.
pub fn cta_button(ui: &mut egui::Ui, text: &str, full_width: bool) -> egui::Response {
    let theme = Theme::dark();
    let rich = egui::RichText::new(text)
        .family(egui::FontFamily::Name(icons::MEDIUM_FAMILY.into()))
        .size(if full_width { 12.0 } else { 10.0 })
        .color(theme.cta_fg);
    let button = egui::Button::new(rich)
        .fill(theme.cta_bg)
        .stroke(egui::Stroke::NONE)
        .corner_radius(8.0)
        .min_size(egui::vec2(if full_width { ui.available_width() } else { 0.0 }, 24.0));
    let response = ui.add(button);
    response.widget_info({
        let label = text.to_owned();
        move || WidgetInfo::labeled(WidgetType::Button, true, label.clone())
    });
    response
}

/// One entry of a panel's kebab menu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub icon: &'static str,
    pub label: String,
    /// Disabled items show `disabled_reason` as their tooltip.
    pub enabled: bool,
    pub disabled_reason: Option<&'static str>,
}

impl MenuItem {
    pub fn new(icon: &'static str, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            enabled: true,
            disabled_reason: None,
        }
    }

    /// A menu entry that cannot work here, with the reason as its tooltip.
    pub fn disabled(mut self, reason: &'static str) -> Self {
        self.enabled = false;
        self.disabled_reason = Some(reason);
        self
    }
}

/// What a panel header shows besides its icon and title.
pub struct HeaderSpec<'a> {
    pub icon: &'a str,
    /// Visible title, e.g. "Dependency Edges". Its accessible name is
    /// "<title> panel" so it never collides with the rail button's label.
    pub title: &'a str,
    /// Accessible name of the kebab, e.g. "LSP menu". Defaults to "<title> menu".
    pub menu_label: Option<&'a str>,
    /// A switch right after the title (the CST panel's "Toggle CST nodes").
    pub switch: Option<(&'a str, &'a mut bool)>,
    /// Kebab entries. No kebab is drawn when this is empty.
    pub menu: Vec<MenuItem>,
}

impl<'a> HeaderSpec<'a> {
    pub fn new(icon: &'a str, title: &'a str) -> Self {
        Self {
            icon,
            title,
            menu_label: None,
            switch: None,
            menu: Vec::new(),
        }
    }
}

/// What the user did in a panel header this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HeaderResponse {
    /// The × was clicked: the caller closes its panel.
    pub close: bool,
    /// Index into [`HeaderSpec::menu`] of the kebab entry that was clicked.
    pub menu_clicked: Option<usize>,
}

/// Draws the `[icon] [title] [switch] [⋮] … [×]` header row and its kebab
/// popup. The close button is labeled "Close <title>", as in v2.
pub fn panel_header(ui: &mut egui::Ui, spec: HeaderSpec<'_>) -> HeaderResponse {
    let mut out = HeaderResponse::default();
    let HeaderSpec {
        icon,
        title,
        menu_label,
        switch,
        menu,
    } = spec;
    let menu_label = menu_label.map_or_else(|| format!("{title} menu"), str::to_owned);
    ui.horizontal(|ui| {
        ui.label(icons::icon(icon, 14.0).color(ui.visuals().weak_text_color()));
        ui.add_space(2.0);
        let heading = ui.label(
            egui::RichText::new(title)
                .family(egui::FontFamily::Name(icons::MEDIUM_FAMILY.into()))
                .size(11.0)
                .color(ui.visuals().text_color()),
        );
        let ak_title = format!("{title} panel");
        heading.widget_info(move || WidgetInfo::labeled(WidgetType::Label, true, ak_title.clone()));
        if let Some((label, value)) = switch {
            switch_only(ui, value, label);
        }
        if !menu.is_empty() {
            let kebab = icons::tool_icon_button(ui, icons::KEBAB, &menu_label);
            let popup_id = ui.make_persistent_id(("panel_menu", title));
            if kebab.clicked() {
                ui.memory_mut(|m| m.toggle_popup(popup_id));
            }
            egui::popup_below_widget(
                ui,
                popup_id,
                &kebab,
                egui::PopupCloseBehavior::CloseOnClick,
                |ui| {
                    ui.set_min_width(200.0);
                    for (i, item) in menu.iter().enumerate() {
                        let text = format!("{}  {}", item.icon, item.label);
                        let button = egui::Button::new(text).frame(false);
                        let response = ui.add_enabled(item.enabled, button);
                        let label = item.label.clone();
                        response.widget_info(move || {
                            WidgetInfo::labeled(WidgetType::Button, item.enabled, label.clone())
                        });
                        let response = match item.disabled_reason {
                            Some(reason) => response.on_disabled_hover_text(reason),
                            None => response,
                        };
                        if response.clicked() {
                            out.menu_clicked = Some(i);
                            // Also for keyboard/accessibility clicks, which
                            // CloseOnClick (pointer only) does not see.
                            ui.memory_mut(|m| m.close_popup());
                        }
                    }
                },
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::tool_icon_button(ui, icons::CLOSE, &format!("Close {title}")).clicked() {
                out.close = true;
            }
        });
    });
    ui.add_space(2.0);
    out
}

/// A labeled group: a muted legend over a bordered frame, as the "Style",
/// "Opacity", "Advanced Settings", … groups in the v2 panels. The legend can
/// carry an info tooltip, a switch, and a collapse chevron.
pub struct Section<'a> {
    label: &'a str,
    info: Option<&'a str>,
    switch: Option<(&'a str, &'a mut bool)>,
    collapsible: Option<bool>,
}

impl<'a> Section<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            info: None,
            switch: None,
            collapsible: None,
        }
    }

    /// An ⓘ after the legend whose hover text is `text`, labeled
    /// "<label> info" for accessibility.
    pub fn info(mut self, text: &'a str) -> Self {
        self.info = Some(text);
        self
    }

    /// A switch in the legend (the Edges "Gradient" toggle), labeled `label`.
    pub fn switch(mut self, label: &'a str, value: &'a mut bool) -> Self {
        self.switch = Some((label, value));
        self
    }

    /// A chevron in the legend folds the body; `open_by_default` is the
    /// initial state. The chevron is labeled "Expand <label>" or
    /// "Collapse <label>" like v2's Folders › Advanced.
    pub fn collapsible(mut self, open_by_default: bool) -> Self {
        self.collapsible = Some(open_by_default);
        self
    }

    /// Draws the section. Returns `None` while it is collapsed.
    pub fn show<R>(self, ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
        let Section {
            label,
            info,
            switch,
            collapsible,
        } = self;
        let open_id = ui.make_persistent_id(("section_open", label));
        let mut open = collapsible
            .is_none_or(|default| ui.data_mut(|d| *d.get_temp_mut_or(open_id, default)));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).small().weak());
            if let Some(text) = info {
                info_icon(ui, &format!("{label} info"), text);
            }
            if let Some((switch_label, value)) = switch {
                switch_only(ui, value, switch_label);
            }
            if collapsible.is_some() {
                let (ch, verb) = if open {
                    (icons::CHEVRON_UP, "Collapse")
                } else {
                    (icons::CHEVRON_DOWN, "Expand")
                };
                if icons::tool_icon_button(ui, ch, &format!("{verb} {label}")).clicked() {
                    open = !open;
                    ui.data_mut(|d| d.insert_temp(open_id, open));
                }
            }
        });
        if !open {
            ui.add_space(2.0);
            return None;
        }
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let out = egui::Frame::NONE
            .stroke(stroke)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                add(ui)
            })
            .inner;
        ui.add_space(2.0);
        Some(out)
    }
}

/// Shorthand for a plain [`Section`].
pub fn section<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Section::new(label)
        .show(ui, add)
        .expect("a non-collapsible section is always open")
}

/// An ⓘ glyph whose hover text explains a group, labeled `label`.
pub fn info_icon(ui: &mut egui::Ui, label: &str, text: &str) -> egui::Response {
    let response = ui.add(
        egui::Label::new(icons::icon(icons::INFO, 12.0).weak()).sense(egui::Sense::hover()),
    );
    let ak_label = label.to_owned();
    response.widget_info(move || WidgetInfo::labeled(WidgetType::Label, true, ak_label.clone()));
    response.on_hover_text(text.to_owned())
}

/// A v2-style toggle switch followed by its label, as one widget. Reported
/// to accessibility as a checkbox named `label`, so e2e tests can find and
/// click it; clicking the text toggles it too.
pub fn switch(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let gap = ui.spacing().item_spacing.x + 2.0;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        ui.available_width() - SWITCH_SIZE.x - gap,
        egui::TextStyle::Body,
    );
    let size = egui::vec2(
        SWITCH_SIZE.x + gap + galley.size().x,
        SWITCH_SIZE.y.max(galley.size().y),
    );
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    describe_switch(ui, &response, *value, label);
    if ui.is_rect_visible(rect) {
        let track = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - SWITCH_SIZE.y / 2.0),
            SWITCH_SIZE,
        );
        paint_switch(ui, &response, track, *value);
        let text_pos = egui::pos2(track.right() + gap, rect.center().y - galley.size().y / 2.0);
        let color = ui.style().interact(&response).text_color();
        ui.painter().galley(text_pos, galley, color);
    }
    response
}

/// A disabled [`switch`] with the reason it cannot be used as its tooltip.
pub fn disabled_switch(ui: &mut egui::Ui, value: &mut bool, label: &str, reason: &str) -> egui::Response {
    ui.add_enabled_ui(false, |ui| switch(ui, value, label))
        .inner
        .on_disabled_hover_text(reason.to_owned())
}

const SWITCH_SIZE: egui::Vec2 = egui::vec2(24.0, 14.0);

/// The switch glyph alone; `label` is its accessible name.
fn switch_only(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(SWITCH_SIZE, egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    describe_switch(ui, &response, *value, label);
    if ui.is_rect_visible(rect) {
        paint_switch(ui, &response, rect, *value);
    }
    response
}

fn describe_switch(ui: &egui::Ui, response: &egui::Response, checked: bool, label: &str) {
    let ak_label = label.to_owned();
    let enabled = ui.is_enabled();
    response.widget_info(move || {
        WidgetInfo::selected(WidgetType::Checkbox, enabled, checked, ak_label.clone())
    });
}

fn paint_switch(ui: &egui::Ui, response: &egui::Response, rect: egui::Rect, checked: bool) {
    let t = ui.ctx().animate_bool(response.id, checked);
    let visuals = ui.style().interact_selectable(response, checked);
    let radius = rect.height() / 2.0;
    let track = if checked {
        ui.visuals().widgets.inactive.bg_fill.lerp_to_gamma(egui::Color32::WHITE, 0.15)
    } else {
        ui.visuals().extreme_bg_color
    };
    let painter = ui.painter();
    painter.rect(rect, radius, track, visuals.bg_stroke, egui::StrokeKind::Inside);
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = if checked {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    painter.circle_filled(egui::pos2(knob_x, rect.center().y), radius * 0.62, knob);
}

/// A row of equal-width segments, one selected, as v2's "Left | Center |
/// Right" groups. Each segment is a selectable label named by its text.
/// Returns `true` when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    let fill = ui.visuals().extreme_bg_color;
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let spacing = 2.0;
            let count = options.len().max(1) as f32;
            let width = ((ui.available_width() - spacing * (count - 1.0)) / count).max(1.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = spacing;
                // v2 marks the chosen segment with a lighter gray, not the
                // accent color.
                let visuals = ui.visuals_mut();
                visuals.selection.bg_fill = visuals.widgets.active.bg_fill;
                visuals.selection.stroke.color = visuals.strong_text_color();
                for &(option, label) in options {
                    let selected = *value == option;
                    let response = ui.add_sized(
                        egui::vec2(width, 22.0),
                        egui::SelectableLabel::new(selected, label),
                    );
                    if response.clicked() && !selected {
                        *value = option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A labeled slider row: label on the left, current value on the right,
/// slider below, as v2's "File Spacing  500" rows. Only the slider is an
/// accessible node, named `label`; the texts above it are painted.
pub fn slider_row<N: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
    format: impl Fn(f64) -> String,
) -> egui::Response {
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let (row, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::hover(),
    );
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text_color = if ui.is_enabled() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let painter = ui.painter();
    painter.text(row.left_center(), egui::Align2::LEFT_CENTER, label, font.clone(), text_color);
    painter.text(
        row.right_center(),
        egui::Align2::RIGHT_CENTER,
        format(value.to_f64()),
        font,
        ui.visuals().weak_text_color(),
    );
    ui.spacing_mut().slider_width = ui.available_width();
    let response = ui.add(egui::Slider::new(value, range).show_value(false));
    let ak_label = label.to_owned();
    let enabled = ui.is_enabled();
    response.widget_info(move || WidgetInfo::labeled(WidgetType::Slider, enabled, ak_label.clone()));
    response
}

/// A bordered cluster of icon buttons, as v2's action groups.
pub fn action_group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(add);
        });
}

/// An icon action that is disabled, with `reason` as its tooltip, unless
/// `enabled`. Returns true when clicked.
pub fn icon_action(ui: &mut egui::Ui, enabled: bool, icon: &str, label: &str, reason: &str) -> bool {
    let response = ui
        .add_enabled_ui(enabled, |ui| icons::tool_icon_button(ui, icon, label))
        .inner;
    let response = if reason.is_empty() { response } else { response.on_disabled_hover_text(reason) };
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_menu_items_carry_their_reason() {
        let item = MenuItem::new(icons::PLAY, "Smooth Transition").disabled("no notes backend");
        assert!(!item.enabled);
        assert_eq!(item.disabled_reason, Some("no notes backend"));
        assert!(MenuItem::new(icons::PLAY, "Reset").enabled);
    }
}
