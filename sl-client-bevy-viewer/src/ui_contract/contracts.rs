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
//! # What the first sweep found, and what is pinned rather than fixed
//!
//! Two groups of rows describe behaviour that is *not* what the viewer wants.
//! They are pinned exactly, so the correction has to pass through this file:
//!
//! - **`Row::emits_wrongly(…, "viewer-widget-any-mouse-button-activates")`** —
//!   122 rows. Every `bevy_ui_widgets` `Button` activates on the middle and
//!   secondary pointer buttons as readily as on the primary one, because
//!   upstream's observers never read `click.button` and `Activate` carries no
//!   button for a downstream observer to read either. Fixing it is a change to
//!   the `taladar/bevy` fork, not to this workspace.
//! - The **arrow keys on a tab strip and a radio group**, which move the
//!   selection on all four arrows regardless of the widget's orientation. That
//!   one is upstream *by design* — `bevy_ui_widgets`' `radio.rs` reads
//!   `ArrowUp | ArrowLeft` as "previous" and the other two as "next" without
//!   consulting a layout, where its `menu.rs` does consult `MenuLayout` — so it
//!   is recorded as ordinary `Row::emits` rather than flagged. It is a parity
//!   gap with the reference viewer, not a defect.
//!
//! A `Row::emits` row is exact in both directions, so any of these becoming
//! right fails the sweep and the table is corrected in the same commit. That is
//! the point of pinning rather than tolerating.

use super::{ElementContract, Gesture, NodeContract, Probe, Row};
use bevy::input_focus::InputFocus;
use bevy::prelude::{App, Name};

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

/// Every element's contract, keyed by `UiElement::id`.
pub(crate) const CONTRACTS: &[ElementContract] = &[
    ElementContract {
        element: "bottom-toolbar",
        nodes: &[
            NodeContract::new(
                "bottom-toolbar-button:toggle-appearance",
                &[
                    Row::emits(Gesture::PrimaryClick, &["toggle-appearance"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["toggle-appearance"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["toggle-appearance"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["toggle-inventory"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["toggle-inventory"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["select-radio"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["select-radio"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["select-radio"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["select-radio"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                Row::emits_wrongly(
                    Gesture::MiddleClick,
                    &["save"],
                    "viewer-widget-any-mouse-button-activates",
                ),
                Row::emits_wrongly(
                    Gesture::SecondaryClick,
                    &["save"],
                    "viewer-widget-any-mouse-button-activates",
                ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["cancel"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["cancel"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["cancel", "cancel"]),
                    Row::emits(Gesture::Enter, &["cancel"]),
                    Row::emits(Gesture::Space, &["cancel"]),
                ],
            ),
            NodeContract::new(
                "button:discard",
                &[
                    Row::emits(Gesture::PrimaryClick, &["discard"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["discard"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["discard"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["discard", "discard"]),
                    Row::emits(Gesture::Enter, &["discard"]),
                    Row::emits(Gesture::Space, &["discard"]),
                ],
            ),
            NodeContract::new(
                "button:save",
                &[
                    Row::emits(Gesture::PrimaryClick, &["save"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["save"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["save"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["block-experience"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["block-experience"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["block-object"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["block-object"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["block-object", "block-object"]),
                    Row::emits(Gesture::Enter, &["block-object"]),
                    Row::emits(Gesture::Space, &["block-object"]),
                ],
            ),
            NodeContract::new(
                "experience-permission-action:No",
                &[
                    Row::emits(Gesture::PrimaryClick, &["no"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["no"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["no"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["no", "no"]),
                    Row::emits(Gesture::Enter, &["no"]),
                    Row::emits(Gesture::Space, &["no"]),
                ],
            ),
            NodeContract::new(
                "experience-permission-action:Yes",
                &[
                    Row::emits(Gesture::PrimaryClick, &["yes"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["yes"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["yes"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["yes", "yes"]),
                    Row::emits(Gesture::Enter, &["yes"]),
                    Row::emits(Gesture::Space, &["yes"]),
                ],
            ),
            NodeContract::new(
                "experience-permission-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["close", "close"]),
                    Row::emits(Gesture::Enter, &["close"]),
                    Row::emits(Gesture::Space, &["close"]),
                ],
            ),
        ],
    },
    ElementContract {
        element: "experiences-floater",
        nodes: &[NodeContract::new(
            "experiences-button",
            &[
                Row::emits(Gesture::PrimaryClick, &["forget"]),
                Row::emits_wrongly(
                    Gesture::MiddleClick,
                    &["forget"],
                    "viewer-widget-any-mouse-button-activates",
                ),
                Row::emits_wrongly(
                    Gesture::SecondaryClick,
                    &["forget"],
                    "viewer-widget-any-mouse-button-activates",
                ),
                Row::emits(Gesture::DoubleClick, &["forget", "forget"]),
                Row::emits(Gesture::Enter, &["forget"]),
                Row::emits(Gesture::Space, &["forget"]),
            ],
        )],
    },
    ElementContract {
        element: "friendship-offer-toast",
        nodes: &[
            NodeContract::new(
                "offer-invite-action:Accept",
                &[
                    Row::emits(Gesture::PrimaryClick, &["accept"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Decline",
                &[
                    Row::emits(Gesture::PrimaryClick, &["decline"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Join",
                &[
                    Row::emits(Gesture::PrimaryClick, &["accept"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["chat"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["chat"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["chat", "chat"]),
                    Row::emits(Gesture::Enter, &["chat"]),
                    Row::emits(Gesture::Space, &["chat"]),
                ],
            ),
            NodeContract::new(
                "group-notice-button:Group Notices",
                &[
                    Row::emits(Gesture::PrimaryClick, &["notices"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["notices"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["notices"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["notices", "notices"]),
                    Row::emits(Gesture::Enter, &["notices"]),
                    Row::emits(Gesture::Space, &["notices"]),
                ],
            ),
            NodeContract::new(
                "group-notice-button:OK",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ok"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["ok"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["ok"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["ok", "ok"]),
                    Row::emits(Gesture::Enter, &["ok"]),
                    Row::emits(Gesture::Space, &["ok"]),
                ],
            ),
            NodeContract::new(
                "group-notice-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Block",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Decline",
                &[
                    Row::emits(Gesture::PrimaryClick, &["decline"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "load-url-action:Ignore",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ignore"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["ignore"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["ignore"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["ignore", "ignore"]),
                    Row::emits(Gesture::Enter, &["ignore"]),
                    Row::emits(Gesture::Space, &["ignore"]),
                ],
            ),
            NodeContract::new(
                "load-url-action:Load",
                &[
                    Row::emits(Gesture::PrimaryClick, &["load"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["load"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["load"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["load", "load"]),
                    Row::emits(Gesture::Enter, &["load"]),
                    Row::emits(Gesture::Space, &["load"]),
                ],
            ),
            NodeContract::new(
                "load-url-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["Cancel"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["Cancel"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["Cancel", "Cancel"]),
                    Row::emits(Gesture::Enter, &["Cancel"]),
                    Row::emits(Gesture::Space, &["Cancel"]),
                ],
            ),
            NodeContract::new(
                "toast-button:OK",
                &[
                    Row::emits(Gesture::PrimaryClick, &["OK"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["OK"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["OK"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["OK", "OK"]),
                    Row::emits(Gesture::Enter, &["OK"]),
                    Row::emits(Gesture::Space, &["OK"]),
                ],
            ),
            NodeContract::new(
                "toast-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["cancel"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["cancel"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["cancel", "cancel"]),
                    Row::emits(Gesture::Enter, &["cancel"]),
                    Row::emits(Gesture::Space, &["cancel"]),
                ],
            ),
            NodeContract::new(
                "button:discard",
                &[
                    Row::emits(Gesture::PrimaryClick, &["discard"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["discard"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["discard"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["discard", "discard"]),
                    Row::emits(Gesture::Enter, &["discard"]),
                    Row::emits(Gesture::Space, &["discard"]),
                ],
            ),
            NodeContract::new(
                "button:save",
                &[
                    Row::emits(Gesture::PrimaryClick, &["save"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["save"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["save"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
        element: "script-dialog-textbox-toast",
        nodes: &[
            NodeContract::new(
                "script-dialog-action:Block",
                &[
                    Row::emits(Gesture::PrimaryClick, &["block"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-action:Ignore",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ignore"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["ignore"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["ignore"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["ignore", "ignore"]),
                    Row::emits(Gesture::Enter, &["ignore"]),
                    Row::emits(Gesture::Space, &["ignore"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-action:Submit",
                &[
                    Row::emits(Gesture::PrimaryClick, &["submit"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["submit"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["submit"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["submit", "submit"]),
                    Row::emits(Gesture::Enter, &["submit"]),
                    Row::emits(Gesture::Space, &["submit"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-action:Ignore",
                &[
                    Row::emits(Gesture::PrimaryClick, &["ignore"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["ignore"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["ignore"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["ignore", "ignore"]),
                    Row::emits(Gesture::Enter, &["ignore"]),
                    Row::emits(Gesture::Space, &["ignore"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Buy",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Cancel",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Gift",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Info",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-button:Redeliver",
                &[
                    Row::emits(Gesture::PrimaryClick, &["button"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["button"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["button", "button"]),
                    Row::emits(Gesture::Enter, &["button"]),
                    Row::emits(Gesture::Space, &["button"]),
                ],
            ),
            NodeContract::new(
                "script-dialog-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["grant"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["grant"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["grant", "grant"]),
                    Row::emits(Gesture::Enter, &["grant"]),
                    Row::emits(Gesture::Space, &["grant"]),
                ],
            ),
            NodeContract::new(
                "script-permission-action:Deny",
                &[
                    Row::emits(Gesture::PrimaryClick, &["deny"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["deny"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["deny"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["deny", "deny"]),
                    Row::emits(Gesture::Enter, &["deny"]),
                    Row::emits(Gesture::Space, &["deny"]),
                ],
            ),
            NodeContract::new(
                "script-permission-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["block"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["block", "block"]),
                    Row::emits(Gesture::Enter, &["block"]),
                    Row::emits(Gesture::Space, &["block"]),
                ],
            ),
            NodeContract::new(
                "script-permission-action:No",
                &[
                    Row::emits(Gesture::PrimaryClick, &["deny"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["deny"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["deny"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["deny", "deny"]),
                    Row::emits(Gesture::Enter, &["deny"]),
                    Row::emits(Gesture::Space, &["deny"]),
                ],
            ),
            NodeContract::new(
                "script-permission-action:Yes",
                &[
                    Row::emits(Gesture::PrimaryClick, &["grant"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["grant"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["grant"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["grant", "grant"]),
                    Row::emits(Gesture::Enter, &["grant"]),
                    Row::emits(Gesture::Space, &["grant"]),
                ],
            ),
            NodeContract::new(
                "script-permission-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
                Row::emits_wrongly(
                    Gesture::MiddleClick,
                    &["select-tab"],
                    "viewer-widget-any-mouse-button-activates",
                ),
                Row::emits_wrongly(
                    Gesture::SecondaryClick,
                    &["select-tab"],
                    "viewer-widget-any-mouse-button-activates",
                ),
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
                Row::emits_wrongly(
                    Gesture::MiddleClick,
                    &["select-tab"],
                    "viewer-widget-any-mouse-button-activates",
                ),
                Row::emits_wrongly(
                    Gesture::SecondaryClick,
                    &["select-tab"],
                    "viewer-widget-any-mouse-button-activates",
                ),
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
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["decline"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["decline", "decline"]),
                    Row::emits(Gesture::Enter, &["decline"]),
                    Row::emits(Gesture::Space, &["decline"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-action:Teleport",
                &[
                    Row::emits(Gesture::PrimaryClick, &["accept"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["accept"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits(Gesture::DoubleClick, &["accept", "accept"]),
                    Row::emits(Gesture::Enter, &["accept"]),
                    Row::emits(Gesture::Space, &["accept"]),
                ],
            ),
            NodeContract::new(
                "offer-invite-close",
                &[
                    Row::emits(Gesture::PrimaryClick, &["close"]),
                    Row::emits_wrongly(
                        Gesture::MiddleClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
                    Row::emits_wrongly(
                        Gesture::SecondaryClick,
                        &["close"],
                        "viewer-widget-any-mouse-button-activates",
                    ),
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
