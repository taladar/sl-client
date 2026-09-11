//! The **Experiences floater** (`viewer-experiences-floater`): the surface that
//! manages every experience the agent has a relationship with — the ones they
//! have allowed or blocked, the ones they administer, contribute to or own, the
//! grid-wide search for more, and the log of what the joined ones have actually
//! done.
//!
//! # Seven tabs, mirroring `LLFloaterExperiences`
//!
//! The reference builds one tab container holding a search panel
//! (`LLPanelExperiencePicker`), five identical experience lists
//! (`LLPanelExperiences`: Allowed, Blocked, Admin, Contrib, Owned) and the event
//! log (`LLPanelExperienceLog`). This window is the same seven, in the same
//! order. Each list is one capability's reply:
//!
//! | Tab | Command | Reply |
//! | --- | --- | --- |
//! | Allowed / Blocked | [`Command::RequestExperiencePermissions`] | [`SlSessionEvent::ExperiencePermissions`] |
//! | Admin | [`Command::RequestAdminExperiences`] | [`SlSessionEvent::AdminExperiences`] |
//! | Contributor | [`Command::RequestCreatorExperiences`] | [`SlSessionEvent::CreatorExperiences`] |
//! | Owned | [`Command::RequestOwnedExperiences`] | [`SlSessionEvent::OwnedExperiences`] |
//! | Search | [`Command::FindExperiences`] | [`SlSessionEvent::ExperienceSearchResults`] |
//!
//! Every one of those replies is a list of **ids**; the names, ratings and
//! owners the rows show come from [`Command::RequestExperienceInfo`], asked once
//! per id this window has not seen before and folded in as
//! [`SlSessionEvent::ExperienceInfo`] arrives — the same request-if-unknown
//! shape the group-name cache uses.
//!
//! # One list mechanism, not three
//!
//! All seven lists are the shared
//! [table widget](sl_viewer_ui_widgets::ui_table) over
//! [virtualized, recycled rows](sl_viewer_ui_core::virtual_list): the rows are
//! pooled and *bound* to whatever the list currently holds, never despawned and
//! respawned when the data moves. That is the rule — a floater builds its
//! content once and updates it in place — that the previous two-list version of
//! this window broke, and the events list, which grows with time rather than
//! with anything bounded, is the one that made it matter.
//!
//! It also moves the per-row **Forget** button out of the rows and into the
//! tab's action row, which is where the reference has it
//! (`LLPanelExperiences`'s `button_panel`, and the picker's `profile_btn`):
//! a pooled row is a *view* of an item, so an action bound into one would have
//! to be rebound on every scroll. Select a row, then act on it.
//!
//! # A forget is a fire-and-forget
//!
//! Opening the window (or **Refresh**) issues the five list GETs. The
//! permissions GET's reply is the authoritative full allowed / blocked pair —
//! but so, on the wire, is the reply to a **mutation**: the
//! `ExperiencePreferences` PUT / DELETE answers with only the single edited
//! experience, which `sl-proto` collapses into the *same*
//! [`SlSessionEvent::ExperiencePermissions`] event with empty lists. So a
//! permissions event is taken as a full list **only** while a GET this window
//! issued is outstanding (`pending_full_list`); a mutation reply, arriving with
//! no GET pending, is ignored and the optimistic update stands.
//!
//! # Search paging
//!
//! `FindExperiences` is paged, and the Next / Prev arrows are enabled from the
//! grid's own `next_page_url` / `previous_page_url` markers, carried through as
//! [`ExperienceSearchPage::has_next_page`] / `has_previous_page` — the same two
//! bits the reference reads (`LLPanelExperiencePicker::processResponse`). Only
//! the grid can answer "is there another page": it counted the matches this
//! page was cut from, and the page itself cannot say.
//!
//! A grid that sends neither marker therefore offers no paging at all, which is
//! what the reference does with such a reply — and better than guessing from
//! the row count, which offers a Next onto nothing whenever the result set is
//! an exact multiple of the page size.
//!
//! # Refusals and divergences
//!
//! - The Owned tab's **acquire** button (the reference's `sendPurchaseRequest`)
//!   is not built: there is no purchase command in our protocol surface, and a
//!   button that could only fail is worse than no button.
//! - The rating filter on the search tab persists the **rating code**, not the
//!   combo index the reference's `ExperienceSearchMaturity` stores, so
//!   reordering the list cannot silently change a saved filter.
//!
//! The in-the-moment grant prompt — the toast a script pops to *join* an
//! experience — is [`crate::experience_permission`]; one experience's own page
//! is [`crate::experience_profile`].
//!
//! Reference (Firestorm, read-only): `llfloaterexperiences`,
//! `llpanelexperiences`, `llpanelexperiencepicker`, `llpanelexperiencelog`,
//! `panel_experience_search.xml`, `panel_experience_log.xml`.

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use std::collections::{BTreeMap, BTreeSet};

use sl_client_bevy::{
    Command, ExperienceInfo, ExperienceKey, ExperiencePermission, ExperienceSearchPage, OwnerKey,
    SlCommand, SlEvent, SlSessionEvent,
};
use sl_l10n::{DateTimeLength, DateTimeStyle};
use sl_settings::{Scope, SettingValue};

use crate::experience_log::{
    ExperienceLog, LoggedExperienceEvent, SETTING_NOTIFY_ALL, permission_short,
};
use crate::experience_profile::{
    MATURITY_GENERAL, MATURITY_KEYS, OpenExperienceProfile, maturity_from_index, maturity_index,
    maturity_key,
};
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_checkbox};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabPlacement, TabSpec, fill_tab_container, spawn_tab_container,
};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, register_table_settings, set_table_cell, spawn_table,
    spawn_table_row,
};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::{AvatarState, GroupsModel};

/// The floater's id (its geometry-persistence key and menu target).
pub const EXPERIENCES_FLOATER_ID: &str = "experiences";

/// The element id the gallery specimen and its inert actions report under.
const EXPERIENCES_ELEMENT: &str = "experiences-floater";

/// The persisted-settings section this window's knobs live under — the same one
/// [`crate::experience_log`] uses, because they are one feature's settings.
const EXPERIENCES_SECTION: &[&str] = &["experiences"];

/// The search tab's max-content-rating filter, stored as a `sim_access` **rating
/// code** (see the module docs' divergence note).
pub const SETTING_SEARCH_MATURITY: &str = "ExperienceSearchMaturity";

/// The skin class a button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The list / body text size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A table row's height, in logical pixels.
const ROW_HEIGHT: f32 = 18.0;

/// The least height a pane's table takes, in logical pixels — see
/// [`spawn_pane_table`] for why a floor is needed at all.
const LIST_MIN_HEIGHT: f32 = 160.0;

/// The floater content's size, in logical pixels.
const CONTENT_SIZE: Vec2 = Vec2::new(620.0, 400.0);

/// The floater's least content size, in logical pixels.
const MIN_CONTENT_SIZE: Vec2 = Vec2::new(440.0, 280.0);

/// The tab strip's width, in logical pixels — a fixed width is what turns on
/// the strip's label truncation, which is the whole reason the strip is
/// vertical (see [`spawn_experiences_floater`]).
const STRIP_WIDTH: f32 = 124.0;

/// The gallery specimen's width, in logical pixels.
///
/// Deliberately **narrower** than [`CONTENT_SIZE`]: the floater sweep spawns a
/// specimen into the window's content *slot*, whose 8 px of padding leave less
/// room than the declared content size. A specimen as wide as the window it
/// stands for overflows that slot by exactly the padding, at every scale.
const SPECIMEN_WIDTH: f32 = 560.0;

/// The primary body text colour.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (table headers, the short-id fallback).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The heading accent — the same emerald the experience toast wears, so the
/// experience surfaces read as one family.
const HEADING_COLOR: Color = Color::srgb(0.42, 0.82, 0.60);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A disabled action's label colour (nothing selected to act on).
const DISABLED_TEXT_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

/// A list's background tint behind its rows.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The Notify checkbox's box, in logical pixels.
const CHECK_SIZE: f32 = 14.0;

/// The checkbox box's fill when unticked.
const CHECK_OFF: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);

/// The checkbox box's fill when ticked — the same emerald the headings wear.
const CHECK_ON: Color = HEADING_COLOR;

/// The number of leading hex characters of an experience id shown as a fallback
/// label while its name is still resolving.
const SHORT_ID_LEN: usize = 8;

// ---------------------------------------------------------------------------
// Tables.
// ---------------------------------------------------------------------------

/// The name column of an experience list.
const COL_LIST_NAME: usize = 0;

/// The rating column of an experience list.
const COL_LIST_RATING: usize = 1;

/// The rating column of the search results.
const COL_SEARCH_RATING: usize = 0;

/// The name column of the search results.
const COL_SEARCH_NAME: usize = 1;

/// The owner column of the search results.
const COL_SEARCH_OWNER: usize = 2;

/// The time column of the event log.
const COL_EVENT_TIME: usize = 0;

/// The kind column of the event log.
const COL_EVENT_KIND: usize = 1;

/// The experience column of the event log.
const COL_EVENT_EXPERIENCE: usize = 2;

/// The object column of the event log.
const COL_EVENT_OBJECT: usize = 3;

/// The two columns every experience list shows.
static LIST_COLUMNS: [TableColumn; 2] = [
    TableColumn {
        header_key: "experiences-col-name",
        token: "name",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-rating",
        token: "rating",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 84.0 },
        align: TableAlign::Start,
        sortable: true,
    },
];

/// Every list sorts by name ascending until the person says otherwise — the
/// reference's `LLExperienceItemComparator`.
static LIST_SORT: [TableSortDefault; 1] = [TableSortDefault {
    column: COL_LIST_NAME,
    ascending: true,
}];

/// One experience list's table spec. A `const fn` rather than five hand-copied
/// literals, because the five differ only in where they persist.
const fn list_table(
    element: &'static str,
    sort_setting: &'static str,
    widths_setting: &'static str,
) -> TableSpec {
    TableSpec {
        element,
        columns: &LIST_COLUMNS,
        selection: TableSelectionMode::Single,
        default_sort: &LIST_SORT,
        builtin_sort: true,
        row_height: ROW_HEIGHT,
        font_size: FONT_SIZE,
        header_color: DIM_TEXT_COLOR,
        cell_color: TEXT_COLOR,
        column_gap: 4.0,
        row_padding: 4.0,
        sort_setting: Some(sort_setting),
        widths_setting: Some(widths_setting),
    }
}

/// The Allowed tab's table.
static ALLOWED_TABLE: TableSpec = list_table(
    "experiences-allowed",
    "ExperiencesAllowedSort",
    "ExperiencesAllowedWidths",
);

/// The Blocked tab's table.
static BLOCKED_TABLE: TableSpec = list_table(
    "experiences-blocked",
    "ExperiencesBlockedSort",
    "ExperiencesBlockedWidths",
);

/// The Admin tab's table.
static ADMIN_TABLE: TableSpec = list_table(
    "experiences-admin",
    "ExperiencesAdminSort",
    "ExperiencesAdminWidths",
);

/// The Contributor tab's table.
static CONTRIBUTOR_TABLE: TableSpec = list_table(
    "experiences-contributor",
    "ExperiencesContributorSort",
    "ExperiencesContributorWidths",
);

/// The Owned tab's table.
static OWNED_TABLE: TableSpec = list_table(
    "experiences-owned",
    "ExperiencesOwnedSort",
    "ExperiencesOwnedWidths",
);

/// The search results' columns — rating, name and owner, as the reference's
/// `search_results` scroll list.
static SEARCH_COLUMNS: [TableColumn; 3] = [
    TableColumn {
        header_key: "experiences-col-rating",
        token: "rating",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 84.0 },
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-name",
        token: "name",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-owner",
        token: "owner",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
];

/// The search results sort by name, like the reference's
/// `sortByColumnIndex(1, true)`.
static SEARCH_SORT: [TableSortDefault; 1] = [TableSortDefault {
    column: COL_SEARCH_NAME,
    ascending: true,
}];

/// The search tab's table.
static SEARCH_TABLE: TableSpec = TableSpec {
    element: "experiences-search",
    columns: &SEARCH_COLUMNS,
    selection: TableSelectionMode::Single,
    default_sort: &SEARCH_SORT,
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_TEXT_COLOR,
    cell_color: TEXT_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: Some("ExperiencesSearchSort"),
    widths_setting: Some("ExperiencesSearchWidths"),
};

/// The event log's four columns — the reference's `experience_log_list`.
static EVENT_COLUMNS: [TableColumn; 4] = [
    TableColumn {
        header_key: "experiences-col-time",
        token: "time",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 112.0 },
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-event",
        token: "event",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 92.0 },
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-experience",
        token: "experience",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "experiences-col-object",
        token: "object",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
];

/// The log opens newest-first — what "recent events" means.
static EVENT_SORT: [TableSortDefault; 1] = [TableSortDefault {
    column: COL_EVENT_TIME,
    ascending: false,
}];

/// The events tab's table.
static EVENTS_TABLE: TableSpec = TableSpec {
    element: "experiences-events",
    columns: &EVENT_COLUMNS,
    selection: TableSelectionMode::None,
    default_sort: &EVENT_SORT,
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_TEXT_COLOR,
    cell_color: TEXT_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: Some("ExperiencesEventsSort"),
    widths_setting: Some("ExperiencesEventsWidths"),
};

// ---------------------------------------------------------------------------
// Panes.
// ---------------------------------------------------------------------------

/// Which of the five id-list tabs a pane is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListTab {
    /// Experiences the agent has allowed.
    Allowed,
    /// Experiences the agent has blocked.
    Blocked,
    /// Experiences the agent administers.
    Admin,
    /// Experiences the agent created (the reference's "Contrib").
    Contributor,
    /// Experiences the agent owns.
    Owned,
}

impl ListTab {
    /// The five tabs, in the reference's order.
    const ALL: [Self; 5] = [
        Self::Allowed,
        Self::Blocked,
        Self::Admin,
        Self::Contributor,
        Self::Owned,
    ];

    /// The tab's Fluent label key.
    const fn label_key(self) -> &'static str {
        match self {
            Self::Allowed => "experiences-tab-allowed",
            Self::Blocked => "experiences-tab-blocked",
            Self::Admin => "experiences-tab-admin",
            Self::Contributor => "experiences-tab-contributor",
            Self::Owned => "experiences-tab-owned",
        }
    }

    /// The tab's table spec.
    const fn spec(self) -> &'static TableSpec {
        match self {
            Self::Allowed => &ALLOWED_TABLE,
            Self::Blocked => &BLOCKED_TABLE,
            Self::Admin => &ADMIN_TABLE,
            Self::Contributor => &CONTRIBUTOR_TABLE,
            Self::Owned => &OWNED_TABLE,
        }
    }

    /// Whether a row in this tab can be forgotten — only the two tabs that are
    /// *preferences* rather than *relationships*.
    const fn forgettable(self) -> bool {
        matches!(self, Self::Allowed | Self::Blocked)
    }
}

/// Which pane of the window a table belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// One of the five id lists.
    List(ListTab),
    /// The search results.
    Search,
    /// The event log.
    Events,
}

impl Pane {
    /// The pane's table spec.
    const fn spec(self) -> &'static TableSpec {
        match self {
            Self::List(tab) => tab.spec(),
            Self::Search => &SEARCH_TABLE,
            Self::Events => &EVENTS_TABLE,
        }
    }
}

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// A search's progress, which is what the results table's empty line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SearchProgress {
    /// Nothing asked for yet.
    #[default]
    Idle,
    /// A query is out.
    Searching,
    /// A page came back, with the grid's word on what lies on either side of
    /// it — which is what enables the two arrows (see the module docs).
    Done {
        /// Whether the grid offered a page after this one.
        has_next_page: bool,
        /// Whether the grid offered a page before this one.
        has_previous_page: bool,
    },
}

impl SearchProgress {
    /// The progress an arrived page puts the search in: done, remembering the
    /// grid's two paging markers verbatim.
    const fn from_page(page: &ExperienceSearchPage) -> Self {
        Self::Done {
            has_next_page: page.has_next_page,
            has_previous_page: page.has_previous_page,
        }
    }

    /// Whether the Next arrow has somewhere to go. Only a page the grid said
    /// has a successor does — a search that has not answered yet has none, and
    /// neither does a grid that sends no markers.
    const fn offers_next(self) -> bool {
        matches!(
            self,
            Self::Done {
                has_next_page: true,
                ..
            }
        )
    }

    /// Whether the Previous arrow has somewhere to go.
    const fn offers_previous(self) -> bool {
        matches!(
            self,
            Self::Done {
                has_previous_page: true,
                ..
            }
        )
    }
}

/// The floater's data: the five id lists, the resolved-metadata cache, the
/// search's own state, the outstanding-GET count that disambiguates a full-list
/// reply from a single-edit reply, and a revision the rebuild watches.
#[derive(Resource, Debug, Default)]
struct ExperiencesState {
    /// The experiences the agent has allowed (accepted).
    allowed: Vec<ExperienceKey>,
    /// The experiences the agent has blocked.
    blocked: Vec<ExperienceKey>,
    /// The experiences the agent administers.
    admin: Vec<ExperienceKey>,
    /// The experiences the agent created.
    contributor: Vec<ExperienceKey>,
    /// The experiences the agent owns.
    owned: Vec<ExperienceKey>,
    /// The current search results, in reply order.
    search: Vec<ExperienceKey>,
    /// The search's progress.
    progress: SearchProgress,
    /// The page the last search asked for (one-based, as the cap numbers them).
    page: i32,
    /// The query the last search asked for, so a Next re-asks the same words.
    query: String,
    /// Resolved experience metadata, folded in as
    /// [`SlSessionEvent::ExperienceInfo`] arrives.
    infos: BTreeMap<ExperienceKey, ExperienceInfo>,
    /// The number of [`Command::RequestExperiencePermissions`] GETs whose reply
    /// is still outstanding. A permissions event is treated as an authoritative
    /// full list only while this is non-zero (see the module docs).
    pending_full_list: u32,
    /// Bumped on any change the view rebuild must react to.
    revision: u64,
}

impl ExperiencesState {
    /// Bump the revision so the view rebuild reacts.
    const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// One list tab's ids.
    fn ids(&self, tab: ListTab) -> &[ExperienceKey] {
        match tab {
            ListTab::Allowed => &self.allowed,
            ListTab::Blocked => &self.blocked,
            ListTab::Admin => &self.admin,
            ListTab::Contributor => &self.contributor,
            ListTab::Owned => &self.owned,
        }
    }

    /// Replace both preference lists from an authoritative GET reply.
    fn set_permissions(&mut self, allowed: Vec<ExperienceKey>, blocked: Vec<ExperienceKey>) {
        self.allowed = allowed;
        self.blocked = blocked;
        self.touch();
    }

    /// Fold a resolved metadata record into the cache.
    fn note_info(&mut self, info: ExperienceInfo) {
        let _previous = self.infos.insert(info.public_id, info);
        self.touch();
    }

    /// Optimistically drop an experience from both preference lists (a forget),
    /// so its row leaves at once.
    fn forget(&mut self, id: ExperienceKey) {
        self.allowed.retain(|other| *other != id);
        self.blocked.retain(|other| *other != id);
        self.touch();
    }

    /// The resolved name for an experience id, if known and non-empty.
    fn name(&self, id: ExperienceKey) -> Option<&str> {
        self.infos
            .get(&id)
            .map(|info| info.name.as_str())
            .filter(|name| !name.is_empty())
    }

    /// Every id any pane mentions — what the metadata fetch asks about.
    fn mentioned_ids(&self, log: &ExperienceLog) -> Vec<ExperienceKey> {
        let mut ids: Vec<ExperienceKey> = ListTab::ALL
            .iter()
            .flat_map(|tab| self.ids(*tab).iter().copied())
            .chain(self.search.iter().copied())
            .chain(log.entries().iter().map(|entry| entry.experience_id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// One list / search row, with every cell already rendered for the current
/// locale and name caches — so the bind pass only writes strings, and a cache
/// that resolves later simply rebuilds the view.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExperienceRow {
    /// The experience this row stands for.
    id: ExperienceKey,
    /// The display name (or the short-id fallback).
    name: String,
    /// The content rating's label, or empty while the metadata is unknown.
    rating: String,
    /// The owner's resolved name, or empty when unknown.
    owner: String,
}

/// One event-log row, rendered the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EventRow {
    /// When it happened.
    time: String,
    /// What was done (with the repeat count folded in).
    kind: String,
    /// Which experience did it.
    experience: String,
    /// The object it did it with.
    object: String,
}

/// The rendered rows of every pane, rebuilt when anything they are derived from
/// moves.
#[derive(Resource, Debug, Default)]
struct ExperiencesView {
    /// The five id lists' rows, in [`ListTab::ALL`] order.
    lists: [Vec<ExperienceRow>; 5],
    /// The search results' rows.
    search: Vec<ExperienceRow>,
    /// The event log's rows.
    events: Vec<EventRow>,
    /// The state revision these rows were built from.
    built_revision: Option<u64>,
    /// The log revision these rows were built from.
    built_log_revision: Option<u64>,
    /// The sort revisions these rows were built from, one per pane.
    built_sorts: [u64; 7],
}

impl ExperiencesView {
    /// One list tab's rendered rows.
    fn list(&self, tab: ListTab) -> &[ExperienceRow] {
        self.lists.get(list_index(tab)).map_or(&[], Vec::as_slice)
    }

    /// One pane's rendered experience rows (the events pane has none).
    fn rows(&self, pane: Pane) -> &[ExperienceRow] {
        match pane {
            Pane::List(tab) => self.list(tab),
            Pane::Search => &self.search,
            Pane::Events => &[],
        }
    }
}

/// A list tab's index into [`ExperiencesView::lists`].
const fn list_index(tab: ListTab) -> usize {
    match tab {
        ListTab::Allowed => 0,
        ListTab::Blocked => 1,
        ListTab::Admin => 2,
        ListTab::Contributor => 3,
        ListTab::Owned => 4,
    }
}

/// A pane's index into [`ExperiencesView::built_sorts`].
const fn pane_index(pane: Pane) -> usize {
    match pane {
        Pane::List(tab) => list_index(tab),
        Pane::Search => 5,
        Pane::Events => 6,
    }
}

/// The floater's entities the systems act on.
#[derive(Resource, Debug)]
pub(crate) struct ExperiencesUi {
    /// The floater root entity (carries [`UiPanelShown`]).
    pub(crate) panel: Entity,
    /// Each pane's table root and viewport, in [`pane_index`] order.
    panes: [PaneHandles; 7],
    /// The search field.
    search_field: Entity,
    /// The search status / paging line.
    search_status: Entity,
    /// The search tab's rating filter combo.
    maturity_combo: Entity,
}

/// One pane's table entities.
#[derive(Debug, Clone, Copy)]
struct PaneHandles {
    /// The table root (carries [`TableState`]).
    table: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
}

/// On a pane's viewport, so the shared row pool systems know which list a
/// freshly-recycled row belongs to without a system per pane.
#[derive(Component, Debug, Clone, Copy)]
struct PaneViewport {
    /// Which pane this viewport shows.
    pane: Pane,
    /// The table root the rows take their column widths from.
    table: Entity,
}

/// Which of the window's non-row buttons a node is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ExperiencesButton {
    /// Re-read every list.
    Refresh,
    /// Open the selected row's profile.
    Profile(Pane),
    /// Forget the selected experience.
    Forget(ListTab),
    /// Run the search from page one.
    Find,
    /// Step the search page (`true` forward).
    Page(bool),
    /// Empty the event log.
    ClearEvents,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// Register the experiences-search setting.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        EXPERIENCES_SECTION,
        SETTING_SEARCH_MATURITY,
        SettingValue::I32(MATURITY_GENERAL),
        "The highest content rating experience search results may carry \
         (13 General, 21 Moderate, 42 Adult)",
    );
}

/// The plugin owning the Experiences floater.
#[derive(Debug)]
pub struct ExperiencesPlugin;

impl Plugin for ExperiencesPlugin {
    /// Register the state and systems, and spawn the (hidden) floater.
    fn build(&self, app: &mut App) {
        app.init_resource::<ExperiencesState>()
            .init_resource::<ExperiencesView>()
            .add_systems(
                Startup,
                spawn_experiences_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    register_experience_tables,
                    request_lists_on_show,
                    ingest_experience_events,
                    request_unknown_experience_infos,
                    find_experiences_on_enter,
                    track_maturity_filter,
                    rebuild_experience_views,
                    paint_experience_actions,
                    paint_notify_checkbox,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            // The row pool is filled by `layout_virtual_lists`, so the two
            // passes that build and bind those rows have to follow it — the
            // ordering every other table consumer uses. Before it, a row
            // spawned this frame would only be populated on the next.
            .add_systems(
                Update,
                (populate_experience_rows, bind_experience_rows)
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

/// Register every table's persisted sort / widths and seed the rating filter
/// from the store, once both the settings store and the window's widgets exist
/// (the store is built after this plugin's `Startup`).
fn register_experience_tables(
    ui: Option<Res<ExperiencesUi>>,
    settings: Option<ResMut<ViewerSettings>>,
    mut combos: Query<&mut ComboSelection>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let (Some(ui), Some(mut settings)) = (ui, settings) else {
        return;
    };
    *done = true;
    for pane in every_pane() {
        register_table_settings(&mut settings, EXPERIENCES_SECTION, pane.spec());
    }
    let wanted = maturity_index(search_ceiling(Some(&settings)));
    if let Ok(mut combo) = combos.get_mut(ui.maturity_combo)
        && combo.active != wanted
    {
        combo.active = wanted;
    }
}

/// Every pane, in [`pane_index`] order.
const fn every_pane() -> [Pane; 7] {
    [
        Pane::List(ListTab::Allowed),
        Pane::List(ListTab::Blocked),
        Pane::List(ListTab::Admin),
        Pane::List(ListTab::Contributor),
        Pane::List(ListTab::Owned),
        Pane::Search,
        Pane::Events,
    ]
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// The experiences floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn experiences_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: EXPERIENCES_FLOATER_ID,
        title: "Experiences".to_owned(),
        position: Vec2::new(360.0, 140.0),
        default_size: Some(CONTENT_SIZE),
        min_size: Some(MIN_CONTENT_SIZE),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Spawn the Experiences floater (hidden): a Refresh row over the seven-tab
/// container, each tab holding its table and its actions.
fn spawn_experiences_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, experiences_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("experiences-title"));
    // Straight into the floater's own content slot, with no wrapper of its own:
    // the slot is already a gapped column, and a child given `width: 100%`
    // there resolves that percentage against the slot's *border* box while
    // sitting inside its *content* box — so it overflows by exactly the slot's
    // padding, every time. Stretch and `flex_grow` are what fill a slot.
    let content = handle.content;

    // The Refresh row above the tabs: the reference refreshes every list at
    // once (`refreshContents`), so the control belongs to the window, not to a
    // tab.
    let refresh_row = commands
        .spawn((
            Node {
                justify_content: JustifyContent::End,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let _refresh = spawn_action(
        &mut commands,
        refresh_row,
        "experiences-refresh",
        ExperiencesButton::Refresh,
        1,
    );

    let mut labels: Vec<String> = vec!["experiences-tab-search".to_owned()];
    labels.extend(ListTab::ALL.iter().map(|tab| tab.label_key().to_owned()));
    labels.push("experiences-tab-events".to_owned());
    // **A vertical strip, where the reference's is horizontal.** Seven labels
    // in a row do not fit a window this wide — not in Latin at scale 1 (they
    // overflow by 16 logical px), and not at all once a translation or a UI
    // scale lengthens them; a horizontal strip answers that by clipping, which
    // slices a label mid-glyph. The reference sidesteps it by *widening the
    // window to the tabs* (`LLFloaterExperiences::resizeToTabs`), which cannot
    // work across scripts. A fixed-width vertical strip truncates each label
    // with the locale's ellipsis instead, and gives the divider that lets the
    // strip be widened by hand — the same answer Preferences already uses for
    // the same reason.
    let tabs = spawn_tab_container(
        &mut commands,
        content,
        &TabSpec {
            element: "experiences-tabs",
            placement: TabPlacement::InlineStart,
            labels: &labels,
            active: 1,
            tab_index: 2,
            font_size: FONT_SIZE,
            strip_width: Some(STRIP_WIDTH),
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(&mut commands, TabPlacement::InlineStart, &tabs);

    let mut panels = tabs.panels.iter().copied();
    let search_panel = panels.next().unwrap_or(content);
    let search = build_search_tab(&mut commands, search_panel);
    let mut list_handles: Vec<PaneHandles> = Vec::with_capacity(ListTab::ALL.len());
    for tab in ListTab::ALL {
        let panel = panels.next().unwrap_or(content);
        list_handles.push(build_list_tab(&mut commands, panel, tab));
    }
    let events_panel = panels.next().unwrap_or(content);
    let events = build_events_tab(&mut commands, events_panel);

    let panes = [
        pane_at(&list_handles, 0),
        pane_at(&list_handles, 1),
        pane_at(&list_handles, 2),
        pane_at(&list_handles, 3),
        pane_at(&list_handles, 4),
        search.handles,
        events,
    ];
    commands.insert_resource(ExperiencesUi {
        panel: handle.root,
        panes,
        search_field: search.field,
        search_status: search.status,
        maturity_combo: search.maturity_combo,
    });
}

/// One built list tab's handles, or a duplicate of the first when the tab
/// container came back short (it cannot, but indexing must not be able to
/// panic).
fn pane_at(handles: &[PaneHandles], index: usize) -> PaneHandles {
    handles.get(index).copied().unwrap_or(PaneHandles {
        table: Entity::PLACEHOLDER,
        viewport: Entity::PLACEHOLDER,
    })
}

/// Build one id-list tab: the table over its action row.
fn build_list_tab(commands: &mut Commands, panel: Entity, tab: ListTab) -> PaneHandles {
    let pane = Pane::List(tab);
    let handles = spawn_pane_table(commands, panel, pane);
    let actions = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    let _profile = spawn_action(
        commands,
        actions,
        "experiences-profile",
        ExperiencesButton::Profile(pane),
        3,
    );
    if tab.forgettable() {
        let _forget = spawn_action(
            commands,
            actions,
            "experiences-forget",
            ExperiencesButton::Forget(tab),
            4,
        );
    }
    handles
}

/// The search tab's handles, returned by [`build_search_tab`].
struct SearchTab {
    /// The results table.
    handles: PaneHandles,
    /// The query field.
    field: Entity,
    /// The status / paging line.
    status: Entity,
    /// The rating filter combo.
    maturity_combo: Entity,
}

/// Build the search tab: the query row, the results table, and the action row.
fn build_search_tab(commands: &mut Commands, panel: Entity) -> SearchTab {
    let query_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    let search = spawn_search_field(
        commands,
        query_row,
        &SearchFieldSpec {
            tab_index: 3,
            font_size: FONT_SIZE,
            min_width: 160.0,
            placeholder: String::new(),
            search_glyph: true,
            ..SearchFieldSpec::new("experiences-query")
        },
    );
    if let Some(placeholder) = search.placeholder {
        commands
            .entity(placeholder)
            .insert(Translated::new("experiences-search-placeholder"));
    }
    let _find = spawn_action(
        commands,
        query_row,
        "experiences-find",
        ExperiencesButton::Find,
        4,
    );

    let filter_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("experiences-search-rating"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(filter_row),
    ));
    let labels: Vec<String> = MATURITY_KEYS.iter().map(|key| (*key).to_owned()).collect();
    let maturity_combo = spawn_combo(
        commands,
        filter_row,
        &ComboSpec {
            element: "experiences-search-rating",
            labels: &labels,
            active: 0,
            tab_index: 5,
            font_size: FONT_SIZE,
            translate_labels: true,
        },
    );

    let handles = spawn_pane_table(commands, panel, Pane::Search);

    let actions = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    let _profile = spawn_action(
        commands,
        actions,
        "experiences-profile",
        ExperiencesButton::Profile(Pane::Search),
        6,
    );
    let _prev = spawn_action(
        commands,
        actions,
        "experiences-page-previous",
        ExperiencesButton::Page(false),
        7,
    );
    let _next = spawn_action(
        commands,
        actions,
        "experiences-page-next",
        ExperiencesButton::Page(true),
        8,
    );
    let status = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_TEXT_COLOR),
            Pickable::IGNORE,
            Name::new("experiences-search-status"),
            ChildOf(actions),
        ))
        .id();

    SearchTab {
        handles,
        field: search.field,
        status,
        maturity_combo,
    }
}

/// Build the events tab: the log table over its Notify / Clear controls.
fn build_events_tab(commands: &mut Commands, panel: Entity) -> PaneHandles {
    let handles = spawn_pane_table(commands, panel, Pane::Events);
    let controls = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    spawn_notify_checkbox(commands, controls);
    let _clear = spawn_action(
        commands,
        controls,
        "experiences-events-clear",
        ExperiencesButton::ClearEvents,
        9,
    );
    handles
}

/// Spawn one pane's table, tagging its viewport so the shared row pool systems
/// can tell which pane a recycled row belongs to.
fn spawn_pane_table(commands: &mut Commands, panel: Entity, pane: Pane) -> PaneHandles {
    // A floor, not a size: the tab panels scroll their overflow, and inside a
    // scrolling column a purely `flex_grow` table resolves to **zero** height —
    // and a layout sweep passes on that, because a zero-height widget is still
    // laid out. The floor is what guarantees rows on screen; the grow is what
    // lets the table take a resized window.
    let wrapper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(LIST_MIN_HEIGHT),
                ..column(Val::Px(2.0))
            },
            ChildOf(panel),
        ))
        .id();
    let table = spawn_table(commands, wrapper, pane.spec());
    commands.entity(table.viewport).insert((
        BackgroundColor(LIST_BACKGROUND),
        PaneViewport {
            pane,
            table: table.root,
        },
    ));
    PaneHandles {
        table: table.root,
        viewport: table.viewport,
    }
}

/// The settings-bound "notify on every event" checkbox and its label — the
/// reference's `notify_all`, which decides whether a recorded event also raises
/// a toast.
fn spawn_notify_checkbox(commands: &mut Commands, parent: Entity) {
    let row_node = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(5.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        bound_checkbox(SettingBinding::account(SETTING_NOTIFY_ALL)),
        Node {
            width: Val::Px(CHECK_SIZE),
            height: Val::Px(CHECK_SIZE),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BorderColor::all(BUTTON_BORDER),
        BackgroundColor(CHECK_OFF),
        TabIndex(9),
        NotifyCheckboxBox,
        Pickable::default(),
        ChildOf(row_node),
    ));
    commands.spawn((
        Text::default(),
        Translated::new("experiences-events-notify"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(row_node),
    ));
}

/// Marks the Notify checkbox's box so its fill can follow [`Checked`] (the
/// headless widget carries the state; the visual is ours).
#[derive(Component, Debug)]
struct NotifyCheckboxBox;

/// Keep the Notify checkbox's fill agreeing with its [`Checked`] state.
fn paint_notify_checkbox(
    mut boxes: Query<(&mut BackgroundColor, Has<Checked>), With<NotifyCheckboxBox>>,
) {
    for (mut fill, checked) in &mut boxes {
        let wanted = if checked { CHECK_ON } else { CHECK_OFF };
        if fill.0 != wanted {
            fill.0 = wanted;
        }
    }
}

/// Spawn one of the window's action buttons, wired to the shared observer.
fn spawn_action(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    button: ExperiencesButton,
    tab: i32,
) -> Entity {
    let entity = spawn_button_shell(commands, parent, tab);
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(entity),
    ));
    commands
        .entity(entity)
        .insert(button)
        .observe(on_experiences_button);
    entity
}

/// Spawn a button shell (the bordered, skinnable box) with no label yet.
fn spawn_button_shell(commands: &mut Commands, parent: Entity, tab: i32) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BUTTON_BORDER),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new("experiences-button"),
            ChildOf(parent),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Requests.
// ---------------------------------------------------------------------------

/// Issue every list GET, counting the permissions one so its reply is accepted
/// as authoritative (see `ExperiencesState::pending_full_list`).
fn request_lists(state: &mut ExperiencesState, sl: &mut MessageWriter<SlCommand>) {
    state.pending_full_list = state.pending_full_list.saturating_add(1);
    sl.write(SlCommand(Command::RequestExperiencePermissions));
    sl.write(SlCommand(Command::RequestOwnedExperiences));
    sl.write(SlCommand(Command::RequestAdminExperiences));
    sl.write(SlCommand(Command::RequestCreatorExperiences));
}

/// When the floater becomes visible, re-read every list — so they are fresh
/// each time it is opened. Fires on the hidden→shown **edge** (tracked in a
/// `Local`), not merely while shown, so it issues one round of GETs per open.
fn request_lists_on_show(
    ui: Option<Res<ExperiencesUi>>,
    panels: Query<&UiPanelShown>,
    mut was_shown: Local<bool>,
    mut state: ResMut<ExperiencesState>,
    mut sl: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    let shown = panels.get(ui.panel).is_ok_and(|panel| panel.0);
    if shown && !*was_shown {
        request_lists(&mut state, &mut sl);
    }
    *was_shown = shown;
}

/// Ask the grid for the metadata of every id any pane mentions that the cache
/// does not hold yet.
///
/// Remembered in a `Local` so a list that keeps naming an id the grid will not
/// resolve does not re-ask every frame; a record that does arrive lands in
/// `infos` and takes the id out of the filter anyway.
fn request_unknown_experience_infos(
    log: Res<ExperienceLog>,
    state: Res<ExperiencesState>,
    mut asked: Local<BTreeSet<ExperienceKey>>,
    mut sl: MessageWriter<SlCommand>,
) {
    if !log.is_changed() && !state.is_changed() {
        return;
    }
    let unknown: Vec<ExperienceKey> = state
        .mentioned_ids(&log)
        .into_iter()
        .filter(|id| !state.infos.contains_key(id) && !asked.contains(id))
        .collect();
    if unknown.is_empty() {
        return;
    }
    asked.extend(unknown.iter().copied());
    sl.write(SlCommand(Command::RequestExperienceInfo {
        experience_ids: unknown,
    }));
}

// ---------------------------------------------------------------------------
// Ingest.
// ---------------------------------------------------------------------------

/// Fold every arriving experience reply into the state.
fn ingest_experience_events(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ExperiencesState>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ExperiencePermissions { allowed, blocked } => {
                // A single-edit reply arrives with no GET pending; the
                // optimistic update already stands (see the module docs).
                if state.pending_full_list == 0 {
                    continue;
                }
                state.pending_full_list = state.pending_full_list.saturating_sub(1);
                state.set_permissions(allowed.clone(), blocked.clone());
            }
            SlSessionEvent::OwnedExperiences(ids) => {
                state.owned.clone_from(ids);
                state.touch();
            }
            SlSessionEvent::AdminExperiences(ids) => {
                state.admin.clone_from(ids);
                state.touch();
            }
            SlSessionEvent::CreatorExperiences(ids) => {
                state.contributor.clone_from(ids);
                state.touch();
            }
            SlSessionEvent::ExperienceSearchResults(page) => {
                state.search = page.infos.iter().map(|info| info.public_id).collect();
                state.progress = SearchProgress::from_page(page);
                for info in &page.infos {
                    if !info.missing {
                        state.note_info(info.clone());
                    }
                }
                state.touch();
            }
            SlSessionEvent::ExperienceInfo(list) => {
                for info in list.iter().filter(|info| !info.missing) {
                    state.note_info(info.clone());
                }
            }
            SlSessionEvent::ExperienceUpdated(info) if !info.missing => {
                state.note_info(info.clone());
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// View.
// ---------------------------------------------------------------------------

/// Rebuild every pane's rendered rows when anything they derive from moved: the
/// state, the log, a table's sort, the name caches or the locale.
#[expect(
    clippy::too_many_arguments,
    reason = "the rendered rows are a function of every one of these: the id lists, \
              the log, the seven sorts, the two name caches and the locale"
)]
fn rebuild_experience_views(
    state: Res<ExperiencesState>,
    log: Res<ExperienceLog>,
    ui: Option<Res<ExperiencesUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    settings: Option<Res<ViewerSettings>>,
    translator: Translator,
    tables: Query<&TableState>,
    mut view: ResMut<ExperiencesView>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    let sorts = every_pane().map(|pane| {
        ui.panes
            .get(pane_index(pane))
            .and_then(|handles| tables.get(handles.table).ok())
            .map_or(0, TableState::sort_revision)
    });
    let caches_moved = avatars.is_changed() || groups.is_changed() || translator.changed();
    if !caches_moved
        && view.built_revision == Some(state.revision)
        && view.built_log_revision == Some(log.revision())
        && view.built_sorts == sorts
    {
        return;
    }
    view.built_revision = Some(state.revision);
    view.built_log_revision = Some(log.revision());
    view.built_sorts = sorts;

    for tab in ListTab::ALL {
        let mut rows = render_rows(&state, state.ids(tab), &avatars, &groups, &translator);
        sort_experience_rows(&mut rows, sort_keys(&ui, &tables, Pane::List(tab)));
        if let Some(slot) = view.lists.get_mut(list_index(tab)) {
            *slot = rows;
        }
    }
    // The rating filter hides results rather than re-querying, which is what
    // the reference's `filterContent` does — the cap has no rating parameter.
    let ceiling = search_ceiling(settings.as_deref());
    let visible: Vec<ExperienceKey> = state
        .search
        .iter()
        .copied()
        .filter(|id| {
            state
                .infos
                .get(id)
                .is_none_or(|info| info.maturity <= ceiling)
        })
        .collect();
    let mut search = render_rows(&state, &visible, &avatars, &groups, &translator);
    sort_experience_rows(&mut search, sort_keys(&ui, &tables, Pane::Search));
    view.search = search;

    let mut events: Vec<(i64, EventRow)> = log
        .entries()
        .iter()
        .map(|entry| (entry.unix, render_event(entry, &state, &log, &translator)))
        .collect();
    sort_event_rows(&mut events, sort_keys(&ui, &tables, Pane::Events));
    view.events = events.into_iter().map(|(_unix, row)| row).collect();

    for pane in every_pane() {
        let count = match pane {
            Pane::Events => view.events.len(),
            _experiences => view.rows(pane).len(),
        };
        if let Some(handles) = ui.panes.get(pane_index(pane))
            && let Ok(mut list) = lists.get_mut(handles.viewport)
            && list.item_count != count
        {
            list.item_count = count;
        }
    }
}

/// One pane's active sort, as `(column token, ascending)` pairs.
fn sort_keys(
    ui: &ExperiencesUi,
    tables: &Query<&TableState>,
    pane: Pane,
) -> Vec<(&'static str, bool)> {
    let Some(state) = ui
        .panes
        .get(pane_index(pane))
        .and_then(|handles| tables.get(handles.table).ok())
    else {
        return Vec::new();
    };
    state
        .sort()
        .keys()
        .iter()
        .filter_map(|key| {
            pane.spec()
                .columns
                .get(key.column)
                .map(|column| (column.token, key.ascending))
        })
        .collect()
}

/// Render one list of ids into display rows.
fn render_rows(
    state: &ExperiencesState,
    ids: &[ExperienceKey],
    avatars: &AvatarState,
    groups: &GroupsModel,
    translator: &Translator,
) -> Vec<ExperienceRow> {
    ids.iter()
        .map(|id| {
            let info = state.infos.get(id);
            ExperienceRow {
                id: *id,
                name: experience_label(state, *id),
                rating: info.map_or_else(String::new, |info| {
                    translator.get(maturity_key(info.maturity))
                }),
                owner: info
                    .and_then(|info| info.owner)
                    .map_or_else(String::new, |owner| owner_label(owner, avatars, groups)),
            }
        })
        .collect()
}

/// One owner's resolved name, or its id in parentheses until the cache has it.
fn owner_label(owner: OwnerKey, avatars: &AvatarState, groups: &GroupsModel) -> String {
    let resolved = match owner {
        OwnerKey::Agent(agent) => avatars.shown_name_of(agent).map(str::to_owned),
        OwnerKey::Group(group) => groups.group_name(group).map(str::to_owned),
    };
    resolved.unwrap_or_else(|| format!("({})", owner.uuid()))
}

/// Render one logged event into its four cells.
fn render_event(
    entry: &LoggedExperienceEvent,
    state: &ExperiencesState,
    log: &ExperienceLog,
    translator: &Translator,
) -> EventRow {
    let time = log
        .civil_local(entry.unix)
        .map_or_else(String::new, |civil| {
            translator.datetime(civil, DateTimeStyle::DateTime, DateTimeLength::Short)
        });
    let short = permission_short(entry.permission, translator);
    let kind = if entry.count > 1 {
        translator.format(
            "experiences-event-kind-repeated",
            &TransArgs::new()
                .text("event", &short)
                .int("count", i64::from(entry.count)),
        )
    } else {
        short
    };
    EventRow {
        time,
        kind,
        experience: experience_label(state, entry.experience_id),
        object: entry.object_name.clone(),
    }
}

/// Order rendered experience rows by a table's sort keys, most significant
/// first. An unknown token leaves the order alone rather than inventing one.
fn sort_experience_rows(rows: &mut [ExperienceRow], keys: Vec<(&'static str, bool)>) {
    rows.sort_by(|left, right| {
        for (token, ascending) in &keys {
            let ordering = match *token {
                "name" => compare_ci(&left.name, &right.name),
                "rating" => compare_ci(&left.rating, &right.rating),
                "owner" => compare_ci(&left.owner, &right.owner),
                _unknown => core::cmp::Ordering::Equal,
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
        core::cmp::Ordering::Equal
    });
}

/// Order rendered event rows. Time orders by the entry's own timestamp rather
/// than its rendered text, because a localized date string does not sort
/// chronologically.
fn sort_event_rows(rows: &mut [(i64, EventRow)], keys: Vec<(&'static str, bool)>) {
    rows.sort_by(|left, right| {
        for (token, ascending) in &keys {
            let ordering = match *token {
                "time" => left.0.cmp(&right.0),
                "event" => compare_ci(&left.1.kind, &right.1.kind),
                "experience" => compare_ci(&left.1.experience, &right.1.experience),
                "object" => compare_ci(&left.1.object, &right.1.object),
                _unknown => core::cmp::Ordering::Equal,
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
        core::cmp::Ordering::Equal
    });
}

/// Case-insensitive comparison, the way the reference's name comparator
/// upper-cases both sides before comparing.
fn compare_ci(left: &str, right: &str) -> core::cmp::Ordering {
    left.to_lowercase().cmp(&right.to_lowercase())
}

/// The row label for an experience: its resolved name, or the leading hex of
/// its id as a stable fallback while the name is still resolving.
fn experience_label(state: &ExperiencesState, id: ExperienceKey) -> String {
    state
        .name(id)
        .map_or_else(|| short_experience_id(id), str::to_owned)
}

/// The leading [`SHORT_ID_LEN`] hex characters of an experience id, an ellipsis
/// appended — the stable fallback shown until the name resolves.
fn short_experience_id(id: ExperienceKey) -> String {
    let hex = id.uuid().simple().to_string();
    let head: String = hex.chars().take(SHORT_ID_LEN).collect();
    format!("{head}\u{2026}")
}

// ---------------------------------------------------------------------------
// Row pool.
// ---------------------------------------------------------------------------

/// Build the table cells of each freshly-pooled row, whichever pane it belongs
/// to. One system for all seven lists: the pane is on the viewport the row was
/// pooled under.
fn populate_experience_rows(
    mut commands: Commands,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
    viewports: Query<&PaneViewport>,
) {
    for (row_entity, child_of) in &new_rows {
        let Ok(viewport) = viewports.get(child_of.parent()) else {
            continue;
        };
        spawn_table_row(
            &mut commands,
            row_entity,
            viewport.table,
            viewport.pane.spec(),
        );
    }
}

/// Bind each pooled row to the item it now presents.
fn bind_experience_rows(
    view: Res<ExperiencesView>,
    viewports: Query<&PaneViewport>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let refresh_all = view.is_changed();
    for (row, child_of, cells) in &rows {
        let Ok(viewport) = viewports.get(child_of.parent()) else {
            continue;
        };
        if !refresh_all && !row.is_changed() {
            continue;
        }
        match viewport.pane {
            Pane::Events => {
                let data = row.index.and_then(|index| view.events.get(index));
                for (column, value) in [
                    (COL_EVENT_TIME, data.map(|row| row.time.as_str())),
                    (COL_EVENT_KIND, data.map(|row| row.kind.as_str())),
                    (
                        COL_EVENT_EXPERIENCE,
                        data.map(|row| row.experience.as_str()),
                    ),
                    (COL_EVENT_OBJECT, data.map(|row| row.object.as_str())),
                ] {
                    if let Some(cell) = cells.cell(column) {
                        set_table_cell(&mut texts, cell, value.unwrap_or(""), TEXT_COLOR);
                    }
                }
            }
            pane => {
                let data = row.index.and_then(|index| view.rows(pane).get(index));
                let columns = if matches!(pane, Pane::Search) {
                    [COL_SEARCH_NAME, COL_SEARCH_RATING, COL_SEARCH_OWNER]
                } else {
                    [COL_LIST_NAME, COL_LIST_RATING, usize::MAX]
                };
                let values = [
                    data.map(|row| row.name.as_str()),
                    data.map(|row| row.rating.as_str()),
                    data.map(|row| row.owner.as_str()),
                ];
                for (column, value) in columns.into_iter().zip(values) {
                    if let Some(cell) = cells.cell(column) {
                        set_table_cell(&mut texts, cell, value.unwrap_or(""), TEXT_COLOR);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Actions.
// ---------------------------------------------------------------------------

/// Grey the actions that would do nothing: Profile and Forget without a
/// selection, and the paging arrows at the ends of the result set. Also writes
/// the search tab's status / page line.
fn paint_experience_actions(
    ui: Option<Res<ExperiencesUi>>,
    state: Res<ExperiencesState>,
    tables: Query<&TableState>,
    buttons: Query<(&ExperiencesButton, &Children)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (button, children) in &buttons {
        let enabled = match *button {
            ExperiencesButton::Profile(pane) => selected_row(&ui, &tables, pane).is_some(),
            ExperiencesButton::Forget(tab) => selected_row(&ui, &tables, Pane::List(tab)).is_some(),
            ExperiencesButton::Page(true) => state.progress.offers_next(),
            ExperiencesButton::Page(false) => state.progress.offers_previous(),
            _always => true,
        };
        let wanted = if enabled {
            TEXT_COLOR
        } else {
            DISABLED_TEXT_COLOR
        };
        for child in children {
            if let Ok((_text, mut color)) = texts.get_mut(*child)
                && color.0 != wanted
            {
                color.0 = wanted;
            }
        }
    }
    let line = match state.progress {
        SearchProgress::Idle => String::new(),
        SearchProgress::Searching => translator.get("experiences-searching"),
        SearchProgress::Done { .. } => translator.format(
            "experiences-search-page",
            &TransArgs::new().int("page", i64::from(state.page)),
        ),
    };
    if let Ok((mut text, _color)) = texts.get_mut(ui.search_status)
        && text.0 != line
    {
        text.0 = line;
    }
}

/// The experience a pane's selected row stands for, if any.
fn selected_row(ui: &ExperiencesUi, tables: &Query<&TableState>, pane: Pane) -> Option<usize> {
    ui.panes
        .get(pane_index(pane))
        .and_then(|handles| tables.get(handles.table).ok())
        .and_then(TableState::primary_selected)
}

/// Every action button, resolved by its [`ExperiencesButton`] kind.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected world access: the button kind, \
              the window handles, both models, the table selections, the query field \
              and the two command sinks"
)]
fn on_experiences_button(
    activate: On<Activate>,
    buttons: Query<&ExperiencesButton>,
    ui: Option<Res<ExperiencesUi>>,
    view: Res<ExperiencesView>,
    tables: Query<&TableState>,
    fields: Query<&EditableText>,
    mut log: ResMut<ExperienceLog>,
    mut state: ResMut<ExperiencesState>,
    mut profiles: MessageWriter<OpenExperienceProfile>,
    mut sl: MessageWriter<SlCommand>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    match *button {
        ExperiencesButton::Refresh => request_lists(&mut state, &mut sl),
        ExperiencesButton::Profile(pane) => {
            if let Some(index) = selected_row(&ui, &tables, pane)
                && let Some(row) = view.rows(pane).get(index)
            {
                profiles.write(OpenExperienceProfile { experience: row.id });
            }
        }
        ExperiencesButton::Forget(tab) => {
            let pane = Pane::List(tab);
            if let Some(index) = selected_row(&ui, &tables, pane)
                && let Some(row) = view.rows(pane).get(index)
            {
                let id = row.id;
                sl.write(SlCommand(Command::SetExperiencePermission {
                    experience_id: id,
                    permission: ExperiencePermission::Forget,
                }));
                state.forget(id);
            }
        }
        ExperiencesButton::Find => {
            let query = fields
                .get(ui.search_field)
                .map(|field| field.value().to_string())
                .unwrap_or_default();
            run_search(&mut state, query, 1, &mut sl);
        }
        ExperiencesButton::Page(forward) => {
            let page = if forward {
                state.page.saturating_add(1)
            } else {
                state.page.saturating_sub(1).max(1)
            };
            let query = state.query.clone();
            run_search(&mut state, query, page, &mut sl);
        }
        ExperiencesButton::ClearEvents => log.clear(),
    }
}

/// Run the search from `page`, remembering the query so a paging step re-asks
/// the same words.
fn run_search(
    state: &mut ExperiencesState,
    query: String,
    page: i32,
    sl: &mut MessageWriter<SlCommand>,
) {
    state.query = query;
    state.page = page.max(1);
    state.progress = SearchProgress::Searching;
    state.search.clear();
    state.touch();
    sl.write(SlCommand(Command::FindExperiences {
        query: state.query.clone(),
        page: state.page,
    }));
}

/// `Enter` in the query field runs the search — the reference's default button
/// on its `search_panel`.
fn find_experiences_on_enter(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    ui: Option<Res<ExperiencesUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<ExperiencesState>,
    mut sl: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    if focus.get() != Some(ui.search_field) {
        return;
    }
    let query = fields
        .get(ui.search_field)
        .map(|field| field.value().to_string())
        .unwrap_or_default();
    run_search(&mut state, query, 1, &mut sl);
    keyboard.clear_just_pressed(KeyCode::Enter);
}

/// Persist the rating filter when it changes, and re-filter the current page.
fn track_maturity_filter(
    mut changes: MessageReader<ComboChanged>,
    ui: Option<Res<ExperiencesUi>>,
    mut settings: Option<ResMut<ViewerSettings>>,
    mut state: ResMut<ExperiencesState>,
) {
    let Some(ui) = ui else {
        return;
    };
    for change in changes.read() {
        if change.combo != ui.maturity_combo {
            continue;
        }
        if let Some(settings) = settings.as_mut() {
            settings.set(
                Scope::Account,
                SETTING_SEARCH_MATURITY,
                SettingValue::I32(maturity_from_index(change.active)),
            );
        }
        state.touch();
    }
}

/// The highest rating a search result may carry, from the persisted filter.
fn search_ceiling(settings: Option<&ViewerSettings>) -> i32 {
    settings
        .and_then(|settings| settings.store().get_i32(SETTING_SEARCH_MATURITY).ok())
        .unwrap_or(MATURITY_GENERAL)
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// The gallery / `ui_test` specimen: a static approximation of one list tab —
/// the tab labels, a two-column list with a few rows, and the action row — so
/// the layout is swept login-free (the live floater needs a session).
/// Registered in `crate::ui_element::ELEMENTS`; its buttons report an inert
/// [`UiAction`].
pub fn spawn_experiences_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            Node {
                width: Val::Px(SPECIMEN_WIDTH),
                min_width: Val::Px(0.0),
                ..row(Val::Px(8.0))
            },
            ChildOf(parent),
        ))
        .id();
    // The tab strip, as plain labels in a fixed-width column: the live strip is
    // the shared tab widget, which the gallery sweeps under its own element.
    // Fixed-width because that is what the live one is, and because a specimen
    // whose labels grow with the script is a specimen that overflows in
    // Devanagari and tells you nothing about this window.
    let strip = commands
        .spawn((
            Node {
                width: Val::Px(STRIP_WIDTH),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                ..column(Val::Px(4.0))
            },
            ChildOf(root),
        ))
        .id();
    for label in [
        cx.text("Search"),
        cx.text("Allowed"),
        cx.text("Blocked"),
        cx.text("Admin"),
        cx.text("Contributor"),
        cx.text("Owned"),
        cx.text("Recent events"),
    ] {
        commands.spawn((
            Text::new(label),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(HEADING_COLOR),
            Pickable::IGNORE,
            ChildOf(strip),
        ));
    }
    let panel = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                overflow: Overflow::clip(),
                ..column(Val::Px(6.0))
            },
            ChildOf(root),
        ))
        .id();
    // The list: a header row over a few value rows, laid out like the table's.
    spawn_specimen_row(
        commands,
        panel,
        &cx.text("Experience"),
        &cx.text("Rating"),
        DIM_TEXT_COLOR,
    );
    for (name, rating) in [
        (cx.text("Neon Speedway"), cx.text("Moderate")),
        (cx.text("Beachside Games"), cx.text("General")),
        (cx.text("Spam Kiosk"), cx.text("General")),
    ] {
        spawn_specimen_row(commands, panel, &name, &rating, TEXT_COLOR);
    }
    // The action row, with the Forget button the element contract drives.
    let actions = commands
        .spawn((
            Node {
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    spawn_specimen_button(commands, actions, &cx.text("Profile\u{2026}"), "profile");
    spawn_specimen_button(commands, actions, &cx.text("Forget"), "forget");
    root
}

/// One specimen list row: a name and a rating.
fn spawn_specimen_row(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    rating: &str,
    color: Color,
) {
    let row_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
                overflow: Overflow::clip(),
                ..row(Val::Px(6.0))
            },
            Name::new("experiences-row"),
            ChildOf(parent),
        ))
        .id();
    for value in [name, rating] {
        commands.spawn((
            Text::new(value.to_owned()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(color),
            Pickable::IGNORE,
            ChildOf(row_entity),
        ));
    }
}

/// One specimen action button, reporting an inert [`UiAction`] (the registry
/// rule: a specimen reaches no session).
fn spawn_specimen_button(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    action: &'static str,
) {
    let button = spawn_button_shell(commands, parent, 0);
    commands.spawn((
        Text::new(label.to_owned()),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    // Named per action, unlike the live buttons: the element contract addresses
    // a node by name and a sweep would otherwise pick whichever of the two
    // identically-named buttons the query reached first.
    commands
        .entity(button)
        .insert(Name::new(format!("experiences-action:{action}")));
    commands.entity(button).observe(
        move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
            actions.write(UiAction {
                element: EXPERIENCES_ELEMENT,
                action,
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{
        ExperienceRow, ExperiencesState, ListTab, Pane, SHORT_ID_LEN, SearchProgress,
        experience_label, list_index, pane_index, short_experience_id, sort_experience_rows,
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{ExperienceInfo, ExperienceKey, ExperienceSearchPage, Uuid};

    /// A row with just the fields the sort reads.
    fn row(name: &str, rating: &str, owner: &str) -> ExperienceRow {
        ExperienceRow {
            id: ExperienceKey::from(Uuid::from_u128(0x1)),
            name: name.to_owned(),
            rating: rating.to_owned(),
            owner: owner.to_owned(),
        }
    }

    /// The short id is the leading hex of the (dash-free) uuid with an ellipsis.
    #[test]
    fn short_id_is_leading_hex_with_ellipsis() {
        let id = ExperienceKey::from(Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0));
        let short = short_experience_id(id);
        assert_eq!(short, "12345678\u{2026}");
        assert_eq!(short.chars().count(), SHORT_ID_LEN + 1);
    }

    /// A row shows the resolved name once known, and the short-id fallback
    /// until then — and an *empty* name is not a resolution.
    #[test]
    fn label_prefers_the_resolved_name() {
        let id = ExperienceKey::from(Uuid::from_u128(0xabcd));
        let mut state = ExperiencesState::default();
        assert_eq!(experience_label(&state, id), short_experience_id(id));

        // A record with an empty name is what the grid sends for an experience
        // it knows of but will not name; the fallback must survive it.
        state.note_info(ExperienceInfo {
            public_id: id,
            name: String::new(),
            ..ExperienceInfo::default()
        });
        assert_eq!(experience_label(&state, id), short_experience_id(id));

        state.note_info(ExperienceInfo {
            public_id: id,
            name: "Neon Speedway".to_owned(),
            ..ExperienceInfo::default()
        });
        assert_eq!(experience_label(&state, id), "Neon Speedway");
    }

    /// A forget drops the experience from whichever preference list held it and
    /// bumps the revision so the view rebuild reacts — and leaves the
    /// *relationship* lists alone, which forgetting a preference does not touch.
    #[test]
    fn forget_drops_from_the_preference_lists_only() {
        let allowed = ExperienceKey::from(Uuid::from_u128(0x1));
        let blocked = ExperienceKey::from(Uuid::from_u128(0x2));
        let mut state = ExperiencesState::default();
        state.set_permissions(vec![allowed], vec![blocked]);
        state.owned = vec![allowed];
        let before = state.revision;

        state.forget(allowed);
        assert!(state.allowed.is_empty());
        assert_eq!(state.blocked, vec![blocked]);
        assert_eq!(state.owned, vec![allowed]);
        assert_ne!(state.revision, before);
    }

    /// Every pane has its own index, and the five list tabs' indices agree with
    /// their pane indices — the invariant the row pool and the sort stamps rely
    /// on.
    #[test]
    fn pane_indices_are_distinct() {
        let mut seen: Vec<usize> = super::every_pane().iter().map(|p| pane_index(*p)).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count);
        for tab in ListTab::ALL {
            assert_eq!(pane_index(Pane::List(tab)), list_index(tab));
        }
    }

    /// The sort is case-insensitive, multi-level, and honours each level's
    /// direction.
    #[test]
    fn rows_sort_case_insensitively_by_each_key() {
        let mut rows = vec![
            row("beta", "General", "Zoe"),
            row("Alpha", "Moderate", "Ann"),
            row("alpha", "General", "Bob"),
        ];
        sort_experience_rows(&mut rows, vec![("name", true), ("rating", true)]);
        assert_eq!(
            rows.iter().map(|r| r.rating.as_str()).collect::<Vec<_>>(),
            vec!["General", "Moderate", "General"]
        );
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "Alpha", "beta"]
        );

        sort_experience_rows(&mut rows, vec![("owner", false)]);
        assert_eq!(
            rows.iter().map(|r| r.owner.as_str()).collect::<Vec<_>>(),
            vec!["Zoe", "Bob", "Ann"]
        );
    }

    /// An unknown sort token leaves the order alone rather than inventing one.
    #[test]
    fn an_unknown_sort_token_is_inert() {
        let mut rows = vec![row("beta", "", ""), row("alpha", "", "")];
        sort_experience_rows(&mut rows, vec![("nonesuch", true)]);
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["beta", "alpha"]
        );
    }

    /// The two arrows are enabled from the grid's own markers, not from the
    /// row count: an empty page whose grid said there is more still offers a
    /// Next, and a page that fills the table but is the last one does not.
    #[test]
    fn the_arrows_follow_the_grids_markers() {
        let progress = |has_next_page, has_previous_page| {
            SearchProgress::from_page(&ExperienceSearchPage {
                infos: Vec::new(),
                has_next_page,
                has_previous_page,
            })
        };
        assert!(progress(true, false).offers_next());
        assert!(!progress(true, false).offers_previous());
        assert!(progress(false, true).offers_previous());
        assert!(!progress(false, true).offers_next());
        assert!(!progress(false, false).offers_next());
        assert!(!progress(false, false).offers_previous());
    }

    /// A search that has not answered offers no paging in either direction —
    /// there is no page to be on the far side of.
    #[test]
    fn an_unanswered_search_offers_no_paging() {
        for progress in [SearchProgress::Idle, SearchProgress::Searching] {
            assert!(!progress.offers_next(), "{progress:?}");
            assert!(!progress.offers_previous(), "{progress:?}");
        }
    }

    /// Only the two preference tabs offer a Forget; the three relationship tabs
    /// have nothing to forget.
    #[test]
    fn only_preference_tabs_are_forgettable() {
        assert!(ListTab::Allowed.forgettable());
        assert!(ListTab::Blocked.forgettable());
        assert!(!ListTab::Admin.forgettable());
        assert!(!ListTab::Contributor.forgettable());
        assert!(!ListTab::Owned.forgettable());
    }
}
