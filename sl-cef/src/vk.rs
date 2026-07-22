//! Windows virtual-key codes, used as the portable key vocabulary of
//! [`crate::KeyInput`] on **every** platform.
//!
//! CEF's `cef_key_event_t::windows_key_code` field expects Windows `VK_*`
//! values even on Linux and macOS (Chromium's cross-platform convention), so
//! a viewer maps its native/toolkit keycodes to these once and the engine
//! side never sees a native key blob. Only the keys a browser page can
//! meaningfully react to are listed; printable text travels separately as
//! committed text.

/// `VK_BACK` — Backspace.
pub const BACK: i32 = 0x08;
/// `VK_TAB` — Tab.
pub const TAB: i32 = 0x09;
/// `VK_RETURN` — Enter / Return.
pub const RETURN: i32 = 0x0D;
/// `VK_SHIFT`.
pub const SHIFT: i32 = 0x10;
/// `VK_CONTROL`.
pub const CONTROL: i32 = 0x11;
/// `VK_MENU` — Alt.
pub const MENU: i32 = 0x12;
/// `VK_PAUSE`.
pub const PAUSE: i32 = 0x13;
/// `VK_CAPITAL` — Caps Lock.
pub const CAPITAL: i32 = 0x14;
/// `VK_ESCAPE`.
pub const ESCAPE: i32 = 0x1B;
/// `VK_SPACE`.
pub const SPACE: i32 = 0x20;
/// `VK_PRIOR` — Page Up.
pub const PRIOR: i32 = 0x21;
/// `VK_NEXT` — Page Down.
pub const NEXT: i32 = 0x22;
/// `VK_END`.
pub const END: i32 = 0x23;
/// `VK_HOME`.
pub const HOME: i32 = 0x24;
/// `VK_LEFT` — Arrow Left.
pub const LEFT: i32 = 0x25;
/// `VK_UP` — Arrow Up.
pub const UP: i32 = 0x26;
/// `VK_RIGHT` — Arrow Right.
pub const RIGHT: i32 = 0x27;
/// `VK_DOWN` — Arrow Down.
pub const DOWN: i32 = 0x28;
/// `VK_SNAPSHOT` — Print Screen.
pub const SNAPSHOT: i32 = 0x2C;
/// `VK_INSERT`.
pub const INSERT: i32 = 0x2D;
/// `VK_DELETE`.
pub const DELETE: i32 = 0x2E;
/// `VK_0` … `VK_9` are `0x30 + digit`.
pub const DIGIT_0: i32 = 0x30;
/// `VK_A` … `VK_Z` are `0x41 + letter_index`.
pub const LETTER_A: i32 = 0x41;
/// `VK_LWIN` — left Super / Windows key.
pub const LWIN: i32 = 0x5B;
/// `VK_RWIN` — right Super / Windows key.
pub const RWIN: i32 = 0x5C;
/// `VK_NUMPAD0` … `VK_NUMPAD9` are `0x60 + digit`.
pub const NUMPAD_0: i32 = 0x60;
/// `VK_MULTIPLY` — numpad `*`.
pub const MULTIPLY: i32 = 0x6A;
/// `VK_ADD` — numpad `+`.
pub const ADD: i32 = 0x6B;
/// `VK_SUBTRACT` — numpad `-`.
pub const SUBTRACT: i32 = 0x6D;
/// `VK_DECIMAL` — numpad `.`.
pub const DECIMAL: i32 = 0x6E;
/// `VK_DIVIDE` — numpad `/`.
pub const DIVIDE: i32 = 0x6F;
/// `VK_F1` … `VK_F24` are `0x70 + f_index`.
pub const F1: i32 = 0x70;
/// `VK_NUMLOCK`.
pub const NUMLOCK: i32 = 0x90;
/// `VK_SCROLL` — Scroll Lock.
pub const SCROLL: i32 = 0x91;
/// `VK_OEM_1` — `;:` on US layouts.
pub const OEM_1: i32 = 0xBA;
/// `VK_OEM_PLUS` — `=+`.
pub const OEM_PLUS: i32 = 0xBB;
/// `VK_OEM_COMMA` — `,<`.
pub const OEM_COMMA: i32 = 0xBC;
/// `VK_OEM_MINUS` — `-_`.
pub const OEM_MINUS: i32 = 0xBD;
/// `VK_OEM_PERIOD` — `.>`.
pub const OEM_PERIOD: i32 = 0xBE;
/// `VK_OEM_2` — `/?`.
pub const OEM_2: i32 = 0xBF;
/// `VK_OEM_3` — `` `~ ``.
pub const OEM_3: i32 = 0xC0;
/// `VK_OEM_4` — `[{`.
pub const OEM_4: i32 = 0xDB;
/// `VK_OEM_5` — `\|`.
pub const OEM_5: i32 = 0xDC;
/// `VK_OEM_6` — `]}`.
pub const OEM_6: i32 = 0xDD;
/// `VK_OEM_7` — `'"`.
pub const OEM_7: i32 = 0xDE;

/// The virtual-key code for a decimal digit (`0`–`9`); `None` for other
/// values.
#[must_use]
pub fn digit(value: u8) -> Option<i32> {
    (value <= 9).then(|| DIGIT_0.saturating_add(i32::from(value)))
}

/// The virtual-key code for an ASCII letter (case-insensitive); `None` for
/// non-letters.
#[must_use]
pub fn letter(value: char) -> Option<i32> {
    value.is_ascii_alphabetic().then(|| {
        let index = u32::from(value.to_ascii_uppercase()).saturating_sub(u32::from('A'));
        LETTER_A.saturating_add(i32::try_from(index).unwrap_or(0))
    })
}

/// The virtual-key code for a function key `F1`–`F24`; `None` outside that
/// range.
#[must_use]
pub fn function_key(number: u8) -> Option<i32> {
    ((1..=24).contains(&number)).then(|| F1.saturating_add(i32::from(number.saturating_sub(1))))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    #[test]
    fn digits_map_to_vk_range() {
        assert_eq!(super::digit(0), Some(0x30));
        assert_eq!(super::digit(9), Some(0x39));
        assert_eq!(super::digit(10), None);
    }

    #[test]
    fn letters_map_case_insensitively() {
        assert_eq!(super::letter('a'), Some(0x41));
        assert_eq!(super::letter('Z'), Some(0x5A));
        assert_eq!(super::letter('ß'), None);
        assert_eq!(super::letter('1'), None);
    }

    #[test]
    fn function_keys_map_to_vk_range() {
        assert_eq!(super::function_key(1), Some(0x70));
        assert_eq!(super::function_key(12), Some(0x7B));
        assert_eq!(super::function_key(24), Some(0x87));
        assert_eq!(super::function_key(0), None);
        assert_eq!(super::function_key(25), None);
    }
}
