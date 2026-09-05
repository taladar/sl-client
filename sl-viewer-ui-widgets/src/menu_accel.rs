//! Menu accelerators (`viewer-menu-accelerators-inert`): the chord drawn against
//! a menu entry is the chord that runs it.
//!
//! [`crate::menu`] has always drawn a [`MenuCommand`]'s `accel` label against its
//! line, and nothing dispatched it — every chord that worked had a bespoke
//! keyboard system of its own somewhere in the viewer, and every chord that did
//! not was a label promising something the keyboard never delivered (`Ctrl+P`,
//! `Ctrl+T`, `Ctrl+F`). This module closes that class: one system walks the menu
//! trees that are actually mounted, matches the pressed chord against the
//! accelerator drawn on each entry, and writes the entry's own [`UiAction`] —
//! the same message the click path writes, routed by the same handler.
//!
//! So a drawn accelerator cannot disagree with the keyboard again: an entry
//! *is* its shortcut, and a future `.accel("Ctrl+…")` is live the moment it is
//! authored.
//!
//! # What it honours
//!
//! - **`enabled_when` / `visible_when`.** A greyed entry does not run, and an
//!   entry that is not drawn at all has no shortcut. That is what keeps
//!   `Ctrl+U` (Upload ▸ Image…, still `UNIMPLEMENTED`) inert while its label is
//!   greyed, and makes it live the day the uploader lands.
//! - **The exact modifier set**, the reference's
//!   `mask == (mAcceleratorMask & MASK_NORMALKEYS)`: `Ctrl+Shift+L` does not
//!   fire `Ctrl+L`'s entry, and `Ctrl+L` does not fire `Ctrl+Shift+L`'s.
//! - **Where the keyboard is.** A focused **text field** takes every chord: the
//!   viewer's text editor claims `Ctrl` chords of its own (select-all, copy,
//!   paste, undo) and nothing here can know whether it wanted this one, so the
//!   menu yields to it wholesale rather than racing it. This is a deliberate
//!   divergence from the reference, which offers modified chords to the menu
//!   bar *before* the focused control unless the focus declares accelerators
//!   (`LLViewerWindow::handleKey`); it costs a `Ctrl+P` typed into the chat bar
//!   and buys the guarantee that nothing here can eat a keystroke meant for
//!   text. A focused **widget** (a button, a list) keeps modified chords —
//!   there is nothing there for them to collide with — and an **unmodified**
//!   accelerator needs the world: it is one keystroke from being typed.
//! - **An open menu.** While a drop-down is up, an unmodified accelerator is a
//!   jump key, not a shortcut (`LLMenuBarGL::handleAcceleratorKey`).
//!
//! Reference (Firestorm, read-only): `indra/llui/llmenugl.cpp`
//! (`LLMenuItemCallGL::handleAcceleratorKey`, `LLMenuBarGL::handleAcceleratorKey`),
//! `indra/newview/llviewerwindow.cpp` (`LLViewerWindow::handleKey`'s
//! focus-versus-menu order).

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use sl_viewer_ui_core::ui_element::UiAction;

use crate::menu::{MenuCommand, MenuConditions, MenuDef, MenuHost, MenuItemDef, conditions_at};

// ---------------------------------------------------------------------------
// The chord.
// ---------------------------------------------------------------------------

/// A parsed accelerator: the modifiers it needs and the key that triggers it.
///
/// Parsed from the very string the entry draws ([`MenuCommand::accelerator`]),
/// so there is one spelling of a shortcut in the codebase and the label is it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accelerator {
    /// Whether `Ctrl` must be held (and, being an exact match, held *only* if
    /// this is set).
    pub ctrl: bool,
    /// Whether `Alt` must be held.
    pub alt: bool,
    /// Whether `Shift` must be held.
    pub shift: bool,
    /// The key whose press fires it.
    pub key: KeyCode,
}

impl Accelerator {
    /// Parse an accelerator label — `"Ctrl+Shift+L"`, `"Ctrl+Alt+Shift+S"`,
    /// `"Home"` — or `None` if it names a modifier or key this does not know.
    ///
    /// Modifier names are matched case-insensitively (`Ctrl` / `Control`, `Alt`,
    /// `Shift`); the last `+`-separated token is the key. A `None` here is a bug
    /// in the label, not a runtime condition — the accelerator would draw and do
    /// nothing, which is the very thing this module exists to prevent — so every
    /// bar pins its own labels through [`accelerators`] in a test.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut key = None;
        let mut tokens = text.split('+').map(str::trim).peekable();
        while let Some(token) = tokens.next() {
            // The last token is the key; everything before it is a modifier.
            if tokens.peek().is_none() {
                key = key_code(token);
                break;
            }
            if token.eq_ignore_ascii_case("ctrl") || token.eq_ignore_ascii_case("control") {
                ctrl = true;
            } else if token.eq_ignore_ascii_case("alt") {
                alt = true;
            } else if token.eq_ignore_ascii_case("shift") {
                shift = true;
            } else {
                return None;
            }
        }
        key.map(|key| Self {
            ctrl,
            alt,
            shift,
            key,
        })
    }

    /// Whether this chord carries a `Ctrl` or `Alt` — the reference's test for
    /// "this keystroke could not have been meant as text".
    #[must_use]
    pub const fn is_modified(&self) -> bool {
        self.ctrl || self.alt
    }

    /// Whether this chord was **just** pressed, with exactly its modifiers held.
    ///
    /// Exact, not "at least": the reference compares the whole normal-key mask,
    /// which is what keeps `Ctrl+Shift+L` off `Ctrl+L`'s entry. Fired on the
    /// key's own press only, so a held chord runs once (the reference's
    /// `allow_key_repeat` is deliberately not reproduced — an OS repeat rate
    /// applied to Undo is not a feature).
    fn just_pressed(&self, keys: &ButtonInput<KeyCode>) -> bool {
        keys.just_pressed(self.key)
            && held(keys, KeyCode::ControlLeft, KeyCode::ControlRight) == self.ctrl
            && held(keys, KeyCode::AltLeft, KeyCode::AltRight) == self.alt
            && held(keys, KeyCode::ShiftLeft, KeyCode::ShiftRight) == self.shift
    }
}

/// Whether either side of a modifier is down.
fn held(keys: &ButtonInput<KeyCode>, left: KeyCode, right: KeyCode) -> bool {
    keys.pressed(left) || keys.pressed(right)
}

/// The [`KeyCode`] an accelerator's key token names, or `None` if unknown.
///
/// A one-character token is a letter or a digit; anything longer is one of the
/// named keys below. Only keys a menu label plausibly draws are here — a token
/// this does not know is a label bug, reported by the pinning test rather than
/// guessed at.
fn key_code(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(only), None) => character_key(only),
        _other => named_key(name),
    }
}

/// The [`KeyCode`] for a single-character key token (a letter or a digit).
const fn character_key(character: char) -> Option<KeyCode> {
    Some(match character.to_ascii_uppercase() {
        'A' => KeyCode::KeyA,
        'B' => KeyCode::KeyB,
        'C' => KeyCode::KeyC,
        'D' => KeyCode::KeyD,
        'E' => KeyCode::KeyE,
        'F' => KeyCode::KeyF,
        'G' => KeyCode::KeyG,
        'H' => KeyCode::KeyH,
        'I' => KeyCode::KeyI,
        'J' => KeyCode::KeyJ,
        'K' => KeyCode::KeyK,
        'L' => KeyCode::KeyL,
        'M' => KeyCode::KeyM,
        'N' => KeyCode::KeyN,
        'O' => KeyCode::KeyO,
        'P' => KeyCode::KeyP,
        'Q' => KeyCode::KeyQ,
        'R' => KeyCode::KeyR,
        'S' => KeyCode::KeyS,
        'T' => KeyCode::KeyT,
        'U' => KeyCode::KeyU,
        'V' => KeyCode::KeyV,
        'W' => KeyCode::KeyW,
        'X' => KeyCode::KeyX,
        'Y' => KeyCode::KeyY,
        'Z' => KeyCode::KeyZ,
        '0' => KeyCode::Digit0,
        '1' => KeyCode::Digit1,
        '2' => KeyCode::Digit2,
        '3' => KeyCode::Digit3,
        '4' => KeyCode::Digit4,
        '5' => KeyCode::Digit5,
        '6' => KeyCode::Digit6,
        '7' => KeyCode::Digit7,
        '8' => KeyCode::Digit8,
        '9' => KeyCode::Digit9,
        _other => return None,
    })
}

/// The [`KeyCode`] for a multi-character key token — a function key or one of
/// the named editing / navigation keys, spelled as a menu label spells it.
fn named_key(name: &str) -> Option<KeyCode> {
    Some(match name.to_ascii_uppercase().as_str() {
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "F11" => KeyCode::F11,
        "F12" => KeyCode::F12,
        "HOME" => KeyCode::Home,
        "END" => KeyCode::End,
        "PAGEUP" | "PGUP" => KeyCode::PageUp,
        "PAGEDOWN" | "PGDN" => KeyCode::PageDown,
        "INSERT" | "INS" => KeyCode::Insert,
        "DELETE" | "DEL" => KeyCode::Delete,
        "BACKSPACE" => KeyCode::Backspace,
        "ENTER" | "RETURN" => KeyCode::Enter,
        "ESC" | "ESCAPE" => KeyCode::Escape,
        "SPACE" => KeyCode::Space,
        "TAB" => KeyCode::Tab,
        "UP" => KeyCode::ArrowUp,
        "DOWN" => KeyCode::ArrowDown,
        "LEFT" => KeyCode::ArrowLeft,
        "RIGHT" => KeyCode::ArrowRight,
        _other => return None,
    })
}

// ---------------------------------------------------------------------------
// The tree walk.
// ---------------------------------------------------------------------------

/// Every accelerator a menu tree draws, depth-first, as the label and the action
/// it is drawn against.
///
/// The pinning surface for a bar: a test walks its own tree with this and
/// asserts that every label [`Accelerator::parse`]s and that no two entries
/// claim one chord — the two ways a drawn accelerator can silently do nothing
/// (or something surprising) once the dispatcher below is live.
///
/// Walks static submenus, conditional ones included (a `visible_when` that does
/// not hold today may hold tomorrow, and the chord it draws is still authored);
/// a dynamic submenu's lines are data with no accelerator to draw.
#[must_use]
pub fn accelerators(menu: &'static MenuDef) -> Vec<(&'static str, &'static str)> {
    let mut found = Vec::new();
    for item in menu.items {
        match item {
            MenuItemDef::Command(command) => {
                if let Some(accelerator) = command.accelerator {
                    found.push((accelerator, command.action));
                }
            }
            MenuItemDef::Submenu(sub) | MenuItemDef::SubmenuWhen(sub, _) => {
                found.extend(accelerators(sub));
            }
            MenuItemDef::DynamicSubmenu { .. } | MenuItemDef::Separator => {}
        }
    }
    found
}

/// Every command in `menu` that is **drawn and enabled** under `held`,
/// depth-first — the entries a keystroke is allowed to reach.
///
/// A submenu is descended only while it is drawn: a `SubmenuWhen` whose
/// condition fails is not on screen, so neither are its entries' shortcuts.
fn live_commands(
    menu: &'static MenuDef,
    held: &MenuConditions,
    found: &mut Vec<&'static MenuCommand>,
) {
    for item in menu.items {
        match item {
            MenuItemDef::Command(command) => {
                if held.holds(command.visible_when) && held.holds(command.enabled_when) {
                    found.push(command);
                }
            }
            MenuItemDef::Submenu(sub) => live_commands(sub, held, found),
            MenuItemDef::SubmenuWhen(sub, when) => {
                if held.holds(Some(when)) {
                    live_commands(sub, held, found);
                }
            }
            MenuItemDef::DynamicSubmenu { .. } | MenuItemDef::Separator => {}
        }
    }
}

// ---------------------------------------------------------------------------
// The dispatcher.
// ---------------------------------------------------------------------------

/// Where the keyboard is this frame, as far as an accelerator is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcceleratorGate {
    /// A text field holds focus: no accelerator fires.
    TextEntry,
    /// A widget holds focus: modified chords fire, unmodified ones do not.
    Widget,
    /// Nothing in the UI holds focus: every accelerator fires.
    Free,
}

impl AcceleratorGate {
    /// Whether `chord` may fire under this gate.
    const fn admits(self, chord: &Accelerator) -> bool {
        match self {
            Self::TextEntry => false,
            Self::Widget => chord.is_modified(),
            Self::Free => true,
        }
    }
}

/// Route a pressed chord to the menu command its accelerator is drawn on.
///
/// Walks every mounted [`MenuHost`] — the top bar's menus, a gear button's
/// drop-down, the inventory `+` menu — so a bar gets its shortcuts by being
/// spawned, not by anyone remembering to bind them. The conditions are the
/// host's own ([`conditions_at`], the same ancestor walk that greys a line), so
/// an entry the user would find greyed is one the keyboard finds greyed too.
pub(crate) fn dispatch_menu_accelerators(
    keys: Res<ButtonInput<KeyCode>>,
    hosts: Query<(Entity, &MenuHost)>,
    conditions: Query<&MenuConditions>,
    child_of: Query<&ChildOf>,
    focus: Res<InputFocus>,
    ui_nodes: Query<Has<EditableText>, With<Node>>,
    mut actions: MessageWriter<UiAction>,
) {
    if keys.get_just_pressed().next().is_none() {
        return;
    }
    let gate = match focus.get().map(|entity| ui_nodes.get(entity)) {
        Some(Ok(true)) => AcceleratorGate::TextEntry,
        Some(Ok(false)) => AcceleratorGate::Widget,
        Some(Err(_)) | None => AcceleratorGate::Free,
    };
    if gate == AcceleratorGate::TextEntry {
        return;
    }
    // While a drop-down is up, an unmodified key is that menu's jump key.
    let menu_open = hosts.iter().any(|(_entity, host)| host.open.is_some());
    let empty = MenuConditions::default();
    let mut fired: Vec<(&'static str, &'static str)> = Vec::new();
    for (entity, host) in &hosts {
        let held = conditions_at(entity, &child_of, &conditions).unwrap_or(&empty);
        let mut commands = Vec::new();
        live_commands(host.def, held, &mut commands);
        for command in commands {
            let Some(chord) = command.accelerator.and_then(Accelerator::parse) else {
                continue;
            };
            if !gate.admits(&chord) || (menu_open && !chord.is_modified()) {
                continue;
            }
            if !chord.just_pressed(&keys) {
                continue;
            }
            let pick = (host.element, command.action);
            if fired.contains(&pick) {
                continue;
            }
            fired.push(pick);
        }
    }
    if fired.len() > 1 {
        warn!(
            "the pressed accelerator is drawn against {} different commands: {fired:?}",
            fired.len()
        );
    }
    for (element, action) in fired {
        debug!("menu accelerator fires {element}/{action}");
        actions.write(UiAction { element, action });
    }
}

#[cfg(test)]
mod tests {
    use super::{Accelerator, accelerators};
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::picking::hover::HoverMap;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use crate::menu::{
        FIXTURE_MENU_BAR, MenuBarDef, MenuCommand, MenuConditions, MenuDef, MenuItemDef,
        MenuWidgetPlugin, spawn_menu_bar,
    };
    use crate::ui_test::{LayoutTest, TestError, drain_actions, enable_action_recording, settle};
    use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};
    use sl_viewer_ui_core::ui_element::{ElementCx, UiAction};

    /// The `element` the test bar attributes its picks to.
    const TEST_ELEMENT: &str = "menu-accel-test";

    /// A submenu, so the walk is exercised past the top level.
    static TEST_SUBMENU: MenuDef = MenuDef {
        label: "Deeper",
        items: &[MenuItemDef::Command(
            MenuCommand::new("Deep", "deep").accel("Ctrl+Alt+Shift+S"),
        )],
    };

    /// One menu holding every accelerator shape the dispatcher must tell apart.
    static TEST_MENU: MenuDef = MenuDef {
        label: "Test",
        items: &[
            MenuItemDef::Command(MenuCommand::new("Open", "open").accel("Ctrl+I")),
            MenuItemDef::Command(MenuCommand::new("Mini", "mini").accel("Ctrl+Shift+M")),
            MenuItemDef::Command(MenuCommand::new("Bare", "bare").accel("Home")),
            MenuItemDef::Command(
                MenuCommand::new("Gated", "gated")
                    .accel("Ctrl+G")
                    .enabled_when("can-gate"),
            ),
            MenuItemDef::Command(
                MenuCommand::new("Hidden", "hidden")
                    .accel("Ctrl+H")
                    .visible_when("advanced"),
            ),
            MenuItemDef::Submenu(&TEST_SUBMENU),
        ],
    };

    /// The test bar.
    static TEST_BAR: MenuBarDef = MenuBarDef {
        menus: &[&TEST_MENU],
    };

    /// A headless app with [`TEST_BAR`] mounted under a full menu-widget runtime
    /// and `held` as the bar's conditions.
    fn bar_app(held: &[&'static str]) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        enable_action_recording(&mut app);
        app.init_resource::<HoverMap>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(MenuWidgetPlugin);
        let conditions = MenuConditions(held.to_vec());
        app.add_systems(
            Startup,
            (move |mut commands: Commands, root: Res<UiRoot>| {
                let bar = spawn_menu_bar(
                    &mut commands,
                    root.0,
                    ElementCx::new(),
                    &TEST_BAR,
                    TEST_ELEMENT,
                );
                commands.entity(bar).insert(conditions.clone());
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        drain_actions(&mut app);
        Ok(app)
    }

    /// Press a chord: hold its modifiers, press its key, run a frame, release.
    fn press(app: &mut App, chord: &str) -> Result<(), TestError> {
        let accelerator = Accelerator::parse(chord)
            .ok_or_else(|| format!("the test chord {chord} does not parse"))?;
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        // `reset_all`, not `clear`: `clear` only drops the just-pressed edge, so
        // a modifier from the previous chord would still be *held* and the next
        // press would silently be a different chord.
        keys.reset_all();
        if accelerator.ctrl {
            keys.press(KeyCode::ControlLeft);
        }
        if accelerator.alt {
            keys.press(KeyCode::AltLeft);
        }
        if accelerator.shift {
            keys.press(KeyCode::ShiftLeft);
        }
        keys.press(accelerator.key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        Ok(())
    }

    /// The action a chord ran, if any — the whole outward effect of a shortcut.
    fn ran(app: &mut App, chord: &str) -> Result<Vec<UiAction>, TestError> {
        press(app, chord)?;
        Ok(drain_actions(app))
    }

    /// Every spelling a menu label uses parses to the chord it reads as.
    #[test]
    fn an_accelerator_label_parses_to_its_chord() {
        assert_eq!(
            Accelerator::parse("Ctrl+P"),
            Some(Accelerator {
                ctrl: true,
                alt: false,
                shift: false,
                key: KeyCode::KeyP,
            }),
            "a single modifier"
        );
        assert_eq!(
            Accelerator::parse("Ctrl+Alt+Shift+S"),
            Some(Accelerator {
                ctrl: true,
                alt: true,
                shift: true,
                key: KeyCode::KeyS,
            }),
            "three modifiers, in the order a label writes them"
        );
        assert_eq!(
            Accelerator::parse("Home"),
            Some(Accelerator {
                ctrl: false,
                alt: false,
                shift: false,
                key: KeyCode::Home,
            }),
            "a bare named key"
        );
        assert_eq!(
            Accelerator::parse("Ctrl+F10").map(|chord| chord.key),
            Some(KeyCode::F10),
            "a function key"
        );
        assert_eq!(
            Accelerator::parse("Ctrl+Meta+X"),
            None,
            "an unknown modifier is a label bug, not a chord"
        );
        assert_eq!(
            Accelerator::parse("Ctrl+Wibble"),
            None,
            "an unknown key is a label bug, not a chord"
        );
    }

    /// The gallery fixture's accelerators all parse, and no two claim one chord
    /// — the shape every live bar pins for itself.
    #[test]
    fn a_bar_draws_only_chords_that_parse() {
        let drawn: Vec<(&'static str, &'static str)> = FIXTURE_MENU_BAR
            .menus
            .iter()
            .flat_map(|menu| accelerators(menu))
            .collect();
        let unparsed: Vec<(&str, &str)> = drawn
            .iter()
            .filter(|(label, _action)| Accelerator::parse(label).is_none())
            .copied()
            .collect();
        assert!(
            unparsed.is_empty(),
            "these drawn accelerators do not parse, so they would do nothing: {unparsed:?}"
        );
        let mut seen: Vec<(Accelerator, &'static str)> = Vec::new();
        for (label, action) in drawn {
            let Some(chord) = Accelerator::parse(label) else {
                continue;
            };
            assert!(
                !seen.iter().any(|(other, _action)| *other == chord),
                "two entries claim {label}: {seen:?} and {action}"
            );
            seen.push((chord, action));
        }
        assert!(!seen.is_empty(), "the fixture draws accelerators at all");
    }

    /// The walk reaches a submenu's entries, and reports every drawn label.
    #[test]
    fn the_walk_reports_every_drawn_accelerator() {
        assert_eq!(
            accelerators(&TEST_MENU),
            vec![
                ("Ctrl+I", "open"),
                ("Ctrl+Shift+M", "mini"),
                ("Home", "bare"),
                ("Ctrl+G", "gated"),
                ("Ctrl+H", "hidden"),
                ("Ctrl+Alt+Shift+S", "deep"),
            ],
        );
    }

    /// The chord drawn against an entry runs that entry — the whole point.
    #[test]
    fn a_drawn_chord_runs_its_entry() -> Result<(), TestError> {
        let mut app = bar_app(&[])?;
        assert_eq!(
            ran(&mut app, "Ctrl+I")?,
            vec![UiAction {
                element: TEST_ELEMENT,
                action: "open",
            }],
            "the chord drawn against Open runs Open"
        );
        assert_eq!(
            ran(&mut app, "Ctrl+Alt+Shift+S")?,
            vec![UiAction {
                element: TEST_ELEMENT,
                action: "deep",
            }],
            "including an entry inside a submenu, which is never on screen"
        );
        Ok(())
    }

    /// A different modifier set is a different chord — the reference's exact
    /// mask comparison, not "at least these modifiers".
    #[test]
    fn a_chord_needs_exactly_its_modifiers() -> Result<(), TestError> {
        let mut app = bar_app(&[])?;
        assert!(
            ran(&mut app, "Ctrl+Shift+I")?.is_empty(),
            "Ctrl+Shift+I is not Ctrl+I"
        );
        assert!(
            ran(&mut app, "Ctrl+M")?.is_empty(),
            "and Ctrl+M is not Ctrl+Shift+M"
        );
        assert_eq!(
            ran(&mut app, "Ctrl+Shift+M")?,
            vec![UiAction {
                element: TEST_ELEMENT,
                action: "mini",
            }],
            "the chord as drawn runs it"
        );
        Ok(())
    }

    /// A greyed entry is greyed to the keyboard too, and goes live with it.
    #[test]
    fn a_disabled_entry_ignores_its_chord() -> Result<(), TestError> {
        let mut app = bar_app(&[])?;
        assert!(
            ran(&mut app, "Ctrl+G")?.is_empty(),
            "the entry is greyed, so its accelerator is dead"
        );
        let mut app = bar_app(&["can-gate"])?;
        assert_eq!(
            ran(&mut app, "Ctrl+G")?,
            vec![UiAction {
                element: TEST_ELEMENT,
                action: "gated",
            }],
            "and runs once the condition that enables the line holds"
        );
        Ok(())
    }

    /// An entry that is not drawn at all has no shortcut either.
    #[test]
    fn a_hidden_entry_has_no_chord() -> Result<(), TestError> {
        let mut app = bar_app(&[])?;
        assert!(
            ran(&mut app, "Ctrl+H")?.is_empty(),
            "a `visible_when` that does not hold takes the accelerator with the line"
        );
        let mut app = bar_app(&["advanced"])?;
        assert_eq!(
            ran(&mut app, "Ctrl+H")?,
            vec![UiAction {
                element: TEST_ELEMENT,
                action: "hidden",
            }],
        );
        Ok(())
    }

    /// A focused text field takes every chord, so typing can never run a menu.
    #[test]
    fn a_focused_text_field_takes_every_chord() -> Result<(), TestError> {
        let mut app = bar_app(&[])?;
        let field = app
            .world_mut()
            .spawn((Node::default(), bevy::text::EditableText::new("")))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        settle(&mut app);
        drain_actions(&mut app);
        assert!(
            ran(&mut app, "Ctrl+I")?.is_empty(),
            "Ctrl+I typed into a text field stays in the text field"
        );
        Ok(())
    }

    /// A focused widget keeps the modified chords and stands the bare ones down:
    /// an unmodified accelerator is one keystroke from being typed.
    #[test]
    fn a_focused_widget_keeps_only_the_modified_chords() -> Result<(), TestError> {
        let mut app = bar_app(&[])?;
        let widget = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(widget, FocusCause::Navigated);
        settle(&mut app);
        drain_actions(&mut app);
        assert!(
            ran(&mut app, "Home")?.is_empty(),
            "a bare Home belongs to whatever holds focus"
        );
        assert_eq!(
            ran(&mut app, "Ctrl+I")?,
            vec![UiAction {
                element: TEST_ELEMENT,
                action: "open",
            }],
            "a Ctrl chord could not have been meant for it"
        );
        Ok(())
    }

    /// With nothing focused, a bare accelerator runs.
    #[test]
    fn an_unmodified_chord_runs_from_the_world() -> Result<(), TestError> {
        let mut app = bar_app(&[])?;
        assert_eq!(
            ran(&mut app, "Home")?,
            vec![UiAction {
                element: TEST_ELEMENT,
                action: "bare",
            }],
        );
        Ok(())
    }
}
