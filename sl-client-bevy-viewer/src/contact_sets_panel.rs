//! The **Contact Sets** surface (`viewer-contact-sets`) — the UI over the
//! [contact-set model](crate::contact_sets), hosted in the fourth sub-tab of the
//! [People pane](crate::people) inside the Conversations floater.
//!
//! # Why it lives in the People pane
//!
//! The reference viewer's **Vintage** skin folds Contacts into the Conversations
//! window as one hosted tab whose content is Friends / Groups / **Contact Sets**
//! (`panel_people_contact_sets.xml`), and [`crate::people`] already owns that
//! sub-tab strip plus an empty content slot per tab. This module fills the
//! Contact Sets slot, laid out like the Blocked list beside it: a set chooser and
//! a filter box over a sortable, virtualized member table with a trailing action
//! column.
//!
//! # What it does
//!
//! - The **set chooser** lists every set, above the three pseudo-sets the
//!   reference offers: *All Sets* (everyone filed anywhere), *No Sets*
//!   (friends filed nowhere — the list to work through when starting out) and
//!   *Pseudonyms* (everyone the user has given an alias, who need not be in any
//!   set at all).
//! - **New Set…** raises the reference's own `AddNewContactSet` prompt;
//!   **Delete Set** its `RemoveContactSet` confirmation; **Configure…** opens the
//!   set's settings floater, where the set is renamed, recoloured, and given the
//!   three behaviours it carries — announce this set's comings and goings, list
//!   it online-first, and answer its members with replies of its own
//!   (`viewer-contact-set-presence-extras`; the reference's
//!   `floater_fs_contact_set_configuration`). The reply fields commit on losing
//!   focus, and on the floater turning to another set, as the reference's do.
//! - **Add Resident…** files someone chosen in the shared
//!   [avatar picker](crate::avatar_picker); **Move to Set…** opens the
//!   add-to-set floater in move mode; **Remove from Set** confirms first.
//! - **Set Alias… / Rem Alias… / Rem DN…** are the reference's three pseudonym
//!   buttons: the first raises its `SetAvatarPseudonym` prompt, the other two
//!   clear an alias and suppress a display name. What they change is not shown
//!   here alone — an alias is mirrored into the name cache, so the person is
//!   renamed everywhere at once ([`crate::contact_sets`]).
//! - Each member row is **tinted with that person's set colour**
//!   ([`ContactSets::color_of`]) — the same answer the radar, name tags and chat
//!   will read once they colour by set, so what the panel shows is what the rest
//!   of the viewer will show.
//!
//! # A button that cannot act says so
//!
//! Each action button is **greyed and inert** whenever it does not apply to
//! what the panel is showing — *Configure…* with a pseudo-set chosen (there is
//! no *All Sets* to configure), *Rem Alias…* with nobody selected or nobody
//! aliased, and so on ([`ContactSetsButton::is_enabled`], the one predicate both
//! [`sync_panel_button_states`] and [`on_panel_button_press`] read, so the look
//! and the behaviour cannot drift). The greying is the **skin's**: each button
//! and label carries a base class and gains `.sk-disabled-surface` /
//! `.sk-disabled-text` on top, so a skin decides what greyed looks like.
//!
//! # One way in
//!
//! Nothing here mutates the sets: every button, floater and pick writes a
//! [`RequestContactSet`], so the model's guards decide — the same arrangement the
//! block list has with [`RequestBlock`](crate::mutes::RequestBlock).
//!
//! Reference (Firestorm, read-only): `fspanelcontactsets`,
//! `fsfloateraddtocontactset`, `fsfloatercontactsetconfiguration`,
//! `panel_people_contact_sets.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{AgentKey, Command, SlCommand};

use crate::avatar_picker::{AvatarPicked, OpenAvatarPicker};
use crate::avatar_profile::OpenAvatarProfile;
use crate::contact_sets::{
    ALL_SETS_LABEL, ContactSet, ContactSets, NO_SETS_LABEL, PSEUDONYMS_KEY, RequestContactSet,
    SetAutoresponseMode, apply_contact_set_requests,
};
use crate::conversations::{ConversationKey, OpenConversation};
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::notifications::{NotificationResponse, ShowNotification};
use crate::people::PeopleUi;
use crate::settings::ViewerSettings;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, SetComboOptions, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, register_table_settings, set_table_cell, spawn_table,
    spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists, spawn_virtual_scrollbar};
use crate::world_api::FriendsModel;

/// The tag the panel opens the shared avatar picker under, so only its own pick
/// is consumed.
const PICKER_REQUESTER: &str = "contact-sets";

/// The add-to-set floater's stable id (persistence, `SL_VIEWER_OPEN_FLOATER`).
const ADD_TO_SET_FLOATER_ID: &str = "add-to-contact-set";

/// The set-settings floater's stable id.
const CONFIG_FLOATER_ID: &str = "contact-set-config";

/// The persisted-settings section the member table's state lives under.
const CONTACT_SETS_SECTION: &[&str] = &["contact_sets"];

// --- Palette / geometry (the sibling People panes' values) ----------------

/// Header / cell font size, logical px.
const FONT_SIZE: f32 = 13.0;

/// Table row height, logical px.
const ROW_HEIGHT: f32 = 20.0;

/// The default cell / label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The dimmed header / secondary colour.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// The list viewport backdrop.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// A selected row's background highlight.
const SELECTED_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);

/// An action button's background. The pre-skin fallback only: the button also
/// carries [`BUTTON_CLASS`], and the skin's `.sk-button` rule is what actually
/// paints it once the stylesheet has loaded.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The skin class every action button's surface carries.
const BUTTON_CLASS: &str = "sk-button";

/// The skin class every action button's **label** carries.
const BUTTON_LABEL_CLASS: &str = "sk-text";

/// The skin class greying the surface of a button whose action does not apply
/// right now (`--control-bg-disabled`; see `assets/skins/common.css`).
const DISABLED_SURFACE_CLASS: &str = "sk-disabled-surface";

/// The skin class greying such a button's label (`--text-disabled`).
const DISABLED_TEXT_CLASS: &str = "sk-disabled-text";

/// The trailing action column's width, logical px.
const ACTION_COL_WIDTH: f32 = 150.0;

/// The glyph a checked settings-floater toggle shows.
const CHECKED_GLYPH: &str = "\u{2611}";

/// The glyph an unchecked one shows.
const UNCHECKED_GLYPH: &str = "\u{2610}";

// --- Table ----------------------------------------------------------------

/// Column index of the member's name.
const COL_NAME: usize = 0;

/// Column index of the sets they are in.
const COL_SETS: usize = 1;

/// The member table: a flexible name beside the sets that name is filed under,
/// sorted by name ascending by default. Selection is module-owned (keyed by
/// agent, not row index) because the list re-sorts as members come and go.
static CONTACT_SETS_TABLE: TableSpec = TableSpec {
    element: "contact-sets",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "contact-sets-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "contact-sets-col-sets",
            token: "sets",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 140.0 },
            align: TableAlign::Start,
            sortable: true,
        },
    ],
    default_sort: &[TableSortDefault {
        column: COL_NAME,
        ascending: true,
    }],
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: Some("ContactSetsSortOrder"),
    widths_setting: Some("ContactSetsColumnWidths"),
};

// --- Pure view model ------------------------------------------------------

/// One member row, as the table shows it.
#[derive(Debug, Clone, PartialEq)]
struct MemberRow {
    /// The resident.
    agent: AgentKey,
    /// Their best-known name (or a short id when nothing has resolved).
    name: String,
    /// The sets they are filed under, comma-joined — informative in the *All
    /// Sets* view, and the second sort key.
    sets: String,
    /// The colour their smallest set gives them, if any.
    color: Option<Color>,
    /// Whether the grid last reported them online — the leading sort key of a
    /// set configured to sort by online status. A member who is not a friend has
    /// no presence to report, and sorts with the offline.
    online: bool,
}

/// Whether `name` survives the list's filter (case-insensitive substring, like
/// the reference's own list filters).
fn matches_filter(name: &str, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase())
}

/// Order `rows` by the table's sort keys (most significant first), falling back
/// to a case-insensitive name compare so the order is total.
///
/// A set configured to **sort by online status**
/// (`viewer-contact-set-presence-extras`) puts the residents the grid last
/// reported online first, ahead of every other key — the reference's own
/// comparator, which likewise falls through to the ordinary order within each
/// group.
fn sort_rows(rows: &mut [MemberRow], keys: &[(&str, bool)], online_first: bool) {
    rows.sort_by(|left, right| {
        if online_first && left.online != right.online {
            // `true` sorts first, which `bool`'s own order has backwards.
            return right.online.cmp(&left.online);
        }
        for (token, ascending) in keys {
            let ordering = match *token {
                "sets" => left.sets.to_lowercase().cmp(&right.sets.to_lowercase()),
                _name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
            };
            let ordering = if *ascending {
                ordering
            } else {
                ordering.reverse()
            };
            if ordering != core::cmp::Ordering::Equal {
                return ordering;
            }
        }
        left.name.to_lowercase().cmp(&right.name.to_lowercase())
    });
}

/// The set-chooser options: the three pseudo-sets first (they always work, even
/// with no sets at all), then every real set in name order.
fn chooser_options(sets: &ContactSets) -> Vec<String> {
    let mut options = vec![
        ALL_SETS_LABEL.to_owned(),
        NO_SETS_LABEL.to_owned(),
        PSEUDONYMS_KEY.to_owned(),
    ];
    options.extend(sets.sets().map(|set| set.name().to_owned()));
    options
}

/// Whether `choice` names a real, mutable set (rather than a pseudo-set) — what
/// gates the buttons that change one.
fn is_real_set(sets: &ContactSets, choice: &str) -> bool {
    sets.set(choice).is_some()
}

/// A short, readable stand-in for an unresolved agent id — its first eight hex
/// digits, matching the People pane's own placeholder.
fn short_id(agent: AgentKey) -> String {
    let text = agent.uuid().to_string();
    text.split('-')
        .next()
        .map_or_else(|| text.clone(), ToOwned::to_owned)
}

// --- Resources ------------------------------------------------------------

/// The panel's live view state: the chosen set, the filter, the ordered rows the
/// virtual list binds, and the stamps they were built against.
#[derive(Resource, Debug, Default)]
struct ContactSetsView {
    /// The chosen set (or pseudo-set) label.
    choice: String,
    /// The live name filter.
    filter: String,
    /// The display rows, in table order.
    rows: Vec<MemberRow>,
    /// The contact-set revision the rows were built at.
    built_revision: u64,
    /// The friends-model revision the rows were built at (the *No Sets* view
    /// reads the roster).
    built_friends_revision: u64,
    /// The table sort revision the rows were ordered at.
    built_sort_revision: u64,
    /// The filter the rows were filtered by.
    built_filter: String,
    /// The choice the rows were built for.
    built_choice: String,
    /// The chooser options the combo was last given.
    built_options: Vec<String>,
}

/// The selected member, which the trailing buttons act on. Keyed by agent (not
/// row index) so it survives the re-sort and the virtualized row recycling.
#[derive(Resource, Debug, Default)]
struct SelectedMember(Option<AgentKey>);

/// The panel's retained entities (inserted by the deferred build; consumers take
/// `Option<Res<ContactSetsUi>>` until then).
#[derive(Resource, Debug)]
struct ContactSetsUi {
    /// The set-chooser combo.
    chooser: Entity,
    /// The table root (carries [`TableState`]).
    table: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The filter box's [`EditableText`] entity.
    filter_field: Entity,
    /// The member-count line.
    count_text: Entity,
}

/// The add-to-set floater's retained entities.
#[derive(Resource, Debug)]
struct AddToSetUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The "Add <name> to contact set:" prompt line.
    prompt: Entity,
    /// The set combo.
    chooser: Entity,
    /// The options the combo was last given, so a pick resolves to a set name.
    options: Vec<String>,
}

/// Who the open add-to-set floater is about.
#[derive(Resource, Debug, Default)]
struct AddToSetTarget {
    /// The residents to file, with the names they were opened under (empty when
    /// the floater is closed).
    agents: Vec<(AgentKey, String)>,
    /// The set to take them out of afterwards — the reference's move mode.
    move_from: Option<String>,
    /// The set the combo currently shows.
    chosen: String,
}

impl AddToSetTarget {
    /// The one resident this is about, or `None` when it is about several (or
    /// none) — what the single-resident prompt and notification need.
    const fn single(&self) -> Option<&(AgentKey, String)> {
        match self.agents.as_slice() {
            [only] => Some(only),
            _several_or_none => None,
        }
    }
}

/// One per-set autoresponse block in the settings floater: the "use a reply of
/// this set's own" toggle and the reply field under it.
#[derive(Debug, Clone, Copy)]
struct ConfigAutoresponseUi {
    /// The toggle's glyph node, showing checked / unchecked.
    glyph: Entity,
    /// The reply field's [`EditableText`] entity.
    field: Entity,
}

/// The set-settings floater's retained entities.
#[derive(Resource, Debug)]
struct ConfigUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The floater's title text, rewritten with the set's name.
    title: Entity,
    /// The set-name field's [`EditableText`] entity.
    name_field: Entity,
    /// The set-colour swatch (also the [`ColorPicked`] requester).
    swatch: Entity,
    /// The "announce this set's comings and goings" toggle's glyph node.
    notify_glyph: Entity,
    /// The "list this set online-first" toggle's glyph node.
    sort_glyph: Entity,
    /// The Do Not Disturb reply block.
    busy_reply: ConfigAutoresponseUi,
    /// The autorespond reply block.
    autorespond_reply: ConfigAutoresponseUi,
    /// The autorespond-to-non-friends reply block.
    non_friends_reply: ConfigAutoresponseUi,
}

impl ConfigUi {
    /// The block for one reply mode.
    const fn autoresponse(&self, mode: SetAutoresponseMode) -> ConfigAutoresponseUi {
        match mode {
            SetAutoresponseMode::Busy => self.busy_reply,
            SetAutoresponseMode::Autorespond => self.autorespond_reply,
            SetAutoresponseMode::NonFriends => self.non_friends_reply,
        }
    }
}

/// The three reply modes in the order the floater lists them (the reference's
/// own order), so the build, the seed and the commit walk the same list.
const AUTORESPONSE_MODES: &[SetAutoresponseMode] = &[
    SetAutoresponseMode::Busy,
    SetAutoresponseMode::Autorespond,
    SetAutoresponseMode::NonFriends,
];

/// Which set the open settings floater is about.
#[derive(Resource, Debug, Default)]
struct ConfigTarget(Option<String>);

/// The name a rename asked for, until the model has had its say — the floater
/// must follow a set that was renamed, and must **not** follow a name the model
/// refused (a duplicate), which is exactly what a hopeful write of
/// [`ConfigTarget`] would do.
#[derive(Resource, Debug, Default)]
struct PendingRename(Option<String>);

/// What a pending confirmation / prompt applies to once it is answered.
#[derive(Resource, Debug, Default)]
enum PendingAction {
    /// Nothing is pending.
    #[default]
    None,
    /// A new set is being named; file `then_add` under it once it exists.
    Create {
        /// The residents to file under the new set — empty when the prompt came
        /// from a path that was not filing anyone (the panel's New Set…).
        then_add: Vec<(AgentKey, String)>,
        /// The set to take them out of afterwards (the add floater's move mode).
        move_from: Option<String>,
    },
    /// A set is being deleted.
    RemoveSet {
        /// The set.
        name: String,
    },
    /// A member is being taken out of a set.
    RemoveMember {
        /// The set.
        set: String,
        /// The resident.
        agent: AgentKey,
    },
    /// A resident is being given an alias, once the prompt is answered.
    SetAlias {
        /// The resident.
        agent: AgentKey,
        /// The best name the raising surface knew for them, remembered with the
        /// alias so an aliased person filed nowhere is still identifiable.
        name: String,
    },
}

/// The agent a pooled row currently presents.
#[derive(Component, Debug, Clone, Copy, Default)]
struct BoundMember(Option<AgentKey>);

/// What a panel button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ContactSetsButton {
    /// Prompt for a name and create a set.
    NewSet,
    /// Confirm, then delete the chosen set.
    DeleteSet,
    /// Open the chosen set's settings floater.
    Configure,
    /// Open the avatar picker to file someone under the chosen set.
    AddResident,
    /// Open the add-to-set floater in move mode for the selected member.
    MoveMember,
    /// Confirm, then take the selected member out of the chosen set.
    RemoveMember,
    /// Open the selected member's profile.
    Profile,
    /// Open a one-to-one IM with the selected member.
    Im,
    /// Offer the selected member a teleport.
    OfferTeleport,
    /// Prompt for an alias to show the selected member under.
    SetAlias,
    /// Drop the selected member's alias.
    ClearAlias,
    /// Show the selected member's legacy name instead of their display name.
    RemoveDisplayName,
}

/// An action button's label node, so [`sync_panel_button_states`] can grey the
/// two nodes together (bevy_ui has no style inheritance, so the label carries
/// its own colour).
#[derive(Component, Debug, Clone, Copy)]
struct PanelButtonLabel(Entity);

impl ContactSetsButton {
    /// The Fluent key for this button's label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::NewSet => "contact-sets-action-new",
            Self::DeleteSet => "contact-sets-action-delete",
            Self::Configure => "contact-sets-action-configure",
            Self::AddResident => "contact-sets-action-add",
            Self::MoveMember => "contact-sets-action-move",
            Self::RemoveMember => "contact-sets-action-remove",
            Self::Profile => "contact-sets-action-profile",
            Self::Im => "contact-sets-action-im",
            Self::OfferTeleport => "contact-sets-action-teleport",
            Self::SetAlias => "contact-sets-action-set-alias",
            Self::ClearAlias => "contact-sets-action-clear-alias",
            Self::RemoveDisplayName => "contact-sets-action-remove-display-name",
        }
    }

    /// Whether this button applies to what the panel is showing right now —
    /// what [`sync_panel_button_states`] greys on, and what the press handler
    /// checks before doing anything.
    ///
    /// Three preconditions between them cover all twelve: a **real** set is
    /// chosen (the three pseudo-sets are views, and there is no *All Sets* to
    /// rename, delete, configure or file someone into), a **member** is
    /// selected, and — for the two alias buttons — that member has an alias to
    /// drop, or a display name still to suppress. *New Set…* alone always
    /// applies: it is how the first set comes into being.
    fn is_enabled(self, sets: &ContactSets, choice: &str, selected: Option<AgentKey>) -> bool {
        let real_set = is_real_set(sets, choice);
        match self {
            Self::NewSet => true,
            Self::DeleteSet | Self::Configure | Self::AddResident => real_set,
            Self::MoveMember | Self::RemoveMember => real_set && selected.is_some(),
            Self::Profile | Self::Im | Self::OfferTeleport | Self::SetAlias => selected.is_some(),
            Self::ClearAlias => selected.is_some_and(|agent| sets.has_alias(agent)),
            Self::RemoveDisplayName => {
                selected.is_some_and(|agent| !sets.has_display_name_removed(agent))
            }
        }
    }
}

/// Which of the add-to-set floater's three buttons a node is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum AddToSetButton {
    /// File the target under the chosen set (and, in move mode, unfile them from
    /// where they were).
    Add,
    /// Close without filing anyone.
    Cancel,
    /// Prompt for a new set's name and file them under it.
    NewSet,
}

/// Which of the settings floater's two buttons a node is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigButton {
    /// Rename the set to what the name field says.
    Rename,
    /// Close the floater.
    Close,
}

/// Which of the settings floater's five checkboxes a node is
/// (`viewer-contact-set-presence-extras`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigToggle {
    /// Announce this set's members coming and going.
    Notify,
    /// List this set online-first.
    SortByOnlineStatus,
    /// Answer this set with a reply of its own, in one mode.
    Autoresponse(SetAutoresponseMode),
}

/// Ask for the "give this resident an alias" prompt (the reference's
/// `SetAvatarPseudonym`). The panel's **Set Alias…** and the avatar pie's
/// **Add ▸ Set Alias** both write it, so the prompt is raised — and answered —
/// in one place.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenSetPseudonym {
    /// The resident to alias.
    pub(crate) agent: AgentKey,
    /// The best name the opening surface knows for them, shown in the prompt and
    /// remembered beside the alias.
    pub(crate) name: String,
}

/// Ask for the add-to-set floater. The avatar pie's **Add ▸ Add to Set**, the
/// panel's **Move to Set…** and the minimap's multi-avatar **Add to Set** all
/// write this.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenAddToContactSet {
    /// The residents to file, each with the best name the opening surface knows
    /// for them (empty when it knows none). Usually one; the reference's
    /// multi-avatar entries hand over several, and the floater then asks for one
    /// set to file the lot under.
    pub(crate) agents: Vec<(AgentKey, String)>,
    /// The set to take them out of once they are filed — the reference's move
    /// mode. `None` for a plain add.
    pub(crate) move_from: Option<String>,
}

impl OpenAddToContactSet {
    /// File one resident.
    pub(crate) fn one(agent: AgentKey, name: String) -> Self {
        Self {
            agents: vec![(agent, name)],
            move_from: None,
        }
    }

    /// File several residents at once.
    pub(crate) const fn many(agents: Vec<(AgentKey, String)>) -> Self {
        Self {
            agents,
            move_from: None,
        }
    }

    /// The same request in the reference's *move* mode: take them out of `set`
    /// once they are filed.
    #[must_use]
    pub(crate) fn moving_from(mut self, set: String) -> Self {
        self.move_from = Some(set);
        self
    }
}

// --- Plugin ---------------------------------------------------------------

/// Registers the Contact Sets panel, its two floaters and their wiring.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ContactSetsPanelPlugin;

impl Plugin for ContactSetsPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContactSetsView>()
            .init_resource::<SelectedMember>()
            .init_resource::<AddToSetTarget>()
            .init_resource::<ConfigTarget>()
            .init_resource::<PendingRename>()
            .init_resource::<PendingAction>()
            .add_message::<OpenAddToContactSet>()
            .add_message::<OpenSetPseudonym>()
            .add_systems(
                Startup,
                (
                    register_contact_sets_settings,
                    spawn_add_to_set_floater.after(UiScaffoldSystems::SpawnRoot),
                    spawn_config_floater.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (
                    spawn_contact_sets_panel.after(UiScaffoldSystems::SpawnRoot),
                    mirror_contact_sets_filter,
                    sync_chooser_options,
                    handle_chooser_picks,
                    handle_open_add_to_set,
                    handle_open_set_pseudonym,
                    handle_contact_set_picks,
                    handle_contact_set_colors,
                    handle_contact_set_notifications,
                    // Ahead of the sync, which re-seeds the reply fields: a
                    // pending edit is written back before it is overwritten.
                    commit_config_autoresponses,
                    sync_config_floater,
                    rebuild_contact_sets_view,
                    sync_panel_button_states,
                )
                    .chain()
                    .before(layout_virtual_lists)
                    // The prompts and picks all write RequestContactSet, so the
                    // model applies them in the same frame they are made.
                    .before(apply_contact_set_requests),
            )
            .add_systems(
                Update,
                // After the model has applied this frame's requests, so a rename
                // is settled against what the guards actually did.
                settle_contact_set_rename.after(apply_contact_set_requests),
            )
            .add_systems(
                Update,
                (populate_member_rows, bind_member_rows)
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

/// Register the member table's sort / width persistence.
fn register_contact_sets_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_table_settings(&mut settings, CONTACT_SETS_SECTION, &CONTACT_SETS_TABLE);
}

// --- Spawn (deferred until the People pane exists) -------------------------

/// Spawn the panel into the People pane's Contact Sets content slot, once
/// ([`ContactSetsUi`] absent) and only after that pane exists ([`PeopleUi`]
/// present) — the same deferral the group and block lists use.
fn spawn_contact_sets_panel(
    mut commands: Commands,
    people: Option<Res<PeopleUi>>,
    existing: Option<Res<ContactSetsUi>>,
) {
    if existing.is_some() {
        return;
    }
    let Some(people) = people else {
        return;
    };
    let content = people.contact_sets_content();

    // The chooser row: which set the list is showing, and the three buttons that
    // change the set itself.
    let chooser_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..row(Val::Px(6.0))
            },
            Name::new("contact-sets-chooser-row"),
            ChildOf(content),
        ))
        .id();
    let chooser = spawn_combo(
        &mut commands,
        chooser_row,
        &ComboSpec {
            element: "contact-sets-chooser",
            labels: &[ALL_SETS_LABEL.to_owned(), NO_SETS_LABEL.to_owned()],
            active: 0,
            tab_index: 1,
            font_size: FONT_SIZE,
            translate_labels: false,
        },
    );
    for button in [
        ContactSetsButton::NewSet,
        ContactSetsButton::Configure,
        ContactSetsButton::DeleteSet,
    ] {
        spawn_panel_button(&mut commands, chooser_row, button);
    }

    // The filter row.
    let filter_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..row(Val::Px(6.0))
            },
            Name::new("contact-sets-filter-row"),
            ChildOf(content),
        ))
        .id();
    let search = spawn_search_field(
        &mut commands,
        filter_row,
        &SearchFieldSpec {
            tab_index: 2,
            font_size: FONT_SIZE,
            min_width: 140.0,
            placeholder: "Filter by name".to_owned(),
            search_glyph: true,
            ..SearchFieldSpec::new("contact-sets-filter")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("contact-sets-filter-placeholder"));
    }

    // The body row: the table takes the width, the actions sit at its trailing
    // edge (mirroring the Friends / Blocked content layout).
    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::Px(6.0))
            },
            Name::new("contact-sets-body"),
            ChildOf(content),
        ))
        .id();
    let table_column = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(2.0))
            },
            Name::new("contact-sets-list-column"),
            ChildOf(body),
        ))
        .id();
    let table = spawn_table(&mut commands, table_column, &CONTACT_SETS_TABLE);
    commands
        .entity(table.viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(3)));
    spawn_virtual_scrollbar(&mut commands, table.viewport);

    let count_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
            Name::new("contact-sets-count"),
            ChildOf(table_column),
        ))
        .id();

    let actions = commands
        .spawn((
            Node {
                width: Val::Px(ACTION_COL_WIDTH),
                flex_shrink: 0.0,
                align_items: AlignItems::Stretch,
                ..column(Val::Px(4.0))
            },
            Name::new("contact-sets-actions"),
            ChildOf(body),
        ))
        .id();
    for button in [
        ContactSetsButton::AddResident,
        ContactSetsButton::MoveMember,
        ContactSetsButton::RemoveMember,
        ContactSetsButton::Profile,
        ContactSetsButton::Im,
        ContactSetsButton::OfferTeleport,
        ContactSetsButton::SetAlias,
        ContactSetsButton::ClearAlias,
        ContactSetsButton::RemoveDisplayName,
    ] {
        spawn_panel_button(&mut commands, actions, button);
    }

    commands.insert_resource(ContactSetsUi {
        chooser,
        table: table.root,
        viewport: table.viewport,
        filter_field: search.field,
        count_text,
    });
}

/// Spawn one panel button.
fn spawn_panel_button(commands: &mut Commands, parent: Entity, button: ContactSetsButton) {
    let label = commands
        .spawn((
            Text::default(),
            Translated::new(button.label_key()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            ClassList::new_with_classes([BUTTON_LABEL_CLASS]),
            Pickable::IGNORE,
        ))
        .id();
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            button,
            // The label is held on the button so the greying pass reaches it
            // without walking children (and cannot grey the wrong node).
            PanelButtonLabel(label),
            Name::new("contact-sets-action"),
            ChildOf(parent),
        ))
        .add_child(label)
        .observe(on_panel_button_press);
}

/// Spawn the add-to-set floater (hidden): the prompt, the set combo, and
/// Add / New Set… / Cancel — the reference's `floater_fs_contact_add`.
fn spawn_add_to_set_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: ADD_TO_SET_FLOATER_ID,
            title: "Add to Contact Set".to_owned(),
            position: Vec2::new(380.0, 220.0),
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("add-to-contact-set-title"));
    let content = handle.content;
    let prompt = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            Name::new("add-to-contact-set-prompt"),
            ChildOf(content),
        ))
        .id();
    let chooser = spawn_combo(
        &mut commands,
        content,
        &ComboSpec {
            element: "add-to-contact-set-chooser",
            labels: &[],
            active: 0,
            tab_index: 1,
            font_size: FONT_SIZE,
            translate_labels: false,
        },
    );
    let buttons = commands
        .spawn((
            Node {
                ..row(Val::Px(8.0))
            },
            Name::new("add-to-contact-set-buttons"),
            ChildOf(content),
        ))
        .id();
    for button in [
        AddToSetButton::Add,
        AddToSetButton::NewSet,
        AddToSetButton::Cancel,
    ] {
        commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(ACTION_BACKGROUND),
                ClassList::new_with_classes([BUTTON_CLASS]),
                Pickable {
                    should_block_lower: true,
                    is_hoverable: true,
                },
                button,
                Name::new("add-to-contact-set-button"),
                ChildOf(buttons),
            ))
            .with_child((
                Text::default(),
                Translated::new(match button {
                    AddToSetButton::Add => "add-to-contact-set-add",
                    AddToSetButton::NewSet => "add-to-contact-set-new",
                    AddToSetButton::Cancel => "add-to-contact-set-cancel",
                }),
                UiFont::Sans.at(FONT_SIZE),
                TextColor(LABEL_COLOR),
                ClassList::new_with_classes([BUTTON_LABEL_CLASS]),
                Pickable::IGNORE,
            ))
            .observe(on_add_to_set_press);
    }

    commands.insert_resource(AddToSetUi {
        panel: handle.root,
        prompt,
        chooser,
        options: Vec::new(),
    });
}

/// Spawn the set-settings floater (hidden): the name field with its Rename
/// button, the colour swatch, and Close — the reference's
/// `floater_fs_contact_set_configuration`, minus the knobs whose features are not
/// built.
fn spawn_config_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: CONFIG_FLOATER_ID,
            title: "Contact Set Settings".to_owned(),
            position: Vec2::new(420.0, 260.0),
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    let content = handle.content;
    let name_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("contact-set-config-name-row"),
            ChildOf(content),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("contact-set-config-name"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(name_row),
    ));
    let name_field = spawn_text_input(
        &mut commands,
        name_row,
        &TextInputSpec {
            font_size: FONT_SIZE,
            width_glyphs: 24.0,
            tab_index: 1,
            ..TextInputSpec::new("contact-set-config-name-field", TextInputKind::Line)
        },
    );
    spawn_config_button(&mut commands, name_row, ConfigButton::Rename);

    let color_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("contact-set-config-color-row"),
            ChildOf(content),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("contact-set-config-color"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(color_row),
    ));
    let swatch = spawn_color_swatch(
        &mut commands,
        color_row,
        "contact-set-config",
        2,
        LABEL_COLOR,
    );

    // The two behaviour checkboxes, then one block per reply mode: a toggle over
    // the reply this set answers with.
    let notify_glyph = spawn_config_toggle(
        &mut commands,
        content,
        "contact-set-config-notify",
        3,
        ConfigToggle::Notify,
    );
    let sort_glyph = spawn_config_toggle(
        &mut commands,
        content,
        "contact-set-config-sort-online",
        4,
        ConfigToggle::SortByOnlineStatus,
    );
    // Two focus stops per block (the toggle and its field), after the four
    // above, in the order [`AUTORESPONSE_MODES`] lists them.
    let busy_reply =
        spawn_config_autoresponse(&mut commands, content, SetAutoresponseMode::Busy, 5);
    let autorespond_reply =
        spawn_config_autoresponse(&mut commands, content, SetAutoresponseMode::Autorespond, 7);
    let non_friends_reply =
        spawn_config_autoresponse(&mut commands, content, SetAutoresponseMode::NonFriends, 9);
    spawn_config_button(&mut commands, content, ConfigButton::Close);

    commands.insert_resource(ConfigUi {
        panel: handle.root,
        title: handle.title_text,
        name_field,
        swatch,
        notify_glyph,
        sort_glyph,
        busy_reply,
        autorespond_reply,
        non_friends_reply,
    });
}

/// Spawn one of the settings floater's checkboxes — a clickable ☐/☑ glyph and a
/// label — returning the glyph node the sync pass writes.
fn spawn_config_toggle(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab: i32,
    toggle: ConfigToggle,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..row(Val::Px(0.0))
            },
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            toggle,
            Name::new("contact-set-config-toggle"),
            ChildOf(parent),
        ))
        .observe(on_config_toggle_press)
        .id();
    let glyph = commands
        .spawn((
            Text::new(UNCHECKED_GLYPH.to_owned()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    glyph
}

/// Spawn one reply block: the "use a reply of this set's own" toggle over the
/// reply field it enables.
fn spawn_config_autoresponse(
    commands: &mut Commands,
    parent: Entity,
    mode: SetAutoresponseMode,
    tab: i32,
) -> ConfigAutoresponseUi {
    let glyph = spawn_config_toggle(
        commands,
        parent,
        match mode {
            SetAutoresponseMode::Busy => "contact-set-config-reply-busy",
            SetAutoresponseMode::Autorespond => "contact-set-config-reply-autorespond",
            SetAutoresponseMode::NonFriends => "contact-set-config-reply-non-friends",
        },
        tab,
        ConfigToggle::Autoresponse(mode),
    );
    let field = spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: FONT_SIZE,
            visible_lines: 3.0,
            tab_index: tab.saturating_add(1),
            ..TextInputSpec::new(
                match mode {
                    SetAutoresponseMode::Busy => "contact-set-config-reply-busy-field",
                    SetAutoresponseMode::Autorespond => {
                        "contact-set-config-reply-autorespond-field"
                    }
                    SetAutoresponseMode::NonFriends => "contact-set-config-reply-non-friends-field",
                },
                TextInputKind::Multiline,
            )
        },
    );
    ConfigAutoresponseUi { glyph, field }
}

/// Spawn one settings-floater button.
fn spawn_config_button(commands: &mut Commands, parent: Entity, button: ConfigButton) {
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                align_self: AlignSelf::Start,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            button,
            Name::new("contact-set-config-button"),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(match button {
                ConfigButton::Rename => "contact-set-config-rename",
                ConfigButton::Close => "contact-set-config-close",
            }),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            ClassList::new_with_classes([BUTTON_LABEL_CLASS]),
            Pickable::IGNORE,
        ))
        .observe(on_config_button_press);
}

// --- View -----------------------------------------------------------------

/// Mirror the filter field's text into the view state.
fn mirror_contact_sets_filter(
    ui: Option<Res<ContactSetsUi>>,
    fields: Query<&EditableText>,
    mut view: ResMut<ContactSetsView>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(field) = fields.get(ui.filter_field) else {
        return;
    };
    let term = field.value().to_string();
    if view.filter != term {
        view.filter = term;
    }
}

/// Keep the set chooser's options in step with the sets, and the chosen set
/// pointing at one that still exists (a deleted set falls back to *All Sets*).
fn sync_chooser_options(
    sets: Res<ContactSets>,
    ui: Option<Res<ContactSetsUi>>,
    mut view: ResMut<ContactSetsView>,
    mut selections: Query<&mut ComboSelection>,
    mut options: MessageWriter<SetComboOptions>,
) {
    let Some(ui) = ui else {
        return;
    };
    let wanted = chooser_options(&sets);
    let stale_choice =
        view.choice.is_empty() || !wanted.iter().any(|option| *option == view.choice);
    if view.built_options == wanted && !stale_choice {
        // The options are current; only the choice may have moved (the New Set
        // prompt switches to the set it just made), so keep the combo on it.
        select_combo_option(&mut selections, ui.chooser, &wanted, &view.choice);
        return;
    }
    if stale_choice {
        ALL_SETS_LABEL.clone_into(&mut view.choice);
    }
    if view.built_options != wanted {
        options.write(SetComboOptions {
            combo: ui.chooser,
            labels: wanted.clone(),
        });
    }
    select_combo_option(&mut selections, ui.chooser, &wanted, &view.choice);
    view.built_options = wanted;
}

/// Point `combo`'s selection at `label` within `options` — the programmatic
/// write the widget derives its closed value text from (it emits no
/// [`ComboChanged`], so this does not loop back through the pick handler).
fn select_combo_option(
    selections: &mut Query<&mut ComboSelection>,
    combo: Entity,
    options: &[String],
    label: &str,
) {
    let Some(active) = options.iter().position(|option| option == label) else {
        return;
    };
    if let Ok(mut selection) = selections.get_mut(combo)
        && selection.active != active
    {
        selection.active = active;
    }
}

/// Adopt a user pick in either combo: the panel's chooser switches the list, the
/// add floater's remembers which set the target would be filed under.
fn handle_chooser_picks(
    mut picks: MessageReader<ComboChanged>,
    ui: Option<Res<ContactSetsUi>>,
    add_ui: Option<Res<AddToSetUi>>,
    mut view: ResMut<ContactSetsView>,
    mut target: ResMut<AddToSetTarget>,
) {
    for pick in picks.read() {
        if let Some(ui) = ui.as_deref()
            && pick.combo == ui.chooser
            && let Some(label) = view.built_options.get(pick.active).cloned()
        {
            view.choice = label;
        }
        if let Some(add_ui) = add_ui.as_deref()
            && pick.combo == add_ui.chooser
            && let Some(label) = add_ui.options.get(pick.active).cloned()
        {
            target.chosen = label;
        }
    }
}

/// Rebuild the ordered, filtered member rows when the sets, the friends roster,
/// the chosen set, the sort or the filter moved, and refresh the count line.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the two models the rows are read from, the panel UI, the \
              translator for the count line, and the view / table / list / text state it \
              writes"
)]
fn rebuild_contact_sets_view(
    sets: Res<ContactSets>,
    friends: Res<FriendsModel>,
    ui: Option<Res<ContactSetsUi>>,
    translator: Translator,
    mut view: ResMut<ContactSetsView>,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let sort = tables
        .get(ui.table)
        .ok()
        .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
    let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
    if view.built_revision == sets.revision()
        && view.built_friends_revision == friends.revision()
        && view.built_sort_revision == sort_revision
        && view.built_filter == view.filter
        && view.built_choice == view.choice
    {
        return;
    }
    // Reborrow so the built-stamp writes and the filter / choice reads are
    // disjoint field borrows rather than repeated whole-resource borrows.
    let view = &mut *view;
    view.built_revision = sets.revision();
    view.built_friends_revision = friends.revision();
    view.built_sort_revision = sort_revision;
    view.built_filter.clone_from(&view.filter);
    view.built_choice.clone_from(&view.choice);

    let total = build_rows(&sets, &friends, &view.choice, &mut view.rows, &view.filter);
    let keys: Vec<(&str, bool)> = sort
        .map(|(_revision, keys)| keys)
        .unwrap_or_default()
        .iter()
        .filter_map(|key| {
            CONTACT_SETS_TABLE
                .columns
                .get(key.column)
                .map(|column| (column.token, key.ascending))
        })
        .collect();
    // Only a real set carries the flag; the pseudo-sets are views, not sets.
    let online_first = sets
        .set(&view.choice)
        .is_some_and(ContactSet::sorts_by_online_status);
    sort_rows(&mut view.rows, &keys, online_first);

    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = view.rows.len();
    }
    let label = translator.format(
        "contact-sets-count",
        &TransArgs::new()
            .int("shown", i64::try_from(view.rows.len()).unwrap_or(i64::MAX))
            .int("total", i64::try_from(total).unwrap_or(i64::MAX)),
    );
    if let Ok(mut text) = texts.get_mut(ui.count_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Fill `rows` with the members the chosen set shows, filtered by `filter`, and
/// return how many there were before filtering.
///
/// *All Sets* is everyone filed anywhere; *No Sets* is every friend filed
/// nowhere (the reference's own reading — it is the list of people still to
/// file); a real set is its own members.
fn build_rows(
    sets: &ContactSets,
    friends: &FriendsModel,
    choice: &str,
    rows: &mut Vec<MemberRow>,
    filter: &str,
) -> usize {
    // One pass over the roster: it labels the friends the sets have never seen a
    // name for, and is the *No Sets* view's whole population.
    let roster: std::collections::HashMap<AgentKey, String> =
        friends.roster().into_iter().collect();
    let agents: Vec<AgentKey> = if choice == ALL_SETS_LABEL {
        sets.everyone_filed()
    } else if choice == PSEUDONYMS_KEY {
        sets.everyone_aliased()
    } else if choice == NO_SETS_LABEL {
        let mut unfiled: Vec<AgentKey> = roster
            .keys()
            .copied()
            .filter(|agent| !sets.is_filed(*agent))
            .collect();
        unfiled.sort_unstable_by_key(AgentKey::uuid);
        unfiled
    } else {
        sets.set(choice)
            .map(|set| set.members().collect())
            .unwrap_or_default()
    };
    let total = agents.len();
    rows.clear();
    rows.extend(agents.into_iter().filter_map(|agent| {
        // The **shown** label: an aliased person is listed under the name the
        // user gave them, quoted, exactly as the rest of the viewer now shows
        // them ([`crate::contact_sets`]'s name-cache hook).
        let name = sets
            .shown_label_of(agent)
            .or_else(|| roster.get(&agent).cloned())
            .unwrap_or_else(|| short_id(agent));
        matches_filter(&name, filter).then(|| MemberRow {
            agent,
            sets: sets.sets_of(agent).join(", "),
            color: sets.color_of(agent),
            online: friends.is_online(agent),
            name,
        })
    }));
    total
}

/// Build the widget cells of each freshly-pooled row and attach its press
/// observer.
fn populate_member_rows(
    mut commands: Commands,
    ui: Option<Res<ContactSetsUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.table, &CONTACT_SETS_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundMember(None))
            .observe(on_member_row_press);
    }
}

/// Bind each pooled row to the member it now presents: the name (tinted with
/// that person's set colour) and sets cells, and the selection highlight.
fn bind_member_rows(
    view: Res<ContactSetsView>,
    selected: Res<SelectedMember>,
    ui: Option<Res<ContactSetsUi>>,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundMember,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let data = row.index.and_then(|index| view.rows.get(index));
        bound.0 = data.map(|member| member.agent);
        if let Some(cell) = cells.cell(COL_NAME) {
            let (name, color) = data.map_or((String::new(), LABEL_COLOR), |member| {
                (member.name.clone(), member.color.unwrap_or(LABEL_COLOR))
            });
            set_table_cell(&mut texts, cell, &name, color);
        }
        if let Some(cell) = cells.cell(COL_SETS) {
            let sets = data.map(|member| member.sets.clone()).unwrap_or_default();
            set_table_cell(&mut texts, cell, &sets, DIM_LABEL_COLOR);
        }
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if data.is_some() && selected.0 == bound.0 {
                SELECTED_BACKGROUND
            } else {
                Color::NONE
            };
            if background.0 != wanted {
                background.0 = wanted;
            }
        }
    }
}

// --- Interaction ----------------------------------------------------------

/// A press on a pooled row selects that member.
fn on_member_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&BoundMember>,
    ui: Res<ContactSetsUi>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedMember>,
) {
    let Ok(BoundMember(Some(agent))) = rows.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    focus.set(ui.viewport, FocusCause::Navigated);
    selected.0 = Some(agent);
}

/// A press on one of the panel's buttons.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the button pool, the \
              sets and the view / selection the actions read, and the channels the twelve \
              buttons write (prompts, picker, floaters, aliases, requests, profile, IM, \
              teleport)"
)]
fn on_panel_button_press(
    mut press: On<Pointer<Press>>,
    buttons: Query<&ContactSetsButton>,
    sets: Res<ContactSets>,
    view: Res<ContactSetsView>,
    selected: Res<SelectedMember>,
    mut pending: ResMut<PendingAction>,
    mut notifications: MessageWriter<ShowNotification>,
    mut pickers: MessageWriter<OpenAvatarPicker>,
    mut adds: MessageWriter<OpenAddToContactSet>,
    mut aliases: MessageWriter<OpenSetPseudonym>,
    mut requests: MessageWriter<RequestContactSet>,
    mut config: ResMut<ConfigTarget>,
    mut profiles: MessageWriter<OpenAvatarProfile>,
    mut conversations: MessageWriter<OpenConversation>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    press.propagate(false);
    // The greyed buttons are inert. `InteractionDisabled` is advisory for a
    // hand-rolled button, so the same predicate that greys them decides here —
    // one source of truth, and no way for the look and the behaviour to drift.
    if !button.is_enabled(&sets, &view.choice, selected.0) {
        return;
    }
    let real_set = is_real_set(&sets, &view.choice).then(|| view.choice.clone());
    match button {
        ContactSetsButton::NewSet => {
            *pending = PendingAction::Create {
                then_add: Vec::new(),
                move_from: None,
            };
            notifications.write(ShowNotification::new("AddNewContactSet"));
        }
        ContactSetsButton::DeleteSet => {
            let Some(name) = real_set else {
                return;
            };
            *pending = PendingAction::RemoveSet { name: name.clone() };
            notifications
                .write(ShowNotification::new("RemoveContactSet").arg("SET_NAME", name.clone()));
        }
        ContactSetsButton::Configure => {
            if let Some(name) = real_set {
                config.0 = Some(name);
            }
        }
        ContactSetsButton::AddResident => {
            if real_set.is_some() {
                // The reference's Add Avatar picker is a multi-picker: a set is
                // exactly the sort of thing one files several people into at
                // once.
                pickers.write(OpenAvatarPicker::many(PICKER_REQUESTER));
            }
        }
        ContactSetsButton::MoveMember => {
            if let (Some(name), Some(agent)) = (real_set, selected.0) {
                adds.write(
                    OpenAddToContactSet::one(
                        agent,
                        sets.label_of(agent)
                            .map(ToOwned::to_owned)
                            .unwrap_or_default(),
                    )
                    .moving_from(name),
                );
            }
        }
        ContactSetsButton::RemoveMember => {
            let (Some(set), Some(agent)) = (real_set, selected.0) else {
                return;
            };
            let name = sets
                .label_of(agent)
                .map_or_else(|| short_id(agent), ToOwned::to_owned);
            *pending = PendingAction::RemoveMember {
                set: set.clone(),
                agent,
            };
            notifications.write(
                ShowNotification::new("RemoveContactFromSet")
                    .arg("TARGET", name)
                    .arg("SET_NAME", set),
            );
        }
        ContactSetsButton::Profile => {
            if let Some(agent) = selected.0 {
                profiles.write(OpenAvatarProfile { agent });
            }
        }
        ContactSetsButton::Im => {
            if let Some(agent) = selected.0 {
                conversations.write(OpenConversation {
                    key: ConversationKey::Direct(agent),
                });
            }
        }
        ContactSetsButton::OfferTeleport => {
            if let Some(agent) = selected.0 {
                sl_commands.write(SlCommand(Command::OfferTeleport {
                    targets: vec![agent],
                    message: String::new(),
                }));
            }
        }
        ContactSetsButton::SetAlias => {
            if let Some(agent) = selected.0 {
                aliases.write(OpenSetPseudonym {
                    agent,
                    name: sets
                        .label_of(agent)
                        .map_or_else(|| short_id(agent), ToOwned::to_owned),
                });
            }
        }
        ContactSetsButton::ClearAlias => {
            // Nothing to clear is nothing to do — as with Delete Set on a
            // pseudo-set, the button is simply inert until it applies.
            if let Some(agent) = selected.0
                && sets.has_alias(agent)
            {
                requests.write(RequestContactSet::ClearPseudonym { agent });
            }
        }
        ContactSetsButton::RemoveDisplayName => {
            if let Some(agent) = selected.0
                && !sets.has_display_name_removed(agent)
            {
                requests.write(RequestContactSet::RemoveDisplayName {
                    agent,
                    name: sets
                        .label_of(agent)
                        .map(ToOwned::to_owned)
                        .unwrap_or_default(),
                });
            }
        }
    }
}

/// Open the add-to-set floater for a resident (the avatar pie's **Add to Set**,
/// or the panel's **Move to Set…**), seeding its combo with the sets there are.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the request stream, the sets it seeds the combo from, the \
              floater's UI / target / translator, and the panel / text / combo / prompt \
              channels one open writes"
)]
fn handle_open_add_to_set(
    mut requests: MessageReader<OpenAddToContactSet>,
    sets: Res<ContactSets>,
    ui: Option<ResMut<AddToSetUi>>,
    translator: Translator,
    mut target: ResMut<AddToSetTarget>,
    mut panels: Query<&mut UiPanelShown>,
    mut texts: Query<&mut Text>,
    mut selections: Query<&mut ComboSelection>,
    mut options: MessageWriter<SetComboOptions>,
    mut notifications: MessageWriter<ShowNotification>,
    mut pending: ResMut<PendingAction>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    for request in requests.read() {
        // A resident whose name the opening surface did not know is filed under
        // a short form of their key rather than an empty line.
        let residents: Vec<(AgentKey, String)> = request
            .agents
            .iter()
            .map(|(agent, name)| {
                let label = if name.is_empty() {
                    short_id(*agent)
                } else {
                    name.clone()
                };
                (*agent, label)
            })
            .collect();
        if residents.is_empty() {
            continue;
        }
        // With no set to file them under, the useful thing is the make-a-set
        // prompt rather than a floater whose only control is empty.
        if sets.set_count() == 0 {
            *pending = PendingAction::Create {
                then_add: residents,
                move_from: request.move_from.clone(),
            };
            notifications.write(ShowNotification::new("AddNewContactSet"));
            continue;
        }
        target.agents = residents;
        target.move_from.clone_from(&request.move_from);

        let labels: Vec<String> = sets.sets().map(|set| set.name().to_owned()).collect();
        let active = request
            .move_from
            .as_ref()
            .and_then(|from| labels.iter().position(|name| name == from))
            .unwrap_or_default();
        target.chosen = labels.get(active).cloned().unwrap_or_default();
        options.write(SetComboOptions {
            combo: ui.chooser,
            labels: labels.clone(),
        });
        select_combo_option(&mut selections, ui.chooser, &labels, &target.chosen);
        ui.options = labels;

        // The reference's own split: one resident is named in the prompt, several
        // are counted (the names are on the lines the user just picked from).
        let prompt = match target.single() {
            Some((_agent, name)) => translator.format(
                if request.move_from.is_some() {
                    "move-to-contact-set-prompt"
                } else {
                    "add-to-contact-set-prompt"
                },
                &TransArgs::new().text("name", name),
            ),
            None => translator.format(
                "add-to-contact-set-prompt-multiple",
                &TransArgs::new().int(
                    "count",
                    i64::try_from(target.agents.len()).unwrap_or(i64::MAX),
                ),
            ),
        };
        if let Ok(mut text) = texts.get_mut(ui.prompt)
            && text.0 != prompt
        {
            text.0 = prompt;
        }
        if let Ok(mut shown) = panels.get_mut(ui.panel) {
            shown.0 = true;
        }
    }
}

/// Raise the reference's `SetAvatarPseudonym` prompt for one resident, and
/// remember who it is about until it is answered. Only the **last** request of a
/// frame stands: the prompt has one text field, so two pending aliases would
/// give the second one's answer to the first.
fn handle_open_set_pseudonym(
    mut requests: MessageReader<OpenSetPseudonym>,
    mut pending: ResMut<PendingAction>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for request in requests.read() {
        let label = if request.name.is_empty() {
            short_id(request.agent)
        } else {
            request.name.clone()
        };
        *pending = PendingAction::SetAlias {
            agent: request.agent,
            name: label.clone(),
        };
        notifications.write(ShowNotification::new("SetAvatarPseudonym").arg("AVATAR", label));
    }
}

/// A press on the add-to-set floater's buttons: **Add** files the target (and,
/// in move mode, unfiles them from where they were), **New Set…** prompts for a
/// set to file them under, **Cancel** just closes.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the button pool, the \
              floater UI and its target, and the request / prompt / panel channels the three \
              buttons write"
)]
fn on_add_to_set_press(
    mut press: On<Pointer<Press>>,
    buttons: Query<&AddToSetButton>,
    ui: Option<Res<AddToSetUi>>,
    mut target: ResMut<AddToSetTarget>,
    mut pending: ResMut<PendingAction>,
    mut requests: MessageWriter<RequestContactSet>,
    mut notifications: MessageWriter<ShowNotification>,
    mut panels: Query<&mut UiPanelShown>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    press.propagate(false);
    if target.agents.is_empty() {
        return;
    }
    match button {
        AddToSetButton::Add => {
            if target.chosen.is_empty() {
                return;
            }
            for (agent, name) in target.agents.clone() {
                match target.move_from.clone() {
                    Some(from) => requests.write(RequestContactSet::Move {
                        from,
                        to: target.chosen.clone(),
                        agent,
                    }),
                    None => requests.write(RequestContactSet::Add {
                        set: target.chosen.clone(),
                        agent,
                        name,
                    }),
                };
            }
            notifications.write(add_success_notification(&target));
        }
        AddToSetButton::NewSet => {
            *pending = PendingAction::Create {
                then_add: target.agents.clone(),
                move_from: target.move_from.clone(),
            };
            notifications.write(ShowNotification::new("AddNewContactSet"));
        }
        AddToSetButton::Cancel => {}
    }
    target.agents.clear();
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = false;
    }
}

/// What the floater says once it has filed its residents.
///
/// The reference says it two ways, because a list of eight names is not a
/// sentence: one resident is named, several are counted.
fn add_success_notification(target: &AddToSetTarget) -> ShowNotification {
    match target.single() {
        Some((_agent, name)) => ShowNotification::new("AddToContactSetSingleSuccess")
            .arg("NAME", name.clone())
            .arg("SET", target.chosen.clone()),
        None => ShowNotification::new("AddToContactSetMultipleSuccess")
            .arg("COUNT", target.agents.len().to_string())
            .arg("SET", target.chosen.clone()),
    }
}

/// A press on the settings floater's buttons: **Rename** asks for the rename
/// (the model refuses a name that is taken, and says so through the reference's
/// notification), **Close** shuts the floater.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the button pool, the \
              floater UI / target / name field the rename reads, and the request / pending / \
              panel state it writes"
)]
fn on_config_button_press(
    mut press: On<Pointer<Press>>,
    buttons: Query<&ConfigButton>,
    ui: Option<Res<ConfigUi>>,
    fields: Query<&EditableText>,
    mut target: ResMut<ConfigTarget>,
    mut pending_rename: ResMut<PendingRename>,
    mut requests: MessageWriter<RequestContactSet>,
    mut panels: Query<&mut UiPanelShown>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity).copied() else {
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    press.propagate(false);
    match button {
        ConfigButton::Rename => {
            let Some(from) = target.0.clone() else {
                return;
            };
            let to = fields
                .get(ui.name_field)
                .map(|field| field.value().to_string())
                .unwrap_or_default();
            if to.trim().is_empty() {
                return;
            }
            let to = to.trim().to_owned();
            pending_rename.0 = Some(to.clone());
            requests.write(RequestContactSet::Rename { from, to });
        }
        ConfigButton::Close => {
            target.0 = None;
            if let Ok(mut shown) = panels.get_mut(ui.panel) {
                shown.0 = false;
            }
        }
    }
}

/// Grey every action button whose action does not apply to what the panel is
/// showing, and mark it [`InteractionDisabled`].
///
/// The greying is the skin's, not ours: each button and label carries a base
/// class (`.sk-button` / `.sk-text`) and gains a disabled one on top, so a skin
/// decides what "greyed" looks like and dropping the class falls back to the
/// base rule. `InteractionDisabled` is the state marker beside it — advisory for
/// our hand-rolled buttons, which is why [`on_panel_button_press`] asks
/// [`ContactSetsButton::is_enabled`] itself rather than trusting the marker.
fn sync_panel_button_states(
    sets: Res<ContactSets>,
    view: Res<ContactSetsView>,
    selected: Res<SelectedMember>,
    buttons: Query<(
        Entity,
        &ContactSetsButton,
        &PanelButtonLabel,
        Has<InteractionDisabled>,
    )>,
    mut classes: Query<&mut ClassList>,
    mut commands: Commands,
) {
    for (entity, button, label, was_disabled) in &buttons {
        let enabled = button.is_enabled(&sets, &view.choice, selected.0);
        set_skin_class(&mut classes, entity, DISABLED_SURFACE_CLASS, !enabled);
        set_skin_class(&mut classes, label.0, DISABLED_TEXT_CLASS, !enabled);
        if enabled && was_disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !was_disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

/// Add or drop one skin class on a node, writing only on a real change so an
/// idle panel does not re-trigger the style pass.
fn set_skin_class(
    classes: &mut Query<&mut ClassList>,
    node: Entity,
    class: &'static str,
    wanted: bool,
) {
    let Ok(mut list) = classes.get_mut(node) else {
        return;
    };
    if wanted {
        if !list.contains(class) {
            list.add(class);
        }
    } else if list.contains(class) {
        list.remove(class);
    }
}

/// A press on one of the settings floater's checkboxes: flip that behaviour on
/// the set the floater is showing. The reply toggles carry the field's current
/// text with them, so turning an override on answers with what is already typed
/// rather than with nothing.
fn on_config_toggle_press(
    activate: On<Activate>,
    toggles: Query<&ConfigToggle>,
    ui: Option<Res<ConfigUi>>,
    sets: Res<ContactSets>,
    fields: Query<&EditableText>,
    target: Res<ConfigTarget>,
    mut requests: MessageWriter<RequestContactSet>,
) {
    let Ok(toggle) = toggles.get(activate.entity).copied() else {
        return;
    };
    let (Some(ui), Some(name)) = (ui, target.0.clone()) else {
        return;
    };
    let Some(set) = sets.set(&name) else {
        return;
    };
    match toggle {
        ConfigToggle::Notify => requests.write(RequestContactSet::SetNotify {
            name,
            notify: !set.notify(),
        }),
        ConfigToggle::SortByOnlineStatus => {
            requests.write(RequestContactSet::SetSortByOnlineStatus {
                name,
                sort: !set.sorts_by_online_status(),
            })
        }
        ConfigToggle::Autoresponse(mode) => {
            let text = fields
                .get(ui.autoresponse(mode).field)
                .map(|field| field.value().to_string())
                .unwrap_or_default();
            requests.write(RequestContactSet::SetAutoresponse {
                name,
                mode,
                enabled: !set.autoresponse(mode).enabled(),
                text,
            })
        }
    };
}

/// Write the reply fields back to the set they belong to, on the two edges that
/// mean the user is done with one: the field losing focus (the reference commits
/// on focus lost too) and the floater turning to another set or closing — the
/// latter committing against the set the fields were *seeded* from, since
/// [`sync_config_floater`] has not re-seeded them yet.
fn commit_config_autoresponses(
    ui: Option<Res<ConfigUi>>,
    sets: Res<ContactSets>,
    target: Res<ConfigTarget>,
    focus: Res<InputFocus>,
    fields: Query<&EditableText>,
    mut requests: MessageWriter<RequestContactSet>,
    mut seeded_for: Local<Option<String>>,
) {
    let Some(ui) = ui else {
        return;
    };
    let switching = *seeded_for != target.0;
    let Some(name) = seeded_for.clone() else {
        seeded_for.clone_from(&target.0);
        return;
    };
    if let Some(set) = sets.set(&name) {
        for mode in AUTORESPONSE_MODES {
            let block = ui.autoresponse(*mode);
            // While the field has focus the user is still typing in it, so only
            // a switch away from this set forces the write.
            if !switching && focus.get() == Some(block.field) {
                continue;
            }
            let Ok(field) = fields.get(block.field) else {
                continue;
            };
            let text = field.value().to_string();
            let stored = set.autoresponse(*mode);
            if stored.text() != text {
                requests.write(RequestContactSet::SetAutoresponse {
                    name: name.clone(),
                    mode: *mode,
                    enabled: stored.enabled(),
                    text,
                });
            }
        }
    }
    if switching {
        seeded_for.clone_from(&target.0);
    }
}

/// Show / hide the settings floater with its target, and keep its title, name
/// field and swatch showing that set.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the sets and the target it follows, the floater's UI, and the \
              panel / title / swatch / name-field state it writes (the field needing parley's \
              two contexts to be set programmatically)"
)]
fn sync_config_floater(
    sets: Res<ContactSets>,
    ui: Option<Res<ConfigUi>>,
    mut target: ResMut<ConfigTarget>,
    translator: Translator,
    mut panels: Query<&mut UiPanelShown>,
    mut texts: Query<&mut Text>,
    mut swatches: Query<(&mut ColorSwatchValue, &mut BackgroundColor)>,
    mut editors: Query<&mut EditableText>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
    mut shown_for: Local<Option<String>>,
) {
    let Some(ui) = ui else {
        return;
    };
    // A set that went away (deleted from the panel behind the floater) takes the
    // floater with it.
    if target
        .0
        .as_ref()
        .is_some_and(|name| sets.set(name).is_none())
    {
        target.0 = None;
    }
    let Ok(mut shown) = panels.get_mut(ui.panel) else {
        return;
    };
    let Some(name) = target.0.clone() else {
        if shown.0 {
            shown.0 = false;
        }
        *shown_for = None;
        return;
    };
    // The floater's own close button only hides it, so a hidden floater that was
    // showing this set is the user having closed it — let go of the target
    // rather than re-opening it under them.
    if !shown.0 && shown_for.as_deref() == Some(name.as_str()) {
        target.0 = None;
        *shown_for = None;
        return;
    }
    if !shown.0 {
        shown.0 = true;
    }
    let Some(set) = sets.set(&name) else {
        return;
    };
    // The swatch follows the set on every change (a recolour lands here too);
    // the name field is seeded only when the floater turns to a new set, so a
    // half-typed rename is not overwritten under the user's hands.
    if let Ok((mut value, mut background)) = swatches.get_mut(ui.swatch) {
        if value.0 != set.color() {
            value.0 = set.color();
        }
        if background.0 != set.color() {
            background.0 = set.color();
        }
    }
    // The five checkboxes follow the set on every change too — each is flipped
    // through the model, so this is what actually draws the new state.
    set_config_check(&mut texts, ui.notify_glyph, set.notify());
    set_config_check(&mut texts, ui.sort_glyph, set.sorts_by_online_status());
    for mode in AUTORESPONSE_MODES {
        set_config_check(
            &mut texts,
            ui.autoresponse(*mode).glyph,
            set.autoresponse(*mode).enabled(),
        );
    }
    if shown_for.as_deref() == Some(name.as_str()) {
        return;
    }
    *shown_for = Some(name.clone());
    if let Ok(mut editor) = editors.get_mut(ui.name_field) {
        crate::web_floater::set_editor_text(&mut editor, &name, &mut font_cx, &mut layout_cx);
    }
    // The reply fields are seeded on the same edge as the name field, for the
    // same reason: they are edited in place, and re-seeding them every frame
    // would fight the user's typing.
    for mode in AUTORESPONSE_MODES {
        let text = set.autoresponse(*mode).text().to_owned();
        if let Ok(mut editor) = editors.get_mut(ui.autoresponse(*mode).field) {
            crate::web_floater::set_editor_text(&mut editor, &text, &mut font_cx, &mut layout_cx);
        }
    }
    let title = translator.format(
        "contact-set-config-title",
        &TransArgs::new().text("name", &name),
    );
    if let Ok(mut text) = texts.get_mut(ui.title)
        && text.0 != title
    {
        text.0 = title;
    }
}

/// Draw one settings-floater checkbox in its checked / unchecked state.
fn set_config_check(texts: &mut Query<&mut Text>, node: Entity, checked: bool) {
    let glyph = if checked {
        CHECKED_GLYPH
    } else {
        UNCHECKED_GLYPH
    };
    if let Ok(mut text) = texts.get_mut(node)
        && text.0 != glyph
    {
        glyph.clone_into(&mut text.0);
    }
}

/// Point the settings floater at the renamed set once the model has taken the
/// rename — and leave it on the set it was already showing when the model
/// refused (the reference's rename-failure notification says why).
fn settle_contact_set_rename(
    sets: Res<ContactSets>,
    mut pending: ResMut<PendingRename>,
    mut target: ResMut<ConfigTarget>,
    mut view: ResMut<ContactSetsView>,
) {
    let Some(wanted) = pending.0.take() else {
        return;
    };
    if sets.set(&wanted).is_none() {
        return;
    }
    // The list is showing the set that was renamed, so it follows it too.
    if target.0.as_deref() == Some(view.choice.as_str()) {
        view.choice.clone_from(&wanted);
    }
    target.0 = Some(wanted);
}

/// File the residents chosen in the shared avatar picker under the chosen set.
fn handle_contact_set_picks(
    mut picks: MessageReader<AvatarPicked>,
    view: Res<ContactSetsView>,
    sets: Res<ContactSets>,
    mut requests: MessageWriter<RequestContactSet>,
) {
    for pick in picks.read() {
        if pick.requester != PICKER_REQUESTER {
            continue;
        }
        if !is_real_set(&sets, &view.choice) {
            continue;
        }
        for chosen in &pick.picks {
            requests.write(RequestContactSet::Add {
                set: view.choice.clone(),
                agent: chosen.agent,
                name: chosen.name.clone(),
            });
        }
    }
}

/// Recolour the settings floater's set from its swatch's picker.
fn handle_contact_set_colors(
    mut picks: MessageReader<ColorPicked>,
    ui: Option<Res<ConfigUi>>,
    target: Res<ConfigTarget>,
    mut requests: MessageWriter<RequestContactSet>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        // Only the committed pick: a live drag would rewrite the store on every
        // frame of the drag for a colour the user has not settled on.
        if pick.requester != ui.swatch || !pick.final_pick {
            continue;
        }
        let Some(name) = target.0.clone() else {
            continue;
        };
        requests.write(RequestContactSet::Recolor {
            name,
            color: pick.color,
        });
    }
}

/// Apply the answers to the prompts and confirmations the panel raises.
fn handle_contact_set_notifications(
    mut responses: MessageReader<NotificationResponse>,
    mut pending: ResMut<PendingAction>,
    mut requests: MessageWriter<RequestContactSet>,
    mut view: ResMut<ContactSetsView>,
) {
    for response in responses.read() {
        match response.template {
            "AddNewContactSet" => {
                let taken = core::mem::take(&mut *pending);
                let PendingAction::Create {
                    then_add,
                    move_from,
                } = taken
                else {
                    continue;
                };
                if response.button != Some("Create") {
                    continue;
                }
                let name = response
                    .input
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_owned();
                if name.is_empty() {
                    continue;
                }
                requests.write(RequestContactSet::Create { name: name.clone() });
                for (agent, label) in then_add {
                    match move_from.clone() {
                        Some(from) => requests.write(RequestContactSet::Move {
                            from,
                            to: name.clone(),
                            agent,
                        }),
                        None => requests.write(RequestContactSet::Add {
                            set: name.clone(),
                            agent,
                            name: label,
                        }),
                    };
                }
                // Show the set that was just made: it is what the user is
                // working on, and an empty new set is otherwise invisible.
                view.choice = name;
            }
            "RemoveContactSet" => {
                let taken = core::mem::take(&mut *pending);
                let PendingAction::RemoveSet { name } = taken else {
                    continue;
                };
                if response.button == Some("OK") {
                    requests.write(RequestContactSet::Remove { name });
                }
            }
            "SetAvatarPseudonym" => {
                let taken = core::mem::take(&mut *pending);
                let PendingAction::SetAlias { agent, name } = taken else {
                    continue;
                };
                if response.button != Some("Create") {
                    continue;
                }
                let alias = response
                    .input
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_owned();
                if alias.is_empty() {
                    continue;
                }
                // The list is not switched to the Pseudonyms pseudo-set: the
                // aliased person is renamed in place wherever they already show,
                // which is the feedback the action wants — and the prompt is
                // just as often raised from the avatar pie, with no panel open.
                requests.write(RequestContactSet::SetPseudonym { agent, alias, name });
            }
            "RemoveContactFromSet" => {
                let taken = core::mem::take(&mut *pending);
                let PendingAction::RemoveMember { set, agent } = taken else {
                    continue;
                };
                if response.button == Some("OK") {
                    requests.write(RequestContactSet::RemoveMember { set, agent });
                }
            }
            _other => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AddToSetTarget, MemberRow, add_success_notification, chooser_options, is_real_set,
        matches_filter, short_id, sort_rows,
    };
    use crate::contact_sets::{
        ALL_SETS_LABEL, ContactSetRefusal, ContactSets, NO_SETS_LABEL, PSEUDONYMS_KEY,
    };
    use bevy::prelude::Color;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, Uuid};

    /// An agent key from a small integer.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(Uuid::from_u128(id))
    }

    /// A row named `name` in `sets`, offline.
    fn row(name: &str, sets: &str) -> MemberRow {
        online_row(name, sets, false)
    }

    /// A row named `name` in `sets`, with an explicit presence.
    fn online_row(name: &str, sets: &str, online: bool) -> MemberRow {
        MemberRow {
            agent: agent(1),
            name: name.to_owned(),
            sets: sets.to_owned(),
            color: None,
            online,
        }
    }

    /// The filter is a case-insensitive substring, and an empty filter keeps
    /// everything.
    #[test]
    fn filter_is_case_insensitive() {
        assert!(matches_filter("Alpha Resident", ""));
        assert!(matches_filter("Alpha Resident", "  "));
        assert!(matches_filter("Alpha Resident", "resid"));
        assert!(!matches_filter("Alpha Resident", "beta"));
    }

    /// Name sorting is case-insensitive and honours the direction; the sets sort
    /// falls back to the name for a tie.
    #[test]
    fn sorting_by_name_and_sets() {
        let mut rows = vec![
            row("beta", "Builders"),
            row("Alpha", "Zulu"),
            row("gamma", "Builders"),
        ];
        sort_rows(&mut rows, &[("name", true)], false);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "gamma"]);

        sort_rows(&mut rows, &[("name", false)], false);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["gamma", "beta", "Alpha"]);

        sort_rows(&mut rows, &[("sets", true)], false);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(
            names,
            ["beta", "gamma", "Alpha"],
            "the two Builders rows tie and fall back to the name"
        );
    }

    /// A set that sorts by online status puts the online first, whatever the
    /// table's own keys say — and falls through to those keys within each group
    /// (`viewer-contact-set-presence-extras`).
    #[test]
    fn sorting_by_online_status_leads() {
        let mut rows = vec![
            online_row("beta", "Builders", false),
            online_row("Alpha", "Builders", false),
            online_row("gamma", "Builders", true),
            online_row("Delta", "Builders", true),
        ];
        sort_rows(&mut rows, &[("name", true)], true);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(
            names,
            ["Delta", "gamma", "Alpha", "beta"],
            "online first, then the name key within each group"
        );

        // The same rows, with the set's flag off, are the plain name order.
        sort_rows(&mut rows, &[("name", true)], false);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "Delta", "gamma"]);
    }

    /// Which buttons apply to which state of the panel: the pseudo-sets are
    /// views, so nothing that changes *a set* applies to them; nothing that acts
    /// on a member applies with none selected; and the two alias buttons need an
    /// alias to drop, or a display name still to suppress.
    #[test]
    fn buttons_grey_out_when_they_do_not_apply() -> Result<(), ContactSetRefusal> {
        use super::ContactSetsButton as Button;

        // A free function rather than a closure over `sets`, so the alias cases
        // below can still mutate it between assertions.
        fn enabled(
            sets: &ContactSets,
            button: Button,
            choice: &str,
            who: Option<AgentKey>,
        ) -> bool {
            button.is_enabled(sets, choice, who)
        }

        let mut sets = ContactSets::default();
        sets.create_set("Builders")?;
        sets.add_member("Builders", agent(1), "Alpha Resident")?;

        // New Set… is the only button that always applies — it is how the first
        // set comes into being.
        for choice in [ALL_SETS_LABEL, NO_SETS_LABEL, PSEUDONYMS_KEY, "Builders"] {
            assert!(
                enabled(&sets, Button::NewSet, choice, None),
                "New Set… applies with {choice} chosen"
            );
        }
        // The set-changing buttons need a real set.
        for button in [Button::DeleteSet, Button::Configure, Button::AddResident] {
            assert!(
                enabled(&sets, button, "Builders", None),
                "{button:?} on a real set"
            );
            for choice in [ALL_SETS_LABEL, NO_SETS_LABEL, PSEUDONYMS_KEY] {
                assert!(
                    !enabled(&sets, button, choice, None),
                    "{button:?} is greyed on the {choice} pseudo-set"
                );
            }
        }
        // The member buttons need a selection; two of them need a real set too.
        for button in [Button::MoveMember, Button::RemoveMember] {
            assert!(
                !enabled(&sets, button, "Builders", None),
                "{button:?} needs a member"
            );
            assert!(enabled(&sets, button, "Builders", Some(agent(1))));
            assert!(
                !enabled(&sets, button, ALL_SETS_LABEL, Some(agent(1))),
                "{button:?} has no set to move or remove from"
            );
        }
        for button in [
            Button::Profile,
            Button::Im,
            Button::OfferTeleport,
            Button::SetAlias,
        ] {
            assert!(
                !enabled(&sets, button, ALL_SETS_LABEL, None),
                "{button:?} needs a member"
            );
            assert!(
                enabled(&sets, button, ALL_SETS_LABEL, Some(agent(1))),
                "{button:?} acts on the person, so a pseudo-set is fine"
            );
        }

        // Rem Alias… only once there is one; Rem DN… only while the display name
        // is not already suppressed.
        assert!(!enabled(
            &sets,
            Button::ClearAlias,
            "Builders",
            Some(agent(1))
        ));
        assert!(enabled(
            &sets,
            Button::RemoveDisplayName,
            "Builders",
            Some(agent(1))
        ));
        sets.set_pseudonym(agent(1), "Neighbour", "")?;
        assert!(enabled(
            &sets,
            Button::ClearAlias,
            "Builders",
            Some(agent(1))
        ));
        sets.remove_display_name(agent(1), "")?;
        assert!(
            !enabled(&sets, Button::RemoveDisplayName, "Builders", Some(agent(1))),
            "there is nothing left to suppress"
        );
        assert!(enabled(
            &sets,
            Button::ClearAlias,
            "Builders",
            Some(agent(1))
        ));
        Ok(())
    }

    /// The chooser lists the two pseudo-sets first, then the sets in name order,
    /// and only a real set counts as one the buttons may change.
    #[test]
    fn the_chooser_lists_the_pseudo_sets_first() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        assert_eq!(
            chooser_options(&sets),
            [
                ALL_SETS_LABEL.to_owned(),
                NO_SETS_LABEL.to_owned(),
                PSEUDONYMS_KEY.to_owned(),
            ],
            "the pseudo-sets work even with no sets at all"
        );
        sets.create_set("Zulu")?;
        sets.create_set("Builders")?;
        assert_eq!(
            chooser_options(&sets),
            [
                ALL_SETS_LABEL.to_owned(),
                NO_SETS_LABEL.to_owned(),
                PSEUDONYMS_KEY.to_owned(),
                "Builders".to_owned(),
                "Zulu".to_owned(),
            ]
        );
        assert!(is_real_set(&sets, "Builders"));
        assert!(!is_real_set(&sets, ALL_SETS_LABEL));
        assert!(!is_real_set(&sets, NO_SETS_LABEL));
        assert!(
            !is_real_set(&sets, PSEUDONYMS_KEY),
            "the alias listing is not a set the buttons may change"
        );
        Ok(())
    }

    /// A member with no resolved name is labelled by the head of their id, which
    /// is still filterable.
    #[test]
    fn an_unresolved_member_is_labelled_by_its_id() {
        let label = short_id(agent(1));
        assert_eq!(label, "00000000");
        assert!(matches_filter(&label, "0000"));
    }

    /// A row is tinted with the colour the model gives that person, so what the
    /// panel shows is what the tinting consumers will show.
    #[test]
    fn a_row_takes_the_model_colour() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        sets.create_set("Builders")?;
        sets.recolor_set("Builders", Color::srgb(1.0, 0.0, 0.0))?;
        sets.add_member("Builders", agent(1), "Alpha Resident")?;
        assert_eq!(sets.color_of(agent(1)), Some(Color::srgb(1.0, 0.0, 0.0)));
        assert_eq!(sets.color_of(agent(2)), None);
        Ok(())
    }

    /// Filing one resident names them; filing several counts them — the
    /// reference's two success notifications.
    #[test]
    fn the_success_notification_follows_the_count() {
        let one = AddToSetTarget {
            agents: vec![(agent(1), "Alpha Resident".to_owned())],
            move_from: None,
            chosen: "Builders".to_owned(),
        };
        let single = add_success_notification(&one);
        assert_eq!(single.template, "AddToContactSetSingleSuccess");
        assert_eq!(
            single.args.pairs(),
            [
                ("NAME".to_owned(), "Alpha Resident".to_owned()),
                ("SET".to_owned(), "Builders".to_owned()),
            ]
        );

        let several = AddToSetTarget {
            agents: vec![
                (agent(1), "Alpha Resident".to_owned()),
                (agent(2), "Beta Resident".to_owned()),
                (agent(3), "Gamma Resident".to_owned()),
            ],
            move_from: None,
            chosen: "Builders".to_owned(),
        };
        let multiple = add_success_notification(&several);
        assert_eq!(multiple.template, "AddToContactSetMultipleSuccess");
        assert_eq!(
            multiple.args.pairs(),
            [
                ("COUNT".to_owned(), "3".to_owned()),
                ("SET".to_owned(), "Builders".to_owned()),
            ]
        );
    }
}
