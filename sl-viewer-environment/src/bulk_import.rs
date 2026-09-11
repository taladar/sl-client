//! **Bulk import** of a folder of legacy WindLight presets: World ▸ Environment
//! ▸ Bulk Import ▸ Days / Skies / Water.
//!
//! The single-file Import buttons on the sky and water editors
//! ([`crate::settings_editor`]) answer "I have *this* preset and want to edit
//! it". This answers the other question a decade of WindLight collections
//! raises: "I have four hundred of them and want them in my inventory". Pick a
//! folder, and every `.xml` in it is converted and filed as a settings asset.
//!
//! # A folder, where the reference takes a multi-selection
//!
//! Firestorm's `File.ImportWindlightBulk` opens a *multi-select file* chooser
//! (`LLFilePickerReplyThread::startPicker(…, FFLOAD_XML, true)`) and imports
//! whatever was ticked. This asks for the folder instead, because
//!
//! - a WindLight collection *is* folders — `windlight/skies`, `windlight/water`,
//!   `windlight/days` — and "import my skies" is one click rather than a click
//!   and a Ctrl+A; and
//! - a **day cycle** needs its siblings anyway. Its keyframes name sky presets
//!   stored in a neighbouring directory, so importing one means reading a folder
//!   whatever the chooser asked for.
//!
//! # Where a day cycle's siblings come from
//!
//! For Days, the `skies/` and `water/` folders are looked for beside the chosen
//! folder first (`windlight/skies` next to `windlight/days`, the layout a real
//! collection has) and inside it second. The reference looks *only* inside
//! (`LLSettingsVODay::buildFromLegacyPreset` derives its base path with one
//! `getDirName` on the day file's full path, which is the day folder itself),
//! and so on a real collection always misses and falls back to the preset
//! folder the viewer itself ships.
//!
//! Within a folder, a preset is found by **name**: every `.xml` stem is
//! percent-unescaped ([`legacy_preset_name`]) and indexed under the name that
//! yields. That is looser than the reference's three-candidate re-escaping
//! (`legacy_name_to_filename`, "a disturbing hack" in its own words), and it has
//! to be — the escaping in the wild is not one scheme, as the viewer's own
//! shipped `%28SS%29%20Atmos%2023%2E30%202.xml` shows, whose `%2E` no current
//! `LLURI::escape` produces.
//!
//! # It files items; it does not upload assets itself
//!
//! Each converted preset takes the path a Save As takes: `CreateInventoryItem`
//! mints a settings item of the right kind, and the body is written onto it when
//! the reply names it — the shared [`PendingSettingsCreations`] queue, in order,
//! so a bulk import and an editor's Save As cannot claim each other's items.
//!
//! # Nothing hangs
//!
//! A run that stops hearing replies is finished by a watchdog
//! ([`BULK_IMPORT_REPLY_TIMEOUT`]) and says how many items it never saw, rather
//! than holding the menu entries greyed for the rest of the session.
//!
//! Reference (Firestorm, read-only): `llviewermenufile.cpp`
//! (`import_windlight_bulk`, `FSFileImportWindlightBulk`,
//! `on_windlight_imported`), `menu_viewer.xml` (World ▸ Environment ▸ Bulk
//! Import), `llsettingsvo.cpp` (`LLSettingsVODay::buildFromLegacyPreset`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future::poll_once};
use sl_client_bevy::{
    FolderType, InventoryFolderKey, SettingsKind, SlCommand, environment_asset_to_bytes,
    legacy_day_cycle_from_bytes, legacy_preset_from_bytes, legacy_preset_name,
};
use sl_viewer_inventory::inventory::InventoryModel;
use sl_viewer_inventory::inventory_actions::new_settings_item;
use sl_viewer_notifications::ShowNotification;
use sl_viewer_platform::file_dialog::{
    FileDialogClosed, FileDialogOutcome, FileDialogSelection, OpenFileDialog,
};
use sl_viewer_ui_core::i18n::{TransArgs, Translator};
use sl_viewer_world_api::{PendingSettingsCreations, SettingsItemCreated};

/// How long a run waits for the next `UpdateCreateInventoryItem` reply before
/// giving up on the items it has not seen.
///
/// Generous, because the whole point of a bulk import is a queue of hundreds:
/// the simulator answers them one at a time and an item it is slow about must
/// not be written off. What this bounds is a run that has stopped entirely.
pub const BULK_IMPORT_REPLY_TIMEOUT: f32 = 120.0;

/// How many failing files a summary names before it stops listing them.
///
/// A folder where every file is the wrong kind produces one failure per file,
/// and a notification is not a log.
const SUMMARY_FAILURE_LIMIT: usize = 8;

/// Start a bulk import of `kind` — World ▸ Environment ▸ Bulk Import ▸ Days /
/// Skies / Water.
///
/// Written by the menu bar; answered by a folder chooser, then by whatever the
/// folder held.
#[derive(Message, Debug, Clone, Copy)]
pub struct StartWindlightBulkImport {
    /// Which of the three legacy preset kinds the chosen folder holds.
    pub kind: SettingsKind,
}

/// The purpose the folder chooser for `kind` is asked under — also the key its
/// last-used directory is remembered by, so Skies and Water reopen in their own
/// corners of a collection.
#[must_use]
pub const fn bulk_import_purpose(kind: SettingsKind) -> &'static str {
    match kind {
        SettingsKind::Sky => "bulk-import-skies",
        SettingsKind::Water => "bulk-import-water",
        SettingsKind::DayCycle => "bulk-import-days",
    }
}

/// One preset that converted: the name to file it under, and the settings asset
/// to write onto the item once the simulator has minted one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BulkConverted {
    /// The inventory item's name — the file's stem, percent-unescaped.
    name: String,
    /// The encoded settings asset.
    data: Vec<u8>,
}

/// One file that did not convert, and why — a line of the run's summary.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BulkFailure {
    /// The file's name (not its whole path: the folder is the same for all of
    /// them and the summary has already said which).
    file: String,
    /// What went wrong, in the words of whichever converter refused.
    reason: String,
}

/// What reading a folder came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BulkConversion {
    /// The presets that converted, in file-name order.
    converted: Vec<BulkConverted>,
    /// The files that did not.
    failed: Vec<BulkFailure>,
}

/// The run in flight, if there is one.
///
/// One at a time, as in the reference (`bulk_windlight_import_active` greys
/// every Bulk Import entry while a run is going): two runs would interleave
/// their creations on the one ordered reply queue and neither could say which
/// items were its own.
#[derive(Resource, Debug, Default)]
pub struct BulkImportRun {
    /// The run, or `None` when the viewer is idle.
    run: Option<RunState>,
}

/// A bulk import's state through its three phases.
#[derive(Debug)]
struct RunState {
    /// Which kind the run is importing — which converter each file goes
    /// through, and which subtype each fresh item is stamped with.
    kind: SettingsKind,
    /// What the run is doing now.
    phase: RunPhase,
    /// The files that failed, accumulated across the phases so the summary at
    /// the end has all of them.
    failed: Vec<BulkFailure>,
    /// How many items the simulator has confirmed.
    filed: usize,
}

/// Where a run has got to.
#[derive(Debug)]
enum RunPhase {
    /// The folder chooser is on screen (or about to be).
    Choosing,
    /// The folder is being read and converted off the frame thread.
    Converting,
    /// The creations are in flight; this many replies are still owed.
    Filing {
        /// Items asked for and not yet seen back.
        outstanding: usize,
        /// Seconds since the last reply, against [`BULK_IMPORT_REPLY_TIMEOUT`].
        quiet: f32,
    },
}

impl BulkImportRun {
    /// Whether a run is in flight — what the menu's Bulk Import entries are
    /// greyed on.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.run.is_some()
    }
}

/// The folder-reading half of a run, off the frame thread.
///
/// A component rather than a resource field for the reason the file dialog's
/// task is one: a task dropped with the world is a task cancelled with it.
#[derive(Component)]
struct BulkConversionTask {
    /// The running read, yielding what the folder held.
    task: Task<BulkConversion>,
}

/// Hand-written: [`Task`] is not [`Debug`], and a pending folder read has
/// nothing else worth printing.
impl core::fmt::Debug for BulkConversionTask {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BulkConversionTask").finish_non_exhaustive()
    }
}

/// Open the folder chooser for a Bulk Import that was asked for.
fn start_bulk_import(
    mut requests: MessageReader<StartWindlightBulkImport>,
    mut state: ResMut<BulkImportRun>,
    mut dialogs: MessageWriter<OpenFileDialog>,
    translator: Translator,
) {
    for request in requests.read() {
        if state.run.is_some() {
            // The menu entries are greyed while a run is going, so this is a
            // race rather than a misuse: say so and drop it.
            warn!("a WindLight bulk import is already running; ignoring another");
            continue;
        }
        state.run = Some(RunState {
            kind: request.kind,
            phase: RunPhase::Choosing,
            failed: Vec::new(),
            filed: 0,
        });
        dialogs.write(OpenFileDialog {
            purpose: bulk_import_purpose(request.kind).into(),
            title: translator.get(match request.kind {
                SettingsKind::Sky => "bulk-import-skies-title",
                SettingsKind::Water => "bulk-import-water-title",
                SettingsKind::DayCycle => "bulk-import-days-title",
            }),
            // A folder chooser has no file types to filter.
            filters: Vec::new(),
            start_dir: None,
            selection: FileDialogSelection::Folder,
        });
    }
}

/// Take the folder the chooser came back with and start reading it.
fn begin_bulk_conversion(
    mut commands: Commands,
    mut closed: MessageReader<FileDialogClosed>,
    mut state: ResMut<BulkImportRun>,
) {
    for reply in closed.read() {
        let Some(run) = state.run.as_mut() else {
            continue;
        };
        if *reply.purpose != *bulk_import_purpose(run.kind) {
            // Somebody else's dialog — an editor's single-file Import, say.
            continue;
        }
        if !matches!(run.phase, RunPhase::Choosing) {
            // A second reply under this run's purpose, while the run is already
            // past the chooser. Answering it would start a second folder read
            // over the first's creations.
            warn!("a bulk import folder was chosen twice; ignoring the second");
            continue;
        }
        let FileDialogOutcome::Picked(ref folder) = reply.outcome else {
            // Cancelled, or refused because another chooser was up: the run
            // never started, so end it silently.
            state.run = None;
            continue;
        };
        let kind = run.kind;
        let folder = folder.clone();
        run.phase = RunPhase::Converting;
        commands.spawn(BulkConversionTask {
            task: IoTaskPool::get().spawn(async move { convert_folder(kind, &folder) }),
        });
    }
}

/// Poll the folder read; when it finishes, ask the simulator for one item per
/// preset that converted.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the task query and its \
              despawns, the run state, the inventory the destination folder comes out of, the \
              creation queue and the command channel each item needs, and the notification \
              channel a run with nothing to file reports through"
)]
fn file_converted_presets(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut BulkConversionTask)>,
    mut state: ResMut<BulkImportRun>,
    inventory: Option<Res<InventoryModel>>,
    mut creations: ResMut<PendingSettingsCreations>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut notify: MessageWriter<ShowNotification>,
    translator: Translator,
) {
    for (entity, mut pending) in &mut tasks {
        let Some(conversion) = block_on(poll_once(&mut pending.task)) else {
            continue;
        };
        commands.entity(entity).despawn();
        let Some(run) = state.run.as_mut() else {
            continue;
        };
        if !matches!(run.phase, RunPhase::Converting) {
            // The read that finished is not the one this run is waiting on.
            continue;
        }
        run.failed.extend(conversion.failed);
        for failure in &run.failed {
            warn!("bulk import: {} — {}", failure.file, failure.reason);
        }
        let folder_id = inventory.as_deref().and_then(settings_destination);
        let Some(folder_id) = folder_id else {
            // The reference's `findCategoryUUIDForType(FT_SETTINGS)`, which it
            // uses without checking; there is nowhere to put an item until the
            // inventory skeleton has arrived.
            run.failed.push(BulkFailure {
                file: String::new(),
                reason: translator.get("bulk-import-no-settings-folder"),
            });
            report(&mut notify, &translator, run);
            state.run = None;
            continue;
        };
        let kind = run.kind;
        let outstanding = conversion.converted.len();
        for converted in conversion.converted {
            sl_commands.write(SlCommand(new_settings_item(
                kind,
                &converted.name,
                folder_id,
            )));
            creations.enqueue(kind, Some(converted.data));
        }
        if outstanding == 0 {
            // Nothing converted — an empty folder, or the wrong one. The
            // reference reports the same case the same way, straight from
            // `import_windlight_bulk`'s "For error cases" tail.
            report(&mut notify, &translator, run);
            state.run = None;
            continue;
        }
        // The reference's "Importing Windlights..." — a modal upload dialog
        // there, a tip here, because a run of four hundred uploads must not
        // take the viewer away for the length of it.
        notify.write(
            ShowNotification::new("WindlightBulkImportStarted")
                .arg("COUNT", outstanding.to_string()),
        );
        run.phase = RunPhase::Filing {
            outstanding,
            quiet: 0.0,
        };
    }
}

/// Count the items the simulator confirms, and report the run when the last one
/// lands.
fn count_filed_items(
    mut created: MessageReader<SettingsItemCreated>,
    mut state: ResMut<BulkImportRun>,
    mut notify: MessageWriter<ShowNotification>,
    translator: Translator,
) {
    let mut done = false;
    for _item in created.read() {
        let Some(run) = state.run.as_mut() else {
            continue;
        };
        let RunPhase::Filing {
            ref mut outstanding,
            ref mut quiet,
        } = run.phase
        else {
            continue;
        };
        if *outstanding == 0 {
            continue;
        }
        // "The next one is mine" holds because every settings creation in the
        // viewer passes through one ordered queue (`PendingSettingsCreations`),
        // so a reply reaching this system while a run owes replies is this
        // run's — the same count the editors' Save As and the library window
        // keep.
        *outstanding = outstanding.saturating_sub(1);
        *quiet = 0.0;
        run.filed = run.filed.saturating_add(1);
        done = *outstanding == 0;
    }
    if done && let Some(run) = state.run.as_mut() {
        report(&mut notify, &translator, run);
        state.run = None;
    }
}

/// End a run that has stopped hearing back, saying how many items it never saw.
fn expire_quiet_bulk_import(
    time: Res<Time>,
    mut state: ResMut<BulkImportRun>,
    mut notify: MessageWriter<ShowNotification>,
    translator: Translator,
) {
    let Some(run) = state.run.as_mut() else {
        return;
    };
    let RunPhase::Filing {
        outstanding,
        ref mut quiet,
    } = run.phase
    else {
        return;
    };
    *quiet += time.delta_secs();
    if *quiet < BULK_IMPORT_REPLY_TIMEOUT {
        return;
    }
    run.failed.push(BulkFailure {
        file: String::new(),
        reason: translator.format(
            "bulk-import-never-answered",
            &TransArgs::new().int("count", i64::try_from(outstanding).unwrap_or(i64::MAX)),
        ),
    });
    report(&mut notify, &translator, run);
    state.run = None;
}

/// The folder a fresh settings item goes in: the Settings system folder, or the
/// agent's root when the skeleton has no such folder.
fn settings_destination(inventory: &InventoryModel) -> Option<InventoryFolderKey> {
    inventory
        .folder_by_type(FolderType::Settings)
        .or_else(|| inventory.agent_root())
}

/// Raise the run's summary: the reference's own tip when every file worked, and
/// a listing of what did not when some did not.
fn report(notify: &mut MessageWriter<ShowNotification>, translator: &Translator, run: &RunState) {
    if run.failed.is_empty() {
        notify.write(ShowNotification::new("WindlightBulkImportFinished"));
        return;
    }
    let named: Vec<String> = run
        .failed
        .iter()
        .take(SUMMARY_FAILURE_LIMIT)
        .map(|failure| {
            if failure.file.is_empty() {
                failure.reason.clone()
            } else {
                format!("{}: {}", failure.file, failure.reason)
            }
        })
        .collect();
    let mut reasons = named.join("\n");
    if let Some(rest) = run.failed.len().checked_sub(SUMMARY_FAILURE_LIMIT)
        && rest > 0
    {
        reasons.push('\n');
        reasons.push_str(&translator.format(
            "bulk-import-and-more",
            &TransArgs::new().int("count", i64::try_from(rest).unwrap_or(i64::MAX)),
        ));
    }
    notify.write(
        ShowNotification::new("WindlightBulkImportSummary")
            .arg("FILED", run.filed.to_string())
            .arg("FAILED", run.failed.len().to_string())
            .arg("REASONS", reasons),
    );
}

// ---------------------------------------------------------------------------
// Reading a folder. No Bevy below this line — this is the part worth testing
// against a real directory.
// ---------------------------------------------------------------------------

/// Convert every legacy preset in `folder` into a settings asset of `kind`.
///
/// Files are taken in name order so a run is reproducible, and a file that will
/// not convert costs itself and nothing else: the reference imports what it can
/// and reports the rest, and so does this.
fn convert_folder(kind: SettingsKind, folder: &Path) -> BulkConversion {
    let files = preset_files(folder);
    // Only a day cycle needs siblings, and building the index means listing two
    // more directories — so it is built once here rather than per file, and not
    // at all for the other two kinds.
    let siblings = (kind == SettingsKind::DayCycle).then(|| SiblingPresets::beside(folder));
    let mut conversion = BulkConversion::default();
    for path in files {
        let file = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map_or_else(String::new, legacy_preset_name);
        match convert_one(kind, &name, &path, siblings.as_ref()) {
            Ok(data) => conversion.converted.push(BulkConverted { name, data }),
            Err(reason) => conversion.failed.push(BulkFailure { file, reason }),
        }
    }
    conversion
}

/// Read and convert one file, yielding the encoded settings asset or the reason
/// it could not be one.
fn convert_one(
    kind: SettingsKind,
    name: &str,
    path: &Path,
    siblings: Option<&SiblingPresets>,
) -> Result<Vec<u8>, String> {
    let bytes = fs_err::read(path).map_err(|error| error.to_string())?;
    let asset = match kind {
        SettingsKind::Sky | SettingsKind::Water => {
            legacy_preset_from_bytes(kind, name, &bytes).map_err(|error| error.to_string())?
        }
        SettingsKind::DayCycle => {
            let siblings =
                siblings.ok_or_else(|| "no sibling presets were looked for".to_owned())?;
            legacy_day_cycle_from_bytes(name, &bytes, |wanted_kind, wanted| {
                siblings.read(wanted_kind, wanted)
            })
            .map_err(|error| error.to_string())?
        }
    };
    Ok(environment_asset_to_bytes(&asset))
}

/// The `.xml` files in `folder`, in name order.
///
/// A folder that cannot be listed reads as empty; the run then reports having
/// converted nothing, which is what happened.
fn preset_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs_err::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_xml_file(path))
        .collect();
    files.sort();
    files
}

/// Whether `path` is a regular file with an `.xml` extension, in any case — a
/// WindLight collection copied off a Windows machine is full of `.XML`.
fn is_xml_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
}

/// The sky and water presets a legacy day cycle's keyframes name, indexed by the
/// preset name each file's stem unescapes to.
#[derive(Debug, Default)]
struct SiblingPresets {
    /// Sky preset name → file.
    skies: BTreeMap<String, PathBuf>,
    /// Water preset name → file.
    water: BTreeMap<String, PathBuf>,
}

impl SiblingPresets {
    /// Index the `skies/` and `water/` folders that go with the day folder
    /// `days` — beside it first, inside it second (see the module docs).
    fn beside(days: &Path) -> Self {
        Self {
            skies: sibling_folder(days, "skies").map_or_else(BTreeMap::new, |dir| index(&dir)),
            water: sibling_folder(days, "water").map_or_else(BTreeMap::new, |dir| index(&dir)),
        }
    }

    /// The bytes of the named preset of `kind`, if the index has one.
    fn read(&self, kind: SettingsKind, name: &str) -> Option<Vec<u8>> {
        let index = match kind {
            SettingsKind::Sky => &self.skies,
            SettingsKind::Water => &self.water,
            // A day cycle never names another day cycle.
            SettingsKind::DayCycle => return None,
        };
        fs_err::read(index.get(name)?).ok()
    }
}

/// The folder called `name` that goes with the day folder `days`: its sibling
/// (`windlight/skies` beside `windlight/days`) if there is one, otherwise a
/// child of `days` itself, otherwise nothing.
fn sibling_folder(days: &Path, name: &str) -> Option<PathBuf> {
    let beside = days.parent().map(|parent| parent.join(name));
    let inside = days.join(name);
    beside
        .filter(|path| path.is_dir())
        .or_else(|| Some(inside).filter(|path| path.is_dir()))
}

/// Index one preset folder by the name each `.xml` stem unescapes to.
///
/// The raw stem is indexed too where it differs and nothing already answers to
/// it, so a collection whose filenames were never escaped is found as readily as
/// one whose were.
fn index(folder: &Path) -> BTreeMap<String, PathBuf> {
    let mut presets: BTreeMap<String, PathBuf> = BTreeMap::new();
    for path in preset_files(folder) {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        drop(presets.insert(legacy_preset_name(stem), path.clone()));
        presets.entry(stem.to_owned()).or_insert(path);
    }
    presets
}

/// Registers the WindLight bulk importer: the [`StartWindlightBulkImport`]
/// channel, the run state the menu greys its entries on, and the four systems
/// that carry a run from the chooser to its summary.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindlightBulkImportPlugin;

impl Plugin for WindlightBulkImportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BulkImportRun>()
            .add_message::<StartWindlightBulkImport>()
            // Registered by whichever plugin owns them as well; all idempotent,
            // and what lets this stand up in a host that brought only some of
            // them (a `MessageWriter` for an unregistered message is a system
            // that never runs).
            .add_message::<OpenFileDialog>()
            .add_message::<FileDialogClosed>()
            .add_message::<ShowNotification>()
            .add_message::<SettingsItemCreated>()
            .add_message::<SlCommand>()
            .init_resource::<PendingSettingsCreations>()
            .add_systems(
                Update,
                (
                    start_bulk_import,
                    begin_bulk_conversion,
                    file_converted_presets,
                    count_filed_items,
                    expire_quiet_bulk_import,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{SettingsKind, environment_asset_from_bytes};
    use sl_viewer_notifications::ShowNotification;
    use sl_viewer_platform::file_dialog::{
        FileDialogClosed, FileDialogOutcome, FileDialogSelection, OpenFileDialog,
    };

    use super::{
        BULK_IMPORT_REPLY_TIMEOUT, BulkConversion, BulkFailure, BulkImportRun, RunPhase, RunState,
        StartWindlightBulkImport, WindlightBulkImportPlugin, bulk_import_purpose, convert_folder,
        index, sibling_folder,
    };

    /// A boxed error, so a test can `?` rather than reach for the `panic!` the
    /// workspace's lints (rightly) forbid.
    type TestError = Box<dyn core::error::Error>;

    /// A unique throwaway directory under the system temp dir (the crate has no
    /// `tempfile` dependency; this mirrors the helper the settings editor's
    /// tests use).
    fn tempdir(label: &str) -> Result<PathBuf, TestError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-{label}-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Write `contents` to `dir/name`, creating `dir` first.
    fn write(dir: &Path, name: &str, contents: &str) -> Result<(), TestError> {
        fs_err::create_dir_all(dir)?;
        fs_err::write(dir.join(name), contents)?;
        Ok(())
    }

    /// Enough of a legacy sky preset to convert — the sun angles alone are a
    /// key the sky converter recognises.
    const LEGACY_SKY: &str = r"<llsd>
    <map>
    <key>east_angle</key>
        <real>0</real>
    <key>sun_angle</key>
        <real>1.0</real>
    <key>star_brightness</key>
        <real>0</real>
    </map>
</llsd>
";

    /// Enough of a legacy water preset to convert.
    const LEGACY_WATER: &str = r"<llsd>
    <map>
    <key>blurMultiplier</key>
        <real>0.25</real>
    <key>waterFogDensity</key>
        <real>16</real>
    </map>
</llsd>
";

    /// A legacy day cycle naming one sky preset whose filename is escaped.
    const LEGACY_DAY: &str = r"<llsd>
    <array>
        <array>
            <real>0</real>
            <string>(SS) Atmos 00:00</string>
        </array>
    </array>
</llsd>
";

    /// A whole folder of skies converts, each under the name its escaped
    /// filename spells, and each into an asset that reads back as a sky.
    #[test]
    fn a_folder_of_skies_converts_file_by_file() -> Result<(), TestError> {
        let dir = tempdir("skies")?;
        write(&dir, "%28SS%29%20Atmos%2000%3A00.xml", LEGACY_SKY)?;
        write(&dir, "Plain.XML", LEGACY_SKY)?;
        // Not a preset at all, and not an XML file: one fails, one is skipped.
        write(&dir, "Broken.xml", "not xml")?;
        write(&dir, "notes.txt", "ignore me")?;

        let conversion = convert_folder(SettingsKind::Sky, &dir);
        assert_eq!(
            conversion
                .converted
                .iter()
                .map(|converted| converted.name.as_str())
                .collect::<Vec<_>>(),
            vec!["(SS) Atmos 00:00", "Plain"],
            "both skies, named by their unescaped stems, `.XML` included"
        );
        assert_eq!(
            conversion
                .failed
                .iter()
                .map(|failure| failure.file.as_str())
                .collect::<Vec<_>>(),
            vec!["Broken.xml"],
            "the unreadable file fails alone; the .txt is not a preset file"
        );
        let first = conversion
            .converted
            .first()
            .ok_or("a converted preset to read back")?;
        assert!(
            matches!(
                environment_asset_from_bytes(&first.name, &first.data),
                Some(sl_client_bevy::EnvironmentAsset::Sky(_))
            ),
            "and what was encoded reads back as a sky"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A day cycle finds its skies in the folder **beside** the day folder,
    /// which is where a real WindLight collection keeps them — and finds one by
    /// name however its filename was escaped.
    #[test]
    fn a_day_cycle_finds_the_skies_beside_its_folder() -> Result<(), TestError> {
        let root = tempdir("windlight")?;
        let days = root.join("days");
        write(&days, "Cycle.xml", LEGACY_DAY)?;
        write(
            &root.join("skies"),
            "%28SS%29%20Atmos%2000%3A00.xml",
            LEGACY_SKY,
        )?;
        write(&root.join("water"), "Default.xml", LEGACY_WATER)?;

        let conversion = convert_folder(SettingsKind::DayCycle, &days);
        assert_eq!(conversion.failed, Vec::new(), "nothing failed");
        let converted = conversion.converted.first().ok_or("the cycle converted")?;
        assert_eq!(converted.name, "Cycle", "named for its file");
        let Some(sl_client_bevy::EnvironmentAsset::DayCycle(cycle)) =
            environment_asset_from_bytes(&converted.name, &converted.data)
        else {
            return Err("the encoded asset reads back as a day cycle".into());
        };
        assert_eq!(
            cycle
                .sky_frames
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["sky:(SS) Atmos 00:00"],
            "the sky the keyframe names is in the asset"
        );
        fs_err::remove_dir_all(&root)?;
        Ok(())
    }

    /// With no skies to be found, every day cycle fails — and says which preset
    /// it was looking for, rather than filing a day of default skies.
    #[test]
    fn a_day_cycle_without_its_skies_fails_by_name() -> Result<(), TestError> {
        let days = tempdir("days-alone")?;
        write(&days, "Cycle.xml", LEGACY_DAY)?;

        let conversion = convert_folder(SettingsKind::DayCycle, &days);
        assert_eq!(conversion.converted, Vec::new(), "nothing was filed");
        let failure = conversion.failed.first().ok_or("the cycle failed")?;
        assert_eq!(failure.file, "Cycle.xml");
        assert!(
            failure.reason.contains("(SS) Atmos 00:00"),
            "naming the preset it could not find: {}",
            failure.reason
        );
        fs_err::remove_dir_all(&days)?;
        Ok(())
    }

    /// The sibling folders are looked for beside the day folder first and
    /// inside it second — the second being the only place the reference looks.
    #[test]
    fn sibling_folders_are_found_beside_first_and_inside_second() -> Result<(), TestError> {
        let root = tempdir("siblings")?;
        let days = root.join("days");
        fs_err::create_dir_all(days.join("skies"))?;
        fs_err::create_dir_all(root.join("skies"))?;
        assert_eq!(
            sibling_folder(&days, "skies"),
            Some(root.join("skies")),
            "beside wins over inside"
        );
        fs_err::remove_dir_all(root.join("skies"))?;
        assert_eq!(
            sibling_folder(&days, "skies"),
            Some(days.join("skies")),
            "and inside is still found when there is no sibling"
        );
        assert_eq!(
            sibling_folder(&days, "water"),
            None,
            "a folder that is nowhere is nowhere"
        );
        fs_err::remove_dir_all(&root)?;
        Ok(())
    }

    /// A preset folder is indexed under the name its filename unescapes to
    /// **and** under the raw stem, so a collection escaped by any scheme — or
    /// by none — is searchable by the name a day cycle spells.
    #[test]
    fn a_preset_folder_is_indexed_by_name_and_by_stem() -> Result<(), TestError> {
        let dir = tempdir("index")?;
        // `%2E` for a dot: an escaping no current `LLURI::escape` produces, and
        // one the viewer's own shipped preset folder is full of.
        write(&dir, "%28SS%29%20Atmos%2023%2E30.xml", LEGACY_SKY)?;
        write(&dir, "A-12AM.xml", LEGACY_SKY)?;

        let presets = index(&dir);
        assert_eq!(
            presets.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["%28SS%29%20Atmos%2023%2E30", "(SS) Atmos 23.30", "A-12AM",],
            "the unescaped name, the raw stem, and a stem that is its own name once"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The run.
    // -----------------------------------------------------------------------

    /// An app with the importer scheduled, so the systems run as they do in the
    /// viewer rather than being called by hand. The strings resolve to their own
    /// keys (`install_untranslated`), which is all these tests need — what they
    /// assert is the run's state machine, not the wording.
    fn run_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WindlightBulkImportPlugin));
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        app
    }

    /// Drain whatever notifications a frame raised.
    fn raised(app: &mut App) -> Vec<ShowNotification> {
        app.world_mut()
            .resource_mut::<Messages<ShowNotification>>()
            .drain()
            .collect()
    }

    /// **Bulk Import asks the desktop for a folder**, under the purpose its kind
    /// is remembered by — not for a file, which is what the single-preset Import
    /// on the editors asks for.
    #[test]
    fn starting_a_run_asks_for_a_folder() -> Result<(), TestError> {
        let mut app = run_app();
        app.world_mut().write_message(StartWindlightBulkImport {
            kind: SettingsKind::DayCycle,
        });
        app.update();
        let asked: Vec<OpenFileDialog> = app
            .world_mut()
            .resource_mut::<Messages<OpenFileDialog>>()
            .drain()
            .collect();
        let first = asked.first().ok_or("the chooser is asked for")?;
        assert_eq!(asked.len(), 1, "exactly one chooser");
        assert_eq!(
            &*first.purpose,
            bulk_import_purpose(SettingsKind::DayCycle),
            "under the day importer's own purpose"
        );
        assert_eq!(
            first.selection,
            FileDialogSelection::Folder,
            "a folder, not a file"
        );
        assert!(
            app.world().resource::<BulkImportRun>().is_running(),
            "and the menu entries are greyed from here on"
        );
        Ok(())
    }

    /// **One run at a time.** The menu greys its entries while a run is going;
    /// a request that gets through anyway is dropped rather than starting a
    /// second read whose creations would interleave with the first's on the one
    /// ordered reply queue.
    #[test]
    fn a_second_run_is_refused_while_one_is_going() {
        let mut app = run_app();
        app.world_mut().write_message(StartWindlightBulkImport {
            kind: SettingsKind::Sky,
        });
        app.update();
        let _first = app
            .world_mut()
            .resource_mut::<Messages<OpenFileDialog>>()
            .drain()
            .count();
        app.world_mut().write_message(StartWindlightBulkImport {
            kind: SettingsKind::Water,
        });
        app.update();
        assert!(
            app.world()
                .resource::<Messages<OpenFileDialog>>()
                .is_empty(),
            "no second chooser while the first run is in flight"
        );
    }

    /// A cancelled chooser ends the run silently — nothing was imported, so
    /// there is nothing to report, and the menu entries come back.
    #[test]
    fn a_cancelled_chooser_ends_the_run_quietly() {
        let mut app = run_app();
        app.world_mut().write_message(StartWindlightBulkImport {
            kind: SettingsKind::Sky,
        });
        app.update();
        app.world_mut().write_message(FileDialogClosed {
            purpose: bulk_import_purpose(SettingsKind::Sky).into(),
            outcome: FileDialogOutcome::Cancelled,
        });
        app.update();
        assert!(
            !app.world().resource::<BulkImportRun>().is_running(),
            "the run is over"
        );
        assert!(raised(&mut app).is_empty(), "and said nothing about it");
    }

    /// Somebody else's file dialog — an editor's single-preset Import — is left
    /// alone, and does not end the run waiting on its own chooser.
    #[test]
    fn another_windows_dialog_is_left_alone() {
        let mut app = run_app();
        app.world_mut().write_message(StartWindlightBulkImport {
            kind: SettingsKind::Sky,
        });
        app.update();
        app.world_mut().write_message(FileDialogClosed {
            purpose: "import-sky".into(),
            outcome: FileDialogOutcome::Picked(PathBuf::from("/presets/skies/A.xml")),
        });
        app.update();
        assert!(
            app.world().resource::<BulkImportRun>().is_running(),
            "still waiting on its own chooser"
        );
    }

    /// **A run that stops hearing back is ended, not left hanging.** The
    /// watchdog reports how many items were never confirmed and frees the menu,
    /// rather than greying Bulk Import for the rest of the session.
    #[test]
    fn a_run_that_goes_quiet_is_expired_with_a_report() -> Result<(), TestError> {
        let mut app = run_app();
        app.world_mut().resource_mut::<BulkImportRun>().run = Some(RunState {
            kind: SettingsKind::Sky,
            phase: RunPhase::Filing {
                outstanding: 3,
                // One tick short of the timeout, so a single frame crosses it
                // whatever that frame's delta happens to be.
                quiet: BULK_IMPORT_REPLY_TIMEOUT,
            },
            failed: Vec::new(),
            filed: 7,
        });
        app.update();
        assert!(
            !app.world().resource::<BulkImportRun>().is_running(),
            "the run is over"
        );
        let notifications = raised(&mut app);
        let first = notifications.first().ok_or("it reported")?;
        assert_eq!(
            first.template, "WindlightBulkImportSummary",
            "the summary, because something did not land"
        );
        Ok(())
    }

    /// A run where every file worked raises the reference's own tip, and names
    /// no failures because there were none.
    #[test]
    fn a_clean_run_raises_the_reference_tip() -> Result<(), TestError> {
        let mut app = run_app();
        app.world_mut().resource_mut::<BulkImportRun>().run = Some(RunState {
            kind: SettingsKind::Sky,
            phase: RunPhase::Filing {
                outstanding: 1,
                quiet: 0.0,
            },
            failed: Vec::new(),
            filed: 4,
        });
        app.world_mut()
            .write_message(sl_viewer_world_api::SettingsItemCreated {
                item: sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::nil()),
                folder: sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::nil()),
                kind: SettingsKind::Sky,
                authored: true,
            });
        app.update();
        assert!(
            !app.world().resource::<BulkImportRun>().is_running(),
            "the last item landed, so the run is done"
        );
        let notifications = raised(&mut app);
        let first = notifications.first().ok_or("it reported")?;
        assert_eq!(
            first.template, "WindlightBulkImportFinished",
            "the reference's own tip"
        );
        Ok(())
    }

    /// A summary names the files that failed, and stops naming them once the
    /// list would be a log rather than a notification.
    #[test]
    fn a_summary_names_the_failures_up_to_a_limit() -> Result<(), TestError> {
        let mut app = run_app();
        let failed: Vec<BulkFailure> = (0..12)
            .map(|index| BulkFailure {
                file: format!("Broken{index}.xml"),
                reason: "not a preset".to_owned(),
            })
            .collect();
        app.world_mut().resource_mut::<BulkImportRun>().run = Some(RunState {
            kind: SettingsKind::Sky,
            phase: RunPhase::Filing {
                outstanding: 1,
                quiet: BULK_IMPORT_REPLY_TIMEOUT,
            },
            failed,
            filed: 0,
        });
        app.update();
        let notifications = raised(&mut app);
        let first = notifications.first().ok_or("it reported")?;
        let arg = |key: &str| {
            first
                .args
                .pairs()
                .iter()
                .find(|(name, _value)| name == key)
                .map(|(_name, value)| value.clone())
                .unwrap_or_default()
        };
        let reasons = arg("REASONS");
        assert!(
            reasons.contains("Broken0.xml") && reasons.contains("Broken7.xml"),
            "the first eight are named: {reasons}"
        );
        assert!(
            !reasons.contains("Broken8.xml"),
            "the ninth is not: {reasons}"
        );
        assert_eq!(
            arg("FAILED"),
            "13",
            "but the count includes every one of them, the watchdog's line too"
        );
        Ok(())
    }

    /// A folder with nothing in it converts nothing and fails nothing — the
    /// run's own report is what says so.
    #[test]
    fn an_empty_folder_converts_nothing() -> Result<(), TestError> {
        let dir = tempdir("empty")?;
        assert_eq!(
            convert_folder(SettingsKind::Sky, &dir),
            BulkConversion::default(),
            "an empty folder is not a failure, it is an empty folder"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }
}
