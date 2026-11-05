//! The About Region **Terrain** tab's actions that leave the form: the RAW
//! heightmap download and upload, the confirmed **Bake Terrain**, and the
//! elevation-range check an **Apply** asks before it sends.
//!
//! Each of them waits on something outside the frame — a file chooser, or a
//! modal confirmation — so the floater ([`crate::about_region`]) only *asks*,
//! with a `RegionTerrainRequest`, and this module carries the request to its
//! end. That keeps the waiting state in one place and out of a window that may
//! be closed while the chooser is still up.
//!
//! # Tied to the region that was asked about
//!
//! Every request names the region the window was about when the button was
//! pressed, and nothing is sent unless the agent is **still** in it when the
//! answer comes. A file chooser can stay up for a minute, and a teleport in that
//! minute would otherwise upload a heightmap over the region the agent landed
//! in — every message here goes out on the current circuit, whatever region the
//! window shows.
//!
//! # The name the grid is told
//!
//! The reference sends the picked file's **full local path** as the transfer's
//! file name, and the simulator keys the transfer on it (OpenSim also chooses
//! the heightmap loader from its extension, so the name has to end in `.raw`).
//! Here the grid is told only the file's own name — the extension survives, the
//! user's directory layout does not leave the machine — and the full path is
//! kept locally, keyed by that name, until the download it belongs to arrives.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use sl_client_bevy::{
    Command, RegionTerrainUpdate, SlCommand, SlCurrentRegion, SlEvent, SlRegionIdentity,
    SlSessionEvent, TerrainMaterialOverrides, Uuid,
};
use sl_settings::{Scope, SettingValue};
use sl_viewer_platform::file_dialog::{
    FileDialogClosed, FileDialogFilter, FileDialogOutcome, FileDialogSelection, OpenFileDialog,
};
use sl_viewer_settings::ViewerSettings;

use crate::i18n::Translator;
use crate::notifications::{
    NotificationResponse, OkCancel, ShowNotification, TemplateRef, YesNoCancel,
};

/// The file-dialog purpose of the RAW **download**'s save chooser.
const DOWNLOAD_PURPOSE: &str = "about-region-terrain-raw-download";

/// The file-dialog purpose of the RAW **upload**'s open chooser.
const UPLOAD_PURPOSE: &str = "about-region-terrain-raw-upload";

/// The name the download's save chooser suggests, as the reference's does.
const DOWNLOAD_FILE_NAME: &str = "terrain.raw";

/// The extension both choosers filter on (the reference's `FFLOAD_RAW` /
/// `FFSAVE_RAW`).
const RAW_EXTENSION: &str = "raw";

/// The confirmation a **Bake Terrain** asks first.
const CONFIRM_BAKE: TemplateRef<OkCancel> = TemplateRef::new("ConfirmBakeTerrain");

/// The confirmation an **Apply** asks when a corner's low elevation is above
/// its high one; its third button is "don't ask me again".
const CONFIRM_TEXTURE_HEIGHTS: TemplateRef<YesNoCancel> = TemplateRef::new("ConfirmTextureHeights");

/// Whether an **Apply** checks the elevation ranges and asks before sending a
/// corner whose low value is above its high one — the reference's
/// `RegionCheckTextureHeights`, which the confirmation's "don't ask" turns off.
pub const SETTING_CHECK_TEXTURE_HEIGHTS: &str = "RegionCheckTextureHeights";

/// The settings section the region floater's preferences live in.
pub const REGION_SECTION: &[&str] = &["region"];

/// Declare the Terrain tab's one preference.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        REGION_SECTION,
        SETTING_CHECK_TEXTURE_HEIGHTS,
        SettingValue::Bool(true),
        "setting-desc-RegionCheckTextureHeights",
    );
}

/// What a Terrain tab button asks for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TerrainRequest {
    /// Save the region's heightmap to a file the user names.
    DownloadRaw,
    /// Replace the region's heightmap with a file the user picks.
    UploadRaw,
    /// Make the heightmap as it stands the region's revert baseline, once the
    /// user confirms.
    Bake,
    /// Send this terrain draft, once its elevation ranges pass (or the user
    /// waves them through).
    Apply(Box<TerrainApply>),
}

/// What an Apply sends.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TerrainApply {
    /// The estate messages' terrain draft.
    pub update: RegionTerrainUpdate,
    /// The PBR materials' transforms to write over `ModifyRegion` after the
    /// estate messages, when the tab is on materials and the region grants the
    /// capability.
    pub overrides: Option<TerrainMaterialOverrides>,
}

impl TerrainRequest {
    /// An Apply of `update`, then `overrides`.
    pub(crate) fn apply(
        update: RegionTerrainUpdate,
        overrides: Option<TerrainMaterialOverrides>,
    ) -> Self {
        Self::Apply(Box::new(TerrainApply { update, overrides }))
    }
}

/// A Terrain tab button's request, about the region its window shows.
#[derive(Message, Debug, Clone, PartialEq)]
pub(crate) struct RegionTerrainRequest {
    /// The region the window was about when the button was pressed.
    pub region: Uuid,
    /// What it asks for.
    pub request: TerrainRequest,
}

/// Which way a RAW transfer runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RawDirection {
    /// From the region to a file.
    Download,
    /// From a file to the region.
    Upload,
}

impl RawDirection {
    /// The file-dialog purpose this direction's chooser is asked under.
    const fn purpose(self) -> &'static str {
        match self {
            Self::Download => DOWNLOAD_PURPOSE,
            Self::Upload => UPLOAD_PURPOSE,
        }
    }

    /// The direction a dialog purpose belongs to, if it is one of these.
    fn from_purpose(purpose: &str) -> Option<Self> {
        [Self::Download, Self::Upload]
            .into_iter()
            .find(|direction| direction.purpose() == purpose)
    }
}

/// A confirmation on screen, and what it would send.
#[derive(Debug, Clone, PartialEq)]
enum PendingConfirm {
    /// The Bake Terrain confirmation.
    Bake,
    /// The elevation-range confirmation, holding what its Apply would send.
    Apply(Box<TerrainApply>),
}

/// The Terrain tab's requests in flight.
#[derive(Resource, Debug, Default)]
pub(crate) struct TerrainActions {
    /// The region each open file chooser was asked about.
    choosers: HashMap<RawDirection, Uuid>,
    /// The confirmation on screen and the region it was asked about. One slot,
    /// as the confirmations are modal: a second cannot be raised over the first.
    confirm: Option<(Uuid, PendingConfirm)>,
    /// The downloads asked for and not yet arrived: the full local path each is
    /// to be written to, by the file name the grid was told.
    downloads: HashMap<String, PathBuf>,
}

/// The name the grid is told for a local file: its own file name, without the
/// directories above it. `None` for a path with no file name (a root, or one
/// ending in `..`), which no chooser answers with.
fn grid_file_name(path: &Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
}

/// Whether every corner's low elevation is at or below its high one — the
/// reference's `validateTextureHeights`, which compares the two spinners of
/// each corner.
fn texture_heights_valid(update: &RegionTerrainUpdate) -> bool {
    update
        .start_heights
        .iter()
        .zip(update.height_ranges.iter())
        .all(|(low, high)| low <= high)
}

/// The chooser a RAW transfer asks for.
fn raw_dialog(direction: RawDirection, translator: &Translator) -> OpenFileDialog {
    let (title, selection) = match direction {
        RawDirection::Download => (
            "about-region-terrain-download-raw-title",
            FileDialogSelection::Save {
                file_name: DOWNLOAD_FILE_NAME.into(),
            },
        ),
        RawDirection::Upload => (
            "about-region-terrain-upload-raw-title",
            FileDialogSelection::File,
        ),
    };
    OpenFileDialog {
        purpose: direction.purpose().into(),
        title: translator.get(title),
        filters: vec![FileDialogFilter {
            label: translator.get("about-region-terrain-raw-filter"),
            extensions: vec![RAW_EXTENSION.to_owned()],
        }],
        start_dir: None,
        selection,
    }
}

/// The notice for a file that could not be opened, read or written: one of the
/// catalogue's `GenericFile…Error` templates, naming the file and the error.
fn file_error_notice(
    template: &'static str,
    path: &Path,
    error: &std::io::Error,
) -> ShowNotification {
    ShowNotification::new(template)
        .arg("FILE_NAME", path.display().to_string())
        .arg("ERROR_MESSAGE", error.to_string())
        .arg("ERROR_CODE", error.raw_os_error().unwrap_or(0).to_string())
}

/// The region the agent is in, if any.
fn current_region(regions: &Query<&SlRegionIdentity, With<SlCurrentRegion>>) -> Option<Uuid> {
    regions.iter().next().map(|identity| identity.0.region_id)
}

/// Whether the elevation-range check is on (the reference's default is on, so
/// an unregistered setting reads as on too).
fn checks_texture_heights(settings: Option<&ViewerSettings>) -> bool {
    settings.is_none_or(|settings| {
        settings
            .store()
            .get_bool(SETTING_CHECK_TEXTURE_HEIGHTS)
            .unwrap_or(true)
    })
}

/// Start each Terrain tab request: put up its chooser or its confirmation, or —
/// for an Apply whose ranges pass — send it.
fn start_terrain_requests(
    mut requests: MessageReader<RegionTerrainRequest>,
    mut actions: ResMut<TerrainActions>,
    mut dialogs: MessageWriter<OpenFileDialog>,
    mut notifications: MessageWriter<ShowNotification>,
    mut sl_commands: MessageWriter<SlCommand>,
    settings: Option<Res<ViewerSettings>>,
    translator: Translator,
) {
    for RegionTerrainRequest { region, request } in requests.read() {
        match request {
            TerrainRequest::DownloadRaw | TerrainRequest::UploadRaw => {
                let direction = if *request == TerrainRequest::DownloadRaw {
                    RawDirection::Download
                } else {
                    RawDirection::Upload
                };
                let _previous = actions.choosers.insert(direction, *region);
                dialogs.write(raw_dialog(direction, &translator));
            }
            TerrainRequest::Bake => {
                actions.confirm = Some((*region, PendingConfirm::Bake));
                notifications.write(ShowNotification::new(CONFIRM_BAKE.name()));
            }
            TerrainRequest::Apply(apply) => {
                if checks_texture_heights(settings.as_deref())
                    && !texture_heights_valid(&apply.update)
                {
                    actions.confirm = Some((*region, PendingConfirm::Apply(apply.clone())));
                    notifications.write(ShowNotification::new(CONFIRM_TEXTURE_HEIGHTS.name()));
                } else {
                    for command in apply_commands((**apply).clone()) {
                        sl_commands.write(SlCommand(command));
                    }
                }
            }
        }
    }
}

/// Act on an answered Bake or elevation-range confirmation — in the region it
/// was asked about, and only while the agent is still there.
fn answer_terrain_confirms(
    mut responses: MessageReader<NotificationResponse>,
    mut actions: ResMut<TerrainActions>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    for response in responses.read() {
        if !response.is_for(CONFIRM_BAKE) && !response.is_for(CONFIRM_TEXTURE_HEIGHTS) {
            continue;
        }
        let Some((region, pending)) = actions.confirm.take() else {
            continue;
        };
        let send = match pending {
            PendingConfirm::Bake => (response.answer(CONFIRM_BAKE) == Some(OkCancel::Ok))
                .then(|| vec![Command::BakeRegionTerrain]),
            PendingConfirm::Apply(apply) => {
                match response.answer(CONFIRM_TEXTURE_HEIGHTS) {
                    Some(YesNoCancel::Yes) => Some(apply_commands(*apply)),
                    // The third button: send it, and stop asking.
                    Some(YesNoCancel::Cancel) => {
                        if let Some(settings) = settings.as_deref_mut() {
                            settings.set(
                                Scope::Global,
                                SETTING_CHECK_TEXTURE_HEIGHTS,
                                SettingValue::Bool(false),
                            );
                        }
                        Some(apply_commands(*apply))
                    }
                    Some(YesNoCancel::No) | None => None,
                }
            }
        };
        let Some(commands) = send else {
            continue;
        };
        if current_region(&regions) != Some(region) {
            warn!(
                region = %region,
                "the agent left the region a terrain confirmation was about; nothing sent"
            );
            continue;
        }
        for command in commands {
            sl_commands.write(SlCommand(command));
        }
    }
}

/// The commands an Apply sends: the four estate messages, then — on materials,
/// where the region grants it — the `ModifyRegion` write, in the reference's
/// order.
fn apply_commands(TerrainApply { update, overrides }: TerrainApply) -> Vec<Command> {
    let mut commands = vec![Command::SetRegionTerrain(update)];
    if let Some(overrides) = overrides {
        commands.push(Command::SetTerrainMaterialOverrides(Box::new(overrides)));
    }
    commands
}

/// Take a RAW chooser's answer: ask the region for its heightmap, or read the
/// picked file and send it up.
fn finish_raw_choosers(
    mut closed: MessageReader<FileDialogClosed>,
    mut actions: ResMut<TerrainActions>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for reply in closed.read() {
        let Some(direction) = RawDirection::from_purpose(&reply.purpose) else {
            // Somebody else's dialog.
            continue;
        };
        let Some(region) = actions.choosers.remove(&direction) else {
            continue;
        };
        let FileDialogOutcome::Picked(ref path) = reply.outcome else {
            // Cancelled, or refused because another chooser was up.
            continue;
        };
        if current_region(&regions) != Some(region) {
            warn!(
                region = %region,
                path = %path.display(),
                "the agent left the region a terrain RAW transfer was about; nothing sent"
            );
            continue;
        }
        let Some(viewer_filename) = grid_file_name(path) else {
            error!(path = %path.display(), "a file chooser answered with no file name");
            continue;
        };
        match direction {
            RawDirection::Download => {
                let _previous = actions
                    .downloads
                    .insert(viewer_filename.clone(), path.clone());
                sl_commands.write(SlCommand(Command::RequestRegionTerrainDownload {
                    viewer_filename,
                }));
            }
            RawDirection::Upload => {
                // On the frame thread, as the reference reads it: a region's
                // RAW is under a megabyte (256 × 256 × 13 bytes), and the slow
                // part — the user choosing — is already over.
                let data = match fs_err::read(path) {
                    Ok(data) => data,
                    Err(error) => {
                        notifications.write(file_error_notice(
                            "GenericFileOpenReadError",
                            path,
                            &error,
                        ));
                        continue;
                    }
                };
                if data.is_empty() {
                    notifications.write(
                        ShowNotification::new("GenericFileEmptyError")
                            .arg("FILE_NAME", path.display().to_string())
                            .arg("ERROR_MESSAGE", "")
                            .arg("ERROR_CODE", "0"),
                    );
                    continue;
                }
                sl_commands.write(SlCommand(Command::RequestRegionTerrainUpload {
                    viewer_filename,
                    data,
                }));
                notifications.write(ShowNotification::new("RawUploadStarted"));
            }
        }
    }
}

/// Write each downloaded heightmap to the file it was asked for, and say so.
fn save_raw_downloads(
    mut events: MessageReader<SlEvent>,
    mut actions: ResMut<TerrainActions>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for event in events.read() {
        let SlSessionEvent::ServerFileDownloaded {
            viewer_filename,
            data,
        } = &event.0
        else {
            continue;
        };
        // A download someone else asked for (a script console, a test): not
        // this window's to write anywhere.
        let Some(path) = actions.downloads.remove(viewer_filename) else {
            continue;
        };
        match fs_err::write(&path, data) {
            Ok(()) => {
                notifications.write(
                    ShowNotification::new("FinishedRawDownload")
                        .arg("DOWNLOAD_PATH", path.display().to_string()),
                );
            }
            Err(error) => {
                notifications.write(file_error_notice(
                    "GenericFileOpenWriteError",
                    &path,
                    &error,
                ));
            }
        }
    }
}

/// Wires the Terrain tab's actions; added by the About Region plugin.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RegionTerrainActionsPlugin;

impl Plugin for RegionTerrainActionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainActions>()
            .add_message::<RegionTerrainRequest>()
            .add_message::<OpenFileDialog>()
            .add_message::<FileDialogClosed>()
            .add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .add_systems(
                Update,
                (
                    start_terrain_requests,
                    answer_terrain_confirms,
                    finish_raw_choosers,
                    save_raw_downloads,
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
    use sl_client_bevy::{
        Command, GridCoordinates, Maturity, ProductType, RegionFlags, RegionHandle, RegionIdentity,
        RegionTerrainComposition, RegionTerrainUpdate, SlCommand, SlCurrentRegion, SlEvent,
        SlRegionIdentity, SlSessionEvent, TerrainMaterialOverrides, Uuid,
    };
    use sl_settings::{Scope, SettingValue, SettingsStore};
    use sl_viewer_platform::file_dialog::{
        FileDialogClosed, FileDialogOutcome, FileDialogSelection, OpenFileDialog,
    };
    use sl_viewer_settings::ViewerSettings;

    use super::{
        CONFIRM_BAKE, CONFIRM_TEXTURE_HEIGHTS, DOWNLOAD_PURPOSE, RegionTerrainActionsPlugin,
        RegionTerrainRequest, SETTING_CHECK_TEXTURE_HEIGHTS, TerrainRequest, UPLOAD_PURPOSE,
        grid_file_name, register_settings, texture_heights_valid,
    };
    use crate::notifications::{
        NotificationManager, NotificationResponse, OkCancel, ShowNotification, YesNoCancel,
    };

    /// A boxed error so tests use `?` rather than the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The region the agent stands in throughout.
    const HERE: Uuid = Uuid::from_u128(0xA1);

    /// A unique throwaway directory under the system temp dir (the crate has no
    /// `tempfile` dependency; the same helper the settings editor's tests use).
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

    /// An app with the plugin, the agent standing in [`HERE`], and the
    /// elevation-range preference registered at its default.
    fn app() -> App {
        let mut app = App::new();
        app.add_message::<SlCommand>()
            .add_message::<SlEvent>()
            .add_plugins((MinimalPlugins, RegionTerrainActionsPlugin));
        crate::i18n::install_untranslated(&mut app);
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        app.insert_resource(settings);
        stand_in(&mut app, HERE);
        app
    }

    /// Make `region` the one the agent is in, replacing whichever was.
    fn stand_in(app: &mut App, region: Uuid) {
        let live: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<SlCurrentRegion>>()
            .iter(app.world())
            .collect();
        for entity in live {
            app.world_mut().entity_mut(entity).despawn();
        }
        let identity = RegionIdentity {
            sim_name: None,
            region_id: region,
            region_handle: RegionHandle::new(0),
            grid_coordinates: GridCoordinates::new(1000, 1000),
            region_flags: RegionFlags::default(),
            region_flags_extended: None,
            region_protocols: 0,
            maturity: Maturity::Pg,
            product: ProductType::Unknown,
            product_sku: String::new(),
            product_name: String::new(),
            colo_name: String::new(),
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
        };
        app.world_mut()
            .spawn((SlCurrentRegion, SlRegionIdentity(identity)));
    }

    /// Ask for `request` about [`HERE`] and run a frame.
    fn ask(app: &mut App, request: TerrainRequest) {
        app.world_mut().write_message(RegionTerrainRequest {
            region: HERE,
            request,
        });
        app.update();
    }

    /// Answer the chooser asked under `purpose` with `outcome` and run a frame.
    fn answer_chooser(app: &mut App, purpose: &str, outcome: FileDialogOutcome) {
        app.world_mut().write_message(FileDialogClosed {
            purpose: purpose.into(),
            outcome,
        });
        app.update();
    }

    /// Answer the confirmation `template` with the button `button`.
    fn answer_confirm(app: &mut App, template: &'static str, button: &'static str) {
        let id = NotificationManager::default().allocate_id();
        app.world_mut().write_message(NotificationResponse {
            id,
            template,
            button: Some(button.into()),
            ignored: false,
            input: None,
        });
        app.update();
    }

    /// Every message of type `M` written so far, drained.
    fn drain<M: Message>(app: &mut App) -> Vec<M> {
        app.world_mut()
            .resource_mut::<Messages<M>>()
            .drain()
            .collect()
    }

    /// What a terrain action sent, in a form a test can compare ([`Command`]
    /// is not `PartialEq`).
    #[derive(Debug, PartialEq)]
    enum Sent {
        /// `RequestRegionTerrainDownload`, by the name the grid was told.
        Download(String),
        /// `RequestRegionTerrainUpload`: the name and the bytes.
        Upload(String, Vec<u8>),
        /// `BakeRegionTerrain`.
        Bake,
        /// `SetRegionTerrain`.
        Apply(RegionTerrainUpdate),
        /// `SetTerrainMaterialOverrides`.
        Overrides(TerrainMaterialOverrides),
        /// Anything else, by its debug form.
        Other(String),
    }

    /// Every command written so far.
    fn commands(app: &mut App) -> Vec<Sent> {
        drain::<SlCommand>(app)
            .into_iter()
            .map(|command| match command.0 {
                Command::RequestRegionTerrainDownload { viewer_filename } => {
                    Sent::Download(viewer_filename)
                }
                Command::RequestRegionTerrainUpload {
                    viewer_filename,
                    data,
                } => Sent::Upload(viewer_filename, data),
                Command::BakeRegionTerrain => Sent::Bake,
                Command::SetRegionTerrain(update) => Sent::Apply(update),
                Command::SetTerrainMaterialOverrides(overrides) => Sent::Overrides(*overrides),
                other => Sent::Other(format!("{other:?}")),
            })
            .collect()
    }

    /// The templates of every notice raised so far.
    fn notices(app: &mut App) -> Vec<&'static str> {
        drain::<ShowNotification>(app)
            .into_iter()
            .map(|notice| notice.template)
            .collect()
    }

    /// A terrain draft whose corners are as given: `(low, high)` each.
    fn draft(corners: [(f32, f32); 4]) -> RegionTerrainUpdate {
        RegionTerrainUpdate {
            start_heights: corners.map(|(low, _high)| low),
            height_ranges: corners.map(|(_low, high)| high),
            ..RegionTerrainUpdate::default()
        }
    }

    /// The grid is told the file's own name, never the directories above it.
    #[test]
    fn the_grid_hears_only_the_file_name() {
        assert_eq!(
            grid_file_name(Path::new("/home/someone/islands/north.raw")),
            Some("north.raw".to_owned())
        );
        assert_eq!(grid_file_name(Path::new("/")), None, "a root has no name");
    }

    /// A corner is valid while its low value is at or below its high one.
    #[test]
    fn texture_heights_are_checked_per_corner() {
        assert!(texture_heights_valid(&draft([(10.0, 60.0); 4])));
        assert!(
            texture_heights_valid(&draft([(20.0, 20.0); 4])),
            "equal is fine"
        );
        assert!(!texture_heights_valid(&draft([
            (10.0, 60.0),
            (10.0, 60.0),
            (70.0, 60.0),
            (10.0, 60.0),
        ])));
    }

    /// Download: a save chooser offering `terrain.raw`, the request naming the
    /// picked file's own name, and the arrived bytes written to the full path.
    #[test]
    fn a_download_is_saved_where_the_user_chose() -> Result<(), TestError> {
        let mut app = app();
        ask(&mut app, TerrainRequest::DownloadRaw);
        let dialogs = drain::<OpenFileDialog>(&mut app);
        let dialog = dialogs.first().ok_or("a chooser is asked for")?;
        assert_eq!(&*dialog.purpose, DOWNLOAD_PURPOSE);
        assert_eq!(
            dialog.selection,
            FileDialogSelection::Save {
                file_name: "terrain.raw".into()
            },
            "a save chooser suggesting the reference's name"
        );

        let dir = tempdir("download")?;
        let path = dir.join("island.raw");
        answer_chooser(
            &mut app,
            DOWNLOAD_PURPOSE,
            FileDialogOutcome::Picked(path.clone()),
        );
        assert_eq!(
            commands(&mut app),
            vec![Sent::Download("island.raw".to_owned())]
        );

        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::ServerFileDownloaded {
                viewer_filename: "island.raw".to_owned(),
                data: vec![1, 2, 3, 4],
            }));
        app.update();
        assert_eq!(fs_err::read(&path)?, vec![1, 2, 3, 4], "written in full");
        let raised = drain::<ShowNotification>(&mut app);
        let finished = raised.first().ok_or("the download is announced")?;
        assert_eq!(finished.template, "FinishedRawDownload");
        assert_eq!(
            finished.args.pairs(),
            [("DOWNLOAD_PATH".to_owned(), path.display().to_string())],
            "naming where it went"
        );
        fs_err::remove_dir_all(dir)?;
        Ok(())
    }

    /// A downloaded file nobody here asked for is not written anywhere.
    #[test]
    fn a_download_nobody_asked_for_is_left_alone() {
        let mut app = app();
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::ServerFileDownloaded {
                viewer_filename: "terrain.raw".to_owned(),
                data: vec![9],
            }));
        app.update();
        assert!(notices(&mut app).is_empty(), "nothing to announce");
    }

    /// Upload: an open chooser, the picked file read and sent under its own
    /// name, and the reference's "upload started" notice.
    #[test]
    fn an_upload_sends_the_picked_file() -> Result<(), TestError> {
        let mut app = app();
        ask(&mut app, TerrainRequest::UploadRaw);
        let dialogs = drain::<OpenFileDialog>(&mut app);
        let dialog = dialogs.first().ok_or("a chooser is asked for")?;
        assert_eq!(&*dialog.purpose, UPLOAD_PURPOSE);
        assert_eq!(dialog.selection, FileDialogSelection::File);

        let dir = tempdir("upload")?;
        let path = dir.join("hills.raw");
        fs_err::write(&path, [7_u8, 8, 9])?;
        answer_chooser(&mut app, UPLOAD_PURPOSE, FileDialogOutcome::Picked(path));
        assert_eq!(
            commands(&mut app),
            vec![Sent::Upload("hills.raw".to_owned(), vec![7, 8, 9])]
        );
        assert_eq!(notices(&mut app), vec!["RawUploadStarted"]);
        fs_err::remove_dir_all(dir)?;
        Ok(())
    }

    /// A file that cannot be read is said so, and nothing is sent.
    #[test]
    fn an_unreadable_upload_is_reported_not_sent() -> Result<(), TestError> {
        let mut app = app();
        ask(&mut app, TerrainRequest::UploadRaw);
        let dir = tempdir("missing")?;
        answer_chooser(
            &mut app,
            UPLOAD_PURPOSE,
            FileDialogOutcome::Picked(dir.join("gone.raw")),
        );
        assert!(commands(&mut app).is_empty(), "nothing to send");
        assert_eq!(notices(&mut app), vec!["GenericFileOpenReadError"]);

        // And an empty one is refused rather than flattening the region.
        ask(&mut app, TerrainRequest::UploadRaw);
        let empty = dir.join("empty.raw");
        fs_err::write(&empty, [])?;
        answer_chooser(&mut app, UPLOAD_PURPOSE, FileDialogOutcome::Picked(empty));
        assert!(commands(&mut app).is_empty(), "nothing to send");
        assert_eq!(notices(&mut app), vec!["GenericFileEmptyError"]);
        fs_err::remove_dir_all(dir)?;
        Ok(())
    }

    /// A chooser answered after a teleport sends nothing: the transfer would
    /// land on the region the agent is in now, not the one that was asked about.
    #[test]
    fn a_chooser_answered_elsewhere_sends_nothing() -> Result<(), TestError> {
        let mut app = app();
        ask(&mut app, TerrainRequest::UploadRaw);
        stand_in(&mut app, Uuid::from_u128(0xB2));
        let dir = tempdir("elsewhere")?;
        let path = dir.join("hills.raw");
        fs_err::write(&path, [1_u8])?;
        answer_chooser(&mut app, UPLOAD_PURPOSE, FileDialogOutcome::Picked(path));
        assert!(
            commands(&mut app).is_empty(),
            "nothing sent to the wrong region"
        );
        fs_err::remove_dir_all(dir)?;
        Ok(())
    }

    /// A cancelled chooser sends nothing.
    #[test]
    fn a_cancelled_chooser_sends_nothing() {
        let mut app = app();
        ask(&mut app, TerrainRequest::DownloadRaw);
        answer_chooser(&mut app, DOWNLOAD_PURPOSE, FileDialogOutcome::Cancelled);
        assert!(commands(&mut app).is_empty());
    }

    /// Bake asks first, and only OK sends it.
    #[test]
    fn bake_waits_for_ok() {
        let mut app = app();
        ask(&mut app, TerrainRequest::Bake);
        assert!(commands(&mut app).is_empty(), "nothing before the answer");
        assert_eq!(notices(&mut app), vec![CONFIRM_BAKE.name()]);
        answer_confirm(&mut app, CONFIRM_BAKE.name(), OkCancel::Cancel.name());
        assert!(commands(&mut app).is_empty(), "Cancel sends nothing");

        ask(&mut app, TerrainRequest::Bake);
        let _asked = notices(&mut app);
        answer_confirm(&mut app, CONFIRM_BAKE.name(), OkCancel::Ok.name());
        assert_eq!(commands(&mut app), vec![Sent::Bake]);
    }

    /// A Bake confirmed after leaving the region is not sent to the new one.
    #[test]
    fn bake_confirmed_elsewhere_sends_nothing() {
        let mut app = app();
        ask(&mut app, TerrainRequest::Bake);
        stand_in(&mut app, Uuid::from_u128(0xB2));
        answer_confirm(&mut app, CONFIRM_BAKE.name(), OkCancel::Ok.name());
        assert!(commands(&mut app).is_empty());
    }

    /// Ranges that pass are sent at once, with no question.
    #[test]
    fn valid_ranges_apply_at_once() {
        let mut app = app();
        let update = draft([(10.0, 60.0); 4]);
        ask(&mut app, TerrainRequest::apply(update.clone(), None));
        assert_eq!(commands(&mut app), vec![Sent::Apply(update)]);
        assert!(notices(&mut app).is_empty());
    }

    /// A low above its high asks; Cancel keeps it, OK sends it, and "don't
    /// ask" sends it and stops asking.
    #[test]
    fn inverted_ranges_ask_before_applying() -> Result<(), TestError> {
        let mut app = app();
        let update = draft([(70.0, 60.0); 4]);
        ask(&mut app, TerrainRequest::apply(update.clone(), None));
        assert!(commands(&mut app).is_empty(), "held for the answer");
        assert_eq!(notices(&mut app), vec![CONFIRM_TEXTURE_HEIGHTS.name()]);
        answer_confirm(
            &mut app,
            CONFIRM_TEXTURE_HEIGHTS.name(),
            YesNoCancel::No.name(),
        );
        assert!(commands(&mut app).is_empty(), "Cancel keeps it");

        ask(&mut app, TerrainRequest::apply(update.clone(), None));
        let _asked = notices(&mut app);
        answer_confirm(
            &mut app,
            CONFIRM_TEXTURE_HEIGHTS.name(),
            YesNoCancel::Yes.name(),
        );
        assert_eq!(
            commands(&mut app),
            vec![Sent::Apply(update.clone())],
            "OK sends it"
        );

        ask(&mut app, TerrainRequest::apply(update.clone(), None));
        let _asked = notices(&mut app);
        answer_confirm(
            &mut app,
            CONFIRM_TEXTURE_HEIGHTS.name(),
            YesNoCancel::Cancel.name(),
        );
        assert_eq!(
            commands(&mut app),
            vec![Sent::Apply(update.clone())],
            "don't-ask sends it"
        );
        assert_eq!(
            app.world()
                .resource::<ViewerSettings>()
                .store()
                .get_bool(SETTING_CHECK_TEXTURE_HEIGHTS)?,
            false,
            "and turns the check off"
        );
        ask(&mut app, TerrainRequest::apply(update.clone(), None));
        assert_eq!(
            commands(&mut app),
            vec![Sent::Apply(update)],
            "so the next one goes straight out"
        );
        assert!(notices(&mut app).is_empty());
        Ok(())
    }

    /// On materials, the overrides go out after the estate messages — and wait
    /// with them when the ranges ask first.
    #[test]
    fn overrides_follow_the_estate_messages() {
        let mut app = app();
        let update = draft([(70.0, 60.0); 4]);
        let overrides = TerrainMaterialOverrides::default();
        ask(
            &mut app,
            TerrainRequest::apply(update.clone(), Some(overrides.clone())),
        );
        assert!(commands(&mut app).is_empty(), "held for the answer");
        let _asked = notices(&mut app);
        answer_confirm(
            &mut app,
            CONFIRM_TEXTURE_HEIGHTS.name(),
            YesNoCancel::Yes.name(),
        );
        assert_eq!(
            commands(&mut app),
            vec![Sent::Apply(update), Sent::Overrides(overrides)]
        );
    }

    /// With the check turned off by hand, inverted ranges go straight out.
    #[test]
    fn the_check_can_be_off_from_the_start() {
        let mut app = app();
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_CHECK_TEXTURE_HEIGHTS,
            SettingValue::Bool(false),
        );
        let update = draft([(70.0, 60.0); 4]);
        ask(&mut app, TerrainRequest::apply(update.clone(), None));
        assert_eq!(commands(&mut app), vec![Sent::Apply(update)]);
    }
}
