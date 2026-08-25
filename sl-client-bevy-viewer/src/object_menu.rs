//! The in-world **object** context / pie menu (`viewer-object-context-menu`):
//! the entry tree offered when a world object is the pick target, and the
//! dispatch of each entry.
//!
//! This is the *entries*, not the widget — the radial widget is
//! [`crate::pie_menu`], and this module declares the [`PieMenuDef`] tree plus
//! the systems that open and act on it, exactly as [`crate::avatar_menu`] does
//! for the avatar targets. The tree reproduces the reference viewer's
//! `menu_pie_object.xml` (shared by every skin — Vintage overrides none) at the
//! reference compass positions: reference slice order maps East → NorthEast → …
//! → SouthEast, the same slot convention the avatar pies established.
//!
//! # What is wired, and what is a disabled placeholder
//!
//! Most of the reference's object actions belong to features this viewer does
//! not have yet (buy / pay, wear / attach, the script and pathfinding tools).
//! Those sit **in their reference
//! compass positions but disabled**, gated on the never-supplied
//! [`UNIMPLEMENTED`] sentinel, so the menu's shape (the muscle memory) is laid
//! down now and each slice lights up when its feature lands — one `when` edit,
//! address unchanged. Wired for real:
//!
//! - **Touch** → [`Command::TouchObject`] on the picked prim, carrying the
//!   [`SurfaceInfo`] of the right-click's own ray hit (what a script reads back
//!   through `llDetectedTouch*`), enabled only for an object whose linkset
//!   handles touch ([`TARGET_TOUCHABLE`], the reference's `Object.EnableTouch`
//!   without its click-action refinements).
//! - **Sit Here / Stand Up** → [`Command::Sit`] on the picked prim (offset =
//!   the reference's `calcFocusOffset` in region axes, [`crate::sit_offset`],
//!   so a no-sit-target seat lands where the reference puts it) /
//!   [`Command::Stand`]. Declared as the reference declares them: an
//!   **autohide chain** sharing one compass position
//!   ([`PieContent::Chain`]), showing whichever applies to the current seated
//!   state.
//! - **Take**, **Take Copy** (both places the reference offers it), **Delete**,
//!   **Return** → [`Command::DerezObjects`] on the linkset root with the
//!   matching [`DeRezDestination`] (Objects folder / Objects folder copy /
//!   Trash / return-to-owner). The enable gates are deliberately simpler than
//!   the reference's full predicates: you-owner for take / delete / return,
//!   the copy permission bit for take-copy (see the condition constants).
//! - **Mute** → a guarded [`crate::world_api::RequestBlock`] for the object (id +
//!   name). The name is not in
//!   the object update stream, so opening the menu fires a
//!   [`Command::RequestObjectPropertiesFamily`] and the reply's name is held on
//!   the open target; a mute picked before the reply lands mutes by id with an
//!   empty name.
//! - **Derender ▸ Blacklist / Temporary** → a guarded
//!   [`crate::derender::RequestDerender`] for the picked **linkset root**:
//!   nothing goes on the wire, this viewer simply stops mirroring the object
//!   (permanently, through the per-avatar blacklist, or for the session). The
//!   object's name rides the same properties-family reply the Mute slice waits
//!   for.
//!
//! # Where we depart from the reference, on purpose
//!
//! - **No Buy / Take autohide chain at west (yet).** The reference chains a
//!   `Buy` slice with the `Take >` sub-pie at the west slot — Buy shows when
//!   the object is for sale. Buying is unimplemented, and a chain member that
//!   is a *sub-pie* is not (yet) expressible in [`crate::pie_menu`]; rather
//!   than grow the widget for a slice that would always be greyed, the west
//!   slot holds `Take >` plainly, and `Buy` keeps its *other* reference
//!   address (the More pie's south-east slice) as a greyed placeholder. When
//!   buying lands, the chain — and the sub-pie chain-member support it needs —
//!   is the deliberate follow-up edit.
//! - **Take's multi-selection slices are placeholders.** The reference take
//!   sub-pie switches its slices on single vs multi selection; there is no
//!   multi-object selection yet (`viewer-object-selection-core`), so the
//!   single-selection pair (Take Copy / Take) is wired and the multi-selection
//!   four are `UNIMPLEMENTED` placeholders at their reference positions. The
//!   reference's two separator slots stay empty.
//! - **`Attach HUD >` is an empty sub-pie.** The reference fills it (and the
//!   plain attach points of `Attach >`) at runtime from the attachment-point
//!   list; until wearing exists they stay as declared — an empty sub-pie
//!   renders as a disabled slice — while the *static* `Ext. Skeleton >` tree
//!   under `Attach >` is reproduced in full as placeholders, pinning the Bento
//!   point addresses.
//! - **Worn attachments do not open this pie.** The reference gives worn
//!   objects their own pies (`menu_pie_attachment_*`), and so do we: a pick
//!   whose [`ObjectPickSummary`] says `attachment` is routed by the shared
//!   resolver to [`crate::attachment_menu`] instead.
//! - **The muted-particle-source pie is deferred.** Its one slice (mute the
//!   particle owner) needs particle picking, which this renderer does not do
//!   yet; declaring the pie now would be dead data with no open path.
//!
//! # How a pick reaches here
//!
//! [`crate::avatar_menu`]'s right-click resolver owns the gesture (click vs
//! camera free-look drag) and the occlusion order (UI, then HUD, then world).
//! The world resolution is the GPU ID-buffer pick ([`crate::gpu_pick`]):
//! its depth test arbitrates avatar vs object nearest-wins in the pick view
//! itself, so an object standing in front of an avatar gets the object pie
//! and vice versa. An object-face hit is then **surface-refined** here
//! ([`ObjectPicker::pick_entity`], a single-entity ray test against exactly
//! the face the ID buffer named), walked up to its [`SceneObject`], and
//! resolved through [`ObjectState::pick_summary`] to the picked prim +
//! linkset root.
//!
//! Reference (Firestorm, read-only): `menu_pie_object.xml` (the compass
//! positions), `lltoolpie.cpp` (the pick), `llviewermenu.cpp` (the handlers).

use bevy::prelude::*;
use sl_client_bevy::{
    Command, DeRezDestination, FolderType, MuteType, SlAgentParcel, SlCommand, SlEvent,
    SlSessionEvent, TransactionId, Uuid, Vector,
};

use crate::avatar_menu::{SELF_SITTING, SELF_STANDING};
use crate::derender::RequestDerender;
use crate::inventory::InventoryModel;
use crate::menu::UNIMPLEMENTED;
use crate::objects::ObjectRayHit;
use crate::pie_menu::{Compass, OpenPieMenu, PieAction, PieContent, PieEntry, PieMenuDef};
use crate::ui_element::UiAction;
use crate::world_api::DerenderKind;
use crate::world_api::ObjectState;
use crate::world_api::RequestBlock;
use crate::world_api::SelfGroundSit;

/// The `element` the object pie attributes its [`UiAction`]s to.
pub(crate) const OBJECT_MENU_ELEMENT: &str = "object-menu";

// ---------------------------------------------------------------------------
// The condition vocabulary (see `crate::avatar_menu` for the shared names:
// `UNIMPLEMENTED`, `SELF_SITTING`, `SELF_STANDING`). Every name is a
// compile-time constant; the set that *holds* is built at open time from the
// picked object's flags and the seated state.
// ---------------------------------------------------------------------------

/// Holds when the picked object's linkset **handles touch**
/// ([`FLAGS_HANDLE_TOUCH`]) — enables Touch, the reference's
/// `Object.EnableTouch` (without its click-action refinements).
pub(crate) const TARGET_TOUCHABLE: &str = "target-touchable";

/// Holds when the agent **owns** the picked object ([`FLAGS_OBJECT_YOU_OWNER`])
/// — enables Take, Delete and Return. Deliberately narrower than the
/// reference's predicates (which also admit gods, group roles, and
/// objects-on-your-land for return); those refinements come with the features
/// that need them.
pub(crate) const TARGET_OWNED: &str = "target-owned";

/// Holds when the agent may **copy** the picked object
/// ([`FLAGS_OBJECT_COPY`]) — enables Take Copy, the reference's
/// `Tools.EnableTakeCopy`.
pub(crate) const TARGET_COPYABLE: &str = "target-copyable";

/// The `FLAGS_OBJECT_COPY` bit of an object's update flags (`object_flags.h`):
/// the agent may copy this object.
const FLAGS_OBJECT_COPY: u32 = 1 << 3;

/// The `FLAGS_OBJECT_YOU_OWNER` bit: the agent owns this object.
const FLAGS_OBJECT_YOU_OWNER: u32 = 1 << 5;

/// The `FLAGS_HANDLE_TOUCH` bit: the object's linkset has a touch handler.
const FLAGS_HANDLE_TOUCH: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// The take sub-pie (the reference's "Take Submenu", chained after Buy at the
// root's west slot — see the module doc for why ours is a plain sub-pie).
// Reference slots: Copies-Separately, two separators, Take Copy, Take, then
// the three multi-selection combined/separate slices.
// ---------------------------------------------------------------------------

/// The "Take >" sub-pie (root west slot).
static TAKE_PIE: PieMenuDef = PieMenuDef {
    label: "Take",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Copies: Separately",
                action: "take-copies-separately",
                when: Some(UNIMPLEMENTED),
            }),
        },
        // North-east and north are the reference's two separator slots: empty.
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Take Copy",
                action: "take-copy",
                when: Some(TARGET_COPYABLE),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Take",
                action: "take",
                when: Some(TARGET_OWNED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Take: Combined",
                action: "take-combined",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Copy: Combined",
                action: "take-copy-combined",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Take: Separately",
                action: "take-separately",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

// ---------------------------------------------------------------------------
// The attach sub-pies: the runtime-filled HUD / plain attach point lists stay
// as declared (empty = disabled) until wearing exists; the static Bento
// "Ext. Skeleton" tree is reproduced in full, pinning its addresses.
// ---------------------------------------------------------------------------

/// The "Attach HUD >" sub-pie — runtime-filled in the reference, so declared
/// empty (which renders disabled) until attaching exists.
static ATTACH_HUD_PIE: PieMenuDef = PieMenuDef {
    label: "Attach HUD",
    entries: &[],
};

/// The deeper "More >" of the Bento attach tree.
static ATTACH_SKELETON_MORE_PIE: PieMenuDef = PieMenuDef {
    label: "More",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Alt R Ear",
                action: "attach-alt-right-ear",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Alt R Eye",
                action: "attach-alt-right-eye",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Tongue",
                action: "attach-tongue",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Alt L Eye",
                action: "attach-alt-left-eye",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Alt L Ear",
                action: "attach-alt-left-ear",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "L Ring Finger",
                action: "attach-left-ring-finger",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Jaw",
                action: "attach-jaw",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "R Ring Finger",
                action: "attach-right-ring-finger",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Ext. Skeleton >" (Bento attach points) sub-pie of "Attach >".
static ATTACH_SKELETON_PIE: PieMenuDef = PieMenuDef {
    label: "Ext. Skeleton",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Right Wing",
                action: "attach-right-wing",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Groin",
                action: "attach-groin",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Tail Base",
                action: "attach-tail-base",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Tail Tip",
                action: "attach-tail-tip",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Left Wing",
                action: "attach-left-wing",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "L Hind Foot",
                action: "attach-left-hind-foot",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&ATTACH_SKELETON_MORE_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "R Hind Foot",
                action: "attach-right-hind-foot",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Attach >" sub-pie: the reference fills the plain attach points at
/// runtime (deferred with wearing); its static Bento tree is declared.
static ATTACH_PIE: PieMenuDef = PieMenuDef {
    label: "Attach",
    entries: &[PieEntry {
        at: Compass::East,
        content: PieContent::SubPie(&ATTACH_SKELETON_PIE),
    }],
};

// ---------------------------------------------------------------------------
// The second-level "More >" (the reference's "Object Pie More 2"): the reset /
// derender / report / export / pathfinding / mute / scripts tails.
// ---------------------------------------------------------------------------

/// The "Reset >" sub-pie of the second More level.
static RESET_PIE: PieMenuDef = PieMenuDef {
    label: "Reset",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Tex Refresh",
                action: "tex-refresh",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Skeleton",
                action: "reset-skeleton",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Derender >" sub-pie of the second More level
/// (`viewer-derender-blacklist`): drop the picked linkset from *this* viewer's
/// scene, permanently (the persisted blacklist) or for the session.
static DERENDER_PIE: PieMenuDef = PieMenuDef {
    label: "Derender",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Blacklist",
                action: "derender-blacklist",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Temporary",
                action: "derender",
                when: None,
            }),
        },
    ],
};

/// The "Save As >" sub-pie of the second More level.
static SAVE_AS_PIE: PieMenuDef = PieMenuDef {
    label: "Save As",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Backup",
                action: "export-backup",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Collada",
                action: "export-collada",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Dump XML",
                action: "dump-xml",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Pathfinding >" sub-pie of the second More level.
static PATHFINDING_PIE: PieMenuDef = PieMenuDef {
    label: "Pathfinding",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Show in Linksets",
                action: "pathfinding-linksets",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Show in Characters",
                action: "pathfinding-characters",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Mute >" sub-pie of the second More level. Mute itself is wired; muting
/// the particle *owner* needs particle picking and stays a placeholder.
static MUTE_PIE: PieMenuDef = PieMenuDef {
    label: "Mute",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Mute",
                action: "mute",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Part. Owner",
                action: "mute-particles",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Scripts >" sub-pie of the second More level.
static SCRIPTS_PIE: PieMenuDef = PieMenuDef {
    label: "Scripts",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Compile (Mono)",
                action: "scripts-compile-mono",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Compile (LSL)",
                action: "scripts-compile-lsl",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Reset Scripts",
                action: "scripts-reset",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Run Scripts",
                action: "scripts-run",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Stop Scripts",
                action: "scripts-stop",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Remove Scripts",
                action: "scripts-remove",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Script Info",
                action: "script-info",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The second "More >" level (the reference's "Object Pie More 2").
static OBJECT_MORE2_PIE: PieMenuDef = PieMenuDef {
    label: "More",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::SubPie(&RESET_PIE),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::SubPie(&DERENDER_PIE),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Report",
                action: "report",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::SubPie(&SAVE_AS_PIE),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::SubPie(&PATHFINDING_PIE),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::SubPie(&MUTE_PIE),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&SCRIPTS_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Inspect",
                action: "inspect",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The first "More >" level (the reference's "Object Pie More 1").
static OBJECT_MORE_PIE: PieMenuDef = PieMenuDef {
    label: "More",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Delete",
                action: "delete",
                when: Some(TARGET_OWNED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Wear",
                action: "wear",
                when: Some(UNIMPLEMENTED),
            }),
        },
        // The reference offers Take Copy here *and* in the Take sub-pie; both
        // addresses are kept, dispatching the same action.
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Take Copy",
                action: "take-copy",
                when: Some(TARGET_COPYABLE),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::SubPie(&ATTACH_HUD_PIE),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::SubPie(&ATTACH_PIE),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Return",
                action: "return",
                when: Some(TARGET_OWNED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&OBJECT_MORE2_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Buy",
                action: "buy",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The object pie. See `menu_pie_object.xml`: Open, Create, Touch, Sit
/// Here/Stand Up, [Buy/]Take, Pay, More, Edit (reference slots 0..7 → compass
/// East..SouthEast).
pub(crate) static OBJECT_PIE: PieMenuDef = PieMenuDef {
    label: "Object",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Open",
                action: "open",
                // Enabled for any object — the Object Contents floater
                // (viewer-prim-inventory-editing) lists whatever the prim holds.
                when: None,
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Create",
                action: "build",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Touch",
                action: "touch",
                when: Some(TARGET_TOUCHABLE),
            }),
        },
        // The reference's Sit Here / Stand Up autohide chain: one position, two
        // mutually exclusive candidates, whichever applies to the seated state.
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Chain(&[
                PieAction {
                    label: "Sit Here",
                    action: "sit-here",
                    when: Some(SELF_STANDING),
                },
                PieAction {
                    label: "Stand Up",
                    action: "stand",
                    when: Some(SELF_SITTING),
                },
            ]),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::SubPie(&TAKE_PIE),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Pay",
                action: "pay",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&OBJECT_MORE_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Edit",
                action: "edit",
                // Unconditional, like the reference: editing a no-modify
                // object still opens the tools (the sim rejects what it must).
                when: None,
            }),
        },
    ],
};

// ---------------------------------------------------------------------------
// The pick: resolving a world ray to an object, the way the left-click touch
// does, packaged for the shared right-click resolver.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The widget-facing wiring: open request → open pie → dispatch.
// ---------------------------------------------------------------------------

/// A resolved request to open the object pie at screen point `at`.
///
/// Written by the shared right-click resolver in [`crate::avatar_menu`] once a
/// right-click has resolved to an in-world object nearer than any avatar, and
/// consumed by [`open_object_menu`].
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenObjectMenu {
    /// The resolved object pick.
    pub(crate) hit: ObjectRayHit,
    /// Where to centre the pie, in logical pixels.
    pub(crate) at: Vec2,
}

/// The object the currently-open object pie acts on.
///
/// The pie's action strings are `&'static` and cannot carry ids, so the target
/// is stashed here when the menu opens and read back when an action fires. Set
/// on every open; a stale value between opens is harmless because no
/// object-menu [`UiAction`] is emitted unless a pie is open.
#[derive(Resource, Debug, Default)]
pub(crate) struct ObjectMenuTarget {
    /// The picked object and its ray surface, or `None` before any open.
    pub(crate) hit: Option<ObjectRayHit>,
    /// The object's name, once the properties-family reply fired at open time
    /// has landed — what a Mute is recorded under. `None` until then (a mute
    /// picked that early goes out with an empty name).
    pub(crate) name: Option<String>,
}

/// The plugin wiring the object context menu into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ObjectMenuPlugin;

impl Plugin for ObjectMenuPlugin {
    /// Register the target resource, the open request, and the systems that turn
    /// a resolved pick into an open pie and a picked slice into a command.
    fn build(&self, app: &mut App) {
        app.init_resource::<ObjectMenuTarget>()
            .add_message::<OpenObjectMenu>()
            .add_systems(
                Update,
                (
                    open_object_menu,
                    capture_object_menu_name,
                    handle_object_menu_actions,
                )
                    .chain(),
            );
    }
}

/// Turn a resolved object pick into an open pie: snapshot the conditions from
/// the object's flags and the seated state, stash the target, and fire the
/// properties-family request whose reply names the object (for Mute).
fn open_object_menu(
    mut requests: MessageReader<OpenObjectMenu>,
    parcel: Res<SlAgentParcel>,
    ground_sit: Res<SelfGroundSit>,
    mut target: ResMut<ObjectMenuTarget>,
    mut pies: MessageWriter<OpenPieMenu>,
    mut commands: MessageWriter<SlCommand>,
) {
    for request in requests.read() {
        target.hit = Some(request.hit.clone());
        target.name = None;
        let mut conditions = Vec::new();
        let flags = request.hit.summary.flags;
        if flags & FLAGS_HANDLE_TOUCH != 0 {
            conditions.push(TARGET_TOUCHABLE);
        }
        if flags & FLAGS_OBJECT_YOU_OWNER != 0 {
            conditions.push(TARGET_OWNED);
        }
        if flags & FLAGS_OBJECT_COPY != 0 {
            conditions.push(TARGET_COPYABLE);
        }
        // Exactly one of sitting / standing holds, resolving the Sit Here /
        // Stand Up chain. Sitting is an object-sit (`seated_on`) or the
        // viewer-tracked ground sit, as in `crate::avatar_menu`.
        let sitting = parcel.seated_on.is_some() || ground_sit.sitting;
        conditions.push(if sitting { SELF_SITTING } else { SELF_STANDING });
        pies.write(OpenPieMenu {
            menu: &OBJECT_PIE,
            at: request.at,
            element: OBJECT_MENU_ELEMENT,
            conditions,
        });
        // The name for a Mute (and, later, the header/properties surfaces) is
        // not in the update stream; ask for the condensed properties now so the
        // reply usually beats the user's slice pick.
        commands.write(SlCommand(Command::RequestObjectPropertiesFamily {
            request_flags: 0,
            object_id: request.hit.summary.root_full,
        }));
    }
}

/// Hold the open target's name once the properties-family reply lands, so a
/// Mute records the object under its real name.
fn capture_object_menu_name(
    mut events: MessageReader<SlEvent>,
    mut target: ResMut<ObjectMenuTarget>,
) {
    for event in events.read() {
        let SlSessionEvent::ObjectPropertiesFamily { properties } = &event.0 else {
            continue;
        };
        let Some(hit) = &target.hit else {
            continue;
        };
        if properties.object_id == hit.summary.root_full
            || properties.object_id == hit.summary.picked_full
        {
            target.name = Some(properties.name.clone());
        }
    }
}

/// Dispatch a picked object-menu slice to the command behind it.
///
/// Only the wired actions are matched; every other slice is a disabled
/// placeholder that never emits, so the fall-through is the whole of the
/// not-yet-implemented set and is intentionally silent.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the action \
              stream, the pie target, the inventory folders a derez needs, the seated state, \
              the edit-tool / selection state the Edit slice drives, and the command writer"
)]
fn handle_object_menu_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<ObjectMenuTarget>,
    inventory: Res<InventoryModel>,
    mut ground_sit: ResMut<SelfGroundSit>,
    tool: Res<crate::world_api::EditToolState>,
    floaters: Query<(Entity, &crate::floater::Floater)>,
    mut panels: Query<&mut crate::ui::UiPanelShown>,
    mut selection: ResMut<crate::world_api::SelectionSet>,
    state: Res<ObjectState>,
    seat_poses: Query<(&GlobalTransform, &crate::objects::ObjectSlMotion)>,
    seat_cameras: Query<&GlobalTransform, With<crate::world_api::ViewerCamera>>,
    mut commands: MessageWriter<SlCommand>,
    mut blocks: MessageWriter<RequestBlock>,
    mut derenders: MessageWriter<RequestDerender>,
    mut open_contents: MessageWriter<crate::edit_contents::OpenObjectContents>,
) {
    for action in actions.read() {
        if action.element != OBJECT_MENU_ELEMENT {
            continue;
        }
        let Some(hit) = &target.hit else {
            continue;
        };
        // Open (the reference's pie Open → LLFloaterOpenObject): show the picked
        // object's contents in the standalone Object Contents floater. Always the
        // whole object (its root), matching the reference's root-object selection.
        if action.action == "open" {
            open_contents.write(crate::edit_contents::OpenObjectContents {
                scoped: hit.summary.root_scoped,
                full: hit.summary.root_full,
            });
            continue;
        }
        // Edit (the reference's pie Edit): open the Build Tools floater —
        // which *is* edit mode — and make the picked object the selection.
        if action.action == "edit" {
            // Resolved by stable id, not the module resource: a lazily-built
            // Build Tools floater has no `BuildToolsUi` until this very open.
            if let Some(panel) =
                crate::floater::floater_panel(&floaters, crate::edit_tool::BUILD_TOOLS_FLOATER_ID)
                && let Ok(mut shown) = panels.get_mut(panel)
            {
                shown.0 = true;
            }
            let (scoped, full) = if tool.edit_linked {
                (hit.summary.picked_scoped, hit.summary.picked_full)
            } else {
                (hit.summary.root_scoped, hit.summary.root_full)
            };
            if let Some(entity) = state.entity_by_scoped(&scoped) {
                selection.clear();
                selection.insert(scoped, full, entity);
            }
            continue;
        }
        // The derez destinations that need a folder: take (and take-copy) land
        // in the system Objects folder, delete in the Trash — the reference's
        // choices. A missing folder (inventory skeleton not fetched yet) skips
        // the action rather than derezzing into nowhere.
        let derez = |destination: Option<DeRezDestination>| {
            destination.map(|destination| Command::DerezObjects {
                local_ids: vec![hit.summary.root_scoped],
                destination,
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
        };
        let folder = |folder_type: FolderType| {
            let folder = inventory.folder_by_type(folder_type);
            if folder.is_none() {
                warn!("object menu: no {folder_type:?} folder known yet; ignoring {action:?}");
            }
            folder
        };
        let command = match action.action {
            "touch" => Some(Command::TouchObject {
                local_id: hit.summary.picked_scoped,
                surface: Some(hit.surface.clone()),
            }),
            "sit-here" => Some(Command::Sit {
                target: hit.summary.picked_full,
                offset: sit_here_offset(hit, &state, &seat_poses, &seat_cameras),
            }),
            "stand" => {
                ground_sit.sitting = false;
                Some(Command::Stand)
            }
            "take" => {
                derez(folder(FolderType::Object).map(DeRezDestination::TakeIntoAgentInventory))
            }
            "take-copy" => {
                derez(folder(FolderType::Object).map(DeRezDestination::AcquireToAgentInventory))
            }
            "delete" => derez(folder(FolderType::Trash).map(DeRezDestination::Trash)),
            "return" => derez(Some(DeRezDestination::ReturnToOwner)),
            "mute" => {
                // Blocks go through the guarded request channel, never straight
                // onto the wire (see `crate::mutes`).
                blocks.write(RequestBlock::new(
                    hit.summary.root_full.uuid(),
                    target.name.clone().unwrap_or_default(),
                    MuteType::Object,
                ));
                None
            }
            // Derender the whole picked linkset (its root, which takes the
            // child prims with it), permanently or for the session. Nothing
            // goes on the wire — the simulator keeps streaming it, this viewer
            // just stops mirroring it (see `crate::derender`).
            action @ ("derender" | "derender-blacklist") => {
                derenders.write(RequestDerender::new(
                    hit.summary.root_full.uuid(),
                    target.name.clone().unwrap_or_default(),
                    DerenderKind::Object,
                    action == "derender-blacklist",
                ));
                None
            }
            // Every other slice is a disabled placeholder: no behaviour yet.
            _other => None,
        };
        if let Some(command) = command {
            commands.write(SlCommand(command));
        }
    }
}

/// The `AgentRequestSit` offset for a "Sit Here" on `hit`: the reference's
/// `calcFocusOffset` ([`crate::sit_offset`]) in Second Life region axes, computed
/// from the seat prim's world pose + scale and the camera.
///
/// Falls back to the raw object-local surface point (the old behaviour) when the
/// seat prim's entity / pose or the camera is not available — the sit still goes
/// out, just without the faithful offset. When a prim has a real sit target the
/// simulator ignores this offset entirely, so the fallback is harmless there.
fn sit_here_offset(
    hit: &ObjectRayHit,
    state: &ObjectState,
    seat_poses: &Query<(&GlobalTransform, &crate::objects::ObjectSlMotion)>,
    seat_cameras: &Query<&GlobalTransform, With<crate::world_api::ViewerCamera>>,
) -> Vector {
    let fallback = || hit.surface.position.clone();
    let Some(entity) = state.entity_by_scoped(&hit.summary.picked_scoped) else {
        return fallback();
    };
    let (Ok((object_global, sl_motion)), Ok(camera_global)) =
        (seat_poses.get(entity), seat_cameras.single())
    else {
        return fallback();
    };

    let obj_pos = object_global.translation();
    let cam_pos = camera_global.translation();
    // The world surface-hit point, recovered from the object-local surface point.
    let hit_world = object_global.transform_point(Vec3::new(
        hit.surface.position.x,
        hit.surface.position.y,
        hit.surface.position.z,
    ));

    // The prim's Second Life world rotation, recovered from its Bevy world
    // rotation (`R_bevy = basis · q_sl`), so it rotates the Second Life-packed
    // vectors below. Works for a linkset-child seat too (the global transform
    // already composes the parent).
    let obj_rotation = crate::coords::sl_to_bevy_rotation()
        .inverse()
        .mul_quat(object_global.rotation());
    let extents = Vec3::new(sl_motion.scale.x, sl_motion.scale.y, sl_motion.scale.z);

    crate::sit_offset::sit_focus_offset(
        obj_rotation,
        extents,
        bevy_delta_to_sl(crate::sit_offset::vsub(cam_pos, obj_pos)),
        bevy_delta_to_sl(crate::sit_offset::vsub(hit_world, cam_pos)).normalize_or_zero(),
        bevy_delta_to_sl(crate::sit_offset::vsub(hit_world, obj_pos)),
        bevy_delta_to_sl(camera_global.forward().as_vec3()),
    )
}

/// A Bevy-space delta [`Vec3`] as a Second Life-packed [`Vec3`] (Second Life
/// components in `x`/`y`/`z`) — the vector form of
/// [`bevy_to_sl_vec`](crate::coords::bevy_to_sl_vec), for a direction / offset
/// (no region origin). Inverting `(x, y, z) -> (x, z, -y)` gives
/// `(bx, by, bz) -> (bx, -bz, by)`.
const fn bevy_delta_to_sl(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y)
}

#[cfg(test)]
mod tests {
    use super::{OBJECT_PIE, TAKE_PIE, TARGET_COPYABLE, TARGET_OWNED, TARGET_TOUCHABLE};
    use crate::avatar_menu::{SELF_SITTING, SELF_STANDING};
    use crate::menu::UNIMPLEMENTED;
    use crate::pie_menu::{
        Compass, PieAddress, PieConditions, PieContent, PieMenuDef, ResolvedSlot, SlotOutcome,
        addresses, resolve_slots,
    };
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The resolved slot at `point`, or a test error naming what was missing.
    fn slot_at(
        slots: &[Option<ResolvedSlot>; crate::pie_menu::PIE_SLICES],
        point: Compass,
    ) -> Result<ResolvedSlot, TestError> {
        slots
            .get(point.slot())
            .copied()
            .flatten()
            .ok_or_else(|| format!("no slot at {}", point.name()).into())
    }

    /// The addresses of the walk, as `(action, path)` pairs, for comparison.
    fn address_pairs(menu: &'static PieMenuDef) -> Vec<(&'static str, Vec<Compass>)> {
        addresses(menu)
            .into_iter()
            .map(|(action, PieAddress(path))| (action, path))
            .collect()
    }

    /// **The object pie's address table, pinned.**
    ///
    /// Moving any object action to a different compass path re-teaches every
    /// user who learned this menu with their hand; this table makes that a loud
    /// diff. If a move is intended, change the table in the same reviewed
    /// commit.
    #[test]
    fn object_pie_keeps_every_address() {
        use Compass::{East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West};
        let expected: Vec<(&str, Vec<Compass>)> = vec![
            ("open", vec![East]),
            ("build", vec![NorthEast]),
            ("touch", vec![North]),
            // The Sit Here / Stand Up autohide chain: one address, two actions.
            ("sit-here", vec![NorthWest]),
            ("stand", vec![NorthWest]),
            ("take-copies-separately", vec![West, East]),
            ("take-copy", vec![West, NorthWest]),
            ("take", vec![West, West]),
            ("take-combined", vec![West, SouthWest]),
            ("take-copy-combined", vec![West, South]),
            ("take-separately", vec![West, SouthEast]),
            ("pay", vec![SouthWest]),
            ("delete", vec![South, East]),
            ("wear", vec![South, NorthEast]),
            ("take-copy", vec![South, North]),
            ("attach-right-wing", vec![South, West, East, East]),
            ("attach-groin", vec![South, West, East, NorthEast]),
            ("attach-tail-base", vec![South, West, East, North]),
            ("attach-tail-tip", vec![South, West, East, NorthWest]),
            ("attach-left-wing", vec![South, West, East, West]),
            ("attach-left-hind-foot", vec![South, West, East, SouthWest]),
            ("attach-alt-right-ear", vec![South, West, East, South, East]),
            (
                "attach-alt-right-eye",
                vec![South, West, East, South, NorthEast],
            ),
            ("attach-tongue", vec![South, West, East, South, North]),
            (
                "attach-alt-left-eye",
                vec![South, West, East, South, NorthWest],
            ),
            ("attach-alt-left-ear", vec![South, West, East, South, West]),
            (
                "attach-left-ring-finger",
                vec![South, West, East, South, SouthWest],
            ),
            ("attach-jaw", vec![South, West, East, South, South]),
            (
                "attach-right-ring-finger",
                vec![South, West, East, South, SouthEast],
            ),
            ("attach-right-hind-foot", vec![South, West, East, SouthEast]),
            ("return", vec![South, SouthWest]),
            ("tex-refresh", vec![South, South, East, East]),
            ("reset-skeleton", vec![South, South, East, NorthEast]),
            ("derender-blacklist", vec![South, South, NorthEast, East]),
            ("derender", vec![South, South, NorthEast, NorthEast]),
            ("report", vec![South, South, North]),
            ("export-backup", vec![South, South, NorthWest, East]),
            ("export-collada", vec![South, South, NorthWest, NorthEast]),
            ("dump-xml", vec![South, South, NorthWest, North]),
            ("pathfinding-linksets", vec![South, South, West, East]),
            (
                "pathfinding-characters",
                vec![South, South, West, NorthEast],
            ),
            ("mute", vec![South, South, SouthWest, East]),
            ("mute-particles", vec![South, South, SouthWest, NorthEast]),
            ("scripts-compile-mono", vec![South, South, South, East]),
            ("scripts-compile-lsl", vec![South, South, South, NorthEast]),
            ("scripts-reset", vec![South, South, South, North]),
            ("scripts-run", vec![South, South, South, NorthWest]),
            ("scripts-stop", vec![South, South, South, West]),
            ("scripts-remove", vec![South, South, South, SouthWest]),
            ("script-info", vec![South, South, South, South]),
            ("inspect", vec![South, South, SouthEast]),
            ("buy", vec![South, SouthEast]),
            ("edit", vec![SouthEast]),
        ];
        assert_eq!(
            address_pairs(&OBJECT_PIE),
            expected,
            "an object pie action moved — if intended, bless it by editing this table"
        );
    }

    /// No pie in the object tree declares two entries at one compass position —
    /// a silent overwrite whose winner would depend on declaration order.
    #[test]
    fn no_pie_declares_two_entries_at_one_position() {
        fn check(menu: &'static PieMenuDef, failures: &mut Vec<String>) {
            for point in Compass::ALL {
                let count = menu
                    .entries
                    .iter()
                    .filter(|entry| entry.at == point)
                    .count();
                if count > 1 {
                    failures.push(format!(
                        "`{}` declares {count} entries at {}",
                        menu.label,
                        point.name()
                    ));
                }
            }
            for entry in menu.entries {
                if let PieContent::SubPie(sub) = entry.content {
                    check(sub, failures);
                }
            }
        }
        let mut failures = Vec::new();
        check(&OBJECT_PIE, &mut failures);
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The Sit Here / Stand Up chain resolves to exactly the member matching the
    /// seated state, at the same north-west position either way.
    #[test]
    fn sit_and_stand_share_the_north_west_slot() -> Result<(), TestError> {
        let standing = resolve_slots(&OBJECT_PIE, &PieConditions::new([SELF_STANDING]));
        let sit = slot_at(&standing, Compass::NorthWest)?;
        assert_eq!(sit.outcome, SlotOutcome::Action("sit-here"));
        assert!(sit.enabled, "Sit Here must be live while standing");

        let sitting = resolve_slots(&OBJECT_PIE, &PieConditions::new([SELF_SITTING]));
        let stand = slot_at(&sitting, Compass::NorthWest)?;
        assert_eq!(stand.outcome, SlotOutcome::Action("stand"));
        assert!(stand.enabled, "Stand Up must be live while sitting");
        Ok(())
    }

    /// Touch is enabled exactly when the picked linkset handles touch, and keeps
    /// its north slot (disabled) when it does not.
    #[test]
    fn touch_tracks_the_touch_handler_flag() -> Result<(), TestError> {
        let touchable = resolve_slots(&OBJECT_PIE, &PieConditions::new([TARGET_TOUCHABLE]));
        let touch = slot_at(&touchable, Compass::North)?;
        assert_eq!(touch.outcome, SlotOutcome::Action("touch"));
        assert!(
            touch.enabled,
            "Touch must be live on a touch-scripted object"
        );

        let plain = resolve_slots(&OBJECT_PIE, &PieConditions::default());
        assert!(
            !slot_at(&plain, Compass::North)?.enabled,
            "Touch must be disabled on an object with no touch handler"
        );
        Ok(())
    }

    /// The wired take / take-copy pair track ownership and copyability, each in
    /// its declared slot of the take sub-pie.
    #[test]
    fn take_and_take_copy_track_the_permission_flags() -> Result<(), TestError> {
        let owned = resolve_slots(
            &TAKE_PIE,
            &PieConditions::new([TARGET_OWNED, TARGET_COPYABLE]),
        );
        let take = slot_at(&owned, Compass::West)?;
        assert_eq!(take.outcome, SlotOutcome::Action("take"));
        assert!(take.enabled, "Take must be live on an owned object");
        assert!(
            slot_at(&owned, Compass::NorthWest)?.enabled,
            "Take Copy must be live on a copyable object"
        );

        let unowned = resolve_slots(&TAKE_PIE, &PieConditions::default());
        assert!(
            !slot_at(&unowned, Compass::West)?.enabled,
            "Take must be disabled on an unowned object"
        );
        assert!(
            !slot_at(&unowned, Compass::NorthWest)?.enabled,
            "Take Copy must be disabled on an uncopyable object"
        );
        Ok(())
    }

    /// In the live viewer's actual state (the sentinel [`UNIMPLEMENTED`] is
    /// never supplied), every placeholder keeps its slot but reads disabled —
    /// so the reference menu shape is present before the features are — and the
    /// runtime-filled Attach HUD sub-pie reads disabled while empty.
    #[test]
    fn unimplemented_entries_are_disabled_but_present() -> Result<(), TestError> {
        let plain = resolve_slots(&OBJECT_PIE, &PieConditions::default());
        for (point, name) in [(Compass::NorthEast, "Create"), (Compass::SouthWest, "Pay")] {
            assert!(
                !slot_at(&plain, point)?.enabled,
                "{name} is a placeholder and must read disabled until it is wired"
            );
        }
        // Edit is wired (viewer-object-edit-floater-shell) and unconditional,
        // like the reference: it opens the build tools on any object.
        assert!(
            slot_at(&plain, Compass::SouthEast)?.enabled,
            "Edit is wired and must read enabled with no conditions held"
        );
        // Open is wired (viewer-prim-inventory-editing) and unconditional, like
        // the reference: it opens the Object Contents floater on any object.
        assert!(
            slot_at(&plain, Compass::East)?.enabled,
            "Open is wired and must read enabled with no conditions held"
        );
        // The proof that the sentinel is what disables the remaining placeholders:
        // hold it, and they light up. The live viewer never does this.
        let held = resolve_slots(&OBJECT_PIE, &PieConditions::new([UNIMPLEMENTED]));
        assert!(
            slot_at(&held, Compass::NorthEast)?.enabled,
            "holding the sentinel proves it is the only thing gating the placeholder"
        );
        // The empty (runtime-filled) Attach HUD sub-pie renders disabled.
        let more = resolve_slots(&super::OBJECT_MORE_PIE, &PieConditions::default());
        let attach_hud = slot_at(&more, Compass::NorthWest)?;
        assert!(
            !attach_hud.enabled,
            "an empty runtime-filled sub-pie must read disabled"
        );
        Ok(())
    }
}
