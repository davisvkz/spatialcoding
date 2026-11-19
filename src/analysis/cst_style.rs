//! How the CST is tinted on the canvas (`codecanvas/renderizacao/cst.md`):
//! the role families and their colors (`DEFAULT_CST` of the reference), the
//! kinds hidden by default and the per-type style (fill, border).

use serde::{Deserialize, Serialize};

/// The families of the reference's `DEFAULT_CST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Function,
    Class,
    Variable,
    Conditional,
    Loop,
    Switch,
    ErrorHandling,
    Return,
    Call,
    Member,
    Expression,
    Jsx,
    Import,
    Export,
    Default,
}

impl Family {
    pub fn color(self) -> [u8; 3] {
        let hex: u32 = match self {
            Self::Function => 0x3B82F6,
            Self::Class => 0xA855F7,
            Self::Variable => 0x22C55E,
            Self::Conditional => 0xF97316,
            Self::Loop => 0xEC4899,
            Self::Switch => 0xEAB308,
            Self::ErrorHandling => 0xEF4444,
            Self::Return => 0x06B6D4,
            Self::Call => 0x0EA5E9,
            Self::Member => 0x6366F1,
            Self::Expression => 0x9CA3AF,
            Self::Jsx => 0x22D3EE,
            Self::Import => 0xFB923C,
            Self::Export => 0xA3E635,
            Self::Default => 0x94A3B8,
        };
        [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
    }
}

/// The family of a grammar node kind (tree-sitter kinds of every bundled
/// grammar).
pub fn family_of(kind: &str) -> Family {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| kind.starts_with(p));
    if kind.contains("import") || matches!(kind, "use_declaration" | "package_clause") {
        Family::Import
    } else if kind.contains("export") {
        Family::Export
    } else if kind.starts_with("jsx_") {
        Family::Jsx
    } else if kind.contains("function") || kind.contains("method") || kind == "closure_expression" {
        Family::Function
    } else if starts(&["class", "interface", "struct", "trait", "type_", "enum", "impl_", "mod_", "decorated_", "abstract_"]) {
        Family::Class
    } else if matches!(
        kind,
        "try_statement" | "catch_clause" | "finally_clause" | "throw_statement" | "try_expression" | "except_clause"
    ) {
        Family::ErrorHandling
    } else if matches!(
        kind,
        "for_statement" | "for_in_statement" | "while_statement" | "do_statement" | "loop_expression"
            | "for_expression" | "while_expression"
    ) {
        Family::Loop
    } else if matches!(kind, "switch_statement" | "switch_body" | "switch_case" | "match_expression" | "expression_switch_statement") {
        Family::Switch
    } else if matches!(kind, "if_statement" | "if_expression" | "else_clause" | "ternary_expression") {
        Family::Conditional
    } else if kind == "return_statement" || kind == "return_expression" {
        Family::Return
    } else if matches!(kind, "call_expression" | "new_expression" | "macro_invocation" | "call") {
        Family::Call
    } else if matches!(kind, "member_expression" | "field_expression" | "subscript_expression" | "attribute") {
        Family::Member
    } else if kind.contains("declaration")
        || kind.contains("assignment")
        || matches!(kind, "const_item" | "static_item" | "property_signature" | "public_field_definition")
    {
        Family::Variable
    } else if matches!(kind, "expression_statement" | "binary_expression" | "unary_expression" | "parenthesized_expression") {
        Family::Expression
    } else {
        Family::Default
    }
}

/// Kinds the reference hides by default (only a left border, no fill).
pub fn hidden_by_default(kind: &str) -> bool {
    matches!(kind, "if_statement" | "else_clause" | "switch_statement" | "if_expression" | "match_expression")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FillMode {
    #[default]
    Solid,
    Gradient,
}

/// Which sides of the tint rectangle get the border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sides {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

impl Sides {
    pub const ALL: Self = Self { top: true, right: true, bottom: true, left: true };
    pub const LEFT_ONLY: Self = Self { top: false, right: false, bottom: false, left: true };
}

/// The style of one node kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CstTypeStyle {
    pub visible: bool,
    pub fill: FillMode,
    pub color: [u8; 3],
    /// End color of a gradient fill.
    pub color_end: [u8; 3],
    /// Fill alpha, 0–1, on top of the level's base alpha (4 % in the editor).
    pub fill_alpha: f32,
    pub border_color: [u8; 3],
    /// Width in screen px at zoom 1; divided by the zoom when drawn (0.5).
    pub border_width: f32,
    pub border_alpha: f32,
    pub sides: Sides,
}

impl Default for CstTypeStyle {
    fn default() -> Self {
        let color = Family::Default.color();
        Self {
            visible: true,
            fill: FillMode::Solid,
            color,
            color_end: color,
            fill_alpha: 1.0,
            border_color: color,
            border_width: 0.5,
            border_alpha: 0.5,
            sides: Sides::ALL,
        }
    }
}

impl CstTypeStyle {
    /// The reference's default for `kind`: its family color, and for the
    /// hidden kinds only a left border.
    pub fn default_for(kind: &str) -> Self {
        let color = family_of(kind).color();
        let hidden = hidden_by_default(kind);
        Self {
            visible: !hidden,
            color,
            color_end: color,
            border_color: color,
            fill_alpha: if hidden { 0.0 } else { 1.0 },
            sides: if hidden { Sides::LEFT_ONLY } else { Sides::ALL },
            ..Default::default()
        }
    }

    /// Stroke alpha: `min(2 · fill alpha, 1)` for a node with a fill (the
    /// fill alpha being the level's base alpha times the type's); a node
    /// without one (the hidden kinds) uses the border alpha.
    pub fn stroke_alpha(&self, fill_alpha: f32) -> f32 {
        if fill_alpha > 0.0 {
            (fill_alpha * 2.0).min(1.0)
        } else {
            self.border_alpha.clamp(0.0, 1.0)
        }
    }

    /// Border width on screen at `zoom` (the stored width is divided by it,
    /// so the line keeps its size in world space).
    pub fn border_px(&self, zoom: f32) -> f32 {
        self.border_width / zoom.max(1e-4)
    }
}

/// Opacity of the tint of a node under a hovered one: by nesting level
/// `[1, .5, .25, .12]`, the last for every deeper level.
pub fn hover_nesting_opacity(level: usize) -> f32 {
    [1.0, 0.5, 0.25, 0.12][level.min(3)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_have_the_reference_colors() {
        let hex = |f: Family| {
            let [r, g, b] = f.color();
            format!("{r:02X}{g:02X}{b:02X}")
        };
        let expected = [
            (Family::Function, "3B82F6"),
            (Family::Class, "A855F7"),
            (Family::Variable, "22C55E"),
            (Family::Conditional, "F97316"),
            (Family::Loop, "EC4899"),
            (Family::Switch, "EAB308"),
            (Family::ErrorHandling, "EF4444"),
            (Family::Return, "06B6D4"),
            (Family::Call, "0EA5E9"),
            (Family::Member, "6366F1"),
            (Family::Expression, "9CA3AF"),
            (Family::Jsx, "22D3EE"),
            (Family::Import, "FB923C"),
            (Family::Export, "A3E635"),
            (Family::Default, "94A3B8"),
        ];
        for (family, color) in expected {
            assert_eq!(hex(family), color, "{family:?}");
        }
    }

    #[test]
    fn kinds_of_every_grammar_land_in_their_family() {
        let cases = [
            ("function_item", Family::Function),
            ("arrow_function", Family::Function),
            ("class_declaration", Family::Class),
            ("struct_item", Family::Class),
            ("type_alias_declaration", Family::Class),
            ("lexical_declaration", Family::Variable),
            ("if_statement", Family::Conditional),
            ("for_statement", Family::Loop),
            ("switch_statement", Family::Switch),
            ("match_expression", Family::Switch),
            ("try_statement", Family::ErrorHandling),
            ("throw_statement", Family::ErrorHandling),
            ("return_statement", Family::Return),
            ("call_expression", Family::Call),
            ("member_expression", Family::Member),
            ("expression_statement", Family::Expression),
            ("jsx_element", Family::Jsx),
            ("import_statement", Family::Import),
            ("use_declaration", Family::Import),
            ("export_statement", Family::Export),
            ("identifier", Family::Default),
        ];
        for (kind, family) in cases {
            assert_eq!(family_of(kind), family, "{kind}");
        }
    }

    #[test]
    fn if_else_and_switch_are_hidden_with_only_a_left_border() {
        for kind in ["if_statement", "else_clause", "switch_statement"] {
            let style = CstTypeStyle::default_for(kind);
            assert!(!style.visible && style.fill_alpha == 0.0, "{kind}");
            assert_eq!(style.sides, Sides::LEFT_ONLY);
        }
        let shown = CstTypeStyle::default_for("function_item");
        assert!(shown.visible);
        assert_eq!(shown.sides, Sides::ALL);
        assert_eq!(shown.border_width, 0.5);
    }

    #[test]
    fn stroke_alpha_doubles_up_to_one_and_border_width_divides_by_zoom() {
        let style = CstTypeStyle::default();
        assert!((style.stroke_alpha(0.04) - 0.08).abs() < 1e-6);
        assert!((style.stroke_alpha(0.10) - 0.20).abs() < 1e-6);
        assert_eq!(style.stroke_alpha(0.9), 1.0, "capped at 1");
        assert_eq!(CstTypeStyle::default_for("if_statement").stroke_alpha(0.0), 0.5);
        assert_eq!(style.border_px(0.25), 2.0);
    }

    #[test]
    fn hover_opacity_falls_by_nesting() {
        assert_eq!([0, 1, 2, 3, 9].map(hover_nesting_opacity), [1.0, 0.5, 0.25, 0.12, 0.12]);
    }
}
