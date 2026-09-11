//! The avatar **profile floater** (`viewer-social-profiles`): the legacy
//! in-viewer profile — 2nd Life / Web / Picks / Classifieds / 1st Life /
//! Notes tabs — shown for any avatar and editable for one's own.
//!
//! # Layout follows the Vintage skin
//!
//! Tabs and controls mirror the reference's in-viewer profile as the Vintage
//! skin lays it out (`floater_profile.xml`,
//! `skins/vintage/xui/en/panel_profile_secondlife.xml`, `panel_profile_pick*`,
//! `panel_profile_classified*`; code `llpanelprofile.cpp`,
//! `llpanelprofilepicks.cpp`, `llpanelprofileclassifieds.cpp`). Two deliberate
//! deviations: the reference's Web tab is an embedded browser, which this
//! viewer does not have yet, so ours shows (and edits) the profile URL only
//! (`viewer-profile-web-tab-browser` upgrades it once CEF lands); and there is
//! no Interests tab — the reference dropped it (`AvatarInterestsReply` is a
//! null handler there), and we follow.
//!
//! # One window per resident
//!
//! A profile is a **keyed floater** ([`FloaterKey`]): opening a second
//! resident's profile opens a second window beside the first rather than
//! re-pointing it, so two people can be compared side by side — the reference's
//! `LLFloaterReg::showInstance("profile", agent)`. Everything one window knows
//! — its subject, the replies received for them, which tabs need repainting,
//! where its fields are — lives in components on that window's floater root
//! (`ProfileState`, `ProfileDirty`, `ProfileUi`), and every system here
//! iterates the open windows rather than reading one resource.
//!
//! Two consequences: closing a profile **despawns** it, so re-opening that
//! resident starts from fresh requests rather than from a stale shell; and
//! nothing spawns at startup, since a window exists only while its subject's
//! profile is open.
//!
//! # Rebuilt per change
//!
//! Each tab's content is torn down and rebuilt when the floater opens on an
//! avatar and when a reply for that avatar arrives (properties, groups, pick
//! and classified lists and details, notes) — the same picker-list pattern as
//! [`crate::inventory_properties`], so fields carry their values as initial
//! text and nothing needs a programmatic text-set API. Text edits commit via
//! the explicit Save buttons (the wire updates are full replacements).
//!
//! Not yet wired (buttons present but greyed, matching the pie menu's
//! placeholder convention): Find on Map / Show on Map (needs the world map
//! floater), Invite to Group (needs a group/role picker). Profile and
//! pick/classified **images** are shown but not editable (needs a texture
//! picker); a save keeps the existing image ids.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash as _, Hasher as _};

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{ControlOrientation, Scrollbar, ScrollbarThumb};
use sl_client_bevy::{
    AgentKey, AvatarClassified, AvatarGroupMembership, AvatarPick, AvatarProperties,
    ClassifiedCategory, ClassifiedInfo, ClassifiedKey, ClassifiedUpdate, Command, FriendKey,
    GlobalCoordinates, GroupKey, LindenAmount, MoneyTransactionType, MuteType, PickInfo, PickKey,
    PickUpdate, ProfileUpdate, RegionCoordinates, RegionHandle, SlCommand, SlEvent, SlIdentity,
    SlSessionEvent, TextureKey, Uuid, Vector, to_bevy_image,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterHandle, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen,
    KeyedFloaters, host_floater,
};
use crate::i18n::Translated;
use crate::inventory_drag::AgentDropTarget;
use crate::inventory_properties::format_unix_date;
use crate::ui::{column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabContainerHandle, TabPlacement, TabSpec, TabStrip, fill_tab_container,
    spawn_tab_container,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::world_api::AVATAR_BOOST_PRIORITY;
use crate::world_api::AvatarState;
use crate::world_api::FriendsModel;
use crate::world_api::GroupsModel;
use crate::world_api::OpenGroupProfile;
use crate::world_api::RequestBlock;
use crate::world_api::RequestFriendship;
use crate::world_api::{BoostTexture, DecodedTextures};
use crate::world_api::{ConversationKey, OpenAvatarProfile, OpenConversation};

/// The chrome font size, in logical pixels.
const PROFILE_FONT_SIZE: f32 = 14.0;

/// The primary label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer secondary label.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A toggle's check glyph colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// The accent colour for a clickable group name in the 2nd-Life group list.
const GROUP_LINK_COLOR: Color = Color::srgb(0.52, 0.68, 0.95);

/// The 2nd-Life group list's bounded scroll height, in logical pixels.
const GROUP_LIST_HEIGHT: f32 = 120.0;

/// The group list scrollbar's track thickness, in logical pixels.
const SCROLLBAR_THICKNESS: f32 = 10.0;
/// The group list scrollbar's minimum thumb length, in logical pixels.
const SCROLLBAR_MIN_THUMB: f32 = 24.0;
/// The group list scrollbar track colour.
const SCROLLBAR_TRACK_COLOR: Color = Color::srgb(0.12, 0.14, 0.18);
/// The group list scrollbar thumb colour.
const SCROLLBAR_THUMB_COLOR: Color = Color::srgb(0.34, 0.40, 0.52);

/// The longest gap between two clicks on the same group row still counted as a
/// double-click (which opens the group profile), in seconds.
const GROUP_DOUBLE_CLICK_SECS: f32 = 0.4;

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);
/// A button's border colour.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The checked glyph.
const CHECKED_GLYPH: &str = "\u{2611}";
/// The unchecked glyph.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// The profile / first-life picture edge, in logical pixels (the reference's
/// second-life picture control is 158×158).
const PROFILE_IMAGE_EDGE: f32 = 158.0;

/// A pick / classified snapshot's width × height, in logical pixels (the
/// reference's 310×174, scaled to the tab panel width).
const SNAPSHOT_SIZE: Vec2 = Vec2::new(272.0, 153.0);

/// The most picks a profile may hold (the reference's `MAX_AVATAR_PICKS`).
const MAX_PICKS: usize = 10;

/// The most classifieds a profile may hold (the reference's
/// `MAX_AVATAR_CLASSIFIEDS`).
const MAX_CLASSIFIEDS: usize = 100;

/// The profile-flags bit for "show in search" (`AVATAR_ALLOW_PUBLISH`).
const FLAG_ALLOW_PUBLISH: u32 = 1;
/// The profile-flags bit for "payment info on file" (`AVATAR_IDENTIFIED`).
/// Shared with the avatar radar's payment column.
pub(crate) const FLAG_IDENTIFIED: u32 = 1 << 2;
/// The profile-flags bit for "payment info used" (`AVATAR_TRANSACTED`).
/// Shared with the avatar radar's payment column.
pub(crate) const FLAG_TRANSACTED: u32 = 1 << 3;
/// The profile-flags bit for "currently online" (`AVATAR_ONLINE`).
const FLAG_ONLINE: u32 = 1 << 4;

/// The classified-flags bit for moderate ("mature") content
/// (`CLASSIFIED_FLAG_MATURE`).
const CLASSIFIED_FLAG_MATURE: u8 = 1 << 1;
/// The classified-flags bit for weekly auto-renew
/// (`CLASSIFIED_FLAG_AUTO_RENEW`).
const CLASSIFIED_FLAG_AUTO_RENEW: u8 = 1 << 5;

/// The pick list strip's element id (also its width-persistence key).
const PICKS_STRIP_ELEMENT: &str = "profile-picks-list";
/// The classified list strip's element id.
const CLASSIFIEDS_STRIP_ELEMENT: &str = "profile-classifieds-list";
/// The pick / classified list strips' fixed label-column width.
const LIST_STRIP_WIDTH: f32 = 110.0;

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// Which parts of one classified the cycle / toggle controls edit — kept
/// outside the rebuilt widgets so a repaint does not lose them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClassifiedDraft {
    /// The listing's search category.
    category: ClassifiedCategory,
    /// Whether the listing is Moderate (vs General) content.
    mature: bool,
    /// Whether the listing auto-renews weekly.
    auto_renew: bool,
}

impl Default for ClassifiedDraft {
    /// A fresh listing: Shopping, General content, no auto-renew.
    fn default() -> Self {
        Self {
            category: ClassifiedCategory::Shopping,
            mature: false,
            auto_renew: false,
        }
    }
}

impl ClassifiedDraft {
    /// The draft matching an existing listing's stored fields.
    const fn from_info(info: &ClassifiedInfo) -> Self {
        Self {
            category: info.category,
            mature: classified_mature(info.classified_flags),
            auto_renew: classified_auto_renew(info.classified_flags),
        }
    }
}

/// One open profile window's live state: the avatar it shows and everything
/// received about them so far.
///
/// A **component on the floater root**, not a resource: profiles are keyed
/// windows (one per resident, [`FloaterKey`]), so there are as many of these as
/// there are open profiles, and closing one despawns its window and this with
/// it.
#[derive(Component, Debug)]
pub(crate) struct ProfileState {
    /// The avatar this window shows. Fixed for the window's life: a profile is
    /// opened *on* a resident and closing it ends the window, so there is no
    /// re-pointing and nothing here is ever `None`.
    target: AgentKey,
    /// The avatar's properties, once received.
    properties: Option<AvatarProperties>,
    /// The avatar's profile group list, once received.
    groups: Option<Vec<AvatarGroupMembership>>,
    /// The avatar's pick list, once received.
    picks: Option<Vec<AvatarPick>>,
    /// The avatar's classified list, once received.
    classifieds: Option<Vec<AvatarClassified>>,
    /// Our private notes about the avatar, once received.
    notes: Option<String>,
    /// Fetched pick details, by pick id.
    pick_info: HashMap<PickKey, PickInfo>,
    /// Fetched classified details, by classified id.
    classified_info: HashMap<ClassifiedKey, ClassifiedInfo>,
    /// The selected pick's index into [`picks`](Self::picks).
    selected_pick: usize,
    /// The selected classified's index into [`classifieds`](Self::classifieds).
    selected_classified: usize,
    /// The "Show in search" checkbox as currently displayed (saved on Save).
    show_in_search: bool,
    /// Picks whose next save should move them to the agent's current parcel
    /// ("Set Location" pressed).
    pick_use_current: HashSet<PickKey>,
    /// Classifieds whose next save should move them to the agent's current
    /// parcel ("Set to Current Location" pressed).
    classified_use_current: HashSet<ClassifiedKey>,
    /// Per-classified cycle / toggle edits not yet saved.
    classified_drafts: HashMap<ClassifiedKey, ClassifiedDraft>,
    /// The in-progress new-classified editor, or `None` when not creating.
    new_classified: Option<ClassifiedDraft>,
    /// Textures awaited from the pipeline, with the node to hand each image to.
    pending_textures: Vec<(TextureKey, Entity)>,
}

impl ProfileState {
    /// A fresh state for a window just opened on `target` — nothing received
    /// yet, every request still in flight.
    fn new(target: AgentKey) -> Self {
        Self {
            target,
            properties: None,
            groups: None,
            picks: None,
            classifieds: None,
            notes: None,
            pick_info: HashMap::new(),
            classified_info: HashMap::new(),
            selected_pick: 0,
            selected_classified: 0,
            show_in_search: false,
            pick_use_current: HashSet::new(),
            classified_use_current: HashSet::new(),
            classified_drafts: HashMap::new(),
            new_classified: None,
            pending_textures: Vec::new(),
        }
    }

    /// The selected pick's list entry, if any.
    fn selected_pick_entry(&self) -> Option<&AvatarPick> {
        self.picks.as_ref()?.get(self.selected_pick)
    }

    /// The selected classified's list entry, if any.
    fn selected_classified_entry(&self) -> Option<&AvatarClassified> {
        self.classifieds.as_ref()?.get(self.selected_classified)
    }
}

/// One of the profile floater's tabs, in strip order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ProfileTab {
    /// The 2nd Life tab.
    SecondLife,
    /// The Web tab.
    Web,
    /// The Picks tab.
    Picks,
    /// The Classifieds tab.
    Classifieds,
    /// The 1st Life tab.
    FirstLife,
    /// The Notes tab.
    Notes,
}

impl ProfileTab {
    /// Every tab, in strip order.
    const ALL: [Self; 6] = [
        Self::SecondLife,
        Self::Web,
        Self::Picks,
        Self::Classifieds,
        Self::FirstLife,
        Self::Notes,
    ];

    /// The tab's index into [`ProfileUi::tabs`].
    const fn index(self) -> usize {
        match self {
            Self::SecondLife => 0,
            Self::Web => 1,
            Self::Picks => 2,
            Self::Classifieds => 3,
            Self::FirstLife => 4,
            Self::Notes => 5,
        }
    }
}

/// Which of **one window's** tabs need their content rebuilt from its
/// [`ProfileState`]. A component beside that state, on the same floater root.
#[derive(Component, Debug, Default)]
struct ProfileDirty(HashSet<ProfileTab>);

impl ProfileDirty {
    /// Mark one tab dirty.
    fn mark(&mut self, tab: ProfileTab) {
        self.0.insert(tab);
    }

    /// Mark every tab dirty (a fresh open, or a properties reply that feeds
    /// several tabs).
    fn mark_all(&mut self) {
        self.0.extend(ProfileTab::ALL);
    }

    /// Whether any tab is dirty.
    fn any(&self) -> bool {
        !self.0.is_empty()
    }

    /// Take the dirty set, leaving it empty.
    fn take(&mut self) -> HashSet<ProfileTab> {
        std::mem::take(&mut self.0)
    }
}

/// Entity handles for **one** profile window: its tab panels, and the
/// per-rebuild field entities that window's Save handlers read.
///
/// A component on the floater root beside [`ProfileState`], so two open
/// profiles keep their own fields — the window itself is the entity these hang
/// off, which is why there is no `panel` field here.
#[derive(Component)]
pub(crate) struct ProfileUi {
    /// The title text node (set to the avatar's name once resolved).
    title_text: Entity,
    /// The six tab panels, in tab order (2nd Life, Web, Picks, Classifieds,
    /// 1st Life, Notes).
    tabs: Vec<Entity>,
    /// The About field (own profile only).
    about_field: Option<Entity>,
    /// The profile URL field (own profile only).
    url_field: Option<Entity>,
    /// The Web tab's embedded browser view, when a profile URL is shown.
    web_view: Option<Entity>,
    /// The Web tab's load-status line under the browser view.
    web_status: Option<Entity>,
    /// The 1st-life About field (own profile only).
    fl_about_field: Option<Entity>,
    /// The Notes field.
    notes_field: Option<Entity>,
    /// The Pay amount field (another avatar only).
    pay_amount_field: Option<Entity>,
    /// The selected pick's name field (own profile only).
    pick_name_field: Option<Entity>,
    /// The selected pick's description field (own profile only).
    pick_desc_field: Option<Entity>,
    /// The selected (or new) classified's title field (own profile only).
    classified_name_field: Option<Entity>,
    /// The selected (or new) classified's description field.
    classified_desc_field: Option<Entity>,
    /// The new classified's price-for-listing field.
    classified_price_field: Option<Entity>,
    /// The `own` flag the 2nd Life tab's structure was built for (`None` = not
    /// built) — that tab is retained and updated in place, never respawned per
    /// reply.
    sl_built: Option<bool>,
    /// Retained value-node handles for the 2nd Life tab.
    sl_handles: SecondLifeHandles,
    /// The 2nd Life tab's retained group rows, keyed by group id (reconciled in
    /// place; a group name resolves in the row, so no respawn).
    sl_group_rows: Vec<(GroupKey, Entity)>,
    /// The signature the other five tabs were last built for — each rebuilds only
    /// when its own content changes (single-source or user-paced), never in the
    /// reply burst. Indexed by [`ProfileTab::index`].
    tab_sig: [Option<u64>; 6],
}

/// Retained handles for the 2nd Life tab. The skeleton (name / key / picture box /
/// facts + groups + about containers / buttons) is built **once** per subject; the
/// facts and About are **filled once** when the properties reply lands (so their
/// translated captions are never re-resolved); the name / partner name / groups
/// update **in place**.
#[derive(Debug, Default)]
struct SecondLifeHandles {
    /// The avatar name value node (updated when the name resolves).
    name: Option<Entity>,
    /// The picture image box (the properties reply requests its texture once).
    picture: Option<Entity>,
    /// Whether the picture's texture has been requested.
    picture_requested: bool,
    /// The facts column container, filled once when properties arrive.
    facts: Option<Entity>,
    /// Whether the facts have been filled.
    facts_built: bool,
    /// The partner value node (updated when the partner name resolves).
    partner: Option<Entity>,
    /// The groups list container (reconciled in place).
    groups_container: Option<Entity>,
    /// The About container (an editable field for own / a read block for another),
    /// filled once when properties arrive.
    about: Option<Entity>,
    /// Whether the About has been filled.
    about_built: bool,
    /// The own-profile "show in search" check glyph (updated in place).
    show_in_search_glyph: Option<Entity>,
    /// The "no groups" placeholder label, shown while the group list is empty.
    groups_none: Option<Entity>,
    /// A signature of the sorted group set the rows were last built for — the rows
    /// are (re)built only when this changes (a single groups reply → once), so the
    /// list is not churned per frame while still coming out alphabetically sorted.
    groups_sig: Option<u64>,
}

/// A button in the profile floater, naming what it does. One observer
/// ([`on_profile_action`]) dispatches on this.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileAction {
    /// Open a 1:1 IM with the shown avatar.
    Im,
    /// Offer the shown avatar a teleport to us.
    OfferTeleport,
    /// Offer the shown avatar friendship.
    AddFriend,
    /// Terminate the friendship with the shown avatar.
    RemoveFriend,
    /// Block (mute) the shown avatar.
    Block,
    /// File the shown avatar under one of the user's own contact sets
    /// ([`crate::contact_sets`]) — the floater asks which.
    AddToContactSet,
    /// Copy the shown avatar's SLURL (`secondlife:///app/agent/<id>/about`) to
    /// the OS clipboard, for pasting into chat / a notecard (the reference
    /// profile's "Copy" → agent SLURL).
    CopySlurl,
    /// Pay the shown avatar the amount in the Pay field.
    Pay,
    /// Flip the own profile's "Show in search" checkbox (saved on Save).
    ToggleShowInSearch,
    /// Save the own profile (about texts, URL, show-in-search).
    SaveProfile,
    /// Discard unsaved profile edits (repaint from the last received state).
    DiscardProfile,
    /// Save the Notes field for the shown avatar.
    SaveNotes,
    /// Create a new pick at the agent's current location.
    NewPick,
    /// Delete the selected pick.
    DeletePick,
    /// Save the selected pick's name / description (and location if set).
    SavePick,
    /// Move the selected pick to the agent's current location on next save.
    SetPickLocation,
    /// Teleport to the selected pick.
    TeleportToPick,
    /// Open the new-classified editor.
    NewClassified,
    /// Close the new-classified editor without publishing.
    CancelNewClassified,
    /// Delete the selected classified.
    DeleteClassified,
    /// Save the selected classified (or publish the new one).
    SaveClassified,
    /// Move the selected classified to the current location on next save.
    SetClassifiedLocation,
    /// Teleport to the selected classified.
    TeleportToClassified,
    /// Cycle the edited classified's category.
    CycleCategory,
    /// Toggle the edited classified between General and Moderate content.
    CycleContentType,
    /// Toggle the edited classified's weekly auto-renew.
    ToggleAutoRenew,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin owning the avatar profile floater.
#[derive(Debug)]
pub struct AvatarProfilePlugin;

impl Plugin for AvatarProfilePlugin {
    /// Register the open message, the shared double-click tracker, and the
    /// open / ingest / rebuild / poll systems.
    ///
    /// Nothing spawns at `Startup`: a profile window exists only while a
    /// resident's profile is open, so `open_profile` both spawns the instance
    /// and builds its content.
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfileGroupClick>()
            .add_message::<OpenAvatarProfile>()
            .add_systems(
                Update,
                (
                    // After the manager's command pass: the click that opens a
                    // profile often raises the window it landed in as well, and
                    // the later raise wins (`FloaterSystems`).
                    open_profile.after(FloaterSystems::Commands),
                    // The per-window systems cost nothing while no profile is
                    // open — which, unlike a singleton window that merely
                    // hides, is most of a session. `ingest_profile_events` in
                    // particular walks the frame's whole session-event stream,
                    // and there is no point doing that for nobody.
                    (
                        ingest_profile_events,
                        track_list_selection,
                        rebuild_profile_tabs,
                        poll_profile_textures,
                        update_profile_web_status,
                    )
                        .chain()
                        .run_if(any_with_component::<ProfileState>),
                )
                    .chain(),
            );
    }
}

/// The profile floater's stable [`crate::floater::Floater::id`] — the **kind**;
/// which resident an instance shows is its [`FloaterKey`].
const PROFILE_FLOATER_ID: &str = "avatar-profile";

/// The [`FloaterKey`] of the window showing `agent`'s profile.
///
/// A [subject](FloaterKey::Subject) key: instances are told apart by the agent
/// id, and none of them persists geometry — a settings entry per resident whose
/// profile was ever opened is exactly what that variant exists to avoid.
fn profile_key(agent: AgentKey) -> FloaterKey {
    FloaterKey::subject(&agent)
}

/// The avatar profile floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn avatar_profile_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: PROFILE_FLOATER_ID,
        title: "Profile".to_owned(),
        position: Vec2::new(300.0, 80.0),
        // A definite, resizable content area — the reference profile
        // floater has a default rect and `can_resize="true"` (485×510,
        // min 480×510; ours differs because the tab panels bound their
        // content width). Roomy enough that the 2nd Life tab fits without
        // scrolling; smaller sizes scroll with a trailing scrollbar.
        default_size: Some(Vec2::new(420.0, 600.0)),
        min_size: Some(Vec2::new(370.0, 420.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Build one profile window's content into the chrome `handle`, and hang this
/// window's state off its root: the six-tab container, a [`ProfileState`] on
/// `target`, its [`ProfileDirty`] flags and its [`ProfileUi`] handles.
///
/// Called by [`open_profile`] the moment an instance is spawned — a keyed
/// window is only ever created because someone opened *this* subject, so there
/// is nothing to defer (the singleton windows' `DeferredFloaterContent` exists
/// to keep three dozen never-opened windows out of the per-frame UI walk; a
/// window that exists only while it is open costs nothing when it does not).
fn build_profile_content(commands: &mut Commands, handle: FloaterHandle, target: AgentKey) {
    let labels: Vec<String> = [
        "profile-tab-second-life",
        "profile-tab-web",
        "profile-tab-picks",
        "profile-tab-classifieds",
        "profile-tab-first-life",
        "profile-tab-notes",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let tabs: TabContainerHandle = spawn_tab_container(
        commands,
        handle.content,
        &TabSpec {
            element: "profile-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: PROFILE_FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    // The floater is resizable (a definite content area), so the widget must
    // track it rather than content-size — panels grow and scroll.
    fill_tab_container(commands, TabPlacement::BlockStart, &tabs);
    // This window's whole model, on the window: the subject, what has yet to be
    // repainted, and where the fields are. Every tab starts dirty — nothing has
    // been drawn yet.
    let mut initial = ProfileDirty::default();
    initial.mark_all();
    commands.entity(handle.root).insert((
        ProfileState::new(target),
        initial,
        ProfileUi {
            title_text: handle.title_text,
            tabs: tabs.panels,
            about_field: None,
            url_field: None,
            web_view: None,
            web_status: None,
            fl_about_field: None,
            notes_field: None,
            pay_amount_field: None,
            pick_name_field: None,
            pick_desc_field: None,
            classified_name_field: None,
            classified_desc_field: None,
            classified_price_field: None,
            sl_built: None,
            sl_handles: SecondLifeHandles::default(),
            sl_group_rows: Vec::new(),
            tab_sig: [None; 6],
        },
    ));
}

// ---------------------------------------------------------------------------
// Open / ingest / selection.
// ---------------------------------------------------------------------------

/// Open a profile **per resident** (`viewer-profile-floater-single-instance`):
/// raise this avatar's window when it is already up, and otherwise spawn one,
/// build its content and fire the profile requests behind it.
///
/// Every open in the frame is honoured, not just the last: two name links
/// clicked in the same frame are two residents, and each gets a window. Only a
/// *new* window sends requests — re-opening a resident already on screen brings
/// their window forward with everything it has already received intact.
fn open_profile(
    mut opens: MessageReader<OpenAvatarProfile>,
    avatars: Res<AvatarState>,
    mut floaters: KeyedFloaters,
    mut dirty: Query<&mut ProfileDirty>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for open in opens.read().copied() {
        let agent = open.agent;
        let opened = floaters.open(avatar_profile_floater_spec(), profile_key(agent));
        let KeyedFloaterOpen::Spawned(handle) = opened else {
            // Already up: repaint it from what it has, so an open after an edit
            // shows the edit. Each tab still skips itself when its content
            // signature has not moved, so this costs nothing when nothing has.
            if let Ok(mut dirty) = dirty.get_mut(opened.root()) {
                dirty.mark_all();
            }
            continue;
        };
        commands
            .entity(handle.title_text)
            .insert(Translated::new("profile-title"));
        build_profile_content(&mut commands, handle, agent);
        sl_commands.write(SlCommand(Command::RequestAvatarProperties(agent)));
        sl_commands.write(SlCommand(Command::RequestAvatarPicks(agent)));
        sl_commands.write(SlCommand(Command::RequestAvatarClassifieds(agent)));
        sl_commands.write(SlCommand(Command::RequestAvatarNotes(agent)));
        if avatars.name_of(agent).is_none() {
            sl_commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
        }
    }
}

/// Fold profile-related session events into **every** open window whose avatar
/// they are about, marking that window's affected tabs dirty.
///
/// The frame's events are collected once and replayed per window: a reader is
/// consumed by the first pass over it, and with two profiles open the second
/// would otherwise see nothing. Each window keeps its own filter on its own
/// subject, so a reply for one resident never touches the other's tabs.
fn ingest_profile_events(
    mut events: MessageReader<SlEvent>,
    mut instances: Query<(&mut ProfileState, &mut ProfileDirty)>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for (mut state, mut dirty) in &mut instances {
        let target = state.target;
        ingest_for_window(&frame, target, &mut state, &mut dirty, &mut sl_commands);
    }
}

/// Fold this frame's events into one window's state (see
/// [`ingest_profile_events`]).
fn ingest_for_window(
    frame: &[&SlEvent],
    target: AgentKey,
    state: &mut ProfileState,
    dirty: &mut ProfileDirty,
    sl_commands: &mut MessageWriter<SlCommand>,
) {
    for event in frame {
        match &event.0 {
            SlSessionEvent::AvatarProperties(properties) => {
                if properties.avatar_id != target {
                    continue;
                }
                state.show_in_search = properties.flags & FLAG_ALLOW_PUBLISH != 0;
                // Show the partner by name once it resolves.
                if let Some(partner) = properties.partner_id {
                    sl_commands.write(SlCommand(Command::RequestAvatarNames(vec![partner])));
                }
                state.properties = Some((**properties).clone());
                dirty.mark(ProfileTab::SecondLife);
                dirty.mark(ProfileTab::Web);
                dirty.mark(ProfileTab::FirstLife);
            }
            SlSessionEvent::AvatarGroups {
                avatar_id, groups, ..
            } => {
                if *avatar_id != target {
                    continue;
                }
                state.groups = Some(groups.clone());
                dirty.mark(ProfileTab::SecondLife);
            }
            SlSessionEvent::AvatarPicks { target_id, picks } => {
                if *target_id != target.uuid() {
                    continue;
                }
                state.picks = Some(picks.clone());
                if state.selected_pick >= picks.len() {
                    state.selected_pick = 0;
                }
                // Fetch the selected pick's detail right away.
                if let Some(pick) = state.selected_pick_entry()
                    && !state.pick_info.contains_key(&pick.pick_id)
                {
                    sl_commands.write(SlCommand(Command::RequestPickInfo {
                        creator_id: target,
                        pick_id: pick.pick_id,
                    }));
                }
                dirty.mark(ProfileTab::Picks);
            }
            SlSessionEvent::AvatarClassifieds {
                target_id,
                classifieds,
            } => {
                if *target_id != target.uuid() {
                    continue;
                }
                state.classifieds = Some(classifieds.clone());
                if state.selected_classified >= classifieds.len() {
                    state.selected_classified = 0;
                }
                if let Some(classified) = state.selected_classified_entry() {
                    let id = classified.classified_id;
                    if !state.classified_info.contains_key(&id) {
                        sl_commands.write(SlCommand(Command::RequestClassifiedInfo(id)));
                    }
                }
                dirty.mark(ProfileTab::Classifieds);
            }
            SlSessionEvent::AvatarNotes { target_id, notes } => {
                if *target_id != target.uuid() {
                    continue;
                }
                state.notes = Some(notes.clone());
                dirty.mark(ProfileTab::Notes);
            }
            SlSessionEvent::PickInfo(info) => {
                if info.creator_id != target {
                    continue;
                }
                state.pick_info.insert(info.pick_id, (**info).clone());
                dirty.mark(ProfileTab::Picks);
            }
            SlSessionEvent::ClassifiedInfo(info) => {
                if info.creator_id != target {
                    continue;
                }
                state
                    .classified_info
                    .insert(info.classified_id, (**info).clone());
                dirty.mark(ProfileTab::Classifieds);
            }
            _other => {}
        }
    }
}

/// Track the pick / classified list strips' selection: update the state,
/// request the newly-selected entry's detail if uncached, and repaint that
/// tab. The strips are respawned on rebuild with `active` taken from the
/// state, so an unchanged selection is a no-op.
fn track_list_selection(
    strips: Query<(Entity, &TabStrip), Changed<TabStrip>>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut instances: Query<(&mut ProfileState, &mut ProfileDirty)>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for (entity, strip) in &strips {
        // Which window's list this is: with two profiles open, both carry a
        // strip under the same element id, so the answer has to come from the
        // tree rather than from the id.
        let Some(instance) = host_floater(entity, &parents, &floaters) else {
            continue;
        };
        let Ok((mut state, mut dirty)) = instances.get_mut(instance) else {
            continue;
        };
        let target = state.target;
        if strip.element == PICKS_STRIP_ELEMENT {
            if strip.active == state.selected_pick {
                continue;
            }
            state.selected_pick = strip.active;
            if let Some(pick) = state.selected_pick_entry()
                && !state.pick_info.contains_key(&pick.pick_id)
            {
                sl_commands.write(SlCommand(Command::RequestPickInfo {
                    creator_id: target,
                    pick_id: pick.pick_id,
                }));
            }
            dirty.mark(ProfileTab::Picks);
        } else if strip.element == CLASSIFIEDS_STRIP_ELEMENT {
            if strip.active == state.selected_classified {
                continue;
            }
            state.selected_classified = strip.active;
            if let Some(classified) = state.selected_classified_entry() {
                let id = classified.classified_id;
                if !state.classified_info.contains_key(&id) {
                    sl_commands.write(SlCommand(Command::RequestClassifiedInfo(id)));
                }
            }
            dirty.mark(ProfileTab::Classifieds);
        }
    }
}

// ---------------------------------------------------------------------------
// Rebuild.
// ---------------------------------------------------------------------------

/// Rebuild every open window's dirty tabs from that window's own state.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the per-window state / \
              dirty flags / UI handles, the identity / name / friendship sources, the texture \
              pipeline, and the spawn outputs"
)]
fn rebuild_profile_tabs(
    mut instances: Query<(Entity, &mut ProfileState, &mut ProfileDirty, &mut ProfileUi)>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    friends: Res<FriendsModel>,
    groups_model: Res<GroupsModel>,
    mut boost: MessageWriter<BoostTexture>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    for (panel, mut state, mut dirty, mut ui) in &mut instances {
        if !dirty.any() {
            continue;
        }
        rebuild_one_profile(
            panel,
            &mut state,
            &mut dirty,
            &mut ui,
            &identity,
            &avatars,
            &friends,
            &groups_model,
            &mut boost,
            &children,
            &mut texts,
            &mut commands,
        );
    }
}

/// Rebuild one window's dirty tabs (see [`rebuild_profile_tabs`]).
#[expect(
    clippy::too_many_arguments,
    reason = "this is `rebuild_profile_tabs`'s body with the window's own three pieces of \
              state taken by reference instead of by query — splitting it further would only \
              scatter one repaint across several functions"
)]
fn rebuild_one_profile(
    window: Entity,
    state: &mut ProfileState,
    dirty: &mut ProfileDirty,
    ui: &mut ProfileUi,
    identity: &SlIdentity,
    avatars: &AvatarState,
    friends: &FriendsModel,
    groups_model: &GroupsModel,
    boost: &mut MessageWriter<BoostTexture>,
    children: &Query<&Children>,
    texts: &mut Query<&mut Text>,
    commands: &mut Commands,
) {
    let target = state.target;
    let own = identity.agent_id == Some(target);
    // Title: the avatar's shown name once known (a plain string, not a Fluent
    // key) — the alias the user gave them, else the display name, else legacy.
    if let Some(name) = avatars.shown_name_of(target)
        && let Ok(mut text) = texts.get_mut(ui.title_text)
    {
        name.clone_into(&mut text.0);
        commands.entity(ui.title_text).remove::<Translated>();
    }
    let dirty_tabs = dirty.take();
    // Dropping a dragged inventory row anywhere on another avatar's profile
    // floater gives them the item (`viewer-inventory-give-via-profile`) — the
    // root carries the target and the drop resolution walks up to it.
    if own {
        commands.entity(window).remove::<AgentDropTarget>();
    } else {
        commands.entity(window).insert(AgentDropTarget(target));
    }
    let build = BuildContext {
        target,
        own,
        avatars,
        friends,
    };
    for tab in ProfileTab::ALL {
        if !dirty_tabs.contains(&tab) {
            continue;
        }
        let Some(panel) = ui.tabs.get(tab.index()).copied() else {
            continue;
        };
        // The 2nd Life tab is retained: its skeleton is built once per subject and
        // its values update in place — it is fed by several near-simultaneous
        // replies (properties + groups + partner name), so a per-reply respawn is
        // exactly the same-frame build+teardown that races bevy_flair.
        if tab == ProfileTab::SecondLife {
            if ui.sl_built != Some(own) {
                despawn_children(children, commands, panel);
                ui.sl_handles = SecondLifeHandles::default();
                ui.sl_group_rows.clear();
                build_second_life_structure(commands, panel, &build, ui);
                ui.sl_built = Some(own);
            }
            update_second_life(commands, &build, state, ui, boost, texts, groups_model);
            continue;
        }
        // The other five tabs are single-source (properties / notes) or user-paced
        // (a pick / classified selection): rebuild only when the tab's content
        // signature actually changes, so the reply burst never respawns them.
        let sig = tab_signature(tab, state, own);
        if ui.tab_sig.get(tab.index()).copied().flatten() == Some(sig) {
            continue;
        }
        if let Some(slot) = ui.tab_sig.get_mut(tab.index()) {
            *slot = Some(sig);
        }
        despawn_children(children, commands, panel);
        match tab {
            ProfileTab::Web => build_web_tab(commands, panel, &build, state, ui),
            ProfileTab::Picks => build_picks_tab(commands, panel, &build, state, ui, boost),
            ProfileTab::Classifieds => {
                build_classifieds_tab(commands, panel, &build, state, ui, boost);
            }
            ProfileTab::FirstLife => {
                build_first_life_tab(commands, panel, &build, state, ui, boost);
            }
            ProfileTab::Notes => build_notes_tab(commands, panel, state, ui),
            ProfileTab::SecondLife => {}
        }
    }
}

/// A content signature for a signature-skipped tab: the tab is despawned+rebuilt
/// only when this changes, so the reply burst never respawns it. Covers everything
/// the tab renders (so a real change still rebuilds).
fn tab_signature(tab: ProfileTab, state: &ProfileState, own: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    own.hash(&mut hasher);
    match tab {
        ProfileTab::SecondLife => {}
        ProfileTab::Web => {
            state
                .properties
                .as_ref()
                .map(|props| props.profile_url.clone())
                .hash(&mut hasher);
        }
        ProfileTab::FirstLife => {
            if let Some(props) = state.properties.as_ref() {
                props.fl_about_text.hash(&mut hasher);
                props.fl_image_id.uuid().hash(&mut hasher);
            }
        }
        ProfileTab::Notes => state.notes.is_some().hash(&mut hasher),
        ProfileTab::Picks => {
            state.selected_pick.hash(&mut hasher);
            for pick in state.picks.iter().flatten() {
                pick.pick_id.uuid().hash(&mut hasher);
                pick.name.hash(&mut hasher);
            }
            if let Some(pick) = state.selected_pick_entry() {
                state
                    .pick_info
                    .contains_key(&pick.pick_id)
                    .hash(&mut hasher);
                state
                    .pick_use_current
                    .contains(&pick.pick_id)
                    .hash(&mut hasher);
            }
        }
        ProfileTab::Classifieds => {
            state.selected_classified.hash(&mut hasher);
            hash_classified_draft(state.new_classified.as_ref(), &mut hasher);
            for classified in state.classifieds.iter().flatten() {
                classified.classified_id.uuid().hash(&mut hasher);
                classified.name.hash(&mut hasher);
            }
            if let Some(classified) = state.selected_classified_entry() {
                let id = classified.classified_id;
                state.classified_info.contains_key(&id).hash(&mut hasher);
                state.classified_use_current.contains(&id).hash(&mut hasher);
                hash_classified_draft(state.classified_drafts.get(&id), &mut hasher);
            }
        }
    }
    hasher.finish()
}

/// Fold a classified edit draft (category / mature / auto-renew) into the tab
/// signature so its cycle / toggle edits rebuild the detail.
fn hash_classified_draft(
    draft: Option<&ClassifiedDraft>,
    hasher: &mut std::collections::hash_map::DefaultHasher,
) {
    match draft {
        Some(draft) => {
            true.hash(hasher);
            draft.category.to_string().hash(hasher);
            draft.mature.hash(hasher);
            draft.auto_renew.hash(hasher);
        }
        None => false.hash(hasher),
    }
}

/// The read-only context every tab builder shares.
struct BuildContext<'world> {
    /// The shown avatar.
    target: AgentKey,
    /// Whether the shown avatar is the logged-in agent.
    own: bool,
    /// Name resolution.
    avatars: &'world AvatarState,
    /// Friendship state (Add vs Remove Friend).
    friends: &'world FriendsModel,
}

/// Despawn every child of `parent`.
fn despawn_children(children: &Query<&Children>, commands: &mut Commands, parent: Entity) {
    if let Ok(existing) = children.get(parent) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
}

/// Build the 2nd Life tab's fixed skeleton once for `own`: name / key / picture box
/// / empty facts + groups + about containers / the action or Save buttons. The
/// facts and About are filled once by [`fill_second_life_from_properties`]; the
/// name / partner / groups update in place. Nothing here is respawned per reply.
fn build_second_life_structure(
    commands: &mut Commands,
    panel: Entity,
    build: &BuildContext,
    ui: &mut ProfileUi,
) {
    ui.about_field = None;
    ui.pay_amount_field = None;
    let name_row = spawn_labeled_row(commands, panel, "profile-name");
    ui.sl_handles.name = Some(spawn_value_node(commands, name_row, LABEL_COLOR));
    let key_row = spawn_labeled_row(commands, panel, "profile-key");
    spawn_value_label(commands, key_row, build.target.to_string(), DIM_LABEL_COLOR);

    // Picture beside the (initially empty) facts column, as the reference lays out.
    let picture_row = commands
        .spawn((
            Node {
                align_items: AlignItems::FlexStart,
                ..row(Val::Px(8.0))
            },
            ChildOf(panel),
        ))
        .id();
    ui.sl_handles.picture = Some(spawn_image_box(
        commands,
        picture_row,
        Vec2::splat(PROFILE_IMAGE_EDGE),
    ));
    ui.sl_handles.facts = Some(
        commands
            .spawn((
                Node {
                    ..column(Val::Px(4.0))
                },
                ChildOf(picture_row),
            ))
            .id(),
    );

    // Groups — a bounded, scrollable, full-width list of clickable group names
    // (double-click opens the group profile) with a visible scrollbar beside it, so
    // a long membership list does not push the rest of the tab down
    // (`viewer-avatar-profile-group-list`).
    spawn_section_label(commands, panel, "profile-groups");
    let groups_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                max_height: Val::Px(GROUP_LIST_HEIGHT),
                align_items: AlignItems::Stretch,
                ..row(Val::Px(0.0))
            },
            ChildOf(panel),
        ))
        .id();
    let groups_container = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..column(Val::Px(1.0))
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.20)),
            ScrollPosition::default(),
            crate::ui_tab::TabViewport { vertical: true },
            Name::new("profile-groups-list"),
            ChildOf(groups_row),
        ))
        .id();
    ui.sl_handles.groups_container = Some(groups_container);
    // A visible vertical scrollbar driving the group list.
    commands
        .spawn((
            Scrollbar {
                target: groups_container,
                orientation: ControlOrientation::Vertical,
                min_thumb_length: SCROLLBAR_MIN_THUMB,
            },
            Node {
                width: Val::Px(SCROLLBAR_THICKNESS),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(SCROLLBAR_TRACK_COLOR),
            Name::new("profile-groups-scrollbar"),
            ChildOf(groups_row),
        ))
        .with_child((
            ScrollbarThumb::default(),
            BackgroundColor(SCROLLBAR_THUMB_COLOR),
        ));

    // About — an empty container filled once from properties.
    spawn_section_label(commands, panel, "profile-about");
    ui.sl_handles.about = Some(
        commands
            .spawn((
                Node {
                    ..column(Val::Px(2.0))
                },
                ChildOf(panel),
            ))
            .id(),
    );

    if build.own {
        ui.sl_handles.show_in_search_glyph = Some(spawn_check_button(
            commands,
            panel,
            "profile-show-in-search",
            ProfileAction::ToggleShowInSearch,
            false,
        ));
        let buttons = spawn_button_row(commands, panel);
        spawn_action_button(
            commands,
            buttons,
            "profile-save",
            ProfileAction::SaveProfile,
            3,
        );
        spawn_action_button(
            commands,
            buttons,
            "profile-discard",
            ProfileAction::DiscardProfile,
            4,
        );
    } else {
        let buttons = spawn_button_row(commands, panel);
        spawn_action_button(commands, buttons, "profile-im", ProfileAction::Im, 3);
        spawn_action_button(
            commands,
            buttons,
            "profile-offer-teleport",
            ProfileAction::OfferTeleport,
            4,
        );
        if build.friends.is_friend(build.target) {
            spawn_action_button(
                commands,
                buttons,
                "profile-remove-friend",
                ProfileAction::RemoveFriend,
                5,
            );
        } else {
            spawn_action_button(
                commands,
                buttons,
                "profile-add-friend",
                ProfileAction::AddFriend,
                5,
            );
        }
        spawn_action_button(commands, buttons, "profile-block", ProfileAction::Block, 6);
        spawn_action_button(
            commands,
            buttons,
            "profile-add-to-contact-set",
            ProfileAction::AddToContactSet,
            7,
        );
        spawn_action_button(
            commands,
            buttons,
            "profile-copy-slurl",
            ProfileAction::CopySlurl,
            8,
        );
        spawn_disabled_button(commands, buttons, "profile-find-on-map");
        spawn_disabled_button(commands, buttons, "profile-invite-to-group");
        spawn_section_label(commands, panel, "profile-share");
        spawn_key_label(commands, panel, "profile-share-hint", DIM_LABEL_COLOR);
        let pay_row = spawn_labeled_row(commands, panel, "profile-pay-amount");
        ui.pay_amount_field = Some(spawn_text_input(
            commands,
            pay_row,
            &TextInputSpec {
                initial: "1".to_owned(),
                font_size: PROFILE_FONT_SIZE,
                width_glyphs: 8.0,
                tab_index: 7,
                ..TextInputSpec::new("profile-pay-amount", TextInputKind::NonNegativeInteger)
            },
        ));
        spawn_action_button(commands, pay_row, "profile-pay", ProfileAction::Pay, 8);
    }
}

/// Fill the 2nd Life facts + About once, when the properties reply is available —
/// a single spawn into the persistent containers (never a respawn), so the
/// translated captions are resolved once and no node is torn down mid-frame.
fn fill_second_life_from_properties(
    commands: &mut Commands,
    build: &BuildContext,
    state: &mut ProfileState,
    ui: &mut ProfileUi,
    boost: &mut MessageWriter<BoostTexture>,
) {
    let Some(props) = state.properties.clone() else {
        return;
    };
    // Request the picture texture once.
    if !ui.sl_handles.picture_requested
        && let Some(node) = ui.sl_handles.picture
    {
        request_ui_texture(commands, Some(props.image_id), node, state, boost);
        ui.sl_handles.picture_requested = true;
    }
    // Facts (once).
    if !ui.sl_handles.facts_built
        && let Some(facts) = ui.sl_handles.facts
    {
        spawn_key_label(
            commands,
            facts,
            online_caption_key(props.flags),
            LABEL_COLOR,
        );
        if !props.born_on.is_empty() {
            let born_row = spawn_labeled_row(commands, facts, "profile-birthdate");
            spawn_value_label(commands, born_row, props.born_on.clone(), LABEL_COLOR);
        }
        let account_row = spawn_labeled_row(commands, facts, "profile-account");
        match account_caption(&props.charter_member) {
            AccountCaption::Key(key) => spawn_key_label(commands, account_row, key, LABEL_COLOR),
            AccountCaption::Literal(text) => {
                spawn_value_label(commands, account_row, text, LABEL_COLOR);
            }
        }
        spawn_key_label(
            commands,
            facts,
            payment_caption_key(props.flags),
            DIM_LABEL_COLOR,
        );
        let partner_row = spawn_labeled_row(commands, facts, "profile-partner");
        match props.partner_id {
            Some(partner) => {
                ui.sl_handles.partner = Some(spawn_value_node(commands, partner_row, LABEL_COLOR));
                if let Some(node) = ui.sl_handles.partner {
                    commands
                        .entity(node)
                        .insert(Text::new(build.avatars.label_text(partner)));
                }
            }
            None => spawn_key_label(
                commands,
                partner_row,
                "profile-partner-none",
                DIM_LABEL_COLOR,
            ),
        }
        ui.sl_handles.facts_built = true;
    }
    // About (once).
    if !ui.sl_handles.about_built
        && let Some(about) = ui.sl_handles.about
    {
        if build.own {
            ui.about_field = Some(spawn_text_input(
                commands,
                about,
                &TextInputSpec {
                    initial: props.about_text.clone(),
                    font_size: PROFILE_FONT_SIZE,
                    visible_lines: 5.0,
                    tab_index: 2,
                    max_characters: Some(510),
                    ..TextInputSpec::new("profile-about", TextInputKind::Multiline)
                },
            ));
        } else {
            spawn_text_block(commands, about, props.about_text.clone());
        }
        ui.sl_handles.about_built = true;
    }
}

/// Update the 2nd Life tab's in-place values from the state — the name and partner
/// name (resolve async), fill facts / About once from properties, and reconcile the
/// group rows. No respawn of built content.
fn update_second_life(
    commands: &mut Commands,
    build: &BuildContext,
    state: &mut ProfileState,
    ui: &mut ProfileUi,
    boost: &mut MessageWriter<BoostTexture>,
    texts: &mut Query<&mut Text>,
    groups_model: &GroupsModel,
) {
    set_value_node(
        texts,
        ui.sl_handles.name,
        &build.avatars.label_text(build.target),
    );
    fill_second_life_from_properties(commands, build, state, ui, boost);
    if let Some(partner) = state.properties.as_ref().and_then(|props| props.partner_id) {
        set_value_node(
            texts,
            ui.sl_handles.partner,
            &build.avatars.label_text(partner),
        );
    }
    set_check_glyph(
        texts,
        ui.sl_handles.show_in_search_glyph,
        state.show_in_search,
    );
    // The **own** profile lists the full membership set (the reference shows your
    // groups even when none are flagged "show in my profile"); **another** avatar's
    // profile shows only the groups they list, from `AvatarGroupsReply`. Either way
    // skip the nil-group-id padding entry the grid sends for a group-less avatar.
    let nil_group = GroupKey::from(Uuid::nil());
    let list: Option<Vec<(GroupKey, String)>> = if build.own {
        Some(
            groups_model
                .group_ids()
                .into_iter()
                .map(|id| {
                    (
                        id,
                        groups_model.group_name(id).unwrap_or_default().to_owned(),
                    )
                })
                .collect(),
        )
    } else {
        state.groups.as_ref().map(|groups| {
            groups
                .iter()
                .filter(|group| group.group_id != nil_group)
                .map(|group| (group.group_id, group.group_name.clone()))
                .collect()
        })
    };
    reconcile_profile_groups(commands, list.as_deref(), ui);
}

/// Reconcile the 2nd Life group rows in place from the resolved `(id, name)` list
/// (`None` = not loaded yet): sorted alphabetically, (re)built only when the set
/// changes (built once for a whole reply), never a per-frame respawn.
fn reconcile_profile_groups(
    commands: &mut Commands,
    groups: Option<&[(GroupKey, String)]>,
    ui: &mut ProfileUi,
) {
    let Some(container) = ui.sl_handles.groups_container else {
        return;
    };
    let Some(groups) = groups else {
        return;
    };
    // Sort alphabetically (case-folded, id tie-break), as the reference does.
    let mut sorted: Vec<&(GroupKey, String)> = groups.iter().collect();
    sorted.sort_by(|left, right| {
        left.1
            .to_lowercase()
            .cmp(&right.1.to_lowercase())
            .then_with(|| left.0.uuid().cmp(&right.0.uuid()))
    });
    // Rebuild only when the sorted set changes — a whole reply builds the rows once;
    // nothing is despawned per frame.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (id, name) in &sorted {
        id.uuid().hash(&mut hasher);
        name.hash(&mut hasher);
    }
    let sig = hasher.finish();
    if ui.sl_handles.groups_sig == Some(sig) {
        return;
    }
    ui.sl_handles.groups_sig = Some(sig);
    for (_, row) in ui.sl_group_rows.drain(..) {
        if let Ok(mut entity) = commands.get_entity(row) {
            entity.despawn();
        }
    }
    if let Some(none_label) = ui.sl_handles.groups_none.take()
        && let Ok(mut entity) = commands.get_entity(none_label)
    {
        entity.despawn();
    }
    if sorted.is_empty() {
        ui.sl_handles.groups_none = Some(spawn_key_label_node(
            commands,
            container,
            "profile-groups-none",
            DIM_LABEL_COLOR,
        ));
        return;
    }
    for (id, name) in sorted {
        let row = spawn_profile_group_row(commands, container, *id, name);
        ui.sl_group_rows.push((*id, row));
    }
}

/// Build the Web tab: the profile URL (editable for one's own profile). The
/// reference renders the URL's feed in an embedded browser; that upgrade is
/// `viewer-profile-web-tab-browser` (blocked on CEF).
fn build_web_tab(
    commands: &mut Commands,
    panel: Entity,
    build: &BuildContext,
    state: &ProfileState,
    ui: &mut ProfileUi,
) {
    ui.url_field = None;
    ui.web_view = None;
    ui.web_status = None;
    spawn_section_label(commands, panel, "profile-web-url");
    let url = state
        .properties
        .as_ref()
        .map(|props| props.profile_url.clone())
        .unwrap_or_default();
    if build.own {
        ui.url_field = Some(spawn_text_input(
            commands,
            panel,
            &TextInputSpec {
                initial: url.clone(),
                font_size: PROFILE_FONT_SIZE,
                width_glyphs: 30.0,
                tab_index: 2,
                max_characters: Some(254),
                ..TextInputSpec::new("profile-url", TextInputKind::Line)
            },
        ));
        let buttons = spawn_button_row(commands, panel);
        spawn_action_button(
            commands,
            buttons,
            "profile-save",
            ProfileAction::SaveProfile,
            3,
        );
        spawn_action_button(
            commands,
            buttons,
            "profile-discard",
            ProfileAction::DiscardProfile,
            4,
        );
    } else if url.is_empty() {
        spawn_key_label(commands, panel, "profile-web-none", DIM_LABEL_COLOR);
    } else {
        spawn_text_block(commands, panel, url.clone());
    }
    // The reference renders the profile URL's page in an embedded browser
    // below the URL line (`LLPanelProfileWeb`), with a load-status string —
    // navigation driven by code, no visible URL bar
    // (`viewer-profile-web-tab-browser`).
    if let Some(page) = crate::system_browser::normalize_web_url(&url) {
        ui.web_view = Some(crate::browser_widget::spawn_browser_view(
            commands,
            panel,
            &crate::browser_widget::BrowserViewSpec {
                initial_url: page,
                isolated: false,
                tab_index: 5,
                fixed_height: Some(320.0),
            },
        ));
        ui.web_status = Some(
            commands
                .spawn((
                    Text::default(),
                    Translated::new("profile-web-loading"),
                    UiFont::Sans.at(PROFILE_FONT_SIZE),
                    TextColor(DIM_LABEL_COLOR),
                    ChildOf(panel),
                ))
                .id(),
        );
    }
}

/// Build the Picks tab: the pick list as a left tab strip with the selected
/// pick's detail, plus New / Delete for one's own profile.
fn build_picks_tab(
    commands: &mut Commands,
    panel: Entity,
    build: &BuildContext,
    state: &mut ProfileState,
    ui: &mut ProfileUi,
    boost: &mut MessageWriter<BoostTexture>,
) {
    ui.pick_name_field = None;
    ui.pick_desc_field = None;
    spawn_key_label(commands, panel, "profile-picks-header", DIM_LABEL_COLOR);
    let picks = state.picks.clone().unwrap_or_default();
    if build.own {
        let buttons = spawn_button_row(commands, panel);
        if picks.len() < MAX_PICKS {
            spawn_action_button(
                commands,
                buttons,
                "profile-pick-new",
                ProfileAction::NewPick,
                2,
            );
        } else {
            spawn_disabled_button(commands, buttons, "profile-pick-new");
        }
        if picks.is_empty() {
            spawn_disabled_button(commands, buttons, "profile-pick-delete");
        } else {
            spawn_action_button(
                commands,
                buttons,
                "profile-pick-delete",
                ProfileAction::DeletePick,
                3,
            );
        }
    }
    if picks.is_empty() {
        spawn_key_label(commands, panel, "profile-picks-none", DIM_LABEL_COLOR);
        return;
    }
    let labels: Vec<String> = picks.iter().map(|pick| pick.name.clone()).collect();
    let tabs = spawn_tab_container(
        commands,
        panel,
        &TabSpec {
            element: PICKS_STRIP_ELEMENT,
            placement: TabPlacement::InlineStart,
            labels: &labels,
            active: state.selected_pick,
            tab_index: 4,
            font_size: PROFILE_FONT_SIZE,
            strip_width: Some(LIST_STRIP_WIDTH),
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
        },
    );
    // Only the selected pick's panel gets detail content; the others fill in
    // when selected (their info may not be fetched yet).
    let Some(detail_panel) = tabs.panels.get(state.selected_pick).copied() else {
        return;
    };
    let Some(info) = state
        .selected_pick_entry()
        .and_then(|pick| state.pick_info.get(&pick.pick_id))
        .cloned()
    else {
        spawn_key_label(commands, detail_panel, "profile-loading", DIM_LABEL_COLOR);
        return;
    };
    spawn_snapshot(commands, detail_panel, info.snapshot_id, state, boost);
    let name_row = spawn_labeled_row(commands, detail_panel, "profile-pick-name");
    if build.own {
        ui.pick_name_field = Some(spawn_text_input(
            commands,
            name_row,
            &TextInputSpec {
                initial: info.name.clone(),
                font_size: PROFILE_FONT_SIZE,
                width_glyphs: 18.0,
                tab_index: 5,
                max_characters: Some(63),
                ..TextInputSpec::new("profile-pick-name", TextInputKind::Line)
            },
        ));
    } else {
        spawn_value_label(commands, name_row, info.name.clone(), LABEL_COLOR);
    }
    spawn_section_label(commands, detail_panel, "profile-pick-desc");
    if build.own {
        ui.pick_desc_field = Some(spawn_text_input(
            commands,
            detail_panel,
            &TextInputSpec {
                initial: info.description.clone(),
                font_size: PROFILE_FONT_SIZE,
                visible_lines: 4.0,
                tab_index: 6,
                max_characters: Some(1023),
                ..TextInputSpec::new("profile-pick-desc", TextInputKind::Multiline)
            },
        ));
    } else {
        spawn_text_block(commands, detail_panel, info.description.clone());
    }
    let location_row = spawn_labeled_row(commands, detail_panel, "profile-pick-location");
    let moved = state.pick_use_current.contains(&info.pick_id);
    let location = if moved {
        String::new()
    } else {
        pick_location_label(&info)
    };
    if moved {
        spawn_key_label(
            commands,
            location_row,
            "profile-location-pending",
            DIM_LABEL_COLOR,
        );
    } else {
        spawn_value_label(commands, location_row, location, LABEL_COLOR);
    }
    let buttons = spawn_button_row(commands, detail_panel);
    spawn_action_button(
        commands,
        buttons,
        "profile-pick-teleport",
        ProfileAction::TeleportToPick,
        7,
    );
    spawn_disabled_button(commands, buttons, "profile-pick-show-on-map");
    if build.own {
        spawn_action_button(
            commands,
            buttons,
            "profile-pick-set-location",
            ProfileAction::SetPickLocation,
            8,
        );
        spawn_action_button(
            commands,
            buttons,
            "profile-pick-save",
            ProfileAction::SavePick,
            9,
        );
    }
}

/// Build the Classifieds tab: the listing list as a left tab strip with the
/// selected listing's detail (editable for one's own), the new-listing editor,
/// and New / Delete.
fn build_classifieds_tab(
    commands: &mut Commands,
    panel: Entity,
    build: &BuildContext,
    state: &mut ProfileState,
    ui: &mut ProfileUi,
    boost: &mut MessageWriter<BoostTexture>,
) {
    ui.classified_name_field = None;
    ui.classified_desc_field = None;
    ui.classified_price_field = None;
    let classifieds = state.classifieds.clone().unwrap_or_default();
    if build.own && state.new_classified.is_none() {
        let buttons = spawn_button_row(commands, panel);
        if classifieds.len() < MAX_CLASSIFIEDS {
            spawn_action_button(
                commands,
                buttons,
                "profile-classified-new",
                ProfileAction::NewClassified,
                2,
            );
        } else {
            spawn_disabled_button(commands, buttons, "profile-classified-new");
        }
        if classifieds.is_empty() {
            spawn_disabled_button(commands, buttons, "profile-classified-delete");
        } else {
            spawn_action_button(
                commands,
                buttons,
                "profile-classified-delete",
                ProfileAction::DeleteClassified,
                3,
            );
        }
    }

    // The new-listing editor replaces the list while it is open.
    if let Some(draft) = state.new_classified {
        build_classified_editor(commands, panel, ui, &draft, None);
        return;
    }

    if classifieds.is_empty() {
        spawn_key_label(commands, panel, "profile-classifieds-none", DIM_LABEL_COLOR);
        return;
    }
    let labels: Vec<String> = classifieds
        .iter()
        .map(|classified| classified.name.clone())
        .collect();
    let tabs = spawn_tab_container(
        commands,
        panel,
        &TabSpec {
            element: CLASSIFIEDS_STRIP_ELEMENT,
            placement: TabPlacement::InlineStart,
            labels: &labels,
            active: state.selected_classified,
            tab_index: 4,
            font_size: PROFILE_FONT_SIZE,
            strip_width: Some(LIST_STRIP_WIDTH),
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
        },
    );
    let Some(detail_panel) = tabs.panels.get(state.selected_classified).copied() else {
        return;
    };
    let Some(info) = state
        .selected_classified_entry()
        .and_then(|classified| state.classified_info.get(&classified.classified_id))
        .cloned()
    else {
        spawn_key_label(commands, detail_panel, "profile-loading", DIM_LABEL_COLOR);
        return;
    };
    spawn_snapshot(commands, detail_panel, info.snapshot_id, state, boost);
    if build.own {
        // The cycle / toggle edits live in a draft initialised from the stored
        // listing, so a repaint keeps them.
        let draft = *state
            .classified_drafts
            .entry(info.classified_id)
            .or_insert_with(|| ClassifiedDraft::from_info(&info));
        build_classified_editor(commands, detail_panel, ui, &draft, Some(&info));
        let moved = state.classified_use_current.contains(&info.classified_id);
        let location_row = spawn_labeled_row(commands, detail_panel, "profile-classified-location");
        if moved {
            spawn_key_label(
                commands,
                location_row,
                "profile-location-pending",
                DIM_LABEL_COLOR,
            );
        } else {
            spawn_value_label(
                commands,
                location_row,
                classified_location_label(&info),
                LABEL_COLOR,
            );
        }
        let buttons = spawn_button_row(commands, detail_panel);
        spawn_action_button(
            commands,
            buttons,
            "profile-classified-teleport",
            ProfileAction::TeleportToClassified,
            10,
        );
        spawn_disabled_button(commands, buttons, "profile-classified-map");
        spawn_action_button(
            commands,
            buttons,
            "profile-classified-set-location",
            ProfileAction::SetClassifiedLocation,
            11,
        );
        spawn_action_button(
            commands,
            buttons,
            "profile-classified-save",
            ProfileAction::SaveClassified,
            12,
        );
    } else {
        // Read-only detail, as the reference's view panel shows it.
        spawn_value_label(commands, detail_panel, info.name.clone(), LABEL_COLOR);
        spawn_text_block(commands, detail_panel, info.description.clone());
        let location_row = spawn_labeled_row(commands, detail_panel, "profile-classified-location");
        spawn_value_label(
            commands,
            location_row,
            classified_location_label(&info),
            LABEL_COLOR,
        );
        let category_row = spawn_labeled_row(commands, detail_panel, "profile-classified-category");
        spawn_category_label(commands, category_row, info.category);
        let type_row = spawn_labeled_row(commands, detail_panel, "profile-classified-content-type");
        spawn_key_label(
            commands,
            type_row,
            content_type_key(classified_mature(info.classified_flags)),
            LABEL_COLOR,
        );
        let date_row =
            spawn_labeled_row(commands, detail_panel, "profile-classified-creation-date");
        spawn_value_label(
            commands,
            date_row,
            format_unix_date(i64::from(info.creation_date)),
            LABEL_COLOR,
        );
        let price_row = spawn_labeled_row(commands, detail_panel, "profile-classified-price");
        spawn_value_label(
            commands,
            price_row,
            format!("L${}", info.price_for_listing.0),
            LABEL_COLOR,
        );
        let buttons = spawn_button_row(commands, detail_panel);
        spawn_action_button(
            commands,
            buttons,
            "profile-classified-teleport",
            ProfileAction::TeleportToClassified,
            10,
        );
        spawn_disabled_button(commands, buttons, "profile-classified-map");
    }
}

/// Build the classified edit controls: title / description fields, the
/// category and content-type cycles, the auto-renew toggle — and, for a new
/// listing (`info` is `None`), the price field and Publish / Cancel buttons.
fn build_classified_editor(
    commands: &mut Commands,
    panel: Entity,
    ui: &mut ProfileUi,
    draft: &ClassifiedDraft,
    info: Option<&ClassifiedInfo>,
) {
    let name_row = spawn_labeled_row(commands, panel, "profile-classified-name");
    ui.classified_name_field = Some(spawn_text_input(
        commands,
        name_row,
        &TextInputSpec {
            initial: info.map(|info| info.name.clone()).unwrap_or_default(),
            font_size: PROFILE_FONT_SIZE,
            width_glyphs: 18.0,
            tab_index: 5,
            max_characters: Some(30),
            ..TextInputSpec::new("profile-classified-name", TextInputKind::Line)
        },
    ));
    spawn_section_label(commands, panel, "profile-classified-desc");
    ui.classified_desc_field = Some(spawn_text_input(
        commands,
        panel,
        &TextInputSpec {
            initial: info
                .map(|info| info.description.clone())
                .unwrap_or_default(),
            font_size: PROFILE_FONT_SIZE,
            visible_lines: 4.0,
            tab_index: 6,
            max_characters: Some(255),
            ..TextInputSpec::new("profile-classified-desc", TextInputKind::Multiline)
        },
    ));
    let category_row = spawn_labeled_row(commands, panel, "profile-classified-category");
    let category_button =
        spawn_cycle_button(commands, category_row, ProfileAction::CycleCategory, 7);
    spawn_category_label_on(commands, category_button, draft.category);
    let type_row = spawn_labeled_row(commands, panel, "profile-classified-content-type");
    let type_button = spawn_cycle_button(commands, type_row, ProfileAction::CycleContentType, 8);
    commands.spawn((
        Text::default(),
        Translated::new(content_type_key(draft.mature)),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(type_button),
    ));
    spawn_check_button(
        commands,
        panel,
        "profile-classified-auto-renew",
        ProfileAction::ToggleAutoRenew,
        draft.auto_renew,
    );
    if info.is_none() {
        // A new listing: the price is set at publish time.
        let price_row = spawn_labeled_row(commands, panel, "profile-classified-price");
        ui.classified_price_field = Some(spawn_text_input(
            commands,
            price_row,
            &TextInputSpec {
                initial: "0".to_owned(),
                font_size: PROFILE_FONT_SIZE,
                width_glyphs: 8.0,
                tab_index: 9,
                ..TextInputSpec::new(
                    "profile-classified-price",
                    TextInputKind::NonNegativeInteger,
                )
            },
        ));
        let location_row = spawn_labeled_row(commands, panel, "profile-classified-location");
        spawn_key_label(
            commands,
            location_row,
            "profile-location-pending",
            DIM_LABEL_COLOR,
        );
        let buttons = spawn_button_row(commands, panel);
        spawn_action_button(
            commands,
            buttons,
            "profile-classified-publish",
            ProfileAction::SaveClassified,
            10,
        );
        spawn_action_button(
            commands,
            buttons,
            "profile-classified-cancel",
            ProfileAction::CancelNewClassified,
            11,
        );
    }
}

/// Build the 1st Life tab: the first-life picture and about text.
fn build_first_life_tab(
    commands: &mut Commands,
    panel: Entity,
    build: &BuildContext,
    state: &mut ProfileState,
    ui: &mut ProfileUi,
    boost: &mut MessageWriter<BoostTexture>,
) {
    ui.fl_about_field = None;
    let image_id = state.properties.as_ref().map(|props| props.fl_image_id);
    spawn_profile_image(commands, panel, image_id, state, boost);
    spawn_section_label(commands, panel, "profile-first-life-about");
    let about = state
        .properties
        .as_ref()
        .map(|props| props.fl_about_text.clone())
        .unwrap_or_default();
    if build.own {
        ui.fl_about_field = Some(spawn_text_input(
            commands,
            panel,
            &TextInputSpec {
                initial: about,
                font_size: PROFILE_FONT_SIZE,
                visible_lines: 5.0,
                tab_index: 2,
                max_characters: Some(253),
                ..TextInputSpec::new("profile-fl-about", TextInputKind::Multiline)
            },
        ));
        let buttons = spawn_button_row(commands, panel);
        spawn_action_button(
            commands,
            buttons,
            "profile-save",
            ProfileAction::SaveProfile,
            3,
        );
        spawn_action_button(
            commands,
            buttons,
            "profile-discard",
            ProfileAction::DiscardProfile,
            4,
        );
    } else {
        spawn_text_block(commands, panel, about);
    }
}

/// Build the Notes tab: our private notes about the avatar.
fn build_notes_tab(
    commands: &mut Commands,
    panel: Entity,
    state: &ProfileState,
    ui: &mut ProfileUi,
) {
    ui.notes_field = None;
    spawn_key_label(commands, panel, "profile-notes-hint", DIM_LABEL_COLOR);
    ui.notes_field = Some(spawn_text_input(
        commands,
        panel,
        &TextInputSpec {
            initial: state.notes.clone().unwrap_or_default(),
            font_size: PROFILE_FONT_SIZE,
            visible_lines: 6.0,
            tab_index: 2,
            max_characters: Some(1023),
            ..TextInputSpec::new("profile-notes", TextInputKind::Multiline)
        },
    ));
    let buttons = spawn_button_row(commands, panel);
    spawn_action_button(
        commands,
        buttons,
        "profile-save",
        ProfileAction::SaveNotes,
        3,
    );
}

// ---------------------------------------------------------------------------
// Small spawn helpers.
// ---------------------------------------------------------------------------

/// A labelled row: the translated label leading, the caller's content after.
fn spawn_labeled_row(commands: &mut Commands, parent: Entity, label_key: &'static str) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        ChildOf(row_entity),
    ));
    row_entity
}

/// A translated section label on its own line.
fn spawn_section_label(commands: &mut Commands, parent: Entity, label_key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        ChildOf(parent),
    ));
}

/// A plain value label.
fn spawn_value_label(commands: &mut Commands, parent: Entity, value: String, color: Color) {
    commands.spawn((
        Text::new(value),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(color),
        ChildOf(parent),
    ));
}

/// An empty value label, returning it so a value-update path can set its text in
/// place ([`set_value_node`]).
fn spawn_value_node(commands: &mut Commands, parent: Entity, color: Color) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(PROFILE_FONT_SIZE),
            TextColor(color),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// Set a retained value node's text in place (only on change).
fn set_value_node(texts: &mut Query<&mut Text>, node: Option<Entity>, value: &str) {
    if let Some(node) = node
        && let Ok(mut text) = texts.get_mut(node)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// A translated label.
fn spawn_key_label(commands: &mut Commands, parent: Entity, key: &'static str, color: Color) {
    spawn_key_label_node(commands, parent, key, color);
}

/// A translated label, returning the node so a caller can remove it later.
fn spawn_key_label_node(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    color: Color,
) -> Entity {
    commands
        .spawn((
            Text::default(),
            Translated::new(key),
            UiFont::Sans.at(PROFILE_FONT_SIZE),
            TextColor(color),
            ChildOf(parent),
        ))
        .id()
}

/// A wrapped read-only text block (about texts, descriptions).
fn spawn_text_block(commands: &mut Commands, parent: Entity, text: String) {
    commands
        .spawn((
            Node {
                max_height: Val::Px(140.0),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            ChildOf(parent),
        ))
        .with_child((
            Text::new(text),
            UiFont::Sans.at(PROFILE_FONT_SIZE),
            TextColor(LABEL_COLOR),
        ));
}

/// A wrapping row for action buttons.
fn spawn_button_row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id()
}

/// A bordered translated button dispatching `action` via [`on_profile_action`].
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: ProfileAction,
    tab_index: i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            action,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("profile-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_profile_action)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// A greyed placeholder button for a feature this viewer does not have yet —
/// present so the reference layout is complete, never interactive.
fn spawn_disabled_button(commands: &mut Commands, parent: Entity, label_key: &'static str) {
    commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Name::new(format!("profile-button-disabled:{label_key}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(PROFILE_FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
        ));
}

/// A borderless cycle button (category / content type), returning the entity
/// the caller labels.
fn spawn_cycle_button(
    commands: &mut Commands,
    parent: Entity,
    action: ProfileAction,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            action,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            ChildOf(parent),
        ))
        .observe(on_profile_action)
        .id()
}

/// A clickable check-glyph toggle dispatching `action`, returning its glyph text
/// node so the checked state can be updated in place ([`set_check_glyph`]).
fn spawn_check_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: ProfileAction,
    on: bool,
) -> Entity {
    let host = commands
        .spawn((
            Button,
            action,
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(4.0))
            },
            Pickable::default(),
            Name::new(format!("profile-toggle:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_profile_action)
        .id();
    let glyph = commands
        .spawn((
            Text::new(if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH }),
            UiFont::Sans.at(PROFILE_FONT_SIZE),
            TextColor(if on { CHECK_COLOR } else { DIM_LABEL_COLOR }),
            Pickable::IGNORE,
            ChildOf(host),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(host),
    ));
    glyph
}

/// Set a check-button glyph's checked state in place (no respawn).
fn set_check_glyph(texts: &mut Query<&mut Text>, glyph: Option<Entity>, on: bool) {
    if let Some(glyph) = glyph
        && let Ok(mut text) = texts.get_mut(glyph)
    {
        let wanted = if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH };
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
    }
}

/// A profile picture: request the texture and show a placeholder until it
/// decodes ([`poll_profile_textures`] swaps the image in).
fn spawn_profile_image(
    commands: &mut Commands,
    parent: Entity,
    image_id: Option<TextureKey>,
    state: &mut ProfileState,
    boost: &mut MessageWriter<BoostTexture>,
) {
    let node = spawn_image_box(commands, parent, Vec2::splat(PROFILE_IMAGE_EDGE));
    request_ui_texture(commands, image_id, node, state, boost);
}

/// A pick / classified snapshot node, with the texture requested like the
/// profile pictures.
fn spawn_snapshot(
    commands: &mut Commands,
    parent: Entity,
    snapshot_id: Option<TextureKey>,
    state: &mut ProfileState,
    boost: &mut MessageWriter<BoostTexture>,
) {
    let node = spawn_image_box(commands, parent, SNAPSHOT_SIZE);
    request_ui_texture(commands, snapshot_id, node, state, boost);
}

/// The empty image box a picture / snapshot fills once decoded.
fn spawn_image_box(commands: &mut Commands, parent: Entity, size: Vec2) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            ChildOf(parent),
        ))
        .id()
}

/// Request a (non-nil) texture and queue the node for the decoded image; an
/// unset image labels the box instead. Editing the images needs the texture
/// picker (`viewer-profile-image-editing`).
fn request_ui_texture(
    commands: &mut Commands,
    image_id: Option<TextureKey>,
    node: Entity,
    state: &mut ProfileState,
    boost: &mut MessageWriter<BoostTexture>,
) {
    let key = image_id.filter(|key| *key != TextureKey::from(Uuid::nil()));
    let Some(key) = key else {
        spawn_key_label(commands, node, "profile-image-none", DIM_LABEL_COLOR);
        return;
    };
    spawn_key_label(commands, node, "profile-loading", DIM_LABEL_COLOR);
    boost.write(BoostTexture {
        key,
        priority: AVATAR_BOOST_PRIORITY,
    });
    state.pending_textures.push((key, node));
}

/// A clickable group row in the 2nd-Life tab's group list, carrying the group it
/// opens the profile for.
#[derive(Component, Debug, Clone, Copy)]
struct ProfileGroupRow(GroupKey);

/// The last group-row press, for detecting a double-click (two presses on the same
/// group within [`GROUP_DOUBLE_CLICK_SECS`] open its profile). Tracked by group id.
#[derive(Resource, Debug, Default)]
struct ProfileGroupClick {
    /// The group the last press landed on, if any.
    group: Option<GroupKey>,
    /// When that press landed, in seconds since startup.
    time: f32,
}

/// Spawn one clickable group row — the group name as an accent label that opens
/// the group profile floater on click (`viewer-avatar-profile-group-list`). A bare
/// `Text` node, matching the profile's other value labels that lay out correctly
/// (a `Button` wrapper + an insignia thumbnail collapsed the row to the fixed
/// thumbnail box — the insignia is deferred to the follow-up).
fn spawn_profile_group_row(
    commands: &mut Commands,
    parent: Entity,
    group_id: GroupKey,
    group_name: &str,
) -> Entity {
    let name = if group_name.is_empty() {
        format!("({group_id})")
    } else {
        group_name.to_owned()
    };
    commands
        .spawn((
            Text::new(name),
            UiFont::Sans.at(PROFILE_FONT_SIZE),
            TextColor(GROUP_LINK_COLOR),
            ProfileGroupRow(group_id),
            Pickable::default(),
            Name::new("profile-group-row"),
            ChildOf(parent),
        ))
        .observe(on_profile_group_open)
        .id()
}

/// Open the group profile floater when a profile group row is **double-clicked**
/// (two primary presses on the same group within [`GROUP_DOUBLE_CLICK_SECS`]) —
/// matching the reference's group-list double-click.
fn on_profile_group_open(
    press: On<Pointer<Press>>,
    rows: Query<&ProfileGroupRow>,
    time: Res<Time>,
    mut tracker: ResMut<ProfileGroupClick>,
    mut groups: MessageWriter<OpenGroupProfile>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let group = row.0;
    let now = time.elapsed_secs();
    if tracker.group == Some(group) && now - tracker.time <= GROUP_DOUBLE_CLICK_SECS {
        groups.write(OpenGroupProfile { group });
        tracker.group = None;
    } else {
        tracker.group = Some(group);
        tracker.time = now;
    }
}

/// The Fluent key for a classified's content type.
const fn content_type_key(mature: bool) -> &'static str {
    if mature {
        "profile-classified-moderate"
    } else {
        "profile-classified-general"
    }
}

/// Label a row with a classified category (a translated key for the named
/// categories, the raw value for an unknown one).
fn spawn_category_label(commands: &mut Commands, parent: Entity, category: ClassifiedCategory) {
    match category_key(category) {
        Some(key) => spawn_key_label(commands, parent, key, LABEL_COLOR),
        None => spawn_value_label(commands, parent, category.to_string(), LABEL_COLOR),
    }
}

/// Label a cycle button with a classified category (children are
/// picking-transparent).
fn spawn_category_label_on(commands: &mut Commands, button: Entity, category: ClassifiedCategory) {
    let label = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(PROFILE_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    match category_key(category) {
        Some(key) => {
            commands.entity(label).insert(Translated::new(key));
        }
        None => {
            commands
                .entity(label)
                .insert(Text::new(category.to_string()));
        }
    }
}

// ---------------------------------------------------------------------------
// Actions.
// ---------------------------------------------------------------------------

/// Dispatch a clicked profile button to the behaviour behind it, **in the
/// window it was clicked in**.
///
/// Which window that is comes from the tree ([`host_floater`]) rather than from
/// a resource: with two profiles open, "Pay" means pay *this* window's
/// resident, and the amount is *this* window's field.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the action marker, \
              the window lookup, its per-window state / UI handles, the field values, the \
              identity / name sources, and the command and repaint outputs"
)]
fn on_profile_action(
    press: On<Pointer<Press>>,
    actions: Query<&ProfileAction>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut instances: Query<(&mut ProfileState, &mut ProfileDirty, &ProfileUi)>,
    fields: Query<&EditableText>,
    avatars: Res<AvatarState>,
    clipboard: Res<crate::clipboard::ViewerClipboard>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut blocks: MessageWriter<RequestBlock>,
    mut friendships: MessageWriter<RequestFriendship>,
    mut conversations: MessageWriter<OpenConversation>,
    mut contact_sets: MessageWriter<crate::world_api::OpenAddToContactSet>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, mut dirty, ui)) = instances.get_mut(window) else {
        return;
    };
    let target = state.target;
    let read = |entity: Option<Entity>| {
        entity
            .and_then(|field| fields.get(field).ok())
            .map(|field| field.value().to_string())
    };
    match action {
        ProfileAction::Im => {
            conversations.write(OpenConversation {
                key: ConversationKey::Direct(target),
            });
        }
        ProfileAction::OfferTeleport => {
            sl_commands.write(SlCommand(Command::OfferTeleport {
                targets: vec![target],
                message: String::new(),
            }));
        }
        ProfileAction::AddFriend => {
            // The prompted path (`crate::add_friend`) asks for the offer's
            // message and confirms the send; writing the command here would be
            // the silent offer the bug records.
            friendships.write(RequestFriendship::one(target));
        }
        ProfileAction::AddToContactSet => {
            contact_sets.write(crate::world_api::OpenAddToContactSet::one(
                target,
                avatars
                    .name_of(target)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default(),
            ));
        }
        ProfileAction::CopySlurl => {
            crate::clipboard::copy_to_clipboard(
                &clipboard,
                &format!("secondlife:///app/agent/{}/about", target.uuid()),
            );
        }
        ProfileAction::RemoveFriend => {
            sl_commands.write(SlCommand(Command::TerminateFriendship(FriendKey::from(
                target.uuid(),
            ))));
        }
        ProfileAction::Block => {
            let name = avatars
                .name_of(target)
                .map(ToOwned::to_owned)
                .unwrap_or_default();
            blocks.write(RequestBlock::new(target.uuid(), name, MuteType::Agent));
        }
        ProfileAction::Pay => {
            let Some(amount) = read(ui.pay_amount_field)
                .and_then(|amount| amount.trim().parse::<u64>().ok())
                .filter(|amount| *amount > 0)
            else {
                return;
            };
            sl_commands.write(SlCommand(Command::SendMoneyTransfer {
                dest: target.uuid(),
                amount: LindenAmount(amount),
                kind: MoneyTransactionType::Gift,
                description: String::new(),
            }));
        }
        ProfileAction::ToggleShowInSearch => {
            state.show_in_search = !state.show_in_search;
            dirty.mark(ProfileTab::SecondLife);
        }
        ProfileAction::SaveProfile => {
            let Some(props) = state.properties.clone() else {
                return;
            };
            let update = ProfileUpdate {
                image_id: props.image_id,
                fl_image_id: props.fl_image_id,
                about_text: read(ui.about_field).unwrap_or_else(|| props.about_text.clone()),
                fl_about_text: read(ui.fl_about_field)
                    .unwrap_or_else(|| props.fl_about_text.clone()),
                allow_publish: state.show_in_search,
                // The reference hardcodes this off: "A profile should never
                // be mature".
                mature_publish: false,
                profile_url: read(ui.url_field).unwrap_or_else(|| props.profile_url.clone()),
            };
            // Reflect the save locally so a repaint shows the new text; the
            // update message carries no ack.
            if let Some(props) = state.properties.as_mut() {
                props.about_text.clone_from(&update.about_text);
                props.fl_about_text.clone_from(&update.fl_about_text);
                props.profile_url.clone_from(&update.profile_url);
            }
            sl_commands.write(SlCommand(Command::UpdateProfile(update)));
        }
        ProfileAction::DiscardProfile => {
            state.show_in_search = state
                .properties
                .as_ref()
                .is_some_and(|props| props.flags & FLAG_ALLOW_PUBLISH != 0);
            dirty.mark(ProfileTab::SecondLife);
            dirty.mark(ProfileTab::Web);
            dirty.mark(ProfileTab::FirstLife);
        }
        ProfileAction::SaveNotes => {
            let Some(notes) = read(ui.notes_field) else {
                return;
            };
            state.notes = Some(notes.clone());
            sl_commands.write(SlCommand(Command::UpdateAvatarNotes {
                target_id: target,
                notes,
            }));
        }
        ProfileAction::NewPick => {
            // Created at the agent's current parcel / position (the simulator
            // fills both in), then refreshed from the volunteered replies.
            sl_commands.write(SlCommand(Command::UpdatePick(PickUpdate {
                pick_id: PickKey::from(Uuid::new_v4()),
                name: "New Pick".to_owned(),
                ..PickUpdate::default()
            })));
            sl_commands.write(SlCommand(Command::RequestAvatarPicks(target)));
        }
        ProfileAction::DeletePick => {
            let Some(pick_id) = state.selected_pick_entry().map(|pick| pick.pick_id) else {
                return;
            };
            sl_commands.write(SlCommand(Command::DeletePick(pick_id)));
            if let Some(picks) = state.picks.as_mut() {
                picks.retain(|pick| pick.pick_id != pick_id);
            }
            state.selected_pick = 0;
            dirty.mark(ProfileTab::Picks);
            sl_commands.write(SlCommand(Command::RequestAvatarPicks(target)));
        }
        ProfileAction::SavePick => {
            let Some(info) = state
                .selected_pick_entry()
                .and_then(|pick| state.pick_info.get(&pick.pick_id))
                .cloned()
            else {
                return;
            };
            let use_current = state.pick_use_current.remove(&info.pick_id);
            let update = PickUpdate {
                pick_id: info.pick_id,
                parcel_id: if use_current {
                    None
                } else {
                    Some(info.parcel_id)
                },
                name: read(ui.pick_name_field).unwrap_or_else(|| info.name.clone()),
                description: read(ui.pick_desc_field).unwrap_or_else(|| info.description.clone()),
                snapshot_id: info.snapshot_id,
                pos_global: if use_current {
                    GlobalCoordinates::new(0.0, 0.0, 0.0)
                } else {
                    info.pos_global
                },
                sort_order: info.sort_order,
                enabled: info.enabled,
            };
            sl_commands.write(SlCommand(Command::UpdatePick(update)));
        }
        ProfileAction::SetPickLocation => {
            let Some(pick_id) = state.selected_pick_entry().map(|pick| pick.pick_id) else {
                return;
            };
            state.pick_use_current.insert(pick_id);
            dirty.mark(ProfileTab::Picks);
        }
        ProfileAction::TeleportToPick => {
            let Some(info) = state
                .selected_pick_entry()
                .and_then(|pick| state.pick_info.get(&pick.pick_id))
            else {
                return;
            };
            teleport_to(&info.pos_global, &mut sl_commands);
        }
        ProfileAction::NewClassified => {
            state.new_classified = Some(ClassifiedDraft::default());
            dirty.mark(ProfileTab::Classifieds);
        }
        ProfileAction::CancelNewClassified => {
            state.new_classified = None;
            dirty.mark(ProfileTab::Classifieds);
        }
        ProfileAction::DeleteClassified => {
            let Some(id) = state
                .selected_classified_entry()
                .map(|classified| classified.classified_id)
            else {
                return;
            };
            sl_commands.write(SlCommand(Command::DeleteClassified(id)));
            if let Some(classifieds) = state.classifieds.as_mut() {
                classifieds.retain(|classified| classified.classified_id != id);
            }
            state.selected_classified = 0;
            dirty.mark(ProfileTab::Classifieds);
            sl_commands.write(SlCommand(Command::RequestAvatarClassifieds(target)));
        }
        ProfileAction::SaveClassified => {
            if let Some(draft) = state.new_classified {
                // Publish the new listing at the agent's current location.
                let price = read(ui.classified_price_field)
                    .and_then(|price| price.trim().parse::<u64>().ok())
                    .unwrap_or(0);
                let id = ClassifiedKey::from(Uuid::new_v4());
                sl_commands.write(SlCommand(Command::UpdateClassified(ClassifiedUpdate {
                    classified_id: id,
                    category: draft.category,
                    name: read(ui.classified_name_field).unwrap_or_default(),
                    description: read(ui.classified_desc_field).unwrap_or_default(),
                    classified_flags: pack_classified_flags(draft.mature, draft.auto_renew),
                    price_for_listing: LindenAmount(price),
                    ..ClassifiedUpdate::default()
                })));
                state.new_classified = None;
                dirty.mark(ProfileTab::Classifieds);
                sl_commands.write(SlCommand(Command::RequestAvatarClassifieds(target)));
                sl_commands.write(SlCommand(Command::RequestClassifiedInfo(id)));
                return;
            }
            let Some(info) = state
                .selected_classified_entry()
                .and_then(|classified| state.classified_info.get(&classified.classified_id))
                .cloned()
            else {
                return;
            };
            let draft = state
                .classified_drafts
                .get(&info.classified_id)
                .copied()
                .unwrap_or_else(|| ClassifiedDraft::from_info(&info));
            let use_current = state.classified_use_current.remove(&info.classified_id);
            sl_commands.write(SlCommand(Command::UpdateClassified(ClassifiedUpdate {
                classified_id: info.classified_id,
                category: draft.category,
                name: read(ui.classified_name_field).unwrap_or_else(|| info.name.clone()),
                description: read(ui.classified_desc_field)
                    .unwrap_or_else(|| info.description.clone()),
                parcel_id: if use_current {
                    None
                } else {
                    Some(info.parcel_id)
                },
                snapshot_id: info.snapshot_id,
                pos_global: if use_current {
                    GlobalCoordinates::new(0.0, 0.0, 0.0)
                } else {
                    info.pos_global
                },
                classified_flags: pack_classified_flags(draft.mature, draft.auto_renew),
                price_for_listing: info.price_for_listing,
            })));
            sl_commands.write(SlCommand(Command::RequestClassifiedInfo(
                info.classified_id,
            )));
        }
        ProfileAction::SetClassifiedLocation => {
            let Some(id) = state
                .selected_classified_entry()
                .map(|classified| classified.classified_id)
            else {
                return;
            };
            state.classified_use_current.insert(id);
            dirty.mark(ProfileTab::Classifieds);
        }
        ProfileAction::TeleportToClassified => {
            let Some(info) = state
                .selected_classified_entry()
                .and_then(|classified| state.classified_info.get(&classified.classified_id))
            else {
                return;
            };
            teleport_to(&info.pos_global, &mut sl_commands);
        }
        ProfileAction::CycleCategory => {
            if let Some(draft) = edited_classified_draft(&mut state) {
                draft.category = next_category(draft.category);
                dirty.mark(ProfileTab::Classifieds);
            }
        }
        ProfileAction::CycleContentType => {
            if let Some(draft) = edited_classified_draft(&mut state) {
                draft.mature = !draft.mature;
                dirty.mark(ProfileTab::Classifieds);
            }
        }
        ProfileAction::ToggleAutoRenew => {
            if let Some(draft) = edited_classified_draft(&mut state) {
                draft.auto_renew = !draft.auto_renew;
                dirty.mark(ProfileTab::Classifieds);
            }
        }
    }
}

/// The classified draft the cycle / toggle buttons currently edit: the
/// new-listing draft while the editor is open, else the selected listing's.
fn edited_classified_draft(state: &mut ProfileState) -> Option<&mut ClassifiedDraft> {
    if state.new_classified.is_some() {
        return state.new_classified.as_mut();
    }
    let id = state
        .selected_classified_entry()
        .map(|classified| classified.classified_id)?;
    let info = state.classified_info.get(&id)?;
    let draft = ClassifiedDraft::from_info(info);
    Some(state.classified_drafts.entry(id).or_insert(draft))
}

/// Teleport to a grid-global position (the pick / classified Teleport
/// buttons).
fn teleport_to(pos_global: &GlobalCoordinates, sl_commands: &mut MessageWriter<SlCommand>) {
    let Some((region_handle, position)) = teleport_destination(pos_global) else {
        return;
    };
    sl_commands.write(SlCommand(Command::Teleport {
        region_handle,
        position,
        look_at: Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
    }));
}

// ---------------------------------------------------------------------------
// Texture polling.
// ---------------------------------------------------------------------------

/// Swap pending profile / snapshot placeholders for their decoded images once
/// the texture pipeline holds them. A rebuild despawns the old boxes, so a
/// pending node may be gone by the time its texture decodes — those entries
/// are dropped, not applied.
fn poll_profile_textures(
    mut instances: Query<&mut ProfileState>,
    store: Res<DecodedTextures>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    for mut state in &mut instances {
        if state.pending_textures.is_empty() {
            continue;
        }
        let pending = std::mem::take(&mut state.pending_textures);
        for (key, node) in pending {
            let Ok(mut entity) = commands.get_entity(node) else {
                continue;
            };
            if let Some(decoded) = store.get(key) {
                let handle = images.add(to_bevy_image(decoded));
                entity.insert(ImageNode::new(handle));
                // Drop the "(loading)" label under the image.
                despawn_children(&children, &mut commands, node);
            } else {
                state.pending_textures.push((key, node));
            }
        }
    }
}

/// Keep every open Web tab's load-status line current: "loading" while the
/// embedded page loads, then the reference's load-time string ("Page loaded in
/// N s") once it finishes.
///
/// The clock is kept **per browser view** rather than per system: a tab rebuild
/// spawns a new view and so restarts its own clock, and two open profiles are
/// two views timed independently. Views that have gone (a rebuild, a closed
/// window) are dropped each pass, so the map is as small as the open tabs.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the open \
              windows' handles, the browser view / surface lookups, the clock, the \
              translator, the per-view timers and the status label"
)]
fn update_profile_web_status(
    instances: Query<&ProfileUi>,
    views: Query<&crate::browser_widget::BrowserView>,
    surfaces: bevy::ecs::system::NonSend<crate::media_engine::MediaSurfaces>,
    time: Res<Time>,
    translator: crate::i18n::Translator,
    mut tracked: Local<HashMap<Entity, (f64, bool)>>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    let mut live: HashSet<Entity> = HashSet::new();
    for ui in &instances {
        let (Some(view_entity), Some(status_entity)) = (ui.web_view, ui.web_status) else {
            continue;
        };
        live.insert(view_entity);
        let (started, done) = tracked.entry(view_entity).or_insert((now, false));
        if *done {
            continue;
        }
        let Ok(view) = views.get(view_entity) else {
            continue;
        };
        let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
            continue;
        };
        if slot.status.loading || slot.status.progress < 1.0 {
            continue;
        }
        let seconds = format!("{:.2}", now - *started);
        let line = translator.format(
            "profile-web-loaded",
            &crate::i18n::TransArgs::new().text("seconds", &seconds),
        );
        if let Ok(mut text) = texts.get_mut(status_entity) {
            text.0 = line;
        }
        commands.entity(status_entity).remove::<Translated>();
        *done = true;
    }
    tracked.retain(|view, _timer| live.contains(view));
}

// ---------------------------------------------------------------------------
// Pure helpers.
// ---------------------------------------------------------------------------

/// The account caption: either a Fluent key (the wire's one-byte caption
/// index, `0..=3`) or the grid's literal caption text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AccountCaption {
    /// A translated caption (Resident / Trial / Charter Member / Employee).
    Key(&'static str),
    /// A grid-supplied literal caption.
    Literal(String),
}

/// Decode the `CharterMember` field: a single byte `0..=3` is a caption index
/// (the reference's `accountType()`), anything longer is literal text, and
/// empty means the default (Resident).
fn account_caption(charter_member: &str) -> AccountCaption {
    match charter_member.as_bytes() {
        [] | [0] => AccountCaption::Key("profile-account-resident"),
        [1] => AccountCaption::Key("profile-account-trial"),
        [2] => AccountCaption::Key("profile-account-charter"),
        [3] => AccountCaption::Key("profile-account-employee"),
        _text => AccountCaption::Literal(charter_member.to_owned()),
    }
}

/// The payment-info caption key for a profile's flags (the reference's
/// `PaymentInfo` captions).
const fn payment_caption_key(flags: u32) -> &'static str {
    if flags & FLAG_TRANSACTED != 0 {
        "profile-payment-used"
    } else if flags & FLAG_IDENTIFIED != 0 {
        "profile-payment-on-file"
    } else {
        "profile-payment-none"
    }
}

/// The online-status caption key for a profile's flags.
const fn online_caption_key(flags: u32) -> &'static str {
    if flags & FLAG_ONLINE != 0 {
        "profile-online"
    } else {
        "profile-offline"
    }
}

/// Whether a classified's flags mark it Moderate ("mature") content.
const fn classified_mature(flags: u8) -> bool {
    flags & CLASSIFIED_FLAG_MATURE != 0
}

/// Whether a classified's flags mark it auto-renewing.
const fn classified_auto_renew(flags: u8) -> bool {
    flags & CLASSIFIED_FLAG_AUTO_RENEW != 0
}

/// Pack the editable classified flags (the reference's
/// `pack_classified_flags`: the mature and auto-renew bits).
const fn pack_classified_flags(mature: bool, auto_renew: bool) -> u8 {
    let mut flags = 0;
    if mature {
        flags |= CLASSIFIED_FLAG_MATURE;
    }
    if auto_renew {
        flags |= CLASSIFIED_FLAG_AUTO_RENEW;
    }
    flags
}

/// The Fluent key naming a classified category, or `None` for an unrecognised
/// wire value (shown as its raw number instead).
const fn category_key(category: ClassifiedCategory) -> Option<&'static str> {
    match category {
        ClassifiedCategory::AnyCategory => Some("profile-category-any"),
        ClassifiedCategory::Shopping => Some("profile-category-shopping"),
        ClassifiedCategory::LandRental => Some("profile-category-land-rental"),
        ClassifiedCategory::PropertyRental => Some("profile-category-property-rental"),
        ClassifiedCategory::SpecialAttraction => Some("profile-category-special-attraction"),
        ClassifiedCategory::NewProducts => Some("profile-category-new-products"),
        ClassifiedCategory::Employment => Some("profile-category-employment"),
        ClassifiedCategory::Wanted => Some("profile-category-wanted"),
        ClassifiedCategory::Service => Some("profile-category-service"),
        ClassifiedCategory::Personal => Some("profile-category-personal"),
        // `Unknown` and any future variant (the enum is non-exhaustive): no
        // named form, shown as the raw value.
        _other => None,
    }
}

/// The next category in the edit cycle (the reference's combo's nine real
/// categories, in wire order; Any / unknown restart at Shopping).
const fn next_category(category: ClassifiedCategory) -> ClassifiedCategory {
    match category {
        ClassifiedCategory::Shopping => ClassifiedCategory::LandRental,
        ClassifiedCategory::LandRental => ClassifiedCategory::PropertyRental,
        ClassifiedCategory::PropertyRental => ClassifiedCategory::SpecialAttraction,
        ClassifiedCategory::SpecialAttraction => ClassifiedCategory::NewProducts,
        ClassifiedCategory::NewProducts => ClassifiedCategory::Employment,
        ClassifiedCategory::Employment => ClassifiedCategory::Wanted,
        ClassifiedCategory::Wanted => ClassifiedCategory::Service,
        ClassifiedCategory::Service => ClassifiedCategory::Personal,
        // Personal wraps around; Any / Unknown / any future variant restart
        // the cycle at the first real category.
        _other => ClassifiedCategory::Shopping,
    }
}

/// The teleport destination for a grid-global position: the containing
/// region's handle and the region-local coordinates. `None` when the position
/// is outside the representable grid.
fn teleport_destination(
    pos_global: &GlobalCoordinates,
) -> Option<(RegionHandle, RegionCoordinates)> {
    let (grid, local) = pos_global.split()?;
    Some((RegionHandle::from_grid(grid.x(), grid.y()), local))
}

/// A pick's location line: `parcel, region (x, y, z)` with region-local
/// coordinates, matching the reference's `pick_location` composition.
fn pick_location_label(info: &PickInfo) -> String {
    location_label(
        &info.original_name,
        info.sim_name.as_ref(),
        &info.pos_global,
    )
}

/// A classified's location line, from its parcel name and position.
fn classified_location_label(info: &ClassifiedInfo) -> String {
    location_label(&info.parcel_name, info.sim_name.as_ref(), &info.pos_global)
}

/// Compose a `parcel, region (x, y, z)` location line; parts the grid did not
/// send are omitted.
fn location_label(
    parcel: &str,
    sim_name: Option<&sl_client_bevy::RegionName>,
    pos_global: &GlobalCoordinates,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !parcel.is_empty() {
        parts.push(parcel.to_owned());
    }
    if let Some(sim) = sim_name {
        parts.push(sim.to_string());
    }
    let position = pos_global
        .split()
        .map(|(_grid, local)| format!("({:.0}, {:.0}, {:.0})", local.x(), local.y(), local.z()));
    let mut label = parts.join(", ");
    if let Some(position) = position {
        if label.is_empty() {
            label = position;
        } else {
            label = format!("{label} {position}");
        }
    }
    label
}

#[cfg(test)]
mod tests {
    use sl_client_bevy::{ClassifiedCategory, GlobalCoordinates};

    use super::{
        AccountCaption, FLAG_IDENTIFIED, FLAG_ONLINE, FLAG_TRANSACTED, account_caption,
        classified_auto_renew, classified_mature, next_category, online_caption_key,
        pack_classified_flags, payment_caption_key, teleport_destination,
    };
    use pretty_assertions::assert_eq;

    /// The charter-member field decodes as the reference's `accountType()`
    /// does: one-byte indices are captions, longer values literal text.
    #[test]
    fn charter_member_decodes_to_caption() {
        assert_eq!(
            account_caption(""),
            AccountCaption::Key("profile-account-resident")
        );
        assert_eq!(
            account_caption("\u{1}"),
            AccountCaption::Key("profile-account-trial")
        );
        assert_eq!(
            account_caption("\u{2}"),
            AccountCaption::Key("profile-account-charter")
        );
        assert_eq!(
            account_caption("\u{3}"),
            AccountCaption::Key("profile-account-employee")
        );
        assert_eq!(
            account_caption("Grid Owner"),
            AccountCaption::Literal("Grid Owner".to_owned())
        );
    }

    /// The profile flags map to the reference's payment / online captions.
    #[test]
    fn profile_flags_map_to_captions() {
        assert_eq!(payment_caption_key(0), "profile-payment-none");
        assert_eq!(
            payment_caption_key(FLAG_IDENTIFIED),
            "profile-payment-on-file"
        );
        // Transacted wins over identified, as in the reference.
        assert_eq!(
            payment_caption_key(FLAG_IDENTIFIED | FLAG_TRANSACTED),
            "profile-payment-used"
        );
        assert_eq!(online_caption_key(FLAG_ONLINE), "profile-online");
        assert_eq!(online_caption_key(0), "profile-offline");
    }

    /// Classified flags pack / unpack the reference's mature (bit 1) and
    /// auto-renew (bit 5) bits.
    #[test]
    fn classified_flags_round_trip() {
        for mature in [false, true] {
            for auto_renew in [false, true] {
                let flags = pack_classified_flags(mature, auto_renew);
                assert_eq!(classified_mature(flags), mature);
                assert_eq!(classified_auto_renew(flags), auto_renew);
            }
        }
        assert_eq!(pack_classified_flags(true, false), 1 << 1);
        assert_eq!(pack_classified_flags(false, true), 1 << 5);
    }

    /// The category cycle walks all nine real categories and restarts at
    /// Shopping from the non-editable values.
    #[test]
    fn category_cycle_covers_all_real_categories() {
        let mut seen = vec![ClassifiedCategory::Shopping];
        let mut current = ClassifiedCategory::Shopping;
        for _step in 0..8 {
            current = next_category(current);
            assert!(
                !seen.contains(&current),
                "the cycle must not repeat before covering all categories"
            );
            seen.push(current);
        }
        assert_eq!(next_category(current), ClassifiedCategory::Shopping);
        assert_eq!(
            next_category(ClassifiedCategory::AnyCategory),
            ClassifiedCategory::Shopping
        );
        assert_eq!(
            next_category(ClassifiedCategory::Unknown(42)),
            ClassifiedCategory::Shopping
        );
    }

    /// A grid-global position splits into the region handle + local position
    /// the `Teleport` command wants.
    #[test]
    fn teleport_destination_splits_global_position() {
        // Region (1000, 1002), local (128.5, 32.25, 22).
        let global = GlobalCoordinates::new(256_128.5, 256_544.25, 22.0);
        let destination = teleport_destination(&global);
        let Some((handle, local)) = destination else {
            assert!(
                destination.is_some(),
                "an in-range global position must split"
            );
            return;
        };
        assert_eq!(handle.grid_coordinates(), (1000, 1002));
        // Approximate: the split goes through f64 metres.
        assert!((local.x() - 128.5).abs() < 0.001);
        assert!((local.y() - 32.25).abs() < 0.001);
        assert!((local.z() - 22.0).abs() < 0.001);
    }

    /// **One window per resident** (`viewer-profile-floater-single-instance`):
    /// the open path itself, driven by the very messages a radar row and a chat
    /// name link write.
    ///
    /// The pure helpers above cannot see this one: the bug was not in any of
    /// them, it was in there being a single `ProfileState` for every subject.
    mod instances {
        use super::super::{
            PROFILE_FLOATER_ID, ProfileState, ProfileUi, open_profile, profile_key,
        };
        use crate::floater::{
            ActiveFloater, Floater, FloaterCommand, FloaterOp, FloaterPlugin, FloaterZTop,
        };
        use crate::ui::UiRoot;
        use crate::world_api::{AvatarState, OpenAvatarProfile};
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{AgentKey, SlCommand, Uuid};

        /// A boxed error so tests can use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// Two distinct residents to open profiles on.
        fn residents() -> (AgentKey, AgentKey) {
            (
                AgentKey::from(Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888)),
                AgentKey::from(Uuid::from_u128(0x8888_7777_6666_5555_4444_3333_2222_1111)),
            )
        }

        /// An app with the floater manager and the **open path** — the module's
        /// own `open_profile`, not the whole plugin.
        ///
        /// The window-facing systems beside it (the repaint, the event fold,
        /// the Web tab's clock) each need a slice of a live session — an
        /// identity, a friend roster, the texture pipeline, the media engine's
        /// non-send surfaces — and standing all of that up would test the
        /// harness rather than the fix. The bug was in the open: one state for
        /// every subject. This is that path, with the real manager under it, so
        /// spawning, keying, raising and closing are the shipped ones.
        fn profile_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<OpenAvatarProfile>()
                .init_resource::<AvatarState>()
                // `bevy_ui`'s scale and the keyboard map, which the manager's
                // on-screen clamp and its `Ctrl+W` read; no `UiPlugin` here.
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_plugins(FloaterPlugin)
                .add_systems(Update, open_profile);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open `agent`'s profile the way every caller does — by writing the
        /// message — and settle a frame.
        fn open(app: &mut App, agent: AgentKey) {
            app.world_mut().write_message(OpenAvatarProfile { agent });
            app.update();
        }

        /// Every live profile window, as (entity, subject) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, AgentKey)> {
            app.world_mut()
                .query::<(Entity, &ProfileState)>()
                .iter(app.world())
                .map(|(entity, state)| (entity, state.target))
                .collect()
        }

        /// Two residents are two windows, each with its own subject, its own
        /// tab handles and its own requests — and re-opening one of them raises
        /// that window instead of adding a third.
        #[test]
        fn two_residents_open_two_windows() -> Result<(), TestError> {
            let (first, second) = residents();
            let mut app = profile_app();
            open(&mut app, first);
            open(&mut app, second);

            let mut open_windows = windows(&mut app);
            open_windows.sort_by_key(|(entity, _agent)| *entity);
            assert_eq!(
                open_windows.len(),
                2,
                "the second resident reused the first window"
            );
            let subjects: Vec<AgentKey> =
                open_windows.iter().map(|(_entity, agent)| *agent).collect();
            assert!(subjects.contains(&first) && subjects.contains(&second));

            // Each window carries its own handles — the fields a Save reads.
            let world = app.world();
            for (window, _agent) in &open_windows {
                assert!(
                    world.get::<ProfileUi>(*window).is_some(),
                    "a profile window has no UI handles of its own"
                );
                let floater = world.get::<Floater>(*window).ok_or("not a floater")?;
                assert_eq!(floater.id, PROFILE_FLOATER_ID);
            }
            // Keyed by subject, so each window is findable by its resident.
            let by_key: Vec<Option<&crate::floater::FloaterKey>> = open_windows
                .iter()
                .map(|(window, _agent)| world.get::<Floater>(*window).and_then(Floater::key))
                .collect();
            assert!(by_key.contains(&Some(&profile_key(first))));
            assert!(by_key.contains(&Some(&profile_key(second))));

            // Re-opening the first resident raises that window rather than
            // spawning another copy of the same person.
            open(&mut app, first);
            assert_eq!(windows(&mut app).len(), 2);
            let raised = windows(&mut app)
                .into_iter()
                .find_map(|(window, agent)| (agent == first).then_some(window))
                .ok_or("the first resident's window is gone")?;
            assert_eq!(
                app.world().resource::<ActiveFloater>().front(),
                Some(raised)
            );
            Ok(())
        }

        /// Closing one profile ends **that** window — the state goes with it —
        /// and leaves the other resident's window open.
        #[test]
        fn closing_one_profile_leaves_the_other() -> Result<(), TestError> {
            let (first, second) = residents();
            let mut app = profile_app();
            open(&mut app, first);
            open(&mut app, second);
            let target = windows(&mut app)
                .into_iter()
                .find_map(|(window, agent)| (agent == first).then_some(window))
                .ok_or("the first resident has no window")?;

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: target,
                    op: FloaterOp::Close,
                });
            app.update();

            let left = windows(&mut app);
            assert_eq!(left.len(), 1, "closing one profile left the wrong count");
            assert_eq!(left.first().map(|(_window, agent)| *agent), Some(second));

            // Re-opening the closed resident builds a fresh window rather than
            // reviving a hidden one.
            open(&mut app, first);
            assert_eq!(windows(&mut app).len(), 2);
            Ok(())
        }

        /// The windows above are the manager's real ones — the fixture adds
        /// [`FloaterPlugin`], so a keyed open, a raise and a close are the
        /// shipped code paths rather than a stand-in.
        #[test]
        fn the_manager_is_wired() {
            let app = profile_app();
            assert!(app.world().contains_resource::<FloaterZTop>());
        }
    }
}
