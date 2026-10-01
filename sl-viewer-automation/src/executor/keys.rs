//! A key press request's keys: `Enter`, `a`, `Ctrl+Shift+S` — parsed into the
//! synthetic input's steps.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use sl_viewer_ui_core::synthetic_input::{InputAction, InputStep, key_code_for};

/// The named keys a request may press, by the name it spells them with.
const NAMED: [(&str, KeyCode, Key); 15] = [
    ("enter", KeyCode::Enter, Key::Enter),
    ("escape", KeyCode::Escape, Key::Escape),
    ("tab", KeyCode::Tab, Key::Tab),
    ("space", KeyCode::Space, Key::Space),
    ("backspace", KeyCode::Backspace, Key::Backspace),
    ("delete", KeyCode::Delete, Key::Delete),
    ("insert", KeyCode::Insert, Key::Insert),
    ("home", KeyCode::Home, Key::Home),
    ("end", KeyCode::End, Key::End),
    ("pageup", KeyCode::PageUp, Key::PageUp),
    ("pagedown", KeyCode::PageDown, Key::PageDown),
    ("arrowup", KeyCode::ArrowUp, Key::ArrowUp),
    ("arrowdown", KeyCode::ArrowDown, Key::ArrowDown),
    ("arrowleft", KeyCode::ArrowLeft, Key::ArrowLeft),
    ("arrowright", KeyCode::ArrowRight, Key::ArrowRight),
];

/// The function keys, `F1` … `F12`.
const FUNCTION: [(KeyCode, Key); 12] = [
    (KeyCode::F1, Key::F1),
    (KeyCode::F2, Key::F2),
    (KeyCode::F3, Key::F3),
    (KeyCode::F4, Key::F4),
    (KeyCode::F5, Key::F5),
    (KeyCode::F6, Key::F6),
    (KeyCode::F7, Key::F7),
    (KeyCode::F8, Key::F8),
    (KeyCode::F9, Key::F9),
    (KeyCode::F10, Key::F10),
    (KeyCode::F11, Key::F11),
    (KeyCode::F12, Key::F12),
];

/// One key: the physical key, the logical key, and the text it types when no
/// command modifier is held.
type Pressed = (KeyCode, Key, Option<String>);

/// The keys `spec` names, as an action: each modifier down in order, the key
/// down, held for `hold_frames` frames (one at least), and up, the modifiers up
/// in reverse.
///
/// A character or `Space` pressed with no modifier but `Shift` carries its
/// text, so a focused text field inserts it; under `Ctrl`, `Alt` or `Super` it
/// is a command and carries none.
///
/// # Errors
///
/// A sentence naming what is wrong: an empty spec, an unknown modifier, or a
/// key that is neither named nor one character.
pub(crate) fn parse_keys(spec: &str, hold_frames: u32) -> Result<InputAction, String> {
    // A trailing `++` is the plus key under modifiers; `+` alone is the key.
    let (modifier_part, key_part) = if spec == "+" {
        ("", "+")
    } else if let Some(modifiers) = spec.strip_suffix("++") {
        (modifiers, "+")
    } else {
        match spec.rsplit_once('+') {
            Some((modifiers, key)) => (modifiers, key),
            None => ("", spec),
        }
    };
    let modifiers = if modifier_part.is_empty() {
        Vec::new()
    } else {
        modifier_part
            .split('+')
            .map(modifier)
            .collect::<Result<Vec<_>, _>>()?
    };
    let command = modifiers
        .iter()
        .any(|(code, _key)| !matches!(code, KeyCode::ShiftLeft));
    let (key_code, logical, text) = key(key_part, command)?;
    let mut steps: Vec<InputStep> = modifiers
        .iter()
        .map(|(key_code, logical)| InputStep::KeyDown {
            key_code: *key_code,
            logical: logical.clone(),
            text: None,
        })
        .collect();
    steps.push(InputStep::KeyDown {
        key_code,
        logical: logical.clone(),
        text,
    });
    let held = usize::try_from(hold_frames.saturating_sub(1)).unwrap_or(usize::MAX);
    steps.extend(core::iter::repeat_n(InputStep::Idle, held));
    steps.push(InputStep::KeyUp { key_code, logical });
    steps.extend(
        modifiers
            .into_iter()
            .rev()
            .map(|(key_code, logical)| InputStep::KeyUp { key_code, logical }),
    );
    Ok(InputAction::from_steps(steps))
}

/// The modifier `name` spells.
fn modifier(name: &str) -> Result<(KeyCode, Key), String> {
    match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Ok((KeyCode::ControlLeft, Key::Control)),
        "shift" => Ok((KeyCode::ShiftLeft, Key::Shift)),
        "alt" => Ok((KeyCode::AltLeft, Key::Alt)),
        "super" | "meta" => Ok((KeyCode::SuperLeft, Key::Super)),
        "" => Err("an empty modifier".to_owned()),
        other => Err(format!(
            "{other:?} is not a modifier (Ctrl, Shift, Alt, Super)"
        )),
    }
}

/// The key `name` spells; its text unless a `command` modifier is held.
fn key(name: &str, command: bool) -> Result<Pressed, String> {
    let lower = name.to_ascii_lowercase();
    if let Some((_name, key_code, logical)) = NAMED.iter().find(|(named, _, _)| *named == lower) {
        let text = (*key_code == KeyCode::Space && !command).then(|| " ".to_owned());
        return Ok((*key_code, logical.clone(), text));
    }
    if let Some(number) = lower.strip_prefix('f')
        && let Ok(index) = number.parse::<usize>()
        && let Some((key_code, logical)) = index.checked_sub(1).and_then(|at| FUNCTION.get(at))
    {
        return Ok((*key_code, logical.clone(), None));
    }
    let mut characters = name.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => {
            let typed = character.to_string();
            Ok((
                key_code_for(character).unwrap_or(KeyCode::F35),
                Key::Character(typed.as_str().into()),
                (!command).then_some(typed),
            ))
        }
        (None, _) => Err("no key after the modifiers".to_owned()),
        (Some(_), Some(_)) => Err(format!(
            "{name:?} is not a key: a named key (Enter, Escape, Tab, Space, Backspace, Delete, \
             Insert, Home, End, PageUp, PageDown, the arrows, F1 … F12) or one character"
        )),
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::Key;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_viewer_ui_core::synthetic_input::InputStep;

    use super::parse_keys;

    /// The steps `spec` parses to.
    fn steps(spec: &str) -> Result<Vec<InputStep>, String> {
        Ok(parse_keys(spec, 0)?.steps().to_vec())
    }

    #[test]
    fn a_held_key_stays_down_for_its_frames_with_the_modifiers_around_it() -> Result<(), String> {
        let held = parse_keys("Shift+w", 4)?.steps().to_vec();
        let shift_down = InputStep::KeyDown {
            key_code: KeyCode::ShiftLeft,
            logical: Key::Shift,
            text: None,
        };
        let w_down = InputStep::KeyDown {
            key_code: KeyCode::KeyW,
            logical: Key::Character("w".into()),
            text: Some("w".to_owned()),
        };
        assert_eq!(
            held,
            vec![
                shift_down,
                w_down,
                InputStep::Idle,
                InputStep::Idle,
                InputStep::Idle,
                InputStep::KeyUp {
                    key_code: KeyCode::KeyW,
                    logical: Key::Character("w".into()),
                },
                InputStep::KeyUp {
                    key_code: KeyCode::ShiftLeft,
                    logical: Key::Shift,
                },
            ]
        );
        // A hold of one frame is a tap.
        assert_eq!(parse_keys("w", 1)?.steps(), parse_keys("w", 0)?.steps());
        Ok(())
    }

    #[test]
    fn a_chord_puts_the_modifiers_down_around_the_key() -> Result<(), String> {
        assert_eq!(
            steps("Ctrl+Shift+s")?,
            vec![
                InputStep::KeyDown {
                    key_code: KeyCode::ControlLeft,
                    logical: Key::Control,
                    text: None,
                },
                InputStep::KeyDown {
                    key_code: KeyCode::ShiftLeft,
                    logical: Key::Shift,
                    text: None,
                },
                InputStep::KeyDown {
                    key_code: KeyCode::KeyS,
                    logical: Key::Character("s".into()),
                    text: None,
                },
                InputStep::KeyUp {
                    key_code: KeyCode::KeyS,
                    logical: Key::Character("s".into()),
                },
                InputStep::KeyUp {
                    key_code: KeyCode::ShiftLeft,
                    logical: Key::Shift,
                },
                InputStep::KeyUp {
                    key_code: KeyCode::ControlLeft,
                    logical: Key::Control,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn a_character_types_its_text_unless_a_command_modifier_is_held() -> Result<(), String> {
        assert!(matches!(
            steps("a")?.first(),
            Some(InputStep::KeyDown { text: Some(text), .. }) if text == "a"
        ));
        assert!(matches!(
            steps("Shift+A")?.get(1),
            Some(InputStep::KeyDown { text: Some(text), .. }) if text == "A"
        ));
        assert!(matches!(
            steps("space")?.first(),
            Some(InputStep::KeyDown { key_code: KeyCode::Space, text: Some(text), .. }) if text == " "
        ));
        assert!(matches!(
            steps("Alt+x")?.get(1),
            Some(InputStep::KeyDown { text: None, .. })
        ));
        assert!(matches!(
            steps("Ctrl++")?.get(1),
            Some(InputStep::KeyDown { logical: Key::Character(plus), text: None, .. })
                if plus.as_str() == "+"
        ));
        Ok(())
    }

    #[test]
    fn named_and_function_keys_are_found_in_any_case() -> Result<(), String> {
        assert!(matches!(
            steps("Enter")?.first(),
            Some(InputStep::KeyDown {
                key_code: KeyCode::Enter,
                logical: Key::Enter,
                text: None
            })
        ));
        assert!(matches!(
            steps("alt+F4")?.get(1),
            Some(InputStep::KeyDown {
                key_code: KeyCode::F4,
                ..
            })
        ));
        assert!(matches!(
            steps("PAGEDOWN")?.first(),
            Some(InputStep::KeyDown {
                key_code: KeyCode::PageDown,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn malformed_keys_are_refused_with_a_reason() {
        for spec in ["", "Ctrl+", "Hyper+a", "Enterr", "F13", "Ctrl++a"] {
            assert!(parse_keys(spec, 0).is_err(), "accepted {spec:?}");
        }
    }
}
