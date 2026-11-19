//! A keyboard shortcut: a key plus exactly the modifiers it needs.

use egui::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    /// Ctrl (Cmd on macOS).
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    /// Another key that triggers the same chord (`=` for `+`).
    pub alt_key: Option<Key>,
}

impl Chord {
    pub const fn new(key: Key) -> Self {
        Self { key, command: false, shift: false, alt: false, alt_key: None }
    }

    pub const fn shift(key: Key) -> Self {
        Self { shift: true, ..Self::new(key) }
    }

    pub const fn alt(key: Key) -> Self {
        Self { alt: true, ..Self::new(key) }
    }

    pub const fn ctrl(key: Key) -> Self {
        Self { command: true, ..Self::new(key) }
    }

    pub const fn ctrl_shift(key: Key) -> Self {
        Self { command: true, shift: true, ..Self::new(key) }
    }

    pub const fn ctrl_alt_shift(key: Key) -> Self {
        Self { command: true, shift: true, alt: true, ..Self::new(key) }
    }

    pub const fn or_key(self, key: Key) -> Self {
        Self { alt_key: Some(key), ..self }
    }

    /// True for `key` with exactly these modifiers: an extra Alt, Shift or
    /// Ctrl means another shortcut. Zoom (`+`) also takes Shift, as a US
    /// keyboard types it with Shift.
    pub fn matches(&self, key: Key, modifiers: Modifiers) -> bool {
        if key != self.key && Some(key) != self.alt_key {
            return false;
        }
        let shift_free = matches!(self.key, Key::Plus) && self.alt_key.is_some();
        modifiers.command == self.command
            && modifiers.alt == self.alt
            && (shift_free || modifiers.shift == self.shift)
    }

    /// A key typed into text fields too: no Ctrl or Alt.
    pub fn is_plain(&self) -> bool {
        !self.command && !self.alt
    }

    /// `Ctrl+Shift+O`.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.command {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }
}

fn key_name(key: Key) -> String {
    match key {
        Key::Slash => "/".into(),
        Key::Backslash => "\\".into(),
        Key::Period => ".".into(),
        Key::Comma => ",".into(),
        Key::Plus => "+".into(),
        Key::Minus => "-".into(),
        Key::Backspace => "Backspace".into(),
        Key::Escape => "Esc".into(),
        Key::Num1 => "1".into(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_list_modifiers_in_a_fixed_order() {
        assert_eq!(Chord::ctrl_alt_shift(Key::O).label(), "Ctrl+Alt+Shift+O");
        assert_eq!(Chord::ctrl(Key::K).label(), "Ctrl+K");
        assert_eq!(Chord::shift(Key::Backslash).label(), "Shift+\\");
        assert_eq!(Chord::new(Key::Num1).label(), "1");
        assert_eq!(Chord::ctrl(Key::Slash).label(), "Ctrl+/");
    }

    #[test]
    fn zoom_in_takes_equals_and_shifted_plus() {
        let zoom_in = Chord::ctrl(Key::Plus).or_key(Key::Equals);
        let ctrl = Modifiers { command: true, ctrl: true, ..Default::default() };
        let ctrl_shift = Modifiers { shift: true, ..ctrl };
        assert!(zoom_in.matches(Key::Equals, ctrl));
        assert!(zoom_in.matches(Key::Plus, ctrl_shift));
        assert!(!zoom_in.matches(Key::Minus, ctrl));
    }
}
