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
use bevy::prelude::{App, Name};
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
            NodeContract::inert("build-specimen-x:field"),
            NodeContract::inert("build-specimen-y:field"),
            NodeContract::inert("build-specimen-z:field"),
            NodeContract::new(
                "build-tool:radio-group",
                &[
                    Row::emits(Gesture::PrimaryClick, &["select-radio"]),
                    Row::emits(Gesture::DoubleClick, &["select-radio"]),
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
            NodeContract::inert("debug-settings-specimen-scope:combo"),
            NodeContract::inert("debug-settings-specimen-value:field"),
            NodeContract::inert("debug-settings-specimen:field"),
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
        nodes: &[
            NodeContract::new(
                "experiences-action:profile",
                &[
                    Row::emits(Gesture::PrimaryClick, &["profile"]),
                    Row::emits(Gesture::DoubleClick, &["profile", "profile"]),
                    Row::emits(Gesture::Enter, &["profile"]),
                    Row::emits(Gesture::Space, &["profile"]),
                ],
            ),
            NodeContract::new(
                "experiences-action:forget",
                &[
                    Row::emits(Gesture::PrimaryClick, &["forget"]),
                    Row::emits(Gesture::DoubleClick, &["forget", "forget"]),
                    Row::emits(Gesture::Enter, &["forget"]),
                    Row::emits(Gesture::Space, &["forget"]),
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
            NodeContract::inert("menu-button:Avatar"),
            NodeContract::inert("menu-button:World"),
        ],
    },
    ElementContract {
        element: "notecard-editor",
        nodes: &[
            NodeContract::inert("notecard-body:field"),
            NodeContract::inert("notecard-save"),
            NodeContract::inert("notecard-view-toggle"),
        ],
    },
    ElementContract {
        element: "notification-toast",
        nodes: &[
            NodeContract::new(
                "toast-button:Cancel",
                &[
                    Row::emits(Gesture::PrimaryClick, &["Cancel"]),
                    Row::emits(Gesture::DoubleClick, &["Cancel", "Cancel"]),
                    Row::emits(Gesture::Enter, &["Cancel"]),
                    Row::emits(Gesture::Space, &["Cancel"]),
                ],
            ),
            NodeContract::new(
                "toast-button:OK",
                &[
                    Row::emits(Gesture::PrimaryClick, &["OK"]),
                    Row::emits(Gesture::DoubleClick, &["OK", "OK"]),
                    Row::emits(Gesture::Enter, &["OK"]),
                    Row::emits(Gesture::Space, &["OK"]),
                ],
            ),
            NodeContract::new(
                "toast-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
            NodeContract::inert("toast-input:field"),
        ],
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
    ElementContract {
        element: "preferences",
        nodes: &[
            NodeContract::new(
                "preferences-specimen-tabs:tab-strip",
                &[
                    Row::emits(Gesture::ArrowUp, &["select-tab"]),
                    Row::emits(Gesture::ArrowDown, &["select-tab"]),
                    Row::emits(Gesture::ArrowLeft, &["select-tab"]),
                    Row::emits(Gesture::ArrowRight, &["select-tab"]),
                ],
            ),
            NodeContract::inert("preferences-specimen:field"),
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
        element: "text-input-unsigned",
        nodes: &[NodeContract::inert("text-input-unsigned:field")],
    },
];
