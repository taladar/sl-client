//! The **avatar** context / pie menu (`viewer-avatar-context-menu`): the entry
//! trees offered when an avatar is the pick target, and the dispatch of each.
//!
//! This is the *entries*, not the widget — the radial widget itself is
//! [`crate::pie_menu`], and this module only declares two [`PieMenuDef`]s and the
//! systems that open and act on them. The reference viewer has a **distinct pie
//! per pick target** (`menu_pie_avatar_self.xml` vs `menu_pie_avatar_other.xml`),
//! and so do we: [`AVATAR_SELF_PIE`] for your own avatar, [`AVATAR_OTHER_PIE`] for
//! anyone else. Which one opens is chosen at pick time by comparing the picked
//! agent to [`SlIdentity::agent_id`](sl_client_bevy::SlIdentity).
//!
//! # What is wired, and what is a disabled placeholder
//!
//! The reference offers many avatar actions; most of them are features this
//! viewer does not have yet (a pay dialog, an abuse report, outfit editing,
//! the moderation powers). Those are declared **in their
//! reference compass positions but disabled** — a slice you can see and aim at but
//! not pick, so the menu's shape (the muscle memory) is laid down now and the
//! slices light up as the features land. A disabled slice is one whose `when`
//! names [`UNIMPLEMENTED`], a condition that is never supplied, so it always reads
//! faded. When the feature behind it exists, its `when` changes to a real
//! condition (or `None`) in one deliberate edit — the address never moves.
//!
//! The handful that already have a home in this viewer are wired for real:
//!
//! - **IM** (other) → opens a one-to-one conversation tab
//!   ([`crate::world_api::OpenConversation`]), exactly as the People list's IM
//!   action does.
//! - **Stand Up / Sit Down** (self) → [`Command::Stand`] / [`Command::SitOnGround`],
//!   each enabled only in the state where it makes sense (you cannot stand up
//!   unless you are sitting), gated on [`SELF_SITTING`] / [`SELF_STANDING`].
//! - **Mute** (other) → a guarded [`crate::world_api::RequestBlock`] for the picked
//!   agent.
//! - **Add as Friend** (other) → [`Command::OfferFriendship`], disabled via
//!   [`TARGET_NOT_FRIEND`] when the agent already is a friend, matching the
//!   reference's `on_enable`.
//! - **Profile** (self and other) → opens the avatar profile floater
//!   ([`crate::world_api::OpenAvatarProfile`]).
//! - **More ▸ Derender ▸ Blacklist / Temporary** (other) → a guarded
//!   [`crate::derender::RequestDerender`] for the picked agent: this viewer
//!   stops drawing them (and, through the scene mirror's suppression index,
//!   their attachments), permanently or for the session.
//!
//! # Where we depart from the reference, on purpose
//!
//! The reference pies do not fit our widget one-to-one, and two departures are
//! worth stating because they are deliberate, not oversights:
//!
//! - **Eight slots, not nine.** The reference self pie has *nine* top-level
//!   slices (it lets the ring overflow); ours is a hard eight ([`PIE_SLICES`]).
//!   The ninth reference slice is `Textures` (a debug texture dump), which we fold
//!   into the `Appearance >` sub-pie next to the other debug entries, so all eight
//!   compass positions still match the reference exactly.
//! - **No `More >`.** The reference leans on nameless `More >` overflow pies
//!   several levels deep; [`crate::pie_menu`] rules those out by construction (a
//!   sub-pie's label is not optional). Where the reference's first level is itself
//!   named `More >` (the "other" avatar pie's south slice), we keep the slice in
//!   its reference position but populate it from the reference's *own* first level
//!   of that overflow, and stop there — the deep debug tails (per-attachment-point
//!   detach lists, impostor display modes, the nested clothing overflow) are left
//!   for later rather than reproduced as dozens of dead slices.
//!
//! # How a pick reaches here
//!
//! Picking is deliberately reusable: every pickable piece of an avatar — the
//! placeholder sphere, each rigged body part, each worn rigged submesh, and the
//! floating name tag — carries [`crate::world_api::AvatarPickTarget`] with the
//! avatar's agent id. [`request_avatar_menu_on_right_click`] resolves a
//! right-click to an agent two ways, mirroring the reference's "name tag or the
//! avatar itself": the on-screen tag rect test
//! ([`crate::name_tag_billboard::NameTagHitTest`] — tags are world-space
//! billboard meshes no picking backend covers) or the **GPU ID-buffer pick**
//! ([`crate::gpu_pick`]) against exactly what is drawn — the avatar's
//! GPU-posed pixels, an object face, or bare terrain — resolved a frame later
//! by [`resolve_right_click_pick`]. The same tag identity is what the
//! inventory drag-and-drop onto an avatar reuses to find its drop target,
//! which is why the identity lives on the entities rather than in a menu-only
//! lookup.
//!
//! Reference (Firestorm, read-only): `menu_pie_avatar_self.xml`,
//! `menu_pie_avatar_other.xml` (the compass positions), and
//! `newview/llviewermenu.cpp` (the action handlers).

use std::collections::HashSet;

use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sl_client_bevy::{AgentKey, Command, MuteType, SlAgentParcel, SlCommand, SlIdentity};

use crate::attachment_menu::{ATTACHMENT_MENU_ELEMENT, OpenAttachmentMenu};
use crate::avatars::{AvatarState, RefetchAvatarTextures};
use crate::derender::RequestDerender;
use crate::gpu_pick::{GpuPickResolved, GpuPicker, PickPurpose, PickResolution};
use crate::hud::HudCamera;
use crate::hud_pick::{pointer_over_blocking_ui, pointer_over_hud};
use crate::input_action::Action;
use crate::land_menu::OpenLandMenu;
use crate::menu::UNIMPLEMENTED;
use crate::name_tag_billboard::NameTagHitTest;
use crate::object_menu::OpenObjectMenu;
use crate::objects::ObjectPicker;
use crate::pie_menu::{Compass, OpenPieMenu, PieAction, PieContent, PieEntry, PieMenuDef};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;
use crate::world_api::DerenderKind;
use crate::world_api::OpenAvatarProfile;
use crate::world_api::RequestBlock;
use crate::world_api::on_hud_layer;
use crate::world_api::{ConversationKey, OpenConversation};
use crate::world_api::{FriendsModel, SelfGroundSit};

/// The `element` both avatar pies attribute their [`UiAction`]s to.
///
/// One tag for self *and* other: the two pies never overlap in the actions they
/// declare (self has Stand / Sit, other has IM / Mute), and the picked agent —
/// the only thing a handler needs beyond the action name — is carried out of band
/// in [`AvatarMenuTarget`], not baked into the tag.
pub(crate) const AVATAR_MENU_ELEMENT: &str = "avatar-menu";

// ---------------------------------------------------------------------------
// The condition vocabulary. Every name here is a compile-time constant; the set
// that *holds* is built at open time from world / session state.
// ---------------------------------------------------------------------------

/// Holds when the local avatar is **sitting** — enables self "Stand Up".
pub(crate) const SELF_SITTING: &str = "self-sitting";

/// Holds when the local avatar is **standing** — enables self "Sit Down".
pub(crate) const SELF_STANDING: &str = "self-standing";

/// Holds when the picked agent is **not already a friend** — enables "Add as
/// Friend", matching the reference's `Avatar.EnableAddFriend`.
pub(crate) const TARGET_NOT_FRIEND: &str = "target-not-friend";

/// Holds when the picked agent is **not** already pinned to "always draw in
/// full" — enables More ▸ Render ▸ Fully.
pub(crate) const TARGET_RENDER_NOT_FULLY: &str = "target-render-not-fully";

/// Holds when the picked agent is **not** already pinned to "never draw in
/// full" — enables More ▸ Render ▸ Never.
pub(crate) const TARGET_RENDER_NOT_NEVER: &str = "target-render-not-never";

/// Holds when the picked agent **has** a standing render exception — enables
/// More ▸ Render ▸ Normally, which clears it. The reference shows the three as
/// check items; a pie slice cannot carry a tick, so the decision already in
/// force is the one shown greyed out.
pub(crate) const TARGET_RENDER_EXCEPTED: &str = "target-render-excepted";

/// The render conditions that hold for an agent whose standing exception is
/// `setting` — every slice of the Render sub-pie except the one already in
/// force.
pub(crate) fn render_pie_conditions(
    setting: crate::avatar_complexity::RenderOverride,
) -> Vec<&'static str> {
    use crate::avatar_complexity::RenderOverride;
    let mut conditions = Vec::new();
    if setting != RenderOverride::AlwaysFull {
        conditions.push(TARGET_RENDER_NOT_FULLY);
    }
    if setting != RenderOverride::Never {
        conditions.push(TARGET_RENDER_NOT_NEVER);
    }
    if setting != RenderOverride::Normal {
        conditions.push(TARGET_RENDER_EXCEPTED);
    }
    conditions
}

// ---------------------------------------------------------------------------
// The "other avatar" pie. Top level matches menu_pie_avatar_other.xml exactly:
// Profile, Mute>, Go to, Report, Add>, Pay, More>, IM (reference slots 0..7 →
// compass East..SouthEast).
// ---------------------------------------------------------------------------

/// The "Mute >" sub-pie of the other-avatar pie (reference slot 1 / north-east),
/// shared verbatim by the attachment-other pie ([`crate::attachment_menu`]).
pub(crate) static OTHER_MUTE_PIE: PieMenuDef = PieMenuDef {
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
                label: "Mute Particle Owner",
                action: "mute-particles",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Add >" sub-pie of the other-avatar pie (reference slot 4 / west).
static OTHER_ADD_PIE: PieMenuDef = PieMenuDef {
    label: "Add",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Add as Friend",
                action: "add-friend",
                when: Some(TARGET_NOT_FRIEND),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Add to Set",
                action: "add-to-set",
                when: None,
            }),
        },
        // A **deliberate addition**: the reference reaches its pseudonyms only
        // from the Contact Sets panel, but naming someone is something you do
        // when you are looking at them, and the pie is where that is done. It
        // sits beside Add to Set because both write the same store.
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Set Alias",
                action: "set-alias",
                when: None,
            }),
        },
    ],
};

/// The nested "More >" of the other-avatar `More >` sub-pie (reference slot 5 /
/// south-west of the first `More >`), which is where the reference buries the
/// per-avatar debug actions. Populated from the reference's own level up to
/// **Tex Refresh** — the one entry wired here (its manual bake re-fetch is a real
/// feature now); Textures / Script Info / Call / Zoom In keep their reference
/// addresses as disabled placeholders, and the reference's later Reset / Dump XML
/// / Display tails are deferred (see the module doc's "stop there" rule).
static OTHER_MORE_MORE_PIE: PieMenuDef = PieMenuDef {
    label: "More",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Textures",
                action: "textures",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Script Info",
                action: "script-info",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Call",
                action: "call",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Zoom In",
                action: "zoom-in",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Tex Refresh",
                action: "tex-refresh",
                when: None,
            }),
        },
    ],
};

/// The "More >" sub-pie of the other-avatar pie (reference slot 6 / south).
///
/// Populated from the reference `More >`'s own first level (Freeze, Give Card,
/// Invite to Group, Face towards, Eject, and its nested `More >` at south-west);
/// the reference's deeper derender tails are deferred rather than reproduced as
/// dead slices (see the module doc).
static OTHER_MORE_PIE: PieMenuDef = PieMenuDef {
    label: "More",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Freeze",
                action: "freeze",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Give Card",
                action: "give-card",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Invite to Group",
                action: "invite-to-group",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Face Towards",
                action: "face-towards",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Eject",
                action: "eject",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::SubPie(&OTHER_MORE_MORE_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::SubPie(&OTHER_DERENDER_PIE),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&OTHER_RENDER_PIE),
        },
    ],
};

/// The "Render >" sub-pie of the other-avatar `More >`
/// (`viewer-avatar-complexity-limit`): this avatar's standing exception to the
/// automatic complexity limit — always draw them in full whatever they cost,
/// never draw them in full, or go back to letting the limit decide.
///
/// The reference offers the same three as check items in the avatar context
/// menu (`AlwaysRenderFully` / `DoNotRender` / `RenderNormally`). A pick is a
/// standing decision about that person, persisted per account by
/// [`crate::avatar_render_settings`] and managed in its floater
/// (World ▸ Avatar Render Settings).
static OTHER_RENDER_PIE: PieMenuDef = PieMenuDef {
    label: "Render",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Fully",
                action: "render-fully",
                when: Some(TARGET_RENDER_NOT_FULLY),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Normally",
                action: "render-normally",
                when: Some(TARGET_RENDER_EXCEPTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Never",
                action: "render-never",
                when: Some(TARGET_RENDER_NOT_NEVER),
            }),
        },
    ],
};

/// The per-avatar render exception a pie action sets, or `None` if the action is
/// not one of [`OTHER_RENDER_PIE`]'s three.
const fn render_override_for(action: &str) -> Option<crate::avatar_complexity::RenderOverride> {
    use crate::avatar_complexity::RenderOverride;
    match action.as_bytes() {
        b"render-fully" => Some(RenderOverride::AlwaysFull),
        b"render-normally" => Some(RenderOverride::Normal),
        b"render-never" => Some(RenderOverride::Never),
        _other => None,
    }
}

/// The "Derender >" sub-pie of the other-avatar `More >`
/// (`viewer-derender-blacklist`): stop drawing this avatar — and, with it, its
/// attachments — in *this* viewer, permanently (the persisted blacklist) or for
/// the session. The reference addresses both slices at the pie's southern
/// slots, leaving the first six empty, and so do we.
static OTHER_DERENDER_PIE: PieMenuDef = PieMenuDef {
    label: "Derender",
    entries: &[
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Blacklist",
                action: "derender-blacklist",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Temporary",
                action: "derender",
                when: None,
            }),
        },
    ],
};

/// The pie for **another** avatar. See `menu_pie_avatar_other.xml`.
pub(crate) static AVATAR_OTHER_PIE: PieMenuDef = PieMenuDef {
    label: "Avatar",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Profile",
                action: "profile",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::SubPie(&OTHER_MUTE_PIE),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Go To",
                action: "go-to",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Report",
                action: "report",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::SubPie(&OTHER_ADD_PIE),
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
            content: PieContent::SubPie(&OTHER_MORE_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "IM",
                action: "im",
                when: None,
            }),
        },
    ],
};

// ---------------------------------------------------------------------------
// The "self avatar" pie. Top level matches menu_pie_avatar_self.xml: Profile,
// Groups, Take Off>, Sit Down, Stand Up, Script Info, Gestures, Appearance>
// (reference slots 0..7 → compass East..SouthEast). The reference's ninth slice,
// Textures, is folded into Appearance> below.
// ---------------------------------------------------------------------------

/// The "Clothes >" sub-pie of the self "Take Off >" pie.
///
/// The reference's eight-plus clothing layers, its first eight taken at their
/// reference compass positions (its `More >` overflow of undershirt / underpants /
/// tattoo / physics / alpha / all-clothes is deferred). Every layer is disabled —
/// wearables / take-off is not implemented yet.
static SELF_CLOTHES_PIE: PieMenuDef = PieMenuDef {
    label: "Clothes",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Shirt",
                action: "takeoff-shirt",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Pants",
                action: "takeoff-pants",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Shoes",
                action: "takeoff-shoes",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Socks",
                action: "takeoff-socks",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Jacket",
                action: "takeoff-jacket",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Gloves",
                action: "takeoff-gloves",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Skirt",
                action: "takeoff-skirt",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The self "Take Off >" sub-pie (reference slot 2 / north).
///
/// The reference's `HUD >` and `Detach >` sub-pies are per-attachment runtime
/// lists (which HUDs / attachments you are actually wearing); until that data is
/// wired they are single disabled leaves rather than empty sub-pies.
static SELF_TAKEOFF_PIE: PieMenuDef = PieMenuDef {
    label: "Take Off",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::SubPie(&SELF_CLOTHES_PIE),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Detach HUD",
                action: "detach-hud",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Detach",
                action: "detach-attachment",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Detach All",
                action: "detach-all",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Reset >" sub-pie of the self "Appearance >" pie, shared verbatim by
/// both attachment pies' reset tails ([`crate::attachment_menu`]).
pub(crate) static SELF_RESET_PIE: PieMenuDef = PieMenuDef {
    label: "Reset",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Skeleton & Animations",
                action: "reset-skel-anim",
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
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Mesh LOD",
                action: "reset-mesh-lod",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The self "Appearance >" sub-pie (reference slot 7 / south-east), with the
/// reference's ninth top-level slice, `Textures`, folded in at south.
static SELF_APPEARANCE_PIE: PieMenuDef = PieMenuDef {
    label: "Appearance",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Edit Shape",
                action: "edit-shape",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::SubPie(&SELF_RESET_PIE),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Texture Refresh",
                action: "tex-refresh",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Edit Outfit",
                action: "edit-outfit",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Dump XML",
                action: "dump-xml",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Hover Height",
                action: "hover-height",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Textures",
                action: "textures",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The pie for your **own** avatar. See `menu_pie_avatar_self.xml`.
pub(crate) static AVATAR_SELF_PIE: PieMenuDef = PieMenuDef {
    label: "Self",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Profile",
                action: "profile",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Groups",
                action: "groups",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::SubPie(&SELF_TAKEOFF_PIE),
        },
        // Sit Down and Stand Up are two fixed slices, each gated on the state it
        // applies in, exactly as the reference keeps them (two greyed slices, one
        // live at a time) — not one autohide chain. Their addresses never move.
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Sit Down",
                action: "sit-ground",
                when: Some(SELF_STANDING),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Stand Up",
                action: "stand",
                when: Some(SELF_SITTING),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Script Info",
                action: "script-info",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Gestures",
                action: "gestures",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::SubPie(&SELF_APPEARANCE_PIE),
        },
    ],
};

// ---------------------------------------------------------------------------
// The widget-facing wiring: pick → open → dispatch.
// ---------------------------------------------------------------------------

/// How far the pointer may travel between a right-button press and release and
/// still count as a **click** rather than a drag, in logical pixels.
///
/// This viewer binds a right-**drag** to camera free-look ([`crate::camera`]), so
/// the menu must open only on a right-*click*: press and release without moving.
/// A few pixels of slop absorbs the tiny motion of an ordinary click.
const RIGHT_CLICK_DRAG_SLOP: f32 = 6.0;

/// Tracks an in-progress right-button gesture, to tell a click from a free-look
/// drag. Reset on each press; the accumulated motion decides at release.
#[derive(Resource, Debug, Default, Clone, Copy)]
struct RightClickGesture {
    /// Whether the right button is currently held from a press this system saw.
    down: bool,
    /// Total pointer travel since the press, in logical pixels.
    moved: f32,
}

/// The agent the currently-open avatar pie acts on.
///
/// The pie's action strings are `&'static` and cannot carry a UUID, so the target
/// is stashed here when the menu opens and read back when an action fires. Set on
/// every open — by [`open_avatar_menu`], and by the attachment pies' opener
/// ([`crate::attachment_menu`]), which stores the **wearer** so its
/// avatar-derived slices dispatch through the shared handler. A stale value
/// between opens is harmless because no avatar-menu [`UiAction`] is emitted
/// unless a pie is open.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub(crate) struct AvatarMenuTarget {
    /// The picked agent, or `None` before any avatar menu has opened.
    pub(crate) agent: Option<AgentKey>,
}

/// A resolved request to open an avatar pie on `agent` at screen point `at`.
///
/// Written by [`request_avatar_menu_on_right_click`] once a right-click has been
/// resolved to an avatar (by name tag or body), and consumed by
/// [`open_avatar_menu`], which decides self vs other and computes the conditions.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenAvatarMenu {
    /// The picked avatar.
    pub(crate) agent: AgentKey,
    /// Where to centre the pie, in logical pixels.
    pub(crate) at: Vec2,
}

/// The plugin wiring the avatar context menu into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AvatarMenuPlugin;

impl Plugin for AvatarMenuPlugin {
    /// Register the target resource, the open request, and the three systems that
    /// turn a right-click into an open pie and a picked slice into an action.
    fn build(&self, app: &mut App) {
        app.init_resource::<AvatarMenuTarget>()
            .init_resource::<RightClickGesture>()
            .init_resource::<SelfGroundSit>()
            .add_message::<OpenAvatarMenu>()
            .add_systems(
                Update,
                (
                    request_avatar_menu_on_right_click,
                    resolve_right_click_pick,
                    open_avatar_menu,
                    handle_avatar_menu_actions,
                    clear_ground_sit_on_move,
                )
                    .chain(),
            );
        // The cursor-following pick inspector, off unless `SL_VIEWER_DEBUG_PICK` is
        // set — a debug aid that shows, live, what a pick at the cursor would hit.
        if pick_inspector_enabled() {
            app.add_systems(Startup, setup_pick_inspector)
                .add_systems(Update, update_pick_inspector);
        }
    }
}

/// The env var that turns on the cursor pick inspector ([`update_pick_inspector`]).
const DEBUG_PICK_ENV: &str = "SL_VIEWER_DEBUG_PICK";

/// Whether the pick inspector is enabled — an internal debugging toggle, so an
/// env var rather than a CLI flag.
fn pick_inspector_enabled() -> bool {
    std::env::var_os(DEBUG_PICK_ENV).is_some()
}

/// Marker on the cursor-following pick-inspector text node.
#[derive(Component, Debug, Clone, Copy)]
struct PickInspector;

/// Spawn the pick-inspector overlay: a small text node that
/// [`update_pick_inspector`] moves to the cursor and rewrites each frame.
fn setup_pick_inspector(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        UiFont::Mono.at(13.0),
        TextColor(Color::srgb(0.4, 1.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        // The inspector must never itself occlude what it inspects.
        Pickable::IGNORE,
        GlobalZIndex(i32::MAX),
        PickInspector,
        Name::new("pick-inspector"),
    ));
}

/// Rewrite the pick inspector each frame with what a pick at the cursor would
/// hit: the name-tag hit, the UI-occlusion verdict, the HUD-occlusion
/// verdict, and the latest resolved GPU ID-buffer pick (requested at
/// ~[`crate::gpu_pick::PICK_HZ`] Hz while the inspector runs), so the failing
/// stage is visible without a click.
#[expect(
    clippy::too_many_arguments,
    reason = "a debug system reading everything a pick reads: the window, the HUD camera, \
              render layers, the hover map / pickables / node sizes for UI occlusion, the \
              name-tag hit test, the ray caster for the HUD test, the GPU pick queue + its \
              resolved channel, and the overlay node it writes"
)]
fn update_pick_inspector(
    time: Res<Time>,
    windows: Query<&Window>,
    hud_camera: Query<(&Camera, &GlobalTransform), With<HudCamera>>,
    layers: Query<(Entity, &RenderLayers)>,
    hover_map: Res<HoverMap>,
    pickables: Query<&Pickable>,
    tag_hit: NameTagHitTest,
    node_sizes: Query<&ComputedNode>,
    mut ray_cast: MeshRayCast,
    mut picker: ResMut<GpuPicker>,
    mut picks: MessageReader<GpuPickResolved>,
    mut last_pick: Local<Option<GpuPickResolved>>,
    mut since_pick: Local<f32>,
    mut inspector: Query<(&mut Node, &mut Text), With<PickInspector>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((mut node, mut text)) = inspector.single_mut() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    node.left = Val::Px(cursor.x + 16.0);
    node.top = Val::Px(cursor.y + 16.0);

    // Keep a rolling GPU pick alive and remember the newest answer.
    *since_pick += time.delta_secs();
    if *since_pick >= 1.0 / crate::gpu_pick::PICK_HZ {
        picker.request(cursor, PickPurpose::Inspector);
        *since_pick = 0.0;
    }
    for pick in picks.read() {
        if pick.purpose == PickPurpose::Inspector {
            *last_pick = Some(pick.clone());
        }
    }

    let ui_blocked = pointer_over_blocking_ui(&hover_map, &pickables, &node_sizes);
    let hud = pointer_over_hud(cursor, &hud_camera, &layers, &mut ray_cast);
    let mut lines = vec![
        format!("cursor {:.0},{:.0}", cursor.x, cursor.y),
        format!("UI blocked={ui_blocked}  HUD={hud}"),
        match tag_hit.agent_at(cursor) {
            Some(agent) => format!("tag (2d)→ {agent}"),
            None => "tag (2d)→ (none)".to_owned(),
        },
    ];
    match last_pick.as_ref() {
        Some(pick) => match pick.hit.as_ref() {
            Some(hit) => {
                let what = match hit.resolution {
                    PickResolution::Avatar { agent, worn: None } => format!("avatar {agent:?}"),
                    PickResolution::Avatar {
                        agent,
                        worn: Some(worn),
                    } => format!("avatar {agent:?} worn {worn:?}"),
                    PickResolution::ObjectFace { scoped, face, .. } => {
                        format!("object {scoped:?} face {}", face.get())
                    }
                    PickResolution::Terrain => "terrain".to_owned(),
                    PickResolution::Water => "water".to_owned(),
                };
                lines.push(format!(
                    "gpu→ {what} @ {:.1}m ({:.1},{:.1},{:.1})",
                    hit.distance, hit.world_point.x, hit.world_point.y, hit.world_point.z,
                ));
            }
            None => lines.push("gpu→ (nothing)".to_owned()),
        },
        None => lines.push("gpu→ (no pick yet)".to_owned()),
    }
    *text = Text::new(lines.join("\n"));
}

/// Resolve a world right-click to its context-menu target — an avatar's pie,
/// the in-world object pie ([`crate::object_menu`]), an attachment pie
/// ([`crate::attachment_menu`]), or the land pie ([`crate::land_menu`]) — and
/// ask for it.
///
/// Avatar resolution has two paths, matching the reference's "the name tag or
/// the avatar itself":
///
/// 1. **The name tag** — a world-space billboard whose on-screen bubble rect is
///    hit by the [`NameTagHitTest`] cursor-vs-rect test (no picking backend
///    covers the tag meshes). Checked first, and it wins even over the body
///    behind it.
/// 2. **The body / sphere** — no mesh-picking backend is installed (the viewer
///    raycasts on demand, like [`crate::hud_pick`]), so this casts a ray from the
///    world camera through the cursor and resolves it **mesh-accurately** via
///    the **GPU ID-buffer pick** ([`crate::gpu_pick`]): the avatar's drawn,
///    GPU-posed pixels decide, so a click just *off* an avatar's silhouette
///    picks nothing, matching the reference — and a click on an animated
///    avatar is pixel-accurate, morphs and physics included. A hit whose
///    pixel belongs to a **worn rigged submesh** resolves past the avatar to
///    the worn object, which gets the attachment pie (self vs other by the
///    wearer), as the reference dispatches (`lltoolpie.cpp` `isAttachment()`).
///
/// The same pick names an object face or bare terrain — the ID buffer's depth
/// test already arbitrated nearest-wins between them, so an object standing
/// in front of an avatar gets the object pie and vice versa; an object hit
/// that is itself a worn (rigid) attachment gets the attachment pie rather
/// than the object one; bare terrain opens the land pie
/// ([`crate::land_menu`]) — the reference's `PICK_LAND` outcome. The world
/// resolution is **asynchronous** (the readback arrives 1–2 frames after the
/// release): this system requests the pick, and [`resolve_right_click_pick`]
/// routes the answer. A **HUD** attachment under the cursor occludes the
/// world and resolves synchronously through its own orthographic ray to the
/// attachment-self pie.
///
/// It opens on the right-button **release** of a click, not the press: a right-
/// *drag* is camera free-look here, so the menu must not appear the moment a look
/// gesture starts. [`RIGHT_CLICK_DRAG_SLOP`] separates the two.
///
/// A right-click over a **blocking** UI element (an open floater) that is *not* a
/// name tag suppresses the pick, so a menu drawn over the world does not also open
/// an avatar or object pie behind it.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the mouse button \
              and motion plus the click/drag tracker, the hover map / pickables / node sizes for \
              the UI occlusion, the name-tag hit test, the window for the \
              cursor, the HUD camera plus render layers and the ray caster for the HUD \
              pick, the object picker for the HUD resolve, the GPU pick queue for the world \
              resolve, and the avatar / attachment open channels"
)]
fn request_avatar_menu_on_right_click(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut gesture: ResMut<RightClickGesture>,
    hover_map: Res<HoverMap>,
    pickables: Query<&Pickable>,
    tag_hit: NameTagHitTest,
    node_sizes: Query<&ComputedNode>,
    windows: Query<&Window>,
    hud_camera: Query<(&Camera, &GlobalTransform), With<HudCamera>>,
    layers: Query<(Entity, &RenderLayers)>,
    object_picker: ObjectPicker,
    mut picker: ResMut<GpuPicker>,
    mut ray_cast: MeshRayCast,
    mut requests: MessageWriter<OpenAvatarMenu>,
    mut attachment_requests: MessageWriter<OpenAttachmentMenu>,
) {
    // Track the gesture: a press starts it, motion accumulates, a release decides.
    if buttons.just_pressed(MouseButton::Right) {
        gesture.down = true;
        gesture.moved = 0.0;
    }
    if buttons.pressed(MouseButton::Right) {
        gesture.moved += motion.delta.length();
    }
    let was_click = buttons.just_released(MouseButton::Right)
        && gesture.down
        && gesture.moved <= RIGHT_CLICK_DRAG_SLOP;
    if buttons.just_released(MouseButton::Right) {
        gesture.down = false;
    }
    if !was_click {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    // 1. The name tag: the screen-space rect test against the visible tags.
    let tag_agent = tag_hit.agent_at(cursor);

    // Occlusion order: tag, then UI, then HUD attachments, then the world (the
    // reference's order too). The name tag above is the avatar's own overlay
    // and wins first.
    let agent = if let Some(agent) = tag_agent {
        Some(agent)
    } else if pointer_over_blocking_ui(&hover_map, &pickables, &node_sizes) {
        // A blocking UI surface (a floater, or the open pie's own ring) is under
        // the cursor: this click is for it, not for an avatar behind it. Passive
        // overlays (the chat heads-up) and non-UI / zero-area hover entries opt out
        // and do not suppress this.
        return;
    } else if pointer_over_hud(cursor, &hud_camera, &layers, &mut ray_cast) {
        // A HUD attachment is under the cursor: it occludes the world (so no
        // avatar or object pie opens behind it), and — only the agent's own
        // HUDs being routed to the screen and shown — it gets the
        // attachment-self pie, resolved through the same orthographic HUD ray
        // the left-click touch uses. A HUD hit that resolves to no tracked
        // object still consumes the click (occlusion).
        if let Ok((hud_cam, hud_transform)) = hud_camera.single()
            && let Ok(hud_ray) = hud_cam.viewport_to_world(hud_transform, cursor)
        {
            let hud_entities: HashSet<Entity> = layers
                .iter()
                .filter(|(_entity, layers)| on_hud_layer(Some(layers)))
                .map(|(entity, _layers)| entity)
                .collect();
            if let Some(hit) = object_picker.pick_hud(hud_ray, &mut ray_cast, &hud_entities) {
                attachment_requests.write(OpenAttachmentMenu {
                    summary: hit.summary,
                    surface: Some(hit.surface),
                    wearer: None,
                    hud: true,
                    at: cursor,
                });
            }
        }
        return;
    } else {
        // 3. The world: request the GPU ID-buffer pick at the cursor. The
        // depth test arbitrates avatar / object / terrain nearest-wins in the
        // pick view itself; `resolve_right_click_pick` routes the readback to
        // the right pie 1–2 frames later.
        picker.request(cursor, PickPurpose::RightClick);
        None
    };

    if let Some(agent) = agent {
        requests.write(OpenAvatarMenu { agent, at: cursor });
    }
}

/// Route a resolved right-click GPU pick to its pie: an avatar's own pixels to
/// the avatar pies, a worn rigged submesh past the avatar to the attachment
/// pies, an object face (surface-refined) to the object or attachment pies,
/// and bare terrain to the land pie — the same dispatch the synchronous
/// resolver used to run on ray casts.
pub(crate) fn resolve_right_click_pick(
    mut picks: MessageReader<GpuPickResolved>,
    object_picker: ObjectPicker,
    mut ray_cast: MeshRayCast,
    mut requests: MessageWriter<OpenAvatarMenu>,
    mut object_requests: MessageWriter<OpenObjectMenu>,
    mut attachment_requests: MessageWriter<OpenAttachmentMenu>,
    mut land_requests: MessageWriter<OpenLandMenu>,
) {
    for pick in picks.read() {
        if pick.purpose != PickPurpose::RightClick {
            continue;
        }
        let cursor = pick.cursor;
        let Some(hit) = pick.hit.as_ref() else {
            continue;
        };
        match hit.resolution {
            PickResolution::Avatar { agent, worn: None } => {
                requests.write(OpenAvatarMenu { agent, at: cursor });
            }
            PickResolution::Avatar {
                agent,
                worn: Some(worn),
            } => {
                // A worn rigged submesh resolves past the avatar to the worn
                // object (submesh → worn object → wearer) and opens the
                // attachment pie. The skinned pick carries no face / UV
                // surface, so Touch on this path goes without one. An
                // unresolvable worn object (already gone from the tracked
                // set) falls back to the wearer's avatar pie.
                match object_picker.summary_of(worn) {
                    Some(summary) => {
                        attachment_requests.write(OpenAttachmentMenu {
                            summary,
                            surface: None,
                            wearer: Some(agent),
                            hud: false,
                            at: cursor,
                        });
                    }
                    None => {
                        requests.write(OpenAvatarMenu { agent, at: cursor });
                    }
                }
            }
            PickResolution::ObjectFace { entity, .. } => {
                // Refine the pick against the one face the ID buffer named
                // (a single-entity ray test, not a scene walk) for the exact
                // struck surface — face index, ST/UV, position, normal.
                let Some(object) = object_picker.pick_entity(pick.ray, &mut ray_cast, entity)
                else {
                    continue;
                };
                if object.summary.attachment {
                    // A worn (rigid) attachment: the attachment pies, self vs
                    // other decided at open time by its wearer.
                    attachment_requests.write(OpenAttachmentMenu {
                        summary: object.summary,
                        surface: Some(object.surface),
                        wearer: None,
                        hud: false,
                        at: cursor,
                    });
                } else {
                    object_requests.write(OpenObjectMenu {
                        hit: object,
                        at: cursor,
                    });
                }
            }
            PickResolution::Terrain => {
                land_requests.write(OpenLandMenu {
                    at: cursor,
                    point: hit.world_point,
                });
            }
            PickResolution::Water => {}
        }
    }
}

/// Turn a resolved pick into an open pie: choose self vs other, snapshot the
/// conditions, and stash the target for the action handler.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the pick stream plus every piece of state a slice's \
              condition is read from (own identity, seat, ground-sit, friendship, \
              the target's standing render exception), and the two things written"
)]
fn open_avatar_menu(
    mut requests: MessageReader<OpenAvatarMenu>,
    identity: Res<SlIdentity>,
    parcel: Res<SlAgentParcel>,
    ground_sit: Res<SelfGroundSit>,
    friends: Res<FriendsModel>,
    exceptions: Res<crate::avatar_render_settings::AvatarRenderSettings>,
    mut target: ResMut<AvatarMenuTarget>,
    mut pies: MessageWriter<OpenPieMenu>,
) {
    for request in requests.read() {
        target.agent = Some(request.agent);
        let is_self = identity.agent_id == Some(request.agent);
        let (menu, conditions) = if is_self {
            // Exactly one of sitting / standing holds, so the Stand Up / Sit Down
            // pair shows one live slice and one greyed, whichever way round.
            // Sitting is either an object-sit (`seated_on`) or a ground-sit (the
            // viewer-tracked flag, since the session keeps no ground-sit state).
            let sitting = parcel.seated_on.is_some() || ground_sit.sitting;
            let condition = if sitting { SELF_SITTING } else { SELF_STANDING };
            (&AVATAR_SELF_PIE, vec![condition])
        } else {
            let mut conditions = Vec::new();
            if !friends.is_friend(request.agent) {
                conditions.push(TARGET_NOT_FRIEND);
            }
            // The Render sub-pie greys out the decision already in force for
            // this person, so the pie also *reads* their standing exception.
            conditions.extend(render_pie_conditions(exceptions.setting_of(request.agent)));
            (&AVATAR_OTHER_PIE, conditions)
        };
        pies.write(OpenPieMenu {
            menu,
            at: request.at,
            element: AVATAR_MENU_ELEMENT,
            conditions,
        });
    }
}

/// Dispatch a picked avatar-menu slice to the behaviour behind it.
///
/// Also accepts the **attachment** pies' element ([`ATTACHMENT_MENU_ELEMENT`]):
/// their avatar-derived slices (IM / Mute / Add as Friend acting on the wearer,
/// the sit / stand chain) declare the same action names, and
/// [`crate::attachment_menu`]'s opener stores the wearer in
/// [`AvatarMenuTarget`], so they run through exactly this code. The
/// attachment-specific actions (detach / drop / touch) fall through here and
/// are matched by the attachment module's own handler instead.
///
/// Only the actions this viewer can honour today are matched; every other slice
/// is a disabled placeholder that never emits, so the fall-through is the whole of
/// the not-yet-implemented set and is intentionally silent.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the picked target / avatar state and the several action \
              channels each wired slice dispatches on (command, conversation, profile, \
              tex-refresh, contact set)"
)]
fn handle_avatar_menu_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<AvatarMenuTarget>,
    avatars: Res<AvatarState>,
    mut ground_sit: ResMut<SelfGroundSit>,
    mut commands: MessageWriter<SlCommand>,
    mut blocks: MessageWriter<RequestBlock>,
    mut derenders: MessageWriter<RequestDerender>,
    mut exceptions: MessageWriter<crate::avatar_render_settings::RequestRenderException>,
    mut conversations: MessageWriter<OpenConversation>,
    mut profiles: MessageWriter<OpenAvatarProfile>,
    mut refetch: MessageWriter<RefetchAvatarTextures>,
    mut contact_sets: MessageWriter<crate::world_api::OpenAddToContactSet>,
    mut aliases: MessageWriter<crate::contact_sets_panel::OpenSetPseudonym>,
) {
    for action in actions.read() {
        if action.element != AVATAR_MENU_ELEMENT && action.element != ATTACHMENT_MENU_ELEMENT {
            continue;
        }
        let Some(agent) = target.agent else {
            continue;
        };
        // Derender (`viewer-derender-blacklist`) is dispatched here only for the
        // **avatar** pies: the attachment pies carry the same action names but
        // target the worn object, and `crate::attachment_menu` dispatches those.
        if matches!(action.action, "derender" | "derender-blacklist") {
            if action.element == AVATAR_MENU_ELEMENT {
                let name = avatars
                    .name_of(agent)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                derenders.write(RequestDerender::new(
                    agent.uuid(),
                    name,
                    DerenderKind::Resident,
                    action.action == "derender-blacklist",
                ));
            }
            continue;
        }
        // The per-avatar render exception (`viewer-avatar-complexity-limit`),
        // likewise only from the avatar pies — an attachment's pie addresses the
        // worn object, not its wearer.
        if let Some(over) = render_override_for(action.action) {
            if action.element == AVATAR_MENU_ELEMENT {
                exceptions.write(crate::avatar_render_settings::RequestRenderException {
                    agent,
                    name: avatars
                        .name_of(agent)
                        .map(ToOwned::to_owned)
                        .unwrap_or_default(),
                    setting: over,
                });
            }
            continue;
        }
        match action.action {
            "stand" => {
                ground_sit.sitting = false;
                commands.write(SlCommand(Command::Stand));
            }
            "sit-ground" => {
                ground_sit.sitting = true;
                commands.write(SlCommand(Command::SitOnGround));
            }
            "im" => {
                conversations.write(OpenConversation {
                    key: ConversationKey::Direct(agent),
                });
            }
            "profile" => {
                profiles.write(OpenAvatarProfile { agent });
            }
            "tex-refresh" => {
                refetch.write(RefetchAvatarTextures { agent });
            }
            "mute" => {
                let name = avatars
                    .name_of(agent)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                blocks.write(RequestBlock::new(agent.uuid(), name, MuteType::Agent));
            }
            "add-friend" => {
                commands.write(SlCommand(Command::OfferFriendship {
                    to_agent_id: agent,
                    message: String::new(),
                }));
            }
            // File this person under one of the user's own contact sets
            // (`viewer-contact-sets`) — the floater asks which, since the pie
            // cannot grow a slice per set.
            "add-to-set" if action.element == AVATAR_MENU_ELEMENT => {
                contact_sets.write(crate::world_api::OpenAddToContactSet::one(
                    agent,
                    avatars
                        .name_of(agent)
                        .map(ToOwned::to_owned)
                        .unwrap_or_default(),
                ));
            }
            // Give this person a name of the user's own
            // (`viewer-contact-set-pseudonyms`) — the prompt is raised where it
            // is answered, beside the sets it is stored with.
            "set-alias" if action.element == AVATAR_MENU_ELEMENT => {
                aliases.write(crate::contact_sets_panel::OpenSetPseudonym {
                    agent,
                    name: avatars
                        .name_of(agent)
                        .map(ToOwned::to_owned)
                        .unwrap_or_default(),
                });
            }
            // Every other slice is a disabled placeholder: no behaviour yet.
            _other => {}
        }
    }
}

/// Clear the tracked ground-sit once the avatar walks — a horizontal move stands
/// a ground-sitting avatar up, so the Sit Down / Stand Up enable must follow.
///
/// Only the horizontal movements matter: jump / fly (up / down) do not end a
/// ground sit the way stepping does.
fn clear_ground_sit_on_move(
    actions: Res<ButtonInput<Action>>,
    mut ground_sit: ResMut<SelfGroundSit>,
) {
    if !ground_sit.sitting {
        return;
    }
    if actions.any_pressed([
        Action::MoveForward,
        Action::MoveBackward,
        Action::MoveLeft,
        Action::MoveRight,
    ]) {
        ground_sit.sitting = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AVATAR_OTHER_PIE, AVATAR_SELF_PIE, SELF_SITTING, SELF_STANDING, TARGET_NOT_FRIEND,
        UNIMPLEMENTED,
    };
    use crate::pie_menu::{
        Compass, PieAddress, PieConditions, PieContent, PieMenuDef, ResolvedSlot, SlotOutcome,
        addresses, resolve_slots,
    };
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The two avatar pies, for the sweeps that must hold on both.
    const PIES: [&PieMenuDef; 2] = [&AVATAR_SELF_PIE, &AVATAR_OTHER_PIE];

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

    /// **The other-avatar pie's address table, pinned.**
    ///
    /// Moving any avatar action to a different compass path re-teaches every user
    /// who learned this menu with their hand; this table makes that a loud diff.
    /// If a move is intended, change the table in the same reviewed commit.
    #[test]
    fn other_avatar_pie_keeps_every_address() {
        let expected: Vec<(&str, Vec<Compass>)> = vec![
            ("profile", vec![Compass::East]),
            ("mute", vec![Compass::NorthEast, Compass::East]),
            (
                "mute-particles",
                vec![Compass::NorthEast, Compass::NorthEast],
            ),
            ("go-to", vec![Compass::North]),
            ("report", vec![Compass::NorthWest]),
            ("add-friend", vec![Compass::West, Compass::East]),
            ("add-to-set", vec![Compass::West, Compass::NorthEast]),
            ("set-alias", vec![Compass::West, Compass::North]),
            ("pay", vec![Compass::SouthWest]),
            ("freeze", vec![Compass::South, Compass::East]),
            ("give-card", vec![Compass::South, Compass::NorthEast]),
            ("invite-to-group", vec![Compass::South, Compass::North]),
            ("face-towards", vec![Compass::South, Compass::NorthWest]),
            ("eject", vec![Compass::South, Compass::West]),
            (
                "textures",
                vec![Compass::South, Compass::SouthWest, Compass::East],
            ),
            (
                "script-info",
                vec![Compass::South, Compass::SouthWest, Compass::NorthEast],
            ),
            (
                "call",
                vec![Compass::South, Compass::SouthWest, Compass::North],
            ),
            (
                "zoom-in",
                vec![Compass::South, Compass::SouthWest, Compass::NorthWest],
            ),
            (
                "tex-refresh",
                vec![Compass::South, Compass::SouthWest, Compass::West],
            ),
            (
                "render-fully",
                vec![Compass::South, Compass::South, Compass::East],
            ),
            (
                "render-normally",
                vec![Compass::South, Compass::South, Compass::North],
            ),
            (
                "render-never",
                vec![Compass::South, Compass::South, Compass::West],
            ),
            (
                "derender-blacklist",
                vec![Compass::South, Compass::SouthEast, Compass::South],
            ),
            (
                "derender",
                vec![Compass::South, Compass::SouthEast, Compass::SouthEast],
            ),
            ("im", vec![Compass::SouthEast]),
        ];
        assert_eq!(
            address_pairs(&AVATAR_OTHER_PIE),
            expected,
            "an other-avatar pie action moved — if intended, bless it by editing this table"
        );
    }

    /// **The self-avatar pie's address table, pinned.** As above, for the self pie.
    #[test]
    fn self_avatar_pie_keeps_every_address() {
        let expected: Vec<(&str, Vec<Compass>)> = vec![
            ("profile", vec![Compass::East]),
            ("groups", vec![Compass::NorthEast]),
            (
                "takeoff-shirt",
                vec![Compass::North, Compass::East, Compass::East],
            ),
            (
                "takeoff-pants",
                vec![Compass::North, Compass::East, Compass::NorthEast],
            ),
            (
                "takeoff-shoes",
                vec![Compass::North, Compass::East, Compass::North],
            ),
            (
                "takeoff-socks",
                vec![Compass::North, Compass::East, Compass::NorthWest],
            ),
            (
                "takeoff-jacket",
                vec![Compass::North, Compass::East, Compass::West],
            ),
            (
                "takeoff-gloves",
                vec![Compass::North, Compass::East, Compass::SouthWest],
            ),
            (
                "takeoff-skirt",
                vec![Compass::North, Compass::East, Compass::SouthEast],
            ),
            ("detach-hud", vec![Compass::North, Compass::NorthEast]),
            ("detach-attachment", vec![Compass::North, Compass::North]),
            ("detach-all", vec![Compass::North, Compass::NorthWest]),
            ("sit-ground", vec![Compass::NorthWest]),
            ("stand", vec![Compass::West]),
            ("script-info", vec![Compass::SouthWest]),
            ("gestures", vec![Compass::South]),
            ("edit-shape", vec![Compass::SouthEast, Compass::East]),
            (
                "reset-skel-anim",
                vec![Compass::SouthEast, Compass::NorthEast, Compass::East],
            ),
            (
                "reset-skeleton",
                vec![Compass::SouthEast, Compass::NorthEast, Compass::NorthEast],
            ),
            (
                "reset-mesh-lod",
                vec![Compass::SouthEast, Compass::NorthEast, Compass::North],
            ),
            ("tex-refresh", vec![Compass::SouthEast, Compass::North]),
            ("edit-outfit", vec![Compass::SouthEast, Compass::NorthWest]),
            ("dump-xml", vec![Compass::SouthEast, Compass::West]),
            ("hover-height", vec![Compass::SouthEast, Compass::SouthWest]),
            ("textures", vec![Compass::SouthEast, Compass::South]),
        ];
        assert_eq!(
            address_pairs(&AVATAR_SELF_PIE),
            expected,
            "a self-avatar pie action moved — if intended, bless it by editing this table"
        );
    }

    /// No pie in either tree declares two entries at one compass position — a
    /// silent overwrite whose winner would depend on declaration order.
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
        for pie in PIES {
            check(pie, &mut failures);
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The wired self actions are enabled exactly in the state they apply to, and
    /// each keeps its slot in both states.
    #[test]
    fn self_stand_and_sit_track_the_seated_state() -> Result<(), TestError> {
        // Sitting: Stand Up is live at west, Sit Down is disabled at north-west.
        let sitting = resolve_slots(&AVATAR_SELF_PIE, &PieConditions::new([SELF_SITTING]));
        let stand = slot_at(&sitting, Compass::West)?;
        assert_eq!(stand.outcome, SlotOutcome::Action("stand"));
        assert!(stand.enabled, "Stand Up must be enabled while sitting");
        assert!(
            !slot_at(&sitting, Compass::NorthWest)?.enabled,
            "Sit Down must be disabled while sitting"
        );

        // Standing: the reverse, at the same two positions.
        let standing = resolve_slots(&AVATAR_SELF_PIE, &PieConditions::new([SELF_STANDING]));
        assert!(
            slot_at(&standing, Compass::NorthWest)?.enabled,
            "Sit Down must be enabled while standing"
        );
        assert!(
            !slot_at(&standing, Compass::West)?.enabled,
            "Stand Up must be disabled while standing"
        );
        Ok(())
    }

    /// "Add as Friend" is enabled only when the target is not already a friend;
    /// "IM" is always available on another avatar.
    #[test]
    fn other_add_friend_tracks_friendship_and_im_is_always_live() -> Result<(), TestError> {
        // A stranger: Add as Friend is live.
        let stranger = resolve_slots(
            &super::OTHER_ADD_PIE,
            &PieConditions::new([TARGET_NOT_FRIEND]),
        );
        let add = slot_at(&stranger, Compass::East)?;
        assert_eq!(add.outcome, SlotOutcome::Action("add-friend"));
        assert!(add.enabled, "Add as Friend must be live for a stranger");

        // A friend (no condition held): Add as Friend is disabled, keeps its slot.
        let friend = resolve_slots(&super::OTHER_ADD_PIE, &PieConditions::default());
        assert!(
            !slot_at(&friend, Compass::East)?.enabled,
            "Add as Friend must be disabled for a friend"
        );

        // IM is unconditional on the other-avatar root.
        let other = resolve_slots(&AVATAR_OTHER_PIE, &PieConditions::default());
        let im = slot_at(&other, Compass::SouthEast)?;
        assert_eq!(im.outcome, SlotOutcome::Action("im"));
        assert!(im.enabled, "IM must always be available on another avatar");
        Ok(())
    }

    /// The Render sub-pie shows which standing exception is in force by greying
    /// out that slice: with no exception only "Normally" is dead, and with one
    /// the slice that would re-decide the same way is.
    #[test]
    fn render_slices_grey_out_the_decision_in_force() -> Result<(), TestError> {
        use crate::avatar_complexity::RenderOverride;

        let live = |setting: RenderOverride, at: Compass| -> Result<bool, TestError> {
            let conditions = PieConditions::new(super::render_pie_conditions(setting));
            let slots = resolve_slots(&super::OTHER_RENDER_PIE, &conditions);
            Ok(slot_at(&slots, at)?.enabled)
        };

        // No exception: Fully and Never are the two things you can decide;
        // "Normally" would clear an exception that is not there.
        assert!(live(RenderOverride::Normal, Compass::East)?);
        assert!(live(RenderOverride::Normal, Compass::West)?);
        assert!(!live(RenderOverride::Normal, Compass::North)?);

        // Pinned to full detail: Fully is the state, not an action.
        assert!(!live(RenderOverride::AlwaysFull, Compass::East)?);
        assert!(live(RenderOverride::AlwaysFull, Compass::North)?);
        assert!(live(RenderOverride::AlwaysFull, Compass::West)?);

        // Pinned to the jellydoll: likewise for Never.
        assert!(live(RenderOverride::Never, Compass::East)?);
        assert!(live(RenderOverride::Never, Compass::North)?);
        assert!(!live(RenderOverride::Never, Compass::West)?);
        Ok(())
    }

    /// In the live viewer's actual state (the sentinel [`UNIMPLEMENTED`] is never
    /// supplied), every placeholder keeps its slot but reads disabled — so the
    /// reference menu shape is present even before the features are.
    #[test]
    fn unimplemented_entries_are_disabled_but_present() -> Result<(), TestError> {
        // No conditions held except standing (so Sit/Stand resolve) — the shape
        // the live viewer opens the other pie in has no sentinel.
        let other = resolve_slots(&AVATAR_OTHER_PIE, &PieConditions::default());
        let go_to = slot_at(&other, Compass::North)?;
        assert_eq!(go_to.outcome, SlotOutcome::Action("go-to"));
        assert!(
            !go_to.enabled,
            "Go To is a placeholder and must read disabled until it is wired"
        );
        // And it is not alone: none of the sentinel-gated leaves are enabled.
        assert!(
            !slot_at(&other, Compass::NorthWest)?.enabled,
            "Report is a placeholder and must read disabled"
        );
        assert!(
            !slot_at(&other, Compass::SouthWest)?.enabled,
            "Pay is a placeholder and must read disabled"
        );
        // Profile went live with the profile floater: unconditional, like IM.
        let profile = slot_at(&other, Compass::East)?;
        assert_eq!(profile.outcome, SlotOutcome::Action("profile"));
        assert!(
            profile.enabled,
            "Profile is wired to the profile floater and must be enabled"
        );
        // The proof that the sentinel is what disables them: hold it, and they
        // light up. The live viewer never does this.
        let held = resolve_slots(&AVATAR_OTHER_PIE, &PieConditions::new([UNIMPLEMENTED]));
        assert!(
            slot_at(&held, Compass::North)?.enabled,
            "holding the sentinel proves it is the only thing gating the placeholder"
        );
        Ok(())
    }
}
