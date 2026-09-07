//! The **Locks** floater (`rlv_locks`): what is held on, what may not go on,
//! and which object decided.
//!
//! A yes/no restriction cannot answer the question a wear path actually asks.
//! `@detach=n` does not mean "detaching is blocked"; it means *this* object may
//! not come off. `@remattach:chest=n` locks a point, `@addoutfit:gloves=n` a
//! layer, and `@detachallthis=n` a folder and everything under it. The lock
//! model ([`sl_rlv::RlvLocks`]) derives all four registries from the held
//! commands, and this window is the inspector over them — the reference's
//! `rlv_locks`, reached from RLVa ▸ Debug ▸ Locks…
//!
//! One list, four sections, in the reference's order: attachment locks, then
//! attachment-point locks, then wearable-layer locks, then folder locks. Each
//! row says which kind of lock it is, which way it points (`add` — nothing new
//! may go here — or `rem` — what is here may not come off), what it names, and
//! the object holding it.
//!
//! # What is not listed yet
//!
//! The reference also lists the **`nostrip`** soft locks (items whose folder
//! name exempts them from `@detach` / `@remoutfit`) and resolves each folder
//! lock to the worn items it currently catches. Both need the shared `#RLV`
//! folder tree, which no part of this viewer builds yet
//! (`viewer-inventory-folder-tree`). The four registries above are complete
//! without it: they are what the *commands* said, and the folder-lock rows here
//! name their source exactly as the command spelled it.
//!
//! Reference (Firestorm, read-only): `rlvfloaters.cpp` (`RlvFloaterLocks`),
//! `floater_rlv_locks.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use sl_rlv::{
    RlvFolderLock, RlvFolderLockPermission, RlvFolderLockScope, RlvFolderLockSource, RlvLockKind,
    RlvLocks, RlvState,
};
use sl_viewer_ui_core::i18n::{TransArgs, Translated, Translator};
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::virtual_list::{
    VirtualList, VirtualRow, layout_virtual_lists, spawn_virtual_scrollbar,
};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSpec, set_table_cell, spawn_table, spawn_table_row,
};
use sl_viewer_world_api::rlv::RlvSession;

use crate::rlv_behaviours::issuer_label;
use crate::style::{DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR, LIST_BACKGROUND, ROW_HEIGHT};

/// The floater's stable id.
pub const LOCKS_FLOATER_ID: &str = "rlv-locks";

/// Column index of the lock's kind.
const COL_TYPE: usize = 0;
/// Column index of the direction it points.
const COL_DIRECTION: usize = 1;
/// Column index of what it names.
const COL_TARGET: usize = 2;
/// Column index of the object holding it.
const COL_ORIGIN: usize = 3;

/// The lock list: kind, direction, target, origin — the reference's four
/// columns.
static LOCK_TABLE: TableSpec = TableSpec {
    element: "rlv-locks",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "rlv-locks-col-type",
            token: "type",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 128.0 },
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-locks-col-direction",
            token: "direction",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 108.0 },
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-locks-col-target",
            token: "target",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "rlv-locks-col-origin",
            token: "origin",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    // Grouped by section, in the reference's order — sorting would scatter the
    // four registries into each other.
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

// --- Pure view model ------------------------------------------------------

/// Which registry a row came out of. The Fluent key for the Type column, and
/// the section order the list is grouped in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockCategory {
    /// A specific worn attachment that may not come off.
    Attachment,
    /// An attachment point nothing may go on or come off.
    AttachmentPoint,
    /// A clothing layer nothing may go on or come off.
    WearableType,
    /// A folder, and possibly its subtree.
    Folder,
}

impl LockCategory {
    /// The Fluent key naming this category in the Type column.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Attachment => "rlv-locks-type-attachment",
            Self::AttachmentPoint => "rlv-locks-type-attachment-point",
            Self::WearableType => "rlv-locks-type-wearable",
            Self::Folder => "rlv-locks-type-folder",
        }
    }
}

/// One row of the lock list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockRow {
    /// Which registry it came from.
    pub category: LockCategory,
    /// Which way the lock points, and for a folder lock also its permission and
    /// scope — the reference's `add`/`rem`, `deny`/`allow`, `node`/`subtree`
    /// joined with slashes.
    pub direction: String,
    /// What the lock names.
    pub target: String,
    /// The object holding it.
    pub origin: String,
}

/// `add` or `rem` — the reference's `rlvLockMaskToString`.
#[must_use]
pub const fn lock_kind_text(kind: RlvLockKind) -> &'static str {
    match kind {
        RlvLockKind::Add => "add",
        RlvLockKind::Remove => "rem",
        // The lock vocabularies are `#[non_exhaustive]`: a direction, a
        // permission or a scope added to the engine later shows as "unknown"
        // rather than failing to compile a window that has nothing to say
        // about it — which is what the reference's own `default:` arms do.
        _ => "unknown",
    }
}

/// `deny` or `allow` — the reference's `rlvFolderLockPermissionToString`.
#[must_use]
pub const fn permission_text(permission: RlvFolderLockPermission) -> &'static str {
    match permission {
        RlvFolderLockPermission::Deny => "deny",
        RlvFolderLockPermission::Allow => "allow",
        _ => "unknown",
    }
}

/// `node` or `subtree` — the reference's `rlvFolderLockScopeToString`.
#[must_use]
pub const fn scope_text(scope: RlvFolderLockScope) -> &'static str {
    match scope {
        RlvFolderLockScope::Node => "node",
        RlvFolderLockScope::Subtree => "subtree",
        _ => "unknown",
    }
}

/// What a folder lock names — the reference's `rlvFolderLockSourceToTarget`.
///
/// Only the two path sources name a folder outright; the other three name
/// *things worn*, and the folders they lock are wherever those came from. The
/// row says which of the five it is, because that is the whole of why two
/// folder locks with the same effect can have been spelled differently.
#[must_use]
pub fn folder_source_text(source: &RlvFolderLockSource) -> String {
    match *source {
        RlvFolderLockSource::Attachment(object) => format!("Attachment ({object})"),
        RlvFolderLockSource::AttachmentPoint(point) => {
            format!("Attachment point ({})", point.name())
        }
        RlvFolderLockSource::WearableType(slot) => format!("Wearable type ({})", slot.name()),
        RlvFolderLockSource::SharedPath(ref path) => {
            let separator = if path.is_empty() { "" } else { "/" };
            format!("Shared path (#RLV{separator}{path})")
        }
        RlvFolderLockSource::RootFolder => "Root folder".to_owned(),
        _ => "(unknown)".to_owned(),
    }
}

/// The direction cell of a folder lock: its kind, permission and scope joined,
/// the way the reference writes them.
#[must_use]
pub fn folder_direction_text(lock: &RlvFolderLock) -> String {
    format!(
        "{}/{}/{}",
        lock_kind_text(lock.kind),
        permission_text(lock.permission),
        scope_text(lock.scope)
    )
}

/// Project the lock model onto the list, in the reference's section order.
#[must_use]
pub fn project(state: &RlvState) -> Vec<LockRow> {
    let locks: RlvLocks = state.locks();
    let mut rows = Vec::new();

    for lock in locks.attachment_locks() {
        rows.push(LockRow {
            category: LockCategory::Attachment,
            direction: lock_kind_text(RlvLockKind::Remove).to_owned(),
            target: lock.attachment.to_string(),
            origin: issuer_label(state, lock.object),
        });
    }
    for lock in locks.point_locks() {
        rows.push(LockRow {
            category: LockCategory::AttachmentPoint,
            direction: lock_kind_text(lock.kind).to_owned(),
            // A bare `@addattach=n` / `@remattach=n` names no point and locks
            // every one at once; the reference shows nothing there, and an
            // asterisk says the same thing without looking like a missing value.
            target: lock
                .point
                .map_or_else(|| "*".to_owned(), |p| p.name().to_owned()),
            origin: issuer_label(state, lock.object),
        });
    }
    for lock in locks.wearable_type_locks() {
        rows.push(LockRow {
            category: LockCategory::WearableType,
            direction: lock_kind_text(lock.kind).to_owned(),
            target: lock
                .slot
                .map_or_else(|| "*".to_owned(), |s| s.name().to_owned()),
            origin: issuer_label(state, lock.object),
        });
    }
    for lock in locks.folder_locks() {
        rows.push(LockRow {
            category: LockCategory::Folder,
            direction: folder_direction_text(lock),
            target: folder_source_text(&lock.source),
            origin: issuer_label(state, lock.object),
        });
    }

    rows
}

// --- Resources ------------------------------------------------------------

/// The floater's live view state.
#[derive(Resource, Debug, Default)]
struct LocksView {
    /// The rows, in section order.
    rows: Vec<LockRow>,
    /// The [`RlvSession`] revision they were projected at.
    built_revision: u64,
    /// Whether anything has been projected yet.
    built: bool,
}

/// The floater's retained entities.
#[derive(Resource, Debug)]
struct LocksUi {
    /// The virtualized viewport.
    viewport: Entity,
    /// The count line under the list.
    count_text: Entity,
}

// --- Plugin ---------------------------------------------------------------

/// Registers the Locks floater and its view systems.
#[derive(Debug, Clone, Copy, Default)]
pub struct RlvLocksPlugin;

impl Plugin for RlvLocksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocksView>()
            .add_systems(
                Startup,
                spawn_locks_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                rebuild_locks_view
                    .before(layout_virtual_lists)
                    .run_if(floater_shown(LOCKS_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (populate_lock_rows, bind_lock_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(LOCKS_FLOATER_ID)),
            );
    }
}

// --- Floater --------------------------------------------------------------

/// The Locks floater's [`FloaterSpec`].
#[must_use]
pub fn rlv_locks_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: LOCKS_FLOATER_ID,
        title: "RLVa Locks".to_owned(),
        position: Vec2::new(360.0, 190.0),
        default_size: Some(Vec2::new(660.0, 320.0)),
        min_size: Some(Vec2::new(440.0, 200.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: chrome only.
fn spawn_locks_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, rlv_locks_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("rlv-locks-title"));
    let builder = commands.register_system(build_locks_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the table and its count line.
fn build_locks_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("rlv-locks-content"),
            ChildOf(handle.content),
        ))
        .id();

    let table = spawn_table(&mut commands, content, &LOCK_TABLE);
    commands
        .entity(table.viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(0)));
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
            Name::new("rlv-locks-count"),
            ChildOf(content),
        ))
        .id();

    commands.insert_resource(LocksUi {
        viewport: table.viewport,
        count_text,
    });
}

// --- View systems ---------------------------------------------------------

/// Reproject when the held set changed; keep the item count and count line in
/// step.
fn rebuild_locks_view(
    session: Res<RlvSession>,
    mut view: ResMut<LocksView>,
    ui: Option<Res<LocksUi>>,
    translator: Translator,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    if view.built && view.built_revision == session.revision() {
        return;
    }
    view.built = true;
    view.built_revision = session.revision();
    view.rows = project(session.state());

    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = view.rows.len();
    }
    let label = translator.format(
        "rlv-locks-count",
        &TransArgs::new().int("locks", i64::try_from(view.rows.len()).unwrap_or(i64::MAX)),
    );
    if let Ok(mut text) = texts.get_mut(ui.count_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Build the cells of each freshly-pooled row.
fn populate_lock_rows(
    mut commands: Commands,
    ui: Option<Res<LocksUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.viewport, &LOCK_TABLE);
    }
}

/// Bind each pooled row to the lock it now presents.
fn bind_lock_rows(
    view: Res<LocksView>,
    ui: Option<Res<LocksUi>>,
    translator: Translator,
    mut rows: Query<(Ref<VirtualRow>, &ChildOf, &TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed();
    for (row, child_of, cells) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let entry = row.index.and_then(|index| view.rows.get(index));
        let values: [(usize, String, Color); 4] = match entry {
            Some(entry) => [
                (
                    COL_TYPE,
                    translator.get(entry.category.label_key()),
                    LABEL_COLOR,
                ),
                (COL_DIRECTION, entry.direction.clone(), LABEL_COLOR),
                (COL_TARGET, entry.target.clone(), LABEL_COLOR),
                (COL_ORIGIN, entry.origin.clone(), DIM_LABEL_COLOR),
            ],
            None => [
                (COL_TYPE, String::new(), LABEL_COLOR),
                (COL_DIRECTION, String::new(), LABEL_COLOR),
                (COL_TARGET, String::new(), LABEL_COLOR),
                (COL_ORIGIN, String::new(), LABEL_COLOR),
            ],
        };
        for (column, value, color) in values {
            if let Some(cell) = cells.cell(column) {
                set_table_cell(&mut texts, cell, &value, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LockCategory, folder_source_text, lock_kind_text, permission_text, project, scope_text,
    };
    use pretty_assertions::assert_eq;
    use sl_rlv::{
        RlvFolderLockPermission, RlvFolderLockScope, RlvFolderLockSource, RlvLockKind, RlvState,
        parse_chat_line,
    };
    use uuid::Uuid;

    /// A `Box<dyn Error>` alias, so a test can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// Feed one chat line to `state` as if `object` had said it.
    fn say(state: &mut RlvState, object: Uuid, line: &str) -> Result<(), TestError> {
        let commands = parse_chat_line(line).ok_or("not an RLV line")?;
        for command in commands {
            let command = command.map_err(|error| error.to_string())?;
            state.apply(object, &command);
        }
        Ok(())
    }

    /// A bare `@detach=n` locks the issuing object on, and shows as an
    /// attachment lock pointing `rem`.
    ///
    /// The lock exists only once the viewer has told the state machine *where*
    /// the object is worn: a bare `@detach=n` locks the attachment the issuer
    /// belongs to, and with no attachment there is nothing to lock. That is the
    /// reference's rule too, and it is why the window can show a restriction
    /// with no lock beside it.
    #[test]
    fn a_bare_detach_lists_as_an_attachment_lock() -> Result<(), TestError> {
        let boot = Uuid::from_u128(1);
        let mut state = RlvState::new();
        say(&mut state, boot, "@detach=n")?;
        assert!(
            project(&state).is_empty(),
            "an unplaced object locks nothing"
        );
        let point = sl_rlv::RlvAttachmentPoint::from_name("left foot").ok_or("no such point")?;
        state.set_object_attachment(boot, Some(sl_rlv::RlvObjectAttachment::new(boot, point)));

        let rows = project(&state);
        assert_eq!(rows.len(), 1);
        let row = rows.first().ok_or("no lock row")?;
        assert_eq!(row.category, LockCategory::Attachment);
        assert_eq!(row.direction, "rem");
        assert_eq!(row.target, boot.to_string());
        Ok(())
    }

    /// A point lock names its point; a bare one names every point, which the
    /// list writes as `*` rather than leaving blank.
    #[test]
    fn a_point_lock_names_its_point_or_all_of_them() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let mut state = RlvState::new();
        say(&mut state, collar, "@remattach:chest=n")?;
        say(&mut state, collar, "@addattach=n")?;

        let rows = project(&state);
        let points: Vec<(&str, &str)> = rows
            .iter()
            .filter(|row| row.category == LockCategory::AttachmentPoint)
            .map(|row| (row.direction.as_str(), row.target.as_str()))
            .collect();
        assert!(points.contains(&("rem", "chest")), "{points:?}");
        assert!(points.contains(&("add", "*")), "{points:?}");
        Ok(())
    }

    /// A layer lock lists under its own section with the layer's name.
    #[test]
    fn a_layer_lock_names_its_layer() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let mut state = RlvState::new();
        say(&mut state, collar, "@remoutfit:gloves=n")?;

        let rows = project(&state);
        let layers: Vec<&str> = rows
            .iter()
            .filter(|row| row.category == LockCategory::WearableType)
            .map(|row| row.target.as_str())
            .collect();
        assert_eq!(layers, vec!["gloves"]);
        Ok(())
    }

    /// The four registries list in the reference's order, so the sections do
    /// not interleave.
    #[test]
    fn the_sections_keep_the_references_order() -> Result<(), TestError> {
        let collar = Uuid::from_u128(1);
        let mut state = RlvState::new();
        say(&mut state, collar, "@remoutfit:gloves=n")?;
        say(&mut state, collar, "@remattach:chest=n")?;
        say(&mut state, collar, "@detach=n")?;
        let point = sl_rlv::RlvAttachmentPoint::from_name("chest").ok_or("no such point")?;
        state.set_object_attachment(
            collar,
            Some(sl_rlv::RlvObjectAttachment::new(collar, point)),
        );

        let order: Vec<LockCategory> = project(&state).iter().map(|row| row.category).collect();
        let mut sorted = order.clone();
        sorted.sort_by_key(|category| match category {
            LockCategory::Attachment => 0_u8,
            LockCategory::AttachmentPoint => 1,
            LockCategory::WearableType => 2,
            LockCategory::Folder => 3,
        });
        assert_eq!(order, sorted);
        Ok(())
    }

    /// Each folder-lock source renders as the thing it names, including the
    /// `#RLV` root, whose empty path must not produce a trailing slash.
    #[test]
    fn a_folder_source_renders_as_what_it_names() {
        let object = Uuid::from_u128(3);
        assert_eq!(
            folder_source_text(&RlvFolderLockSource::Attachment(object)),
            format!("Attachment ({object})")
        );
        assert_eq!(
            folder_source_text(&RlvFolderLockSource::SharedPath(String::new())),
            "Shared path (#RLV)"
        );
        assert_eq!(
            folder_source_text(&RlvFolderLockSource::SharedPath("Boots".to_owned())),
            "Shared path (#RLV/Boots)"
        );
        assert_eq!(
            folder_source_text(&RlvFolderLockSource::RootFolder),
            "Root folder"
        );
    }

    /// The three vocabularies are the reference's words, which is what makes a
    /// row readable next to a Firestorm screenshot.
    #[test]
    fn the_lock_vocabulary_is_the_references() {
        assert_eq!(lock_kind_text(RlvLockKind::Add), "add");
        assert_eq!(lock_kind_text(RlvLockKind::Remove), "rem");
        assert_eq!(permission_text(RlvFolderLockPermission::Deny), "deny");
        assert_eq!(permission_text(RlvFolderLockPermission::Allow), "allow");
        assert_eq!(scope_text(RlvFolderLockScope::Node), "node");
        assert_eq!(scope_text(RlvFolderLockScope::Subtree), "subtree");
    }

    /// An unrestricted viewer has no locks at all.
    #[test]
    fn an_unrestricted_viewer_has_no_locks() {
        assert!(project(&RlvState::new()).is_empty());
    }
}
