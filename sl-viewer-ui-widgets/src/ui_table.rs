//! A **reusable table widget** (`viewer-ui-table-widget`).
//!
//! Several surfaces have each hand-rolled the *same* table: a fixed header over
//! virtualized rows with a mix of flexible and fixed-width columns — the group
//! profile's members / notices tables, the People friends / groups lists,
//! inventory columns. Each re-solved (often inconsistently) column widths,
//! header↔row alignment, per-cell clipping / no-wrap, ellipsis truncation, and
//! row virtualization. This module owns that once.
//!
//! # What it provides
//!
//! - A **column spec** ([`TableColumn`]) — a fluent header key, a width that is
//!   either a `flex-grow` share or a fixed (draggable-resizable) pixel width, a
//!   text alignment, and whether the column is a **sort** key.
//! - A **header row** derived from the columns ([`spawn_table`]), guaranteed
//!   aligned with the body cells because both carry [`TableColumnCell`] and the
//!   one `sync_table_column_widths` system writes the same width to each.
//! - **Cells that clip + no-wrap** and reveal a **locale-aware ellipsis**
//!   (`i18n::LocaleEllipsisMarker`, driven by `i18n` exactly
//!   like the tab widget's) when the value overflows the column.
//! - **Virtualized, recycled rows** on top of `virtual_list`: the
//!   consumer builds a row's cells once ([`spawn_table_row`]) and binds them from
//!   its own projection, exactly as the `populate_*` / `bind_*` pattern already
//!   does.
//! - **Column sorting** with the sort order **persisted** per avatar
//!   ([`TableSort`] ↔ [`ViewerSettings`]), and **draggable column widths** also
//!   persisted.
//! - **A disabled state**: [`InteractionDisabled`](bevy::ui::InteractionDisabled)
//!   on the table root makes the sort, resize and row-selection gestures inert
//!   and greys the header; on one header cell or one row it disables just that
//!   part. Scrolling and reading a disabled table stay live, because a table the
//!   agent may not *change* is still one they must be able to *read*.
//!
//! # The split: generic chrome, consumer-supplied rows and ordering
//!
//! The widget knows nothing about what a row *contains* or how two rows *order*.
//! It owns the header, the columns, the resize/sort gestures, the ellipsis, and
//! the scroll; the consumer owns its item model, projects each row's cell text on
//! bind, and re-sorts its own data when the table's [`TableSort`] revision
//! advances (mapping a column index to its own comparator). That keeps the
//! ordering logic — which is domain-specific — out of the widget while the
//! widget owns everything that was being re-solved inconsistently.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use sl_settings::SettingValue;

use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui_ellipsis::{RevealEllipsis, spawn_ellipsis_marker};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::virtual_list::{VirtualList, VirtualRow, VirtualViewport, amend_row_node};

/// The smallest a resizable column may be dragged, in logical pixels — enough to
/// keep its header legible.
const MIN_COLUMN_WIDTH: f32 = 24.0;

/// The largest a resizable column may be dragged, in logical pixels — a generous
/// bound so a wide value can be read without letting one column swallow the row.
const MAX_COLUMN_WIDTH: f32 = 800.0;

/// The width of a column's drag-to-resize handle, in logical pixels — a strip
/// straddling the column's trailing border, wide enough to grab comfortably.
const RESIZER_WIDTH: f32 = 7.0;

/// The faint colour of a resize handle, so the draggable column borders are
/// discoverable without shouting.
const RESIZER_COLOR: Color = Color::srgba(0.62, 0.66, 0.74, 0.35);

/// The gap between a header label and its sort-direction arrow, in logical pixels.
const ARROW_GAP: f32 = 2.0;

/// The sort-direction arrow shown on the primary sort column's header when it is
/// ascending (`▲`).
const SORT_ASCENDING_GLYPH: &str = "\u{25B2}";

/// The sort-direction arrow shown on the primary sort column's header when it is
/// descending (`▼`).
const SORT_DESCENDING_GLYPH: &str = "\u{25BC}";

/// The ellipsis shown before any locale bundle has resolved `i18n`'s
/// `ui-ellipsis` — the Latin single ellipsis, matching the tab widget's default.
const FALLBACK_ELLIPSIS: &str = "\u{2026}";

/// A [disabled](bevy::ui::InteractionDisabled) header's label / sort-arrow
/// colour — dimmed so a header that will not answer a click reads as inert. The
/// same grey the combo and text-field widgets use for their disabled text.
const DISABLED_TEXT_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

// ---------------------------------------------------------------------------
// Column / table specification (static, const-constructible).
// ---------------------------------------------------------------------------

/// How a column claims horizontal space.
#[derive(Debug, Clone, Copy)]
pub enum TableColumnWidth {
    /// A flexible column that grows to fill the row's slack, sharing it in
    /// proportion to this `flex-grow` factor. Not drag-resizable (it *is* the
    /// slack); at least one column is usually flexible so the row fills its width.
    Flex(f32),
    /// A fixed-width column, drag-resizable between `MIN_COLUMN_WIDTH` and
    /// `MAX_COLUMN_WIDTH`. `default` is its width until a persisted or dragged
    /// value replaces it.
    Fixed {
        /// The column's initial pixel width.
        default: f32,
    },
}

/// A cell value's horizontal alignment within its column.
#[derive(Debug, Clone, Copy)]
pub enum TableAlign {
    /// Leading edge (the common case: a name or subject), so the *start* of a
    /// long value shows and the *end* clips under the trailing ellipsis.
    Start,
    /// Centred (a single glyph in a fixed column, e.g. a presence dot).
    Center,
    /// Trailing edge (a right-aligned number, e.g. a contribution).
    End,
}

impl TableAlign {
    /// The `justify_content` this alignment maps to inside a cell's clip
    /// container.
    const fn justify(self) -> JustifyContent {
        match self {
            Self::Start => JustifyContent::Start,
            Self::Center => JustifyContent::Center,
            Self::End => JustifyContent::End,
        }
    }
}

/// What a column's header and body cells contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableColumnKind {
    /// The widget owns the cell: a clipped, no-wrap value with a locale ellipsis
    /// (the common case). Its header is the translated [`TableColumn::header_key`].
    Text,
    /// The **consumer** owns the cell: the widget spawns only an empty sized
    /// container (width-synced) for the header and each body row, and the consumer
    /// fills it with its own content — icons, checkboxes, a grouped sub-header.
    /// Used by the People friends list's permission grid. No label, no ellipsis,
    /// and no built-in sort is added, and the column is **not** drag-resizable
    /// (its content is fixed-size).
    Custom,
}

/// One column of a [`TableSpec`] — its header, width, alignment, and whether it
/// is a sort key. Const-constructible so a table's columns are a `static` array.
#[derive(Debug, Clone, Copy)]
pub struct TableColumn {
    /// The Fluent key of the column's header label ([`TableColumnKind::Text`]
    /// only; ignored for a [`TableColumnKind::Custom`] column).
    pub header_key: &'static str,
    /// A stable token identifying this column in the persisted sort / width
    /// strings, so persistence survives a later column reorder or rename.
    pub token: &'static str,
    /// What the column's cells contain.
    pub kind: TableColumnKind,
    /// How the column claims width.
    pub width: TableColumnWidth,
    /// The cell value's alignment.
    pub align: TableAlign,
    /// Whether the widget's built-in sort orders by this column when clicked (only
    /// honoured while [`TableSpec::builtin_sort`] is set).
    pub sortable: bool,
}

/// A default sort level for a fresh table, before any persisted order loads.
#[derive(Debug, Clone, Copy)]
pub struct TableSortDefault {
    /// The column index this level orders by.
    pub column: usize,
    /// Ascending (else descending).
    pub ascending: bool,
}

/// The full specification of a table: its columns, geometry, palette, and where
/// its sort order / column widths persist. A `static`, shared by every instance
/// of the same table.
#[derive(Debug, Clone, Copy)]
pub struct TableSpec {
    /// A short element name, used in the debug `Name`s of the table's entities.
    pub element: &'static str,
    /// The columns, left to right.
    pub columns: &'static [TableColumn],
    /// The row-selection mode the widget provides.
    pub selection: TableSelectionMode,
    /// The default (pre-persistence) sort levels, most-significant first. Empty
    /// leaves the table unsorted (the consumer's natural order).
    pub default_sort: &'static [TableSortDefault],
    /// Whether the widget owns the sort: clickable sortable headers, `▲`/`▼`
    /// arrows, and (with [`sort_setting`](Self::sort_setting)) persistence. Set to
    /// `false` for a table whose consumer drives its own ordering — the People
    /// friends list keeps its bespoke 8-way sort, so the widget adds no sort
    /// observers or arrows and the consumer wires its own header clicks.
    pub builtin_sort: bool,
    /// The uniform row height, in logical pixels.
    pub row_height: f32,
    /// The font size of header and cell text, in logical pixels.
    pub font_size: f32,
    /// The header label colour.
    pub header_color: Color,
    /// The default cell value colour (a bind may override a given cell).
    pub cell_color: Color,
    /// The gap between columns, in logical pixels — identical on the header and
    /// every row, which (with the shared widths) is what keeps them aligned.
    pub column_gap: f32,
    /// The horizontal padding inside the header and each row, in logical pixels.
    pub row_padding: f32,
    /// The persisted-setting name for this table's sort order, or `None` to not
    /// persist it. The consumer registers it (see [`register_table_settings`]).
    pub sort_setting: Option<&'static str>,
    /// The persisted-setting name for this table's column widths, or `None`.
    pub widths_setting: Option<&'static str>,
}

/// How the table lets the user select rows — owned by the widget (the highlight,
/// the click handling, the [`TableState`] read-back), with *what happens* on a
/// selection change left to the consumer (it reads [`TableState::selected`] and
/// [`TableState::selection_revision`], exactly as it reads the sort). Mirrors the
/// reference `LLScrollListCtrl`'s `multi_select` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableSelectionMode {
    /// No widget-owned selection: rows are not selectable and the widget never
    /// paints a selection highlight (a display-only table, or one whose consumer
    /// wires its own row selection). The default.
    #[default]
    None,
    /// At most one selected row; a click selects only that row.
    Single,
    /// Any number of selected rows: a plain click selects one, `Ctrl`+click
    /// toggles a row, `Shift`+click selects the range from the anchor — the
    /// radar's mode, and the algebra the avatar picker's (non-table) results
    /// list borrows through [`apply_selection_click`].
    Multi,
}

// ---------------------------------------------------------------------------
// Pure sort state (generalised from the People friends list, keyed by column
// index instead of a bespoke enum).
// ---------------------------------------------------------------------------

/// The most sort levels a table remembers; clicks past this drop the least
/// significant.
const MAX_SORT_KEYS: usize = 6;

/// One level of a multi-column sort: a column and its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSortKey {
    /// The column index this level orders by.
    pub column: usize,
    /// Ascending (else descending).
    pub ascending: bool,
}

/// The ordered multi-column sort — most-significant key first. A header click
/// promotes its column to the front (or flips its direction if already front),
/// demoting the previous order to tie-breakers: "sort by the last-clicked column,
/// then the one before that, …". Persisted per avatar.
///
/// Pure and Bevy-free so it is unit-tested in isolation; the widget carries one
/// on each table's [`TableState`].
#[derive(Debug, Clone, Default)]
pub struct TableSort {
    /// The sort levels, most-significant first.
    keys: Vec<TableSortKey>,
}

impl TableSort {
    /// A sort seeded from a spec's [`TableSpec::default_sort`].
    fn from_defaults(defaults: &[TableSortDefault]) -> Self {
        Self {
            keys: defaults
                .iter()
                .map(|level| TableSortKey {
                    column: level.column,
                    ascending: level.ascending,
                })
                .collect(),
        }
    }

    /// Apply a header click on `column`: flip the front column's direction if it
    /// is already primary, else promote `column` to the front (demoting the rest,
    /// dropping the least significant past [`MAX_SORT_KEYS`]). A freshly-promoted
    /// column starts ascending.
    fn click(&mut self, column: usize) {
        if let Some(front) = self.keys.first_mut()
            && front.column == column
        {
            front.ascending = !front.ascending;
            return;
        }
        self.keys.retain(|key| key.column != column);
        self.keys.insert(
            0,
            TableSortKey {
                column,
                ascending: true,
            },
        );
        self.keys.truncate(MAX_SORT_KEYS);
    }

    /// The primary (most-significant) sort key, if any — what the header arrow
    /// reflects.
    #[must_use]
    pub fn primary(&self) -> Option<TableSortKey> {
        self.keys.first().copied()
    }

    /// The full key stack, most-significant first — the consumer walks it to
    /// order two rows (comparing by each column until one breaks the tie).
    #[must_use]
    pub fn keys(&self) -> &[TableSortKey] {
        &self.keys
    }

    /// Encode the sort as a compact `token:dir,token:dir` string (columns named
    /// by their stable [`TableColumn::token`], so persistence survives a reorder).
    fn encode(&self, columns: &[TableColumn]) -> String {
        self.keys
            .iter()
            .filter_map(|key| {
                columns.get(key.column).map(|column| {
                    let dir = if key.ascending { "a" } else { "d" };
                    format!("{}:{dir}", column.token)
                })
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse a persisted sort string against `columns`, falling back to the
    /// spec's defaults when it is empty or wholly unrecognised (dropping
    /// duplicate / unknown tokens).
    fn parse(text: &str, columns: &[TableColumn], defaults: &[TableSortDefault]) -> Self {
        let mut keys: Vec<TableSortKey> = Vec::new();
        for part in text.split(',') {
            let mut fields = part.split(':');
            let (Some(token), Some(dir)) = (fields.next(), fields.next()) else {
                continue;
            };
            let Some(column) = columns.iter().position(|column| column.token == token) else {
                continue;
            };
            if keys.iter().any(|key| key.column == column) {
                continue;
            }
            keys.push(TableSortKey {
                column,
                ascending: dir == "a",
            });
        }
        if keys.is_empty() {
            Self::from_defaults(defaults)
        } else {
            Self { keys }
        }
    }
}

/// Clamp a column-resize drag to the legal width range.
///
/// Pure so the sign/clamp is unit-tested without a running app. A drag on a
/// column's trailing handle widens it as the pointer moves in the inline
/// direction; `direction_sign` folds in right-to-left layout, where the inline
/// direction points the other way on screen.
fn resize_column_width(current: f32, delta_x: f32, direction_sign: f32) -> f32 {
    (current + direction_sign * delta_x).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
}

// ---------------------------------------------------------------------------
// Runtime components.
// ---------------------------------------------------------------------------

/// The live state of a table, on its **root** entity: the current per-column
/// widths, the sort order, a revision the consumer watches to re-sort, and the
/// persistence bookkeeping.
#[derive(Debug, Component)]
pub struct TableState {
    /// The table's specification.
    spec: &'static TableSpec,
    /// The current width of each column, indexed to match
    /// [`TableSpec::columns`]. For a [`TableColumnWidth::Fixed`] column this is
    /// its live pixel width (dragged / persisted); for a
    /// [`TableColumnWidth::Flex`] column it is the (unchanging) grow factor.
    widths: Vec<f32>,
    /// The current sort order.
    sort: TableSort,
    /// Bumped whenever the sort order changes, so a consumer re-sorts its data
    /// exactly when the order actually moved (never on a mere width drag).
    sort_revision: u64,
    /// Set when the sort or a width changed and should be written back to
    /// settings; cleared once [`persist_table_state`] has saved it.
    dirty: bool,
    /// Whether the persisted sort / widths have been seeded from settings yet
    /// (a once-guard, since the account scope loads after the table spawns).
    seeded: bool,
    /// The selected row **data indices** (into the consumer's ordered items), in
    /// ascending order. Empty when nothing is selected; at most one for
    /// [`TableSelectionMode::Single`]. Stored as indices (not entities) so it
    /// survives row recycling.
    selected: Vec<usize>,
    /// The range anchor for a `Shift`+click in [`TableSelectionMode::Multi`] — the
    /// last row plainly selected or `Ctrl`-toggled on.
    anchor: Option<usize>,
    /// Bumped whenever the selection changes, so a consumer reacts (populates a
    /// detail pane, enables an action) exactly when the selection moved.
    selection_revision: u64,
}

impl TableState {
    /// The current sort order — the consumer reads this (plus [`sort_revision`])
    /// to order its rows.
    ///
    /// [`sort_revision`]: Self::sort_revision
    #[must_use]
    pub const fn sort(&self) -> &TableSort {
        &self.sort
    }

    /// The sort revision — a consumer stores the value it last sorted at and
    /// re-sorts when it advances.
    #[must_use]
    pub const fn sort_revision(&self) -> u64 {
        self.sort_revision
    }

    /// The table's selection mode.
    #[must_use]
    pub const fn selection_mode(&self) -> TableSelectionMode {
        self.spec.selection
    }

    /// The selected row data indices, ascending. Empty when nothing is selected.
    #[must_use]
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }

    /// The range anchor — the last row plainly selected or `Ctrl`-toggled on, the
    /// far end a `Shift`+click ranges from. A consumer that re-keys the selection
    /// across a re-sort ([`set_selection`]) carries this over too, or the next
    /// `Shift`+click ranges from whatever row the old index now happens to be.
    ///
    /// [`set_selection`]: Self::set_selection
    #[must_use]
    pub const fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// The single selected index for a single-select consumer (the first, if
    /// several are somehow selected).
    #[must_use]
    pub fn primary_selected(&self) -> Option<usize> {
        self.selected.first().copied()
    }

    /// Whether `index` is currently selected.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    /// The selection revision — a consumer stores the value it last acted on and
    /// re-reads when it advances.
    #[must_use]
    pub const fn selection_revision(&self) -> u64 {
        self.selection_revision
    }

    /// Clear the selection (e.g. when the consumer replaces the table's data, so a
    /// stale index does not point at a different row). A no-op — with no revision
    /// bump — when already empty.
    pub fn clear_selection(&mut self) {
        if !self.selected.is_empty() || self.anchor.is_some() {
            self.selected.clear();
            self.anchor = None;
            self.selection_revision = self.selection_revision.wrapping_add(1);
        }
    }

    /// Replace the selection (and its range anchor) wholesale.
    ///
    /// The widget stores a selection as **data indices**, which is what a click
    /// hands it; a consumer whose rows re-sort or re-filter under the user (the
    /// radar re-sorts every second) keeps the selection by its own row identity
    /// and re-projects it here after each rebuild. Out-of-order or repeated
    /// indices are normalised, and the revision only advances on a real change,
    /// so a rebuild that leaves the selection where it was is not a selection
    /// event.
    pub fn set_selection(&mut self, selected: Vec<usize>, anchor: Option<usize>) {
        let mut selected = selected;
        selected.sort_unstable();
        selected.dedup();
        if self.selected != selected || self.anchor != anchor {
            self.selected = selected;
            self.anchor = anchor;
            self.selection_revision = self.selection_revision.wrapping_add(1);
        }
    }

    /// Apply a click on row `index` under the current mode and modifier keys,
    /// bumping the revision only on a real change. The algebra itself is
    /// [`apply_selection_click`], shared with the lists that are not tables.
    fn apply_click(&mut self, index: usize, ctrl: bool, shift: bool) {
        let mode = self.spec.selection;
        if mode == TableSelectionMode::None {
            return;
        }
        let before = self.selected.clone();
        apply_selection_click(
            &mut self.selected,
            &mut self.anchor,
            index,
            mode == TableSelectionMode::Multi,
            ctrl,
            shift,
        );
        if self.selected != before {
            self.selection_revision = self.selection_revision.wrapping_add(1);
        }
    }
}

/// Apply a click on row `index` to a selection of row **data indices** and its
/// range anchor: `Ctrl` toggles the row in or out, `Shift` selects the inclusive
/// range from the anchor, and a plain click selects only the clicked row (which
/// is also all a `multi == false` list ever does). `selected` stays sorted and
/// duplicate-free.
///
/// Pure, so the algebra is unit-testable without an ECS world — and free rather
/// than a [`TableState`] method because it is not only tables that let the user
/// pick several rows: the avatar picker's results list
/// (`avatar_picker`) is a plain column with the same `Ctrl` / `Shift`
/// idiom, and one implementation of "what a modified click means" is what keeps
/// the two feeling like one viewer.
pub fn apply_selection_click(
    selected: &mut Vec<usize>,
    anchor: &mut Option<usize>,
    index: usize,
    multi: bool,
    ctrl: bool,
    shift: bool,
) {
    if multi && ctrl {
        // Ctrl+click toggles the row in or out of the selection.
        if let Some(position) = selected.iter().position(|row| *row == index) {
            selected.remove(position);
        } else {
            selected.push(index);
            selected.sort_unstable();
        }
        *anchor = Some(index);
    } else if multi && shift {
        // Shift+click selects the inclusive range from the anchor; the anchor
        // stays put across a shift-drag of the far end.
        let start = anchor.unwrap_or(index);
        let (low, high) = (start.min(index), start.max(index));
        *selected = (low..=high).collect();
    } else {
        // A plain click (either mode) selects only the clicked row.
        *selected = vec![index];
        *anchor = Some(index);
    }
}

/// On a pooled body **row**, naming the table it belongs to, so the widget's
/// selection click handler and highlight can find the row's [`TableState`].
#[derive(Component, Debug, Clone, Copy)]
struct TableRow {
    /// The table root this row belongs to.
    table: Entity,
}

/// The background of a selected row — a translucent accent, matching the bespoke
/// selection highlights the migrated tables used.
const SELECTED_ROW_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);

/// Links a header or body cell (the width-bearing node) to its table and column,
/// so `sync_table_column_widths` keeps every cell of a column the same width as
/// its header.
#[derive(Component, Debug, Clone, Copy)]
pub struct TableColumnCell {
    /// The table root this cell belongs to.
    pub table: Entity,
    /// The column index within the table's spec.
    pub column: usize,
}

/// On a sortable column's header arrow node — the `▲` / `▼` indicator, updated
/// from the table's primary sort key.
#[derive(Component, Debug, Clone, Copy)]
struct TableHeaderArrow {
    /// The table root this header belongs to.
    table: Entity,
    /// The column index this header sorts by.
    column: usize,
}

/// On every text node the *widget* owns in a header cell — the column label and
/// (where there is one) its sort arrow — naming the table root and the header
/// cell it sits in, so `reflect_table_disabled` can grey exactly the text a
/// disabled table stops answering for.
///
/// The body cells deliberately carry no such marker: a consumer sets each cell's
/// colour itself on bind ([`set_table_cell`]), so a widget system that wrote them
/// would have nothing to restore them to when the table is re-enabled.
#[derive(Component, Debug, Clone, Copy)]
struct TableHeaderText {
    /// The table root this text belongs to.
    table: Entity,
    /// The header cell this text sits in — the entity a consumer disables to
    /// grey out (and make inert) one column rather than the whole table.
    cell: Entity,
}

/// A handle to a freshly-spawned table's key entities.
#[derive(Debug, Clone)]
pub struct TableHandle {
    /// The table root (a column: header over viewport). Carries [`TableState`];
    /// parent it wherever the table should live.
    pub root: Entity,
    /// The header row (its cells are [`header_cells`](Self::header_cells)) — a
    /// consumer may style it (e.g. a header background).
    pub header: Entity,
    /// The virtualized scrolling viewport (carries [`VirtualList`]). The consumer
    /// keeps its item count current and pools rows under it.
    pub viewport: Entity,
    /// The header cell of each column, in column order — a consumer fills the
    /// [`TableColumnKind::Custom`] ones (and adds its own sort click / arrow to a
    /// column whose sort it drives itself).
    pub header_cells: Vec<Entity>,
}

impl TableHandle {
    /// The header cell of `column`, if present.
    #[must_use]
    pub fn header_cell(&self, column: usize) -> Option<Entity> {
        self.header_cells.get(column).copied()
    }
}

/// The cell nodes of a pooled row, in column order — returned by
/// [`spawn_table_row`] for the consumer to bind its projection into. For a
/// [`TableColumnKind::Text`] column the entry is the value text node (pass it to
/// [`set_table_cell`]); for a [`TableColumnKind::Custom`] column it is the empty
/// sized container the consumer fills with its own content.
#[derive(Component, Debug, Clone)]
pub struct TableRowCells {
    /// One cell node per column, left to right.
    pub cells: Vec<Entity>,
}

impl TableRowCells {
    /// The cell node for `column`, if present.
    #[must_use]
    pub fn cell(&self, column: usize) -> Option<Entity> {
        self.cells.get(column).copied()
    }
}

// ---------------------------------------------------------------------------
// Spawning.
// ---------------------------------------------------------------------------

/// Spawn a table under `parent`: a column holding the derived header over an
/// empty virtualized viewport. The consumer pools rows under
/// [`TableHandle::viewport`] with [`spawn_table_row`] and keeps its
/// [`VirtualList::item_count`] current.
pub fn spawn_table(
    commands: &mut Commands,
    parent: Entity,
    spec: &'static TableSpec,
) -> TableHandle {
    let widths = spec
        .columns
        .iter()
        .map(|column| match column.width {
            TableColumnWidth::Flex(grow) => grow,
            TableColumnWidth::Fixed { default } => default,
        })
        .collect();
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..default()
            },
            TableState {
                spec,
                widths,
                sort: TableSort::from_defaults(spec.default_sort),
                sort_revision: 0,
                dirty: false,
                seeded: false,
                selected: Vec::new(),
                anchor: None,
                selection_revision: 0,
            },
            Name::new(format!("{}:table", spec.element)),
            ChildOf(parent),
        ))
        .id();
    let (header, header_cells) = spawn_table_header(commands, root, spec);
    let viewport = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..default()
            },
            VirtualList::new(spec.row_height),
            VirtualViewport,
            Pickable::default(),
            Name::new(format!("{}:table-viewport", spec.element)),
            ChildOf(root),
        ))
        .id();
    // The overlay scrollbar every long table needs (hidden while content fits).
    sl_viewer_ui_core::virtual_list::spawn_virtual_scrollbar(commands, viewport);
    // Focus the viewport on a primary click, so the wheel scrolls it. The observed
    // entity is captured (not read from the event target, which bubbles up from a
    // clicked row), so a click on a row still focuses the viewport it lives in.
    commands.entity(viewport).observe(
        move |press: On<Pointer<Press>>, mut focus: ResMut<InputFocus>| {
            if press.button == PointerButton::Primary {
                focus.set(viewport, FocusCause::Navigated);
            }
        },
    );
    TableHandle {
        root,
        header,
        viewport,
        header_cells,
    }
}

/// Build the header row from the spec's columns — one header cell per column,
/// each with (for a text column) a label + optional sort arrow / click observer,
/// or (for a custom column) an empty sized container for the consumer to fill, and
/// (for a fixed column) a drag-to-resize handle on its trailing edge. Returns each
/// column's header cell so a consumer can fill a custom header or add its own sort.
fn spawn_table_header(
    commands: &mut Commands,
    root: Entity,
    spec: &'static TableSpec,
) -> (Entity, Vec<Entity>) {
    let header = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(spec.column_gap),
                padding: UiRect::horizontal(Val::Px(spec.row_padding)),
                ..default()
            },
            Name::new(format!("{}:table-header", spec.element)),
            ChildOf(root),
        ))
        .id();
    let cells: Vec<Entity> = spec
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| spawn_header_cell(commands, header, root, spec, index, column))
        .collect();
    // A drag handle on each internal column border (between column i and i+1)
    // whose drag would land on a resizable column — parented to the left cell's
    // trailing edge.
    for left in 0..spec.columns.len().saturating_sub(1) {
        if border_draggable(spec, left)
            && let Some(&cell) = cells.get(left)
        {
            spawn_border_resizer(commands, cell, root, spec, left);
        }
    }
    (header, cells)
}

/// Whether column `index` claims its width flexibly.
fn column_is_flex(spec: &TableSpec, index: usize) -> bool {
    matches!(
        spec.columns.get(index).map(|column| column.width),
        Some(TableColumnWidth::Flex(_))
    )
}

/// Whether column `index` can be drag-resized: a fixed-width **text** column.
/// A flexible column owns the row's slack (nothing to drag), and a **custom**
/// column holds fixed-size content (icons / a grouped sub-header), so widening or
/// narrowing it is pointless — those are not resizable.
fn column_resizable(spec: &TableSpec, index: usize) -> bool {
    spec.columns.get(index).is_some_and(|column| {
        column.kind == TableColumnKind::Text
            && matches!(column.width, TableColumnWidth::Fixed { .. })
    })
}

/// Whether the border between column `left` and `left + 1` can be dragged: the
/// column(s) the drag would resize must all be resizable. When one side is the
/// flexible column, the drag resizes only the other (fixed) side; when both are
/// fixed, the drag transfers width between them, so both must be resizable.
fn border_draggable(spec: &TableSpec, left: usize) -> bool {
    let right = left.saturating_add(1);
    let left_flex = column_is_flex(spec, left);
    let right_flex = column_is_flex(spec, right);
    match (left_flex, right_flex) {
        (true, true) => false,
        (true, false) => column_resizable(spec, right),
        (false, true) => column_resizable(spec, left),
        (false, false) => column_resizable(spec, left) && column_resizable(spec, right),
    }
}

/// Spawn one header cell (returning it): a sized container carrying (for a text
/// column) a clipped label + optional sort arrow, or (for a custom column) nothing
/// — plus, for a fixed column, a drag-to-resize handle on its trailing edge.
fn spawn_header_cell(
    commands: &mut Commands,
    header: Entity,
    root: Entity,
    spec: &'static TableSpec,
    index: usize,
    column: &TableColumn,
) -> Entity {
    let builtin_sort = spec.builtin_sort && column.sortable;
    let cell = commands
        .spawn((
            column_cell_node(column, spec.column_gap),
            TableColumnCell {
                table: root,
                column: index,
            },
            // Always pickable so a consumer can observe a custom / externally-sorted
            // header; the built-in sort blocks lower only when it owns the click.
            Pickable {
                should_block_lower: builtin_sort,
                is_hoverable: true,
            },
            Name::new(format!("{}:table-header-cell:{index}", spec.element)),
            ChildOf(header),
        ))
        .id();
    // A custom column's header is the consumer's to fill.
    if column.kind == TableColumnKind::Custom {
        return cell;
    }
    if builtin_sort {
        commands
            .entity(cell)
            .observe(sort_header_on_press(root, index));
    }
    // The label sits in a clip container so a long header truncates rather than
    // pushing the arrow out / overflowing into the next column.
    let clip = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                overflow: Overflow::clip(),
                justify_content: column.align.justify(),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(cell),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(column.header_key),
        TextLayout::no_wrap(),
        UiFont::Sans.at(spec.font_size),
        TextColor(spec.header_color),
        TableHeaderText { table: root, cell },
        Pickable::IGNORE,
        ChildOf(clip),
    ));
    if builtin_sort {
        commands.spawn((
            Text::new(String::new()),
            TextLayout::no_wrap(),
            UiFont::Sans.at(spec.font_size),
            TextColor(spec.header_color),
            TableHeaderText { table: root, cell },
            Node {
                flex_shrink: 0.0,
                margin: UiRect::left(Val::Px(ARROW_GAP)),
                ..default()
            },
            TableHeaderArrow {
                table: root,
                column: index,
            },
            Pickable::IGNORE,
            Name::new(format!("{}:table-header-arrow:{index}", spec.element)),
            ChildOf(cell),
        ));
    }
    cell
}

/// Spawn a drag handle straddling the border between column `left` and `left + 1`,
/// parented to the left column's header cell so it sits at that cell's trailing
/// edge. Dragging it moves the border ([`resize_border_on_drag`]).
fn spawn_border_resizer(
    commands: &mut Commands,
    cell: Entity,
    root: Entity,
    spec: &'static TableSpec,
    left: usize,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                // Straddle the trailing border so the grab target is centred on it.
                right: Val::Px(-RESIZER_WIDTH * 0.5),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(RESIZER_WIDTH),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new(format!("{}:table-border-resizer:{left}", spec.element)),
            ChildOf(cell),
        ))
        // A faint centred hairline so the draggable border is visible.
        .with_child((
            Node {
                width: Val::Px(1.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },
            BackgroundColor(RESIZER_COLOR),
            Pickable::IGNORE,
        ))
        .observe(resize_border_on_drag(root, left))
        .observe(persist_on_drag_end(root))
        // Stop a press on the handle from bubbling to the header cell and sorting —
        // grabbing the resize handle must not also flip the sort.
        .observe(|mut press: On<Pointer<Press>>| press.propagate(false));
}

/// The layout node for a column cell — a flexible cell (`Flex`) that grows, or a
/// fixed-width one that never shrinks. Header and body cells share this so a
/// column's header and values line up exactly.
///
/// The cell itself does **not** clip: text truncation is done by each text cell's
/// inner clip container, and clipping the outer cell would clip its absolute
/// resize handle out of the pointer-pick chain (the handle sits at the cell's
/// trailing edge). `position_type: Relative` makes the cell the resize handle's
/// positioning context.
fn column_cell_node(column: &TableColumn, column_gap: f32) -> Node {
    let base = Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(column_gap),
        position_type: PositionType::Relative,
        ..default()
    };
    match column.width {
        TableColumnWidth::Flex(grow) => Node {
            flex_grow: grow,
            flex_shrink: 1.0,
            min_width: Val::Px(0.0),
            ..base
        },
        TableColumnWidth::Fixed { default } => Node {
            width: Val::Px(default),
            flex_shrink: 0.0,
            ..base
        },
    }
}

/// Write the table's own row-node fields — the band shape, and the gap and
/// padding that line a body row up with the header — leaving every other field
/// of the node alone.
///
/// `top` and `display` belong to `virtual_list::layout_virtual_lists`, which
/// owns where a pooled row sits and whether it is parked, which is why this is
/// an amendment through `virtual_list::amend_row_node` rather than an insert.
const fn apply_table_row_node(node: &mut Node, spec: &TableSpec) {
    node.position_type = PositionType::Absolute;
    node.left = Val::Px(0.0);
    node.right = Val::Px(0.0);
    node.height = Val::Px(spec.row_height);
    node.align_items = AlignItems::Center;
    node.column_gap = Val::Px(spec.column_gap);
    node.padding = UiRect::horizontal(Val::Px(spec.row_padding));
}

/// Build one pooled row's cells under `row_entity` (already a
/// `virtual_list::VirtualRow`) and configure the row node to match the
/// header's gap / padding. Returns [`TableRowCells`] — one text node per column,
/// which the consumer keeps and binds its projection into on each rebind. Also
/// inserts that component on the row so widget systems can find the cells.
///
/// The row node is **amended, not replaced**: `top` and `display` belong to
/// [`virtual_list::layout_virtual_lists`](sl_viewer_ui_core::virtual_list::layout_virtual_lists),
/// which owns where a pooled row sits and whether it is parked.
pub fn spawn_table_row(
    commands: &mut Commands,
    row_entity: Entity,
    root: Entity,
    spec: &'static TableSpec,
) -> TableRowCells {
    amend_row_node(commands, row_entity, move |node| {
        apply_table_row_node(node, spec);
    });
    commands
        .entity(row_entity)
        .insert((
            BackgroundColor(Color::NONE),
            Pickable::default(),
            TableRow { table: root },
        ))
        // The widget's built-in selection click; a no-op for a
        // [`TableSelectionMode::None`] table, so a consumer with its own row
        // observer (the group members list) is unaffected.
        .observe(select_table_row_on_press);
    let mut cells = Vec::with_capacity(spec.columns.len());
    for (index, column) in spec.columns.iter().enumerate() {
        cells.push(spawn_body_cell(
            commands, row_entity, root, spec, index, column,
        ));
    }
    let row_cells = TableRowCells { cells };
    commands.entity(row_entity).insert(row_cells.clone());
    row_cells
}

/// Spawn one body cell. For a [`TableColumnKind::Text`] column: a sized, clipping
/// container whose value sits in an inner clip container (so the value truncates
/// rather than painting over the ellipsis), beside a trailing locale-ellipsis
/// marker hidden until the value overflows — returns the value's text node for
/// binding. For a [`TableColumnKind::Custom`] column: just the empty sized
/// container — returned for the consumer to fill.
fn spawn_body_cell(
    commands: &mut Commands,
    row_entity: Entity,
    root: Entity,
    spec: &'static TableSpec,
    index: usize,
    column: &TableColumn,
) -> Entity {
    let cell = commands
        .spawn((
            column_cell_node(column, spec.column_gap),
            TableColumnCell {
                table: root,
                column: index,
            },
            Pickable::IGNORE,
            Name::new(format!("{}:table-cell:{index}", spec.element)),
            ChildOf(row_entity),
        ))
        .id();
    if column.kind == TableColumnKind::Custom {
        return cell;
    }
    let clip = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                overflow: Overflow::clip(),
                justify_content: column.align.justify(),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(cell),
        ))
        .id();
    let text = commands
        .spawn((
            Text::new(String::new()),
            TextLayout::no_wrap(),
            UiFont::Sans.at(spec.font_size),
            TextColor(spec.cell_color),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(clip),
        ))
        .id();
    let marker = spawn_ellipsis_marker(
        commands,
        cell,
        spec.font_size,
        spec.cell_color,
        FALLBACK_ELLIPSIS,
    );
    commands.entity(marker).insert(Name::new(format!(
        "{}:table-cell-ellipsis:{index}",
        spec.element
    )));
    commands.entity(clip).insert(RevealEllipsis { marker });
    text
}

/// Set a cell's value text and colour in place (change-guarded), the binding
/// counterpart to [`spawn_table_row`]. A no-op if `cell` is not a live text node.
pub fn set_table_cell(
    texts: &mut Query<(&mut Text, &mut TextColor)>,
    cell: Entity,
    value: &str,
    color: Color,
) {
    if let Ok((mut text, mut text_color)) = texts.get_mut(cell) {
        if text.0 != value {
            value.clone_into(&mut text.0);
        }
        if text_color.0 != color {
            text_color.0 = color;
        }
    }
}

// ---------------------------------------------------------------------------
// Interaction: sort clicks and column resize.
// ---------------------------------------------------------------------------

/// The query the gesture observers use to test Bevy's advisory
/// [`InteractionDisabled`](bevy::ui::InteractionDisabled) marker: a filter-only
/// query, so `contains` is the whole of its use.
///
/// Named, because each of the three observer factories below spells it in its
/// `impl Fn(..)` return type, where an inline `Query<(), With<..>>` tips those
/// signatures over `clippy::type_complexity`.
type DisabledQuery<'w, 's> = Query<'w, 's, (), With<bevy::ui::InteractionDisabled>>;

/// Whether a table gesture is refused because the table, or the part of it the
/// pointer landed on, is [disabled](bevy::ui::InteractionDisabled).
///
/// Bevy's marker is advisory — it updates the a11y tree and nothing else — so
/// every widget enforces it in its own press / value-change observers. Both ends
/// are checked here, so a consumer can disable the whole table (the marker on the
/// root) or just one column's header or one row (the marker on that entity).
///
/// Only the *gesture* observers consult this. Scrolling a disabled table, and
/// focusing its viewport so the wheel reaches it, stay live: a table the agent
/// may not change is still one they must be able to read.
fn gesture_refused(disabled: &DisabledQuery, table: Entity, target: Entity) -> bool {
    disabled.contains(table) || disabled.contains(target)
}

/// An observer that sorts `table` by `column` on a primary-button header press.
fn sort_header_on_press(
    table: Entity,
    column: usize,
) -> impl Fn(On<Pointer<Press>>, Query<&mut TableState>, DisabledQuery) {
    move |mut press: On<Pointer<Press>>,
          mut tables: Query<&mut TableState>,
          disabled: DisabledQuery| {
        press.propagate(false);
        if press.button != PointerButton::Primary {
            return;
        }
        // A disabled table (or a disabled column header) still swallows the press
        // — the click lands nowhere rather than falling through to whatever is
        // behind — but changes nothing.
        if gesture_refused(&disabled, table, press.entity) {
            return;
        }
        if let Ok(mut state) = tables.get_mut(table) {
            state.sort.click(column);
            state.sort_revision = state.sort_revision.wrapping_add(1);
            state.dirty = true;
        }
    }
}

/// An observer that moves the border between column `left` and `left + 1` as its
/// handle is dragged, so the grabbed border follows the pointer.
///
/// The two adjacent columns share the change: dragging right grows the left
/// column and shrinks the right (the standard spreadsheet feel). Where one side is
/// the flexible column, only the fixed side is resized and the flexible column
/// absorbs the rest — again so the border tracks the pointer (a bare
/// resize-this-column would make a *distant* flex column absorb, moving the wrong
/// border, or the border backwards).
fn resize_border_on_drag(
    root: Entity,
    left: usize,
) -> impl Fn(On<Pointer<Drag>>, Query<&mut TableState>, DisabledQuery) {
    move |mut drag: On<Pointer<Drag>>,
          mut tables: Query<&mut TableState>,
          disabled: DisabledQuery| {
        drag.propagate(false);
        if drag.button != PointerButton::Primary {
            return;
        }
        if gesture_refused(&disabled, root, drag.entity) {
            return;
        }
        let dx = drag.delta.x;
        let Ok(mut state) = tables.get_mut(root) else {
            return;
        };
        let right = left.saturating_add(1);
        let left_flex = column_is_flex(state.spec, left);
        let right_flex = column_is_flex(state.spec, right);
        // Dragging right (dx > 0) moves the border right: the left column grows and
        // the right shrinks. A flexible neighbour is left to absorb, not resized.
        if !left_flex {
            adjust_width(&mut state.widths, left, dx);
        }
        if !right_flex {
            adjust_width(&mut state.widths, right, -dx);
        }
    }
}

/// Nudge column `index`'s stored width by `delta`, clamped to the legal range.
fn adjust_width(widths: &mut [f32], index: usize, delta: f32) {
    if let Some(width) = widths.get_mut(index) {
        // Assigned unconditionally: a drag event always carries motion, so a guard
        // would only trade a real write for a disallowed float comparison.
        *width = resize_column_width(*width, delta, 1.0);
    }
}

/// An observer that marks a table dirty (to persist its widths) when a column
/// resize drag ends — so the write happens once per gesture, not per delta.
fn persist_on_drag_end(
    root: Entity,
) -> impl Fn(On<Pointer<DragEnd>>, Query<&mut TableState>, DisabledQuery) {
    move |mut drag: On<Pointer<DragEnd>>,
          mut tables: Query<&mut TableState>,
          disabled: DisabledQuery| {
        drag.propagate(false);
        if drag.button != PointerButton::Primary {
            return;
        }
        // Nothing moved, so nothing is owed to the settings file — and marking the
        // table dirty anyway would write the widths on a gesture the widget just
        // refused.
        if gesture_refused(&disabled, root, drag.entity) {
            return;
        }
        if let Ok(mut state) = tables.get_mut(root) {
            state.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Reconciliation systems.
// ---------------------------------------------------------------------------

/// Reflect each table's current column widths onto its header and body cells, so
/// a fixed column and its header stay the same width through a resize drag and a
/// persisted restore. Change-guarded per cell, so a settled table costs a compare
/// and nothing more.
fn sync_table_column_widths(
    tables: Query<&TableState>,
    mut cells: Query<(&TableColumnCell, &mut Node)>,
) {
    for (cell, mut node) in &mut cells {
        let Ok(state) = tables.get(cell.table) else {
            continue;
        };
        let Some(column) = state.spec.columns.get(cell.column) else {
            continue;
        };
        // Only fixed columns carry a live width; flex columns own the slack.
        if !matches!(column.width, TableColumnWidth::Fixed { .. }) {
            continue;
        }
        let Some(width) = state.widths.get(cell.column).copied() else {
            continue;
        };
        let wanted = Val::Px(width);
        if node.width != wanted {
            node.width = wanted;
        }
    }
}

/// Set each sortable header's arrow from its table's primary sort key — the
/// ascending / descending glyph on the most-significant column, blank elsewhere.
fn drive_table_sort_arrows(
    tables: Query<&TableState>,
    mut arrows: Query<(&TableHeaderArrow, &mut Text)>,
) {
    for (arrow, mut text) in &mut arrows {
        let Ok(state) = tables.get(arrow.table) else {
            continue;
        };
        let glyph = match state.sort.primary() {
            Some(primary) if primary.column == arrow.column => {
                if primary.ascending {
                    SORT_ASCENDING_GLYPH
                } else {
                    SORT_DESCENDING_GLYPH
                }
            }
            _other => "",
        };
        if text.0 != glyph {
            glyph.clone_into(&mut text.0);
        }
    }
}

/// Grey each header label and sort arrow whose table — or whose own header cell
/// — is [disabled](bevy::ui::InteractionDisabled), and restore the spec's header
/// colour when it is enabled again: the visible half of the disabled state, so a
/// header that refuses a click does not look like one that would answer it.
///
/// Header text only. A body cell's colour is the consumer's, written on every
/// bind by [`set_table_cell`], so a widget system that greyed it would have no
/// colour to put back — and the consumer's next bind would undo the greying
/// anyway. Every write is guarded, so a settled table costs a compare.
fn reflect_table_disabled(
    tables: Query<(&TableState, Has<bevy::ui::InteractionDisabled>)>,
    disabled: DisabledQuery,
    mut texts: Query<(&TableHeaderText, &mut TextColor)>,
) {
    for (header, mut color) in &mut texts {
        let Ok((state, table_disabled)) = tables.get(header.table) else {
            continue;
        };
        let wanted = if table_disabled || disabled.contains(header.cell) {
            DISABLED_TEXT_COLOR
        } else {
            state.spec.header_color
        };
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// Seed each table's sort order and column widths from the persisted account
/// settings, once, after the per-avatar account scope has loaded (mirrors the
/// People friends list's seed stage). Bumps the sort revision so the consumer
/// re-sorts into the restored order.
fn seed_tables_from_settings(
    settings: Option<Res<ViewerSettings>>,
    mut tables: Query<&mut TableState>,
) {
    let Some(settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    for mut state in &mut tables {
        if state.seeded {
            continue;
        }
        state.seeded = true;
        let spec = state.spec;
        if let Some(name) = spec.sort_setting
            && let Ok(encoded) = settings.store().get_str(name)
        {
            state.sort = TableSort::parse(encoded, spec.columns, spec.default_sort);
            state.sort_revision = state.sort_revision.wrapping_add(1);
        }
        if let Some(name) = spec.widths_setting
            && let Ok(encoded) = settings.store().get_str(name)
        {
            apply_persisted_widths(&mut state, encoded);
        }
    }
}

/// Apply a persisted `token:px,token:px` width string onto a table's fixed
/// columns (unknown tokens ignored, each width clamped to the legal range).
fn apply_persisted_widths(state: &mut TableState, encoded: &str) {
    for part in encoded.split(',') {
        let mut fields = part.split(':');
        let (Some(token), Some(value)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Some(index) = state
            .spec
            .columns
            .iter()
            .position(|column| column.token == token)
        else {
            continue;
        };
        let Ok(width) = value.parse::<f32>() else {
            continue;
        };
        if let Some(slot) = state.widths.get_mut(index) {
            *slot = width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
        }
    }
}

/// Encode a table's fixed-column widths as `token:px,token:px` for persistence.
fn encode_widths(state: &TableState) -> String {
    state
        .spec
        .columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| {
            if !matches!(column.width, TableColumnWidth::Fixed { .. }) {
                return None;
            }
            state
                .widths
                .get(index)
                .map(|width| format!("{}:{width:.0}", column.token))
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Write each dirty table's sort order and column widths back to the account
/// settings and save, then clear the dirty flag — so a header click or a resize
/// gesture survives a restart. Runs only once the account scope is loaded.
fn persist_table_state(
    settings: Option<ResMut<ViewerSettings>>,
    mut tables: Query<&mut TableState>,
) {
    let Some(mut settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    let mut wrote = false;
    for mut state in &mut tables {
        if !state.dirty {
            continue;
        }
        state.dirty = false;
        let spec = state.spec;
        if let Some(name) = spec.sort_setting {
            let encoded = state.sort.encode(spec.columns);
            settings.set_account(name, SettingValue::String(encoded));
            wrote = true;
        }
        if let Some(name) = spec.widths_setting {
            let encoded = encode_widths(&state);
            settings.set_account(name, SettingValue::String(encoded));
            wrote = true;
        }
    }
    if wrote {
        settings.save_async();
    }
}

/// Register a table's persisted sort / width settings so the account file that
/// loads at login is coerced to the right types. The consumer calls this from its
/// settings-registration step (as the People list does).
pub fn register_table_settings(settings: &mut ViewerSettings, section: &[&str], spec: &TableSpec) {
    if let Some(name) = spec.sort_setting {
        let default = TableSort::from_defaults(spec.default_sort).encode(spec.columns);
        settings.register_hidden_in(
            section,
            name,
            SettingValue::String(default),
            "Table sort order, most-significant column first (token:dir, …).",
        );
    }
    if let Some(name) = spec.widths_setting {
        settings.register_hidden_in(
            section,
            name,
            SettingValue::String(String::new()),
            "Table column widths (token:px, …).",
        );
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin that drives every [`TableState`]: column-width sync, ellipsis
/// reveal, sort arrows, the disabled greying, and the settings seed / persist.
#[derive(Debug)]
pub struct TableWidgetPlugin;

impl Plugin for TableWidgetPlugin {
    /// Register the reconciliation systems. The width sync and ellipsis reveal
    /// run after the virtual-list layout (they read laid-out sizes); so does the
    /// selection highlight, which paints by the row's *data index* and would
    /// otherwise repaint a recycled row from the index it held last frame; the
    /// sort / seed / persist systems are independent.
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                seed_tables_from_settings,
                sync_table_column_widths,
                drive_table_sort_arrows,
                reflect_table_disabled,
                apply_table_selection_highlight
                    .after(sl_viewer_ui_core::virtual_list::layout_virtual_lists),
                persist_table_state,
            ),
        );
    }
}

/// The widget's built-in row-selection click: select the pressed row under the
/// table's [`TableSelectionMode`] and the held modifier keys, bumping the table's
/// selection revision. A no-op for a [`TableSelectionMode::None`] table (so a
/// consumer with its own row observer is unaffected) and for a parked row.
fn select_table_row_on_press(
    press: On<Pointer<Press>>,
    rows: Query<(&VirtualRow, &TableRow)>,
    keyboard: Res<ButtonInput<KeyCode>>,
    disabled: DisabledQuery,
    mut tables: Query<&mut TableState>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok((row, row_ref)) = rows.get(press.entity) else {
        return;
    };
    let Some(index) = row.index else {
        return;
    };
    // A disabled table (or a single disabled row) does not move its selection.
    if gesture_refused(&disabled, row_ref.table, press.entity) {
        return;
    }
    let Ok(mut state) = tables.get_mut(row_ref.table) else {
        return;
    };
    if state.selection_mode() == TableSelectionMode::None {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    state.apply_click(index, ctrl, shift);
}

/// Paint each selectable table's rows: the selection accent for a row whose data
/// index is selected, transparent otherwise. Skips [`TableSelectionMode::None`]
/// tables entirely, so a consumer that owns its own row backgrounds keeps them.
fn apply_table_selection_highlight(
    tables: Query<&TableState>,
    mut rows: Query<(&VirtualRow, &TableRow, &mut BackgroundColor)>,
) {
    for (row, row_ref, mut background) in &mut rows {
        let Ok(state) = tables.get(row_ref.table) else {
            continue;
        };
        if state.selection_mode() == TableSelectionMode::None {
            continue;
        }
        let selected = row.index.is_some_and(|index| state.is_selected(index));
        let wanted = if selected {
            SELECTED_ROW_BACKGROUND
        } else {
            Color::NONE
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DISABLED_TEXT_COLOR, MAX_COLUMN_WIDTH, MIN_COLUMN_WIDTH, TableAlign, TableColumn,
        TableColumnKind, TableColumnWidth, TableHandle, TableHeaderText, TableSelectionMode,
        TableSort, TableSortDefault, TableSpec, TableState, TableWidgetPlugin, resize_column_width,
        spawn_table, spawn_table_row,
    };
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_viewer_ui_core::ui::UiRoot;
    use sl_viewer_ui_core::virtual_list::VirtualRow;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A three-column fixture: a flexible Name, a fixed Title, a fixed Land.
    const COLUMNS: &[TableColumn] = &[
        TableColumn {
            header_key: "name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "title",
            token: "title",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 110.0 },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "land",
            token: "land",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 64.0 },
            align: TableAlign::End,
            sortable: true,
        },
    ];

    /// The default sort — by name, ascending.
    const DEFAULTS: &[TableSortDefault] = &[TableSortDefault {
        column: 0,
        ascending: true,
    }];

    /// A fresh click promotes a column to the front, ascending.
    #[test]
    fn click_promotes_to_front_ascending() {
        let mut sort = TableSort::from_defaults(DEFAULTS);
        sort.click(2);
        assert_eq!(sort.primary().map(|key| key.column), Some(2));
        assert_eq!(sort.primary().map(|key| key.ascending), Some(true));
        // The old primary is demoted to a tie-breaker, not dropped.
        assert_eq!(sort.keys().len(), 2);
        assert_eq!(sort.keys().get(1).map(|key| key.column), Some(0));
    }

    /// Re-clicking the primary column flips its direction in place.
    #[test]
    fn reclick_flips_direction() {
        let mut sort = TableSort::from_defaults(DEFAULTS);
        assert!(sort.primary().is_some_and(|key| key.ascending));
        sort.click(0);
        assert!(sort.primary().is_some_and(|key| !key.ascending));
        assert_eq!(sort.keys().len(), 1);
    }

    /// The sort survives an encode → parse round-trip, named by stable tokens.
    #[test]
    fn encode_parse_round_trip() {
        let mut sort = TableSort::from_defaults(DEFAULTS);
        sort.click(1); // title asc, then name asc
        let encoded = sort.encode(COLUMNS);
        assert_eq!(encoded, "title:a,name:a");
        let parsed = TableSort::parse(&encoded, COLUMNS, DEFAULTS);
        assert_eq!(parsed.keys(), sort.keys());
    }

    /// A wholly unrecognised persisted string falls back to the defaults, and
    /// unknown / duplicate tokens are dropped.
    #[test]
    fn parse_falls_back_and_drops_unknown() {
        let fallback = TableSort::parse("garbage", COLUMNS, DEFAULTS);
        assert_eq!(fallback.keys(), TableSort::from_defaults(DEFAULTS).keys());
        // A known token beside an unknown one keeps only the known.
        let mixed = TableSort::parse("land:d,bogus:a,land:a", COLUMNS, DEFAULTS);
        assert_eq!(mixed.keys().len(), 1);
        assert_eq!(mixed.keys().first().map(|key| key.column), Some(2));
        assert!(mixed.keys().first().is_some_and(|key| !key.ascending));
    }

    /// A resize drag widens with a rightward delta and clamps to the range.
    #[expect(
        clippy::float_cmp,
        reason = "resize_column_width returns exact clamped sums, asserted exactly"
    )]
    #[test]
    fn resize_clamps_to_range() {
        assert_eq!(resize_column_width(100.0, 10.0, 1.0), 110.0);
        // Dragging far past the minimum clamps, never goes below.
        assert_eq!(resize_column_width(30.0, -100.0, 1.0), MIN_COLUMN_WIDTH);
        // Dragging far past the maximum clamps at the top.
        assert_eq!(resize_column_width(790.0, 100.0, 1.0), MAX_COLUMN_WIDTH);
    }

    /// A table spec fixture with the given selection mode (the other fields are
    /// irrelevant to the selection algebra).
    const fn spec_with(selection: TableSelectionMode) -> TableSpec {
        TableSpec {
            element: "test",
            selection,
            columns: COLUMNS,
            default_sort: DEFAULTS,
            builtin_sort: false,
            row_height: 20.0,
            font_size: 12.0,
            header_color: Color::WHITE,
            cell_color: Color::WHITE,
            column_gap: 4.0,
            row_padding: 4.0,
            sort_setting: None,
            widths_setting: None,
        }
    }

    static SINGLE_SPEC: TableSpec = spec_with(TableSelectionMode::Single);
    static MULTI_SPEC: TableSpec = spec_with(TableSelectionMode::Multi);
    static NONE_SPEC: TableSpec = spec_with(TableSelectionMode::None);

    /// A fresh [`TableState`] over `spec`, with nothing selected.
    fn state(spec: &'static TableSpec) -> TableState {
        TableState {
            spec,
            widths: Vec::new(),
            sort: TableSort::from_defaults(spec.default_sort),
            sort_revision: 0,
            dirty: false,
            seeded: false,
            selected: Vec::new(),
            anchor: None,
            selection_revision: 0,
        }
    }

    /// Single-select replaces the selection with the clicked row.
    #[test]
    fn single_select_replaces() {
        let mut table = state(&SINGLE_SPEC);
        table.apply_click(2, false, false);
        assert_eq!(table.selected(), &[2]);
        assert_eq!(table.primary_selected(), Some(2));
        table.apply_click(5, false, false);
        assert_eq!(table.selected(), &[5]);
        // Two real changes → two revisions.
        assert_eq!(table.selection_revision(), 2);
    }

    /// Multi-select: a plain click replaces, Ctrl toggles, and a Ctrl-toggle off
    /// removes a row.
    #[test]
    fn multi_ctrl_toggles() {
        let mut table = state(&MULTI_SPEC);
        table.apply_click(1, false, false);
        assert_eq!(table.selected(), &[1]);
        table.apply_click(3, true, false);
        assert_eq!(table.selected(), &[1, 3]);
        table.apply_click(1, true, false);
        assert_eq!(table.selected(), &[3]);
    }

    /// Multi-select: a Shift click selects the inclusive range from the anchor.
    #[test]
    fn multi_shift_range() {
        let mut table = state(&MULTI_SPEC);
        table.apply_click(2, false, false);
        table.apply_click(5, false, true);
        assert_eq!(table.selected(), &[2, 3, 4, 5]);
    }

    /// Re-projecting a selection across a re-sort: the consumer hands back the
    /// indices its identities now sit at (in whatever order it found them), the
    /// widget normalises them, and only a *different* selection is an event —
    /// so a rebuild that did not move the selection is silent.
    #[test]
    fn set_selection_reprojects_and_only_bumps_on_change() {
        let mut table = state(&MULTI_SPEC);
        table.apply_click(1, false, false);
        table.apply_click(3, true, false);
        assert_eq!(table.selected(), &[1, 3]);
        assert_eq!(table.anchor(), Some(3));
        let revision = table.selection_revision();
        // The same rows, re-sorted: the consumer found them at 4 and 0, anchor 0.
        table.set_selection(vec![4, 0, 4], Some(0));
        assert_eq!(table.selected(), &[0, 4]);
        assert_eq!(table.anchor(), Some(0));
        assert_eq!(table.selection_revision(), revision + 1);
        // A rebuild that leaves them where they are says nothing.
        table.set_selection(vec![0, 4], Some(0));
        assert_eq!(table.selection_revision(), revision + 1);
        // A Shift+click now ranges from the re-projected anchor, not the old index.
        table.apply_click(2, false, true);
        assert_eq!(table.selected(), &[0, 1, 2]);
    }

    /// A `None` table never records a selection, and clearing an empty selection
    /// does not churn the revision.
    #[test]
    fn none_mode_and_clear_are_inert() {
        let mut table = state(&NONE_SPEC);
        table.apply_click(2, false, false);
        assert!(table.selected().is_empty());
        assert_eq!(table.selection_revision(), 0);
        table.clear_selection();
        assert_eq!(table.selection_revision(), 0);
    }

    // -----------------------------------------------------------------------
    // The disabled state: every gesture the widget owns, refused.
    // -----------------------------------------------------------------------

    /// The same fixture with the widget's own sort and multi-selection on, so
    /// the header-click and row-click observers are actually installed.
    static SORTABLE_SPEC: TableSpec = TableSpec {
        builtin_sort: true,
        ..spec_with(TableSelectionMode::Multi)
    };

    /// A headless app with the widget's plugin and the keyboard resource the
    /// row-selection observer reads. No settings resource: the seed and persist
    /// systems take it as an `Option` and no-op without it, which is what a
    /// disabled-state test wants — nothing writing widths behind the assertions.
    fn table_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(TableWidgetPlugin)
            .init_resource::<ButtonInput<KeyCode>>();
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        app.update();
        app
    }

    /// Spawn a table under the harness root and settle a frame.
    fn table(app: &mut App) -> TableHandle {
        let root = app.world().resource::<UiRoot>().0;
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let handle = {
            let mut commands = Commands::new(&mut queue, app.world());
            spawn_table(&mut commands, root, &SORTABLE_SPEC)
        };
        queue.apply(app.world_mut());
        app.update();
        handle
    }

    /// Pool one virtual row bound to data index `index` under a table's viewport,
    /// as a consumer's row pool would.
    fn row(app: &mut App, handle: &TableHandle, slot: usize, index: usize) -> Entity {
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let entity = {
            let mut commands = Commands::new(&mut queue, app.world());
            let entity = commands
                .spawn((
                    VirtualRow {
                        slot,
                        index: Some(index),
                    },
                    ChildOf(handle.viewport),
                ))
                .id();
            spawn_table_row(&mut commands, entity, handle.root, &SORTABLE_SPEC);
            entity
        };
        queue.apply(app.world_mut());
        app.update();
        entity
    }

    /// `spawn_table_row` amends the row node the virtual list owns rather than
    /// replacing it. `top` and `display` are the list's — where the pooled row
    /// sits, and whether it is parked — and a fresh `Node` would reset both, so a
    /// parked row would flash at `top: Auto`, visible, until the next layout pass
    /// took it back.
    #[test]
    fn spawning_a_row_keeps_the_placement_the_virtual_list_owns() -> Result<(), TestError> {
        let mut app = table_app();
        let handle = table(&mut app);
        // A parked pooled row, exactly as `layout_virtual_lists` leaves one.
        let entity = app
            .world_mut()
            .spawn((
                VirtualRow {
                    slot: 0,
                    index: None,
                },
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(140.0),
                    display: Display::None,
                    ..default()
                },
                ChildOf(handle.viewport),
            ))
            .id();
        app.update();

        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, app.world());
            spawn_table_row(&mut commands, entity, handle.root, &SORTABLE_SPEC);
        }
        queue.apply(app.world_mut());
        app.update();

        let node = app
            .world()
            .get::<Node>(entity)
            .ok_or("the row lost its node")?;
        assert_eq!(node.top, Val::Px(140.0), "the list's placement survives");
        assert_eq!(node.display, Display::None, "a parked row stays parked");
        // …while the table's own fields are applied.
        assert_eq!(
            node.padding,
            UiRect::horizontal(Val::Px(SORTABLE_SPEC.row_padding))
        );
        assert_eq!(node.column_gap, Val::Px(SORTABLE_SPEC.column_gap));
        assert_eq!(node.height, Val::Px(SORTABLE_SPEC.row_height));
        Ok(())
    }

    /// Trigger a primary press on `entity` and settle a frame — the observer call
    /// a real click makes, without a picking backend to aim one.
    fn press(app: &mut App, entity: Entity) {
        let event = Pointer::new(
            PointerId::Mouse,
            location(),
            Press {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                count: 1,
            },
            entity,
        );
        app.world_mut().trigger(event);
        app.update();
    }

    /// Trigger a primary drag of `dx` logical pixels on `entity`, then its end.
    fn drag(app: &mut App, entity: Entity, dx: f32) {
        let event = Pointer::new(
            PointerId::Mouse,
            location(),
            Drag {
                button: PointerButton::Primary,
                distance: Vec2::new(dx, 0.0),
                delta: Vec2::new(dx, 0.0),
            },
            entity,
        );
        app.world_mut().trigger(event);
        app.update();
    }

    /// A pointer location on no render target — the events are triggered by hand,
    /// so nothing reads it.
    fn location() -> Location {
        Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::ZERO,
        }
    }

    /// A table's primary sort column.
    fn primary_column(app: &App, table: Entity) -> Option<usize> {
        app.world()
            .get::<TableState>(table)
            .and_then(|state| state.sort().primary().map(|key| key.column))
    }

    /// A table's live width for column `index`.
    fn width(app: &App, table: Entity, index: usize) -> Option<f32> {
        app.world()
            .get::<TableState>(table)
            .and_then(|state| state.widths.get(index).copied())
    }

    /// A table's selected data indices.
    fn selected(app: &App, table: Entity) -> Vec<usize> {
        app.world()
            .get::<TableState>(table)
            .map(|state| state.selected().to_vec())
            .unwrap_or_default()
    }

    /// The colour of the first header text under `cell`, walking the cell's
    /// subtree (the label sits inside a clip container, the arrow beside it).
    fn header_color(app: &mut App, cell: Entity) -> Option<Color> {
        let mut texts = app.world_mut().query::<(&TableHeaderText, &TextColor)>();
        texts
            .iter(app.world())
            .find(|(header, _)| header.cell == cell)
            .map(|(_, color)| color.0)
    }

    /// The named descendant of the table (its column-resize handle).
    fn find_by_name(app: &mut App, wanted: &str) -> Option<Entity> {
        let mut names = app.world_mut().query::<(Entity, &Name)>();
        names
            .iter(app.world())
            .find(|(_, name)| name.as_str() == wanted)
            .map(|(entity, _)| entity)
    }

    /// The baseline the disabled tests are measured against: with nothing
    /// disabled, a header press sorts, a border drag resizes, and a row press
    /// selects. Without this the tests below could pass on a table that never
    /// reacts to anything at all.
    #[test]
    fn the_gestures_work_when_nothing_is_disabled() -> Result<(), TestError> {
        let mut app = table_app();
        let handle = table(&mut app);
        let cell = *handle.header_cells.get(2).ok_or("no header cell 2")?;
        let resizer = find_by_name(&mut app, "test:table-border-resizer:1")
            .ok_or("no resize handle on border 1")?;
        let pooled = row(&mut app, &handle, 0, 4);

        assert_eq!(
            primary_column(&app, handle.root),
            Some(0),
            "the default sort"
        );
        press(&mut app, cell);
        assert_eq!(
            primary_column(&app, handle.root),
            Some(2),
            "the click sorts"
        );

        let before = width(&app, handle.root, 1).ok_or("no width for column 1")?;
        drag(&mut app, resizer, 10.0);
        let after = width(&app, handle.root, 1).ok_or("no width for column 1")?;
        assert!(
            after > before,
            "the drag widened column 1: {before} -> {after}"
        );

        press(&mut app, pooled);
        assert_eq!(selected(&app, handle.root), vec![4], "the click selects");
        Ok(())
    }

    /// A table wearing [`InteractionDisabled`](bevy::ui::InteractionDisabled)
    /// answers none of the three gestures, and greys its header.
    #[test]
    fn a_disabled_table_refuses_every_gesture() -> Result<(), TestError> {
        let mut app = table_app();
        let handle = table(&mut app);
        let cell = *handle.header_cells.get(2).ok_or("no header cell 2")?;
        let resizer = find_by_name(&mut app, "test:table-border-resizer:1")
            .ok_or("no resize handle on border 1")?;
        let pooled = row(&mut app, &handle, 0, 4);
        app.world_mut()
            .entity_mut(handle.root)
            .insert(bevy::ui::InteractionDisabled);
        app.update();

        let before = width(&app, handle.root, 1).ok_or("no width for column 1")?;
        press(&mut app, cell);
        drag(&mut app, resizer, 10.0);
        press(&mut app, pooled);

        assert_eq!(
            primary_column(&app, handle.root),
            Some(0),
            "the sort did not move"
        );
        assert_eq!(
            width(&app, handle.root, 1),
            Some(before),
            "the column width did not move"
        );
        assert!(
            selected(&app, handle.root).is_empty(),
            "nothing was selected"
        );
        assert_eq!(
            header_color(&mut app, cell),
            Some(DISABLED_TEXT_COLOR),
            "the header greyed"
        );
        Ok(())
    }

    /// The marker on **one header cell** disables that column alone: its own
    /// click is refused and its label greys, while the next column still sorts.
    #[test]
    fn one_disabled_header_leaves_the_others_live() -> Result<(), TestError> {
        let mut app = table_app();
        let handle = table(&mut app);
        let frozen = *handle.header_cells.get(2).ok_or("no header cell 2")?;
        let live = *handle.header_cells.get(1).ok_or("no header cell 1")?;
        app.world_mut()
            .entity_mut(frozen)
            .insert(bevy::ui::InteractionDisabled);
        app.update();

        press(&mut app, frozen);
        assert_eq!(
            primary_column(&app, handle.root),
            Some(0),
            "the disabled column did not sort"
        );
        press(&mut app, live);
        assert_eq!(
            primary_column(&app, handle.root),
            Some(1),
            "its neighbour still sorts"
        );
        assert_eq!(
            header_color(&mut app, frozen),
            Some(DISABLED_TEXT_COLOR),
            "the disabled header greyed"
        );
        assert_eq!(
            header_color(&mut app, live),
            Some(SORTABLE_SPEC.header_color),
            "the live header kept the spec's colour"
        );
        Ok(())
    }

    /// The marker on **one row** refuses that row's selection click while its
    /// neighbours still select — and re-enabling restores the header's colour,
    /// so the greying is a reflection of the state, not a one-way write.
    #[test]
    fn one_disabled_row_and_a_restored_header() -> Result<(), TestError> {
        let mut app = table_app();
        let handle = table(&mut app);
        let frozen = row(&mut app, &handle, 0, 1);
        let live = row(&mut app, &handle, 1, 2);
        app.world_mut()
            .entity_mut(frozen)
            .insert(bevy::ui::InteractionDisabled);
        app.update();

        press(&mut app, frozen);
        assert!(
            selected(&app, handle.root).is_empty(),
            "the disabled row did not select"
        );
        press(&mut app, live);
        assert_eq!(
            selected(&app, handle.root),
            vec![2],
            "its neighbour still selects"
        );

        let cell = *handle.header_cells.first().ok_or("no header cell 0")?;
        app.world_mut()
            .entity_mut(handle.root)
            .insert(bevy::ui::InteractionDisabled);
        app.update();
        assert_eq!(header_color(&mut app, cell), Some(DISABLED_TEXT_COLOR));
        app.world_mut()
            .entity_mut(handle.root)
            .remove::<bevy::ui::InteractionDisabled>();
        app.update();
        assert_eq!(
            header_color(&mut app, cell),
            Some(SORTABLE_SPEC.header_color),
            "re-enabling puts the spec's colour back"
        );
        Ok(())
    }

    /// **The table, driven** (`viewer-ui-widget-interaction-suite`): header
    /// clicks that sort, a border drag that resizes without sorting, and the
    /// three selection gestures — all through a laid-out table under the real
    /// pointer.
    ///
    /// The tests above trigger `Pointer<Press>` / `Pointer<Drag>` straight at
    /// an entity, so they prove the *observers* are right and say nothing about
    /// whether the pointer can reach them. Two of this widget's controls
    /// overlap by design: the border resizer sits **on top of** the trailing
    /// edge of a header cell that sorts when clicked. Which of the two a
    /// gesture reaches is decided by hit-testing and by
    /// `should_block_lower` — neither of which a triggered event exercises at
    /// all.
    mod scenarios {
        use bevy::input::keyboard::Key;
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;

        use super::{SORTABLE_SPEC, TableState, TestError, spawn_table, spawn_table_row};
        use crate::ui_table::{TableRow, TableWidgetPlugin};
        use crate::ui_test::interact::{self, InteractionTest, centre_of};
        use crate::ui_test::{find_by_name, settle};
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};
        use sl_viewer_ui_core::virtual_list::{
            VirtualList, VirtualListPlugin, VirtualRow, index_to_f32,
        };

        /// How many rows the fixture's consumer has. Small enough that the
        /// pool covers all of them at once, so a row's data index and its
        /// pooled entity stay paired for the length of a test.
        const ITEM_COUNT: usize = 8;

        /// The fixture host's size, in logical pixels — a definite box, because
        /// the table's viewport grows into its parent and would otherwise lay
        /// out at zero height with no row to click.
        const HOST_SIZE: Vec2 = Vec2::new(520.0, 240.0);

        /// A laid-out table under the real pointer stack, with [`ITEM_COUNT`]
        /// rows.
        fn table_app() -> App {
            table_app_with(ITEM_COUNT)
        }

        /// A laid-out table under the real pointer stack: the widget's plugin,
        /// the virtual-list pool that positions its rows, and a consumer that
        /// binds each pooled row and names it for its data index.
        fn table_app_with(items: usize) -> App {
            let mut app = InteractionTest::new().build();
            app.add_plugins((TableWidgetPlugin, VirtualListPlugin));
            app.add_systems(
                Startup,
                (move |mut commands: Commands, root: Res<UiRoot>| {
                    let host = commands
                        .spawn((
                            Node {
                                width: Val::Px(HOST_SIZE.x),
                                height: Val::Px(HOST_SIZE.y),
                                flex_direction: FlexDirection::Column,
                                ..Node::default()
                            },
                            Name::new("table-host"),
                            ChildOf(root.0),
                        ))
                        .id();
                    let handle = spawn_table(&mut commands, host, &SORTABLE_SPEC);
                    // The consumer's half of the virtual list: how many items
                    // it is presenting. The pool follows from there.
                    commands
                        .entity(handle.viewport)
                        .entry::<VirtualList>()
                        .and_modify(move |mut list: Mut<VirtualList>| {
                            list.item_count = items;
                        });
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            app.add_systems(Update, bind_rows);
            settle(&mut app);
            settle(&mut app);
            app
        }

        /// The consumer half: give each pooled row the widget's row parts (once)
        /// and a `Name` naming the data index it is currently showing, so a test
        /// can aim at "row 3" the way a user aims at the fourth line.
        fn bind_rows(
            mut commands: Commands,
            table: Query<Entity, With<TableState>>,
            rows: Query<(Entity, &VirtualRow, Has<TableRow>), Changed<VirtualRow>>,
        ) {
            let Ok(root) = table.single() else {
                return;
            };
            for (entity, row, bound) in &rows {
                if !bound {
                    spawn_table_row(&mut commands, entity, root, &SORTABLE_SPEC);
                }
                let name = row
                    .index
                    .map_or_else(|| "row:parked".to_owned(), |index| format!("row:{index}"));
                commands.entity(entity).insert(Name::new(name));
            }
        }

        /// The table's state.
        fn state(app: &mut App) -> Option<&TableState> {
            let root = find_by_name(app, "test:table")?;
            app.world().get::<TableState>(root)
        }

        /// The primary sort level: the column ordered by, and its direction.
        fn primary_sort(app: &mut App) -> Option<(usize, bool)> {
            let key = state(app)?.sort().primary()?;
            Some((key.column, key.ascending))
        }

        /// The glyph the named column's sort arrow is showing.
        fn arrow(app: &mut App, column: usize) -> Option<String> {
            let entity = find_by_name(app, &format!("test:table-header-arrow:{column}"))?;
            app.world().get::<Text>(entity).map(|text| text.0.clone())
        }

        /// A header click sorts by that column, and clicking it again flips the
        /// direction — with the arrow following.
        #[test]
        fn a_header_click_sorts_and_the_arrow_follows() -> Result<(), TestError> {
            let mut app = table_app();
            assert_eq!(
                primary_sort(&mut app),
                Some((0, true)),
                "the fixture rests on its declared default sort"
            );

            interact::click_node(&mut app, "test:table-header-cell:1")?;
            settle(&mut app);
            assert_eq!(
                primary_sort(&mut app),
                Some((1, true)),
                "a fresh column is promoted ascending"
            );
            assert_eq!(
                arrow(&mut app, 1),
                Some(super::super::SORT_ASCENDING_GLYPH.to_owned())
            );
            assert_eq!(
                arrow(&mut app, 0),
                Some(String::new()),
                "the demoted column drops its arrow"
            );

            interact::click_node(&mut app, "test:table-header-cell:1")?;
            settle(&mut app);
            assert_eq!(
                primary_sort(&mut app),
                Some((1, false)),
                "re-clicking the front column flips it"
            );
            assert_eq!(
                arrow(&mut app, 1),
                Some(super::super::SORT_DESCENDING_GLYPH.to_owned())
            );
            Ok(())
        }

        /// Dragging the border between two columns resizes them — and does
        /// **not** sort, even though the handle sits inside the header cell
        /// whose every other pixel does.
        ///
        /// This is the one test in the widget that can tell those two apart:
        /// the handle blocks lower and swallows its press, and both halves of
        /// that are hit-test behaviour.
        #[test]
        fn a_border_drag_resizes_without_sorting() -> Result<(), TestError> {
            const TRAVEL: f32 = 30.0;

            let mut app = table_app();
            let before = state(&mut app).map(|state| state.widths.clone());
            let sort_before = primary_sort(&mut app);
            let revision = state(&mut app).map(TableState::sort_revision);

            let grip = centre_of(&mut app, "test:table-border-resizer:1")
                .ok_or("the column border never laid out")?;
            interact::drag(
                &mut app,
                grip,
                Vec2::new(grip.x + TRAVEL, grip.y),
                3,
                MouseButton::Left,
            );
            settle(&mut app);

            let widths = state(&mut app)
                .map(|state| state.widths.clone())
                .ok_or("the table lost its state")?;
            let before = before.ok_or("the table lost its state")?;
            let grown = widths.get(1).copied().ok_or("no column 1")?;
            let was = before.get(1).copied().ok_or("no column 1")?;
            assert!(
                (grown - was - TRAVEL).abs() < 1.0,
                "dragging the border {TRAVEL} px right widens column 1 by {TRAVEL}: {was} -> {grown}"
            );
            let shrunk = widths.get(2).copied().ok_or("no column 2")?;
            let was_right = before.get(2).copied().ok_or("no column 2")?;
            assert!(
                (was_right - shrunk - TRAVEL).abs() < 1.0,
                "and takes it from column 2: {was_right} -> {shrunk}"
            );
            assert_eq!(
                primary_sort(&mut app),
                sort_before,
                "grabbing the resize handle must not sort the table"
            );
            assert_eq!(
                state(&mut app).map(TableState::sort_revision),
                revision,
                "and must not tell the consumer to re-sort"
            );
            Ok(())
        }

        /// The three row-selection gestures, on rows the pointer actually
        /// finds: a plain click selects one, `Ctrl`+click adds a second,
        /// `Shift`+click takes the range from the anchor.
        #[test]
        fn clicks_select_rows_and_the_modifiers_extend() -> Result<(), TestError> {
            let mut app = table_app();
            let selected = |app: &mut App| -> Vec<usize> {
                state(app)
                    .map(|state| state.selected().to_vec())
                    .unwrap_or_default()
            };
            assert!(
                selected(&mut app).is_empty(),
                "a fresh table selects nothing"
            );

            interact::click_node(&mut app, "row:2")?;
            settle(&mut app);
            assert_eq!(selected(&mut app), vec![2], "a plain click selects one row");

            interact::with_modifier(&mut app, KeyCode::ControlLeft, Key::Control, |app| {
                let _clicked = interact::click_node(app, "row:5");
            });
            settle(&mut app);
            assert_eq!(
                selected(&mut app),
                vec![2, 5],
                "Ctrl+click adds a row without dropping the first"
            );

            interact::with_modifier(&mut app, KeyCode::ShiftLeft, Key::Shift, |app| {
                let _clicked = interact::click_node(app, "row:7");
            });
            settle(&mut app);
            assert_eq!(
                selected(&mut app),
                vec![5, 6, 7],
                "Shift+click takes the range from the last row Ctrl left as the anchor"
            );
            Ok(())
        }

        /// How many rows the scrolling fixture presents — far more than fit, so
        /// the window has somewhere to move to.
        const LONG_LIST: usize = 400;

        /// The list's scroll offset.
        fn scroll_offset(app: &mut App) -> Option<f32> {
            let viewport = find_by_name(app, "test:table-viewport")?;
            app.world()
                .get::<VirtualList>(viewport)
                .map(VirtualList::scroll_offset)
        }

        /// Every live pooled row's data index, ascending — the rows a user can
        /// see something in.
        fn live_indices(app: &mut App) -> Vec<usize> {
            let mut query = app.world_mut().query::<(&VirtualRow, &Node)>();
            let mut rows: Vec<usize> = query
                .iter(app.world())
                .filter(|(_row, node)| node.display != Display::None)
                .filter_map(|(row, _node)| row.index)
                .collect();
            rows.sort_unstable();
            rows
        }

        /// **The wheel, and where it belongs**: a notch over the rows scrolls
        /// the list, the window moves, no live row is left blank or misplaced —
        /// and the same notch over the header, which is *not* inside the
        /// viewport, scrolls nothing.
        ///
        /// This is the virtual list's own pointer test, driven through its main
        /// consumer. It lives here rather than beside `virtual_list.rs` because
        /// this harness is built on that crate: a dev-dependency back onto
        /// `sl-viewer-ui-core` links two copies of it into one test binary — the
        /// `cfg(test)` one and the harness's — and a fixture's `Res<UiRoot>`
        /// then names a type nothing ever inserted. A table is the right
        /// stand-in anyway: it has a header the wheel must ignore, which a bare
        /// list fixture would not.
        #[test]
        fn the_wheel_scrolls_the_rows_and_not_the_header() -> Result<(), TestError> {
            let mut app = table_app_with(LONG_LIST);
            assert_eq!(scroll_offset(&mut app), Some(0.0), "it starts at the top");
            let before = live_indices(&mut app);
            assert!(!before.is_empty(), "the pool covers the viewport");

            let rows =
                centre_of(&mut app, "test:table-viewport").ok_or("the viewport never laid out")?;
            // Negative y is a notch towards the user: the content moves up.
            for _notch in 0..3 {
                interact::scroll(&mut app, rows, Vec2::new(0.0, -3.0));
            }
            settle(&mut app);

            let scrolled = scroll_offset(&mut app).ok_or("the list lost its scroll")?;
            assert!(scrolled > 0.0, "the wheel over the rows scrolls them");
            let after = live_indices(&mut app);
            assert!(
                after.first() > before.first(),
                "the window moved down the list: {:?} -> {:?}",
                before.first(),
                after.first()
            );
            // The window stays a window: bounded whatever the list's length —
            // at the top it is short (the overscan above is clamped away), in
            // the middle it is full, and never anywhere near the item count.
            assert!(
                after.len() <= 32,
                "a {LONG_LIST}-row list must not put {} rows on screen",
                after.len()
            );
            // And contiguous: a virtualised window is a run of consecutive
            // items, so a gap in it is a hole in the list.
            for pair in after.windows(2) {
                let (low, high) = (pair.first().copied(), pair.get(1).copied());
                let (low, high) = (
                    low.ok_or("a pair without a first")?,
                    high.ok_or("a pair without a second")?,
                );
                assert_eq!(
                    high,
                    low.saturating_add(1),
                    "the live rows must be consecutive: {after:?}"
                );
            }
            // No dead rows: every live row sits where its item belongs — one
            // row-height per item below the top, less the scroll. A blank or
            // duplicated line is exactly what a row that failed this looks
            // like on screen.
            let mut query = app.world_mut().query::<(&VirtualRow, &Node)>();
            let mut placed = Vec::new();
            for (row, node) in query.iter(app.world()) {
                if node.display == Display::None {
                    continue;
                }
                let index = row.index.ok_or("a live row shows no item")?;
                placed.push((index, node.top));
            }
            for (index, top) in placed {
                let wanted = index_to_f32(index) * SORTABLE_SPEC.row_height - scrolled;
                assert_eq!(
                    top,
                    Val::Px(wanted),
                    "row {index} is drawn where row {index} belongs"
                );
            }

            let header = centre_of(&mut app, "test:table-header").ok_or("no header")?;
            interact::scroll(&mut app, header, Vec2::new(0.0, -3.0));
            settle(&mut app);
            assert_eq!(
                scroll_offset(&mut app),
                Some(scrolled),
                "a notch over the header is not the list's"
            );
            Ok(())
        }

        /// Dragging the overlay scrollbar's thumb scrolls by the **track's**
        /// ratio: a short thumb over a long list means one pixel of thumb is
        /// many rows of content, so the list moves much further than the
        /// pointer did.
        #[test]
        fn dragging_the_scrollbar_thumb_scrolls_further_than_the_pointer() -> Result<(), TestError>
        {
            const TRAVEL: f32 = 40.0;

            let mut app = table_app_with(LONG_LIST);
            let thumb = centre_of(&mut app, "virtual-list:scrollbar-thumb")
                .ok_or("the scrollbar never laid out")?;
            interact::drag(
                &mut app,
                thumb,
                Vec2::new(thumb.x, thumb.y + TRAVEL),
                4,
                MouseButton::Left,
            );
            settle(&mut app);

            let scrolled = scroll_offset(&mut app).ok_or("the list lost its scroll")?;
            assert!(
                scrolled > TRAVEL,
                "{TRAVEL} px of thumb is many rows of a {LONG_LIST}-row list, not {scrolled} px"
            );
            let visible = live_indices(&mut app);
            let first = visible.first().copied().ok_or("the pool emptied")?;
            assert!(
                first > 0,
                "and the window followed the thumb rather than staying at the top"
            );
            Ok(())
        }
    }
}
