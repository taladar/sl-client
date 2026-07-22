//! Bevy → engine keyboard mapping for web-media surfaces.
//!
//! CEF's portable key vocabulary is *Windows virtual-key codes* (its
//! `windows_key_code` field is a VK code on every platform), so this module
//! maps Bevy's **physical** [`KeyCode`] onto [`sl_cef::vk`] once, and every
//! browser consumer (the UI widget, the in-world media face) shares it.
//! Printable text does NOT travel through here — it arrives as the keyboard
//! event's committed `text` and is fed via
//! [`sl_cef::MediaSurface::insert_text`], so non-US layouts and IME output
//! are correct by construction. The VK code is what pages see as
//! `keydown.keyCode` for navigation / shortcut keys.

use bevy::input::keyboard::KeyCode;
use sl_cef::vk;

/// The Windows virtual-key code for a Bevy physical key, or `None` for keys a
/// browser page has no use for (media keys, IME keys, …).
#[expect(
    clippy::too_many_lines,
    reason = "a keyboard has this many keys; splitting the table would only obscure it"
)]
pub(crate) const fn vk_for_key_code(code: KeyCode) -> Option<i32> {
    Some(match code {
        KeyCode::Backspace => vk::BACK,
        KeyCode::Tab => vk::TAB,
        KeyCode::Enter | KeyCode::NumpadEnter => vk::RETURN,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => vk::SHIFT,
        KeyCode::ControlLeft | KeyCode::ControlRight => vk::CONTROL,
        KeyCode::AltLeft | KeyCode::AltRight => vk::MENU,
        KeyCode::Pause => vk::PAUSE,
        KeyCode::CapsLock => vk::CAPITAL,
        KeyCode::Escape => vk::ESCAPE,
        KeyCode::Space => vk::SPACE,
        KeyCode::PageUp => vk::PRIOR,
        KeyCode::PageDown => vk::NEXT,
        KeyCode::End => vk::END,
        KeyCode::Home => vk::HOME,
        KeyCode::ArrowLeft => vk::LEFT,
        KeyCode::ArrowUp => vk::UP,
        KeyCode::ArrowRight => vk::RIGHT,
        KeyCode::ArrowDown => vk::DOWN,
        KeyCode::PrintScreen => vk::SNAPSHOT,
        KeyCode::Insert => vk::INSERT,
        KeyCode::Delete => vk::DELETE,
        KeyCode::Digit0 => vk::DIGIT_0,
        KeyCode::Digit1 => vk::DIGIT_0.saturating_add(1),
        KeyCode::Digit2 => vk::DIGIT_0.saturating_add(2),
        KeyCode::Digit3 => vk::DIGIT_0.saturating_add(3),
        KeyCode::Digit4 => vk::DIGIT_0.saturating_add(4),
        KeyCode::Digit5 => vk::DIGIT_0.saturating_add(5),
        KeyCode::Digit6 => vk::DIGIT_0.saturating_add(6),
        KeyCode::Digit7 => vk::DIGIT_0.saturating_add(7),
        KeyCode::Digit8 => vk::DIGIT_0.saturating_add(8),
        KeyCode::Digit9 => vk::DIGIT_0.saturating_add(9),
        KeyCode::KeyA => vk::LETTER_A,
        KeyCode::KeyB => vk::LETTER_A.saturating_add(1),
        KeyCode::KeyC => vk::LETTER_A.saturating_add(2),
        KeyCode::KeyD => vk::LETTER_A.saturating_add(3),
        KeyCode::KeyE => vk::LETTER_A.saturating_add(4),
        KeyCode::KeyF => vk::LETTER_A.saturating_add(5),
        KeyCode::KeyG => vk::LETTER_A.saturating_add(6),
        KeyCode::KeyH => vk::LETTER_A.saturating_add(7),
        KeyCode::KeyI => vk::LETTER_A.saturating_add(8),
        KeyCode::KeyJ => vk::LETTER_A.saturating_add(9),
        KeyCode::KeyK => vk::LETTER_A.saturating_add(10),
        KeyCode::KeyL => vk::LETTER_A.saturating_add(11),
        KeyCode::KeyM => vk::LETTER_A.saturating_add(12),
        KeyCode::KeyN => vk::LETTER_A.saturating_add(13),
        KeyCode::KeyO => vk::LETTER_A.saturating_add(14),
        KeyCode::KeyP => vk::LETTER_A.saturating_add(15),
        KeyCode::KeyQ => vk::LETTER_A.saturating_add(16),
        KeyCode::KeyR => vk::LETTER_A.saturating_add(17),
        KeyCode::KeyS => vk::LETTER_A.saturating_add(18),
        KeyCode::KeyT => vk::LETTER_A.saturating_add(19),
        KeyCode::KeyU => vk::LETTER_A.saturating_add(20),
        KeyCode::KeyV => vk::LETTER_A.saturating_add(21),
        KeyCode::KeyW => vk::LETTER_A.saturating_add(22),
        KeyCode::KeyX => vk::LETTER_A.saturating_add(23),
        KeyCode::KeyY => vk::LETTER_A.saturating_add(24),
        KeyCode::KeyZ => vk::LETTER_A.saturating_add(25),
        KeyCode::SuperLeft => vk::LWIN,
        KeyCode::SuperRight => vk::RWIN,
        KeyCode::Numpad0 => vk::NUMPAD_0,
        KeyCode::Numpad1 => vk::NUMPAD_0.saturating_add(1),
        KeyCode::Numpad2 => vk::NUMPAD_0.saturating_add(2),
        KeyCode::Numpad3 => vk::NUMPAD_0.saturating_add(3),
        KeyCode::Numpad4 => vk::NUMPAD_0.saturating_add(4),
        KeyCode::Numpad5 => vk::NUMPAD_0.saturating_add(5),
        KeyCode::Numpad6 => vk::NUMPAD_0.saturating_add(6),
        KeyCode::Numpad7 => vk::NUMPAD_0.saturating_add(7),
        KeyCode::Numpad8 => vk::NUMPAD_0.saturating_add(8),
        KeyCode::Numpad9 => vk::NUMPAD_0.saturating_add(9),
        KeyCode::NumpadMultiply => vk::MULTIPLY,
        KeyCode::NumpadAdd => vk::ADD,
        KeyCode::NumpadSubtract => vk::SUBTRACT,
        KeyCode::NumpadDecimal => vk::DECIMAL,
        KeyCode::NumpadDivide => vk::DIVIDE,
        KeyCode::F1 => vk::F1,
        KeyCode::F2 => vk::F1.saturating_add(1),
        KeyCode::F3 => vk::F1.saturating_add(2),
        KeyCode::F4 => vk::F1.saturating_add(3),
        KeyCode::F5 => vk::F1.saturating_add(4),
        KeyCode::F6 => vk::F1.saturating_add(5),
        KeyCode::F7 => vk::F1.saturating_add(6),
        KeyCode::F8 => vk::F1.saturating_add(7),
        KeyCode::F9 => vk::F1.saturating_add(8),
        KeyCode::F10 => vk::F1.saturating_add(9),
        KeyCode::F11 => vk::F1.saturating_add(10),
        KeyCode::F12 => vk::F1.saturating_add(11),
        KeyCode::NumLock => vk::NUMLOCK,
        KeyCode::ScrollLock => vk::SCROLL,
        KeyCode::Semicolon => vk::OEM_1,
        KeyCode::Equal => vk::OEM_PLUS,
        KeyCode::Comma => vk::OEM_COMMA,
        KeyCode::Minus => vk::OEM_MINUS,
        KeyCode::Period => vk::OEM_PERIOD,
        KeyCode::Slash => vk::OEM_2,
        KeyCode::Backquote => vk::OEM_3,
        KeyCode::BracketLeft => vk::OEM_4,
        KeyCode::Backslash => vk::OEM_5,
        KeyCode::BracketRight => vk::OEM_6,
        KeyCode::Quote => vk::OEM_7,
        _ => return None,
    })
}

/// The current modifier state as the engine's portable [`sl_cef::Modifiers`].
pub(crate) fn current_modifiers(
    keyboard: &bevy::input::ButtonInput<KeyCode>,
    mouse: &bevy::input::ButtonInput<bevy::input::mouse::MouseButton>,
) -> sl_cef::Modifiers {
    sl_cef::Modifiers {
        shift: keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight),
        control: keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight),
        alt: keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight),
        left_button: mouse.pressed(bevy::input::mouse::MouseButton::Left),
    }
}

/// Whether a committed text string is worth sending to a page as character
/// input: non-empty and free of control characters (Enter / Backspace / Tab
/// travel as key events, not text).
pub(crate) fn is_printable_text(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::KeyCode;
    use pretty_assertions::assert_eq;

    use super::{is_printable_text, vk_for_key_code};

    #[test]
    fn navigation_keys_map_to_their_vk_codes() {
        assert_eq!(vk_for_key_code(KeyCode::Enter), Some(0x0D));
        assert_eq!(vk_for_key_code(KeyCode::Backspace), Some(0x08));
        assert_eq!(vk_for_key_code(KeyCode::ArrowLeft), Some(0x25));
        assert_eq!(vk_for_key_code(KeyCode::ArrowDown), Some(0x28));
        assert_eq!(vk_for_key_code(KeyCode::PageUp), Some(0x21));
        assert_eq!(vk_for_key_code(KeyCode::Home), Some(0x24));
        assert_eq!(vk_for_key_code(KeyCode::Delete), Some(0x2E));
    }

    #[test]
    fn letters_digits_and_function_keys_map_contiguously() {
        assert_eq!(vk_for_key_code(KeyCode::KeyA), Some(0x41));
        assert_eq!(vk_for_key_code(KeyCode::KeyZ), Some(0x5A));
        assert_eq!(vk_for_key_code(KeyCode::Digit0), Some(0x30));
        assert_eq!(vk_for_key_code(KeyCode::Digit9), Some(0x39));
        assert_eq!(vk_for_key_code(KeyCode::F1), Some(0x70));
        assert_eq!(vk_for_key_code(KeyCode::F12), Some(0x7B));
        assert_eq!(vk_for_key_code(KeyCode::Numpad5), Some(0x65));
    }

    #[test]
    fn unmapped_keys_yield_none() {
        assert_eq!(vk_for_key_code(KeyCode::LaunchMail), None);
    }

    #[test]
    fn printable_text_filter() {
        assert!(is_printable_text("a"));
        assert!(is_printable_text("ß"));
        assert!(is_printable_text("日本"));
        assert!(!is_printable_text(""));
        assert!(!is_printable_text("\r"));
        assert!(!is_printable_text("\u{8}"));
        assert!(!is_printable_text("\t"));
    }
}
