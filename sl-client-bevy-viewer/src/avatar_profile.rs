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

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AgentKey, AvatarClassified, AvatarGroupMembership, AvatarPick, AvatarProperties,
    ClassifiedCategory, ClassifiedInfo, ClassifiedKey, ClassifiedUpdate, Command, FriendKey,
    GlobalCoordinates, LindenAmount, MoneyTransactionType, MuteFlags, MuteType, PickInfo, PickKey,
    PickUpdate, ProfileUpdate, RegionCoordinates, RegionHandle, SlCommand, SlEvent, SlIdentity,
    SlSessionEvent, TextureKey, Uuid, Vector, to_bevy_image,
};

use crate::avatars::AvatarState;
use crate::conversations::{ConversationKey, OpenConversation};
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::inventory_drag::AgentDropTarget;
use crate::inventory_properties::format_unix_date;
use crate::people::FriendsModel;
use crate::render_priority::AVATAR_BOOST_PRIORITY;
use crate::textures::TextureManager;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabContainerHandle, TabPlacement, TabSpec, TabStrip, fill_tab_container,
    spawn_tab_container,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The chrome font size, in logical pixels.
const PROFILE_FONT_SIZE: f32 = 14.0;

/// The primary label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer secondary label.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A toggle's check glyph colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

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
const FLAG_IDENTIFIED: u32 = 1 << 2;
/// The profile-flags bit for "payment info used" (`AVATAR_TRANSACTED`).
const FLAG_TRANSACTED: u32 = 1 << 3;
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

/// Open the profile floater on an avatar (from the pie menu's Profile slice,
/// the People list, or a repaint after an edit).
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenAvatarProfile {
    /// The avatar whose profile to show.
    pub(crate) agent: AgentKey,
}

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

/// The profile floater's live state: the shown avatar and everything received
/// about them so far.
#[derive(Resource, Debug, Default)]
pub(crate) struct ProfileState {
    /// The avatar shown, or `None` before the first open.
    target: Option<AgentKey>,
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
    /// Reset everything to a fresh open on `target`.
    fn reset(&mut self, target: AgentKey) {
        *self = Self {
            target: Some(target),
            ..Self::default()
        };
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

/// Which tabs need their content rebuilt from [`ProfileState`].
#[derive(Resource, Debug, Default)]
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

/// Entity handles for the profile floater: the shell spawned once at startup,
/// and the per-rebuild field entities the Save handlers read.
#[derive(Resource)]
pub(crate) struct ProfileUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
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
pub(crate) struct AvatarProfilePlugin;

impl Plugin for AvatarProfilePlugin {
    /// Register the state, the open message, and the spawn / open / ingest /
    /// rebuild / poll systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfileState>()
            .init_resource::<ProfileDirty>()
            .add_message::<OpenAvatarProfile>()
            .add_systems(
                Startup,
                spawn_profile_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_profile,
                    ingest_profile_events,
                    track_list_selection,
                    rebuild_profile_tabs,
                    poll_profile_textures,
                    update_profile_web_status,
                )
                    .chain(),
            );
    }
}

/// Spawn the (hidden) profile floater shell: the floater and the six-tab
/// container; tab contents are rebuilt per open.
fn spawn_profile_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "avatar-profile",
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
        },
    );
    // Subject-bound: the target avatar is not persisted, so neither is the
    // floater — no restored rectangle, no restored "open" (an empty shell).
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands
        .entity(handle.title_text)
        .insert(Translated::new("profile-title"));
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
        &mut commands,
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
    fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);
    commands.insert_resource(ProfileUi {
        panel: handle.root,
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
    });
}

// ---------------------------------------------------------------------------
// Open / ingest / selection.
// ---------------------------------------------------------------------------

/// Open the floater on an avatar: reset the state, fire the profile requests,
/// and mark every tab for rebuild.
fn open_profile(
    mut opens: MessageReader<OpenAvatarProfile>,
    mut state: ResMut<ProfileState>,
    mut dirty: ResMut<ProfileDirty>,
    avatars: Res<AvatarState>,
    ui: Option<Res<ProfileUi>>,
    mut panels: Query<&mut UiPanelShown>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(open) = opens.read().last().copied() else {
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    let agent = open.agent;
    // Re-opening the same avatar keeps the received state (a repaint after an
    // edit); a different avatar starts fresh.
    if state.target != Some(agent) {
        state.reset(agent);
        sl_commands.write(SlCommand(Command::RequestAvatarProperties(agent)));
        sl_commands.write(SlCommand(Command::RequestAvatarPicks(agent)));
        sl_commands.write(SlCommand(Command::RequestAvatarClassifieds(agent)));
        sl_commands.write(SlCommand(Command::RequestAvatarNotes(agent)));
        if avatars.name_of(agent).is_none() {
            sl_commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
        }
    }
    dirty.mark_all();
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
}

/// Fold profile-related session events for the shown avatar into the state,
/// marking the affected tabs dirty.
fn ingest_profile_events(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ProfileState>,
    mut dirty: ResMut<ProfileDirty>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(target) = state.target else {
        return;
    };
    for event in events.read() {
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
    strips: Query<&TabStrip, Changed<TabStrip>>,
    mut state: ResMut<ProfileState>,
    mut dirty: ResMut<ProfileDirty>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(target) = state.target else {
        return;
    };
    for strip in &strips {
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

/// Rebuild every dirty tab's content from the state.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the dirty flags, the \
              state, the UI handles, the identity / name / friendship sources, the texture \
              pipeline, and the spawn outputs"
)]
fn rebuild_profile_tabs(
    mut dirty: ResMut<ProfileDirty>,
    mut state: ResMut<ProfileState>,
    mut ui: ResMut<ProfileUi>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    friends: Res<FriendsModel>,
    mut textures: ResMut<TextureManager>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    if !dirty.any() {
        return;
    }
    let Some(target) = state.target else {
        *dirty = ProfileDirty::default();
        return;
    };
    let own = identity.agent_id == Some(target);
    // Title: the avatar's name once known (a plain string, not a Fluent key).
    if let Some(name) = avatars.name_of(target)
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
        commands.entity(ui.panel).remove::<AgentDropTarget>();
    } else {
        commands.entity(ui.panel).insert(AgentDropTarget(target));
    }
    let build = BuildContext {
        target,
        own,
        avatars: &avatars,
        friends: &friends,
    };
    for tab in ProfileTab::ALL {
        if !dirty_tabs.contains(&tab) {
            continue;
        }
        let Some(panel) = ui.tabs.get(tab.index()).copied() else {
            continue;
        };
        despawn_children(&children, &mut commands, panel);
        match tab {
            ProfileTab::SecondLife => build_second_life_tab(
                &mut commands,
                panel,
                &build,
                &mut state,
                &mut ui,
                &mut textures,
            ),
            ProfileTab::Web => build_web_tab(&mut commands, panel, &build, &state, &mut ui),
            ProfileTab::Picks => build_picks_tab(
                &mut commands,
                panel,
                &build,
                &mut state,
                &mut ui,
                &mut textures,
            ),
            ProfileTab::Classifieds => build_classifieds_tab(
                &mut commands,
                panel,
                &build,
                &mut state,
                &mut ui,
                &mut textures,
            ),
            ProfileTab::FirstLife => build_first_life_tab(
                &mut commands,
                panel,
                &build,
                &mut state,
                &mut ui,
                &mut textures,
            ),
            ProfileTab::Notes => build_notes_tab(&mut commands, panel, &state, &mut ui),
        }
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

impl BuildContext<'_> {
    /// The display name for an agent, falling back to its id.
    fn name_of(&self, agent: AgentKey) -> String {
        self.avatars
            .name_of(agent)
            .map_or_else(|| format!("({agent})"), str::to_owned)
    }
}

/// Despawn every child of `parent`.
fn despawn_children(children: &Query<&Children>, commands: &mut Commands, parent: Entity) {
    if let Ok(existing) = children.get(parent) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
}

/// Build the 2nd Life tab: name / key / picture / status / account / partner /
/// groups / about, then the action buttons (other) or Save controls (own).
fn build_second_life_tab(
    commands: &mut Commands,
    panel: Entity,
    build: &BuildContext,
    state: &mut ProfileState,
    ui: &mut ProfileUi,
    textures: &mut TextureManager,
) {
    ui.about_field = None;
    ui.pay_amount_field = None;
    let name_row = spawn_labeled_row(commands, panel, "profile-name");
    spawn_value_label(commands, name_row, build.name_of(build.target), LABEL_COLOR);
    let key_row = spawn_labeled_row(commands, panel, "profile-key");
    spawn_value_label(commands, key_row, build.target.to_string(), DIM_LABEL_COLOR);

    // Picture beside the status / account facts, as the reference lays it out.
    let picture_row = commands
        .spawn((
            Node {
                align_items: AlignItems::FlexStart,
                ..row(Val::Px(8.0))
            },
            ChildOf(panel),
        ))
        .id();
    let image_id = state.properties.as_ref().map(|props| props.image_id);
    spawn_profile_image(commands, picture_row, image_id, state, textures);
    let facts = commands
        .spawn((
            Node {
                ..column(Val::Px(4.0))
            },
            ChildOf(picture_row),
        ))
        .id();
    if let Some(props) = state.properties.clone() {
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
                spawn_value_label(commands, partner_row, build.name_of(partner), LABEL_COLOR);
            }
            None => spawn_key_label(
                commands,
                partner_row,
                "profile-partner-none",
                DIM_LABEL_COLOR,
            ),
        }
    } else {
        spawn_key_label(commands, facts, "profile-loading", DIM_LABEL_COLOR);
    }

    // Groups.
    spawn_section_label(commands, panel, "profile-groups");
    let group_list = commands
        .spawn((
            Node {
                max_height: Val::Px(90.0),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            ChildOf(panel),
        ))
        .id();
    match state.groups.as_deref() {
        Some([]) | None => {
            spawn_key_label(commands, group_list, "profile-groups-none", DIM_LABEL_COLOR);
        }
        Some(groups) => {
            for group in groups {
                spawn_value_label(commands, group_list, group.group_name.clone(), LABEL_COLOR);
            }
        }
    }

    // About.
    spawn_section_label(commands, panel, "profile-about");
    let about = state
        .properties
        .as_ref()
        .map(|props| props.about_text.clone())
        .unwrap_or_default();
    if build.own {
        ui.about_field = Some(spawn_text_input(
            commands,
            panel,
            &TextInputSpec {
                initial: about,
                font_size: PROFILE_FONT_SIZE,
                visible_lines: 5.0,
                tab_index: 2,
                max_characters: Some(510),
                ..TextInputSpec::new("profile-about", TextInputKind::Multiline)
            },
        ));
    } else {
        spawn_text_block(commands, panel, about);
    }

    if build.own {
        // Show in search + Save / Discard.
        spawn_check_button(
            commands,
            panel,
            "profile-show-in-search",
            ProfileAction::ToggleShowInSearch,
            state.show_in_search,
        );
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
        // The reference's action button row.
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
        // Placeholders for features this viewer does not have yet.
        spawn_disabled_button(commands, buttons, "profile-find-on-map");
        spawn_disabled_button(commands, buttons, "profile-invite-to-group");
        // The reference's Share area: the whole floater is the drop target
        // (`AgentDropTarget` on the root); this hint says so.
        spawn_section_label(commands, panel, "profile-share");
        spawn_key_label(commands, panel, "profile-share-hint", DIM_LABEL_COLOR);
        // Pay: amount + button.
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
    if let Some(page) = crate::web_floater::normalize_web_url(&url) {
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
    textures: &mut TextureManager,
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
    spawn_snapshot(commands, detail_panel, info.snapshot_id, state, textures);
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
    textures: &mut TextureManager,
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
    spawn_snapshot(commands, detail_panel, info.snapshot_id, state, textures);
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
    textures: &mut TextureManager,
) {
    ui.fl_about_field = None;
    let image_id = state.properties.as_ref().map(|props| props.fl_image_id);
    spawn_profile_image(commands, panel, image_id, state, textures);
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

/// A translated label.
fn spawn_key_label(commands: &mut Commands, parent: Entity, key: &'static str, color: Color) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(color),
        ChildOf(parent),
    ));
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

/// A clickable check-glyph toggle dispatching `action`.
fn spawn_check_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: ProfileAction,
    on: bool,
) {
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
    commands.spawn((
        Text::new(if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH }),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(if on { CHECK_COLOR } else { DIM_LABEL_COLOR }),
        Pickable::IGNORE,
        ChildOf(host),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROFILE_FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(host),
    ));
}

/// A profile picture: request the texture and show a placeholder until it
/// decodes ([`poll_profile_textures`] swaps the image in).
fn spawn_profile_image(
    commands: &mut Commands,
    parent: Entity,
    image_id: Option<TextureKey>,
    state: &mut ProfileState,
    textures: &mut TextureManager,
) {
    let node = spawn_image_box(commands, parent, Vec2::splat(PROFILE_IMAGE_EDGE));
    request_ui_texture(commands, image_id, node, state, textures);
}

/// A pick / classified snapshot node, with the texture requested like the
/// profile pictures.
fn spawn_snapshot(
    commands: &mut Commands,
    parent: Entity,
    snapshot_id: Option<TextureKey>,
    state: &mut ProfileState,
    textures: &mut TextureManager,
) {
    let node = spawn_image_box(commands, parent, SNAPSHOT_SIZE);
    request_ui_texture(commands, snapshot_id, node, state, textures);
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
    textures: &mut TextureManager,
) {
    let key = image_id.filter(|key| *key != TextureKey::from(Uuid::nil()));
    let Some(key) = key else {
        spawn_key_label(commands, node, "profile-image-none", DIM_LABEL_COLOR);
        return;
    };
    spawn_key_label(commands, node, "profile-loading", DIM_LABEL_COLOR);
    textures.request_boosted(key, AVATAR_BOOST_PRIORITY);
    state.pending_textures.push((key, node));
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

/// Dispatch a clicked profile button to the behaviour behind it.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the action marker, \
              the state, the UI field handles, the identity / name sources, and the command \
              and repaint outputs"
)]
fn on_profile_action(
    press: On<Pointer<Press>>,
    actions: Query<&ProfileAction>,
    mut state: ResMut<ProfileState>,
    mut dirty: ResMut<ProfileDirty>,
    ui: Res<ProfileUi>,
    fields: Query<&EditableText>,
    avatars: Res<AvatarState>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut conversations: MessageWriter<OpenConversation>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    let Some(target) = state.target else {
        return;
    };
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
            sl_commands.write(SlCommand(Command::OfferFriendship {
                to_agent_id: target,
                message: String::new(),
            }));
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
            sl_commands.write(SlCommand(Command::Mute {
                id: target.uuid(),
                name,
                mute_type: MuteType::Agent,
                flags: MuteFlags::default(),
            }));
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
    mut state: ResMut<ProfileState>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    if state.pending_textures.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut state.pending_textures);
    for (key, node) in pending {
        let Ok(mut entity) = commands.get_entity(node) else {
            continue;
        };
        if let Some(decoded) = manager.decoded(key) {
            let handle = images.add(to_bevy_image(decoded));
            entity.insert(ImageNode::new(handle));
            // Drop the "(loading)" label under the image.
            despawn_children(&children, &mut commands, node);
        } else {
            state.pending_textures.push((key, node));
        }
    }
}

/// Keep the Web tab's load-status line current: "loading" while the embedded
/// page loads, then the reference's load-time string ("Page loaded in N s")
/// once it finishes. Tracks the view entity so a tab rebuild restarts the
/// clock.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the profile \
              floater's entities, the browser view / surface lookups, the clock, the \
              translator and the status label"
)]
fn update_profile_web_status(
    ui: Res<ProfileUi>,
    views: Query<&crate::browser_widget::BrowserView>,
    surfaces: bevy::ecs::system::NonSend<crate::media_engine::MediaSurfaces>,
    time: Res<Time>,
    translator: crate::i18n::Translator,
    mut tracked: Local<Option<(Entity, f64, bool)>>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let (Some(view_entity), Some(status_entity)) = (ui.web_view, ui.web_status) else {
        *tracked = None;
        return;
    };
    let now = time.elapsed_secs_f64();
    let restart = !matches!(*tracked, Some((entity, _, _)) if entity == view_entity);
    if restart {
        *tracked = Some((view_entity, now, false));
    }
    let Some((_, started, done)) = tracked.as_mut() else {
        return;
    };
    if *done {
        return;
    }
    let Ok(view) = views.get(view_entity) else {
        return;
    };
    let Some(slot) = view.surface.and_then(|id| surfaces.get(id)) else {
        return;
    };
    if slot.status.loading || slot.status.progress < 1.0 {
        return;
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
}
