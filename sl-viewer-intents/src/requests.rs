//! Cross-tier intents.
//!
//! Messages a surface writes to ask for something it does not own: block this
//! resident, open that profile, pick a texture. Each is read by a feature that
//! sits far from the ones asking -- `RequestBlock` alone is written from avatar
//! menus, the radar, the minimap, the profile, the inspector, the friends list,
//! three kinds of toast and a `secondlife:///` link.
//!
//! They live here rather than with the floater that answers them, so asking
//! does not mean depending on the answer. Every payload is an id or a string
//! this crate or `sl-client-bevy` already owns.

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, ChatSessionKind, Command, ExperienceKey, ExperienceProperties, GroupKey, ImSessionId,
    MuteFlags, MuteType, RegionCoordinates, RegionHandle, RegionLocalParcelId, SlCommand,
    TextureKey, Uuid, Vector,
};

/// A request to block a target: the single **guarded** entry point every Block
/// surface writes instead of putting a `Command::Mute` on the wire itself.
///
/// `apply_block_requests` runs the reference's `LLMuteList::add` checks and
/// only then sends, so every Block in the viewer — the avatar / object pie
/// menus, the radar, the minimap, the profile floater, the inspector, the
/// friends list, the offer / dialog / URL toasts, a `secondlife:///…/mute`
/// link, and the block list's own add paths — refuses a Linden, the agent
/// itself, a malformed or duplicate by-name entry and an over-full list
/// identically, and reports the refusal with the same notification.
#[derive(Message, Debug, Clone)]
pub struct RequestBlock {
    /// The blocked entity's id (nil for a [`MuteType::ByName`] block).
    pub id: Uuid,
    /// The blocked entity's name, as the asking surface knows it.
    pub name: String,
    /// What kind of entity is blocked.
    pub mute_type: MuteType,
    /// The per-aspect *exception* flags ([`MuteFlags::default`] mutes all).
    pub flags: MuteFlags,
}

impl RequestBlock {
    /// Block `id` as `mute_type` under `name`, with every aspect muted — what a
    /// menu's plain "Block" does.
    pub fn new(id: Uuid, name: impl Into<String>, mute_type: MuteType) -> Self {
        Self {
            id,
            name: name.into(),
            mute_type,
            flags: MuteFlags::default(),
        }
    }

    /// The same request with explicit exception flags — the block list's
    /// per-aspect toggles re-sending an edited entry.
    #[must_use]
    pub const fn with_flags(mut self, flags: MuteFlags) -> Self {
        self.flags = flags;
        self
    }
}

/// A request to offer friendship to one or more residents: the single
/// **prompted** entry point every Add Friend surface writes instead of putting
/// a `Command::OfferFriendship` on the wire itself.
///
/// `sl_viewer_people::add_friend` answers it the way the reference's
/// `LLAvatarActions::requestFriendshipDialog` does — refuse the agent itself,
/// ask for the accompanying message, and only then send — so the avatar pie,
/// the radar, the minimap, the profile, the inspector, search and a
/// `secondlife:///…/requestfriend` link all gain the prompt and the
/// confirmation at once, rather than each sending an empty offer in silence.
#[derive(Message, Debug, Clone)]
pub struct RequestFriendship {
    /// The residents to offer friendship to. A multi-selection is **one**
    /// request: it asks once and offers the typed message to each, the way the
    /// multi-avatar menus already treat one action over a list.
    pub targets: Vec<AgentKey>,
}

impl RequestFriendship {
    /// Offer friendship to one resident — what a single-subject surface writes.
    #[must_use]
    pub fn one(agent: AgentKey) -> Self {
        Self {
            targets: vec![agent],
        }
    }

    /// Offer friendship to every resident in a selection.
    #[must_use]
    pub const fn many(targets: Vec<AgentKey>) -> Self {
        Self { targets }
    }
}

/// Open the profile floater on an avatar (from the pie menu's Profile slice,
/// the People list, or a repaint after an edit).
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenAvatarProfile {
    /// The avatar whose profile to show.
    pub agent: AgentKey,
}

/// Open the group profile floater on a group (from the Groups list's Info button).
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenGroupProfile {
    /// The group whose profile to show.
    pub group: GroupKey,
}

/// A client-generated local-chat notice — a line the viewer itself posts to the
/// overlay (not a `ChatReceived` from the grid), for feedback like a build-tool
/// no-permission alert. Written by whichever system produced the notice
/// (e.g. `crate::gizmos::dispatch_shift_drag_copy`) and rendered by
/// `update_chat_overlay` alongside received chat.
#[derive(Message, Debug, Clone)]
pub struct LocalChatNotice {
    /// The already-formatted line to show.
    pub text: String,
}

impl LocalChatNotice {
    /// A notice carrying `text`.
    #[must_use]
    pub const fn new(text: String) -> Self {
        Self { text }
    }
}

/// Ask the picker to open for a control. `requester` is the control that asked
/// — the [`AvatarPicked`] reply carries it back, so only the widget that asked
/// consumes it, and the window that widget lives in is the picker's own.
///
/// The reply used to be routed by a `&'static str` tag, which is a property of
/// the *control* rather than of the window: two instances of one floater have
/// the same controls, so both claimed the same answer. See `picker_identity`.
#[derive(Message, Debug, Clone)]
pub struct OpenAvatarPicker {
    /// The control that asked — the Add / Kick / Send-Home button. Echoed back
    /// in [`AvatarPicked`], and the consumer resolves its host floater to reach
    /// the window's own state.
    pub requester: Entity,
    /// **Which** of the opening window's pickers this is — the field name, one
    /// per Add button. Two fields of one window are two picker windows; the
    /// same field in two instances of that window is also two.
    ///
    /// Owned rather than `&'static str` for the same reason
    /// [`OpenTexturePicker::field`] is: a window holding several of one control
    /// (the Conversations window's per-conversation Add-participants button)
    /// names them apart at spawn time, and two of them sharing a field name
    /// would be one picker they fight over.
    pub field: Box<str>,
    /// Whether the user may choose several residents at once — the reference's
    /// `allow_multiple`. Build one with [`OpenAvatarPicker::one`] or
    /// [`OpenAvatarPicker::many`] rather than by hand, so the choice reads at
    /// the call site.
    pub allow_multiple: bool,
}

impl OpenAvatarPicker {
    /// Ask for exactly one resident.
    #[must_use]
    pub fn one(requester: Entity, field: impl Into<Box<str>>) -> Self {
        Self {
            requester,
            field: field.into(),
            allow_multiple: false,
        }
    }

    /// Ask for any number of residents at once.
    #[must_use]
    pub fn many(requester: Entity, field: impl Into<Box<str>>) -> Self {
        Self {
            requester,
            field: field.into(),
            allow_multiple: true,
        }
    }
}

/// One resident the picker returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedAvatar {
    /// The chosen avatar.
    pub agent: AgentKey,
    /// The label the picked row carried — the avatar's name as the source that
    /// produced the row knew it (a search reply's legacy name, the friend's
    /// name, or the nearby avatar's name). Consumers that must *record* a name
    /// against the id (the block list writes it into the mute entry) take it
    /// from here rather than re-resolving.
    pub name: String,
}

/// The confirmed pick: every chosen resident, in list order. A picker opened
/// with [`OpenAvatarPicker::one`] answers with exactly one element.
#[derive(Message, Debug, Clone)]
pub struct AvatarPicked {
    /// The control that opened the picker. A consumer matches on it — and, for
    /// a window that opens per subject, resolves its host floater to reach that
    /// instance's state.
    pub requester: Entity,
    /// The chosen residents — never empty (the picker does not confirm an empty
    /// selection).
    pub picks: Vec<PickedAvatar>,
}

impl AvatarPicked {
    /// The first chosen resident — for a single-resident requester, *the* pick.
    #[must_use]
    pub fn first(&self) -> Option<&PickedAvatar> {
        self.picks.first()
    }
}

/// Ask the group picker to open for a control — the reference's
/// `LLFloaterGroupPicker` (`floater_choose_group.xml`), the dialog behind the
/// estate's allowed-groups Add, About Land's group **Set…** and the build
/// tool's set-group.
///
/// Which groups a picker may offer.
///
/// Not a preference but a protocol fact: the simulator refuses to set a
/// parcel's or an object's group to one the agent is not a member of, so a
/// set-group control that offered a stranger's group would only ever produce a
/// silently-refused update. Where the answer is merely *recorded* against an id
/// — the estate's allowed-groups list — any group will do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupPickerScope {
    /// The agent's own memberships alone — the reference's entire list
    /// (`init_group_list` walks `gAgent.mGroups`).
    #[default]
    MemberGroups,
    /// The memberships plus a directory name search, so a group the agent is
    /// not in can be named. The reference cannot do this at all.
    AnyGroup,
}

/// Ask the group picker to open for a control — the reference's
/// `LLFloaterGroupPicker` (`floater_choose_group.xml`), the dialog behind the
/// estate's allowed-groups Add, About Land's group **Set…** and the build
/// tool's set-group.
///
/// Routed like [`OpenAvatarPicker`]: `requester` is the control that asked, the
/// [`GroupPicked`] reply carries it back, and `field` keys the window so two
/// controls — or two instances of one floater — each get their own picker.
///
/// Built from [`OpenGroupPicker::new`] and narrowed with
/// [`without_none`](OpenGroupPicker::without_none) /
/// [`searching_the_directory`](OpenGroupPicker::searching_the_directory), so
/// the call site reads as what that control can accept.
#[derive(Message, Debug, Clone)]
pub struct OpenGroupPicker {
    /// The control that asked — the Add / Set… button. Echoed back in
    /// [`GroupPicked`], and the consumer resolves its host floater to reach the
    /// window's own state.
    pub requester: Entity,
    /// **Which** of the opening window's pickers this is. See
    /// [`OpenAvatarPicker::field`].
    pub field: Box<str>,
    /// Whether **none** is an answer — the reference's `removeNoneOption`,
    /// inverted so the permissive case is the plain constructor. Setting a
    /// parcel's or an object's group may clear it; adding to the estate's
    /// allowed-groups list may not (a null group is not a group).
    pub allow_none: bool,
    /// Which groups this open may offer.
    pub scope: GroupPickerScope,
}

impl OpenGroupPicker {
    /// Ask for one of the agent's own groups, with **none** among the answers —
    /// a set-group control, which may also unset.
    #[must_use]
    pub fn new(requester: Entity, field: impl Into<Box<str>>) -> Self {
        Self {
            requester,
            field: field.into(),
            allow_none: true,
            scope: GroupPickerScope::MemberGroups,
        }
    }

    /// Drop the **none** row: a list the answer is *added* to, where a null
    /// group would be a row naming nothing.
    #[must_use]
    pub const fn without_none(mut self) -> Self {
        self.allow_none = false;
        self
    }

    /// Offer the directory search as well as the memberships — for a control
    /// that only records the id, so a group the agent is not in is a usable
    /// answer.
    #[must_use]
    pub const fn searching_the_directory(mut self) -> Self {
        self.scope = GroupPickerScope::AnyGroup;
        self
    }
}

/// The confirmed group pick. The picker chooses exactly one group, or — where
/// the open allowed it — **none**.
#[derive(Message, Debug, Clone)]
pub struct GroupPicked {
    /// The control that opened the picker (see [`OpenGroupPicker`]).
    pub requester: Entity,
    /// The chosen group, or `None` for the "none" row (only ever sent for an
    /// open that set [`OpenGroupPicker::allow_none`]).
    pub group: Option<GroupKey>,
    /// The name the picked row carried, so a consumer that must show the group
    /// before the id resolves has something to show. Empty for the "none" row.
    pub name: String,
}

/// Which experiences an experience picker may offer — the reference's
/// `LLPanelExperiencePicker` filter list, as the three shapes its callers
/// actually construct (`LLPanelRegionExperiences::refreshFromRegion`).
///
/// The reference passes a vector of predicates; naming the three combinations
/// instead keeps the *reason* for each filter with the list it belongs to,
/// which a bare predicate list loses at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExperiencePickerFilter {
    /// Anything may be picked — the estate's **Key** (trusted) list, which the
    /// reference opens with no filters at all.
    #[default]
    Any,
    /// Only **land-scoped** experiences: the estate's Allowed list, which the
    /// reference filters with `FilterWithProperty(PROPERTY_GRID)` — a
    /// grid-scoped experience already runs everywhere, so allowing one here
    /// would say nothing.
    LandScoped,
    /// Only **grid-scoped, non-privileged** experiences: the estate's Blocked
    /// list. Blocking is meaningful only for something that would otherwise run
    /// grid-wide (`FilterWithoutProperty(PROPERTY_GRID)`), and a privileged
    /// experience cannot be refused at all
    /// (`FilterWithProperty(PROPERTY_PRIVILEGED)`).
    GridScopedUnprivileged,
}

impl ExperiencePickerFilter {
    /// Whether an experience with `properties` passes this filter.
    #[must_use]
    pub const fn admits(self, properties: ExperienceProperties) -> bool {
        match self {
            Self::Any => true,
            Self::LandScoped => !properties.is_grid(),
            Self::GridScopedUnprivileged => properties.is_grid() && !properties.is_privileged(),
        }
    }
}

/// Ask the experience picker to open for a control. `requester` is the control
/// that asked, echoed back in [`ExperiencePicked`] — the same out-of-band shape
/// as [`OpenAvatarPicker`].
///
/// The reference's equivalent is `LLFloaterExperiencePicker::show`, which every
/// estate / parcel experience list opens from its Add button.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenExperiencePicker {
    /// The control that asked — the list's Add button. Echoed back in
    /// [`ExperiencePicked`], and its host floater is the window the pick
    /// belongs to.
    pub requester: Entity,
    /// **Which** of the opening window's pickers this is — one per experience
    /// list. See [`OpenAvatarPicker::field`].
    pub field: &'static str,
    /// Which experiences this open may offer.
    pub filter: ExperiencePickerFilter,
    /// One experience this open must **not** offer, whatever
    /// [`filter`](Self::filter) says about it.
    ///
    /// A property filter cannot express "not that one": the reference adds the
    /// exclusion as a second, id-matching filter beside the property one
    /// (`LLPanelExperiencePicker::FilterMatching`), which is how the estate's
    /// Allowed and Blocked pickers keep the estate's own **default experience**
    /// off their lists — it is neither something to allow nor something to
    /// block.
    pub excluded: Option<ExperienceKey>,
}

/// The confirmed experience pick. The picker chooses exactly one (the
/// reference's estate lists open it with `allow_multiple` false and
/// `close_on_select` true).
#[derive(Message, Debug, Clone)]
pub struct ExperiencePicked {
    /// The control that opened the picker (see [`OpenExperiencePicker`]).
    pub requester: Entity,
    /// The chosen experience.
    pub experience: ExperienceKey,
    /// The name the picked row carried, so a consumer that must show the
    /// experience before its own metadata fetch answers has something to show.
    pub name: String,
}

/// What a picker open browses: **textures** (the default — the reference's
/// `LLTextureCtrl` `PICK_TEXTURE`) or **materials** (GLTF render materials, the
/// reference's `PICK_MATERIAL`). It drives the inventory filter, the floater
/// title, and which quick choices show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerKind {
    /// Browse texture / snapshot items.
    #[default]
    Texture,
    /// Browse GLTF render-material items (`InventoryType::Material`).
    Material,
}

/// Open the texture picker for `requester`, seeded with `current`.
#[derive(Message, Debug, Clone)]
pub struct OpenTexturePicker {
    /// The swatch (or other widget) the reply is tagged back to.
    pub requester: Entity,
    /// **Which field** is being picked for — the swatch's element id, or a
    /// name the opener chooses. The picker opens one window per field of the
    /// **opening window** (the reference gives every `LLTextureCtrl` its own
    /// picker), so this is half of that window's identity and the opener is the
    /// other half: two fields are two pickers, and so are the same field in two
    /// instances of one floater. See `picker_identity`.
    ///
    /// Two swatches declared with the same element id *in one window* share one
    /// picker, since they are the same field as far as the UI is concerned.
    ///
    /// Owned rather than `&'static str` because a field id is not always a
    /// literal: a table-driven panel (the environment editors) names its
    /// controls `{window}-{knob}`, which is a name computed at spawn time.
    pub field: Box<str>,
    /// The texture (or, in material mode, material id) to open on.
    pub current: TextureKey,
    /// Whether to browse textures or materials.
    pub kind: PickerKind,
}

/// The chosen texture, tagged back to the [`requester`](Self::requester). Emitted
/// **non-final** on each selection so a consumer can live-preview it, once on
/// **OK** with [`final_pick`](Self::final_pick) true, and on **Cancel** as the
/// original (a revert), mirroring the colour picker.
#[derive(Message, Debug, Clone, Copy)]
pub struct TexturePicked {
    /// The widget that opened the picker.
    pub requester: Entity,
    /// The chosen texture.
    pub texture: TextureKey,
    /// Whether this is the committed choice (**OK**) rather than a live-preview
    /// or revert update.
    pub final_pick: bool,
}

/// A conversation's stable identity — the per-tab key. `Nearby` is the singleton
/// local-chat tab; the rest key on the peer, group or conference.
///
/// Derives [`Ord`] so it can key the `ConversationsUi` view map (sl-types gives
/// the newtypes their ordering).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ConversationKey {
    /// The local (nearby) chat tab — always present, always first.
    Nearby,
    /// A one-to-one instant-message conversation with a peer.
    Direct(AgentKey),
    /// A group IM session.
    Group(GroupKey),
    /// An ad-hoc conference IM session.
    Conference(ImSessionId),
}

impl ConversationKey {
    /// Whether this is the un-closable Nearby tab.
    #[must_use]
    pub const fn is_nearby(self) -> bool {
        matches!(self, Self::Nearby)
    }

    /// The runtime chat-session kind behind a keyed tab, or `None` for Nearby
    /// (local chat is not a [`ChatSessionKind`] session).
    #[must_use]
    pub const fn session_kind(self) -> Option<ChatSessionKind> {
        match self {
            Self::Nearby => None,
            Self::Direct(peer) => Some(ChatSessionKind::Direct { peer }),
            Self::Group(group_id) => Some(ChatSessionKind::Group { group_id }),
            Self::Conference(id) => Some(ChatSessionKind::Conference { id }),
        }
    }

    /// The tab key for a runtime chat-session kind (the inverse of
    /// [`Self::session_kind`]).
    #[must_use]
    pub const fn from_session_kind(kind: ChatSessionKind) -> Self {
        match kind {
            ChatSessionKind::Direct { peer } => Self::Direct(peer),
            ChatSessionKind::Group { group_id } => Self::Group(group_id),
            ChatSessionKind::Conference { id } => Self::Conference(id),
        }
    }
}

/// A request to open (create if needed) and activate `key`'s conversation — the
/// hook another module uses to start an IM from outside the floater. The
/// `crate::people` Friends list writes this to open a one-to-one IM tab for a
/// selected friend in this same floater.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenConversation {
    /// The conversation to open and select.
    pub key: ConversationKey,
}

/// A request to parse and dispatch a raw SLURL / app-command string — the entry
/// point for sources outside the in-app link widgets: the `secondlife://` OS
/// protocol handler / command line, an inspector popup handing its own SLURL
/// back, and any future caller (a landmark's embedded SLURL, a typed address
/// bar). The dispatcher runs the string through the same matcher the text layer
/// uses and routes its first recognised link.
#[derive(Message, Debug, Clone)]
pub struct DispatchSlurl {
    /// The raw URL string to parse and act on.
    pub url: String,
}

/// Open (and optionally navigate) the web browser floater.
#[derive(Message, Debug, Clone)]
pub struct OpenWebBrowser {
    /// The URL to show; `None` keeps the current page (or the home page on
    /// first open).
    pub url: Option<String>,
}

/// A concrete teleport target, kept so the overlay's **Retry** button can
/// re-issue the exact same teleport after a failure.
#[derive(Debug, Clone)]
pub struct TeleportTarget {
    /// The destination region handle.
    pub region_handle: RegionHandle,
    /// The destination region-local arrival position.
    pub position: RegionCoordinates,
    /// The arrival look-at direction.
    pub look_at: Vector,
}

/// A request to open the teleport overlay for a teleport this frame's surface is
/// initiating. Emitting it is optional — the overlay also opens from the incoming
/// teleport events — but it lets a surface pre-fill the destination label and
/// enable Retry. Prefer the [`issue_teleport`] helper, which writes this and the
/// [`Command::Teleport`] together.
#[derive(Message, Debug, Clone)]
pub struct BeginTeleportFlow {
    /// A human-readable destination label (e.g. a region name or `Region (128, 128)`),
    /// shown on the overlay. `None` leaves the destination line blank.
    pub destination: Option<String>,
    /// The target to re-issue if the user hits Retry. `None` (landmark / lure
    /// teleports, whose destination is not known until arrival) disables Retry.
    pub retry: Option<TeleportTarget>,
}

/// Fire a location teleport **and** open the progress overlay in one call: writes
/// [`Command::Teleport`] and a [`BeginTeleportFlow`] carrying the destination
/// label and a Retry payload. The shared entry point every location-teleport
/// surface (double-click, minimap, world map) routes through.
pub fn issue_teleport(
    commands: &mut MessageWriter<SlCommand>,
    begin: &mut MessageWriter<BeginTeleportFlow>,
    target: TeleportTarget,
    destination: Option<String>,
) {
    begin.write(BeginTeleportFlow {
        destination,
        retry: Some(target.clone()),
    });
    commands.write(SlCommand(Command::Teleport {
        region_handle: target.region_handle,
        position: target.position,
        look_at: target.look_at,
    }));
}

/// Ask for the add-to-set floater. The avatar pie's **Add ▸ Add to Set**, the
/// panel's **Move to Set…** and the minimap's multi-avatar **Add to Set** all
/// write this.
#[derive(Message, Debug, Clone)]
pub struct OpenAddToContactSet {
    /// The residents to file, each with the best name the opening surface knows
    /// for them (empty when it knows none). Usually one; the reference's
    /// multi-avatar entries hand over several, and the floater then asks for one
    /// set to file the lot under.
    pub agents: Vec<(AgentKey, String)>,
    /// The set to take them out of once they are filed — the reference's move
    /// mode. `None` for a plain add.
    pub move_from: Option<String>,
}

impl OpenAddToContactSet {
    /// File one resident.
    #[must_use]
    pub fn one(agent: AgentKey, name: String) -> Self {
        Self {
            agents: vec![(agent, name)],
            move_from: None,
        }
    }

    /// File several residents at once.
    #[must_use]
    pub const fn many(agents: Vec<(AgentKey, String)>) -> Self {
        Self {
            agents,
            move_from: None,
        }
    }

    /// The same request in the reference's *move* mode: take them out of `set`
    /// once they are filed.
    #[must_use]
    pub fn moving_from(mut self, set: String) -> Self {
        self.move_from = Some(set);
        self
    }
}

/// A request to open the About Land floater.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenAboutLand {
    /// Which parcel to describe.
    pub subject: AboutLandSubject,
    /// Open without edit affordances (the read-only "About this location" view).
    pub read_only: bool,
}

/// How the About Land floater's subject parcel is identified.
#[derive(Debug, Clone, Copy)]
pub enum AboutLandSubject {
    /// A known region-local parcel id (the agent's current parcel -- the top-bar
    /// read-out, the World menu, the Land tool's selection). Its data is
    /// already local.
    CurrentParcel(RegionLocalParcelId),
    /// A region-local ground point (a land-pie right-click). The parcel is
    /// resolved by asking the simulator for the parcel at that point
    /// (`ParcelPropertiesRequest`), so a click on **any** parcel -- not just one
    /// already fetched -- opens on that parcel, not the agent's own.
    AtPoint {
        /// The region-local east metre.
        x: f32,
        /// The region-local north metre.
        y: f32,
    },
}
