//! Pure edge geometry and weights of the Code Canvas v2 reference
//! (`codecanvas/renderizacao/dependency-edges.md`): curve control points,
//! the gradient gap, arrow size, line widths and the folder-distance opacity.

use egui::{Pos2, Rect};
use std::path::Path;

use crate::edges::style::{GradientGapMode, OpacityMode};

/// Longest bezier handle for an edge that runs backwards, in world px.
pub const BEZIER_MAX_BACKWARDS_REACH: f32 = 3000.0;

/// Cubic bezier leaving `from` and entering `to` horizontally. The handles
/// are `h = dx * k` long (`k` 0–2); a backwards edge (`dx < 0`) caps them at
/// 3000 world px (`zoom` converts that to screen px). `*_dir` is +1 when the
/// edge leaves/enters through a right side and -1 through a left side.
pub fn bezier_points(from: Pos2, from_dir: f32, to: Pos2, to_dir: f32, k: f32, zoom: f32) -> [Pos2; 4] {
    let dx = to.x - from.x;
    let mut reach = dx.abs() * k.clamp(0.0, 2.0);
    if dx < 0.0 {
        reach = reach.min(BEZIER_MAX_BACKWARDS_REACH * zoom);
    }
    [
        from,
        Pos2::new(from.x + reach * from_dir, from.y),
        Pos2::new(to.x + reach * to_dir, to.y),
        to,
    ]
}

/// Point of the cubic `p` at `t`.
pub fn cubic(p: [Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Pos2::new(
        b0 * p[0].x + b1 * p[1].x + b2 * p[2].x + b3 * p[3].x,
        b0 * p[0].y + b1 * p[1].y + b2 * p[2].y + b3 * p[3].y,
    )
}

/// `from → (mid, from.y) → (mid, to.y) → to`, `mid` halfway in x.
pub fn step_points(from: Pos2, to: Pos2) -> Vec<Pos2> {
    let mid_x = (from.x + to.x) * 0.5;
    vec![from, Pos2::new(mid_x, from.y), Pos2::new(mid_x, to.y), to]
}

/// Horizontal, then a 45° diagonal, then horizontal. `Nc = (dx - |dy|) / 2`
/// is each horizontal run; with `|dy| < 1` the edge is a straight line and
/// with `Nc < 0` (no room for the diagonal) it falls back to a step.
pub fn pcb_points(from: Pos2, to: Pos2) -> Vec<Pos2> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    if dy.abs() < 1.0 {
        return vec![from, to];
    }
    let run = (dx - dy.abs()) / 2.0;
    if run < 0.0 {
        return step_points(from, to);
    }
    vec![
        from,
        Pos2::new(from.x + run, from.y),
        Pos2::new(from.x + run + dy.abs(), to.y),
        to,
    ]
}

/// Total length of a polyline.
pub fn polyline_length(points: &[Pos2]) -> f32 {
    points.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

/// The polyline cut into about `pieces` equal-length straight pieces:
/// `(start, end, t at the middle of the piece)`.
pub fn polyline_pieces(points: &[Pos2], pieces: usize) -> Vec<(Pos2, Pos2, f32)> {
    let total = polyline_length(points);
    if total <= 0.0 || pieces == 0 {
        return Vec::new();
    }
    let at = |distance: f32| -> Pos2 {
        let mut left = distance;
        for w in points.windows(2) {
            let len = (w[1] - w[0]).length();
            if left <= len || len == 0.0 {
                let f = if len == 0.0 { 0.0 } else { left / len };
                return w[0] + (w[1] - w[0]) * f;
            }
            left -= len;
        }
        *points.last().unwrap()
    };
    (0..pieces)
        .map(|i| {
            let (a, b) = (i as f32 / pieces as f32, (i + 1) as f32 / pieces as f32);
            (at(a * total), at(b * total), (a + b) * 0.5)
        })
        .collect()
}

/// Share of the edge visible at each end when the gradient gap is on.
///
/// `%` mode erases `0.5 ± p/200` of the length (80 % leaves 10 % at each
/// end); Fixed keeps `fixed_px` world px per end. `None` when the edge is
/// shorter than Min Dist (no gradient: it draws solid).
pub fn gradient_visible_fraction(
    mode: GradientGapMode,
    gap_pct: f32,
    fixed_px: f32,
    min_dist: f32,
    length_world: f32,
) -> Option<f32> {
    if length_world <= 0.0 || length_world < min_dist {
        return None;
    }
    let visible = match mode {
        GradientGapMode::Percent => (1.0 - gap_pct.clamp(0.0, 1.0)) * 0.5,
        GradientGapMode::Fixed => (fixed_px.max(0.0) / length_world).min(0.5),
    };
    Some(visible)
}

/// Fade steps per end ("≤ 20 passos").
pub const GRADIENT_STEPS: f32 = 20.0;

/// Alpha factor of the point at `t` (0 at the source, 1 at the target) of
/// an edge with `visible` share at each end: 1 at the tips, fading to 0 at
/// the gap, in at most [`GRADIENT_STEPS`] steps.
pub fn gradient_alpha(t: f32, visible: f32) -> f32 {
    if visible <= 0.0 {
        return 0.0;
    }
    let d = t.min(1.0 - t).max(0.0);
    if d >= visible {
        return 0.0;
    }
    let f = (d / visible * GRADIENT_STEPS).floor() / GRADIENT_STEPS;
    1.0 - f
}

/// Arrow length in screen px. The reference scales the head by
/// `(1/zoom)^(1 + ln p / ln 1000)` in world space, i.e. `10 * zoom^(-ln p /
/// ln 1000)` on screen (p = Min Size, 0.3–1): 10 px at 100 %, shrinking
/// slowly as the view zooms out. Without scaling it stays 10 px.
pub fn arrow_length(zoom: f32, min_size: f32, scale: bool) -> f32 {
    const BASE: f32 = 10.0;
    if !scale {
        return BASE;
    }
    let p = min_size.clamp(0.3, 1.0);
    BASE * zoom.max(1e-4).powf(-p.ln() / 1000f32.ln())
}

/// Half the arrow's base: 5 px for the 10 px head.
pub fn arrow_half_width(length: f32) -> f32 {
    length * 0.5
}

/// Edge widths in world px: 1.5 base, 2.75 selected, 4 backwards, 4.4 selected
/// backwards.
pub fn edge_width_world(selected: bool, backwards: bool) -> f32 {
    match (selected, backwards) {
        (true, true) => 4.4,
        (false, true) => 4.0,
        (true, false) => 2.75,
        (false, false) => 1.5,
    }
}

/// Screen width: the world width scaled by the zoom, never below the 1 px
/// pixel line the reference falls back to when zoomed out.
pub fn edge_width_px(selected: bool, backwards: bool, zoom: f32) -> f32 {
    (edge_width_world(selected, backwards) * zoom).max(1.0)
}

/// Folders between two files: the steps up from one's folder to the closest
/// folder both share, plus the steps down to the other's.
pub fn folder_distance(a: &Path, b: &Path) -> usize {
    let dirs = |p: &Path| -> Vec<std::ffi::OsString> {
        p.parent()
            .map(|d| d.components().map(|c| c.as_os_str().to_owned()).collect())
            .unwrap_or_default()
    };
    let (da, db) = (dirs(a), dirs(b));
    let common = da.iter().zip(&db).take_while(|(x, y)| x == y).count();
    (da.len() - common) + (db.len() - common)
}

/// Opacity of an idle edge. Depth: 1 inside one folder, .5 up to two
/// folders apart, .25 up to four, else .125. Full ignores the distance.
pub fn depth_opacity(mode: OpacityMode, distance: usize) -> f32 {
    match mode {
        OpacityMode::Full => 1.0,
        OpacityMode::Depth => match distance {
            0 => 1.0,
            1..=2 => 0.5,
            3..=4 => 0.25,
            _ => 0.125,
        },
    }
}

/// An edge from `source` to `target` runs backwards when the importer ends
/// to the right of where the exported file starts:
/// `source.x + source.width > target.x`.
pub fn is_backwards(source: Rect, target: Rect) -> bool {
    source.right() > target.left()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Pos2 {
        Pos2::new(x, y)
    }

    #[test]
    fn bezier_handles_are_dx_times_k_and_cap_only_when_backwards() {
        let [a, c1, c2, b] = bezier_points(p(0.0, 0.0), 1.0, p(400.0, 100.0), -1.0, 0.5, 1.0);
        assert_eq!((a, b), (p(0.0, 0.0), p(400.0, 100.0)));
        assert_eq!(c1, p(200.0, 0.0));
        assert_eq!(c2, p(200.0, 100.0));
        // A forward edge is never capped…
        let [_, c1, ..] = bezier_points(p(0.0, 0.0), 1.0, p(20_000.0, 0.0), -1.0, 1.0, 1.0);
        assert_eq!(c1.x, 20_000.0);
        // …a backwards one is, at 3000 world px (here zoom .5).
        let [_, c1, ..] = bezier_points(p(20_000.0, 0.0), 1.0, p(0.0, 0.0), -1.0, 1.0, 0.5);
        assert_eq!(c1.x - 20_000.0, 1500.0);
        // k outside 0–2 is clamped.
        let [_, c1, ..] = bezier_points(p(0.0, 0.0), 1.0, p(100.0, 0.0), -1.0, 9.0, 1.0);
        assert_eq!(c1.x, 200.0);
    }

    #[test]
    fn pcb_runs_horizontal_diagonal_horizontal() {
        // dx 100, dy 40: Nc = 30, then 40 diagonal, then 30.
        let pts = pcb_points(p(0.0, 0.0), p(100.0, 40.0));
        assert_eq!(pts, vec![p(0.0, 0.0), p(30.0, 0.0), p(70.0, 40.0), p(100.0, 40.0)]);
        // Upwards mirrors the diagonal.
        let pts = pcb_points(p(0.0, 0.0), p(100.0, -40.0));
        assert_eq!(pts[2], p(70.0, -40.0));
        // |dy| < 1 is a straight line; no room for the diagonal is a step.
        assert_eq!(pcb_points(p(0.0, 0.0), p(100.0, 0.5)), vec![p(0.0, 0.0), p(100.0, 0.5)]);
        assert_eq!(pcb_points(p(0.0, 0.0), p(10.0, 100.0)), step_points(p(0.0, 0.0), p(10.0, 100.0)));
    }

    #[test]
    fn step_turns_at_the_midpoint() {
        assert_eq!(
            step_points(p(0.0, 0.0), p(100.0, 50.0)),
            vec![p(0.0, 0.0), p(50.0, 0.0), p(50.0, 50.0), p(100.0, 50.0)]
        );
    }

    #[test]
    fn polyline_pieces_cover_the_whole_length_with_midpoint_t() {
        let pieces = polyline_pieces(&[p(0.0, 0.0), p(100.0, 0.0)], 4);
        assert_eq!(pieces.len(), 4);
        assert_eq!(pieces[0], (p(0.0, 0.0), p(25.0, 0.0), 0.125));
        assert_eq!(pieces[3].1, p(100.0, 0.0));
        assert!(polyline_pieces(&[p(1.0, 1.0), p(1.0, 1.0)], 4).is_empty());
    }

    #[test]
    fn percent_gap_leaves_the_complement_visible_at_each_end() {
        let v = |pct| gradient_visible_fraction(GradientGapMode::Percent, pct, 1000.0, 0.0, 5000.0).unwrap();
        assert!((v(0.8) - 0.1).abs() < 1e-6, "80% gap leaves 10% per end");
        assert_eq!(v(0.0), 0.5);
        assert_eq!(v(1.0), 0.0);
    }

    #[test]
    fn fixed_gap_keeps_px_per_end_and_min_dist_disables_it() {
        let f = |len| gradient_visible_fraction(GradientGapMode::Fixed, 0.8, 1000.0, 40_000.0, len);
        assert_eq!(f(39_999.0), None, "shorter than Min Dist draws solid");
        assert_eq!(f(50_000.0), Some(0.02));
        let short = gradient_visible_fraction(GradientGapMode::Fixed, 0.8, 1000.0, 0.0, 1500.0);
        assert_eq!(short, Some(0.5), "never more than half per end");
    }

    #[test]
    fn gradient_alpha_fades_from_the_tips_to_the_gap() {
        assert_eq!(gradient_alpha(0.0, 0.1), 1.0);
        assert_eq!(gradient_alpha(1.0, 0.1), 1.0);
        assert_eq!(gradient_alpha(0.5, 0.1), 0.0, "inside the gap");
        assert_eq!(gradient_alpha(0.1, 0.1), 0.0, "gap starts at the visible share");
        let mid = gradient_alpha(0.05, 0.1);
        assert!((mid - 0.5).abs() < 0.06, "{mid}");
        assert!(gradient_alpha(0.02, 0.1) > gradient_alpha(0.08, 0.1));
        assert_eq!(gradient_alpha(0.0, 0.0), 0.0, "no visible share, no stroke");
    }

    #[test]
    fn arrow_length_follows_the_reference_scale_formula() {
        // p = 1: constant 10 px. p = .5 at zoom 1: 10 px.
        assert!((arrow_length(0.01, 1.0, true) - 10.0).abs() < 1e-3);
        assert!((arrow_length(1.0, 0.5, true) - 10.0).abs() < 1e-3);
        // p = .5 at zoom .01: 10 * .01^(0.1003) ≈ 6.3 px.
        assert!((arrow_length(0.01, 0.5, true) - 6.3).abs() < 0.1);
        // Smaller p shrinks faster; scaling off keeps 10 px.
        assert!(arrow_length(0.01, 0.3, true) < arrow_length(0.01, 0.5, true));
        assert_eq!(arrow_length(0.01, 0.5, false), 10.0);
        assert_eq!(arrow_half_width(10.0), 5.0);
    }

    #[test]
    fn widths_follow_selection_and_backwards_with_a_one_pixel_floor() {
        assert_eq!(edge_width_world(false, false), 1.5);
        assert_eq!(edge_width_world(true, false), 2.75);
        assert_eq!(edge_width_world(false, true), 4.0);
        assert_eq!(edge_width_world(true, true), 4.4);
        assert_eq!(edge_width_px(false, false, 1.0), 1.5);
        assert_eq!(edge_width_px(true, true, 0.5), 2.2);
        assert_eq!(edge_width_px(false, false, 0.1), 1.0);
    }

    #[test]
    fn folder_distance_counts_steps_through_the_closest_shared_folder() {
        let d = |a: &str, b: &str| folder_distance(Path::new(a), Path::new(b));
        assert_eq!(d("src/a.rs", "src/b.rs"), 0);
        assert_eq!(d("src/a.rs", "src/x/b.rs"), 1);
        assert_eq!(d("src/x/a.rs", "src/y/b.rs"), 2);
        assert_eq!(d("src/x/y/a.rs", "lib/b.rs"), 3 + 1);
        assert_eq!(d("a.rs", "b.rs"), 0);
    }

    #[test]
    fn depth_opacity_bands_and_full_mode() {
        let depth = |d| depth_opacity(OpacityMode::Depth, d);
        assert_eq!([depth(0), depth(1), depth(2), depth(3), depth(4), depth(5)], [1.0, 0.5, 0.5, 0.25, 0.25, 0.125]);
        assert_eq!(depth_opacity(OpacityMode::Full, 9), 1.0);
    }

    #[test]
    fn backwards_when_the_importer_ends_right_of_the_exporter_start() {
        let src = Rect::from_min_size(p(0.0, 0.0), egui::vec2(700.0, 100.0));
        let right_of = Rect::from_min_size(p(1200.0, 0.0), egui::vec2(700.0, 100.0));
        let overlapping = Rect::from_min_size(p(600.0, 0.0), egui::vec2(700.0, 100.0));
        assert!(!is_backwards(src, right_of));
        assert!(is_backwards(src, overlapping));
        assert!(is_backwards(right_of, src));
    }
}
