//! The **worn attachment** context / pie menus (`viewer-attachment-context-menu`,
//! `viewer-hud-context-menu`): the entry trees offered when a worn object — in
//! world on an avatar, or a HUD on the screen — is the pick target, and the
//! dispatch of each entry.
//!
//! This is the *entries*, not the widget — the radial widget is
//! [`crate::pie_menu`], and this module declares two [`PieMenuDef`] trees plus
//! the systems that open and act on them, following [`crate::avatar_menu`] /
//! [`crate::object_menu`]. The reference viewer has a **distinct pie per
//! wearer**: `menu_pie_attachment_self.xml` for an attachment you wear,
//! `menu_pie_attachment_other.xml` for one worn by someone else — and, notably,
//! **no separate HUD menu**: a right-click on any *own* attachment, HUD
//! included, shows the attachment-self pie (`lltoolpie.cpp`: `isAttachment()` →
//! `gPieMenuAttachmentSelf`), so one tree here serves both roadmap tasks. The
//! pie XMLs are shared by every skin (Vintage overrides none), so
//! `default/xui/en/` is authoritative; reference slice order maps East →
//! NorthEast → … → SouthEast as everywhere else.
//!
//! # What is wired, and what is a disabled placeholder
//!
//! Placeholders follow the established pattern: declared **in their reference
//! compass positions but disabled**, gated on the never-supplied
//! [`UNIMPLEMENTED`] sentinel, so the shape (the muscle memory) is laid down
//! now and each slice lights up when its feature lands. Wired for real on the
//! **self** pie:
//!
//! - **Detach** → [`Command::DetachObjects`] on the attachment root — the worn
//!   object goes back to inventory. Always live: the pick reaching this pie
//!   guarantees an own worn object (the reference's `Attachment.EnableDetach`
//!   RLV refinements come with RLV).
//! - **Drop** → [`Command::DropAttachments`] on the attachment root — the worn
//!   object is dropped in world. Enabled only for an in-world attachment
//!   ([`TARGET_DROPPABLE`]): a HUD has no world position to drop at, which is
//!   the core of the reference's `Attachment.EnableDrop` (its per-item no-drop
//!   permission refinement comes with inventory-permission wiring).
//! - **Touch** (in `More >`) → [`Command::TouchObject`] on the picked prim,
//!   with the ray's [`SurfaceInfo`] when the pick produced one (a HUD or rigid
//!   attachment ray hit does; the CPU-skinned rigged pick carries no surface,
//!   so a touch there goes without one), enabled via the shared
//!   [`TARGET_TOUCHABLE`] flag gate.
//! - **Sit Here / Stand Up** → the reference's autohide chain
//!   ([`PieContent::Chain`]) at one position, dispatching the avatar pie's own
//!   ground-sit / stand actions (`Self.SitDown` in the reference is the ground
//!   sit).
//!
//! On the **other** pie the avatar-derived slices — **IM**, **Mute >**, **Add
//! as Friend** — act on the *wearer* and reuse the avatar pie's dispatch
//! wholesale: this module's opener stores the wearer in
//! [`AvatarMenuTarget`] and the shared handler in [`crate::avatar_menu`]
//! accepts this menu's element, so those actions run through exactly the code
//! the avatar pies use.
//!
//! # Where we depart from the reference, on purpose
//!
//! - **The other pie's deep `More >` tails are reproduced in full** (Freeze /
//!   Give Card / … / the Textures / Call / Display tails), pinning their
//!   addresses as placeholders — the [`crate::object_menu`] convention. The
//!   *avatar*-other pie earlier chose to stop at the first `More >` level; the
//!   two will be reconciled by the planned reorder tasks
//!   (`viewer-attachment-menu-reorder-when-implemented`).
//! - **Shared sub-pies are shared.** The reset tail and the mute pair are
//!   byte-identical to the avatar pies', so the [`SELF_RESET_PIE`] /
//!   [`OTHER_MUTE_PIE`] statics are reused rather than redeclared; their
//!   addresses are still pinned per-tree by this module's tests.
//!
//! # How a pick reaches here
//!
//! [`crate::avatar_menu`]'s right-click resolver owns the gesture and the
//! occlusion order (UI, then HUD, then world) and routes three pick shapes to
//! [`OpenAttachmentMenu`]:
//!
//! - a **HUD** hit — the orthographic HUD-camera ray resolved through
//!   [`ObjectPicker::pick_hud`](crate::object_menu::ObjectPicker) (only own
//!   HUDs are shown, so always the self pie);
//! - a **rigid worn attachment** in world — the object ray's
//!   [`ObjectPickSummary`] says `attachment`;
//! - a **worn rigged submesh** — the mesh-accurate avatar pick landed on a
//!   piece tagged [`WornPickTarget`](crate::objects::WornPickTarget)
//!   (submesh → worn object → wearer).
//!
//! Self vs other is decided by the wearer, as `lltoolpie.cpp` does
//! (`isAttachment()` + ownership → `gPieMenuAttachmentSelf` /
//! `gMenuAttachmentOther`).
//!
//! Reference (Firestorm, read-only): `menu_pie_attachment_self.xml`,
//! `menu_pie_attachment_other.xml` (the compass positions), `lltoolpie.cpp`
//! (the dispatch), `llviewermenu.cpp` (the handlers).

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, Command, SlAgentParcel, SlCommand, SlIdentity, SurfaceInfo};

use crate::avatar_menu::{
    AvatarMenuTarget, OTHER_MUTE_PIE, SELF_RESET_PIE, SELF_SITTING, SELF_STANDING, SelfGroundSit,
    TARGET_NOT_FRIEND, UNIMPLEMENTED,
};
use crate::avatars::AvatarState;
use crate::object_menu::TARGET_TOUCHABLE;
use crate::objects::ObjectPickSummary;
use crate::people::FriendsModel;
use crate::pie_menu::{Compass, OpenPieMenu, PieAction, PieContent, PieEntry, PieMenuDef};
use crate::ui_element::UiAction;

/// The `element` both attachment pies attribute their [`UiAction`]s to.
///
/// One tag for self *and* other, as the avatar pies do — and the shared avatar
/// handler ([`crate::avatar_menu`]) accepts this element too, so the
/// avatar-derived slices (IM / Mute / Add as Friend on the other pie, the sit /
/// stand chain on the self pie) dispatch through the existing avatar code with
/// the wearer stored in [`AvatarMenuTarget`].
pub(crate) const ATTACHMENT_MENU_ELEMENT: &str = "attachment-menu";

/// Holds when the picked attachment can be **dropped** into the world — it is
/// worn on the body, not on a HUD point (a HUD is screen-space and has no world
/// position to drop at). The core of the reference's `Attachment.EnableDrop`;
/// its per-item no-drop permission refinement comes with inventory-permission
/// wiring.
pub(crate) const TARGET_DROPPABLE: &str = "target-droppable";

// ---------------------------------------------------------------------------
// The "self attachment" pie. Top level matches menu_pie_attachment_self.xml:
// Profile, Drop, More>, Sit Here/Stand Up (autohide chain), Detach, Gestures,
// Appearance>, Edit (reference slots 0..7 → compass East..SouthEast).
// ---------------------------------------------------------------------------

/// The "Derender >" sub-pie of the self `More >` level. The reference declares
/// three leading separators, so its two slices sit at north-west and west.
static SELF_DERENDER_PIE: PieMenuDef = PieMenuDef {
    label: "Derender",
    entries: &[
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Temporary",
                action: "derender",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Blacklist",
                action: "derender-blacklist",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Save As >" sub-pie of the self `More >` level (Backup / Collada).
static SELF_SAVE_AS_PIE: PieMenuDef = PieMenuDef {
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
    ],
};

/// The self "More >" sub-pie (reference slot 2 / north): the touch / tex-refresh
/// / script-info / derender / export / inventory / inspect / textures tail.
static SELF_MORE_PIE: PieMenuDef = PieMenuDef {
    label: "More",
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
                label: "Script Info",
                action: "script-info",
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
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::SubPie(&SELF_DERENDER_PIE),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::SubPie(&SELF_SAVE_AS_PIE),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Show in Inv.",
                action: "show-in-inventory",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Inspect",
                action: "inspect",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Textures",
                action: "textures",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The self "Appearance >" sub-pie (reference slot 6 / south).
///
/// Deliberately **not** the avatar-self pie's appearance tree: the attachment
/// XML lays the same actions out differently (separators at north / north-west
/// / south-west; Dump XML at south, Hover Height at south-east) and omits the
/// Texture Refresh / Textures slices, so this pins the attachment layout.
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
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Edit Outfit",
                action: "edit-outfit",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Dump XML",
                action: "dump-xml",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Hover Height",
                action: "hover-height",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The pie for an attachment **you** wear — in world or on a HUD point. See
/// `menu_pie_attachment_self.xml`.
pub(crate) static ATTACHMENT_SELF_PIE: PieMenuDef = PieMenuDef {
    label: "Attachment",
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
                label: "Drop",
                action: "drop",
                when: Some(TARGET_DROPPABLE),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::SubPie(&SELF_MORE_PIE),
        },
        // The reference's Sit Here / Stand Up autohide chain: one position, two
        // mutually exclusive candidates, whichever applies to the seated state
        // (unlike the avatar-self pie, whose reference declares them as two
        // fixed slices).
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Chain(&[
                PieAction {
                    label: "Sit Here",
                    action: "sit-ground",
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
            content: PieContent::Action(PieAction {
                label: "Detach",
                action: "detach",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Gestures",
                action: "gestures",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&SELF_APPEARANCE_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Edit",
                action: "edit",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

// ---------------------------------------------------------------------------
// The "attachment on another avatar" pie. Top level matches
// menu_pie_attachment_other.xml: Profile, Mute>, Go to, Report, Add>, Pay,
// More>, IM (reference slots 0..7 → compass East..SouthEast) — the avatar-other
// shape plus the object-ish tails in its More levels.
// ---------------------------------------------------------------------------

/// The "Add >" sub-pie of the other pie. The reference declares **four**
/// leading separators here (unlike the avatar-other pie's two-slice layout), so
/// As Friend sits at west and To Set at south-west.
static OTHER_ADD_PIE: PieMenuDef = PieMenuDef {
    label: "Add",
    entries: &[
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Add as Friend",
                action: "add-friend",
                when: Some(TARGET_NOT_FRIEND),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Add to Set",
                action: "add-to-set",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Save As >" sub-pie of the other pie's second `More >` level (Backup /
/// Collada, then Dump XML after the reference's four separators).
static OTHER_SAVE_AS_PIE: PieMenuDef = PieMenuDef {
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
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Dump XML",
                action: "dump-xml",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The "Display >" sub-pie of the other pie's second `More >` level: the
/// wearer's impostor render mode (Normally / Never / Fully).
static OTHER_DISPLAY_PIE: PieMenuDef = PieMenuDef {
    label: "Display",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "Normally",
                action: "render-normally",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Never",
                action: "render-never",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Fully",
                action: "render-fully",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The second "More >" level of the other pie (the reference's "Avatar Pie
/// More 2"): the debug / call / zoom / reset / export / display tails.
static OTHER_MORE2_PIE: PieMenuDef = PieMenuDef {
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
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::SubPie(&SELF_RESET_PIE),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&OTHER_SAVE_AS_PIE),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::SubPie(&OTHER_DISPLAY_PIE),
        },
    ],
};

/// The "Derender >" sub-pie of the other pie's first `More >` level. The
/// reference declares **six** leading separators, so Blacklist sits at south
/// and Temporary at south-east.
static OTHER_DERENDER_PIE: PieMenuDef = PieMenuDef {
    label: "Derender",
    entries: &[
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Blacklist",
                action: "derender-blacklist",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Temporary",
                action: "derender",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

/// The first "More >" level of the other pie (the reference's "Avatar Pie
/// More 1"): the moderation slices plus the nested second level, Inspect, and
/// the derender tail.
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
            content: PieContent::SubPie(&OTHER_MORE2_PIE),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Inspect",
                action: "inspect",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::SubPie(&OTHER_DERENDER_PIE),
        },
    ],
};

/// The pie for an attachment worn by **another** avatar. See
/// `menu_pie_attachment_other.xml` (whose root the reference itself names
/// "Avatar Pie" — it is the avatar-other shape plus the object-ish tails).
pub(crate) static ATTACHMENT_OTHER_PIE: PieMenuDef = PieMenuDef {
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
// The widget-facing wiring: open request → open pie → dispatch.
// ---------------------------------------------------------------------------

/// A resolved request to open an attachment pie at screen point `at`.
///
/// Written by the shared right-click resolver in [`crate::avatar_menu`] for any
/// of the three attachment pick shapes (HUD ray, rigid world attachment, worn
/// rigged submesh — see the module doc), and consumed by
/// [`open_attachment_menu`], which decides self vs other by the wearer.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenAttachmentMenu {
    /// The picked worn object, resolved to its attachment root.
    pub(crate) summary: ObjectPickSummary,
    /// The surface the pick ray struck, when the pick produced one (a mesh ray
    /// hit does; the CPU-skinned rigged pick does not) — carried into Touch.
    pub(crate) surface: Option<SurfaceInfo>,
    /// The wearer, when the pick already resolved it (the avatar-pick path);
    /// otherwise looked up from [`ObjectPickSummary::wearer`] at open time.
    pub(crate) wearer: Option<AgentKey>,
    /// Whether the pick came through the HUD ray — a HUD attachment cannot be
    /// dropped into the world.
    pub(crate) hud: bool,
    /// Where to centre the pie, in logical pixels.
    pub(crate) at: Vec2,
}

/// The worn object the currently-open attachment pie acts on.
///
/// The pie's action strings are `&'static` and cannot carry ids, so the target
/// is stashed here when the menu opens and read back when an action fires (the
/// wearer goes to [`AvatarMenuTarget`] instead, for the shared avatar
/// dispatch). A stale value between opens is harmless because no
/// attachment-menu [`UiAction`] is emitted unless a pie is open.
#[derive(Resource, Debug, Default)]
pub(crate) struct AttachmentMenuTarget {
    /// The picked worn object, or `None` before any open.
    pub(crate) summary: Option<ObjectPickSummary>,
    /// The pick ray's surface on that object, when the pick produced one.
    pub(crate) surface: Option<SurfaceInfo>,
}

/// The plugin wiring the attachment context menus into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AttachmentMenuPlugin;

impl Plugin for AttachmentMenuPlugin {
    /// Register the target resource, the open request, and the systems that
    /// turn a resolved pick into an open pie and a picked slice into a command.
    fn build(&self, app: &mut App) {
        app.init_resource::<AttachmentMenuTarget>()
            .add_message::<OpenAttachmentMenu>()
            .add_systems(
                Update,
                (open_attachment_menu, handle_attachment_menu_actions).chain(),
            );
    }
}

/// Turn a resolved attachment pick into an open pie: resolve the wearer, choose
/// self vs other, snapshot the conditions, and stash the targets — the worn
/// object here, the wearer in [`AvatarMenuTarget`] for the shared avatar
/// dispatch.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the open requests, the \
              identity / seated state / friends / avatar registry the conditions and the wearer \
              resolution read, the two target stashes, and the pie channel"
)]
fn open_attachment_menu(
    mut requests: MessageReader<OpenAttachmentMenu>,
    identity: Res<SlIdentity>,
    parcel: Res<SlAgentParcel>,
    ground_sit: Res<SelfGroundSit>,
    friends: Res<FriendsModel>,
    avatars: Res<AvatarState>,
    mut target: ResMut<AttachmentMenuTarget>,
    mut avatar_target: ResMut<AvatarMenuTarget>,
    mut pies: MessageWriter<OpenPieMenu>,
) {
    for request in requests.read() {
        // The wearer: resolved by the avatar pick, or looked up from the
        // summary's wearer avatar. Without one, self vs other cannot be
        // decided, so nothing opens (an attachment whose avatar object has not
        // arrived yet — the next click will have it).
        let wearer = request.wearer.or_else(|| {
            request
                .summary
                .wearer
                .and_then(|avatar| avatars.agent_of(avatar))
        });
        let Some(wearer) = wearer else {
            warn!(
                "attachment menu: no wearer resolvable for {:?}; not opening",
                request.summary.root_scoped
            );
            continue;
        };
        target.summary = Some(request.summary);
        target.surface.clone_from(&request.surface);
        // The wearer is the agent the avatar-derived slices act on, dispatched
        // by the shared avatar handler (which accepts this menu's element).
        avatar_target.agent = Some(wearer);
        let is_self = identity.agent_id == Some(wearer);
        let mut conditions = Vec::new();
        let menu = if is_self {
            // Exactly one of sitting / standing holds, resolving the Sit Here /
            // Stand Up chain, as in `crate::avatar_menu`.
            let sitting = parcel.seated_on.is_some() || ground_sit.sitting;
            conditions.push(if sitting { SELF_SITTING } else { SELF_STANDING });
            if request.summary.flags & FLAGS_HANDLE_TOUCH != 0 {
                conditions.push(TARGET_TOUCHABLE);
            }
            if !request.hud {
                conditions.push(TARGET_DROPPABLE);
            }
            &ATTACHMENT_SELF_PIE
        } else {
            if !friends.is_friend(wearer) {
                conditions.push(TARGET_NOT_FRIEND);
            }
            &ATTACHMENT_OTHER_PIE
        };
        pies.write(OpenPieMenu {
            menu,
            at: request.at,
            element: ATTACHMENT_MENU_ELEMENT,
            conditions,
        });
    }
}

/// The `FLAGS_HANDLE_TOUCH` bit of an object's update flags (`object_flags.h`):
/// the linkset has a touch handler. (The object pie keeps its own copy; both
/// mirror the wire constant.)
const FLAGS_HANDLE_TOUCH: u32 = 1 << 7;

/// Dispatch a picked attachment-menu slice to the command behind it.
///
/// Only the attachment-specific actions are matched here — Detach, Drop, and
/// Touch. The avatar-derived slices (IM / Mute / Add as Friend / the sit-stand
/// chain) carry the avatar pies' own action names and are dispatched by the
/// shared handler in [`crate::avatar_menu`], which accepts this menu's element;
/// every remaining slice is a disabled placeholder that never emits.
fn handle_attachment_menu_actions(
    mut actions: MessageReader<UiAction>,
    target: Res<AttachmentMenuTarget>,
    mut commands: MessageWriter<SlCommand>,
) {
    for action in actions.read() {
        if action.element != ATTACHMENT_MENU_ELEMENT {
            continue;
        }
        let Some(summary) = target.summary else {
            continue;
        };
        let command = match action.action {
            // Detach / Drop act on the attachment root: the worn object as a
            // whole goes back to inventory / onto the ground.
            "detach" => Some(Command::DetachObjects {
                local_ids: vec![summary.root_scoped],
            }),
            "drop" => Some(Command::DropAttachments {
                local_ids: vec![summary.root_scoped],
            }),
            // Touch targets the picked prim, with the ray's surface when the
            // pick produced one (`llDetectedTouch*`).
            "touch" => Some(Command::TouchObject {
                local_id: summary.picked_scoped,
                surface: target.surface.clone(),
            }),
            // The avatar-derived and placeholder slices: handled elsewhere or
            // not yet implemented.
            _other => None,
        };
        if let Some(command) = command {
            commands.write(SlCommand(command));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ATTACHMENT_OTHER_PIE, ATTACHMENT_SELF_PIE, SELF_MORE_PIE, TARGET_DROPPABLE,
        TARGET_TOUCHABLE,
    };
    use crate::avatar_menu::{SELF_SITTING, SELF_STANDING, TARGET_NOT_FRIEND, UNIMPLEMENTED};
    use crate::pie_menu::{
        Compass, PieAddress, PieConditions, PieContent, PieMenuDef, ResolvedSlot, SlotOutcome,
        addresses, resolve_slots,
    };
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The two attachment pies, for the sweeps that must hold on both.
    const PIES: [&PieMenuDef; 2] = [&ATTACHMENT_SELF_PIE, &ATTACHMENT_OTHER_PIE];

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

    /// **The attachment-self pie's address table, pinned.**
    ///
    /// Moving any action to a different compass path re-teaches every user who
    /// learned this menu with their hand; this table makes that a loud diff.
    /// If a move is intended, change the table in the same reviewed commit.
    #[test]
    fn attachment_self_pie_keeps_every_address() {
        use Compass::{East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West};
        let expected: Vec<(&str, Vec<Compass>)> = vec![
            ("profile", vec![East]),
            ("drop", vec![NorthEast]),
            ("tex-refresh", vec![North, East]),
            ("script-info", vec![North, NorthEast]),
            ("touch", vec![North, North]),
            ("derender", vec![North, NorthWest, NorthWest]),
            ("derender-blacklist", vec![North, NorthWest, West]),
            ("export-backup", vec![North, West, East]),
            ("export-collada", vec![North, West, NorthEast]),
            ("show-in-inventory", vec![North, SouthWest]),
            ("inspect", vec![North, South]),
            ("textures", vec![North, SouthEast]),
            // The Sit Here / Stand Up autohide chain: one address, two actions.
            ("sit-ground", vec![NorthWest]),
            ("stand", vec![NorthWest]),
            ("detach", vec![West]),
            ("gestures", vec![SouthWest]),
            ("edit-shape", vec![South, East]),
            ("reset-skel-anim", vec![South, NorthEast, East]),
            ("reset-skeleton", vec![South, NorthEast, NorthEast]),
            ("reset-mesh-lod", vec![South, NorthEast, North]),
            ("edit-outfit", vec![South, West]),
            ("dump-xml", vec![South, South]),
            ("hover-height", vec![South, SouthEast]),
            ("edit", vec![SouthEast]),
        ];
        assert_eq!(
            address_pairs(&ATTACHMENT_SELF_PIE),
            expected,
            "an attachment-self pie action moved — if intended, bless it by editing this table"
        );
    }

    /// **The attachment-other pie's address table, pinned.** As above, for the
    /// pie on someone else's attachment.
    #[test]
    fn attachment_other_pie_keeps_every_address() {
        use Compass::{East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West};
        let expected: Vec<(&str, Vec<Compass>)> = vec![
            ("profile", vec![East]),
            ("mute", vec![NorthEast, East]),
            ("mute-particles", vec![NorthEast, NorthEast]),
            ("go-to", vec![North]),
            ("report", vec![NorthWest]),
            ("add-friend", vec![West, West]),
            ("add-to-set", vec![West, SouthWest]),
            ("pay", vec![SouthWest]),
            ("freeze", vec![South, East]),
            ("give-card", vec![South, NorthEast]),
            ("invite-to-group", vec![South, North]),
            ("face-towards", vec![South, NorthWest]),
            ("eject", vec![South, West]),
            ("textures", vec![South, SouthWest, East]),
            ("script-info", vec![South, SouthWest, NorthEast]),
            ("call", vec![South, SouthWest, North]),
            ("zoom-in", vec![South, SouthWest, NorthWest]),
            ("tex-refresh", vec![South, SouthWest, West]),
            ("reset-skel-anim", vec![South, SouthWest, SouthWest, East]),
            (
                "reset-skeleton",
                vec![South, SouthWest, SouthWest, NorthEast],
            ),
            ("reset-mesh-lod", vec![South, SouthWest, SouthWest, North]),
            ("export-backup", vec![South, SouthWest, South, East]),
            ("export-collada", vec![South, SouthWest, South, NorthEast]),
            ("dump-xml", vec![South, SouthWest, South, South]),
            ("render-normally", vec![South, SouthWest, SouthEast, East]),
            ("render-never", vec![South, SouthWest, SouthEast, NorthEast]),
            ("render-fully", vec![South, SouthWest, SouthEast, North]),
            ("inspect", vec![South, South]),
            ("derender-blacklist", vec![South, SouthEast, South]),
            ("derender", vec![South, SouthEast, SouthEast]),
            ("im", vec![SouthEast]),
        ];
        assert_eq!(
            address_pairs(&ATTACHMENT_OTHER_PIE),
            expected,
            "an attachment-other pie action moved — if intended, bless it by editing this table"
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

    /// Detach is live regardless of state; Drop tracks the droppable (non-HUD)
    /// condition, keeping its slot disabled for a HUD attachment.
    #[test]
    fn detach_is_always_live_and_drop_tracks_the_hud_state() -> Result<(), TestError> {
        // A world attachment: droppable.
        let world = resolve_slots(
            &ATTACHMENT_SELF_PIE,
            &PieConditions::new([SELF_STANDING, TARGET_DROPPABLE]),
        );
        let detach = slot_at(&world, Compass::West)?;
        assert_eq!(detach.outcome, SlotOutcome::Action("detach"));
        assert!(detach.enabled, "Detach must be live on an own attachment");
        let drop = slot_at(&world, Compass::NorthEast)?;
        assert_eq!(drop.outcome, SlotOutcome::Action("drop"));
        assert!(drop.enabled, "Drop must be live on a world attachment");

        // A HUD attachment: not droppable, Detach still live.
        let hud = resolve_slots(&ATTACHMENT_SELF_PIE, &PieConditions::new([SELF_STANDING]));
        assert!(
            slot_at(&hud, Compass::West)?.enabled,
            "Detach must stay live on a HUD attachment"
        );
        assert!(
            !slot_at(&hud, Compass::NorthEast)?.enabled,
            "Drop must be disabled on a HUD attachment"
        );
        Ok(())
    }

    /// Touch (in the More sub-pie) is enabled exactly when the worn linkset
    /// handles touch, and keeps its north slot disabled when it does not.
    #[test]
    fn touch_tracks_the_touch_handler_flag() -> Result<(), TestError> {
        let touchable = resolve_slots(&SELF_MORE_PIE, &PieConditions::new([TARGET_TOUCHABLE]));
        let touch = slot_at(&touchable, Compass::North)?;
        assert_eq!(touch.outcome, SlotOutcome::Action("touch"));
        assert!(
            touch.enabled,
            "Touch must be live on a touch-scripted attachment"
        );
        let plain = resolve_slots(&SELF_MORE_PIE, &PieConditions::default());
        assert!(
            !slot_at(&plain, Compass::North)?.enabled,
            "Touch must be disabled on an attachment with no touch handler"
        );
        Ok(())
    }

    /// The Sit Here / Stand Up chain resolves to exactly the member matching
    /// the seated state, at the same north-west position either way.
    #[test]
    fn sit_and_stand_share_the_north_west_slot() -> Result<(), TestError> {
        let standing = resolve_slots(&ATTACHMENT_SELF_PIE, &PieConditions::new([SELF_STANDING]));
        let sit = slot_at(&standing, Compass::NorthWest)?;
        assert_eq!(sit.outcome, SlotOutcome::Action("sit-ground"));
        assert!(sit.enabled, "Sit Here must be live while standing");

        let sitting = resolve_slots(&ATTACHMENT_SELF_PIE, &PieConditions::new([SELF_SITTING]));
        let stand = slot_at(&sitting, Compass::NorthWest)?;
        assert_eq!(stand.outcome, SlotOutcome::Action("stand"));
        assert!(stand.enabled, "Stand Up must be live while sitting");
        Ok(())
    }

    /// On another avatar's attachment IM is always live, Add as Friend tracks
    /// friendship, and the placeholders keep their slots disabled.
    #[test]
    fn other_pie_wires_im_and_add_friend_and_greys_the_rest() -> Result<(), TestError> {
        // A stranger's attachment: IM live, Add as Friend live.
        let stranger = resolve_slots(
            &ATTACHMENT_OTHER_PIE,
            &PieConditions::new([TARGET_NOT_FRIEND]),
        );
        let im = slot_at(&stranger, Compass::SouthEast)?;
        assert_eq!(im.outcome, SlotOutcome::Action("im"));
        assert!(im.enabled, "IM must always be live on another's attachment");
        let add = resolve_slots(
            &super::OTHER_ADD_PIE,
            &PieConditions::new([TARGET_NOT_FRIEND]),
        );
        assert!(
            slot_at(&add, Compass::West)?.enabled,
            "Add as Friend must be live for a stranger"
        );
        let friend = resolve_slots(&super::OTHER_ADD_PIE, &PieConditions::default());
        assert!(
            !slot_at(&friend, Compass::West)?.enabled,
            "Add as Friend must be disabled for a friend"
        );
        // The placeholders: present, disabled — until the sentinel is held.
        for (point, name) in [
            (Compass::East, "Profile"),
            (Compass::North, "Go To"),
            (Compass::NorthWest, "Report"),
            (Compass::SouthWest, "Pay"),
        ] {
            assert!(
                !slot_at(&stranger, point)?.enabled,
                "{name} is a placeholder and must read disabled until it is wired"
            );
        }
        let held = resolve_slots(&ATTACHMENT_OTHER_PIE, &PieConditions::new([UNIMPLEMENTED]));
        assert!(
            slot_at(&held, Compass::East)?.enabled,
            "holding the sentinel proves it is the only thing gating the placeholder"
        );
        Ok(())
    }
}
