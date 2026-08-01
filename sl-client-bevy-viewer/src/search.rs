//! The **Search floater** (`viewer-search-floater`): the in-viewer directory
//! search, reproducing Firestorm's legacy `fsfloatersearch` layout — a tab strip
//! of result lists on the left plus a **shared details pane** on the right that
//! shows the selected result and its actions.
//!
//! # Tabs
//!
//! **Web** (an embedded browser to the grid's search site), then six
//! protocol-backed directory tabs — **People**, **Groups**, **Places**, **Land**,
//! **Events**, **Classifieds** — over the four `Dir*Query` wire calls
//! (`api-g4`). Each directory tab is a [results **table**](crate::ui_table)
//! (single-select) with the reference's columns, its own `query_start` paging and
//! result count, plus per-tab filters (Places / Classifieds category, Land
//! sale-type + sort, Events date-mode + category). A shared **maturity** control
//! (General / Moderate / Adult → the `INC_*` bits) gates every query.
//!
//! # The shared details pane
//!
//! Selecting a row fills the right-hand pane and fires a **secondary detail
//! request** for the categories that need one: `RequestParcelInfo` for
//! Places / Land (→ [`ParcelDetails`](SlSessionEvent::ParcelDetails), the source
//! of the teleport position), `EventInfoRequest` for Events, and
//! `RequestClassifiedInfo` for Classifieds. People / Groups need none (the reply
//! already carries the name). The pane's actions are **Open Profile** (People /
//! Groups → the profile floaters), **Teleport** and **Show on Map**
//! ([`OpenWorldMap`]) for the located categories, and **Remind me** for Events
//! (`EventNotification` add / remove, tracked locally since the protocol has no
//! read-back). The pane and the tables are built once and updated in place.
//!
//! Reference (Firestorm, read-only): `fsfloatersearch.cpp` (legacy search),
//! `floater_fs_search.xml` + `panel_fs_search_legacy_*.xml`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::Button;
use sl_client_bevy::{
    AgentKey, AvatarProperties, ClassifiedCategory, ClassifiedInfo, ClassifiedKey, Command,
    DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult, DirLandResult,
    DirPeopleResult, DirPlaceResult, EventId, EventInfo, GlobalCoordinates, GroupKey, GroupProfile,
    ParcelCategory, ParcelDetails, ParcelKey, QueryId, RegionCoordinates, RegionHandle, SlCommand,
    SlEvent, SlIdentity, SlSessionEvent, TextureKey, Uuid, Vector, to_bevy_image,
};
use sl_settings::SettingValue;

use crate::avatar_profile::OpenAvatarProfile;
use crate::browser_widget::{BrowserView, BrowserViewSpec, spawn_browser_view};
use crate::conversations::{ConversationKey, OpenConversation};
use crate::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater,
};
use crate::group_profile::OpenGroupProfile;
use crate::i18n::{Translated, UiLocale};
use crate::media_engine::MediaSurfaces;
use crate::render_priority::AVATAR_BOOST_PRIORITY;
use crate::settings::ViewerSettings;
use crate::settings_binding::{SettingBinding, bound_checkbox};
use crate::textures::TextureManager;
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_combo::{ComboChanged, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_radio::{RadioLayout, RadioSelection, RadioSpec, spawn_radio_group};
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabContainerHandle, TabPlacement, TabSpec, TabStrip, fill_tab_container,
    spawn_tab_container,
};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSpec, TableState, set_table_cell, spawn_table, spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow};
use crate::world_map::OpenWorldMap;

// ---------------------------------------------------------------------------
// Constants.
// ---------------------------------------------------------------------------

/// The floater's [`FloaterSpec::id`].
pub(crate) const SEARCH_FLOATER_ID: &str = "search";

/// The `[search]` settings section.
const SEARCH_SECTION: &[&str] = &["search"];

/// Restrict a People search to online avatars.
const SETTING_ONLINE_ONLY: &str = "SearchPeopleOnlineOnly";

/// Sort Land results ascending (else descending, the default).
const SETTING_LAND_ASCENDING: &str = "SearchLandAscending";

/// The base UI font size, in logical pixels.
const FONT: f32 = 13.0;

/// A results table row height, in logical pixels.
const ROW_HEIGHT: f32 = 20.0;

/// How many results one page requests.
const PAGE_SIZE: i32 = 100;

/// [`PAGE_SIZE`] as a `usize`.
const PAGE_SIZE_USIZE: usize = 100;

/// The details pane's fixed width, in logical pixels.
const DETAIL_WIDTH: f32 = 300.0;

/// The SL grid search website (used for the Web tab; OpenSim overrides it from
/// `SimulatorFeatures` when present).
const SL_SEARCH_URL: &str = "https://search.secondlife.com/";

/// A label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dim secondary colour.
const SECONDARY_COLOR: Color = Color::srgb(0.72, 0.76, 0.84);

/// A table header colour.
const HEADER_COLOR: Color = Color::srgb(0.78, 0.82, 0.90);

/// A checkbox box's border.
const CHECK_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A checkbox box's fill when unchecked.
const CHECK_OFF: Color = Color::srgb(0.12, 0.14, 0.18);

/// A checkbox box's fill when checked.
const CHECK_ON: Color = Color::srgb(0.30, 0.70, 0.45);

/// A checkbox box's side length, in logical pixels.
const CHECK_SIZE: f32 = 16.0;

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// A link value's colour.
const LINK_COLOR: Color = Color::srgb(0.52, 0.68, 0.95);

// ---------------------------------------------------------------------------
// Tabs & categories.
// ---------------------------------------------------------------------------

/// A floater tab. **Web** is the embedded browser; the rest are the directory
/// categories. The variant order is the tab-strip order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SearchTab {
    /// The embedded web search.
    #[default]
    Web,
    /// People (`DirFindQuery` / [`DirFindFlags::PEOPLE`]).
    People,
    /// Groups (`DirFindQuery` / [`DirFindFlags::GROUPS`]).
    Groups,
    /// Places (`DirPlacesQuery`).
    Places,
    /// Land for sale (`DirLandQuery`).
    Land,
    /// Events (`DirFindQuery` / [`DirFindFlags::EVENTS`]).
    Events,
    /// Classifieds (`DirClassifiedQuery`).
    Classifieds,
}

/// The tab order, matching the spawned strip and the panel slots.
const TAB_ORDER: [SearchTab; 7] = [
    SearchTab::Web,
    SearchTab::People,
    SearchTab::Groups,
    SearchTab::Places,
    SearchTab::Land,
    SearchTab::Events,
    SearchTab::Classifieds,
];

/// A directory category (a tab that runs a wire query — every tab but Web).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchCategory {
    /// People.
    People,
    /// Groups.
    Groups,
    /// Places.
    Places,
    /// Land.
    Land,
    /// Events.
    Events,
    /// Classifieds.
    Classifieds,
}

/// The six directory categories, in tab order (for iterating the panels).
const CATEGORY_ORDER: [SearchCategory; 6] = [
    SearchCategory::People,
    SearchCategory::Groups,
    SearchCategory::Places,
    SearchCategory::Land,
    SearchCategory::Events,
    SearchCategory::Classifieds,
];

impl SearchTab {
    /// The directory category this tab queries, or `None` for the Web tab.
    const fn category(self) -> Option<SearchCategory> {
        match self {
            Self::Web => None,
            Self::People => Some(SearchCategory::People),
            Self::Groups => Some(SearchCategory::Groups),
            Self::Places => Some(SearchCategory::Places),
            Self::Land => Some(SearchCategory::Land),
            Self::Events => Some(SearchCategory::Events),
            Self::Classifieds => Some(SearchCategory::Classifieds),
        }
    }
}

impl SearchCategory {
    /// Whether the category's query is driven by the shared text field (every
    /// category except **Land**, a pure filter query).
    const fn needs_query_text(self) -> bool {
        !matches!(self, Self::Land)
    }
}

// ---------------------------------------------------------------------------
// Land / events filter enums.
// ---------------------------------------------------------------------------

/// The Land tab's sale-type filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LandSaleFilter {
    /// Every sale type.
    #[default]
    All,
    /// Auctioned land only.
    Auction,
    /// Mainland parcels only.
    Mainland,
    /// Estate parcels only.
    Estate,
}

/// The Land sale-type options, in combo order.
const LAND_SALE_ORDER: [LandSaleFilter; 4] = [
    LandSaleFilter::All,
    LandSaleFilter::Auction,
    LandSaleFilter::Mainland,
    LandSaleFilter::Estate,
];

/// The Land sale-type option labels.
const LAND_SALE_LABELS: [&str; 4] = ["All", "Auction", "Mainland", "Estate"];

impl LandSaleFilter {
    /// The wire sale-type mask this filter selects.
    const fn to_search_type(self) -> sl_client_bevy::LandSearchType {
        use sl_client_bevy::LandSearchType;
        match self {
            Self::All => LandSearchType::ALL,
            Self::Auction => LandSearchType::AUCTION,
            Self::Mainland => LandSearchType::MAINLAND,
            Self::Estate => LandSearchType::ESTATE,
        }
    }
}

/// The Land sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LandSort {
    /// By asking price (the reference default).
    #[default]
    Price,
    /// By parcel name.
    Name,
    /// By area.
    Area,
    /// By price per square metre.
    PerMeter,
}

/// The Land sort options, in combo order.
const LAND_SORT_ORDER: [LandSort; 4] = [
    LandSort::Price,
    LandSort::Name,
    LandSort::Area,
    LandSort::PerMeter,
];

/// The Land sort option labels.
const LAND_SORT_LABELS: [&str; 4] = ["Price", "Name", "Area", "Price / m²"];

impl LandSort {
    /// The wire sort flag this order selects.
    const fn to_flag(self) -> DirFindFlags {
        match self {
            Self::Name => DirFindFlags::NAME_SORT,
            Self::Price => DirFindFlags::PRICE_SORT,
            Self::Area => DirFindFlags::AREA_SORT,
            Self::PerMeter => DirFindFlags::PER_METER_SORT,
        }
    }
}

/// The Events tab's date mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EventsMode {
    /// Ongoing and upcoming events (the wire `"u"` day token).
    #[default]
    Current,
    /// Events on a specific day offset from today.
    ByDate,
}

/// The Places category options, in combo order.
const PLACES_CATEGORIES: [ParcelCategory; 8] = [
    ParcelCategory::None,
    ParcelCategory::Linden,
    ParcelCategory::Residential,
    ParcelCategory::Commercial,
    ParcelCategory::Industrial,
    ParcelCategory::ParkAndRecreation,
    ParcelCategory::Other,
    ParcelCategory::Adult,
];

/// The Places category labels.
const PLACES_CATEGORY_LABELS: [&str; 8] = [
    "Any",
    "Linden",
    "Residential",
    "Commercial",
    "Industrial",
    "Parks & Rec",
    "Other",
    "Adult",
];

/// The Classifieds category options, in combo order.
const CLASSIFIED_CATEGORIES: [ClassifiedCategory; 10] = [
    ClassifiedCategory::AnyCategory,
    ClassifiedCategory::Shopping,
    ClassifiedCategory::LandRental,
    ClassifiedCategory::PropertyRental,
    ClassifiedCategory::SpecialAttraction,
    ClassifiedCategory::NewProducts,
    ClassifiedCategory::Employment,
    ClassifiedCategory::Wanted,
    ClassifiedCategory::Service,
    ClassifiedCategory::Personal,
];

/// The Classifieds category labels.
const CLASSIFIED_CATEGORY_LABELS: [&str; 10] = [
    "Any",
    "Shopping",
    "Land Rental",
    "Property Rental",
    "Attraction",
    "New Products",
    "Employment",
    "Wanted",
    "Service",
    "Personal",
];

/// The Events category options: the wire category number paired with a label.
const EVENT_CATEGORIES: [(u32, &str); 13] = [
    (0, "Any"),
    (18, "Discussion"),
    (19, "Sports"),
    (30, "Live DJ"),
    (20, "Live Music"),
    (22, "Commercial"),
    (23, "Nightlife"),
    (24, "Games/Contests"),
    (25, "Pageants"),
    (26, "Education"),
    (27, "Arts and Culture"),
    (28, "Charity/Support"),
    (29, "Miscellaneous"),
];

// ---------------------------------------------------------------------------
// Table specs (one per category).
// ---------------------------------------------------------------------------

/// A leading icon column (unused for now; kept for reference-column parity).
const fn text_column(
    header_key: &'static str,
    token: &'static str,
    width: TableColumnWidth,
    align: TableAlign,
) -> TableColumn {
    TableColumn {
        header_key,
        token,
        kind: TableColumnKind::Text,
        width,
        align,
        sortable: false,
    }
}

/// The People results table (name only).
static PEOPLE_TABLE: TableSpec = TableSpec {
    element: "search-people",
    selection: TableSelectionMode::Single,
    columns: &[text_column(
        "search-col-name",
        "name",
        TableColumnWidth::Flex(1.0),
        TableAlign::Start,
    )],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Groups results table (name + members).
static GROUPS_TABLE: TableSpec = TableSpec {
    element: "search-groups",
    selection: TableSelectionMode::Single,
    columns: &[
        text_column(
            "search-col-name",
            "name",
            TableColumnWidth::Flex(1.0),
            TableAlign::Start,
        ),
        text_column(
            "search-col-members",
            "members",
            TableColumnWidth::Fixed { default: 70.0 },
            TableAlign::End,
        ),
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Places results table (name + traffic).
static PLACES_TABLE: TableSpec = TableSpec {
    element: "search-places",
    selection: TableSelectionMode::Single,
    columns: &[
        text_column(
            "search-col-name",
            "name",
            TableColumnWidth::Flex(1.0),
            TableAlign::Start,
        ),
        text_column(
            "search-col-traffic",
            "traffic",
            TableColumnWidth::Fixed { default: 70.0 },
            TableAlign::End,
        ),
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Land results table (name + price + area + L$/m + type).
static LAND_TABLE: TableSpec = TableSpec {
    element: "search-land",
    selection: TableSelectionMode::Single,
    columns: &[
        text_column(
            "search-col-name",
            "name",
            TableColumnWidth::Flex(1.0),
            TableAlign::Start,
        ),
        text_column(
            "search-col-price",
            "price",
            TableColumnWidth::Fixed { default: 60.0 },
            TableAlign::End,
        ),
        text_column(
            "search-col-area",
            "area",
            TableColumnWidth::Fixed { default: 60.0 },
            TableAlign::End,
        ),
        text_column(
            "search-col-ppm",
            "ppm",
            TableColumnWidth::Fixed { default: 60.0 },
            TableAlign::End,
        ),
        text_column(
            "search-col-type",
            "type",
            TableColumnWidth::Fixed { default: 64.0 },
            TableAlign::Start,
        ),
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Events results table (name + date).
static EVENTS_TABLE: TableSpec = TableSpec {
    element: "search-events",
    selection: TableSelectionMode::Single,
    columns: &[
        text_column(
            "search-col-name",
            "name",
            TableColumnWidth::Flex(1.0),
            TableAlign::Start,
        ),
        text_column(
            "search-col-date",
            "date",
            TableColumnWidth::Fixed { default: 120.0 },
            TableAlign::Start,
        ),
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Classifieds results table (name + price).
static CLASSIFIEDS_TABLE: TableSpec = TableSpec {
    element: "search-classifieds",
    selection: TableSelectionMode::Single,
    columns: &[
        text_column(
            "search-col-name",
            "name",
            TableColumnWidth::Flex(1.0),
            TableAlign::Start,
        ),
        text_column(
            "search-col-price",
            "price",
            TableColumnWidth::Fixed { default: 80.0 },
            TableAlign::End,
        ),
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 6.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The table spec for a category.
const fn table_spec(category: SearchCategory) -> &'static TableSpec {
    match category {
        SearchCategory::People => &PEOPLE_TABLE,
        SearchCategory::Groups => &GROUPS_TABLE,
        SearchCategory::Places => &PLACES_TABLE,
        SearchCategory::Land => &LAND_TABLE,
        SearchCategory::Events => &EVENTS_TABLE,
        SearchCategory::Classifieds => &CLASSIFIEDS_TABLE,
    }
}

// ---------------------------------------------------------------------------
// Per-category page state.
// ---------------------------------------------------------------------------

/// One category's live results and paging state.
#[derive(Debug)]
struct Page<T> {
    /// The 0-based paging offset of the current page.
    query_start: i32,
    /// The in-flight query id, so a stale reply is ignored.
    pending: Option<QueryId>,
    /// The results currently shown.
    results: Vec<T>,
    /// Whether the last reply filled a whole page (so a Next is meaningful).
    filled: bool,
    /// Bumped whenever `results` changes, driving the row rebind + count.
    revision: u64,
}

impl<T> Default for Page<T> {
    /// An empty first page (not `#[derive]`d so `T` need not be `Default`).
    fn default() -> Self {
        Self {
            query_start: 0,
            pending: None,
            results: Vec::new(),
            filled: false,
            revision: 0,
        }
    }
}

impl<T> Page<T> {
    /// Fold a fresh reply in.
    fn set_results(&mut self, results: Vec<T>) {
        self.filled = results.len() >= PAGE_SIZE_USIZE;
        self.results = results;
        self.revision = self.revision.wrapping_add(1);
    }
}

/// The floater's live state.
#[derive(Resource, Default)]
struct SearchState {
    /// The committed query text (trimmed).
    query: String,
    /// The active tab.
    active: SearchTab,
    /// The grid's search website base, from `SimulatorFeatures` (OpenSim); `None`
    /// falls back to the SL search site.
    web_search_base: Option<String>,
    /// The Places category filter.
    places_category: ParcelCategory,
    /// The Classifieds category filter.
    classified_category: ClassifiedCategory,
    /// The Land sale-type filter.
    land_sale: LandSaleFilter,
    /// The Land sort order.
    land_sort: LandSort,
    /// The Land price limit (0 = no limit), read from the price field on search.
    land_price_limit: i32,
    /// The Land area limit (0 = no limit), read from the area field on search.
    land_area_limit: i32,
    /// The Events date mode.
    events_mode: EventsMode,
    /// The Events day offset from today (used in `ByDate` mode).
    events_day: i32,
    /// The Events category index (into [`EVENT_CATEGORIES`]).
    events_category: usize,
    /// The People page.
    people: Page<DirPeopleResult>,
    /// The Groups page.
    groups: Page<DirGroupResult>,
    /// The Places page.
    places: Page<DirPlaceResult>,
    /// The Land page.
    land: Page<DirLandResult>,
    /// The Events page.
    events: Page<DirEventResult>,
    /// The Classifieds page.
    classifieds: Page<DirClassifiedResult>,
}

impl SearchState {
    /// The category's result count.
    const fn result_count(&self, category: SearchCategory) -> usize {
        match category {
            SearchCategory::People => self.people.results.len(),
            SearchCategory::Groups => self.groups.results.len(),
            SearchCategory::Places => self.places.results.len(),
            SearchCategory::Land => self.land.results.len(),
            SearchCategory::Events => self.events.results.len(),
            SearchCategory::Classifieds => self.classifieds.results.len(),
        }
    }

    /// The category's paging offset.
    const fn query_start(&self, category: SearchCategory) -> i32 {
        match category {
            SearchCategory::People => self.people.query_start,
            SearchCategory::Groups => self.groups.query_start,
            SearchCategory::Places => self.places.query_start,
            SearchCategory::Land => self.land.query_start,
            SearchCategory::Events => self.events.query_start,
            SearchCategory::Classifieds => self.classifieds.query_start,
        }
    }

    /// Set the category's paging offset.
    const fn set_query_start(&mut self, category: SearchCategory, value: i32) {
        match category {
            SearchCategory::People => self.people.query_start = value,
            SearchCategory::Groups => self.groups.query_start = value,
            SearchCategory::Places => self.places.query_start = value,
            SearchCategory::Land => self.land.query_start = value,
            SearchCategory::Events => self.events.query_start = value,
            SearchCategory::Classifieds => self.classifieds.query_start = value,
        }
    }

    /// Whether the category's last page was full.
    const fn filled(&self, category: SearchCategory) -> bool {
        match category {
            SearchCategory::People => self.people.filled,
            SearchCategory::Groups => self.groups.filled,
            SearchCategory::Places => self.places.filled,
            SearchCategory::Land => self.land.filled,
            SearchCategory::Events => self.events.filled,
            SearchCategory::Classifieds => self.classifieds.filled,
        }
    }
}

// ---------------------------------------------------------------------------
// The selected-result detail subject.
// ---------------------------------------------------------------------------

/// What the shared details pane currently shows.
#[derive(Debug, Clone, Default)]
enum DetailSubject {
    /// Nothing selected.
    #[default]
    None,
    /// A person — name from the reply, full detail from an [`AvatarProperties`]
    /// reply.
    Person {
        /// The matched avatar.
        agent: AgentKey,
        /// The avatar's display name.
        name: String,
        /// The full avatar properties, once the reply arrives.
        props: Option<AvatarProperties>,
    },
    /// A group — name + members from the reply, full detail from a
    /// [`GroupProfile`] reply.
    Group {
        /// The matched group.
        group: GroupKey,
        /// The group's name.
        name: String,
        /// The group's member count (from the search reply).
        members: i32,
        /// The full group profile, once the reply arrives.
        profile: Option<GroupProfile>,
    },
    /// A parcel (Places / Land) — filled from a [`ParcelDetails`] reply.
    Parcel {
        /// The parcel to look up.
        parcel_id: ParcelKey,
        /// The parcel name from the result row (shown before the detail arrives).
        name: String,
        /// The full parcel detail, once the reply arrives.
        details: Option<ParcelDetails>,
    },
    /// An event — filled from an [`EventInfo`] reply.
    Event {
        /// The event to look up.
        event_id: EventId,
        /// The event name from the result row.
        name: String,
        /// The full event info, once the reply arrives.
        info: Option<EventInfo>,
    },
    /// A classified — filled from a [`ClassifiedInfo`] reply.
    Classified {
        /// The classified to look up.
        classified_id: ClassifiedKey,
        /// The classified name from the result row.
        name: String,
        /// The full classified info, once the reply arrives.
        info: Option<ClassifiedInfo>,
    },
}

/// The details-pane state: what is selected and the local "notify me" toggle.
#[derive(Resource, Default)]
struct SearchDetail {
    /// The current subject.
    subject: DetailSubject,
    /// Whether the local Events "notify me" toggle is on.
    notify: bool,
    /// Bumped when `subject` changes, so the pane repaints.
    revision: u64,
    /// The snapshot texture last requested, so a stable snapshot is not re-fetched
    /// every frame.
    snapshot_requested: Option<TextureKey>,
    /// Texture requests whose decode is awaited: `(id, image node)`.
    pending_textures: Vec<(TextureKey, Entity)>,
}

impl SearchDetail {
    /// Replace the subject and bump the revision.
    fn set(&mut self, subject: DetailSubject) {
        self.subject = subject;
        self.notify = false;
        self.revision = self.revision.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Entity handles.
// ---------------------------------------------------------------------------

/// Marks a category's paging + count row, so its label / buttons are found.
#[derive(Component, Clone, Copy)]
struct SearchCount(SearchCategory);

/// Marks a Prev / Next paging button.
#[derive(Component, Clone, Copy)]
struct PagingButton {
    /// The category the button pages.
    category: SearchCategory,
    /// Whether it advances to the next page (else the previous).
    forward: bool,
}

/// Marks a maturity / online checkbox's box node.
#[derive(Component, Clone, Copy)]
struct SearchCheckboxBox;

/// Which value node of the details pane a [`Text`] is.
#[derive(Component, Clone, Copy)]
enum DetailField {
    /// The bold title (name).
    Title,
    /// The first aux line.
    Aux1,
    /// The second aux line.
    Aux2,
    /// The location line.
    Location,
    /// The description block.
    Description,
}

/// Which action a details-pane button performs.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum DetailAction {
    /// Open the selected person / group profile.
    Profile,
    /// Open a direct IM with the selected person.
    Message,
    /// Offer friendship to the selected person.
    AddFriend,
    /// Open the selected group's chat.
    JoinChat,
    /// Join the selected group.
    JoinGroup,
    /// Teleport to the selected location.
    Teleport,
    /// Show the selected location on the world map.
    ShowMap,
    /// Toggle the Events reminder.
    Remind,
}

/// The per-category table + paging handles.
#[derive(Clone, Copy)]
struct CatTable {
    /// The table root (carries [`TableState`]).
    root: Entity,
    /// The virtualized viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The last selection revision the detail sync acted on.
    last_selection: u64,
    /// The last page revision the row rebind acted on.
    last_page: u64,
}

/// The floater's retained entity handles.
#[derive(Resource)]
pub(crate) struct SearchUi {
    /// The shared query text field.
    search_field: Entity,
    /// The category tab strip.
    tab_strip: Entity,
    /// The embedded web-search browser view.
    web_view: Entity,
    /// The Places category combo.
    places_combo: Entity,
    /// The Classifieds category combo.
    classified_combo: Entity,
    /// The Land sale-type combo.
    land_sale_combo: Entity,
    /// The Land sort combo.
    land_sort_combo: Entity,
    /// The Land price-limit field.
    land_price_field: Entity,
    /// The Land area-limit field.
    land_area_field: Entity,
    /// The Events date-mode radio group.
    events_mode_radio: Entity,
    /// The Events category combo.
    events_category_combo: Entity,
    /// The Events day-offset label.
    events_day_label: Entity,
    /// The details pane root (shown only when a subject is set).
    detail_panel: Entity,
    /// The details pane's snapshot image box.
    detail_snapshot: Entity,
    /// The People table.
    people: CatTable,
    /// The Groups table.
    groups: CatTable,
    /// The Places table.
    places: CatTable,
    /// The Land table.
    land: CatTable,
    /// The Events table.
    events: CatTable,
    /// The Classifieds table.
    classifieds: CatTable,
}

impl SearchUi {
    /// The per-category table handles (mutable, to track last-seen revisions).
    const fn table_mut(&mut self, category: SearchCategory) -> &mut CatTable {
        match category {
            SearchCategory::People => &mut self.people,
            SearchCategory::Groups => &mut self.groups,
            SearchCategory::Places => &mut self.places,
            SearchCategory::Land => &mut self.land,
            SearchCategory::Events => &mut self.events,
            SearchCategory::Classifieds => &mut self.classifieds,
        }
    }
}

/// Marks a pooled row of a category's table, so the rebind finds its category.
#[derive(Component, Clone, Copy)]
struct SearchRow(SearchCategory);

// ---------------------------------------------------------------------------
// Settings.
// ---------------------------------------------------------------------------

/// Register the floater's maturity / online settings.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        SEARCH_SECTION,
        SETTING_ONLINE_ONLY,
        SettingValue::Bool(false),
        "Restrict People search to online avatars",
    );
    settings.register_in(
        SEARCH_SECTION,
        SETTING_LAND_ASCENDING,
        SettingValue::Bool(false),
        "Sort Land search results ascending",
    );
    // Per-tab maturity: General / Moderate on by default, Adult off (the
    // reference keeps a separate maturity filter per category).
    for category in CATEGORY_ORDER {
        if let Some([general, moderate, adult]) = maturity_settings(category) {
            settings.register_in(
                SEARCH_SECTION,
                general,
                SettingValue::Bool(true),
                "Include General (PG) results in this search category",
            );
            settings.register_in(
                SEARCH_SECTION,
                moderate,
                SettingValue::Bool(true),
                "Include Moderate (Mature) results in this search category",
            );
            settings.register_in(
                SEARCH_SECTION,
                adult,
                SettingValue::Bool(false),
                "Include Adult results in this search category",
            );
        }
    }
}

/// The per-category maturity settings `[General, Moderate, Adult]`, or `None` for
/// People (which the reference gives no maturity filter).
const fn maturity_settings(category: SearchCategory) -> Option<[&'static str; 3]> {
    match category {
        SearchCategory::People => None,
        SearchCategory::Groups => {
            Some(["SearchGroupsPG", "SearchGroupsMature", "SearchGroupsAdult"])
        }
        SearchCategory::Places => {
            Some(["SearchPlacesPG", "SearchPlacesMature", "SearchPlacesAdult"])
        }
        SearchCategory::Land => Some(["SearchLandPG", "SearchLandMature", "SearchLandAdult"]),
        SearchCategory::Events => {
            Some(["SearchEventsPG", "SearchEventsMature", "SearchEventsAdult"])
        }
        SearchCategory::Classifieds => Some([
            "SearchClassifiedsPG",
            "SearchClassifiedsMature",
            "SearchClassifiedsAdult",
        ]),
    }
}

/// Read a boolean setting's effective value with a default.
fn setting_bool(settings: Option<&ViewerSettings>, name: &str, default: bool) -> bool {
    settings.map_or(default, |viewer| {
        viewer.store().get_bool(name).unwrap_or(default)
    })
}

/// The maturity-inclusion flags the checkboxes currently select.
fn maturity_flags(category: SearchCategory, settings: Option<&ViewerSettings>) -> DirFindFlags {
    let Some([general, moderate, adult]) = maturity_settings(category) else {
        return DirFindFlags::NONE;
    };
    let mut flags = DirFindFlags::NONE;
    if setting_bool(settings, general, true) {
        flags = flags.union(DirFindFlags::INC_PG);
    }
    if setting_bool(settings, moderate, true) {
        flags = flags.union(DirFindFlags::INC_MATURE);
    }
    if setting_bool(settings, adult, false) {
        flags = flags.union(DirFindFlags::INC_ADULT);
    }
    flags
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin owning the Search floater.
pub(crate) struct SearchFloaterPlugin;

impl Plugin for SearchFloaterPlugin {
    /// Register the state and systems, and spawn the (hidden) floater.
    fn build(&self, app: &mut App) {
        app.init_resource::<SearchState>()
            .init_resource::<SearchDetail>()
            .add_systems(
                Startup,
                spawn_search_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    bridge_search_tabs,
                    apply_filter_combos,
                    apply_events_mode,
                    enter_to_search,
                    ingest_search_replies,
                    rebind_search_rows,
                    sync_search_detail,
                    ingest_detail_replies,
                    update_detail_pane,
                    request_detail_snapshot,
                    poll_detail_snapshot,
                    update_search_counts,
                    drive_search_checkbox_visual,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// Spawn the floater's chrome (hidden); the left column (query, maturity,
/// tabs) and the shared details pane are built once, on the first open
/// ([`DeferredFloaterContent`]) — which also defers the embedded web-search
/// browser view (and so its CEF browser) until the window is actually used.
fn spawn_search_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: SEARCH_FLOATER_ID,
            title: "Search".to_owned(),
            position: Vec2::new(200.0, 80.0),
            default_size: Some(Vec2::new(720.0, 460.0)),
            min_size: Some(Vec2::new(560.0, 340.0)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("search-title"));
    let builder = commands.register_system(build_search_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build (see the chrome spawn above): the split, the
/// category tabs, the filter panels and the details pane, ending with the
/// [`SearchUi`] insert whose appearance wakes the `Option<Res<SearchUi>>`
/// consumers.
#[expect(
    clippy::too_many_lines,
    reason = "one floater built once: the seven tab panels and the shared detail pane are laid \
              out inline so every retained handle is gathered in one place"
)]
fn build_search_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    // The content splits into the left column and the details pane.
    let split = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..row(Val::Px(8.0))
            },
            ChildOf(handle.content),
        ))
        .id();
    let left = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(6.0))
            },
            ChildOf(split),
        ))
        .id();

    // The shared query row.
    let query_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(left),
        ))
        .id();
    spawn_label(&mut commands, query_row, "search-query-label", LABEL_COLOR);
    let search_field = spawn_text_input(
        &mut commands,
        query_row,
        &TextInputSpec {
            font_size: FONT,
            width_glyphs: 20.0,
            tab_index: 2,
            fill: true,
            ..TextInputSpec::new("search-query", TextInputKind::Line)
        },
    );
    let search_button = spawn_text_button(&mut commands, query_row, "search-button", 3);
    commands.entity(search_button).observe(on_search_press);

    // Maturity is per-tab (spawned into each category's filter row), matching the
    // reference — so there is no shared maturity row here.

    // The tabs.
    let labels: Vec<String> = [
        "search-tab-web",
        "search-tab-people",
        "search-tab-groups",
        "search-tab-places",
        "search-tab-land",
        "search-tab-events",
        "search-tab-classifieds",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let tabs: TabContainerHandle = spawn_tab_container(
        &mut commands,
        left,
        &TabSpec {
            element: "search-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: FONT,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);

    // The Web panel: an embedded browser.
    let web_panel = tabs.panels.first().copied().unwrap_or(Entity::PLACEHOLDER);
    let web_view = spawn_browser_view(
        &mut commands,
        web_panel,
        &BrowserViewSpec {
            initial_url: SL_SEARCH_URL.to_owned(),
            isolated: false,
            tab_index: 0,
            fixed_height: None,
        },
    );

    // The six directory panels.
    let filters = spawn_category_panels(&mut commands, &tabs);

    // The details pane.
    let detail = spawn_detail_pane(&mut commands, split);

    commands.insert_resource(SearchUi {
        search_field,
        tab_strip: tabs.strip,
        web_view,
        places_combo: filters.places_combo,
        classified_combo: filters.classified_combo,
        land_sale_combo: filters.land_sale_combo,
        land_sort_combo: filters.land_sort_combo,
        land_price_field: filters.land_price_field,
        land_area_field: filters.land_area_field,
        events_mode_radio: filters.events_mode_radio,
        events_category_combo: filters.events_category_combo,
        events_day_label: filters.events_day_label,
        detail_panel: detail.panel,
        detail_snapshot: detail.snapshot,
        people: filters.people,
        groups: filters.groups,
        places: filters.places,
        land: filters.land,
        events: filters.events,
        classifieds: filters.classifieds,
    });
}

/// The filter / table handles gathered while building the category panels.
struct PanelHandles {
    /// The Places category combo.
    places_combo: Entity,
    /// The Classifieds category combo.
    classified_combo: Entity,
    /// The Land sale-type combo.
    land_sale_combo: Entity,
    /// The Land sort combo.
    land_sort_combo: Entity,
    /// The Land price-limit field.
    land_price_field: Entity,
    /// The Land area-limit field.
    land_area_field: Entity,
    /// The Events date-mode radio group.
    events_mode_radio: Entity,
    /// The Events category combo.
    events_category_combo: Entity,
    /// The Events day-offset label.
    events_day_label: Entity,
    /// The People table.
    people: CatTable,
    /// The Groups table.
    groups: CatTable,
    /// The Places table.
    places: CatTable,
    /// The Land table.
    land: CatTable,
    /// The Events table.
    events: CatTable,
    /// The Classifieds table.
    classifieds: CatTable,
}

/// Build each directory panel (filters + table + paging), returning the handles.
fn spawn_category_panels(commands: &mut Commands, tabs: &TabContainerHandle) -> PanelHandles {
    let mut places_combo = Entity::PLACEHOLDER;
    let mut classified_combo = Entity::PLACEHOLDER;
    let mut land_sale_combo = Entity::PLACEHOLDER;
    let mut land_sort_combo = Entity::PLACEHOLDER;
    let mut land_price_field = Entity::PLACEHOLDER;
    let mut land_area_field = Entity::PLACEHOLDER;
    let mut events_mode_radio = Entity::PLACEHOLDER;
    let mut events_category_combo = Entity::PLACEHOLDER;
    let mut events_day_label = Entity::PLACEHOLDER;
    let mut tables: [Option<CatTable>; 6] = [None; 6];

    for (order_index, category) in CATEGORY_ORDER.into_iter().enumerate() {
        // Panel slot: category order + 1 (Web is slot 0).
        let Some(panel) = tabs.panels.get(order_index.saturating_add(1)).copied() else {
            continue;
        };
        match category {
            SearchCategory::People => {
                let filters = spawn_filter_row(commands, panel);
                spawn_search_checkbox(commands, filters, SETTING_ONLINE_ONLY, "search-online-only");
            }
            SearchCategory::Places => {
                let filters = spawn_filter_row(commands, panel);
                spawn_label(commands, filters, "search-label-category", SECONDARY_COLOR);
                places_combo = spawn_category_combo(
                    commands,
                    filters,
                    "search-places-category",
                    &PLACES_CATEGORY_LABELS,
                );
            }
            SearchCategory::Classifieds => {
                let filters = spawn_filter_row(commands, panel);
                spawn_label(commands, filters, "search-label-category", SECONDARY_COLOR);
                classified_combo = spawn_category_combo(
                    commands,
                    filters,
                    "search-classified-category",
                    &CLASSIFIED_CATEGORY_LABELS,
                );
            }
            SearchCategory::Land => {
                let filters = spawn_filter_row(commands, panel);
                spawn_label(commands, filters, "search-label-saletype", SECONDARY_COLOR);
                land_sale_combo = spawn_category_combo(
                    commands,
                    filters,
                    "search-land-saletype",
                    &LAND_SALE_LABELS,
                );
                spawn_label(commands, filters, "search-label-sort", SECONDARY_COLOR);
                land_sort_combo =
                    spawn_category_combo(commands, filters, "search-land-sort", &LAND_SORT_LABELS);
                spawn_search_checkbox(
                    commands,
                    filters,
                    SETTING_LAND_ASCENDING,
                    "search-land-ascending",
                );
                // A second row for the numeric price / area limits.
                let limits = spawn_filter_row(commands, panel);
                spawn_label(commands, limits, "search-label-price-max", SECONDARY_COLOR);
                land_price_field = spawn_limit_field(commands, limits, "search-land-price");
                spawn_label(commands, limits, "search-label-area-min", SECONDARY_COLOR);
                land_area_field = spawn_limit_field(commands, limits, "search-land-area");
            }
            SearchCategory::Events => {
                let events = spawn_events_filters(commands, panel);
                events_mode_radio = events.0;
                events_category_combo = events.1;
                events_day_label = events.2;
            }
            SearchCategory::Groups => {}
        }
        // Per-tab maturity row (every category but People has one).
        if maturity_settings(category).is_some() {
            let maturity = spawn_filter_row(commands, panel);
            spawn_label(commands, maturity, "search-maturity-label", SECONDARY_COLOR);
            spawn_maturity_checkboxes(commands, maturity, category);
        }
        spawn_paging_row(commands, panel, category);
        let handle = spawn_table(commands, panel, table_spec(category));
        if let Some(slot) = tables.get_mut(order_index) {
            *slot = Some(CatTable {
                root: handle.root,
                viewport: handle.viewport,
                last_selection: 0,
                last_page: 0,
            });
        }
    }

    let take = |index: usize| {
        tables
            .get(index)
            .and_then(|slot| *slot)
            .unwrap_or(CatTable {
                root: Entity::PLACEHOLDER,
                viewport: Entity::PLACEHOLDER,
                last_selection: 0,
                last_page: 0,
            })
    };
    PanelHandles {
        places_combo,
        classified_combo,
        land_sale_combo,
        land_sort_combo,
        land_price_field,
        land_area_field,
        events_mode_radio,
        events_category_combo,
        events_day_label,
        people: take(0),
        groups: take(1),
        places: take(2),
        land: take(3),
        events: take(4),
        classifieds: take(5),
    }
}

/// The details-pane handles.
struct DetailHandles {
    /// The details pane root.
    panel: Entity,
    /// The snapshot image box.
    snapshot: Entity,
}

/// The details pane's snapshot image box width, in logical pixels.
const SNAPSHOT_EDGE: f32 = DETAIL_WIDTH - 16.0;

/// Build the shared details pane (title, aux lines, location, description, action
/// buttons), returning its root. Hidden until a subject is selected.
fn spawn_detail_pane(commands: &mut Commands, parent: Entity) -> DetailHandles {
    let panel = commands
        .spawn((
            Node {
                width: Val::Px(DETAIL_WIDTH),
                flex_shrink: 0.0,
                display: Display::None,
                overflow: Overflow::scroll_y(),
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(6.0))
            },
            ScrollPosition::default(),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.20)),
            ChildOf(parent),
        ))
        .id();
    // The snapshot box: a fixed 4:3 image the poll system fills once decoded.
    let snapshot = commands
        .spawn((
            Node {
                width: Val::Px(SNAPSHOT_EDGE),
                height: Val::Px(SNAPSHOT_EDGE * 0.75),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.09, 0.11)),
            ChildOf(panel),
        ))
        .id();
    spawn_detail_value(commands, panel, DetailField::Title, LABEL_COLOR, 15.0);
    spawn_detail_value(commands, panel, DetailField::Aux1, SECONDARY_COLOR, FONT);
    spawn_detail_value(commands, panel, DetailField::Aux2, SECONDARY_COLOR, FONT);
    spawn_detail_value(
        commands,
        panel,
        DetailField::Location,
        SECONDARY_COLOR,
        FONT,
    );
    spawn_detail_value(commands, panel, DetailField::Description, LABEL_COLOR, FONT);
    // The action buttons row.
    let actions = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    for (action, key) in [
        (DetailAction::Profile, "search-detail-profile"),
        (DetailAction::Message, "search-detail-message"),
        (DetailAction::AddFriend, "search-detail-friend"),
        (DetailAction::JoinChat, "search-detail-chat"),
        (DetailAction::JoinGroup, "search-detail-join"),
        (DetailAction::Teleport, "search-detail-teleport"),
        (DetailAction::ShowMap, "search-detail-map"),
        (DetailAction::Remind, "search-detail-remind"),
    ] {
        let button = spawn_action_button(commands, actions, key, action);
        commands.entity(button).observe(on_detail_action);
    }
    DetailHandles { panel, snapshot }
}

/// Spawn a details-pane value node with the given field marker.
fn spawn_detail_value(
    commands: &mut Commands,
    parent: Entity,
    field: DetailField,
    color: Color,
    size: f32,
) {
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(size),
        TextColor(color),
        Node {
            max_width: Val::Px(DETAIL_WIDTH - 16.0),
            ..default()
        },
        field,
        ChildOf(parent),
    ));
}

/// The Events filter row: a mode radio, day buttons + label, and a category combo.
/// Returns `(mode radio, category combo, day label)`.
fn spawn_events_filters(commands: &mut Commands, panel: Entity) -> (Entity, Entity, Entity) {
    let filters = spawn_filter_row(commands, panel);
    let mode_labels = [
        "search-events-current".to_owned(),
        "search-events-bydate".to_owned(),
    ];
    let mode_radio = spawn_radio_group(
        commands,
        filters,
        &RadioSpec {
            element: "search-events-mode",
            labels: &mode_labels,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            layout: RadioLayout::Row,
            translate_labels: true,
        },
    );
    // Day stepper: ‹ label ›.
    spawn_events_day_button(commands, filters, false);
    let day_label = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT),
            TextColor(LABEL_COLOR),
            ChildOf(filters),
        ))
        .id();
    spawn_events_day_button(commands, filters, true);
    spawn_label(commands, filters, "search-label-category", SECONDARY_COLOR);
    let category_combo = spawn_events_category_combo(commands, filters);
    (mode_radio, category_combo, day_label)
}

/// Spawn an Events day-stepper button (`‹` back / `›` forward).
fn spawn_events_day_button(commands: &mut Commands, parent: Entity, forward: bool) {
    let glyph = if forward { "\u{203a}" } else { "\u{2039}" };
    let button = commands
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            EventsDayButton { forward },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(glyph.to_owned()),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.entity(button).observe(on_events_day_press);
}

/// Spawn the Events category combo over the [`EVENT_CATEGORIES`] labels.
fn spawn_events_category_combo(commands: &mut Commands, parent: Entity) -> Entity {
    let labels: Vec<String> = EVENT_CATEGORIES
        .iter()
        .map(|(_number, label)| (*label).to_owned())
        .collect();
    spawn_combo(
        commands,
        parent,
        &ComboSpec {
            element: "search-events-category",
            labels: &labels,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            translate_labels: false,
        },
    )
}

/// Marks an Events day-stepper button.
#[derive(Component, Clone, Copy)]
struct EventsDayButton {
    /// Whether it steps forward (a later day).
    forward: bool,
}

/// Spawn a filter row inside a category panel.
fn spawn_filter_row(commands: &mut Commands, panel: Entity) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id()
}

/// Spawn a category panel's paging row: Prev, a count read-out, and Next.
fn spawn_paging_row(commands: &mut Commands, panel: Entity, category: SearchCategory) {
    let paging = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            ChildOf(panel),
        ))
        .id();
    spawn_paging_button(commands, paging, category, false);
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(FONT),
        TextColor(SECONDARY_COLOR),
        SearchCount(category),
        ChildOf(paging),
    ));
    spawn_paging_button(commands, paging, category, true);
}

/// Spawn one Prev / Next paging button.
fn spawn_paging_button(
    commands: &mut Commands,
    parent: Entity,
    category: SearchCategory,
    forward: bool,
) {
    let key = if forward {
        "search-next"
    } else {
        "search-prev"
    };
    let button = commands
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            PagingButton { category, forward },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.entity(button).observe(on_paging_press);
}

/// Spawn a translated static label.
fn spawn_label(commands: &mut Commands, parent: Entity, key: &'static str, color: Color) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT),
        TextColor(color),
        ChildOf(parent),
    ));
}

/// Spawn a bordered translated push button; the caller attaches the observer.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    tab_index: i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            bevy::input_focus::tab_navigation::TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Spawn a details-pane action button with a translated label and its action tag.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    action: DetailAction,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            action,
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Spawn a category's three maturity checkboxes (General / Moderate / Adult).
fn spawn_maturity_checkboxes(commands: &mut Commands, parent: Entity, category: SearchCategory) {
    if let Some([general, moderate, adult]) = maturity_settings(category) {
        spawn_search_checkbox(commands, parent, general, "search-maturity-general");
        spawn_search_checkbox(commands, parent, moderate, "search-maturity-moderate");
        spawn_search_checkbox(commands, parent, adult, "search-maturity-adult");
    }
}

/// Spawn a settings-bound checkbox with a box visual and a translated label.
fn spawn_search_checkbox(
    commands: &mut Commands,
    parent: Entity,
    setting: &'static str,
    label_key: &'static str,
) {
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
        bound_checkbox(SettingBinding::global(setting)),
        Node {
            width: Val::Px(CHECK_SIZE),
            height: Val::Px(CHECK_SIZE),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BorderColor::all(CHECK_BORDER),
        BackgroundColor(CHECK_OFF),
        bevy::input_focus::tab_navigation::TabIndex(0),
        SearchCheckboxBox,
        Pickable::default(),
        ChildOf(row_node),
    ));
    spawn_label(commands, row_node, label_key, LABEL_COLOR);
}

/// Spawn a small non-negative-integer limit field (empty = no limit).
fn spawn_limit_field(commands: &mut Commands, parent: Entity, element: &'static str) -> Entity {
    spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: FONT,
            width_glyphs: 6.0,
            tab_index: 0,
            ..TextInputSpec::new(element, TextInputKind::NonNegativeInteger)
        },
    )
}

/// Spawn a filter combo over literal English labels, returning the anchor.
fn spawn_category_combo(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    labels: &[&str],
) -> Entity {
    let owned: Vec<String> = labels.iter().map(|label| (*label).to_owned()).collect();
    spawn_combo(
        commands,
        parent,
        &ComboSpec {
            element,
            labels: &owned,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            translate_labels: false,
        },
    )
}

// ---------------------------------------------------------------------------
// Query dispatch.
// ---------------------------------------------------------------------------

/// The reference events query text: `"<day>|<category>|<text>"`, where day is
/// `"u"` in current mode or the integer day offset in by-date mode, and category
/// is the wire category number.
fn events_query_text(mode: EventsMode, day_offset: i32, category: u32, text: &str) -> String {
    let day = match mode {
        EventsMode::Current => "u".to_owned(),
        EventsMode::ByDate => day_offset.to_string(),
    };
    format!("{day}|{category}|{text}")
}

/// The search button: run a new search on the active category from page 0.
#[expect(
    clippy::too_many_arguments,
    reason = "one observer that runs the active search — it needs the query field, the state, \
              the settings for maturity, and the browser view + surfaces for the Web tab"
)]
fn on_search_press(
    press: On<Pointer<Press>>,
    ui: Option<Res<SearchUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<SearchState>,
    settings: Option<Res<ViewerSettings>>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
    identity: Res<SlIdentity>,
    ui_locale: Res<UiLocale>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    run_new_search(
        &ui,
        &fields,
        &mut state,
        settings.as_deref(),
        &views,
        &surfaces,
        identity.session_id,
        &ui_locale.lang.to_string(),
        &mut commands,
    );
}

/// Commit the field's text and run the active tab (a directory query, or a web
/// navigation for the Web tab).
#[expect(
    clippy::too_many_arguments,
    reason = "commits the field and runs the active tab: the UI table, query field, state, \
              settings, the browser view + surfaces for the Web tab, the session id and language \
              for the templated SL search URL, and the command writer"
)]
fn run_new_search(
    ui: &SearchUi,
    fields: &Query<&EditableText>,
    state: &mut SearchState,
    settings: Option<&ViewerSettings>,
    views: &Query<&BrowserView>,
    surfaces: &MediaSurfaces,
    session_id: Option<Uuid>,
    language: &str,
    commands: &mut MessageWriter<SlCommand>,
) {
    let text = fields
        .get(ui.search_field)
        .map(|field| field.value().to_string().trim().to_owned())
        .unwrap_or_default();
    state.query = text;
    state.land_price_limit = read_limit(fields, ui.land_price_field);
    state.land_area_limit = read_limit(fields, ui.land_area_field);
    match state.active.category() {
        Some(category) => {
            state.set_query_start(category, 0);
            dispatch_query(category, state, settings, commands);
        }
        None => navigate_web(ui, state, views, surfaces, session_id, language),
    }
}

/// Read a numeric limit field as a non-negative `i32` (0 when empty / invalid).
fn read_limit(fields: &Query<&EditableText>, field: Entity) -> i32 {
    fields
        .get(field)
        .ok()
        .and_then(|editable| editable.value().to_string().trim().parse::<i32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(0)
}

/// Navigate the embedded web view to the grid's search site for the current
/// query.
///
/// On **OpenSim** (the grid announced a `search-server-url` via
/// `SimulatorFeatures`, kept in [`SearchState::web_search_base`]) this keeps the
/// old behaviour — substitute the query into the base URL. On **Second Life**
/// (no per-grid base) it builds the reference viewer's full templated search
/// URL ([`build_sl_search_url`]) — the `search.[GRID]/viewer/?…&sid=[SESSION_ID]`
/// form the SL search site expects, carrying the login session id so the Web
/// tab opens signed in (paired with the OpenID cookie auto-login,
/// `viewer-web-openid-auth`). Falls back to the bare search site before login.
fn navigate_web(
    ui: &SearchUi,
    state: &SearchState,
    views: &Query<&BrowserView>,
    surfaces: &MediaSurfaces,
    session_id: Option<Uuid>,
    language: &str,
) {
    let url = match (state.web_search_base.as_deref(), session_id) {
        // OpenSim: substitute the query into the grid-provided base URL.
        (Some(base), _) => {
            if state.query.is_empty() {
                base.to_owned()
            } else {
                let escaped: String =
                    url::form_urlencoded::byte_serialize(state.query.as_bytes()).collect();
                let separator = if base.contains('?') { '&' } else { '?' };
                format!("{base}{separator}q={escaped}")
            }
        }
        // Second Life, logged in: the full templated search URL.
        (None, Some(session_id)) => build_sl_search_url(session_id, &state.query, language),
        // Second Life, not yet logged in: the bare search site.
        (None, None) => SL_SEARCH_URL.to_owned(),
    };
    if let Ok(view) = views.get(ui.web_view)
        && let Some(slot) = view.surface.and_then(|id| surfaces.get(id))
    {
        slot.surface.navigate(&url);
    }
}

/// The standard-collection query parameters the reference searches by default
/// on the Web tab (people / places / events / groups / destinations), each a
/// repeated `collection_chosen` following `search_type=standard`
/// (`llfloatersearch.cpp`, the `[COLLECTION]` substitution).
const SL_SEARCH_COLLECTIONS: &str = "&collection_chosen=people&collection_chosen=places\
&collection_chosen=events&collection_chosen=groups&collection_chosen=destinations";

/// Build the Second Life search site's templated URL for the Web tab
/// (`SearchURL` in the reference: `https://search.[GRID]/viewer/?query_term=…\
/// &search_type=standard[COLLECTION]&maturity=…&lang=…&sid=[SESSION_ID]`).
///
/// `session_id` is the login session id ([`SlIdentity::session_id`], the
/// reference's `[SESSION_ID]` = `gAgent.getSessionID()`); `language` is the
/// active UI language tag. The maturity is left at the widest (`gma`): the Web
/// tab carries no maturity checkboxes and the site applies the signed-in
/// account's own content settings.
fn build_sl_search_url(session_id: Uuid, query: &str, language: &str) -> String {
    let escaped: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    format!(
        "https://search.secondlife.com/viewer/?query_term={escaped}\
&search_type=standard{SL_SEARCH_COLLECTIONS}&maturity=gma&lang={language}&sid={session_id}"
    )
}

/// Send the wire query for `category` at its current paging offset.
fn dispatch_query(
    category: SearchCategory,
    state: &mut SearchState,
    settings: Option<&ViewerSettings>,
    commands: &mut MessageWriter<SlCommand>,
) {
    if category.needs_query_text() && state.query.is_empty() && category != SearchCategory::Events {
        return;
    }
    let maturity = maturity_flags(category, settings);
    let query_text = state.query.clone();
    let query_id = QueryId::from(Uuid::new_v4());
    let query_start = state.query_start(category);
    match category {
        SearchCategory::People => {
            let mut flags = DirFindFlags::PEOPLE.union(maturity);
            if setting_bool(settings, SETTING_ONLINE_ONLY, false) {
                flags = flags.union(DirFindFlags::ONLINE);
            }
            state.people.pending = Some(query_id);
            commands.write(SlCommand(Command::DirFindQuery {
                query_id,
                query_text,
                flags,
                query_start,
            }));
        }
        SearchCategory::Groups => {
            state.groups.pending = Some(query_id);
            commands.write(SlCommand(Command::DirFindQuery {
                query_id,
                query_text,
                flags: DirFindFlags::GROUPS.union(maturity),
                query_start,
            }));
        }
        SearchCategory::Events => {
            let category_number = EVENT_CATEGORIES
                .get(state.events_category)
                .map_or(0, |(number, _label)| *number);
            let text = events_query_text(
                state.events_mode,
                state.events_day,
                category_number,
                &query_text,
            );
            state.events.pending = Some(query_id);
            commands.write(SlCommand(Command::DirFindQuery {
                query_id,
                query_text: text,
                flags: DirFindFlags::DATE_EVENTS.union(maturity),
                query_start,
            }));
        }
        SearchCategory::Places => {
            let places_category = state.places_category;
            state.places.pending = Some(query_id);
            commands.write(SlCommand(Command::DirPlacesQuery {
                query_id,
                query_text,
                flags: maturity.union(DirFindFlags::DWELL_SORT),
                category: places_category,
                sim_name: String::new(),
                query_start,
            }));
        }
        SearchCategory::Land => {
            let mut flags = DirFindFlags::FOR_SALE
                .union(maturity)
                .union(state.land_sort.to_flag());
            if setting_bool(settings, SETTING_LAND_ASCENDING, false) {
                flags = flags.union(DirFindFlags::SORT_ASC);
            }
            let price = state.land_price_limit;
            let area = state.land_area_limit;
            if price > 0 {
                flags = flags.union(DirFindFlags::LIMIT_BY_PRICE);
            }
            if area > 0 {
                flags = flags.union(DirFindFlags::LIMIT_BY_AREA);
            }
            let search_type = state.land_sale.to_search_type();
            state.land.pending = Some(query_id);
            commands.write(SlCommand(Command::DirLandQuery {
                query_id,
                flags,
                search_type,
                price,
                area,
                query_start,
            }));
        }
        SearchCategory::Classifieds => {
            let classified_category = state.classified_category;
            state.classifieds.pending = Some(query_id);
            commands.write(SlCommand(Command::DirClassifiedQuery {
                query_id,
                query_text,
                flags: maturity,
                category: classified_category,
                query_start,
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// Systems.
// ---------------------------------------------------------------------------

/// Track the tab strip's active tab into the state and toggle the details pane's
/// visibility (hidden on the Web tab).
fn bridge_search_tabs(
    ui: Option<Res<SearchUi>>,
    strips: Query<&TabStrip, Changed<TabStrip>>,
    mut state: ResMut<SearchState>,
    mut nodes: Query<&mut Node>,
    detail: Res<SearchDetail>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(strip) = strips.get(ui.tab_strip) else {
        return;
    };
    let Some(tab) = TAB_ORDER.get(strip.active).copied() else {
        return;
    };
    if state.active != tab {
        state.active = tab;
    }
    if let Ok(mut node) = nodes.get_mut(ui.detail_panel) {
        let show = tab.category().is_some() && !matches!(detail.subject, DetailSubject::None);
        node.display = if show { Display::Flex } else { Display::None };
    }
}

/// Apply a combo pick to its filter.
fn apply_filter_combos(
    ui: Option<Res<SearchUi>>,
    mut changes: MessageReader<ComboChanged>,
    mut state: ResMut<SearchState>,
) {
    let Some(ui) = ui else {
        changes.clear();
        return;
    };
    for change in changes.read() {
        if change.combo == ui.places_combo {
            state.places_category = PLACES_CATEGORIES
                .get(change.active)
                .copied()
                .unwrap_or_default();
        } else if change.combo == ui.classified_combo {
            state.classified_category = CLASSIFIED_CATEGORIES
                .get(change.active)
                .copied()
                .unwrap_or_default();
        } else if change.combo == ui.land_sale_combo {
            state.land_sale = LAND_SALE_ORDER
                .get(change.active)
                .copied()
                .unwrap_or_default();
        } else if change.combo == ui.land_sort_combo {
            state.land_sort = LAND_SORT_ORDER
                .get(change.active)
                .copied()
                .unwrap_or_default();
        } else if change.combo == ui.events_category_combo {
            state.events_category = change.active;
        }
    }
}

/// Reflect the Events date-mode radio into the state.
fn apply_events_mode(
    ui: Option<Res<SearchUi>>,
    radios: Query<&RadioSelection, Changed<RadioSelection>>,
    mut state: ResMut<SearchState>,
) {
    let Some(ui) = ui else {
        return;
    };
    if let Ok(selection) = radios.get(ui.events_mode_radio) {
        state.events_mode = if selection.active == 0 {
            EventsMode::Current
        } else {
            EventsMode::ByDate
        };
    }
}

/// An Events day-stepper press: shift the day offset (and switch to by-date mode).
fn on_events_day_press(
    press: On<Pointer<Press>>,
    buttons: Query<&EventsDayButton>,
    mut state: ResMut<SearchState>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity) else {
        return;
    };
    state.events_mode = EventsMode::ByDate;
    state.events_day = if button.forward {
        state.events_day.saturating_add(1)
    } else {
        state.events_day.saturating_sub(1)
    };
}

/// Run a new search when `Enter` is pressed while the query field is focused.
#[expect(
    clippy::too_many_arguments,
    reason = "the Enter shortcut mirrors the Search button: query field, focus, state, settings, \
              and the browser view + surfaces for the Web tab"
)]
fn enter_to_search(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    ui: Option<Res<SearchUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<SearchState>,
    settings: Option<Res<ViewerSettings>>,
    views: Query<&BrowserView>,
    surfaces: NonSend<MediaSurfaces>,
    identity: Res<SlIdentity>,
    ui_locale: Res<UiLocale>,
    mut commands: MessageWriter<SlCommand>,
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
    run_new_search(
        &ui,
        &fields,
        &mut state,
        settings.as_deref(),
        &views,
        &surfaces,
        identity.session_id,
        &ui_locale.lang.to_string(),
        &mut commands,
    );
    keyboard.clear_just_pressed(KeyCode::Enter);
}

/// A Prev / Next press: step the category's page and re-run its query.
fn on_paging_press(
    press: On<Pointer<Press>>,
    buttons: Query<&PagingButton>,
    mut state: ResMut<SearchState>,
    settings: Option<Res<ViewerSettings>>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity) else {
        return;
    };
    let category = button.category;
    let start = state.query_start(category);
    if button.forward {
        if !state.filled(category) {
            return;
        }
        state.set_query_start(category, start.saturating_add(PAGE_SIZE));
    } else {
        if start <= 0 {
            return;
        }
        state.set_query_start(category, start.saturating_sub(PAGE_SIZE).max(0));
    }
    dispatch_query(category, &mut state, settings.as_deref(), &mut commands);
}

/// Fold a directory reply into its category's page (Places sorted dwell-desc).
fn ingest_search_replies(mut events: MessageReader<SlEvent>, mut state: ResMut<SearchState>) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::SimulatorFeatures(features) => {
                if let Some(url) = features
                    .open_sim_extras
                    .as_ref()
                    .and_then(|extras| extras.search_server_url.as_ref())
                {
                    state.web_search_base = Some(url.to_string());
                }
            }
            SlSessionEvent::DirPeopleReply { query_id, results }
                if pending_matches(state.people.pending, *query_id) =>
            {
                state.people.pending = None;
                state.people.set_results(results.clone());
            }
            SlSessionEvent::DirGroupsReply { query_id, results }
                if pending_matches(state.groups.pending, *query_id) =>
            {
                state.groups.pending = None;
                state.groups.set_results(results.clone());
            }
            SlSessionEvent::DirEventsReply {
                query_id, results, ..
            } if pending_matches(state.events.pending, *query_id) => {
                state.events.pending = None;
                state.events.set_results(results.clone());
            }
            SlSessionEvent::DirPlacesReply {
                query_id, results, ..
            } if pending_matches(state.places.pending, *query_id) => {
                state.places.pending = None;
                let mut sorted = results.clone();
                sorted.sort_by(|a, b| b.dwell.total_cmp(&a.dwell));
                state.places.set_results(sorted);
            }
            SlSessionEvent::DirLandReply { query_id, results }
                if pending_matches(state.land.pending, *query_id) =>
            {
                state.land.pending = None;
                state.land.set_results(results.clone());
            }
            SlSessionEvent::DirClassifiedReply {
                query_id, results, ..
            } if pending_matches(state.classifieds.pending, *query_id) => {
                state.classifieds.pending = None;
                state.classifieds.set_results(results.clone());
            }
            _other => {}
        }
    }
}

/// Whether an in-flight query id matches a reply's echoed id.
fn pending_matches(pending: Option<QueryId>, reply: Uuid) -> bool {
    pending.is_some_and(|query| query.get() == reply)
}

/// Keep each table's item count current and rebind its pooled rows from the
/// results; clear the table's selection when its page is replaced.
#[expect(
    clippy::too_many_arguments,
    reason = "the row pump reads the UI handles, the results, the child/added/list/table queries \
              and the row cells + texts it binds — one coherent per-frame rebind"
)]
fn rebind_search_rows(
    mut ui: Option<ResMut<SearchUi>>,
    state: Res<SearchState>,
    child_of: Query<&ChildOf>,
    added: Query<Entity, Added<VirtualRow>>,
    mut lists: Query<&mut VirtualList>,
    mut tables: Query<&mut TableState>,
    rows: Query<(&VirtualRow, &SearchRow, &TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut commands: Commands,
) {
    let Some(ui) = ui.as_deref_mut() else {
        return;
    };
    for category in CATEGORY_ORDER {
        let handle = *ui.table_mut(category);
        // On a new page, reset the item count + selection.
        let page_rev = state_page_revision(&state, category);
        if handle.last_page != page_rev {
            ui.table_mut(category).last_page = page_rev;
            if let Ok(mut list) = lists.get_mut(handle.viewport) {
                list.item_count = state.result_count(category);
            }
            if let Ok(mut table) = tables.get_mut(handle.root) {
                table.clear_selection();
            }
        }
        // Adopt freshly-pooled rows for this table's viewport.
        for row_entity in &added {
            if child_of
                .get(row_entity)
                .is_ok_and(|parent| parent.parent() == handle.viewport)
            {
                spawn_table_row(&mut commands, row_entity, handle.root, table_spec(category));
                commands.entity(row_entity).insert(SearchRow(category));
            }
        }
    }
    // Bind every pooled row's cells from its result.
    for (row, tag, cells) in &rows {
        let Some(index) = row.index else {
            continue;
        };
        bind_row(tag.0, index, cells, &state, &mut texts);
    }
}

/// A category's page revision.
const fn state_page_revision(state: &SearchState, category: SearchCategory) -> u64 {
    match category {
        SearchCategory::People => state.people.revision,
        SearchCategory::Groups => state.groups.revision,
        SearchCategory::Places => state.places.revision,
        SearchCategory::Land => state.land.revision,
        SearchCategory::Events => state.events.revision,
        SearchCategory::Classifieds => state.classifieds.revision,
    }
}

/// Bind one row's cells from `results[index]` of its category.
fn bind_row(
    category: SearchCategory,
    index: usize,
    cells: &TableRowCells,
    state: &SearchState,
    texts: &mut Query<(&mut Text, &mut TextColor)>,
) {
    let mut set = |column: usize, value: String, color: Color| {
        if let Some(cell) = cells.cell(column) {
            set_table_cell(texts, cell, &value, color);
        }
    };
    match category {
        SearchCategory::People => {
            if let Some(result) = state.people.results.get(index) {
                set(
                    0,
                    format!("{} {}", result.first_name, result.last_name),
                    LINK_COLOR,
                );
            }
        }
        SearchCategory::Groups => {
            if let Some(result) = state.groups.results.get(index) {
                set(0, result.group_name.clone(), LINK_COLOR);
                set(1, result.members.to_string(), SECONDARY_COLOR);
            }
        }
        SearchCategory::Places => {
            if let Some(result) = state.places.results.get(index) {
                set(0, result.name.clone(), LABEL_COLOR);
                set(1, format!("{:.0}", result.dwell), SECONDARY_COLOR);
            }
        }
        SearchCategory::Land => {
            if let Some(result) = state.land.results.get(index) {
                set(0, result.name.clone(), LABEL_COLOR);
                set(1, land_price(result), SECONDARY_COLOR);
                set(2, format!("{}", result.actual_area), SECONDARY_COLOR);
                set(3, land_ppm(result), SECONDARY_COLOR);
                let land_type = if result.auction { "Auction" } else { "Sale" };
                set(4, land_type.to_owned(), SECONDARY_COLOR);
            }
        }
        SearchCategory::Events => {
            if let Some(result) = state.events.results.get(index) {
                set(0, result.name.clone(), LABEL_COLOR);
                set(1, result.date.clone(), SECONDARY_COLOR);
            }
        }
        SearchCategory::Classifieds => {
            if let Some(result) = state.classifieds.results.get(index) {
                set(0, result.name.clone(), LABEL_COLOR);
                set(1, format!("{}", result.price_for_listing), SECONDARY_COLOR);
            }
        }
    }
}

/// A land result's price cell: its asking price, or a dash when not for sale.
fn land_price(result: &DirLandResult) -> String {
    match &result.sale_price {
        Some(amount) => format!("{amount}"),
        None => "\u{2014}".to_owned(),
    }
}

/// A land result's price-per-square-metre cell (L$/m²), or a dash.
fn land_ppm(result: &DirLandResult) -> String {
    let area = u64::from(result.actual_area.get());
    result
        .sale_price
        .as_ref()
        .and_then(|amount| amount.0.checked_div(area))
        .map_or_else(|| "\u{2014}".to_owned(), |ppm| ppm.to_string())
}

/// When the active category's table selection moves, set the detail subject from
/// the selected result and fire its secondary detail request.
fn sync_search_detail(
    mut ui: Option<ResMut<SearchUi>>,
    state: Res<SearchState>,
    tables: Query<&TableState>,
    mut detail: ResMut<SearchDetail>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui.as_deref_mut() else {
        return;
    };
    let Some(category) = state.active.category() else {
        return;
    };
    let handle = *ui.table_mut(category);
    let Ok(table) = tables.get(handle.root) else {
        return;
    };
    let revision = table.selection_revision();
    if handle.last_selection == revision {
        return;
    }
    ui.table_mut(category).last_selection = revision;
    let subject = match table.primary_selected() {
        Some(index) => subject_for(category, index, &state, &mut commands),
        None => DetailSubject::None,
    };
    detail.set(subject);
}

/// Build the detail subject for the selected row, firing any secondary request.
fn subject_for(
    category: SearchCategory,
    index: usize,
    state: &SearchState,
    commands: &mut MessageWriter<SlCommand>,
) -> DetailSubject {
    match category {
        SearchCategory::People => {
            state
                .people
                .results
                .get(index)
                .map_or(DetailSubject::None, |result| {
                    commands.write(SlCommand(Command::RequestAvatarProperties(result.agent_id)));
                    DetailSubject::Person {
                        agent: result.agent_id,
                        name: format!("{} {}", result.first_name, result.last_name),
                        props: None,
                    }
                })
        }
        SearchCategory::Groups => {
            state
                .groups
                .results
                .get(index)
                .map_or(DetailSubject::None, |result| {
                    commands.write(SlCommand(Command::RequestGroupProfile(result.group_id)));
                    DetailSubject::Group {
                        group: result.group_id,
                        name: result.group_name.clone(),
                        members: result.members,
                        profile: None,
                    }
                })
        }
        SearchCategory::Places => {
            state
                .places
                .results
                .get(index)
                .map_or(DetailSubject::None, |result| {
                    commands.write(SlCommand(Command::RequestParcelInfo {
                        parcel_id: result.parcel_id,
                    }));
                    DetailSubject::Parcel {
                        parcel_id: result.parcel_id,
                        name: result.name.clone(),
                        details: None,
                    }
                })
        }
        SearchCategory::Land => {
            state
                .land
                .results
                .get(index)
                .map_or(DetailSubject::None, |result| {
                    commands.write(SlCommand(Command::RequestParcelInfo {
                        parcel_id: result.parcel_id,
                    }));
                    DetailSubject::Parcel {
                        parcel_id: result.parcel_id,
                        name: result.name.clone(),
                        details: None,
                    }
                })
        }
        SearchCategory::Events => {
            state
                .events
                .results
                .get(index)
                .map_or(DetailSubject::None, |result| {
                    commands.write(SlCommand(Command::EventInfoRequest {
                        event_id: result.event_id,
                    }));
                    DetailSubject::Event {
                        event_id: result.event_id,
                        name: result.name.clone(),
                        info: None,
                    }
                })
        }
        SearchCategory::Classifieds => {
            state
                .classifieds
                .results
                .get(index)
                .map_or(DetailSubject::None, |result| {
                    commands.write(SlCommand(Command::RequestClassifiedInfo(
                        result.classified_id,
                    )));
                    DetailSubject::Classified {
                        classified_id: result.classified_id,
                        name: result.name.clone(),
                        info: None,
                    }
                })
        }
    }
}

/// Fold a secondary detail reply into the current subject.
fn ingest_detail_replies(mut events: MessageReader<SlEvent>, mut detail: ResMut<SearchDetail>) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AvatarProperties(properties) => {
                if let DetailSubject::Person {
                    agent, props: slot, ..
                } = &mut detail.subject
                    && *agent == properties.avatar_id
                {
                    *slot = Some((**properties).clone());
                    detail.revision = detail.revision.wrapping_add(1);
                }
            }
            SlSessionEvent::GroupProfileReceived(profile) => {
                if let DetailSubject::Group {
                    group,
                    profile: slot,
                    ..
                } = &mut detail.subject
                    && *group == profile.group_id
                {
                    *slot = Some((**profile).clone());
                    detail.revision = detail.revision.wrapping_add(1);
                }
            }
            SlSessionEvent::ParcelDetails(details) => {
                if let DetailSubject::Parcel {
                    parcel_id,
                    details: slot,
                    ..
                } = &mut detail.subject
                    && *parcel_id == details.parcel_id
                {
                    *slot = Some(details.clone());
                    detail.revision = detail.revision.wrapping_add(1);
                }
            }
            SlSessionEvent::EventInfoReply { info } => {
                if let DetailSubject::Event {
                    event_id,
                    info: slot,
                    ..
                } = &mut detail.subject
                    && *event_id == info.event_id
                {
                    *slot = Some(info.clone());
                    detail.revision = detail.revision.wrapping_add(1);
                }
            }
            SlSessionEvent::ClassifiedInfo(info) => {
                if let DetailSubject::Classified {
                    classified_id,
                    info: slot,
                    ..
                } = &mut detail.subject
                    && *classified_id == info.classified_id
                {
                    *slot = Some((**info).clone());
                    detail.revision = detail.revision.wrapping_add(1);
                }
            }
            _other => {}
        }
    }
}

/// Populate the details pane from the current subject: the value fields, the
/// button visibility, and the pane's own visibility.
fn update_detail_pane(
    detail: Res<SearchDetail>,
    state: Res<SearchState>,
    ui: Option<Res<SearchUi>>,
    mut fields: Query<(&DetailField, &mut Text)>,
    mut buttons: Query<(&DetailAction, &mut Node), Without<DetailField>>,
    mut panels: Query<&mut Node, (Without<DetailField>, Without<DetailAction>)>,
) {
    if !detail.is_changed() {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    for (field, mut text) in &mut fields {
        let value = detail_field_value(*field, &detail.subject);
        if text.0 != value {
            text.0 = value;
        }
    }
    for (action, mut node) in &mut buttons {
        let show = detail_action_visible(*action, &detail.subject);
        node.display = if show { Display::Flex } else { Display::None };
    }
    if let Ok(mut node) = panels.get_mut(ui.detail_panel) {
        let show =
            !matches!(detail.subject, DetailSubject::None) && state.active.category().is_some();
        node.display = if show { Display::Flex } else { Display::None };
    }
}

/// The text for a details-pane value field from the subject.
fn detail_field_value(field: DetailField, subject: &DetailSubject) -> String {
    match subject {
        DetailSubject::None => String::new(),
        DetailSubject::Person { name, props, .. } => match field {
            DetailField::Title => name.clone(),
            DetailField::Aux1 => props
                .as_ref()
                .map_or_else(String::new, |p| format!("Born {}", p.born_on)),
            DetailField::Aux2 => props.as_ref().map_or_else(String::new, |p| {
                if p.partner_id.is_some() {
                    "Partnered".to_owned()
                } else {
                    String::new()
                }
            }),
            DetailField::Description => props
                .as_ref()
                .map_or_else(String::new, |p| p.about_text.clone()),
            DetailField::Location => String::new(),
        },
        DetailSubject::Group {
            name,
            members,
            profile,
            ..
        } => match field {
            DetailField::Title => name.clone(),
            DetailField::Aux1 => {
                let count = profile.as_ref().map_or(*members, |p| p.member_count);
                format!("{count} members")
            }
            DetailField::Aux2 => profile.as_ref().map_or_else(String::new, |p| {
                if p.open_enrollment {
                    "Open enrollment".to_owned()
                } else {
                    "Invitation only".to_owned()
                }
            }),
            DetailField::Description => profile
                .as_ref()
                .map_or_else(String::new, |p| p.charter.clone()),
            DetailField::Location => String::new(),
        },
        DetailSubject::Parcel { name, details, .. } => match field {
            DetailField::Title => name.clone(),
            DetailField::Aux1 => details
                .as_ref()
                .map_or_else(String::new, |d| format!("Traffic: {:.0}", d.dwell)),
            DetailField::Aux2 => details
                .as_ref()
                .map_or_else(String::new, |d| format!("{}", d.actual_area)),
            DetailField::Location => details.as_ref().map_or_else(String::new, |d| {
                location_label(d.sim_name.as_ref(), &d.global_position)
            }),
            DetailField::Description => details
                .as_ref()
                .map_or_else(String::new, |d| d.description.clone()),
        },
        DetailSubject::Event { name, info, .. } => match field {
            DetailField::Title => name.clone(),
            DetailField::Aux1 => info.as_ref().map_or_else(String::new, |i| {
                format!("{} \u{2022} {} min", i.date, i.duration)
            }),
            DetailField::Aux2 => info.as_ref().map_or_else(String::new, event_cover_label),
            DetailField::Location => info.as_ref().map_or_else(String::new, |i| {
                location_label(i.sim_name.as_ref(), &i.global_position)
            }),
            DetailField::Description => info
                .as_ref()
                .map_or_else(String::new, |i| i.description.clone()),
        },
        DetailSubject::Classified { name, info, .. } => match field {
            DetailField::Title => name.clone(),
            DetailField::Aux1 => info
                .as_ref()
                .map_or_else(String::new, |i| format!("{}", i.category)),
            DetailField::Aux2 => info
                .as_ref()
                .map_or_else(String::new, |i| format!("{} / wk", i.price_for_listing)),
            DetailField::Location => info.as_ref().map_or_else(String::new, |i| {
                location_label(i.sim_name.as_ref(), &i.pos_global)
            }),
            DetailField::Description => info
                .as_ref()
                .map_or_else(String::new, |i| i.description.clone()),
        },
    }
}

/// An event's cover-charge aux line.
fn event_cover_label(info: &EventInfo) -> String {
    match &info.amount {
        Some(amount) => format!("Cover: {amount}"),
        None => "Free".to_owned(),
    }
}

/// Whether a details-pane action button is shown for the subject.
fn detail_action_visible(action: DetailAction, subject: &DetailSubject) -> bool {
    match action {
        DetailAction::Profile => {
            matches!(
                subject,
                DetailSubject::Person { .. } | DetailSubject::Group { .. }
            )
        }
        DetailAction::Message | DetailAction::AddFriend => {
            matches!(subject, DetailSubject::Person { .. })
        }
        DetailAction::JoinChat | DetailAction::JoinGroup => {
            matches!(subject, DetailSubject::Group { .. })
        }
        DetailAction::Teleport | DetailAction::ShowMap => {
            subject_global_position(subject).is_some()
        }
        DetailAction::Remind => matches!(subject, DetailSubject::Event { .. }),
    }
}

/// The selected subject's snapshot texture, once its detail has arrived.
fn subject_snapshot(subject: &DetailSubject) -> Option<TextureKey> {
    match subject {
        DetailSubject::None | DetailSubject::Event { .. } => None,
        DetailSubject::Person { props, .. } => props.as_ref().map(|p| p.image_id),
        DetailSubject::Group { profile, .. } => profile.as_ref().and_then(|p| p.insignia_id),
        DetailSubject::Parcel { details, .. } => details.as_ref().and_then(|d| d.snapshot_id),
        DetailSubject::Classified { info, .. } => info.as_ref().and_then(|i| i.snapshot_id),
    }
}

/// The selected subject's global position, once its detail has arrived.
fn subject_global_position(subject: &DetailSubject) -> Option<GlobalCoordinates> {
    match subject {
        DetailSubject::Parcel { details, .. } => details.as_ref().map(|d| d.global_position),
        DetailSubject::Event { info, .. } => info.as_ref().map(|i| i.global_position),
        DetailSubject::Classified { info, .. } => info.as_ref().map(|i| i.pos_global),
        DetailSubject::None | DetailSubject::Person { .. } | DetailSubject::Group { .. } => None,
    }
}

/// The "region (x, y, z)" location line for a global position.
fn location_label(
    sim_name: Option<&sl_client_bevy::RegionName>,
    pos_global: &GlobalCoordinates,
) -> String {
    let mut label = sim_name.map(ToString::to_string).unwrap_or_default();
    if let Some((_grid, local)) = pos_global.split() {
        let position = format!("({:.0}, {:.0}, {:.0})", local.x(), local.y(), local.z());
        label = if label.is_empty() {
            position
        } else {
            format!("{label} {position}")
        };
    }
    label
}

/// A details-pane action button press.
#[expect(
    clippy::too_many_arguments,
    reason = "the shared details-pane actions fan out to every target: profiles, conversations, \
              the world map, and the session for teleport / friendship / join"
)]
fn on_detail_action(
    press: On<Pointer<Press>>,
    actions: Query<&DetailAction>,
    mut detail: ResMut<SearchDetail>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut avatar_profiles: MessageWriter<OpenAvatarProfile>,
    mut group_profiles: MessageWriter<OpenGroupProfile>,
    mut conversations: MessageWriter<OpenConversation>,
    mut world_map: MessageWriter<OpenWorldMap>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    match action {
        DetailAction::Profile => match &detail.subject {
            DetailSubject::Person { agent, .. } => {
                avatar_profiles.write(OpenAvatarProfile { agent: *agent });
            }
            DetailSubject::Group { group, .. } => {
                group_profiles.write(OpenGroupProfile { group: *group });
            }
            _other => {}
        },
        DetailAction::Message => {
            if let DetailSubject::Person { agent, .. } = &detail.subject {
                conversations.write(OpenConversation {
                    key: ConversationKey::Direct(*agent),
                });
            }
        }
        DetailAction::AddFriend => {
            if let DetailSubject::Person { agent, .. } = &detail.subject {
                sl_commands.write(SlCommand(Command::OfferFriendship {
                    to_agent_id: *agent,
                    message: String::new(),
                }));
            }
        }
        DetailAction::JoinChat => {
            if let DetailSubject::Group { group, .. } = &detail.subject {
                conversations.write(OpenConversation {
                    key: ConversationKey::Group(*group),
                });
            }
        }
        DetailAction::JoinGroup => {
            if let DetailSubject::Group { group, .. } = &detail.subject {
                sl_commands.write(SlCommand(Command::JoinGroup(*group)));
            }
        }
        DetailAction::Teleport => {
            if let Some(position) = subject_global_position(&detail.subject)
                && let Some((region_handle, local)) = teleport_destination(&position)
            {
                sl_commands.write(SlCommand(Command::Teleport {
                    region_handle,
                    position: local,
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                }));
            }
        }
        DetailAction::ShowMap => {
            if let Some(position) = subject_global_position(&detail.subject) {
                world_map.write(OpenWorldMap {
                    east: position.x(),
                    north: position.y(),
                });
            }
        }
        DetailAction::Remind => {
            if let DetailSubject::Event { event_id, .. } = detail.subject {
                detail.notify = !detail.notify;
                let command = if detail.notify {
                    Command::EventNotificationAddRequest { event_id }
                } else {
                    Command::EventNotificationRemoveRequest { event_id }
                };
                sl_commands.write(SlCommand(command));
            }
        }
    }
}

/// Split a global position into a region handle + local coords for a teleport.
fn teleport_destination(
    pos_global: &GlobalCoordinates,
) -> Option<(RegionHandle, RegionCoordinates)> {
    let (grid, local) = pos_global.split()?;
    Some((RegionHandle::from_grid(grid.x(), grid.y()), local))
}

/// Keep each category's count read-out current, and the Events day label.
fn update_search_counts(
    state: Res<SearchState>,
    ui: Option<Res<SearchUi>>,
    mut counts: Query<(&SearchCount, &mut Text), Without<DetailField>>,
    mut labels: Query<&mut Text, (Without<SearchCount>, Without<DetailField>)>,
) {
    if !state.is_changed() {
        return;
    }
    for (count, mut text) in &mut counts {
        let category = count.0;
        let len = state.result_count(category);
        let wanted = if len == 0 {
            "No results".to_owned()
        } else {
            let start = usize::try_from(state.query_start(category)).unwrap_or(0);
            let first = start.saturating_add(1);
            let last = start.saturating_add(len);
            format!("Showing {first}\u{2013}{last}")
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    if let Some(ui) = ui
        && let Ok(mut text) = labels.get_mut(ui.events_day_label)
    {
        let wanted = events_day_label(state.events_mode, state.events_day);
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// The Events day-offset label ("Today" / "+2 days" / …).
fn events_day_label(mode: EventsMode, day: i32) -> String {
    match (mode, day) {
        (EventsMode::Current, _any) => "Upcoming".to_owned(),
        (EventsMode::ByDate, 0) => "Today".to_owned(),
        (EventsMode::ByDate, 1) => "Tomorrow".to_owned(),
        (EventsMode::ByDate, -1) => "Yesterday".to_owned(),
        (EventsMode::ByDate, other) if other > 0 => format!("+{other} days"),
        (EventsMode::ByDate, other) => format!("{other} days"),
    }
}

/// Colour each maturity / online checkbox's box from its `Checked` state.
fn drive_search_checkbox_visual(
    mut boxes: Query<(&mut BackgroundColor, Has<Checked>), With<SearchCheckboxBox>>,
) {
    for (mut fill, checked) in &mut boxes {
        let wanted = if checked { CHECK_ON } else { CHECK_OFF };
        if fill.0 != wanted {
            fill.0 = wanted;
        }
    }
}

/// Request the selected subject's snapshot texture when it changes, clearing the
/// image while a new one loads (skips the nil texture).
fn request_detail_snapshot(
    mut detail: ResMut<SearchDetail>,
    ui: Option<Res<SearchUi>>,
    textures: Option<ResMut<TextureManager>>,
    mut commands: Commands,
) {
    if !detail.is_changed() {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let wanted =
        subject_snapshot(&detail.subject).filter(|id| *id != TextureKey::from(Uuid::nil()));
    if wanted == detail.snapshot_requested {
        return;
    }
    detail.snapshot_requested = wanted;
    commands.entity(ui.detail_snapshot).remove::<ImageNode>();
    if let Some(id) = wanted {
        if let Some(mut textures) = textures {
            textures.request_boosted(id, AVATAR_BOOST_PRIORITY);
        }
        detail.pending_textures.push((id, ui.detail_snapshot));
    }
}

/// Swap a decoded snapshot into its image box once the texture arrives (the
/// `poll_profile_textures` pattern).
fn poll_detail_snapshot(
    mut detail: ResMut<SearchDetail>,
    textures: Option<Res<TextureManager>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    if detail.pending_textures.is_empty() {
        return;
    }
    let Some(textures) = textures else {
        return;
    };
    let pending = std::mem::take(&mut detail.pending_textures);
    let mut still = Vec::new();
    for (id, node) in pending {
        if let Some(decoded) = textures.decoded(id) {
            let handle = images.add(to_bevy_image(decoded));
            commands.entity(node).insert(ImageNode::new(handle));
        } else {
            still.push((id, node));
        }
    }
    detail.pending_textures = still;
}

#[cfg(test)]
mod tests {

    use super::{
        CATEGORY_ORDER, EventsMode, LandSaleFilter, LandSort, PAGE_SIZE_USIZE, Page,
        SearchCategory, SearchTab, build_sl_search_url, events_query_text,
    };
    use pretty_assertions::assert_eq;

    /// The tab order leads with Web, then the six categories.
    #[test]
    fn tab_order_leads_with_web() {
        assert_eq!(super::TAB_ORDER.first().copied(), Some(SearchTab::Web));
        assert_eq!(CATEGORY_ORDER.len(), 6);
        assert_eq!(SearchTab::Web.category(), None);
        assert_eq!(SearchTab::People.category(), Some(SearchCategory::People));
    }

    /// Only Land runs without query text.
    #[test]
    fn only_land_skips_query_text() {
        for category in CATEGORY_ORDER {
            let expected = category != SearchCategory::Land;
            assert_eq!(category.needs_query_text(), expected);
        }
    }

    /// A page counts a reply as full only at the page size.
    #[test]
    fn full_page_detected_at_page_size() {
        let mut page: Page<u8> = Page::default();
        page.set_results(vec![0_u8; PAGE_SIZE_USIZE - 1]);
        assert!(!page.filled);
        page.set_results(vec![0_u8; PAGE_SIZE_USIZE]);
        assert!(page.filled);
        assert_eq!(page.revision, 2);
    }

    /// The Land sort default (the first combo option) is Price, matching the
    /// reference.
    #[test]
    fn land_sort_default_is_price() {
        assert_eq!(
            super::LAND_SORT_ORDER.first().copied(),
            Some(LandSort::Price)
        );
        assert_eq!(LandSort::default(), LandSort::Price);
    }

    /// The events query text is the reference's `"<day>|<category>|<text>"`.
    #[test]
    fn events_query_text_is_pipe_delimited() {
        // Current mode uses the "u" day token.
        assert_eq!(
            events_query_text(EventsMode::Current, 3, 0, "jazz"),
            "u|0|jazz"
        );
        // By-date mode uses the day offset.
        assert_eq!(
            events_query_text(EventsMode::ByDate, 2, 20, "party"),
            "2|20|party"
        );
    }

    /// The land sale filter maps to the wire sale-type mask.
    #[test]
    fn land_sale_maps() {
        use sl_client_bevy::LandSearchType;
        assert_eq!(LandSaleFilter::All.to_search_type(), LandSearchType::ALL);
        assert_eq!(
            LandSaleFilter::Estate.to_search_type(),
            LandSearchType::ESTATE
        );
    }

    /// The Second Life templated Web-tab URL carries the session id, the
    /// URL-escaped query, the standard collections and the language tag in the
    /// reference's `SearchURL` shape.
    #[test]
    fn sl_search_url_is_templated() {
        // 0x2222… renders as 22222222-2222-2222-2222-222222222222, no fallible
        // parse (the workspace denies `expect`/`unwrap` in tests too).
        let session = super::Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222);
        let url = build_sl_search_url(session, "cool stuff", "en");
        assert_eq!(
            url,
            "https://search.secondlife.com/viewer/?query_term=cool+stuff\
&search_type=standard&collection_chosen=people&collection_chosen=places\
&collection_chosen=events&collection_chosen=groups&collection_chosen=destinations\
&maturity=gma&lang=en&sid=22222222-2222-2222-2222-222222222222"
        );
    }
}
