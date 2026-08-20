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
//! - The **set chooser** lists every set, above the two pseudo-sets the
//!   reference offers: *All Sets* (everyone filed anywhere) and *No Sets*
//!   (friends filed nowhere — the list to work through when starting out).
//! - **New Set…** raises the reference's own `AddNewContactSet` prompt;
//!   **Delete Set** its `RemoveContactSet` confirmation; **Configure…** opens the
//!   set's settings floater, where it is renamed and recoloured (the reference's
//!   `floater_fs_contact_set_configuration`, minus the notification / autoresponse
//!   knobs, which have no consumer here yet).
//! - **Add Resident…** files someone chosen in the shared
//!   [avatar picker](crate::avatar_picker); **Move to Set…** opens the
//!   add-to-set floater in move mode; **Remove from Set** confirms first.
//! - Each member row is **tinted with that person's set colour**
//!   ([`ContactSets::color_of`]) — the same answer the radar, name tags and chat
//!   will read once they colour by set, so what the panel shows is what the rest
//!   of the viewer will show.
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
use sl_client_bevy::{AgentKey, Command, SlCommand};

use crate::avatar_picker::{AvatarPicked, OpenAvatarPicker};
use crate::avatar_profile::OpenAvatarProfile;
use crate::contact_sets::{
    ALL_SETS_LABEL, ContactSets, NO_SETS_LABEL, RequestContactSet, apply_contact_set_requests,
};
use crate::conversations::{ConversationKey, OpenConversation};
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::notifications::{NotificationResponse, ShowNotification};
use crate::people::{FriendsModel, PeopleUi};
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

/// An action button's background.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The trailing action column's width, logical px.
const ACTION_COL_WIDTH: f32 = 150.0;

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
}

/// Whether `name` survives the list's filter (case-insensitive substring, like
/// the reference's own list filters).
fn matches_filter(name: &str, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase())
}

/// Order `rows` by the table's sort keys (most significant first), falling back
/// to a case-insensitive name compare so the order is total.
fn sort_rows(rows: &mut [MemberRow], keys: &[(&str, bool)]) {
    rows.sort_by(|left, right| {
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

/// The set-chooser options: the two pseudo-sets first (they always work, even
/// with no sets at all), then every real set in name order.
fn chooser_options(sets: &ContactSets) -> Vec<String> {
    let mut options = vec![ALL_SETS_LABEL.to_owned(), NO_SETS_LABEL.to_owned()];
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
    /// The resident to file (`None` when the floater is closed).
    agent: Option<AgentKey>,
    /// The best name the opening surface knew for them.
    name: String,
    /// The set to take them out of afterwards — the reference's move mode.
    move_from: Option<String>,
    /// The set the combo currently shows.
    chosen: String,
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
}

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
        /// The resident to file under the new set, if the prompt came from a
        /// path that was filing someone.
        then_add: Option<(AgentKey, String)>,
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
}

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

/// Ask for the add-to-set floater, for one resident. The avatar pie's
/// **Add ▸ Add to Set** and the panel's **Move to Set…** both write this.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenAddToContactSet {
    /// The resident to file.
    pub(crate) agent: AgentKey,
    /// The best name the opening surface knows for them (empty when it knows
    /// none).
    pub(crate) name: String,
    /// The set to take them out of once they are filed — the reference's move
    /// mode. `None` for a plain add.
    pub(crate) move_from: Option<String>,
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
                    handle_contact_set_picks,
                    handle_contact_set_colors,
                    handle_contact_set_notifications,
                    sync_config_floater,
                    rebuild_contact_sets_view,
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
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            button,
            Name::new("contact-sets-action"),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(button.label_key()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
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
    spawn_config_button(&mut commands, content, ConfigButton::Close);

    commands.insert_resource(ConfigUi {
        panel: handle.root,
        title: handle.title_text,
        name_field,
        swatch,
    });
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
    sort_rows(&mut view.rows, &keys);

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
        let name = sets
            .label_of(agent)
            .map(ToOwned::to_owned)
            .or_else(|| roster.get(&agent).cloned())
            .unwrap_or_else(|| short_id(agent));
        matches_filter(&name, filter).then(|| MemberRow {
            agent,
            sets: sets.sets_of(agent).join(", "),
            color: sets.color_of(agent),
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
              sets and the view / selection the actions read, and the five channels the nine \
              buttons write (prompts, picker, floaters, profile, IM, teleport)"
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
    let real_set = is_real_set(&sets, &view.choice).then(|| view.choice.clone());
    match button {
        ContactSetsButton::NewSet => {
            *pending = PendingAction::Create {
                then_add: None,
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
                pickers.write(OpenAvatarPicker {
                    requester: PICKER_REQUESTER,
                });
            }
        }
        ContactSetsButton::MoveMember => {
            if let (Some(name), Some(agent)) = (real_set, selected.0) {
                adds.write(OpenAddToContactSet {
                    agent,
                    name: sets
                        .label_of(agent)
                        .map(ToOwned::to_owned)
                        .unwrap_or_default(),
                    move_from: Some(name),
                });
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
        let label = if request.name.is_empty() {
            short_id(request.agent)
        } else {
            request.name.clone()
        };
        // With no set to file them under, the useful thing is the make-a-set
        // prompt rather than a floater whose only control is empty.
        if sets.set_count() == 0 {
            *pending = PendingAction::Create {
                then_add: Some((request.agent, label)),
                move_from: request.move_from.clone(),
            };
            notifications.write(ShowNotification::new("AddNewContactSet"));
            continue;
        }
        target.agent = Some(request.agent);
        target.name.clone_from(&label);
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

        let prompt = translator.format(
            if request.move_from.is_some() {
                "move-to-contact-set-prompt"
            } else {
                "add-to-contact-set-prompt"
            },
            &TransArgs::new().text("name", &label),
        );
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
    let Some(agent) = target.agent else {
        return;
    };
    match button {
        AddToSetButton::Add => {
            if target.chosen.is_empty() {
                return;
            }
            match target.move_from.clone() {
                Some(from) => requests.write(RequestContactSet::Move {
                    from,
                    to: target.chosen.clone(),
                    agent,
                }),
                None => requests.write(RequestContactSet::Add {
                    set: target.chosen.clone(),
                    agent,
                    name: target.name.clone(),
                }),
            };
            notifications.write(
                ShowNotification::new("AddToContactSetSingleSuccess")
                    .arg("NAME", target.name.clone())
                    .arg("SET", target.chosen.clone()),
            );
        }
        AddToSetButton::NewSet => {
            *pending = PendingAction::Create {
                then_add: Some((agent, target.name.clone())),
                move_from: target.move_from.clone(),
            };
            notifications.write(ShowNotification::new("AddNewContactSet"));
        }
        AddToSetButton::Cancel => {}
    }
    target.agent = None;
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = false;
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
    if shown_for.as_deref() == Some(name.as_str()) {
        return;
    }
    *shown_for = Some(name.clone());
    if let Ok(mut editor) = editors.get_mut(ui.name_field) {
        crate::web_floater::set_editor_text(&mut editor, &name, &mut font_cx, &mut layout_cx);
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

/// File a resident chosen in the shared avatar picker under the chosen set.
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
        requests.write(RequestContactSet::Add {
            set: view.choice.clone(),
            agent: pick.agent,
            name: pick.name.clone(),
        });
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
                if let Some((agent, label)) = then_add {
                    match move_from {
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
    use super::{MemberRow, chooser_options, is_real_set, matches_filter, short_id, sort_rows};
    use crate::contact_sets::{ALL_SETS_LABEL, ContactSetRefusal, ContactSets, NO_SETS_LABEL};
    use bevy::prelude::Color;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, Uuid};

    /// An agent key from a small integer.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(Uuid::from_u128(id))
    }

    /// A row named `name` in `sets`.
    fn row(name: &str, sets: &str) -> MemberRow {
        MemberRow {
            agent: agent(1),
            name: name.to_owned(),
            sets: sets.to_owned(),
            color: None,
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
        sort_rows(&mut rows, &[("name", true)]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "gamma"]);

        sort_rows(&mut rows, &[("name", false)]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["gamma", "beta", "Alpha"]);

        sort_rows(&mut rows, &[("sets", true)]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(
            names,
            ["beta", "gamma", "Alpha"],
            "the two Builders rows tie and fall back to the name"
        );
    }

    /// The chooser lists the two pseudo-sets first, then the sets in name order,
    /// and only a real set counts as one the buttons may change.
    #[test]
    fn the_chooser_lists_the_pseudo_sets_first() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        assert_eq!(
            chooser_options(&sets),
            [ALL_SETS_LABEL.to_owned(), NO_SETS_LABEL.to_owned()],
            "the pseudo-sets work even with no sets at all"
        );
        sets.create_set("Zulu")?;
        sets.create_set("Builders")?;
        assert_eq!(
            chooser_options(&sets),
            [
                ALL_SETS_LABEL.to_owned(),
                NO_SETS_LABEL.to_owned(),
                "Builders".to_owned(),
                "Zulu".to_owned(),
            ]
        );
        assert!(is_real_set(&sets, "Builders"));
        assert!(!is_real_set(&sets, ALL_SETS_LABEL));
        assert!(!is_real_set(&sets, NO_SETS_LABEL));
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
}
