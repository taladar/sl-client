//! The **interaction contract** sweep: every registered element, every named
//! node in it, every gesture in the input alphabet.
//!
//! `ui_test.rs` sweeps the registry for *layout* — an element in eight scripts,
//! at three font sizes, in two directions. This module sweeps it for
//! *behaviour*, and the two compose: after every gesture the whole of
//! `interaction_violations` is re-asserted, so each layout check the harness
//! owns doubles as a post-interaction regression check for free.
//!
//! # The default expectation is inert-and-harmless
//!
//! A node × gesture pair the table says nothing about must not panic, must emit
//! no `UiAction`, and must leave the layout clean. That default is what makes
//! the sweep scale to the whole registry without hand-writing a case per
//! element — a new element inherits the whole sweep by being registered, the
//! same way it inherits the layout matrix.
//!
//! [`CONTRACTS`] tightens it where an element does something: a row says which
//! actions a gesture must emit, in order, and optionally probes the state the
//! gesture left behind. A declared row is *exact*, so a button that grows a
//! second action fails until the table says so.
//!
//! # The address space
//!
//! A contract addresses a node by its `Name`, the convention every element
//! already follows for the widgets worth pointing at. Pointer gestures sweep
//! every named node that could *react* to one — anything observed, a `Button`,
//! a focus stop, an editable field (`interactive_nodes`); keyboard gestures
//! sweep the **focus stops**, the named nodes carrying a live `TabIndex`, which
//! after a settle is exactly the set `Tab` can reach (the scaffold parks a
//! hidden subtree's index, so a hidden stop is correctly absent —
//! `viewer-audit-tab-panel-focus-order`).
//!
//! # Regenerating the table
//!
//! There is no bless path, deliberately — the same choice the pie's
//! compass-address tables make. A gesture that emits an action the table does
//! not declare fails with the row to add, printed in the form it is pasted in.
//! Changing the table is how a behaviour change is admitted, and the diff is
//! then reviewable as what it is.

use bevy::prelude::App;

/// One gesture from the input alphabet the sweep drives.
///
/// Flat rather than parameterised (`Click(MouseButton)`) so that a contract row
/// and a failure message name one thing, and so the alphabets below are plain
/// `const` slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gesture {
    /// A single **primary**-button click on the node's centre.
    ///
    /// Named for `bevy_picking`'s `PointerButton`, not for a side of the mouse,
    /// because that is the vocabulary every widget guard in this workspace is
    /// written in (`if press.button != PointerButton::Primary`) — and because
    /// which physical button is primary is the user's setting, not ours. The
    /// driver presses `MouseButton::Left`, which `bevy_picking`'s input plugin
    /// maps to `PointerButton::Primary`.
    PrimaryClick,
    /// A single middle-button click on the node's centre.
    MiddleClick,
    /// A single **secondary**-button click — the context gesture, which most
    /// nodes must ignore rather than treat as a primary click. Driven as
    /// `MouseButton::Right` → `PointerButton::Secondary`.
    SecondaryClick,
    /// Two primary clicks inside the multi-click interval.
    DoubleClick,
    /// Wheel lines away from the user.
    ScrollUp,
    /// Wheel lines towards the user.
    ScrollDown,
    /// Press on the node, drag off it, release.
    DragAcross,
    /// `Tab` while the node holds focus.
    Tab,
    /// `Shift+Tab` while the node holds focus.
    ShiftTab,
    /// `Enter` while the node holds focus.
    Enter,
    /// `Space` while the node holds focus.
    Space,
    /// `Escape` while the node holds focus.
    Escape,
    /// `ArrowUp` while the node holds focus.
    ArrowUp,
    /// `ArrowDown` while the node holds focus.
    ArrowDown,
    /// `ArrowLeft` while the node holds focus.
    ArrowLeft,
    /// `ArrowRight` while the node holds focus.
    ArrowRight,
}

/// The gestures swept against **every interactive named node**.
pub(crate) const POINTER_ALPHABET: &[Gesture] = &[
    Gesture::PrimaryClick,
    Gesture::MiddleClick,
    Gesture::SecondaryClick,
    Gesture::DoubleClick,
    Gesture::ScrollUp,
    Gesture::ScrollDown,
    Gesture::DragAcross,
];

/// The gestures swept against the **focus stops**, each with the node focused.
pub(crate) const KEYBOARD_ALPHABET: &[Gesture] = &[
    Gesture::Tab,
    Gesture::ShiftTab,
    Gesture::Enter,
    Gesture::Space,
    Gesture::Escape,
    Gesture::ArrowUp,
    Gesture::ArrowDown,
    Gesture::ArrowLeft,
    Gesture::ArrowRight,
];

/// A state check run after a gesture settles, for a reaction that is not an
/// action.
///
/// A tab strip switching pages and a field taking the caret both emit nothing;
/// without this they would be indistinguishable from inert.
#[derive(Clone, Copy)]
pub(crate) struct Probe {
    /// What the probe asserts, phrased for the failure message ("the field
    /// holds focus").
    pub(crate) what: &'static str,
    /// The check itself, run on the settled app.
    pub(crate) check: fn(&mut App) -> bool,
}

/// Hand-written, because a `fn` pointer's derived `Debug` would print an
/// address.
impl core::fmt::Debug for Probe {
    /// Name what the probe checks, which is the only part worth reading.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Probe").field("what", &self.what).finish()
    }
}

/// What a cell claims about the layout once the gesture has settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutClaim {
    /// The layout must still be clean. The default, and what every row that
    /// does not say otherwise gets.
    Clean,
    /// The gesture is **known to break the layout**, against the roadmap id
    /// named here.
    ///
    /// The check is *inverted*, deliberately: the breakage must still be there.
    /// A pin that merely tolerated a bug would go on passing for years after
    /// the fix and quietly stop meaning anything; a pin that asserts the bug is
    /// present fails the day someone fixes it, and tells them to delete the
    /// row. It is the same canary shape as
    /// `a_text_node_may_not_carry_its_own_padding`.
    KnownBroken(&'static str),
}

/// One row of a node's contract: what this gesture must do.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row {
    /// The gesture this row governs.
    pub(crate) gesture: Gesture,
    /// The `UiAction::action` names the gesture must emit, in order. Empty
    /// means "emits nothing", which is only worth writing down alongside a
    /// [`Probe`] — a gesture with no row at all already means that.
    pub(crate) emits: &'static [&'static str],
    /// The state the gesture must leave behind, where there is one.
    pub(crate) probe: Option<Probe>,
    /// What this cell claims about the layout afterwards.
    pub(crate) layout: LayoutClaim,
    /// The roadmap id this row's *emission* is wrong against, where it is —
    /// see [`Row::emits_wrongly`].
    pub(crate) bug: Option<&'static str>,
}

impl Row {
    /// A gesture that emits exactly `actions`, in order, and nothing else.
    pub(crate) const fn emits(gesture: Gesture, actions: &'static [&'static str]) -> Self {
        Self {
            gesture,
            emits: actions,
            probe: None,
            layout: LayoutClaim::Clean,
            bug: None,
        }
    }

    /// A gesture that emits `actions` — and **should not**, against `bug`.
    ///
    /// Identical in force to [`Self::emits`]: the emission is pinned exactly, so
    /// the row fails the moment the bug is fixed and the correction has to be
    /// made here, in the same commit. What naming the bug adds is the census —
    /// grep the id and every control it reaches is listed, which is how "62
    /// controls" in
    /// `roadmap/bugs/viewer-widget-any-mouse-button-activates.md` was counted.
    pub(crate) const fn emits_wrongly(
        gesture: Gesture,
        actions: &'static [&'static str],
        bug: &'static str,
    ) -> Self {
        Self {
            gesture,
            emits: actions,
            probe: None,
            layout: LayoutClaim::Clean,
            bug: Some(bug),
        }
    }

    /// A gesture that emits nothing but leaves a state the probe confirms.
    pub(crate) const fn leaves(gesture: Gesture, probe: Probe) -> Self {
        Self {
            gesture,
            emits: &[],
            probe: Some(probe),
            layout: LayoutClaim::Clean,
            bug: None,
        }
    }

    /// The same row, but its layout is known broken against `bug`.
    pub(crate) const fn known_broken(self, bug: &'static str) -> Self {
        Self {
            layout: LayoutClaim::KnownBroken(bug),
            ..self
        }
    }
}

/// One named node's contract.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NodeContract {
    /// The node's `Name`.
    pub(crate) node: &'static str,
    /// The gestures this node reacts to. Every other gesture in the alphabet
    /// falls back to inert-and-harmless.
    pub(crate) rows: &'static [Row],
}

impl NodeContract {
    /// A focus stop declared to do nothing but take focus.
    ///
    /// The registry guard wants a row for every stop, so this is how an element
    /// says "yes, this was looked at, and it is inert" — which distinguishes a
    /// considered stop from one nobody has thought about yet.
    pub(crate) const fn inert(node: &'static str) -> Self {
        Self { node, rows: &[] }
    }

    /// A node whose reactions are `rows`.
    pub(crate) const fn new(node: &'static str, rows: &'static [Row]) -> Self {
        Self { node, rows }
    }
}

/// One registered element's contract table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ElementContract {
    /// The `UiElement::id` this belongs to.
    pub(crate) element: &'static str,
    /// Its nodes' contracts.
    pub(crate) nodes: &'static [NodeContract],
}

mod contracts;
pub(crate) use contracts::CONTRACTS;

/// Stand up the registrations an element's spawn-attached observers read.
///
/// The registry's `spawn` functions attach observers belonging to widgets
/// whose **plugins** declare the messages and non-send handles those
/// observers take as system parameters — `PieMenuPlugin` declares
/// `OpenPieMenu`, `MediaEnginePlugin` inserts `MediaSurfaces`. The gallery
/// adds the whole plugins; a sweep that wants the widget's reaction and not
/// its runtime adds the registrations alone, so a `MessageWriter` has
/// somewhere inert to write and a `NonSend` handle exists to be read.
///
/// Each line here was found the same way: without it the observer fails
/// parameter validation on the first gesture and takes the app down, which
/// is the sweep's inert-and-harmless default doing its job. Anything a new
/// element needs belongs here beside them.
///
/// Shared with [`crate::floater_chrome`], whose sweep spawns the *same*
/// content specimens inside their windows and drives a pointer over them — two
/// sweeps with one answer to "what does a specimen need to be live", so a new
/// element's hosting is added once.
pub(crate) fn install_element_hosting(app: &mut App) {
    // The widgets' runtime halves, exactly the set the gallery adds and no
    // more: pure logic, no renderer, no engine. A specimen without its
    // plugin is an inert shell, so a contract row taken from one would be
    // pinning the shell rather than the widget.
    app.add_plugins((
        crate::menu::MenuWidgetPlugin,
        crate::ui_tab::TabWidgetPlugin,
        crate::ui_radio::RadioWidgetPlugin,
        crate::ui_text_input::TextInputPlugin,
        crate::ui_search::SearchFieldPlugin,
        crate::emoji_complete::ColonCompletePlugin,
        crate::chat_input::ChatInputPlugin,
        crate::local_chat_input::LocalChatInputPlugin,
    ));
    // The messages whose *writers* are attached by a spawn but whose
    // registration lives in a plugin the sweep does not want whole.
    // `radial-menu-target`'s right-click observer opens a pie; the chat
    // input's emoji button opens the picker floater; a linkified link
    // reaches for the session and the web browser.
    app.add_message::<crate::pie_menu::OpenPieMenu>();
    app.add_message::<crate::emoji_picker::OpenEmojiPicker>();
    app.add_message::<sl_client_bevy::SlCommand>();
    app.add_message::<crate::world_api::OpenWebBrowser>();
    // `browser-view`: every pointer and key observer reads the surface
    // table before it reaches the disabled check. Empty is the right
    // fixture — no CEF, no engine, and the widget stays the placeholder.
    app.insert_non_send(crate::media_engine::MediaSurfaces::default());
}

#[cfg(test)]
mod tests {
    use super::{
        CONTRACTS, ElementContract, Gesture, KEYBOARD_ALPHABET, LayoutClaim, NodeContract,
        POINTER_ALPHABET, Row, install_element_hosting,
    };
    use crate::ui_element::{ElementCx, UiAction, UiElement};
    use crate::ui_elements::ELEMENTS;
    use crate::ui_test::interact::{self, InteractionTest};
    use crate::ui_test::{
        TestError, drain_actions, find_by_name, focusable_nodes, interaction_violations,
        interactive_nodes, settle, spawn_element_into,
    };
    use bevy::input::keyboard::Key;
    use bevy::prelude::*;

    /// An interactive app with one registered element spawned and settled.
    ///
    /// The whole of a sweep cell's setup, and deliberately not
    /// `LayoutTest`-flavoured: a contract is about what a *pointer* does, so the
    /// element is laid out under the picking stack it will be clicked through.
    fn element_app(test: InteractionTest, element: &UiElement) -> App {
        let mut app = test.build();
        install_element_hosting(&mut app);
        spawn_element_into(&mut app, element, ElementCx::new());
        settle(&mut app);
        app
    }

    /// How far a drag travels, in logical pixels: far enough to leave a small
    /// control entirely, which is the case a press-and-slide-off must survive.
    const DRAG_DISTANCE: Vec2 = Vec2::new(64.0, 40.0);

    /// How many frames a drag is stepped over. Four, because a reader that only
    /// looks at the press and the release would pass a one-step drag.
    const DRAG_STEPS: u32 = 4;

    /// Wheel lines per scroll gesture.
    const SCROLL_LINES: f32 = 3.0;

    /// The contract for `element`, or `None` when the table has no entry.
    fn contract_for(element: &str) -> Option<&'static ElementContract> {
        CONTRACTS
            .iter()
            .find(|contract| contract.element == element)
    }

    /// The contract for one node of `element`.
    fn node_contract(element: &str, node: &str) -> Option<&'static NodeContract> {
        contract_for(element)?
            .nodes
            .iter()
            .find(|contract| contract.node == node)
    }

    /// The row governing `gesture` on that node, if the table declares one.
    fn row_for(element: &str, node: &str, gesture: Gesture) -> Option<&'static Row> {
        node_contract(element, node)?
            .rows
            .iter()
            .find(|row| row.gesture == gesture)
    }

    /// The key a keyboard gesture presses: its physical code and its logical
    /// meaning, which the two-message rule says must agree.
    ///
    /// `None` for the pointer gestures, which is also how [`drive`] tells the
    /// two halves of the alphabet apart.
    fn key_of(gesture: Gesture) -> Option<(KeyCode, Key)> {
        Some(match gesture {
            Gesture::Tab | Gesture::ShiftTab => (KeyCode::Tab, Key::Tab),
            Gesture::Enter => (KeyCode::Enter, Key::Enter),
            Gesture::Space => (KeyCode::Space, Key::Space),
            Gesture::Escape => (KeyCode::Escape, Key::Escape),
            Gesture::ArrowUp => (KeyCode::ArrowUp, Key::ArrowUp),
            Gesture::ArrowDown => (KeyCode::ArrowDown, Key::ArrowDown),
            Gesture::ArrowLeft => (KeyCode::ArrowLeft, Key::ArrowLeft),
            Gesture::ArrowRight => (KeyCode::ArrowRight, Key::ArrowRight),
            Gesture::PrimaryClick
            | Gesture::MiddleClick
            | Gesture::SecondaryClick
            | Gesture::DoubleClick
            | Gesture::ScrollUp
            | Gesture::ScrollDown
            | Gesture::DragAcross => return None,
        })
    }

    /// Drive one gesture at the named node, and settle.
    ///
    /// # Errors
    ///
    /// When the node cannot be aimed at — which is itself a finding, since a
    /// named node the pointer cannot reach is a node no user can use.
    fn drive(app: &mut App, node: &str, gesture: Gesture) -> Result<(), String> {
        if let Some((key_code, logical)) = key_of(gesture) {
            let entity =
                find_by_name(app, node).ok_or_else(|| format!("no node named `{node}`"))?;
            interact::focus(app, entity);
            if gesture == Gesture::ShiftTab {
                interact::with_modifier(app, KeyCode::ShiftLeft, Key::Shift, |app| {
                    interact::tap(app, key_code, logical);
                });
            } else {
                interact::tap(app, key_code, logical);
            }
            settle(app);
            return Ok(());
        }

        let at = interact::centre_of(app, node)
            .ok_or_else(|| format!("no laid-out node named `{node}`"))?;
        match gesture {
            Gesture::PrimaryClick => interact::click(app, at, MouseButton::Left),
            Gesture::MiddleClick => interact::click(app, at, MouseButton::Middle),
            Gesture::SecondaryClick => interact::click(app, at, MouseButton::Right),
            Gesture::DoubleClick => interact::double_click(app, at, MouseButton::Left),
            Gesture::ScrollUp => interact::scroll(app, at, Vec2::new(0.0, SCROLL_LINES)),
            Gesture::ScrollDown => interact::scroll(app, at, Vec2::new(0.0, -SCROLL_LINES)),
            Gesture::DragAcross => {
                // Component-wise `f32`: the workspace's arithmetic lint fires on
                // `glam`'s overloaded operators.
                let to = Vec2::new(at.x + DRAG_DISTANCE.x, at.y + DRAG_DISTANCE.y);
                interact::drag(app, at, to, DRAG_STEPS, MouseButton::Left);
            }
            // Driven and returned above: `key_of` answered for these.
            Gesture::Tab
            | Gesture::ShiftTab
            | Gesture::Enter
            | Gesture::Space
            | Gesture::Escape
            | Gesture::ArrowUp
            | Gesture::ArrowDown
            | Gesture::ArrowLeft
            | Gesture::ArrowRight => {}
        }
        settle(app);
        Ok(())
    }

    /// Check one settled app against the expectation for this cell, appending
    /// what is wrong to `failures`.
    fn judge(
        app: &mut App,
        test: InteractionTest,
        element: &str,
        node: &str,
        gesture: Gesture,
        failures: &mut Vec<String>,
    ) {
        let emitted: Vec<&'static str> = drain_actions(app)
            .into_iter()
            .map(|action: UiAction| action.action)
            .collect();
        let row = row_for(element, node, gesture);
        let expected: &[&str] = row.map_or(&[], |row| row.emits);

        if emitted.as_slice() != expected {
            let observed = emitted
                .iter()
                .map(|action| format!("{action:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            failures.push(format!(
                "element `{element}` node `{node}` on {gesture:?}: emitted [{observed}], the \
                 contract wants {expected:?}. If the reaction is right, the row is \
                 `Row::emits(Gesture::{gesture:?}, &[{observed}])` under \
                 `NodeContract::new({node:?}, …)` in `ui_contract::contracts`."
            ));
        }

        if let Some(probe) = row.and_then(|row| row.probe)
            && !(probe.check)(app)
        {
            failures.push(format!(
                "element `{element}` node `{node}` on {gesture:?}: {} did not hold",
                probe.what
            ));
        }

        let violations = interaction_violations(app, test.layout());
        let claim = row.map_or(LayoutClaim::Clean, |row| row.layout);
        if let Some(why) = judge_layout(claim, &violations) {
            failures.push(format!(
                "element `{element}` node `{node}` after {gesture:?}: {why}"
            ));
        }
    }

    /// What `claim` says about the `violations` a settled gesture left behind:
    /// `None` when the claim holds, else what is wrong with it.
    ///
    /// Split out of [`judge`] so both directions of the **inverted** claim can
    /// be driven directly — see `a_known_broken_pin_fails_the_day_it_is_fixed`.
    /// A canary that has never been heard is not a canary.
    fn judge_layout(claim: LayoutClaim, violations: &[String]) -> Option<String> {
        match claim {
            LayoutClaim::Clean => (!violations.is_empty()).then(|| format!("{violations:#?}")),
            LayoutClaim::KnownBroken(bug) => violations.is_empty().then(|| {
                format!(
                    "the layout is clean, but the row pins it as broken against `{bug}`. If that \
                     bug is fixed, delete the `.known_broken({bug:?})` from this row — the pin \
                     exists to tell you exactly this."
                )
            }),
        }
    }

    /// The sweep: every element × every address × every gesture in `alphabet`.
    ///
    /// `stops_only` picks the address space — the focus stops for the keyboard
    /// alphabet, every named node for the pointer one.
    ///
    /// One fresh app per cell, because a gesture is allowed to change state and
    /// the next cell must not inherit it: a click that opens a menu would
    /// otherwise put the menu under the following cell's pointer, and the sweep
    /// would be pinning an order rather than a contract.
    fn sweep(alphabet: &[Gesture], stops_only: bool) -> Vec<String> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for element in ELEMENTS {
            let addresses = {
                let mut probe = element_app(test, element);
                if stops_only {
                    focusable_nodes(&mut probe)
                } else {
                    interactive_nodes(&mut probe)
                }
            };
            for node in &addresses {
                for gesture in alphabet {
                    let mut app = element_app(test, element);
                    // A spawn is allowed to announce itself; a cell is about
                    // what the *gesture* did.
                    let _settling = drain_actions(&mut app);
                    match drive(&mut app, node, *gesture) {
                        Ok(()) => {
                            judge(&mut app, test, element.id, node, *gesture, &mut failures);
                        }
                        Err(why) => failures.push(format!("element `{}`: {why}", element.id)),
                    }
                }
            }
        }
        failures
    }

    // -----------------------------------------------------------------------
    // The sweep, split by gesture family. One sweep conceptually; four tests
    // because cargo's test threads are the parallelism, and a single one would
    // run the whole alphabet on one thread.
    // -----------------------------------------------------------------------

    /// **Every element × every interactive node × the three pointer buttons and
    /// the double click.**
    ///
    /// The family that ships bugs: a control that treats the secondary button
    /// as the primary one, or fires twice on a double click, is a control the
    /// user breaks by accident.
    #[test]
    fn every_node_answers_for_every_pointer_button() {
        let failures = sweep(
            &[
                Gesture::PrimaryClick,
                Gesture::MiddleClick,
                Gesture::SecondaryClick,
                Gesture::DoubleClick,
            ],
            false,
        );
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every element × every interactive node × scrolling and dragging.**
    ///
    /// The gestures that reach a control without a press ever landing on it:
    /// the wheel over a panel that is not a list, and a press that slides off
    /// the button it started on.
    #[test]
    fn every_node_answers_for_the_wheel_and_a_drag() {
        let failures = sweep(
            &[Gesture::ScrollUp, Gesture::ScrollDown, Gesture::DragAcross],
            false,
        );
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every element × every focus stop × `Tab` and `Shift+Tab`.**
    ///
    /// Focus moving is not a `UiAction`, so what this pins is the default:
    /// walking the cycle from any stop emits nothing and breaks no layout.
    #[test]
    fn every_stop_answers_for_tab_navigation() {
        let failures = sweep(&[Gesture::Tab, Gesture::ShiftTab], true);
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every element × every focus stop × the activation and arrow keys.**
    ///
    /// `Enter` and `Space` are the keyboard's click, `Escape` its cancel, and
    /// the arrows are what a field must consume and a panel must not.
    #[test]
    fn every_stop_answers_for_the_activation_and_arrow_keys() {
        let failures = sweep(
            &[
                Gesture::Enter,
                Gesture::Space,
                Gesture::Escape,
                Gesture::ArrowUp,
                Gesture::ArrowDown,
                Gesture::ArrowLeft,
                Gesture::ArrowRight,
            ],
            true,
        );
        assert!(failures.is_empty(), "{failures:#?}");
    }

    // -----------------------------------------------------------------------
    // The guards: the sweep is only as honest as the registry it walks and the
    // table it compares against.
    // -----------------------------------------------------------------------

    /// **No focus stop lacks a contract row.**
    ///
    /// The registry guard, the counterpart of
    /// `ui_test::the_matrix_covers_the_whole_registry`. A stop the table says
    /// nothing about is swept as inert — right for a decoration, wrong for a
    /// control someone forgot to think about, and from the outside the two look
    /// the same. Requiring a row (an empty one, via [`NodeContract::inert`], is
    /// a legitimate answer) makes the distinction something an author states
    /// rather than something they omit.
    #[test]
    fn every_focus_stop_has_a_contract_row() {
        let test = InteractionTest::new();
        let mut missing = Vec::new();
        for element in ELEMENTS {
            let mut app = element_app(test, element);
            for stop in focusable_nodes(&mut app) {
                if node_contract(element.id, &stop).is_none() {
                    missing.push(format!(
                        "element `{}` stop `{stop}`: add `NodeContract::inert({stop:?})` — or a \
                         row saying what it does",
                        element.id
                    ));
                }
            }
        }
        assert!(missing.is_empty(), "{missing:#?}");
    }

    /// **The table names only things that exist.**
    ///
    /// The other way a pinned table rots: an element is renamed or a node
    /// dropped, the row stops matching anything, and it goes on passing by
    /// describing a world that is gone.
    #[test]
    fn the_contract_table_addresses_only_live_nodes() {
        let test = InteractionTest::new();
        let mut stale = Vec::new();
        for contract in CONTRACTS {
            let Some(element) = ELEMENTS
                .iter()
                .find(|element| element.id == contract.element)
            else {
                stale.push(format!(
                    "contract for `{}`, which is not a registered element",
                    contract.element
                ));
                continue;
            };
            let mut app = element_app(test, element);
            let live = interactive_nodes(&mut app);
            for node in contract.nodes {
                if !live.iter().any(|name| name == node.node) {
                    stale.push(format!(
                        "element `{}` has no node named `{}`",
                        contract.element, node.node
                    ));
                }
            }
        }
        assert!(stale.is_empty(), "{stale:#?}");
    }

    /// **One row per gesture per node.**
    ///
    /// A duplicate row would be silently ignored — [`row_for`] takes the first
    /// — so the second would look enforced and enforce nothing.
    #[test]
    fn no_contract_row_is_shadowed() {
        let mut duplicates = Vec::new();
        for contract in CONTRACTS {
            let mut nodes: Vec<&str> = contract.nodes.iter().map(|node| node.node).collect();
            let total = nodes.len();
            nodes.sort_unstable();
            nodes.dedup();
            if nodes.len() != total {
                duplicates.push(format!("element `{}` names a node twice", contract.element));
            }
            for node in contract.nodes {
                let mut gestures: Vec<String> = node
                    .rows
                    .iter()
                    .map(|row| format!("{:?}", row.gesture))
                    .collect();
                let rows = gestures.len();
                gestures.sort_unstable();
                gestures.dedup();
                if gestures.len() != rows {
                    duplicates.push(format!(
                        "element `{}` node `{}` declares a gesture twice",
                        contract.element, node.node
                    ));
                }
            }
        }
        assert!(duplicates.is_empty(), "{duplicates:#?}");
    }

    /// **Every pinned bug names a roadmap item that exists.**
    ///
    /// A `KnownBroken` row is a promise that someone wrote the finding down. If
    /// the id is a typo, or the task file is later renamed, the pin degenerates
    /// into a bare "this is allowed to be broken" with nothing behind it — which
    /// is the one thing a pinned table must never become.
    #[test]
    fn every_pinned_bug_names_a_live_roadmap_item() {
        let roadmap = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("roadmap");
        let mut dangling = Vec::new();
        for contract in CONTRACTS {
            for node in contract.nodes {
                for row in node.rows {
                    let pinned = match row.layout {
                        LayoutClaim::KnownBroken(bug) => Some(bug),
                        LayoutClaim::Clean => row.bug,
                    };
                    let Some(bug) = pinned else {
                        continue;
                    };
                    let found = [
                        "bugs",
                        "ready",
                        "in-progress",
                        "blocked",
                        "done",
                        "deferred",
                    ]
                    .iter()
                    .any(|status| roadmap.join(status).join(format!("{bug}.md")).is_file());
                    if !found {
                        dangling.push(format!(
                            "element `{}` node `{}` on {:?} pins `{bug}`, which is not a roadmap \
                             item",
                            contract.element, node.node, row.gesture
                        ));
                    }
                }
            }
        }
        assert!(dangling.is_empty(), "{dangling:#?}");
    }

    /// **The sweep has an alphabet and a registry to sweep.**
    ///
    /// The anti-vacuity guard every matrix here carries: a green sweep must
    /// mean it found nothing wrong, never that it ran out of work.
    #[test]
    fn the_sweep_covers_the_whole_registry() {
        assert!(!ELEMENTS.is_empty(), "no elements to sweep");
        assert!(
            POINTER_ALPHABET.len() >= 4,
            "the pointer alphabet has shrunk to something a sweep cannot claim to cover"
        );
        assert!(
            KEYBOARD_ALPHABET.len() >= 4,
            "the keyboard alphabet has shrunk to something a sweep cannot claim to cover"
        );
        assert!(
            !CONTRACTS.is_empty(),
            "no element declares a contract, so every cell is running on the default"
        );
    }

    /// **The canary's teeth.** A pinned-broken layout must fail once it is fixed.
    ///
    /// [`LayoutClaim::KnownBroken`] is the one check in this module that runs
    /// *backwards* — it asserts a bug is still present — and a backwards check
    /// that is never exercised is the easiest kind to get wrong, because a
    /// silently-inverted one passes on every row forever. Both directions are
    /// driven here through the real [`judge_layout`], so the day a pin's bug is
    /// fixed the sweep is known to say so rather than quietly agreeing.
    ///
    /// Its first subject was the chat volume dropdown, which laid out above the
    /// top of the window until it became a `Popover`; that fix is what turned
    /// this from a mechanism with a user into a mechanism with a test.
    #[test]
    fn a_known_broken_pin_fails_the_day_it_is_fixed() -> Result<(), TestError> {
        const PIN: &str = "viewer-chat-volume-dropdown-opens-off-screen";
        let pinned = Row::emits(Gesture::PrimaryClick, &[]).known_broken(PIN);
        let breach = vec!["`some-panel`: laid out outside the viewport".to_owned()];

        // The pin holds while the breakage is there, and fires the moment it is
        // not — naming the row to delete.
        assert!(
            judge_layout(pinned.layout, &breach).is_none(),
            "a pinned-broken row failed while its breakage was still present"
        );
        let complaint = judge_layout(pinned.layout, &[])
            .ok_or("a pinned-broken row passed on a clean layout, so the pin means nothing")?;
        assert!(
            complaint.contains(PIN),
            "the complaint does not name the bug to unpin: {complaint}"
        );

        // And the ordinary claim is the other way round, so the two are not the
        // same check wearing different names.
        assert!(
            judge_layout(LayoutClaim::Clean, &[]).is_none(),
            "a clean layout failed the default claim"
        );
        assert!(
            judge_layout(LayoutClaim::Clean, &breach).is_some(),
            "a broken layout passed the default claim"
        );
        Ok(())
    }

    /// The registry's plainest control, and the subject of the teeth below:
    /// one focusable button whose left click emits `save`.
    const TEETH_ELEMENT: &str = "button";

    /// That button's node name — `button:{action}`, per `spawn_button`.
    const TEETH_NODE: &str = "button:save";

    /// **The teeth.** A contract that cannot fail protects nothing.
    ///
    /// Both directions of a broken contract, driven through the real sweep
    /// machinery rather than a mock of it: an action the table does not declare
    /// must fail, and a declared action that never fires must fail. Each is
    /// produced by asking [`judge`] about a *different* address than the one
    /// that was driven, which is exactly the shape of the two mistakes — a
    /// reaction nobody wrote down, and a written-down reaction that has gone.
    #[test]
    fn the_sweep_catches_both_directions_of_a_broken_contract() {
        let test = InteractionTest::new();
        assert!(
            ELEMENTS.iter().any(|element| element.id == TEETH_ELEMENT),
            "the `{TEETH_ELEMENT}` element left the registry, so these teeth bite nothing"
        );
        let Some(element) = ELEMENTS.iter().find(|element| element.id == TEETH_ELEMENT) else {
            return;
        };

        // Undeclared: the click really emits, and the expectation looked up for
        // an address no contract mentions says nothing.
        let mut undeclared = Vec::new();
        let mut app = element_app(test, element);
        let _settling = drain_actions(&mut app);
        let clicked = drive(&mut app, TEETH_NODE, Gesture::PrimaryClick);
        assert!(
            matches!(clicked, Ok(())),
            "the button could not be clicked: {clicked:?}"
        );
        judge(
            &mut app,
            test,
            TEETH_ELEMENT,
            "button:no-such-node",
            Gesture::PrimaryClick,
            &mut undeclared,
        );
        assert!(
            !undeclared.is_empty(),
            "an action nothing declared passed the sweep"
        );

        // Missing: the wheel emits nothing, and the expectation looked up is
        // the declared left-click row.
        let mut missing = Vec::new();
        let mut quiet = element_app(test, element);
        let _quiet_settling = drain_actions(&mut quiet);
        let scrolled = drive(&mut quiet, TEETH_NODE, Gesture::ScrollUp);
        assert!(
            matches!(scrolled, Ok(())),
            "the button could not be scrolled over: {scrolled:?}"
        );
        judge(
            &mut quiet,
            test,
            TEETH_ELEMENT,
            TEETH_NODE,
            Gesture::PrimaryClick,
            &mut missing,
        );
        assert!(
            !missing.is_empty(),
            "a declared action that never fired passed the sweep"
        );
    }
}
