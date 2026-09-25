//! The pinned contract table: what each registered element's named nodes do.
//!
//! Companion to `crate::ui_elements::ELEMENTS` — an element is *declared* there
//! and its *reactions* are declared here, the same split the pie menu makes
//! between a `PieMenuDef` and its compass-address table.
//!
//! Read a row as a sentence: *this gesture, on this node of this element, emits
//! exactly these actions*. Everything not written down is inert-and-harmless,
//! which is why a 54-element registry needs no row for most of its nodes: they
//! are labels, backdrops and containers, and the sweep already holds them to
//! doing nothing. Two rules keep the table honest, both enforced by the tests
//! in the parent module: every focus stop needs a row here (an explicitly inert
//! one counts), and every row must address a node that still exists.
//!
//! # What the first sweep found, and what became of it
//!
//! One group of rows still describes behaviour that is *not* what the reference
//! viewer does, and it is deliberately *not* flagged as a defect:
//!
//! - The **arrow keys on a tab strip and a radio group**, which move the
//!   selection on all four arrows regardless of the widget's orientation. That
//!   one is upstream *by design* — `bevy_ui_widgets`' `radio.rs` reads
//!   `ArrowUp | ArrowLeft` as "previous" and the other two as "next" without
//!   consulting a layout, where its `menu.rs` does consult `MenuLayout` — so it
//!   is recorded as ordinary `Row::emits` rather than flagged. It is a parity
//!   gap with the reference viewer, not a defect.
//!
//! A `Row::emits` row is exact in both directions, so this becoming right fails
//! the sweep and the table is corrected in the same commit. That is the point
//! of pinning rather than tolerating.
//!
//! Two groups of pinned defects have already left the table, and how they left
//! is the point of pinning at all.
//!
//! 124 rows recorded every control that answered a **middle or secondary
//! click** as readily as a primary one, against
//! `viewer-widget-any-mouse-button-activates` — upstream in `bevy_ui_widgets`,
//! whose observers never read the event's button. The fix landed in the
//! `taladar/bevy` fork and arrived here as exactly what the pin promised: 124
//! sweep failures saying "emitted [], the contract wants […]", and a correction
//! that is the table diff deleting them. Those two clicks are inert now, which
//! needs no row at all.
//!
//! Two rows on the chat volume button were `known_broken` against
//! `viewer-chat-volume-dropdown-opens-off-screen`: the whisper/say/shout panel
//! was hand-positioned upward with no fallback placement and no window margin,
//! so it laid out above the top of the window. It is a `Popover` now, which
//! asks which side has room, and the pin failed the day that landed.

use super::{ElementContract, Gesture, NodeContract, Probe, Row};
use bevy::input_focus::InputFocus;
use bevy::prelude::{App, Has, Name};
use bevy::ui::Checked;
use sl_viewer_ui_widgets::ui_trackball::{SPECIMEN_MOON_AIM, SPECIMEN_SUN_AIM, TrackballAim};

/// The named node this probe asks about, since a [`Probe`] is handed the app
/// and not the address it was declared under.
const FOCUSED_FIELD: &str = "text-input-line:field";

/// A click on a text field must leave the **keyboard** on it.
///
/// The reaction that is invisible to the action recorder: taking focus emits no
/// `UiAction`, so without a probe a field that had quietly stopped accepting
/// the caret would sweep as "inert" and pass. It is also the reaction a user
/// notices first — a field you can click but not type into.
const CLICK_TAKES_THE_CARET: Probe = Probe {
    what: "the clicked field holds the keyboard",
    check: |app: &mut App| {
        let Some(focused) = app.world().resource::<InputFocus>().get() else {
            return false;
        };
        app.world()
            .get::<Name>(focused)
            .is_some_and(|name| name.as_str() == FOCUSED_FIELD)
    },
};

/// The read-only field specimen, for [`CLICK_TAKES_THE_CARET_READ_ONLY`].
const FOCUSED_READ_ONLY_FIELD: &str = "text-input-read-only:field";

/// A click on a **read-only** field must leave the keyboard on it too.
///
/// The one reaction that separates the two greyed stances: a disabled field
/// refuses the caret (and so cannot be copied from), a read-only one takes it
/// and keeps it, which is what puts a selection within reach of `Ctrl+C`. Both
/// look identical, so only a probe can tell them apart.
const CLICK_TAKES_THE_CARET_READ_ONLY: Probe = Probe {
    what: "the clicked read-only field holds the keyboard",
    check: |app: &mut App| {
        let Some(focused) = app.world().resource::<InputFocus>().get() else {
            return false;
        };
        app.world()
            .get::<Name>(focused)
            .is_some_and(|name| name.as_str() == FOCUSED_READ_ONLY_FIELD)
    },
};

/// The gallery specimen's sun trackball.
const SUN_TRACKBALL: &str = "gallery-sun:trackball";

/// Its moon.
const MOON_TRACKBALL: &str = "gallery-moon:trackball";

/// The aim the named trackball is holding, or `None` if this element has none.
fn aim_of(app: &mut App, node: &str) -> Option<TrackballAim> {
    let mut query = app.world_mut().query::<(&Name, &TrackballAim)>();
    query
        .iter(app.world())
        .find(|(name, _aim)| name.as_str() == node)
        .map(|(_name, aim)| *aim)
}

/// Whether the named trackball is aimed at the **zenith** — what a click on the
/// centre of the disc means, and the reaction that emits nothing at all.
fn aimed_at_the_zenith(app: &mut App, node: &str) -> bool {
    aim_of(app, node).is_some_and(|aim| (aim.elevation.abs() - 90.0).abs() < 0.5)
}

/// Whether the named trackball has moved off the aim it was spawned with.
fn aim_moved(app: &mut App, node: &str, from: TrackballAim) -> bool {
    aim_of(app, node).is_some_and(|aim| aim != from)
}

/// Whether the named trackball is aimed somewhere between the pole and the
/// horizon — where a drag across the disc leaves it.
///
/// The sweep's drag starts at the centre and ends far outside the rim, and the
/// two ends bracket the two things worth pinning: the press aims at the zenith,
/// and the part of the travel that is still *on* the disc keeps aiming, so the
/// control ends up neither where the press put it nor pinned to the horizon by
/// the part that left. That last is the widget's deliberate refusal to clamp —
/// the reference's `pointInTouchCircle` guard.
fn aimed_off_both_ends(app: &mut App, node: &str) -> bool {
    aim_of(app, node).is_some_and(|aim| {
        let height = aim.elevation.abs();
        height > 0.5 && height < 89.5
    })
}

/// A click on the sun disc's centre aims the sun straight up.
///
/// The whole of the trackball's reaction is a component nobody records: it
/// emits `ValueChange`, not a `UiAction`, so without these probes every gesture
/// on it would sweep as "inert" and a control that had quietly stopped aiming
/// would pass.
const SUN_CLICK_AIMS_UP: Probe = Probe {
    what: "the clicked sun trackball is aimed at the zenith",
    check: |app: &mut App| aimed_at_the_zenith(app, SUN_TRACKBALL),
};

/// The same for the moon — which keeps its hemisphere, so "the zenith" there is
/// the nadir, and the check is on the magnitude.
const MOON_CLICK_AIMS_UP: Probe = Probe {
    what: "the clicked moon trackball is aimed straight up or straight down",
    check: |app: &mut App| aimed_at_the_zenith(app, MOON_TRACKBALL),
};

/// A drag across the sun's disc leaves it aimed where the pointer last was on
/// the disc.
const SUN_DRAG_AIMS: Probe = Probe {
    what: "the dragged sun trackball is aimed off both the zenith and the horizon",
    check: |app: &mut App| aimed_off_both_ends(app, SUN_TRACKBALL),
};

/// The same for the moon, which keeps its own hemisphere throughout.
const MOON_DRAG_AIMS: Probe = Probe {
    what: "the dragged moon trackball is aimed off both the pole and the horizon",
    check: |app: &mut App| aimed_off_both_ends(app, MOON_TRACKBALL),
};

/// An arrow key steps the sun's aim off where it was spawned.
const SUN_ARROW_STEPS: Probe = Probe {
    what: "an arrow key moved the sun trackball's aim",
    check: |app: &mut App| aim_moved(app, SUN_TRACKBALL, SPECIMEN_SUN_AIM),
};

/// The same for the moon.
const MOON_ARROW_STEPS: Probe = Probe {
    what: "an arrow key moved the moon trackball's aim",
    check: |app: &mut App| aim_moved(app, MOON_TRACKBALL, SPECIMEN_MOON_AIM),
};

/// Whether the node named `node` is ticked.
///
/// A checkbox's whole reaction is this marker: `bevy_ui_widgets`' headless
/// [`Checkbox`](bevy::ui_widgets::Checkbox) announces an activation as a
/// `ValueChange<bool>` and emits no `UiAction` at all, `spawn_checkbox`'s
/// self-update observer turns that into [`Checked`](bevy::ui::Checked), and the
/// skin draws the tick from `:checked`. So without a probe every gesture on
/// every checkbox in the viewer would sweep as "inert" and a box that had
/// quietly stopped toggling would pass — which one had, until this row was
/// written: nothing observed the event at all.
fn ticked(app: &mut App, node: &str) -> bool {
    let mut query = app.world_mut().query::<(&Name, Has<Checked>)>();
    query
        .iter(app.world())
        .find(|(name, _ticked)| name.as_str() == node)
        .is_some_and(|(_name, ticked)| ticked)
}

/// The Preferences window's first checkbox on its first (General) tab — the
/// name-tags master toggle — spawned unticked: the specimen has no settings
/// store for the binding layer to tick it from. The other tabs' checkboxes are
/// unaddressable: a hidden panel's `TabIndex` is parked by the scaffold, which
/// `the_contract_table_addresses_only_live_nodes` says out loud.
const PREF_CHECK: &str = "preferences-row-name-tags:checkbox";

/// The Phototools window's reflections checkbox (dynamic probe content), on
/// its first tab, also spawned unticked for want of a store.
const PHOTO_CHECK: &str = "phototools:probe-dynamic:checkbox";

/// The Build Tools specimen's snap-to-grid checkbox, also ticked.
const BUILD_SNAP: &str = "build-toggle-snap:checkbox";

/// The gallery element's resting box — the one of its four spawned unticked and
/// live.
const GALLERY_RESTING: &str = "checkbox-resting:checkbox";

/// Its ticked one.
const GALLERY_CHECKED: &str = "checkbox-checked:checkbox";

/// The notification toast's "don't show me this again" box, spawned unticked.
const TOAST_IGNORE: &str = "toast-ignore:checkbox";

/// The script editor specimen's Running box, spawned ticked (a running task
/// script).
const SCRIPT_RUNNING: &str = "script-running:checkbox";

/// An activation ticks the Preferences window's first checkbox — the binding
/// layer's observer reflecting the toggle at once, store or no store.
///
/// The other direction — a ticked box clearing — is pinned by the widget's own
/// `announcing_a_change_moves_the_marker`, which drives both ways without
/// needing a second specimen.
const PREF_CHECK_SETS: Probe = Probe {
    what: "the activated General-tab checkbox is now ticked",
    check: |app: &mut App| ticked(app, PREF_CHECK),
};

/// The gallery's resting box **ticks** — the direction only an unticked
/// specimen can pin.
const GALLERY_SETS: Probe = Probe {
    what: "the activated resting checkbox is now ticked",
    check: |app: &mut App| ticked(app, GALLERY_RESTING),
};

/// Its ticked one clears.
const GALLERY_CLEARS: Probe = Probe {
    what: "the activated ticked checkbox is no longer ticked",
    check: |app: &mut App| !ticked(app, GALLERY_CHECKED),
};

/// The toast's ignore box ticks — and it is the box's own `Checked` that the
/// resolve pass reads when the toast is answered, so a box that stopped ticking
/// would silently stop suppressing anything.
const TOAST_IGNORE_SETS: Probe = Probe {
    what: "the activated ignore checkbox is now ticked",
    check: |app: &mut App| ticked(app, TOAST_IGNORE),
};

/// The same for the Build Tools specimen.
const SNAP_CLEARS: Probe = Probe {
    what: "the activated snap-to-grid checkbox is no longer ticked",
    check: |app: &mut App| !ticked(app, BUILD_SNAP),
};

/// The same for the script editor's Running box.
const SCRIPT_RUNNING_CLEARS: Probe = Probe {
    what: "the activated Running checkbox is no longer ticked",
    check: |app: &mut App| !ticked(app, SCRIPT_RUNNING),
};

/// The same for the Phototools window's reflections checkbox.
const PHOTO_SETS: Probe = Probe {
    what: "the activated reflections checkbox is now ticked",
    check: |app: &mut App| ticked(app, PHOTO_CHECK),
};

/// The three gestures that activate a checkbox — the keyboard's two and the
/// pointer's one — each leaving `probe`.
///
/// A checkbox answers all three identically, so the rows are generated rather
/// than written out twice; the double click is deliberately absent, because
/// toggling twice lands back where it started and there is nothing to pin.
const fn toggles(probe: Probe) -> [Row; 3] {
    [
        Row::leaves(Gesture::PrimaryClick, probe),
        Row::leaves(Gesture::Enter, probe),
        Row::leaves(Gesture::Space, probe),
    ]
}

/// The Preferences window's rows, named so `CONTRACTS` can borrow them: a
/// `const fn` call in a `&[…]` there would be a temporary.
const PREF_CHECK_ROWS: [Row; 3] = toggles(PREF_CHECK_SETS);

/// The Photo Tools specimen's.
const PHOTO_ROWS: [Row; 3] = toggles(PHOTO_SETS);

/// The Build Tools specimen's.
const SNAP_ROWS: [Row; 3] = toggles(SNAP_CLEARS);

/// The script editor specimen's Running box.
const SCRIPT_RUNNING_ROWS: [Row; 3] = toggles(SCRIPT_RUNNING_CLEARS);

/// The gallery element's resting box.
const GALLERY_RESTING_ROWS: [Row; 3] = toggles(GALLERY_SETS);

/// Its ticked one.
const GALLERY_CHECKED_ROWS: [Row; 3] = toggles(GALLERY_CLEARS);

/// The toast's ignore box.
const TOAST_IGNORE_ROWS: [Row; 3] = toggles(TOAST_IGNORE_SETS);

/// A toast's Cancel button.
const TOAST_CANCEL: NodeContract = NodeContract::new(
    "toast-button:Cancel",
    &[
        Row::emits(Gesture::PrimaryClick, &["Cancel"]),
        Row::emits(Gesture::DoubleClick, &["Cancel", "Cancel"]),
        Row::emits(Gesture::Enter, &["Cancel"]),
        Row::emits(Gesture::Space, &["Cancel"]),
    ],
);

/// A toast's OK button.
const TOAST_OK: NodeContract = NodeContract::new(
    "toast-button:OK",
    &[
        Row::emits(Gesture::PrimaryClick, &["OK"]),
        Row::emits(Gesture::DoubleClick, &["OK", "OK"]),
        Row::emits(Gesture::Enter, &["OK"]),
        Row::emits(Gesture::Space, &["OK"]),
    ],
);

/// A toast's close box.
const TOAST_CLOSE: NodeContract = NodeContract::new(
    "toast-close",
    &[
        Row::emits(Gesture::PrimaryClick, &["close"]),
        Row::emits(Gesture::DoubleClick, &["close", "close"]),
        Row::emits(Gesture::Enter, &["close"]),
        Row::emits(Gesture::Space, &["close"]),
    ],
);

/// A toast card's nodes: the ignore checkbox, the two buttons, the close box
/// and the input field.
const TOAST_NODES: [NodeContract; 5] = [
    NodeContract::new(TOAST_IGNORE, &TOAST_IGNORE_ROWS),
    TOAST_CANCEL,
    TOAST_OK,
    TOAST_CLOSE,
    NodeContract::inert("toast-input:field"),
];

/// The full channel: the same card as [`TOAST_NODES`], above the "N more ▸"
/// overflow control that pages the queue.
const TOAST_NODES_WITH_OVERFLOW: [NodeContract; 6] = [
    NodeContract::new(
        "notification-overflow",
        &[
            Row::emits(Gesture::PrimaryClick, &["overflow"]),
            Row::emits(Gesture::DoubleClick, &["overflow", "overflow"]),
            Row::emits(Gesture::Enter, &["overflow"]),
            Row::emits(Gesture::Space, &["overflow"]),
        ],
    ),
    NodeContract::new(TOAST_IGNORE, &TOAST_IGNORE_ROWS),
    TOAST_CANCEL,
    TOAST_OK,
    TOAST_CLOSE,
    NodeContract::inert("toast-input:field"),
];

/// Every element's contract, keyed by `UiElement::id`.
pub(crate) const CONTRACTS: &[ElementContract] = &[
    ElementContract {
        element: "bottom-toolbar",
        nodes: &[
            NodeContract::new(
                "bottom-toolbar-button:toggle-appearance",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-appearance"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["toggle-appearance", "toggle-appearance"],
                    ),
                    Row::emits(Gesture::Enter, &["toggle-appearance"]),
                    Row::emits(Gesture::Space, &["toggle-appearance"]),
                ],
            ),
            NodeContract::new(
                "bottom-toolbar-button:toggle-inventory",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-inventory"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["toggle-inventory", "toggle-inventory"],
                    ),
                    Row::emits(Gesture::Enter, &["toggle-inventory"]),
                    Row::emits(Gesture::Space, &["toggle-inventory"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "browser-view",
        nodes: &[NodeContract::inert("browser-view")],
    },
    ElementContract {
        element: "build-create",
        nodes: &[
            NodeContract::new(
                "build-create-base:radio-group",
                &[
                    Row::emits(Gesture::PrimaryClick, &["select-radio"]),
                    Row::emits(Gesture::DoubleClick, &["select-radio"]),
                    Row::emits(Gesture::ArrowUp, &["select-radio"]),
                    Row::emits(Gesture::ArrowDown, &["select-radio"]),
                    Row::emits(Gesture::ArrowLeft, &["select-radio"]),
                    Row::emits(Gesture::ArrowRight, &["select-radio"]),
                ],
            ),
            NodeContract::inert("build-create-tree:combo"),
        ],
    },
    ElementContract {
        element: "build-tools",
        nodes: &[
            NodeContract::new(BUILD_SNAP, &SNAP_ROWS),
            NodeContract::inert("build-toggle-local-frame:checkbox"),
            NodeContract::inert("build-toggle-edit-linked:checkbox"),
            NodeContract::inert("build-toggle-stretch-both:checkbox"),
            NodeContract::inert("build-tools:link-part-prev"),
            NodeContract::inert("build-tools:link-part-next"),
            NodeContract::inert("build-grid-unit:field"),
            NodeContract::inert("build-pos-x:field"),
            NodeContract::inert("build-pos-y:field"),
            NodeContract::inert("build-pos-z:field"),
            // The General tab, the page the window opens on. Its controls act
            // on the selection through the session, which a specimen has none
            // of, so they are inert here and emit nothing of the widgets' own.
            NodeContract::inert("build-name:field"),
            NodeContract::inert("build-desc:field"),
            NodeContract::inert("build-params:action:build-set-group"),
            NodeContract::inert("build-params:action:build-deed"),
            NodeContract::inert("build-share-group:checkbox"),
            NodeContract::inert("build-perm-modify:checkbox"),
            NodeContract::inert("build-perm-copy:checkbox"),
            NodeContract::inert("build-perm-transfer:checkbox"),
            NodeContract::inert("build-perm-move:checkbox"),
            NodeContract::new(
                "build-tabs:tab-strip",
                &[
                    Row::emits(Gesture::ArrowUp, &["select-tab"]),
                    Row::emits(Gesture::ArrowDown, &["select-tab"]),
                    Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                    Row::emits(Gesture::ArrowRight, &["select-tab"]),
                ],
            ),
            NodeContract::new(
                "build-tool:radio-group",
                &[
                    // The group's centre is not an option that moves the tool:
                    // the live window's six tools wrap to two lines at its
                    // width, and the centre falls between them or on Move, the
                    // tool the specimen opens on — a gap and a re-pick of the
                    // active tool are both inert. The arrows still step it.
                    Row::emits(Gesture::PrimaryClick, &[]),
                    Row::emits(Gesture::DoubleClick, &[]),
                    Row::emits(Gesture::ArrowUp, &["select-radio"]),
                    Row::emits(Gesture::ArrowDown, &["select-radio"]),
                    Row::emits(Gesture::ArrowLeft, &["select-radio"]),
                    Row::emits(Gesture::ArrowRight, &["select-radio"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "button",
        nodes: &[NodeContract::new(
            "button:save",
            &[
                Row::emits(Gesture::PrimaryClick, &["save"]),
                Row::emits(Gesture::DoubleClick, &["save", "save"]),
                Row::emits(Gesture::Enter, &["save"]),
                Row::emits(Gesture::Space, &["save"]),
            ],
        )],
    },
    ElementContract {
        element: "button-row",
        nodes: &[
            NodeContract::new(
                "button:cancel",
                &[
                    Row::emits(Gesture::PrimaryClick, &["cancel"]),
                    Row::emits(Gesture::DoubleClick, &["cancel", "cancel"]),
                    Row::emits(Gesture::Enter, &["cancel"]),
                    Row::emits(Gesture::Space, &["cancel"]),
                ],
            ),
            NodeContract::new(
                "button:discard",
                &[
                    Row::emits(Gesture::PrimaryClick, &["discard"]),
                    Row::emits(Gesture::DoubleClick, &["discard", "discard"]),
                    Row::emits(Gesture::Enter, &["discard"]),
                    Row::emits(Gesture::Space, &["discard"]),
                ],
            ),
            NodeContract::new(
                "button:save",
                &[
                    Row::emits(Gesture::PrimaryClick, &["save"]),
                    Row::emits(Gesture::DoubleClick, &["save", "save"]),
                    Row::emits(Gesture::Enter, &["save"]),
                    Row::emits(Gesture::Space, &["save"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "checkbox-states",
        nodes: &[
            NodeContract::new(GALLERY_RESTING, &GALLERY_RESTING_ROWS),
            NodeContract::new(GALLERY_CHECKED, &GALLERY_CHECKED_ROWS),
            // The two refused boxes carry `InteractionDisabled`, so every
            // gesture on them is inert — which is the whole of what a refused
            // control claims, and worth saying rather than omitting.
            NodeContract::inert("checkbox-refused:checkbox"),
            NodeContract::inert("checkbox-refused-checked:checkbox"),
        ],
    },
    ElementContract {
        element: "chat-input",
        nodes: &[NodeContract::inert("chat-input:field")],
    },
    ElementContract {
        element: "combo-box",
        nodes: &[NodeContract::inert("combo-demo:combo")],
    },
    ElementContract {
        element: "debug-settings",
        nodes: &[
            NodeContract::inert("debug-settings-alpha:field"),
            NodeContract::inert("debug-settings-f32:field"),
            NodeContract::inert("debug-settings-i32:field"),
            NodeContract::inert("debug-settings-rect:field"),
            NodeContract::inert("debug-settings-scope:combo"),
            NodeContract::inert("debug-settings-string:field"),
            NodeContract::inert("debug-settings-u32:field"),
            NodeContract::inert("debug-settings-vec:field"),
            NodeContract::inert("debug-settings:changed-only:checkbox"),
            NodeContract::inert("debug-settings:color-swatch"),
            NodeContract::inert("debug-settings:edit:bool:checkbox"),
            NodeContract::inert("debug-settings:field"),
            // The editor's buttons are the preferences footer's, named for it.
            NodeContract::inert("preferences:button:debug-settings-copy-name"),
            NodeContract::inert("preferences:button:debug-settings-reset"),
        ],
    },
    ElementContract {
        element: "emoji-picker",
        nodes: &[
            NodeContract::inert("emoji-picker:field"),
            NodeContract::new(
                "emoji-picker-tabs:tab-strip",
                &[
                    Row::emits(Gesture::PrimaryClick, &["select-tab"]),
                    Row::emits(Gesture::DoubleClick, &["select-tab"]),
                    Row::emits(Gesture::ArrowUp, &["select-tab"]),
                    Row::emits(Gesture::ArrowDown, &["select-tab"]),
                    Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                    Row::emits(Gesture::ArrowRight, &["select-tab"]),
                ],
            ),
            // The grid's viewport takes focus so the wheel scrolls it; the
            // cells are what a press picks, and they need the picker's target.
            NodeContract::inert("emoji-picker-viewport"),
        ],
    },
    ElementContract {
        element: "experience-permission-toast",
        nodes: &[
            NodeContract::new(
                "experience-permission-action:Block Experience",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block-experience"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["block-experience", "block-experience"],
                    ),
                    Row::emits(Gesture::Enter, &["block-experience"]),
                    Row::emits(Gesture::Space, &["block-experience"]),
                ],
            ),
            NodeContract::new(
                "experience-permission-action:Block Object",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block-object"]),
                    Row::emits(Gesture::DoubleClick, &["block-object", "block-object"]),
                    Row::emits(Gesture::Enter, &["block-object"]),
                    Row::emits(Gesture::Space, &["block-object"]),
                ],
            ),
            NodeContract::new(
                "experience-permission-action:No",
                &[
                    Row::emits(Gesture::PrimaryClick, &["no"]),
                    Row::emits(Gesture::DoubleClick, &["no", "no"]),
                    Row::emits(Gesture::Enter, &["no"]),
                    Row::emits(Gesture::Space, &["no"]),
                ],
            ),
            NodeContract::new(
                "experience-permission-action:Yes",
                &[
                    Row::emits(Gesture::PrimaryClick, &["yes"]),
                    Row::emits(Gesture::DoubleClick, &["yes", "yes"]),
                    Row::emits(Gesture::Enter, &["yes"]),
                    Row::emits(Gesture::Space, &["yes"]),
                ],
            ),
            NodeContract::new(
                "experience-permission-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "experiences-floater",
        // The live window's buttons. They act on the window's own state — a
        // Profile opens the selected experience's page, a Forget sends the
        // permission change, a Refresh re-asks every list — through resources
        // an element host has none of, so here each press is a no-op: no
        // `UiAction` (they never raised one live either), and no failed
        // observer.
        nodes: &[
            NodeContract::inert("experiences-action:refresh"),
            NodeContract::inert("experiences-action:profile"),
            NodeContract::inert("experiences-action:forget"),
            NodeContract::new(
                "experiences-tabs:tab-strip",
                &[
                    Row::emits(Gesture::ArrowUp, &["select-tab"]),
                    Row::emits(Gesture::ArrowDown, &["select-tab"]),
                    Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                    Row::emits(Gesture::ArrowRight, &["select-tab"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "friendship-offer-toast",
        nodes: &[
            NodeContract::new(
                "offer-invite-action:Accept",
                &[
                    Row::emits(Gesture::PrimaryClick, &["accept"]),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Decline",
                &[
                    Row::emits(Gesture::PrimaryClick, &["decline"]),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "group-invite-toast",
        nodes: &[
            NodeContract::new(
                "offer-invite-action:Decline",
                &[
                    Row::emits(Gesture::PrimaryClick, &["decline"]),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Join",
                &[
                    Row::emits(Gesture::PrimaryClick, &["accept"]),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "group-notice-toast",
        nodes: &[
            NodeContract::new(
                "group-notice-button:Group Chat",
                &[
                    Row::emits(Gesture::PrimaryClick, &["chat"]),
                    Row::emits(Gesture::DoubleClick, &["chat", "chat"]),
                    Row::emits(Gesture::Enter, &["chat"]),
                    Row::emits(Gesture::Space, &["chat"]),
                ],
            ),
            NodeContract::new(
                "group-notice-button:Group Notices",
                &[
                    Row::emits(Gesture::PrimaryClick, &["notices"]),
                    Row::emits(Gesture::DoubleClick, &["notices", "notices"]),
                    Row::emits(Gesture::Enter, &["notices"]),
                    Row::emits(Gesture::Space, &["notices"]),
                ],
            ),
            NodeContract::new(
                "group-notice-button:OK",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ok"]),
                    Row::emits(Gesture::DoubleClick, &["ok", "ok"]),
                    Row::emits(Gesture::Enter, &["ok"]),
                    Row::emits(Gesture::Space, &["ok"]),
                ],
            ),
            NodeContract::new(
                "group-notice-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "inventory-offer-toast",
        nodes: &[
            NodeContract::new(
                "offer-invite-action:Accept",
                &[
                    Row::emits(Gesture::PrimaryClick, &["accept"]),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Block",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block"]),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Decline",
                &[
                    Row::emits(Gesture::PrimaryClick, &["decline"]),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "load-url-toast",
        nodes: &[
            NodeContract::new(
                "load-url-action:Block",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block"]),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "load-url-action:Ignore",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ignore"]),
                    Row::emits(Gesture::DoubleClick, &["ignore", "ignore"]),
                    Row::emits(Gesture::Enter, &["ignore"]),
                    Row::emits(Gesture::Space, &["ignore"]),
                ],
            ),
            NodeContract::new(
                "load-url-action:Load",
                &[
                    Row::emits(Gesture::PrimaryClick, &["load"]),
                    Row::emits(Gesture::DoubleClick, &["load", "load"]),
                    Row::emits(Gesture::Enter, &["load"]),
                    Row::emits(Gesture::Space, &["load"]),
                ],
            ),
            NodeContract::new(
                "load-url-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "local-chat-input",
        nodes: &[
            NodeContract::inert("local-chat-input:field"),
            NodeContract::new(
                "local-chat-volume-button",
                &[
                    // Both gestures open the whisper/say/shout panel, and both
                    // used to be pinned `known_broken` against
                    // `viewer-chat-volume-dropdown-opens-off-screen`: the panel
                    // was hand-positioned at `bottom: 100%` with no fallback
                    // placement and no window margin, so it laid out above the
                    // top edge of the window and three of its four rows were
                    // unreachable. It is a `Popover` now, so the rows are clean
                    // — and a clean layout here is the whole assertion, since
                    // opening a drop-down emits no action.
                    Row::emits(Gesture::PrimaryClick, &[]),
                    Row::emits(Gesture::DragAcross, &[]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "menu-bar",
        nodes: &[
            NodeContract::inert("menu-button:menu-fixture-avatar"),
            NodeContract::inert("menu-button:menu-fixture-world"),
        ],
    },
    ElementContract {
        element: "notecard-editor",
        nodes: &[
            NodeContract::inert("notecard-body:field"),
            NodeContract::inert("notecard-save"),
        ],
    },
    ElementContract {
        // A no-modify notecard's body is a focus stop like any other field: it
        // refuses the edits that would change it, but it is still clicked into,
        // selected across and copied out of, so `Tab` reaches it.
        element: "notecard-reader",
        nodes: &[NodeContract::inert("notecard-body:field")],
    },
    ElementContract {
        element: "notification-overflow",
        nodes: &TOAST_NODES_WITH_OVERFLOW,
    },
    ElementContract {
        element: "notification-toast",
        nodes: &TOAST_NODES,
    },
    ElementContract {
        element: "panel",
        nodes: &[
            NodeContract::new(
                "button:cancel",
                &[
                    Row::emits(Gesture::PrimaryClick, &["cancel"]),
                    Row::emits(Gesture::DoubleClick, &["cancel", "cancel"]),
                    Row::emits(Gesture::Enter, &["cancel"]),
                    Row::emits(Gesture::Space, &["cancel"]),
                ],
            ),
            NodeContract::new(
                "button:discard",
                &[
                    Row::emits(Gesture::PrimaryClick, &["discard"]),
                    Row::emits(Gesture::DoubleClick, &["discard", "discard"]),
                    Row::emits(Gesture::Enter, &["discard"]),
                    Row::emits(Gesture::Space, &["discard"]),
                ],
            ),
            NodeContract::new(
                "button:save",
                &[
                    Row::emits(Gesture::PrimaryClick, &["save"]),
                    Row::emits(Gesture::DoubleClick, &["save", "save"]),
                    Row::emits(Gesture::Enter, &["save"]),
                    Row::emits(Gesture::Space, &["save"]),
                ],
            ),
        ],
    },
    // The parcel audio bar: the live bar's own glyph buttons, each raising its
    // action however it is pressed.
    ElementContract {
        element: "parcel-audio-bar",
        nodes: &[
            NodeContract::new(
                "parcel-audio-button:mute-toggle",
                &[
                    Row::emits(Gesture::PrimaryClick, &["mute-toggle"]),
                    Row::emits(Gesture::DoubleClick, &["mute-toggle", "mute-toggle"]),
                    Row::emits(Gesture::Enter, &["mute-toggle"]),
                    Row::emits(Gesture::Space, &["mute-toggle"]),
                ],
            ),
            NodeContract::new(
                "parcel-audio-button:play-stop",
                &[
                    Row::emits(Gesture::PrimaryClick, &["play-stop"]),
                    Row::emits(Gesture::DoubleClick, &["play-stop", "play-stop"]),
                    Row::emits(Gesture::Enter, &["play-stop"]),
                    Row::emits(Gesture::Space, &["play-stop"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "phototools",
        nodes: &[
            NodeContract::inert("phototools-aim-moon-azimuth:slider"),
            NodeContract::inert("phototools-aim-moon-elevation:slider"),
            NodeContract::inert("phototools-aim-moon:trackball"),
            NodeContract::inert("phototools-aim-sun-azimuth:slider"),
            NodeContract::inert("phototools-aim-sun-elevation:slider"),
            NodeContract::inert("phototools-aim-sun:trackball"),
            NodeContract::inert("phototools-preset-day-cycle:combo"),
            NodeContract::inert("phototools-preset-day-cycle:next"),
            NodeContract::inert("phototools-preset-day-cycle:prev"),
            NodeContract::inert("phototools-preset-sky:combo"),
            NodeContract::inert("phototools-preset-sky:next"),
            NodeContract::inert("phototools-preset-sky:prev"),
            NodeContract::inert("phototools-preset-water:combo"),
            NodeContract::inert("phototools-preset-water:next"),
            NodeContract::inert("phototools-preset-water:prev"),
            NodeContract::new(
                "phototools-tabs:tab-strip",
                &[
                    Row::emits(Gesture::ArrowUp, &["select-tab"]),
                    Row::emits(Gesture::ArrowDown, &["select-tab"]),
                    Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                    Row::emits(Gesture::ArrowRight, &["select-tab"]),
                ],
            ),
            NodeContract::inert("phototools:button:env-shared:phototools-env-shared"),
            NodeContract::inert("phototools:button:env-time:quick-prefs-time-midday"),
            NodeContract::inert("phototools:button:env-time:quick-prefs-time-midnight"),
            NodeContract::inert("phototools:button:env-time:quick-prefs-time-sunrise"),
            NodeContract::inert("phototools:button:env-time:quick-prefs-time-sunset"),
            NodeContract::inert("phototools:button:personal-lighting:phototools-personal-lighting"),
            NodeContract::inert("phototools:env-group:combo"),
            NodeContract::inert("phototools:mirror-resolution:combo"),
            NodeContract::inert("phototools:mirror-update-rate:combo"),
            NodeContract::inert("phototools:mirrors:checkbox"),
            NodeContract::new(PHOTO_CHECK, &PHOTO_ROWS),
        ],
    },
    ElementContract {
        element: "preferences",
        nodes: &[
            NodeContract::new(
                "preferences-tabs:tab-strip",
                &[
                    Row::emits(Gesture::ArrowUp, &["select-tab"]),
                    Row::emits(Gesture::ArrowDown, &["select-tab"]),
                    Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                    Row::emits(Gesture::ArrowRight, &["select-tab"]),
                ],
            ),
            NodeContract::inert("preferences-search:field"),
            NodeContract::inert("preferences-row-afk-timeout:combo"),
            NodeContract::inert("preferences-row-language:combo"),
            NodeContract::inert("preferences-row-maturity:combo"),
            NodeContract::inert("preferences-row-quit-after-afk:combo"),
            NodeContract::inert("preferences-row-start-location:combo"),
            // The specimen's footer carries no behaviour: OK and Cancel act on
            // the live shell's `PreferencesUi`, which a specimen never is.
            NodeContract::inert("preferences:button:preferences-cancel"),
            NodeContract::inert("preferences:button:preferences-ok"),
            NodeContract::inert("preferences:button:preferences-ui-scale-reset"),
            NodeContract::new(PREF_CHECK, &PREF_CHECK_ROWS),
            NodeContract::inert("preferences-row-own-name-tag:checkbox"),
            NodeContract::inert("preferences-row-name-tag-display-names:checkbox"),
            NodeContract::inert("preferences-row-name-tag-usernames:checkbox"),
            NodeContract::inert("preferences-row-name-tag-group-titles:checkbox"),
            NodeContract::inert("preferences-row-name-tag-typing:checkbox"),
            NodeContract::inert("preferences-row-name-tag-distance:checkbox"),
            NodeContract::inert("preferences-row-name-tag-friend-color:checkbox"),
            NodeContract::inert("preferences-row-name-tag-color-by-distance:checkbox"),
            NodeContract::inert("preferences-row-name-tag-complexity:checkbox"),
            NodeContract::inert("preferences-row-name-tag-own-complexity:checkbox"),
            NodeContract::inert("preferences-row-name-tag-complexity-limited-only:checkbox"),
            NodeContract::inert("preferences-row-sit-on-away:checkbox"),
        ],
    },
    ElementContract {
        element: "quick-preferences",
        nodes: &[
            NodeContract::inert("quick-prefs-env-group:combo"),
            NodeContract::inert("quick-prefs-env-time:combo"),
            NodeContract::inert("quick-prefs-preset-day-cycle:combo"),
            NodeContract::inert("quick-prefs-preset-day-cycle:next"),
            NodeContract::inert("quick-prefs-preset-day-cycle:prev"),
            NodeContract::inert("quick-prefs-preset-sky:combo"),
            NodeContract::inert("quick-prefs-preset-sky:next"),
            NodeContract::inert("quick-prefs-preset-sky:prev"),
            NodeContract::inert("quick-prefs-preset-water:combo"),
            NodeContract::inert("quick-prefs-preset-water:next"),
            NodeContract::inert("quick-prefs-preset-water:prev"),
            NodeContract::inert("quick-prefs:checkbox"),
            NodeContract::inert("quick-prefs:quality:combo"),
        ],
    },
    ElementContract {
        // The live radar's controls. The filter and range fields and the
        // range-limit box feed the view systems and settings a host does not
        // run; the table's own selection gestures raise no `UiAction`.
        element: "radar",
        nodes: &[
            NodeContract::inert("radar-filter:field"),
            NodeContract::inert("radar-limit:checkbox"),
            NodeContract::inert("radar-range:field"),
            NodeContract::inert("radar:table-viewport"),
        ],
    },
    ElementContract {
        element: "radio-group-column",
        nodes: &[NodeContract::new(
            "radio-group-column:radio-group",
            &[
                Row::emits(Gesture::ArrowUp, &["select-radio"]),
                Row::emits(Gesture::ArrowDown, &["select-radio"]),
                Row::emits(Gesture::ArrowLeft, &["select-radio"]),
                Row::emits(Gesture::ArrowRight, &["select-radio"]),
            ],
        )],
    },
    ElementContract {
        element: "radio-group-row",
        nodes: &[NodeContract::new(
            "radio-group-row:radio-group",
            &[
                Row::emits(Gesture::ArrowUp, &["select-radio"]),
                Row::emits(Gesture::ArrowDown, &["select-radio"]),
                Row::emits(Gesture::ArrowLeft, &["select-radio"]),
                Row::emits(Gesture::ArrowRight, &["select-radio"]),
            ],
        )],
    },
    ElementContract {
        element: "sun-moon-trackball",
        nodes: &[
            NodeContract::new(
                SUN_TRACKBALL,
                &[
                    Row::leaves(Gesture::PrimaryClick, SUN_CLICK_AIMS_UP),
                    Row::leaves(Gesture::DoubleClick, SUN_CLICK_AIMS_UP),
                    Row::leaves(Gesture::DragAcross, SUN_DRAG_AIMS),
                    Row::leaves(Gesture::ArrowUp, SUN_ARROW_STEPS),
                    Row::leaves(Gesture::ArrowDown, SUN_ARROW_STEPS),
                    Row::leaves(Gesture::ArrowLeft, SUN_ARROW_STEPS),
                    Row::leaves(Gesture::ArrowRight, SUN_ARROW_STEPS),
                ],
            ),
            NodeContract::new(
                MOON_TRACKBALL,
                &[
                    Row::leaves(Gesture::PrimaryClick, MOON_CLICK_AIMS_UP),
                    Row::leaves(Gesture::DoubleClick, MOON_CLICK_AIMS_UP),
                    Row::leaves(Gesture::DragAcross, MOON_DRAG_AIMS),
                    Row::leaves(Gesture::ArrowUp, MOON_ARROW_STEPS),
                    Row::leaves(Gesture::ArrowDown, MOON_ARROW_STEPS),
                    Row::leaves(Gesture::ArrowLeft, MOON_ARROW_STEPS),
                    Row::leaves(Gesture::ArrowRight, MOON_ARROW_STEPS),
                ],
            ),
        ],
    },
    ElementContract {
        element: "script-dialog-textbox-toast",
        nodes: &[
            NodeContract::new(
                "script-dialog-action:Block",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block"]),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-action:Ignore",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ignore"]),
                    Row::emits(Gesture::DoubleClick, &["ignore", "ignore"]),
                    Row::emits(Gesture::Enter, &["ignore"]),
                    Row::emits(Gesture::Space, &["ignore"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-action:Submit",
                &[
                    Row::emits(Gesture::PrimaryClick, &["submit"]),
                    Row::emits(Gesture::DoubleClick, &["submit", "submit"]),
                    Row::emits(Gesture::Enter, &["submit"]),
                    Row::emits(Gesture::Space, &["submit"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
            NodeContract::inert("script-dialog-textbox:field"),
        ],
    },
    ElementContract {
        element: "script-dialog-toast",
        nodes: &[
            NodeContract::new(
                "script-dialog-action:Block",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block"]),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-action:Ignore",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ignore"]),
                    Row::emits(Gesture::DoubleClick, &["ignore", "ignore"]),
                    Row::emits(Gesture::Enter, &["ignore"]),
                    Row::emits(Gesture::Space, &["ignore"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Buy",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Cancel",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Gift",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Info",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Redeliver",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "script-editor",
        nodes: &[
            NodeContract::inert("script-body:field"),
            NodeContract::inert("script-save"),
            // The Running box is the live window's: it ticks and clears (the
            // run state it feeds is the window's, and a specimen sits in none).
            NodeContract::new(SCRIPT_RUNNING, &SCRIPT_RUNNING_ROWS),
        ],
    },
    ElementContract {
        element: "script-permission-caution-toast",
        nodes: &[
            NodeContract::new(
                "script-permission-action:Allow access",
                &[
                    Row::emits(Gesture::PrimaryClick, &["grant"]),
                    Row::emits(Gesture::DoubleClick, &["grant", "grant"]),
                    Row::emits(Gesture::Enter, &["grant"]),
                    Row::emits(Gesture::Space, &["grant"]),
                ],
            ),
            NodeContract::new(
                "script-permission-action:Deny",
                &[
                    Row::emits(Gesture::PrimaryClick, &["deny"]),
                    Row::emits(Gesture::DoubleClick, &["deny", "deny"]),
                    Row::emits(Gesture::Enter, &["deny"]),
                    Row::emits(Gesture::Space, &["deny"]),
                ],
            ),
            NodeContract::new(
                "script-permission-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "script-permission-toast",
        nodes: &[
            NodeContract::new(
                "script-permission-action:Block",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block"]),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "script-permission-action:No",
                &[
                    Row::emits(Gesture::PrimaryClick, &["deny"]),
                    Row::emits(Gesture::DoubleClick, &["deny", "deny"]),
                    Row::emits(Gesture::Enter, &["deny"]),
                    Row::emits(Gesture::Space, &["deny"]),
                ],
            ),
            NodeContract::new(
                "script-permission-action:Yes",
                &[
                    Row::emits(Gesture::PrimaryClick, &["grant"]),
                    Row::emits(Gesture::DoubleClick, &["grant", "grant"]),
                    Row::emits(Gesture::Enter, &["grant"]),
                    Row::emits(Gesture::Space, &["grant"]),
                ],
            ),
            NodeContract::new(
                "script-permission-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "search-field",
        nodes: &[NodeContract::inert("search-field:field")],
    },
    ElementContract {
        element: "tabs-bottom",
        nodes: &[NodeContract::new(
            "tabs-bottom:tab-strip",
            &[
                Row::emits(Gesture::PrimaryClick, &["select-tab"]),
                Row::emits(Gesture::DoubleClick, &["select-tab"]),
                Row::emits(Gesture::ArrowUp, &["select-tab"]),
                Row::emits(Gesture::ArrowDown, &["select-tab"]),
                Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                Row::emits(Gesture::ArrowRight, &["select-tab"]),
            ],
        )],
    },
    ElementContract {
        element: "tabs-leading",
        nodes: &[NodeContract::new(
            "tabs-leading:tab-strip",
            &[
                Row::emits(Gesture::ArrowUp, &["select-tab"]),
                Row::emits(Gesture::ArrowDown, &["select-tab"]),
                Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                Row::emits(Gesture::ArrowRight, &["select-tab"]),
            ],
        )],
    },
    ElementContract {
        element: "tabs-top",
        nodes: &[NodeContract::new(
            "tabs-top:tab-strip",
            &[
                Row::emits(Gesture::PrimaryClick, &["select-tab"]),
                Row::emits(Gesture::DoubleClick, &["select-tab"]),
                Row::emits(Gesture::ArrowUp, &["select-tab"]),
                Row::emits(Gesture::ArrowDown, &["select-tab"]),
                Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                Row::emits(Gesture::ArrowRight, &["select-tab"]),
            ],
        )],
    },
    ElementContract {
        element: "tabs-trailing",
        nodes: &[NodeContract::new(
            "tabs-trailing:tab-strip",
            &[
                Row::emits(Gesture::ArrowUp, &["select-tab"]),
                Row::emits(Gesture::ArrowDown, &["select-tab"]),
                Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                Row::emits(Gesture::ArrowRight, &["select-tab"]),
            ],
        )],
    },
    ElementContract {
        element: "teleport-offer-toast",
        nodes: &[
            NodeContract::new(
                "offer-invite-action:Decline",
                &[
                    Row::emits(Gesture::PrimaryClick, &["decline"]),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Teleport",
                &[
                    Row::emits(Gesture::PrimaryClick, &["accept"]),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "text-editor",
        nodes: &[NodeContract::inert("text-editor")],
    },
    ElementContract {
        element: "text-input-float",
        nodes: &[NodeContract::inert("text-input-float:field")],
    },
    ElementContract {
        element: "text-input-integer",
        nodes: &[NodeContract::inert("text-input-integer:field")],
    },
    ElementContract {
        element: "text-input-line",
        nodes: &[NodeContract::new(
            "text-input-line:field",
            &[Row::leaves(Gesture::PrimaryClick, CLICK_TAKES_THE_CARET)],
        )],
    },
    ElementContract {
        element: "text-input-multiline",
        nodes: &[NodeContract::inert("text-input-multiline:field")],
    },
    ElementContract {
        element: "text-input-read-only",
        nodes: &[NodeContract::new(
            "text-input-read-only:field",
            &[Row::leaves(
                Gesture::PrimaryClick,
                CLICK_TAKES_THE_CARET_READ_ONLY,
            )],
        )],
    },
    ElementContract {
        element: "text-input-unsigned",
        nodes: &[NodeContract::inert("text-input-unsigned:field")],
    },
    ElementContract {
        element: "worldmap",
        nodes: &[
            NodeContract::new(
                "worldmap-button:copy-slurl",
                &[
                    Row::emits(Gesture::PrimaryClick, &["copy-slurl"]),
                    Row::emits(Gesture::DoubleClick, &["copy-slurl", "copy-slurl"]),
                    Row::emits(Gesture::Enter, &["copy-slurl"]),
                    Row::emits(Gesture::Space, &["copy-slurl"]),
                ],
            ),
            NodeContract::new(
                "worldmap-button:teleport-selected",
                &[
                    Row::emits(Gesture::PrimaryClick, &["teleport-selected"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["teleport-selected", "teleport-selected"],
                    ),
                    Row::emits(Gesture::Enter, &["teleport-selected"]),
                    Row::emits(Gesture::Space, &["teleport-selected"]),
                ],
            ),
            // The layer filters: a primary click on the checkbox, or `Enter` /
            // `Space` on the focused one, flips its layer.
            NodeContract::new(
                "worldmap-filter:toggle-adult-events",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-adult-events"]),
                    Row::emits(Gesture::Enter, &["toggle-adult-events"]),
                    Row::emits(Gesture::Space, &["toggle-adult-events"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["toggle-adult-events", "toggle-adult-events"],
                    ),
                ],
            ),
            NodeContract::new(
                "worldmap-filter:toggle-events",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-events"]),
                    Row::emits(Gesture::Enter, &["toggle-events"]),
                    Row::emits(Gesture::Space, &["toggle-events"]),
                    Row::emits(Gesture::DoubleClick, &["toggle-events", "toggle-events"]),
                ],
            ),
            NodeContract::new(
                "worldmap-filter:toggle-infohubs",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-infohubs"]),
                    Row::emits(Gesture::Enter, &["toggle-infohubs"]),
                    Row::emits(Gesture::Space, &["toggle-infohubs"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["toggle-infohubs", "toggle-infohubs"],
                    ),
                ],
            ),
            NodeContract::new(
                "worldmap-filter:toggle-land-sale",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-land-sale"]),
                    Row::emits(Gesture::Enter, &["toggle-land-sale"]),
                    Row::emits(Gesture::Space, &["toggle-land-sale"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["toggle-land-sale", "toggle-land-sale"],
                    ),
                ],
            ),
            NodeContract::new(
                "worldmap-filter:toggle-mature-events",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-mature-events"]),
                    Row::emits(Gesture::Enter, &["toggle-mature-events"]),
                    Row::emits(Gesture::Space, &["toggle-mature-events"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["toggle-mature-events", "toggle-mature-events"],
                    ),
                ],
            ),
            NodeContract::new(
                "worldmap-filter:toggle-people",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-people"]),
                    Row::emits(Gesture::Enter, &["toggle-people"]),
                    Row::emits(Gesture::Space, &["toggle-people"]),
                    Row::emits(Gesture::DoubleClick, &["toggle-people", "toggle-people"]),
                ],
            ),
            NodeContract::new(
                "worldmap-filter:toggle-region-names",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-region-names"]),
                    Row::emits(Gesture::Enter, &["toggle-region-names"]),
                    Row::emits(Gesture::Space, &["toggle-region-names"]),
                    Row::emits(
                        Gesture::DoubleClick,
                        &["toggle-region-names", "toggle-region-names"],
                    ),
                ],
            ),
            // The X / Y / Z fields and the search field: typing edits them,
            // and the map reads them back each frame.
            NodeContract::inert("worldmap:field"),
        ],
    },
];
