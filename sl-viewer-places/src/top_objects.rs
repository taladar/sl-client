//! The **Top Scripts** and **Top Colliders** floaters
//! (`viewer-region-top-objects`): the estate tool that asks a region which of
//! its objects cost it the most — script time, or collisions — and acts on what
//! comes back.
//!
//! Reached from the Region / Estate floater's Debug tab ("Get Top Colliders…" /
//! "Get Top Scripts…", [`crate::about_region`]), which is where the reference
//! puts it too (`LLPanelRegionDebugInfo::onClickTopScripts`).
//!
//! # Two windows, one per region
//!
//! The reference has a single `top_objects` floater that `setMode` switches
//! between the two reports, retitling itself. Ours is **two window kinds** —
//! comparing "what is running" against "what is colliding" is a real thing to
//! want side by side, and a window that silently becomes a different report is a
//! surprise — and each kind is **instanced per region**, keyed exactly as the
//! About Region window that opened it is (`about_region::region_key`, which is
//! crate-internal). Walk across a border, open the Debug tab again, and the
//! neighbouring region gets its own report rather than overwriting the one being
//! read.
//!
//! A subject-keyed floater persists nothing and is not restored at start-up, so
//! a report never comes back after a re-login — which is right: it describes a
//! region at a moment.
//!
//! A window keeps showing the report it was given after the agent leaves its
//! region, and stops being able to ask for or act on anything (its own
//! `is_current`, the freeze About Land and About Region apply too): every write
//! goes out on the *current* circuit, so a request made from a window about a
//! region the agent has left would be answered by the region it is in.
//!
//! # One request, several replies
//!
//! The wire is a UDP `LandStatRequest` ([`Command::RequestLandStat`]) answered by
//! one or more `LandStatReply`s ([`SlSessionEvent::LandStatReply`]) — the report
//! is as long as the region wants it to be, and a long one arrives split across
//! messages. So a request **clears** the list and every reply **appends** to it,
//! exactly as the reference's `handleReply` does; the `TotalObjectCount` the
//! reply carries is the region's own count, which is what the summary line
//! states rather than the number of rows that fitted.
//!
//! The **reply comes back over the CAPS event queue**, not as the UDP packet the
//! request went out as: every simulator with an event queue answers this over it
//! (OpenSim's `SendLandStatReply` only falls back to the packet when it has
//! none, and the message is marked `UDPDeprecated` for the same reason). The
//! session decodes both forms into the one event, and only the event-queue form
//! carries the `DataExtended` half of a row — the parcel, the rez date, the
//! script memory and the URL count.
//!
//! A reply names no window: it carries the report type and nothing else. It is
//! routed to the window of that kind about the region the agent is in — the only
//! window that could have asked, since a request only goes out on the current
//! circuit.
//!
//! # What the score is
//!
//! The number in the first column is the simulator's, shown as the reference
//! shows it (`%0.3f`, unscaled). For the top-scripts report it is **milliseconds
//! of script time**, and how much time it covers is the simulator's business:
//! OpenSim's XEngine sums a script's execution over a rolling 30-second window
//! (`ScriptInstance::MeasurementWindow`), so a script in a tight loop reads in
//! the hundreds or thousands, not as a per-frame or per-cent figure. For the
//! top-colliders report it is a collision count. Neither is normalised here —
//! a viewer that rescaled it would be inventing a unit the region never stated.
//!
//! # The filters are the region's, not ours
//!
//! The three filter rows do not filter the list here: each sends a **new**
//! request carrying a filter string and the flag saying what it applies to
//! (`STAT_FILTER_BY_OBJECT` / `_OWNER` / `_PARCEL_NAME`), and the region answers
//! with a report already narrowed. That is the reference's design, and it is the
//! only one that can work — the region holds the whole report, and what arrives
//! here is only its top rows.
//!
//! As in the reference, the filter and its flag are **consumed by the request**:
//! a plain Refresh afterwards asks for the unfiltered report again.
//!
//! # What the actions do
//!
//! - **Show Beacon** tracks the selected row's position, the reference's
//!   `LLTracker::trackLocation` — the same beacon the world map sets, so the
//!   in-world beam, the minimap and the map all point at the object. A
//!   double-click on a row does the same, as it does there.
//! - **Return Selected** / **Return All** send `ParcelReturnObjects`
//!   ([`Command::ReturnParcelObjects`]) naming the task ids, over the whole
//!   region (`LocalID = -1`) with `RT_NONE` — the exact shape the simulator
//!   recognises as "return these specific objects" rather than "return this
//!   parcel's objects of some kind" (OpenSim's `ReturnObjectsInParcel` takes
//!   that branch on `localID == -1`). The session splits a list too long for one
//!   datagram into several messages.
//! - **Disable Selected** / **Disable All** are the same over
//!   `ParcelDisableObjects` ([`Command::DisableParcelObjects`]): stop the
//!   scripts, leave the objects where they are. The current reference viewer
//!   dropped these buttons but kept their confirmation (`DisableAllTopObjects`),
//!   and stopping a runaway script without taking somebody's build away is the
//!   gentler half of what this window is for.
//!
//! Returning or disabling **all** asks first (`ReturnAllTopObjects` /
//! `DisableAllTopObjects`); the per-selection half does not, as in the
//! reference.
//!
//! # Divergences from the reference
//!
//! - Two windows keyed per region, as above.
//! - The colliders window has **no Memory or URLs column**. The reference keeps
//!   the columns and blanks their headings for that report, because one scroll
//!   list serves both; two window kinds can simply not have them.
//! - The owner is shown as the report spells it. The reference folds it to a
//!   username (`LLCacheName::buildUsername`); the report carries a legacy name
//!   and nothing else, and every other list in this viewer shows those.
//! - `Enter` in a filter field runs **that** filter. In the reference `Enter`
//!   fires the floater's default button, which is Show Beacon — so typing a
//!   filter and pressing Enter moves the beacon.
//! - The reference's Debug-tab buttons ask over an `EstateOwnerMessage`
//!   (`"scripts"` / `"colliders"`) and only the floater's own Refresh sends a
//!   `LandStatRequest`. Both paths produce the same reply, and only the second
//!   is implemented by OpenSim, so both buttons here send the request.
//!
//! Reference (Firestorm, read-only): `llfloatertopobjects.cpp`,
//! `floater_top_objects.xml`, `llfloaterregioninfo.cpp`
//! (`LLPanelRegionDebugInfo::onClickTopScripts`), `llparcel.h` (`RT_NONE`).

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use sl_client_bevy::{
    Command, GlobalCoordinates, GridCoordinates, LandStatExtended, LandStatItem,
    LandStatReportType, LandStatScore, ObjectKey, ParcelReturnType, RegionIdentity,
    RegionLocalParcelId, ScopedParcelId, SlCommand, SlCurrentRegion, SlEvent, SlIdentity,
    SlRegionIdentity, SlSessionEvent,
};

use crate::about_region::region_key;
use crate::floater::{
    Floater, FloaterCaps, FloaterHandle, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen,
    KeyedFloaters, host_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::inventory_properties::format_unix_date;
use crate::notifications::{NotificationResponse, ShowNotification};
use crate::ui::{column, row};
use crate::ui_font::UiFont;
use crate::ui_format::format_duration_units;
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSpec, TableState, set_table_cell, spawn_table, spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{SCROLLBAR_THICKNESS, VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::{MapTracking, TrackTarget};

/// The Top Scripts window's stable [`Floater::id`].
pub const TOP_SCRIPTS_FLOATER_ID: &str = "top-scripts";

/// The Top Colliders window's stable [`Floater::id`].
pub const TOP_COLLIDERS_FLOATER_ID: &str = "top-colliders";

/// The reference's `STAT_FILTER_BY_OWNER`: the filter string names an owner.
const FILTER_BY_OWNER: u32 = 0x0000_0002;

/// The reference's `STAT_FILTER_BY_OBJECT`: the filter string names an object.
const FILTER_BY_OBJECT: u32 = 0x0000_0004;

/// The reference's `STAT_FILTER_BY_PARCEL_NAME`: the filter string names a
/// parcel.
const FILTER_BY_PARCEL_NAME: u32 = 0x0000_0008;

/// The whole region, as the scope of a return / disable — the reference's
/// `LocalID = -1`, which is what makes the simulator read the task-id list
/// instead of the parcel.
const WHOLE_REGION: RegionLocalParcelId = RegionLocalParcelId(-1);

/// The whole region, as the scope of a report request (the reference's
/// `ParcelLocalID = 0`).
const WHOLE_REGION_REPORT: RegionLocalParcelId = RegionLocalParcelId(0);

/// The confirmation before returning every listed object.
const RETURN_ALL_CONFIRM: &str = "ReturnAllTopObjects";

/// The confirmation before disabling every listed object's scripts.
const DISABLE_ALL_CONFIRM: &str = "DisableAllTopObjects";

/// The button an `OK` / `Cancel` confirmation answers with to go ahead.
const CONFIRM_BUTTON: &str = "OK";

/// The floater's body font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A value / label text colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dim label / secondary text colour.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A disabled control's text colour.
const DISABLED_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

/// An action button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// An action button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The list's background.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// One list row's height, in logical pixels.
const ROW_HEIGHT: f32 = 20.0;

/// Seconds within which a second press on the same row is a double-click (the
/// radar's value, so the two lists feel the same).
const DOUBLE_CLICK_SECS: f32 = 0.4;

/// Bytes in a kibibyte — the unit the reference shows script memory in.
const BYTES_PER_KIB: f32 = 1024.0;

/// Column index of the score cell (both kinds).
const COL_SCORE: usize = 0;

/// Column index of the object-name cell (both kinds).
const COL_NAME: usize = 1;

/// Column index of the owner cell (both kinds).
const COL_OWNER: usize = 2;

/// Column index of the location cell (both kinds).
const COL_LOCATION: usize = 3;

/// Column index of the parcel-name cell (both kinds).
const COL_PARCEL: usize = 4;

/// Column index of the rez-date cell (both kinds).
const COL_DATE: usize = 5;

/// Column index of the script-memory cell (the scripts window only).
const COL_MEMORY: usize = 6;

/// Column index of the public-URL-count cell (the scripts window only).
const COL_URLS: usize = 7;

/// The six columns both reports share: the score, the object, its owner, where
/// it is, and the two `DataExtended` fields that mean something either way.
const SHARED_COLUMNS: [TableColumn; 6] = [
    TableColumn {
        // The scripts list overrides this with "Time": the same number is
        // milliseconds there and a collision count here.
        header_key: "top-objects-col-score",
        token: "score",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 56.0 },
        align: TableAlign::End,
        sortable: true,
    },
    TableColumn {
        header_key: "top-objects-col-name",
        token: "name",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Flex(1.0),
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "top-objects-col-owner",
        token: "owner",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 110.0 },
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "top-objects-col-location",
        token: "location",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 112.0 },
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "top-objects-col-parcel",
        token: "parcel",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 100.0 },
        align: TableAlign::Start,
        sortable: true,
    },
    TableColumn {
        header_key: "top-objects-col-date",
        token: "date",
        kind: TableColumnKind::Text,
        width: TableColumnWidth::Fixed { default: 116.0 },
        align: TableAlign::Start,
        sortable: true,
    },
];

/// The report arrives highest-first; saying so out loud puts the arrow where the
/// order already is.
const SCORE_FIRST: [TableSortDefault; 1] = [TableSortDefault {
    column: COL_SCORE,
    ascending: false,
}];

/// The Top Scripts list: the shared columns plus the two script-only ones.
static TOP_SCRIPTS_TABLE: TableSpec = TableSpec {
    element: "top-scripts",
    selection: TableSelectionMode::Multi,
    columns: &[
        TableColumn {
            header_key: "top-objects-col-time",
            // Wider than the colliders' count: a script time reads in mixed
            // units, and `1h 2m 21s 979ms` is what a busy region reports.
            width: TableColumnWidth::Fixed { default: 124.0 },
            ..SHARED_COLUMNS[COL_SCORE]
        },
        SHARED_COLUMNS[COL_NAME],
        SHARED_COLUMNS[COL_OWNER],
        SHARED_COLUMNS[COL_LOCATION],
        SHARED_COLUMNS[COL_PARCEL],
        SHARED_COLUMNS[COL_DATE],
        TableColumn {
            header_key: "top-objects-col-memory",
            token: "memory",
            kind: TableColumnKind::Text,
            // Sized for its **heading**, not its digits: "Memory (KB)" plus
            // the sort arrow is the widest thing the column ever holds, and a
            // heading worth reading is one that fits.
            width: TableColumnWidth::Fixed { default: 92.0 },
            align: TableAlign::End,
            sortable: true,
        },
        TableColumn {
            header_key: "top-objects-col-urls",
            token: "urls",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 48.0 },
            align: TableAlign::End,
            sortable: true,
        },
    ],
    default_sort: &SCORE_FIRST,
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The Top Colliders list: the shared columns alone. A collision report carries
/// no script memory and no URL count, and the reference blanks those two
/// headings for it.
static TOP_COLLIDERS_TABLE: TableSpec = TableSpec {
    element: "top-colliders",
    selection: TableSelectionMode::Multi,
    columns: &SHARED_COLUMNS,
    default_sort: &SCORE_FIRST,
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

// ---------------------------------------------------------------------------
// The two window kinds.
// ---------------------------------------------------------------------------

/// Which report a window is about. Fixed when the window is spawned: a window
/// never becomes the other report.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopObjectsKind {
    /// The region's top script users.
    Scripts,
    /// The region's top colliders.
    Colliders,
}

impl TopObjectsKind {
    /// The kind a report type asks for. An unrecognised report type reads as the
    /// scripts one, which is what its wire value `0` is.
    const fn of(report: LandStatReportType) -> Self {
        match report {
            LandStatReportType::TopColliders => Self::Colliders,
            _ => Self::Scripts,
        }
    }

    /// The report this kind asks the region for.
    const fn report(self) -> LandStatReportType {
        match self {
            Self::Scripts => LandStatReportType::TopScripts,
            Self::Colliders => LandStatReportType::TopColliders,
        }
    }

    /// This kind's list.
    const fn table(self) -> &'static TableSpec {
        match self {
            Self::Scripts => &TOP_SCRIPTS_TABLE,
            Self::Colliders => &TOP_COLLIDERS_TABLE,
        }
    }

    /// This kind's window title key.
    const fn title_key(self) -> &'static str {
        match self {
            Self::Scripts => "top-objects-title-scripts",
            Self::Colliders => "top-objects-title-colliders",
        }
    }

    /// This kind's summary-line key.
    const fn summary_key(self) -> &'static str {
        match self {
            Self::Scripts => "top-objects-summary-scripts",
            Self::Colliders => "top-objects-summary-colliders",
        }
    }

    /// This kind's [`FloaterSpec`].
    fn spec(self) -> FloaterSpec {
        match self {
            Self::Scripts => top_scripts_floater_spec(),
            Self::Colliders => top_colliders_floater_spec(),
        }
    }
}

/// The horizontal padding the window's content column adds on each side.
const CONTENT_PADDING: f32 = 8.0;

/// The width a list needs before its flexible Name column gets anything: every
/// fixed column, the gaps between them, the row padding on both edges, and the
/// scrollbar gutter the header holds open. A window narrower than this pushes
/// its own columns out of sight — the table widget does not scroll sideways —
/// so it is what both windows floor their width at.
fn table_floor(spec: &TableSpec) -> f32 {
    let fixed: f32 = spec
        .columns
        .iter()
        .map(|column| match column.width {
            TableColumnWidth::Fixed { default } => default,
            TableColumnWidth::Flex(_grow) => 0.0,
        })
        .sum();
    // A column count is small; `u16` reaches `f32` exactly, and the workspace
    // forbids a bare `as`.
    let gap_count = u16::try_from(spec.columns.len().saturating_sub(1)).unwrap_or(u16::MAX);
    let gaps = spec.column_gap * f32::from(gap_count);
    fixed + gaps + spec.row_padding * 2.0 + SCROLLBAR_THICKNESS + CONTENT_PADDING * 2.0
}

/// The width a list wants: its floor plus room for the object-name column,
/// which is the one that holds a real name.
fn table_width(spec: &TableSpec) -> f32 {
    table_floor(spec) + NAME_COLUMN_ROOM
}

/// How much a window's default width leaves for the flexible Name column.
const NAME_COLUMN_ROOM: f32 = 180.0;

/// Both windows' default height.
const WINDOW_HEIGHT: f32 = 420.0;

/// Both windows' minimum height.
const MIN_WINDOW_HEIGHT: f32 = 300.0;

/// The Top Scripts floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn top_scripts_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: TOP_SCRIPTS_FLOATER_ID,
        title: "Top Scripts".to_owned(),
        position: Vec2::new(100.0, 110.0),
        // Sized to its own list rather than to a round number: the reference's
        // window is 800 × 350 over the same eight columns, and clips them when
        // dragged narrower — which is exactly what the floor below prevents.
        default_size: Some(Vec2::new(table_width(&TOP_SCRIPTS_TABLE), WINDOW_HEIGHT)),
        min_size: Some(Vec2::new(
            table_floor(&TOP_SCRIPTS_TABLE),
            MIN_WINDOW_HEIGHT,
        )),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// The Top Colliders floater's [`FloaterSpec`] — two columns narrower than its
/// sibling, which is exactly the two it does not have.
#[must_use]
pub fn top_colliders_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: TOP_COLLIDERS_FLOATER_ID,
        title: "Top Colliders".to_owned(),
        position: Vec2::new(130.0, 140.0),
        default_size: Some(Vec2::new(table_width(&TOP_COLLIDERS_TABLE), WINDOW_HEIGHT)),
        min_size: Some(Vec2::new(
            table_floor(&TOP_COLLIDERS_TABLE),
            MIN_WINDOW_HEIGHT,
        )),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

/// Open (or raise) a report window **on a region** — written by the Region /
/// Estate floater's Debug tab buttons, naming the region that window is about.
#[derive(Message, Debug, Clone)]
pub struct OpenTopObjects {
    /// Which report to ask the region for.
    pub report: LandStatReportType,
    /// The region the asking window is about; the instance is keyed by it.
    pub region: Box<RegionIdentity>,
}

// ---------------------------------------------------------------------------
// Per-window state.
// ---------------------------------------------------------------------------

/// A press-dispatch tag on a window's action buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum TopObjectsAction {
    /// Track the selected row's position.
    ShowBeacon,
    /// Return the selected objects to their owners.
    ReturnSelected,
    /// Return every listed object (asks first).
    ReturnAll,
    /// Stop the selected objects' scripts.
    DisableSelected,
    /// Stop every listed object's scripts (asks first).
    DisableAll,
    /// Ask the region for the report again, unfiltered.
    Refresh,
    /// Ask again, narrowed to the object-name field.
    FilterByObject,
    /// Ask again, narrowed to the owner field.
    FilterByOwner,
    /// Ask again, narrowed to the parcel field.
    FilterByParcel,
}

impl TopObjectsAction {
    /// The filter flag and field this action asks with, if it is a filter.
    const fn filter(self) -> Option<(u32, FilterField)> {
        match self {
            Self::FilterByObject => Some((FILTER_BY_OBJECT, FilterField::Object)),
            Self::FilterByOwner => Some((FILTER_BY_OWNER, FilterField::Owner)),
            Self::FilterByParcel => Some((FILTER_BY_PARCEL_NAME, FilterField::Parcel)),
            _ => None,
        }
    }
}

/// Which of the three filter fields a filter reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterField {
    /// The object-name field.
    Object,
    /// The owner field.
    Owner,
    /// The parcel field.
    Parcel,
}

/// One window's node handles, for the in-place updates.
#[derive(Component, Debug)]
struct TopObjectsUi {
    /// The summary line over the list.
    summary: Entity,
    /// The list's root (carries its [`TableState`]).
    table: Entity,
    /// The list's scrolling viewport (the pooled rows' parent).
    viewport: Entity,
    /// The selected object's id, shown read-only.
    id_line: Entity,
    /// The object-name filter field.
    object_field: Entity,
    /// The owner filter field.
    owner_field: Entity,
    /// The parcel filter field.
    parcel_field: Entity,
}

impl TopObjectsUi {
    /// The field a filter action reads.
    const fn field(&self, field: FilterField) -> Entity {
        match field {
            FilterField::Object => self.object_field,
            FilterField::Owner => self.owner_field,
            FilterField::Parcel => self.parcel_field,
        }
    }
}

/// How far one window's current request has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AskState {
    /// Nothing has been asked yet, or the last answer was thrown away.
    Unasked,
    /// A request has gone out and no reply has landed yet.
    Asking,
    /// At least one reply to the last request has landed. An empty list in this
    /// state is the region saying "nothing", which is not the same as not
    /// having asked.
    Answered,
}

/// What one window's region last said, and what the window has done about it.
#[derive(Component, Debug)]
struct TopObjectsState {
    /// The region this window is about — its instance key.
    region: FloaterKey,
    /// Whether that region is still the one the agent is in. A window whose
    /// region the agent has left keeps its report and stops asking.
    is_current: bool,
    /// The rows, in arrival order.
    items: Vec<LandStatItem>,
    /// The region's own count of matching objects, which may exceed
    /// [`items`](Self::items) — the report carries only its top rows.
    total: u32,
    /// The sum of the listed scores (the reference's `mtotalScore`).
    total_score: f32,
    /// How far the current request has got.
    ask: AskState,
    /// The flag a filter press armed for the next request (the reference's
    /// `mFlags`), consumed by the request it belongs to.
    flags: u32,
    /// The filter string armed with [`flags`](Self::flags) (`mFilter`).
    filter: String,
    /// The selected task ids, mirrored out of the table so a re-sort can put the
    /// selection back on the same objects.
    selected: Vec<ObjectKey>,
    /// The table selection revision [`selected`](Self::selected) was read at.
    read_revision: u64,
    /// The view needs rebuilding (a reply landed, or the window opened).
    dirty: bool,
}

impl TopObjectsState {
    /// A window's state, about the region `region` keys.
    const fn new(region: FloaterKey) -> Self {
        Self {
            region,
            is_current: true,
            items: Vec::new(),
            total: 0,
            total_score: 0.0,
            ask: AskState::Unasked,
            flags: 0,
            filter: String::new(),
            selected: Vec::new(),
            read_revision: 0,
            dirty: true,
        }
    }

    /// Whether a request is in flight.
    const fn awaiting(&self) -> bool {
        matches!(self.ask, AskState::Asking)
    }

    /// Forget the report — a new request is going out.
    fn clear(&mut self) {
        self.items.clear();
        self.total = 0;
        self.total_score = 0.0;
        self.ask = AskState::Unasked;
        self.selected.clear();
        self.dirty = true;
    }

    /// The row at a position in the list's current order.
    fn row<'a>(&'a self, view: &TopObjectsView, position: usize) -> Option<&'a LandStatItem> {
        view.order
            .get(position)
            .and_then(|index| self.items.get(*index))
    }
}

/// The sorted projection of one window's [`TopObjectsState::items`].
#[derive(Component, Debug, Default)]
struct TopObjectsView {
    /// Indices into the items, in list order.
    order: Vec<usize>,
    /// The sort revision this order was built at.
    built_sort_revision: u64,
}

/// One window's double-click bookkeeping: the object pressed last and when (rows
/// are pooled, so the object is the identity, not the row entity).
#[derive(Component, Debug, Default)]
struct TopObjectsClicks {
    /// The object pressed last.
    object: Option<ObjectKey>,
    /// When it was pressed, in seconds since startup.
    time: f32,
}

/// The confirmation in flight, if any, and the window that asked for it.
///
/// One slot for the whole viewer because these are modal alerts: a second window
/// cannot raise one while the first is up, and a [`NotificationResponse`] names
/// the template rather than the asker.
#[derive(Resource, Debug, Default)]
struct TopObjectsConfirm(Option<(Entity, TopObjectsAction)>);

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the two report floaters into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TopObjectsPlugin;

impl Plugin for TopObjectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenTopObjects>()
            // The confirmation round-trip the two destructive actions ask over.
            // Their owner is the notification host; declaring them here too
            // (`add_message` is idempotent) is what lets this plugin be
            // scheduled on its own — the About Region tests do exactly that, and
            // a reader of a message nothing registered fails on frame one.
            .add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            // Likewise the session facts every request here is scoped by (the
            // circuit a command goes out on, the region a position is global
            // against) and the beacon a row sets: their real owners are the
            // client driver and the map layer.
            .init_resource::<SlIdentity>()
            .init_resource::<MapTracking>()
            .init_resource::<TopObjectsConfirm>()
            .add_systems(
                Update,
                // After the manager's command pass, for the reason
                // `KeyedFloaters::open` documents: the click that opens a window
                // also raises the window it was clicked in, and the later raise
                // wins.
                open_top_objects
                    .after(FloaterSystems::Commands)
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    follow_current_region,
                    ingest_land_stat_replies,
                    rebuild_top_objects_views,
                    populate_top_objects_rows,
                    bind_top_objects_rows,
                    mirror_selections,
                    update_summary_lines,
                    update_button_enable,
                    filter_on_enter,
                    answer_confirmations,
                )
                    .chain()
                    .after(open_top_objects)
                    .before(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// Open.
// ---------------------------------------------------------------------------

/// Open (or raise) the window for a report **on the region that asked**, and ask
/// that region for the report — a re-open re-asks, which is what pressing the
/// Debug tab's button again means.
fn open_top_objects(
    mut requests: MessageReader<OpenTopObjects>,
    mut windows: KeyedFloaters,
    mut states: Query<&mut TopObjectsState>,
    identity: Res<SlIdentity>,
    mut spawner: Commands,
    mut commands: MessageWriter<SlCommand>,
) {
    for request in requests.read() {
        let kind = TopObjectsKind::of(request.report);
        let key = region_key(&request.region);
        match windows.open(kind.spec(), key.clone()) {
            KeyedFloaterOpen::Spawned(handle) => {
                let ui = build_top_objects_content(&mut spawner, handle, kind);
                spawner
                    .entity(handle.title_text)
                    .insert(Translated::new(kind.title_key()));
                // Seeded here rather than after the insert: the components only
                // reach the world when this frame's commands flush, so a window
                // spawned now is not queryable yet.
                let mut state = TopObjectsState::new(key);
                ask_region(kind, &mut state, &identity, &mut commands);
                spawner.entity(handle.root).insert((
                    kind,
                    state,
                    TopObjectsView::default(),
                    TopObjectsClicks::default(),
                    ui,
                ));
            }
            KeyedFloaterOpen::Existing(window) => {
                if let Ok(mut state) = states.get_mut(window) {
                    ask_region(kind, &mut state, &identity, &mut commands);
                }
            }
        }
    }
}

/// Send the request a window is armed for, clearing the list it replaces and
/// consuming the filter — the reference's `onRefresh`, which ends by clearing
/// `mFilter` and `mFlags` so the next plain refresh is unfiltered.
///
/// A window about a region the agent has left asks nothing: the request would go
/// out on the current circuit and be answered by the wrong region.
fn ask_region(
    kind: TopObjectsKind,
    state: &mut TopObjectsState,
    identity: &SlIdentity,
    commands: &mut MessageWriter<SlCommand>,
) {
    let Some(circuit) = identity.circuit_id else {
        return;
    };
    if !state.is_current {
        return;
    }
    state.clear();
    state.ask = AskState::Asking;
    commands.write(SlCommand(Command::RequestLandStat {
        report_type: kind.report(),
        request_flags: state.flags,
        filter: std::mem::take(&mut state.filter),
        parcel_local_id: ScopedParcelId::new(circuit, WHOLE_REGION_REPORT),
    }));
    state.flags = 0;
}

/// Keep every window's "is this still the region I am in?" flag current.
fn follow_current_region(
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut windows: Query<&mut TopObjectsState>,
) {
    let current = regions.iter().next().map(|region| region_key(&region.0));
    for mut state in &mut windows {
        let is_current = current.as_ref() == Some(&state.region);
        if state.is_current != is_current {
            state.is_current = is_current;
        }
    }
}

// ---------------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------------

/// A window's content, built as it is spawned: the summary line, the list, the
/// id line, the three filter rows and the action row.
fn build_top_objects_content(
    commands: &mut Commands,
    handle: FloaterHandle,
    kind: TopObjectsKind,
) -> TopObjectsUi {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(6.0))
            },
            Name::new(format!("{}:content", kind.table().element)),
            ChildOf(handle.content),
        ))
        .id();

    let summary = spawn_line(commands, content, LABEL_COLOR);

    // The list takes the window's slack; everything under it keeps its height.
    let wrapper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..default()
            },
            BackgroundColor(LIST_BACKGROUND),
            ChildOf(content),
        ))
        .id();
    let table = spawn_table(commands, wrapper, kind.table());

    let id_row = spawn_labelled_row(commands, content, "top-objects-object-id");
    let id_line = spawn_line(commands, id_row, DIM_LABEL_COLOR);
    spawn_action_button(
        commands,
        id_row,
        "top-objects-show-beacon",
        TopObjectsAction::ShowBeacon,
        0,
    );

    let object_row = spawn_labelled_row(commands, content, "top-objects-object-name");
    let object_field = spawn_filter_field(commands, object_row, "top-objects-object-field", 1);
    spawn_action_button(
        commands,
        object_row,
        "top-objects-filter",
        TopObjectsAction::FilterByObject,
        2,
    );

    let owner_row = spawn_labelled_row(commands, content, "top-objects-owner");
    let owner_field = spawn_filter_field(commands, owner_row, "top-objects-owner-field", 3);
    spawn_action_button(
        commands,
        owner_row,
        "top-objects-filter",
        TopObjectsAction::FilterByOwner,
        4,
    );

    let parcel_row = spawn_labelled_row(commands, content, "top-objects-parcel");
    let parcel_field = spawn_filter_field(commands, parcel_row, "top-objects-parcel-field", 5);
    spawn_action_button(
        commands,
        parcel_row,
        "top-objects-filter",
        TopObjectsAction::FilterByParcel,
        6,
    );

    let actions = spawn_row(commands, content);
    spawn_action_button(
        commands,
        actions,
        "top-objects-return-selected",
        TopObjectsAction::ReturnSelected,
        7,
    );
    spawn_action_button(
        commands,
        actions,
        "top-objects-return-all",
        TopObjectsAction::ReturnAll,
        8,
    );
    spawn_action_button(
        commands,
        actions,
        "top-objects-disable-selected",
        TopObjectsAction::DisableSelected,
        9,
    );
    spawn_action_button(
        commands,
        actions,
        "top-objects-disable-all",
        TopObjectsAction::DisableAll,
        10,
    );
    spawn_action_button(
        commands,
        actions,
        "top-objects-refresh",
        TopObjectsAction::Refresh,
        11,
    );

    TopObjectsUi {
        summary,
        table: table.root,
        viewport: table.viewport,
        id_line,
        object_field,
        owner_field,
        parcel_field,
    }
}

/// A wrapping row of controls.
fn spawn_row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id()
}

/// A row opening with a translated label.
fn spawn_labelled_row(commands: &mut Commands, parent: Entity, key: &'static str) -> Entity {
    let row_entity = spawn_row(commands, parent);
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    row_entity
}

/// An empty text line the caller updates in place.
fn spawn_line(commands: &mut Commands, parent: Entity, color: Color) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(color),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// One of the three filter fields.
fn spawn_filter_field(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    tab_index: i32,
) -> Entity {
    spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: FONT_SIZE,
            width_glyphs: 24.0,
            tab_index,
            max_characters: Some(63),
            ..TextInputSpec::new(element, TextInputKind::Line)
        },
    )
}

/// A translated action button dispatching `action`.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: TopObjectsAction,
    tab_index: i32,
) {
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
            Name::new(format!("top-objects-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_top_objects_action)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

// ---------------------------------------------------------------------------
// Ingest.
// ---------------------------------------------------------------------------

/// Fold every `LandStatReply` into the window that could have asked for it: the
/// one of that report's kind about the region the agent is in. A reply carries
/// no window of its own, and a request only ever goes out on the current
/// circuit, so that window is the only candidate.
///
/// A reply **appends**: one request is answered by as many messages as the
/// report needs.
fn ingest_land_stat_replies(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<(&TopObjectsKind, &mut TopObjectsState)>,
) {
    for event in events.read() {
        let SlSessionEvent::LandStatReply {
            report_type,
            total_object_count,
            items,
            ..
        } = &event.0
        else {
            continue;
        };
        let kind = TopObjectsKind::of(*report_type);
        for (window_kind, mut state) in &mut windows {
            if *window_kind != kind || !state.is_current {
                continue;
            }
            state.total = *total_object_count;
            state.ask = AskState::Answered;
            for item in items {
                state.total_score += item.score.raw();
                state.items.push(item.clone());
            }
            state.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// The list.
// ---------------------------------------------------------------------------

/// Order two rows by one sort key.
fn compare_rows(left: &LandStatItem, right: &LandStatItem, token: &str) -> std::cmp::Ordering {
    match token {
        // By the raw number, whatever unit it is in: two rows of one report are
        // always the same unit, and the raw value is the one the region ranked
        // them by.
        "score" => left
            .score
            .raw()
            .partial_cmp(&right.score.raw())
            .unwrap_or(std::cmp::Ordering::Equal),
        "name" => left
            .task_name
            .to_lowercase()
            .cmp(&right.task_name.to_lowercase()),
        "owner" => left
            .owner_name
            .to_lowercase()
            .cmp(&right.owner_name.to_lowercase()),
        // Numerically, by the coordinates the cell shows — a list sorted by the
        // formatted string would put `<90, …>` after `<100, …>`.
        "location" => (left.location.x(), left.location.y(), left.location.z())
            .partial_cmp(&(right.location.x(), right.location.y(), right.location.z()))
            .unwrap_or(std::cmp::Ordering::Equal),
        // The extended columns. A row with no extended block sorts as a zero /
        // empty one rather than being dropped out of the order.
        "parcel" => {
            extended_str(left, |extended| extended.parcel_name.to_lowercase())
                .cmp(&extended_str(right, |extended| {
                    extended.parcel_name.to_lowercase()
                }))
        }
        "date" => extended_num(left, |extended| f64::from(extended.timestamp))
            .partial_cmp(&extended_num(right, |extended| {
                f64::from(extended.timestamp)
            }))
            .unwrap_or(std::cmp::Ordering::Equal),
        "memory" => extended_num(left, |extended| f64::from(extended.script_size_bytes))
            .partial_cmp(&extended_num(right, |extended| {
                f64::from(extended.script_size_bytes)
            }))
            .unwrap_or(std::cmp::Ordering::Equal),
        "urls" => extended_num(left, |extended| f64::from(extended.public_urls))
            .partial_cmp(&extended_num(right, |extended| {
                f64::from(extended.public_urls)
            }))
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => std::cmp::Ordering::Equal,
    }
}

/// A sortable number out of a row's extended block, or zero when it has none.
fn extended_num(item: &LandStatItem, read: impl Fn(&LandStatExtended) -> f64) -> f64 {
    item.extended.as_ref().map_or(0.0, read)
}

/// A sortable string out of a row's extended block, or empty when it has none.
fn extended_str(item: &LandStatItem, read: impl Fn(&LandStatExtended) -> String) -> String {
    item.extended.as_ref().map_or_else(String::new, read)
}

/// The item order for a sort, most-significant key first.
fn sorted_order(items: &[LandStatItem], keys: &[(&'static str, bool)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|left, right| {
        let (Some(left), Some(right)) = (items.get(*left), items.get(*right)) else {
            return std::cmp::Ordering::Equal;
        };
        for (token, ascending) in keys {
            let ordering = compare_rows(left, right, token);
            if ordering != std::cmp::Ordering::Equal {
                return if *ascending {
                    ordering
                } else {
                    ordering.reverse()
                };
            }
        }
        std::cmp::Ordering::Equal
    });
    order
}

/// Rebuild each window's sorted view when its report or its sort changed, and
/// put the selection back on the same objects at their new positions.
fn rebuild_top_objects_views(
    mut windows: Query<(
        &TopObjectsKind,
        &mut TopObjectsState,
        &mut TopObjectsView,
        &TopObjectsUi,
    )>,
    mut tables: Query<&mut TableState>,
    mut lists: Query<&mut VirtualList>,
) {
    for (kind, mut state, mut view, ui) in &mut windows {
        let sort = tables
            .get(ui.table)
            .ok()
            .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
        let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
        if !state.dirty && view.built_sort_revision == sort_revision {
            continue;
        }
        state.dirty = false;
        view.built_sort_revision = sort_revision;

        let keys: Vec<(&'static str, bool)> = sort
            .map(|(_revision, keys)| keys)
            .unwrap_or_default()
            .iter()
            .filter_map(|key| {
                kind.table()
                    .columns
                    .get(key.column)
                    .map(|column| (column.token, key.ascending))
            })
            .collect();
        view.order = sorted_order(&state.items, &keys);

        if let Ok(mut list) = lists.get_mut(ui.viewport) {
            list.item_count = view.order.len();
        }
        if let Ok(mut table) = tables.get_mut(ui.table) {
            let positions = selection_positions(&state, &view);
            let anchor = positions.first().copied();
            table.set_selection(positions, anchor);
            // Re-projecting is not a selection *event* — it is the same objects
            // at new positions — so the mirror must not read it back as one.
            state.read_revision = table.selection_revision();
        }
    }
}

/// Where the selected objects sit in the current order.
fn selection_positions(state: &TopObjectsState, view: &TopObjectsView) -> Vec<usize> {
    view.order
        .iter()
        .enumerate()
        .filter_map(|(position, index)| {
            let item = state.items.get(*index)?;
            state.selected.contains(&item.task_id).then_some(position)
        })
        .collect()
}

/// Give each pooled row its cells the first time it is spawned.
fn populate_top_objects_rows(
    mut commands: Commands,
    windows: Query<(&TopObjectsKind, &TopObjectsUi)>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    for (row_entity, child_of) in &new_rows {
        // Which window's list this row belongs to: every open window pools its
        // own rows under its own viewport.
        let Some((kind, ui)) = windows
            .iter()
            .find(|(_kind, ui)| ui.viewport == child_of.parent())
        else {
            continue;
        };
        spawn_table_row(&mut commands, row_entity, ui.table, kind.table());
        commands
            .entity(row_entity)
            .observe(on_top_objects_row_press);
    }
}

/// Write each visible row's cells.
fn bind_top_objects_rows(
    windows: Query<(
        &TopObjectsKind,
        Ref<TopObjectsState>,
        Ref<TopObjectsView>,
        &TopObjectsUi,
    )>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (kind, state, view, ui) in &windows {
        let refresh = state.is_changed() || view.is_changed();
        for (row, child_of, cells) in &rows {
            if child_of.parent() != ui.viewport {
                continue;
            }
            if !refresh && !row.is_changed() {
                continue;
            }
            let Some(item) = row.index.and_then(|position| state.row(&view, position)) else {
                continue;
            };
            for (column, value) in row_cells(item, *kind) {
                if let Some(cell) = cells.cell(column) {
                    set_table_cell(&mut texts, cell, &value, LABEL_COLOR);
                }
            }
        }
    }
}

/// One row's cells, by column index — the last two only for the scripts window,
/// which is the only one whose list has them.
fn row_cells(item: &LandStatItem, kind: TopObjectsKind) -> Vec<(usize, String)> {
    let extended = item.extended.as_ref();
    let mut cells = vec![
        (COL_SCORE, format_score(item.score)),
        (COL_NAME, item.task_name.trim().to_owned()),
        (COL_OWNER, item.owner_name.trim().to_owned()),
        (COL_LOCATION, format_location(item)),
        (
            COL_PARCEL,
            extended.map_or_else(String::new, |extended| extended.parcel_name.clone()),
        ),
        (
            COL_DATE,
            extended.map_or_else(String::new, |extended| format_rez_date(extended.timestamp)),
        ),
    ];
    if kind == TopObjectsKind::Scripts {
        cells.push((
            COL_MEMORY,
            extended.map_or_else(String::new, |extended| {
                format_memory(extended.script_size_bytes)
            }),
        ));
        cells.push((
            COL_URLS,
            extended.map_or_else(String::new, |extended| extended.public_urls.to_string()),
        ));
    }
    cells
}

/// A score, in the unit its report gives it.
///
/// **Not** the reference's `%0.3f` of the raw number, deliberately: a script
/// time is a duration, and a region that has been up a while reports one in the
/// hundreds of thousands of milliseconds — a figure that says nothing at a
/// glance and everything once it reads `16m 34s`. A collision count is a count,
/// and reads as one (the reference's three decimals on an integer are an
/// artefact of one column serving both reports).
fn format_score(score: LandStatScore) -> String {
    match score {
        LandStatScore::ScriptTime(time) => format_duration_units(time),
        // Every other score is a bare number, including one this build does not
        // know the unit of: showing it is better than showing nothing.
        _other => format_count(score.raw()),
    }
}

/// A count the wire sends as a float: whole where it is whole, and with the
/// reference's three decimals where a simulator sends a fraction.
fn format_count(count: f32) -> String {
    if count.fract() == 0.0 && count.abs() < 1e9 {
        format!("{count:.0}")
    } else {
        format!("{count:.3}")
    }
}

/// A row's script memory in kibibytes, as the reference shows it (`%0.0f` of
/// `size / 1024`). A row that uses none reads empty rather than `0`.
fn format_memory(bytes: f32) -> String {
    if bytes <= 0.0 {
        return String::new();
    }
    format!("{:.0}", bytes / BYTES_PER_KIB)
}

/// A row's rez date. A zero timestamp is "not stated" rather than 1970.
fn format_rez_date(timestamp: u32) -> String {
    if timestamp == 0 {
        return String::new();
    }
    format_unix_date(i64::from(timestamp))
}

/// A row's position, as the reference formats it (`<%0.f, %0.f, %0.f>`).
fn format_location(item: &LandStatItem) -> String {
    format!(
        "<{:.0}, {:.0}, {:.0}>",
        item.location.x(),
        item.location.y(),
        item.location.z()
    )
}

// ---------------------------------------------------------------------------
// The selection readout.
// ---------------------------------------------------------------------------

/// Mirror each table's selection into its window, and fill that window's id line
/// and object / owner fields from its primary row — the reference's
/// `updateSelectionInfo`, where those fields are both the selection readout and
/// the filter input.
fn mirror_selections(
    mut windows: Query<(&mut TopObjectsState, &TopObjectsView, &TopObjectsUi)>,
    tables: Query<&TableState>,
    mut fields: Query<&mut EditableText>,
    mut texts: Query<&mut Text>,
) {
    for (mut state, view, ui) in &mut windows {
        let Ok(table) = tables.get(ui.table) else {
            continue;
        };
        if table.selection_revision() == state.read_revision {
            continue;
        }
        state.read_revision = table.selection_revision();
        state.selected = table
            .selected()
            .iter()
            .filter_map(|position| state.row(view, *position).map(|item| item.task_id))
            .collect();

        let primary = table
            .primary_selected()
            .and_then(|position| state.row(view, position))
            .cloned();
        let (id, name, owner) = primary.map_or_else(
            || (String::new(), String::new(), String::new()),
            |item| {
                (
                    item.task_id.uuid().to_string(),
                    item.task_name.trim().to_owned(),
                    item.owner_name.trim().to_owned(),
                )
            },
        );
        if let Ok(mut text) = texts.get_mut(ui.id_line) {
            id.clone_into(&mut text.0);
        }
        set_field_text(&mut fields, ui.object_field, &name);
        set_field_text(&mut fields, ui.owner_field, &owner);
    }
}

/// Set a field's text in place, leaving a field mid-composition alone.
fn set_field_text(fields: &mut Query<&mut EditableText>, field: Entity, value: &str) {
    if let Ok(mut editable) = fields.get_mut(field)
        && !editable.is_composing()
        && editable.value() != value
    {
        editable.editor_mut().set_text(value);
    }
}

// ---------------------------------------------------------------------------
// The summary line and the button states.
// ---------------------------------------------------------------------------

/// Repaint each window's line over its list: what was asked, and what the region
/// answered.
fn update_summary_lines(
    windows: Query<(&TopObjectsKind, Ref<TopObjectsState>, &TopObjectsUi)>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    for (kind, state, ui) in &windows {
        if !state.is_changed() {
            continue;
        }
        let summary = summary_text(*kind, &state, &translator);
        if let Ok(mut text) = texts.get_mut(ui.summary)
            && text.0 != summary
        {
            summary.clone_into(&mut text.0);
        }
    }
}

/// A window's summary line for its current state.
fn summary_text(kind: TopObjectsKind, state: &TopObjectsState, translator: &Translator) -> String {
    if state.awaiting() {
        return translator.get("top-objects-loading");
    }
    if !state.is_current {
        return translator.get("top-objects-left-region");
    }
    if state.ask == AskState::Answered && state.items.is_empty() {
        return translator.get("top-objects-none");
    }
    let args = TransArgs::new().int("count", i64::from(state.total));
    match kind {
        TopObjectsKind::Colliders => translator.format(kind.summary_key(), &args),
        // The listed rows' script time, added up and read the way a cell reads
        // one — the reference's "[COUNT] scripts taking a total of [TIME] ms",
        // with the unit in the value rather than in the sentence.
        TopObjectsKind::Scripts => translator.format(
            kind.summary_key(),
            &args.text(
                "time",
                &format_score(LandStatScore::from_wire(
                    LandStatReportType::TopScripts,
                    state.total_score,
                )),
            ),
        ),
    }
}

/// Whether the agent may manage this region's estate — the gate the reference
/// puts on the Debug tab this window is reached from, re-checked here because
/// the window outlives the press that opened it.
fn can_manage(regions: &Query<&SlRegionIdentity, With<SlCurrentRegion>>) -> bool {
    regions
        .iter()
        .next()
        .is_some_and(|identity| identity.0.is_estate_manager)
}

/// Whether an action can be taken right now.
const fn action_enabled(
    action: TopObjectsAction,
    manage: bool,
    listed: usize,
    selected: usize,
    awaiting: bool,
) -> bool {
    match action {
        // Tracking a position is not an estate act.
        TopObjectsAction::ShowBeacon => selected > 0,
        TopObjectsAction::ReturnSelected | TopObjectsAction::DisableSelected => {
            manage && selected > 0
        }
        TopObjectsAction::ReturnAll | TopObjectsAction::DisableAll => manage && listed > 0,
        // The reference disables Refresh until the reply lands, so a slow region
        // is not asked the same question five times.
        TopObjectsAction::Refresh
        | TopObjectsAction::FilterByObject
        | TopObjectsAction::FilterByOwner
        | TopObjectsAction::FilterByParcel => manage && !awaiting,
    }
}

/// Whether a window's action can be taken: its own state, and the estate rights
/// of the region the agent is in. A window whose region the agent has left can
/// do nothing but be read.
const fn window_action_enabled(
    action: TopObjectsAction,
    manage: bool,
    state: &TopObjectsState,
) -> bool {
    state.is_current
        && action_enabled(
            action,
            manage,
            state.items.len(),
            state.selected.len(),
            state.awaiting(),
        )
}

/// Grey out and refuse the buttons whose action cannot be taken, per window.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the windows, the \
              region rights, the buttons and the ancestry walk that finds each one's window, the \
              disabled marker, and the label recolouring"
)]
fn update_button_enable(
    windows: Query<&TopObjectsState>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    buttons: Query<(Entity, &TopObjectsAction)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    disabled: Query<(), With<InteractionDisabled>>,
    children: Query<&Children>,
    mut texts: Query<&mut TextColor>,
    mut commands: Commands,
) {
    let manage = can_manage(&regions);
    for (entity, action) in &buttons {
        let Some(state) =
            host_floater(entity, &parents, &floaters).and_then(|window| windows.get(window).ok())
        else {
            continue;
        };
        let enabled = window_action_enabled(*action, manage, state);
        let is_disabled = disabled.contains(entity);
        if enabled && is_disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !is_disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
        let want = if enabled { LABEL_COLOR } else { DISABLED_COLOR };
        if let Ok(label) = children.get(entity) {
            for child in label.iter() {
                if let Ok(mut color) = texts.get_mut(child)
                    && color.0 != want
                {
                    color.0 = want;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The actions.
// ---------------------------------------------------------------------------

/// A press on a pooled row: the table widget already owns the selection, so this
/// only adds the reference's double-click, which shows the beacon.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected resources / queries: the press, the \
              pressed row, the ancestry walk that finds its window, the windows themselves, the \
              clock the double-click is measured with, the identity the position is global \
              against, and the tracking beacon it sets"
)]
fn on_top_objects_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&VirtualRow>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut windows: Query<(&TopObjectsState, &TopObjectsView, &mut TopObjectsClicks)>,
    time: Res<Time>,
    identity: Res<SlIdentity>,
    mut tracking: ResMut<MapTracking>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((state, view, mut clicks)) = windows.get_mut(window) else {
        return;
    };
    let Some(item) = row
        .index
        .and_then(|position| state.row(view, position))
        .cloned()
    else {
        return;
    };
    press.propagate(false);
    let now = time.elapsed_secs();
    if clicks.object == Some(item.task_id) && now - clicks.time <= DOUBLE_CLICK_SECS {
        clicks.object = None;
        track_item(&item, &identity, &mut tracking);
    } else {
        clicks.object = Some(item.task_id);
        clicks.time = now;
    }
}

/// Point the shared tracking beacon at a row's position — the reference's
/// `LLTracker::trackLocation` over `getPosGlobalFromAgent`, so the position the
/// report gave is read as region-local in the region the agent is in.
fn track_item(item: &LandStatItem, identity: &SlIdentity, tracking: &mut MapTracking) {
    let Some(handle) = identity.region_handle else {
        return;
    };
    let (grid_x, grid_y) = handle.grid_coordinates();
    let global = GlobalCoordinates::from_grid_and_region(
        GridCoordinates::new(grid_x, grid_y),
        item.location,
    );
    tracking.target = Some(TrackTarget::Location {
        east: global.x(),
        north: global.y(),
        up: item.location.z(),
    });
}

/// `Enter` in a filter field runs that window's filter (see the module header
/// for why this is not the reference's default-button behaviour).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the keyboard and \
              focus the gesture is read from, the ancestry walk that finds the focused field's \
              window, the windows and region rights, the field the filter is read out of, the \
              identity the request is scoped by, and the command output"
)]
fn filter_on_enter(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut windows: Query<(&TopObjectsKind, &mut TopObjectsState, &TopObjectsUi)>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    fields: Query<&EditableText>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    let Some(window) = host_floater(focused, &parents, &floaters) else {
        return;
    };
    let Ok((kind, mut state, ui)) = windows.get_mut(window) else {
        return;
    };
    let Some((flags, field)) = [
        (FILTER_BY_OBJECT, FilterField::Object),
        (FILTER_BY_OWNER, FilterField::Owner),
        (FILTER_BY_PARCEL_NAME, FilterField::Parcel),
    ]
    .into_iter()
    .find(|(_flags, field)| ui.field(*field) == focused) else {
        return;
    };
    if !window_action_enabled(TopObjectsAction::Refresh, can_manage(&regions), &state) {
        return;
    }
    arm_filter(&mut state, flags, ui.field(field), &fields);
    ask_region(*kind, &mut state, &identity, &mut commands);
    keyboard.clear_just_pressed(KeyCode::Enter);
}

/// Arm a window's next request with a field's contents as its filter.
fn arm_filter(
    state: &mut TopObjectsState,
    flags: u32,
    field: Entity,
    fields: &Query<&EditableText>,
) {
    state.flags = flags;
    state.filter = fields
        .get(field)
        .map(|field| field.value().to_string())
        .unwrap_or_default();
}

/// Send the pressed button's command, on behalf of the window it sits in.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected resources / queries: the press, the \
              pressed button's action, the ancestry walk that finds its window, the windows and \
              their lists, the region rights, the filter fields, the identity the commands are \
              scoped by, the pending confirmation, and the command / notification / tracking \
              outputs"
)]
fn on_top_objects_action(
    press: On<Pointer<Press>>,
    actions: Query<&TopObjectsAction>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut windows: Query<(
        &TopObjectsKind,
        &mut TopObjectsState,
        &TopObjectsView,
        &TopObjectsUi,
    )>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    fields: Query<&EditableText>,
    identity: Res<SlIdentity>,
    tables: Query<&TableState>,
    mut confirm: ResMut<TopObjectsConfirm>,
    mut commands: MessageWriter<SlCommand>,
    mut notifications: MessageWriter<ShowNotification>,
    mut tracking: ResMut<MapTracking>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity).copied() else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((kind, mut state, view, ui)) = windows.get_mut(window) else {
        return;
    };
    if !window_action_enabled(action, can_manage(&regions), &state) {
        return;
    }
    match action {
        TopObjectsAction::ShowBeacon => {
            if let Some(item) = tables
                .get(ui.table)
                .ok()
                .and_then(TableState::primary_selected)
                .and_then(|position| state.row(view, position))
            {
                track_item(item, &identity, &mut tracking);
            }
        }
        TopObjectsAction::ReturnSelected => {
            let ids = state.selected.clone();
            send_return(&identity, &ids, &mut commands);
        }
        TopObjectsAction::DisableSelected => {
            let ids = state.selected.clone();
            send_disable(&identity, &ids, &mut commands);
        }
        // Everything, listed or not, goes back to its owner — the one action
        // here that cannot be undone, so it asks first.
        TopObjectsAction::ReturnAll => {
            confirm.0 = Some((window, action));
            notifications.write(ShowNotification::new(RETURN_ALL_CONFIRM));
        }
        TopObjectsAction::DisableAll => {
            confirm.0 = Some((window, action));
            notifications.write(ShowNotification::new(DISABLE_ALL_CONFIRM));
        }
        TopObjectsAction::Refresh => {
            ask_region(*kind, &mut state, &identity, &mut commands);
        }
        TopObjectsAction::FilterByObject
        | TopObjectsAction::FilterByOwner
        | TopObjectsAction::FilterByParcel => {
            if let Some((flags, field)) = action.filter() {
                arm_filter(&mut state, flags, ui.field(field), &fields);
            }
            ask_region(*kind, &mut state, &identity, &mut commands);
        }
    }
}

/// Act on an answered confirmation (or drop it, when the answer was Cancel).
fn answer_confirmations(
    mut responses: MessageReader<NotificationResponse>,
    mut confirm: ResMut<TopObjectsConfirm>,
    windows: Query<&TopObjectsState>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
) {
    for response in responses.read() {
        if !matches!(response.template, RETURN_ALL_CONFIRM | DISABLE_ALL_CONFIRM) {
            continue;
        }
        let Some((window, pending)) = confirm.0.take() else {
            continue;
        };
        if response.button != Some(CONFIRM_BUTTON) {
            continue;
        }
        // The window may have been closed, or its region left, while the
        // confirmation was up.
        let Ok(state) = windows.get(window) else {
            continue;
        };
        if !state.is_current {
            continue;
        }
        let ids: Vec<ObjectKey> = state.items.iter().map(|item| item.task_id).collect();
        match pending {
            TopObjectsAction::ReturnAll => send_return(&identity, &ids, &mut commands),
            TopObjectsAction::DisableAll => send_disable(&identity, &ids, &mut commands),
            _ => {}
        }
    }
}

/// Return the named objects to their owners, region-wide.
fn send_return(
    identity: &SlIdentity,
    task_ids: &[ObjectKey],
    commands: &mut MessageWriter<SlCommand>,
) {
    let Some(circuit) = identity.circuit_id else {
        return;
    };
    if task_ids.is_empty() {
        return;
    }
    commands.write(SlCommand(Command::ReturnParcelObjects {
        local_id: ScopedParcelId::new(circuit, WHOLE_REGION),
        return_type: ParcelReturnType::NONE,
        owner_ids: Vec::new(),
        task_ids: task_ids.to_vec(),
    }));
}

/// Stop the named objects' scripts, region-wide.
fn send_disable(
    identity: &SlIdentity,
    task_ids: &[ObjectKey],
    commands: &mut MessageWriter<SlCommand>,
) {
    let Some(circuit) = identity.circuit_id else {
        return;
    };
    if task_ids.is_empty() {
        return;
    }
    commands.write(SlCommand(Command::DisableParcelObjects {
        local_id: ScopedParcelId::new(circuit, WHOLE_REGION),
        return_type: ParcelReturnType::NONE,
        owner_ids: Vec::new(),
        task_ids: task_ids.to_vec(),
    }));
}

#[cfg(test)]
mod tests {
    use super::{
        FILTER_BY_OBJECT, FILTER_BY_OWNER, FILTER_BY_PARCEL_NAME, TopObjectsAction, TopObjectsKind,
        TopObjectsState, TopObjectsView, action_enabled, format_location, format_memory,
        format_rez_date, format_score, row_cells, selection_positions, sorted_order,
    };
    use crate::floater::FloaterKey;
    use sl_client_bevy::{
        LandStatExtended, LandStatItem, LandStatReportType, LandStatScore, ObjectKey,
        RegionCoordinates, Uuid,
    };

    /// One report row, with the extended block a real (event-queue) reply
    /// carries. `score` is the raw wire number — milliseconds, since these rows
    /// stand in for a top-scripts report.
    fn item(score: f32, name: &str, owner: &str, position: (f32, f32, f32)) -> LandStatItem {
        LandStatItem {
            task_local_id: sl_client_bevy::RegionLocalObjectId(1),
            task_id: ObjectKey::from(Uuid::from_u128(u128::from(score.to_bits()))),
            location: RegionCoordinates::new(position.0, position.1, position.2),
            score: LandStatScore::from_wire(LandStatReportType::TopScripts, score),
            task_name: name.to_owned(),
            owner_name: owner.to_owned(),
            extended: Some(LandStatExtended {
                mono_score: 0.0,
                owner_id: None,
                parcel_name: format!("{name} parcel"),
                public_urls: 0,
                script_size_bytes: score * 1024.0,
                timestamp: 1_700_000_000,
            }),
        }
    }

    /// A window state holding `items`, about a region it is still in.
    fn state(items: Vec<LandStatItem>) -> TopObjectsState {
        let mut state = TopObjectsState::new(FloaterKey::subject(&"region"));
        state.items = items;
        state
    }

    /// A position reads as the reference formats it; a score reads in its own
    /// unit — a duration for script time, a count for collisions.
    #[test]
    fn a_row_reads_in_the_units_of_its_report() {
        pretty_assertions::assert_eq!(
            format_location(&item(0.0, "x", "y", (128.4, 27.6, 21.0))),
            "<128, 28, 21>"
        );
        // The four-digit millisecond figures a live region reports read as a
        // time, not as a number with three decimals.
        pretty_assertions::assert_eq!(
            format_score(LandStatScore::from_wire(
                LandStatReportType::TopScripts,
                994_012.0
            )),
            "16m 34s 12ms"
        );
        pretty_assertions::assert_eq!(
            format_score(LandStatScore::from_wire(
                LandStatReportType::TopScripts,
                1.25
            )),
            "1ms 250µs"
        );
        // A collision count is a count.
        pretty_assertions::assert_eq!(
            format_score(LandStatScore::from_wire(
                LandStatReportType::TopColliders,
                17.0
            )),
            "17"
        );
        pretty_assertions::assert_eq!(
            format_score(LandStatScore::from_wire(
                LandStatReportType::TopColliders,
                17.5
            )),
            "17.500"
        );
    }

    /// The extended columns: memory in kibibytes, a date, and "not stated"
    /// rather than a zero for either.
    #[test]
    fn the_extended_cells_read_as_the_reference_shows_them() {
        pretty_assertions::assert_eq!(format_memory(4096.0), "4");
        pretty_assertions::assert_eq!(format_memory(1536.0), "2");
        // A row that uses no script memory, and one whose rez time the region
        // did not state, read empty rather than `0` and `1970-01-01`.
        pretty_assertions::assert_eq!(format_memory(0.0), "");
        pretty_assertions::assert_eq!(format_rez_date(0), "");
        assert!(format_rez_date(1_700_000_000).starts_with("2023-11-"));
    }

    /// The colliders list has no memory / URL column, so a row bound into it
    /// writes six cells where the scripts list writes eight.
    #[test]
    fn a_collider_row_has_no_script_columns() {
        let row = item(2.0, "thing", "abe", (1.0, 2.0, 3.0));
        pretty_assertions::assert_eq!(row_cells(&row, TopObjectsKind::Scripts).len(), 8);
        pretty_assertions::assert_eq!(row_cells(&row, TopObjectsKind::Colliders).len(), 6);
        pretty_assertions::assert_eq!(
            TopObjectsKind::Scripts.table().columns.len(),
            row_cells(&row, TopObjectsKind::Scripts).len()
        );
        pretty_assertions::assert_eq!(
            TopObjectsKind::Colliders.table().columns.len(),
            row_cells(&row, TopObjectsKind::Colliders).len()
        );
    }

    /// Each window kind asks for its own report, and nothing else.
    #[test]
    fn each_kind_asks_for_its_own_report() {
        pretty_assertions::assert_eq!(
            TopObjectsKind::Scripts.report(),
            LandStatReportType::TopScripts
        );
        pretty_assertions::assert_eq!(
            TopObjectsKind::Colliders.report(),
            LandStatReportType::TopColliders
        );
        pretty_assertions::assert_eq!(
            TopObjectsKind::of(LandStatReportType::TopColliders),
            TopObjectsKind::Colliders
        );
        // An unrecognised report type reads as the scripts one, which is what
        // its wire value `0` is — a reply for it reaches a window rather than
        // none.
        pretty_assertions::assert_eq!(
            TopObjectsKind::of(LandStatReportType::Other(7)),
            TopObjectsKind::Scripts
        );
    }

    /// The three filter flags are the reference's, and each action carries its
    /// own.
    #[test]
    fn each_filter_action_carries_the_references_flag() {
        pretty_assertions::assert_eq!(
            TopObjectsAction::FilterByObject.filter().map(|f| f.0),
            Some(FILTER_BY_OBJECT)
        );
        pretty_assertions::assert_eq!(
            TopObjectsAction::FilterByOwner.filter().map(|f| f.0),
            Some(FILTER_BY_OWNER)
        );
        pretty_assertions::assert_eq!(
            TopObjectsAction::FilterByParcel.filter().map(|f| f.0),
            Some(FILTER_BY_PARCEL_NAME)
        );
        assert!(TopObjectsAction::Refresh.filter().is_none());
    }

    /// Sorting orders by the clicked column, and a location sorts numerically
    /// rather than by the string the cell shows.
    #[test]
    fn the_list_sorts_by_column() {
        let items = vec![
            item(2.0, "beta", "zoe", (90.0, 10.0, 20.0)),
            item(10.0, "alpha", "abe", (100.0, 10.0, 20.0)),
            item(5.0, "gamma", "mia", (95.0, 10.0, 20.0)),
        ];
        pretty_assertions::assert_eq!(sorted_order(&items, &[("score", false)]), vec![1, 2, 0]);
        pretty_assertions::assert_eq!(sorted_order(&items, &[("name", true)]), vec![1, 0, 2]);
        pretty_assertions::assert_eq!(sorted_order(&items, &[("owner", true)]), vec![1, 2, 0]);
        // `<90, …>` sorts before `<100, …>`, which a string sort would not do.
        pretty_assertions::assert_eq!(sorted_order(&items, &[("location", true)]), vec![0, 2, 1]);
    }

    /// A row with no extended block (one that came over UDP) still sorts, as a
    /// zero / empty row rather than dropping out of the order.
    #[test]
    fn a_row_without_extended_data_still_sorts() {
        let mut bare = item(4.0, "bare", "abe", (10.0, 10.0, 10.0));
        bare.extended = None;
        let items = vec![
            bare,
            item(1.0, "small", "zoe", (20.0, 20.0, 20.0)),
            item(9.0, "large", "mia", (30.0, 30.0, 30.0)),
        ];
        // `script_size_bytes` is `score * 1024` for the two extended rows, and
        // the bare row counts as zero.
        pretty_assertions::assert_eq!(sorted_order(&items, &[("memory", true)]), vec![0, 1, 2]);
        pretty_assertions::assert_eq!(sorted_order(&items, &[("urls", true)]), vec![0, 1, 2]);
        // An empty parcel name sorts before any named one.
        pretty_assertions::assert_eq!(sorted_order(&items, &[("parcel", true)]), vec![0, 2, 1]);
    }

    /// A re-sort keeps the selection on the same objects, at their new
    /// positions.
    #[test]
    fn a_resort_reprojects_the_selection() {
        let items = vec![
            item(2.0, "beta", "zoe", (90.0, 10.0, 20.0)),
            item(10.0, "alpha", "abe", (100.0, 10.0, 20.0)),
            item(5.0, "gamma", "mia", (95.0, 10.0, 20.0)),
        ];
        let selected = items
            .first()
            .map(|item| item.task_id)
            .into_iter()
            .collect::<Vec<_>>();
        let mut state = state(items.clone());
        state.selected = selected;
        // Score-descending puts the `2.0` row last…
        let view = TopObjectsView {
            order: sorted_order(&items, &[("score", false)]),
            built_sort_revision: 0,
        };
        pretty_assertions::assert_eq!(selection_positions(&state, &view), vec![2]);
        // …and score-ascending puts it first.
        let view = TopObjectsView {
            order: sorted_order(&items, &[("score", true)]),
            built_sort_revision: 0,
        };
        pretty_assertions::assert_eq!(selection_positions(&state, &view), vec![0]);
    }

    /// Estate rights gate everything that writes; tracking a position does not
    /// need them, and a request in flight holds the refresh / filter buttons.
    #[test]
    fn each_action_has_its_own_precondition() {
        // No rights: the estate actions are refused whatever is listed.
        for action in [
            TopObjectsAction::ReturnSelected,
            TopObjectsAction::ReturnAll,
            TopObjectsAction::DisableSelected,
            TopObjectsAction::DisableAll,
            TopObjectsAction::Refresh,
        ] {
            assert!(!action_enabled(action, false, 3, 1, false));
        }
        // Show Beacon needs a selection, not rights.
        assert!(action_enabled(
            TopObjectsAction::ShowBeacon,
            false,
            3,
            1,
            false
        ));
        assert!(!action_enabled(
            TopObjectsAction::ShowBeacon,
            true,
            3,
            0,
            false
        ));
        // The "selected" actions need a selection; the "all" ones need a list.
        assert!(!action_enabled(
            TopObjectsAction::ReturnSelected,
            true,
            3,
            0,
            false
        ));
        assert!(action_enabled(
            TopObjectsAction::ReturnSelected,
            true,
            3,
            1,
            false
        ));
        assert!(!action_enabled(
            TopObjectsAction::DisableAll,
            true,
            0,
            0,
            false
        ));
        assert!(action_enabled(
            TopObjectsAction::DisableAll,
            true,
            3,
            0,
            false
        ));
        // A request in flight holds the asking buttons, and only those.
        assert!(!action_enabled(TopObjectsAction::Refresh, true, 3, 1, true));
        assert!(action_enabled(
            TopObjectsAction::ReturnSelected,
            true,
            3,
            1,
            true
        ));
    }

    /// A window about a region the agent has left can be read and nothing else.
    #[test]
    fn a_window_whose_region_was_left_does_nothing() {
        let mut state = state(vec![item(1.0, "thing", "abe", (1.0, 2.0, 3.0))]);
        state.selected = state.items.iter().map(|item| item.task_id).collect();
        assert!(super::window_action_enabled(
            TopObjectsAction::ReturnSelected,
            true,
            &state
        ));
        state.is_current = false;
        for action in [
            TopObjectsAction::ShowBeacon,
            TopObjectsAction::ReturnSelected,
            TopObjectsAction::ReturnAll,
            TopObjectsAction::DisableSelected,
            TopObjectsAction::DisableAll,
            TopObjectsAction::Refresh,
            TopObjectsAction::FilterByObject,
        ] {
            assert!(!super::window_action_enabled(action, true, &state));
        }
    }

    /// The two windows as the viewer schedules them: the plugin's systems, the
    /// floater manager, and no grid — what a `LandStatRequest` goes out over,
    /// which window a `LandStatReply` reaches, and one instance per region.
    mod session {
        use super::super::{
            OpenTopObjects, TOP_COLLIDERS_FLOATER_ID, TOP_SCRIPTS_FLOATER_ID, TopObjectsKind,
            TopObjectsPlugin, TopObjectsState, TopObjectsUi, TopObjectsView,
        };
        use crate::floater::{Floater, FloaterPlugin};
        use crate::ui::{UiPanelShown, UiRoot};
        use crate::virtual_list::VirtualList;
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            AgentKey, CircuitId, Command, GridCoordinates, LandStatItem, LandStatReportType,
            LandStatScore, Maturity, ObjectKey, ProductType, RegionCoordinates, RegionHandle,
            RegionIdentity, RegionLocalObjectId, RegionName, RegionTerrainComposition, SlCommand,
            SlCurrentRegion, SlEvent, SlIdentity, SlRegionIdentity, SlSessionEvent, Uuid,
        };

        /// A boxed error so tests use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// The circuit the test identity is on.
        const fn circuit() -> CircuitId {
            CircuitId::new(1)
        }

        /// A region the agent may manage, identified by `id`.
        fn region(id: u128, name: &str) -> RegionIdentity {
            RegionIdentity {
                sim_name: RegionName::try_new(name).ok(),
                region_id: Uuid::from_u128(id),
                region_handle: RegionHandle::from_grid(1000, 1000),
                grid_coordinates: GridCoordinates::new(1000, 1000),
                region_flags: 0,
                region_flags_extended: 0,
                region_protocols: 0,
                maturity: Maturity::Pg,
                product: ProductType::Unknown,
                product_sku: String::new(),
                product_name: String::new(),
                cpu_class_id: 0,
                cpu_ratio: 0,
                sim_owner: Uuid::nil(),
                is_estate_manager: true,
                water_height: 20.0,
                billable_factor: 1.0,
                terrain: RegionTerrainComposition {
                    detail_textures: [Uuid::nil(); 4],
                    start_heights: [0.0; 4],
                    height_ranges: [0.0; 4],
                },
            }
        }

        /// An app with the floater manager, this module's plugin, and the world
        /// facts its systems read — no grid, no window.
        fn top_objects_app() -> App {
            let mut app = App::new();
            let identity = SlIdentity {
                agent_id: Some(AgentKey::from(Uuid::from_u128(0xA9))),
                circuit_id: Some(circuit()),
                region_handle: Some(RegionHandle::from_grid(1000, 1000)),
                ..SlIdentity::default()
            };
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .insert_resource(identity)
                .init_resource::<Time>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<bevy::input_focus::InputFocus>()
                .add_plugins((FloaterPlugin, TopObjectsPlugin));
            crate::i18n::install_untranslated(&mut app);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Make `identity` the region the agent is in, replacing whichever was.
        fn stand_in(app: &mut App, identity: &RegionIdentity) {
            let live: Vec<Entity> = app
                .world_mut()
                .query_filtered::<Entity, With<SlCurrentRegion>>()
                .iter(app.world())
                .collect();
            for entity in live {
                app.world_mut().entity_mut(entity).despawn();
            }
            app.world_mut()
                .spawn((SlCurrentRegion, SlRegionIdentity(identity.clone())));
            app.update();
        }

        /// Ask for a report on a region, the way the Debug tab's buttons do.
        fn open(app: &mut App, report: LandStatReportType, on: &RegionIdentity) {
            app.world_mut().write_message(OpenTopObjects {
                report,
                region: Box::new(on.clone()),
            });
            app.update();
        }

        /// One report row.
        fn item(score: f32, name: &str, id: u128) -> LandStatItem {
            LandStatItem {
                task_local_id: RegionLocalObjectId(7),
                task_id: ObjectKey::from(Uuid::from_u128(id)),
                location: RegionCoordinates::new(128.0, 64.0, 30.0),
                score: LandStatScore::from_wire(LandStatReportType::TopScripts, score),
                task_name: name.to_owned(),
                owner_name: "Someone Resident".to_owned(),
                extended: None,
            }
        }

        /// Deliver a `LandStatReply`, as the session would.
        fn reply(app: &mut App, report: LandStatReportType, total: u32, items: Vec<LandStatItem>) {
            app.world_mut()
                .write_message(SlEvent(SlSessionEvent::LandStatReply {
                    report_type: report,
                    request_flags: 0,
                    total_object_count: total,
                    items,
                }));
            app.update();
        }

        /// Every `RequestLandStat` written so far, drained.
        fn requests(app: &mut App) -> Vec<(LandStatReportType, u32, String, i32)> {
            let mut messages = app.world_mut().resource_mut::<Messages<SlCommand>>();
            let drained: Vec<SlCommand> = messages.drain().collect();
            drained
                .into_iter()
                .filter_map(|command| match command.0 {
                    Command::RequestLandStat {
                        report_type,
                        request_flags,
                        filter,
                        parcel_local_id,
                    } => Some((report_type, request_flags, filter, parcel_local_id.id.0)),
                    _ => None,
                })
                .collect()
        }

        /// Every live report window: its kind and whether it is shown.
        fn windows(app: &mut App) -> Vec<(&'static str, bool)> {
            app.world_mut()
                .query::<(&Floater, &UiPanelShown)>()
                .iter(app.world())
                .filter_map(|(floater, shown)| {
                    matches!(
                        floater.id,
                        TOP_SCRIPTS_FLOATER_ID | TOP_COLLIDERS_FLOATER_ID
                    )
                    .then_some((floater.id, shown.0))
                })
                .collect()
        }

        /// Opening asks the region for the report the button named, over the
        /// whole region and with no filter.
        #[test]
        fn opening_asks_for_the_named_report() -> Result<(), TestError> {
            let mut app = top_objects_app();
            let alpha = region(0xA1, "Alpha");
            stand_in(&mut app, &alpha);
            open(&mut app, LandStatReportType::TopColliders, &alpha);

            assert_eq!(
                requests(&mut app),
                vec![(LandStatReportType::TopColliders, 0, String::new(), 0)]
            );
            assert_eq!(windows(&mut app), vec![(TOP_COLLIDERS_FLOATER_ID, true)]);
            Ok(())
        }

        /// The two reports are two windows, and each is one per region.
        #[test]
        fn two_reports_and_two_regions_are_four_windows() -> Result<(), TestError> {
            let mut app = top_objects_app();
            let (alpha, beta) = (region(0xA1, "Alpha"), region(0xB2, "Beta"));
            stand_in(&mut app, &alpha);
            open(&mut app, LandStatReportType::TopScripts, &alpha);
            open(&mut app, LandStatReportType::TopColliders, &alpha);
            // Re-opening the same report on the same region raises the window it
            // already has rather than spawning a second.
            open(&mut app, LandStatReportType::TopScripts, &alpha);
            assert_eq!(windows(&mut app).len(), 2);

            stand_in(&mut app, &beta);
            open(&mut app, LandStatReportType::TopScripts, &beta);
            open(&mut app, LandStatReportType::TopColliders, &beta);
            assert_eq!(windows(&mut app).len(), 4);
            Ok(())
        }

        /// A reply reaches the window of its own report kind, and not the other
        /// one.
        #[test]
        fn a_reply_reaches_only_its_own_report() -> Result<(), TestError> {
            let mut app = top_objects_app();
            let alpha = region(0xA1, "Alpha");
            stand_in(&mut app, &alpha);
            open(&mut app, LandStatReportType::TopScripts, &alpha);
            open(&mut app, LandStatReportType::TopColliders, &alpha);
            reply(
                &mut app,
                LandStatReportType::TopScripts,
                5,
                vec![item(3.0, "first", 1), item(2.0, "second", 2)],
            );
            // A second message of the same report appends to the same list.
            reply(
                &mut app,
                LandStatReportType::TopScripts,
                5,
                vec![item(1.0, "third", 3)],
            );

            let counts: Vec<(TopObjectsKind, usize, u32)> = app
                .world_mut()
                .query::<(&TopObjectsKind, &TopObjectsState)>()
                .iter(app.world())
                .map(|(kind, state)| (*kind, state.items.len(), state.total))
                .collect();
            assert!(counts.contains(&(TopObjectsKind::Scripts, 3, 5)));
            assert!(counts.contains(&(TopObjectsKind::Colliders, 0, 0)));

            // The list shows every row it holds, in score order.
            let (view, viewport) = app
                .world_mut()
                .query::<(&TopObjectsKind, &TopObjectsView, &TopObjectsUi)>()
                .iter(app.world())
                .find_map(|(kind, view, ui)| {
                    (*kind == TopObjectsKind::Scripts).then(|| (view.order.clone(), ui.viewport))
                })
                .ok_or("no scripts window")?;
            assert_eq!(view, vec![0, 1, 2]);
            let list = app
                .world()
                .get::<VirtualList>(viewport)
                .ok_or("no virtual list")?;
            assert_eq!(list.item_count, 3);
            Ok(())
        }

        /// A window about the region the agent has left keeps its report, asks
        /// nothing more, and takes no reply meant for the region it is in now.
        #[test]
        fn leaving_a_region_freezes_its_window() -> Result<(), TestError> {
            let mut app = top_objects_app();
            let (alpha, beta) = (region(0xA1, "Alpha"), region(0xB2, "Beta"));
            stand_in(&mut app, &alpha);
            open(&mut app, LandStatReportType::TopScripts, &alpha);
            reply(
                &mut app,
                LandStatReportType::TopScripts,
                1,
                vec![item(3.0, "first", 1)],
            );
            stand_in(&mut app, &beta);
            drop(requests(&mut app));

            // A reply for the region the agent is in now does not land in the
            // frozen window…
            reply(
                &mut app,
                LandStatReportType::TopScripts,
                9,
                vec![item(1.0, "elsewhere", 2)],
            );
            let frozen: Vec<(bool, usize, u32)> = app
                .world_mut()
                .query::<&TopObjectsState>()
                .iter(app.world())
                .map(|state| (state.is_current, state.items.len(), state.total))
                .collect();
            assert_eq!(frozen, vec![(false, 1, 1)]);

            // …and re-opening it on its own region asks nothing, because that
            // request would go out on the circuit of the region the agent is in.
            open(&mut app, LandStatReportType::TopScripts, &alpha);
            assert!(requests(&mut app).is_empty());
            Ok(())
        }
    }
}
