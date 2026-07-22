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
//! viewer does not have yet (a profile floater, a pay dialog, an abuse report,
//! outfit editing, the moderation powers). Those are declared **in their
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
//!   ([`crate::conversations::OpenConversation`]), exactly as the People list's IM
//!   action does.
//! - **Stand Up / Sit Down** (self) → [`Command::Stand`] / [`Command::SitOnGround`],
//!   each enabled only in the state where it makes sense (you cannot stand up
//!   unless you are sitting), gated on [`SELF_SITTING`] / [`SELF_STANDING`].
//! - **Mute** (other) → [`Command::Mute`] the picked agent.
//! - **Add as Friend** (other) → [`Command::OfferFriendship`], disabled via
//!   [`TARGET_NOT_FRIEND`] when the agent already is a friend, matching the
//!   reference's `on_enable`.
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
//! floating name tag — carries [`crate::avatars::AvatarPickTarget`] with the
//! avatar's agent id. [`request_avatar_menu_on_right_click`] resolves a
//! right-click to an agent two ways, mirroring the reference's "name tag or the
//! avatar itself": a UI hit on the name tag (through the hover map) or the
//! mesh-accurate world pick ([`crate::avatar_pick::AvatarPicker`]) against the
//! avatar's **posed** geometry. That same picker is what a future **inventory
//! drag-and-drop onto an avatar** will reuse to find its drop target, which is
//! why the identity lives on the entities rather than in a menu-only lookup.
//!
//! Reference (Firestorm, read-only): `menu_pie_avatar_self.xml`,
//! `menu_pie_avatar_other.xml` (the compass positions), and
//! `newview/llviewermenu.cpp` (the action handlers).

use std::collections::HashSet;

use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, Command, MuteFlags, MuteType, SlAgentParcel, SlCommand, SlIdentity,
};

use crate::attachment_menu::{ATTACHMENT_MENU_ELEMENT, OpenAttachmentMenu};
use crate::avatar_pick::{AvatarPicker, PickAccuracy};
use crate::avatars::{AvatarPickTarget, AvatarState};
use crate::camera::ViewerCamera;
use crate::conversations::{ConversationKey, OpenConversation};
use crate::hud::{HudCamera, on_hud_layer};
use crate::hud_pick::{pointer_over_blocking_ui, pointer_over_hud};
use crate::input_action::Action;
use crate::object_menu::{ObjectPicker, OpenObjectMenu};
use crate::people::FriendsModel;
use crate::pie_menu::{Compass, OpenPieMenu, PieAction, PieContent, PieEntry, PieMenuDef};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;

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

/// The condition marking a slice whose feature does not exist yet.
///
/// It is **never** pushed into the live condition set, so any entry gated on it
/// always resolves disabled — the "declared in its reference place, but not yet
/// pickable" state. Replacing an entry's `when` with a real condition (or `None`)
/// is how a slice goes live, in a single deliberate edit that leaves its address
/// untouched.
pub(crate) const UNIMPLEMENTED: &str = "unimplemented";

/// Holds when the local avatar is **sitting** — enables self "Stand Up".
pub(crate) const SELF_SITTING: &str = "self-sitting";

/// Holds when the local avatar is **standing** — enables self "Sit Down".
pub(crate) const SELF_STANDING: &str = "self-standing";

/// Holds when the picked agent is **not already a friend** — enables "Add as
/// Friend", matching the reference's `Avatar.EnableAddFriend`.
pub(crate) const TARGET_NOT_FRIEND: &str = "target-not-friend";

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
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "More >" sub-pie of the other-avatar pie (reference slot 6 / south).
///
/// Populated from the reference `More >`'s own first level (Freeze, Give Card,
/// Invite to Group, Face towards, Eject); the reference's deeper debug / derender
/// tails are deferred rather than reproduced as dead slices (see the module doc).
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
                when: Some(UNIMPLEMENTED),
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
                when: Some(UNIMPLEMENTED),
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
                when: Some(UNIMPLEMENTED),
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

/// Tracks whether the local avatar is **ground-sitting**.
///
/// The session records object-sits ([`SlAgentParcel::seated_on`]) but keeps *no*
/// ground-sit state — `sit_on_ground` sends only a transient control bit, so
/// there is nothing on the wire to read back. The viewer therefore tracks it
/// here: set when this menu sends Sit Down, cleared when it sends Stand Up or the
/// avatar walks (which stands it up). Best-effort — a ground sit begun or ended
/// by something other than this menu or ordinary locomotion is not observed, and
/// the worst case is a momentarily wrong Stand Up / Sit Down enable that the next
/// sit / stand / step corrects.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub(crate) struct SelfGroundSit {
    /// Whether the local avatar is currently sitting on the ground.
    pub(crate) sitting: bool,
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

/// Rewrite the pick inspector each frame with what a pick at the cursor would hit:
/// the UI-occlusion verdict, the HUD-occlusion verdict, the nearest world-ray
/// hit, and the resolved mesh-accurate avatar pick, so the failing stage is
/// visible without a click.
#[expect(
    clippy::too_many_arguments,
    reason = "a debug system reading everything a pick reads: the window, the world and HUD \
              cameras, render layers, the hover map / pickables / node sizes for UI occlusion, the \
              name / parent / avatar-target queries to describe a hit, the ray caster, the avatar \
              picker, and the overlay node it writes"
)]
fn update_pick_inspector(
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    hud_camera: Query<(&Camera, &GlobalTransform), With<HudCamera>>,
    layers: Query<(Entity, &RenderLayers)>,
    hover_map: Res<HoverMap>,
    pickables: Query<&Pickable>,
    node_sizes: Query<&ComputedNode>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    pick_targets: Query<&AvatarPickTarget>,
    picker: AvatarPicker,
    mut ray_cast: MeshRayCast,
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

    // The nearest named ancestor of an entity, to describe a hit readably.
    let named = |mut entity: Entity| -> String {
        for _step in 0..12 {
            if let Ok(name) = names.get(entity) {
                return name.as_str().to_owned();
            }
            match parents.get(entity) {
                Ok(parent) => entity = parent.parent(),
                Err(_e) => break,
            }
        }
        format!("{entity:?}")
    };

    let ui_blocked = pointer_over_blocking_ui(&hover_map, &pickables, &node_sizes);
    let hud = pointer_over_hud(cursor, &hud_camera, &layers, &mut ray_cast);
    let mut lines = vec![
        format!("cursor {:.0},{:.0}", cursor.x, cursor.y),
        format!("UI blocked={ui_blocked}  HUD={hud}"),
    ];
    if let Ok((camera, camera_transform)) = camera.single()
        && let Ok(ray) = camera.viewport_to_world(camera_transform, cursor)
    {
        let any_settings = MeshRayCastSettings::default()
            .with_visibility(bevy::picking::mesh_picking::ray_cast::RayCastVisibility::Any);
        match ray_cast.cast_ray(ray, &any_settings).first() {
            Some((entity, hit)) => lines.push(format!(
                "world→ {} @ {:.1}m avatar={}",
                named(*entity),
                hit.distance,
                pick_targets.contains(*entity)
            )),
            None => lines.push("world→ (nothing)".to_owned()),
        }
        // The real avatar resolution: the mesh-accurate pick against the posed
        // geometry (with its box fallback), exactly what a right-click uses.
        match picker.pick(ray) {
            Some(hit) => {
                let accuracy = match hit.accuracy {
                    PickAccuracy::Mesh => "mesh",
                    PickAccuracy::BoxFallback => "box",
                };
                lines.push(format!(
                    "avatar→ {:?} @ {:.1}m ({accuracy})",
                    hit.agent, hit.distance
                ));
            }
            None => lines.push("avatar→ (none)".to_owned()),
        }
    }
    *text = Text::new(lines.join("\n"));
}

/// Resolve a world right-click to its context-menu target — an avatar's pie,
/// the in-world object pie ([`crate::object_menu`]), or an attachment pie
/// ([`crate::attachment_menu`]) — and ask for it.
///
/// Avatar resolution has two paths, matching the reference's "the name tag or
/// the avatar itself":
///
/// 1. **The name tag** — a `bevy_ui` node, so a hit shows up in the [`HoverMap`].
///    Checked first, and it wins even over the body behind it.
/// 2. **The body / sphere** — no mesh-picking backend is installed (the viewer
///    raycasts on demand, like [`crate::hud_pick`]), so this casts a ray from the
///    world camera through the cursor and resolves it **mesh-accurately** via
///    [`AvatarPicker`]: the avatar's posed, CPU-skinned triangles decide (the
///    fitted box only stands in for an avatar with no visible decoded geometry
///    yet), so a click just *off* an avatar's silhouette picks nothing, matching
///    the reference. A hit whose nearest triangle belongs to a **worn rigged
///    submesh** resolves past the avatar to the worn object, which gets the
///    attachment pie (self vs other by the wearer), as the reference dispatches
///    (`lltoolpie.cpp` `isAttachment()`).
///
/// The same world ray is also resolved to an in-world object
/// ([`ObjectPicker`]); when both an avatar and an object are hit, the **nearer**
/// wins, so an object standing in front of an avatar gets the object pie and
/// vice versa — and an object hit that is itself a worn (rigid) attachment gets
/// the attachment pie rather than the object one. A **HUD** attachment under
/// the cursor occludes the world and resolves through its own orthographic ray
/// to the attachment-self pie.
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
              the UI occlusion + name-tag path, the avatar-identity query, the window for the \
              cursor, the world and HUD cameras plus render layers and the ray caster for the HUD \
              pick, the mesh-accurate avatar picker and the object picker for the world \
              pick, and the three open-request channels"
)]
fn request_avatar_menu_on_right_click(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut gesture: ResMut<RightClickGesture>,
    hover_map: Res<HoverMap>,
    pickables: Query<&Pickable>,
    pick_targets: Query<&AvatarPickTarget>,
    node_sizes: Query<&ComputedNode>,
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    hud_camera: Query<(&Camera, &GlobalTransform), With<HudCamera>>,
    layers: Query<(Entity, &RenderLayers)>,
    picker: AvatarPicker,
    object_picker: ObjectPicker,
    mut ray_cast: MeshRayCast,
    // The three open-request channels, tupled into one system param to stay
    // within Bevy's per-system parameter limit.
    requests: (
        MessageWriter<OpenAvatarMenu>,
        MessageWriter<OpenObjectMenu>,
        MessageWriter<OpenAttachmentMenu>,
    ),
) {
    let (mut requests, mut object_requests, mut attachment_requests) = requests;
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

    // 1. The name tag: a hovered UI node carrying the avatar identity.
    let tag_agent = hover_map
        .values()
        .flat_map(|hits| hits.keys())
        .find_map(|entity| pick_targets.get(*entity).ok().map(AvatarPickTarget::agent));

    // Occlusion order: UI, then HUD attachments, then the world (the reference's
    // order too). The name tag above is the avatar's own UI and wins first.
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
        // 3. The world: resolve the ray at the cursor against both candidate
        // targets — the avatars' **posed** geometry (mesh-accurate, with the
        // fitted box only as the no-geometry fallback — see `crate::avatar_pick`)
        // and the in-world objects (`crate::object_menu`) — and let the nearer
        // hit win.
        let Ok((camera, camera_transform)) = camera.single() else {
            return;
        };
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
            return;
        };
        let avatar_hit = picker.pick(ray);
        // The object ray must not strike HUD geometry: a HUD is screen-space
        // (and has already had its chance to occlude above).
        let hud_entities: HashSet<Entity> = layers
            .iter()
            .filter(|(_entity, layers)| on_hud_layer(Some(layers)))
            .map(|(entity, _layers)| entity)
            .collect();
        let object_hit = object_picker.pick(ray, &mut ray_cast, &hud_entities);
        match (avatar_hit, object_hit) {
            (avatar, Some(object))
                if avatar
                    .as_ref()
                    .is_none_or(|avatar| object.distance < avatar.distance) =>
            {
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
                None
            }
            (Some(avatar), _object) => match avatar.worn {
                // The nearest posed triangle belongs to a worn rigged submesh:
                // resolve past the avatar to the worn object (submesh → worn
                // object → wearer) and open the attachment pie. The CPU-skinned
                // pick carries no face / UV surface, so Touch on this path goes
                // without one. An unresolvable worn object (already gone from
                // the tracked set) falls back to the wearer's avatar pie.
                Some(worn) => match object_picker.summary_of(worn) {
                    Some(summary) => {
                        attachment_requests.write(OpenAttachmentMenu {
                            summary,
                            surface: None,
                            wearer: Some(avatar.agent),
                            hud: false,
                            at: cursor,
                        });
                        None
                    }
                    None => Some(avatar.agent),
                },
                None => Some(avatar.agent),
            },
            // Nothing hit. (`(None, Some(_))` is impossible — with no avatar
            // hit the guard on the first arm always admits the object — but the
            // guard does not count towards exhaustiveness.)
            (None, _object) => None,
        }
    };

    if let Some(agent) = agent {
        requests.write(OpenAvatarMenu { agent, at: cursor });
    }
}

/// Turn a resolved pick into an open pie: choose self vs other, snapshot the
/// conditions, and stash the target for the action handler.
fn open_avatar_menu(
    mut requests: MessageReader<OpenAvatarMenu>,
    identity: Res<SlIdentity>,
    parcel: Res<SlAgentParcel>,
    ground_sit: Res<SelfGroundSit>,
    friends: Res<FriendsModel>,
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
fn handle_avatar_menu_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<AvatarMenuTarget>,
    avatars: Res<AvatarState>,
    mut ground_sit: ResMut<SelfGroundSit>,
    mut commands: MessageWriter<SlCommand>,
    mut conversations: MessageWriter<OpenConversation>,
) {
    for action in actions.read() {
        if action.element != AVATAR_MENU_ELEMENT && action.element != ATTACHMENT_MENU_ELEMENT {
            continue;
        }
        let Some(agent) = target.agent else {
            continue;
        };
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
            "mute" => {
                let name = avatars
                    .name_of(agent)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                commands.write(SlCommand(Command::Mute {
                    id: agent.uuid(),
                    name,
                    mute_type: MuteType::Agent,
                    flags: MuteFlags::default(),
                }));
            }
            "add-friend" => {
                commands.write(SlCommand(Command::OfferFriendship {
                    to_agent_id: agent,
                    message: String::new(),
                }));
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
            ("pay", vec![Compass::SouthWest]),
            ("freeze", vec![Compass::South, Compass::East]),
            ("give-card", vec![Compass::South, Compass::NorthEast]),
            ("invite-to-group", vec![Compass::South, Compass::North]),
            ("face-towards", vec![Compass::South, Compass::NorthWest]),
            ("eject", vec![Compass::South, Compass::West]),
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

    /// In the live viewer's actual state (the sentinel [`UNIMPLEMENTED`] is never
    /// supplied), every placeholder keeps its slot but reads disabled — so the
    /// reference menu shape is present even before the features are.
    #[test]
    fn unimplemented_entries_are_disabled_but_present() -> Result<(), TestError> {
        // No conditions held except standing (so Sit/Stand resolve) — the shape
        // the live viewer opens the other pie in has no sentinel.
        let other = resolve_slots(&AVATAR_OTHER_PIE, &PieConditions::default());
        let profile = slot_at(&other, Compass::East)?;
        assert_eq!(profile.outcome, SlotOutcome::Action("profile"));
        assert!(
            !profile.enabled,
            "Profile is a placeholder and must read disabled until it is wired"
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
        // The proof that the sentinel is what disables them: hold it, and they
        // light up. The live viewer never does this.
        let held = resolve_slots(&AVATAR_OTHER_PIE, &PieConditions::new([UNIMPLEMENTED]));
        assert!(
            slot_at(&held, Compass::East)?.enabled,
            "holding the sentinel proves it is the only thing gating the placeholder"
        );
        Ok(())
    }
}
