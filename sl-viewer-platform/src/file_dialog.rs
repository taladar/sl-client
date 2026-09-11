//! Asking the **desktop** to pick a file — or a folder: the viewer's one
//! file-open dialog.
//!
//! Every "… from disk" the viewer will ever grow — importing a legacy WindLight
//! preset, uploading a texture or a mesh, loading a notecard — needs the host's
//! own file chooser, and under a confined Wayland session an application cannot
//! draw one itself: it has to go through `org.freedesktop.portal.FileChooser`
//! and let the compositor's picker do it. That is what `rfd` does here (its
//! `xdg-portal` backend talks to the portal over D-Bus, falling back to `zenity`
//! where there is no `libdbus`, and to the platform's native dialog on Windows
//! and macOS), so this module is the portal's file half of
//! `viewer-os-portals-linux` for all three platforms at once.
//!
//! # It is a request and a reply, never a call
//!
//! The dialog is a different process, and the user may sit in it for a minute.
//! So a caller writes [`OpenFileDialog`] and, whenever the answer comes, reads
//! [`FileDialogClosed`] tagged with the [`purpose`](OpenFileDialog::purpose) it
//! asked under — the same shape as the texture picker, and for the same reason.
//! Nothing here blocks a frame: the dialog is driven on the [`IoTaskPool`], and
//! the polling system costs one non-blocking poll per frame while one is open.
//!
//! # One at a time
//!
//! A second request while a dialog is already on screen is answered
//! [`FileDialogOutcome::Busy`] rather than stacking a second chooser the user
//! did not ask for — and rather than being dropped, which would leave whoever
//! asked waiting for a reply that never came. The reference is equally
//! single-flight (`LLFilePickerReplyThread` refuses to start over a locked
//! `LLFilePicker`).
//!
//! # The chooser is not parented to the viewer's window
//!
//! `rfd` will hand the portal a parent handle so the compositor can place the
//! dialog over the window that asked, but the only way to get one out of Bevy
//! is `RawHandleWrapper::get_handle`, which is an `unsafe fn` — and this
//! workspace *forbids* `unsafe_code`. So the chooser comes up with no parent
//! window, which costs placement and modality, not function. The way to fix it
//! is a safe handle accessor upstream, not an exception here.
//!
//! # What "cancelled" covers
//!
//! `rfd` hands back "no file" for both a user who pressed Cancel and a desktop
//! with no working portal *and* no `zenity` — it logs the difference but does
//! not report it. So does [`FileDialogOutcome::Cancelled`]; a caller that needs
//! to say something to the user should say it about the file it did not get,
//! not about why.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future::poll_once};

/// A request for the host's file-open dialog.
///
/// Written by whoever wants a file; answered by exactly one
/// [`FileDialogClosed`] carrying the same [`purpose`](Self::purpose).
#[derive(Message, Debug, Clone)]
pub struct OpenFileDialog {
    /// What this dialog is *for*, and the tag its reply comes back under — the
    /// texture picker's `field` by another name.
    ///
    /// Owned rather than `&'static str` because a purpose is not always a
    /// literal: a window that can be open twice over different items names its
    /// dialogs after the item.
    ///
    /// It is also the key the last-used directory is remembered under, so the
    /// sky editor's Import reopens where the skies are and the water editor's
    /// where the water is.
    pub purpose: Box<str>,
    /// The dialog's title bar.
    pub title: String,
    /// The file-type filters offered, most specific first. Each is a label and
    /// the extensions it covers, written **without** a leading dot (`"xml"`).
    ///
    /// Ignored for [`FileDialogSelection::Folder`], which has no file types to
    /// filter.
    pub filters: Vec<FileDialogFilter>,
    /// Where to open, when nothing has been picked under this purpose yet. The
    /// remembered directory wins over this once there is one.
    pub start_dir: Option<PathBuf>,
    /// Whether the user is picking a file or a whole directory.
    pub selection: FileDialogSelection,
}

/// What a file dialog asks the user to choose.
///
/// Both answer with one [`PathBuf`], because a directory is a path like any
/// other; what differs is what the chooser lets the user point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileDialogSelection {
    /// One file (`rfd`'s `pick_file`) — the default, and what every "… from
    /// disk" wants.
    #[default]
    File,
    /// One directory (`rfd`'s `pick_folder`), for a bulk operation that works
    /// on a folder's worth of files rather than on one the user named.
    Folder,
}

/// One entry of a file-open dialog's type filter.
#[derive(Debug, Clone)]
pub struct FileDialogFilter {
    /// The human-readable label (`"WindLight preset"`).
    pub label: String,
    /// The extensions it matches, without a leading dot (`["xml"]`).
    pub extensions: Vec<String>,
}

/// The answer to an [`OpenFileDialog`], tagged with the purpose it was asked
/// under.
#[derive(Message, Debug, Clone)]
pub struct FileDialogClosed {
    /// The [`OpenFileDialog::purpose`] this answers.
    pub purpose: Box<str>,
    /// What came of it.
    pub outcome: FileDialogOutcome,
}

/// What came of a file-open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDialogOutcome {
    /// The user chose this file.
    Picked(PathBuf),
    /// No file: the user cancelled, or the desktop could not show a chooser at
    /// all — see the module documentation on why those are one answer.
    Cancelled,
    /// A dialog was already on screen, so this request was not shown. Whoever
    /// asked should leave its own state alone and let the open one finish.
    Busy,
}

/// The dialog service's state: whether one is open, and where each purpose last
/// picked from.
#[derive(Resource, Debug, Default)]
pub struct FileDialogState {
    /// Whether a chooser is on screen right now — the single-flight gate.
    open: bool,
    /// The directory each purpose last picked a file from, so the next dialog
    /// under that purpose opens where the user left off. The reference keeps
    /// one such directory for the whole viewer; one per purpose is the same
    /// idea told apart, and matters because the sky and the water presets live
    /// in sibling folders.
    ///
    /// It is the picked path's **parent** either way, so a
    /// [`FileDialogSelection::Folder`] purpose reopens looking *at* the folder
    /// it chose last time rather than inside it — which is what a second bulk
    /// import of a sibling folder wants.
    last_dir: HashMap<Box<str>, PathBuf>,
}

impl FileDialogState {
    /// Whether a chooser is on screen.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// The directory `purpose` last picked a file from.
    #[must_use]
    pub fn last_dir(&self, purpose: &str) -> Option<&Path> {
        self.last_dir.get(purpose).map(PathBuf::as_path)
    }
}

/// A chooser in flight: the off-thread dialog and the purpose its answer is
/// owed to. An entity rather than a resource field so the task is dropped (and
/// so cancelled) with the world, like every other pending task in the viewer.
#[derive(Component)]
struct FileDialogTask {
    /// The purpose the reply is tagged with.
    purpose: Box<str>,
    /// The running dialog, yielding the chosen path or `None`.
    task: Task<Option<PathBuf>>,
}

/// Hand-written: [`Task`] is not [`Debug`], and what is worth printing about a
/// pending dialog is which purpose is waiting on it.
impl core::fmt::Debug for FileDialogTask {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileDialogTask")
            .field("purpose", &self.purpose)
            .finish_non_exhaustive()
    }
}

/// Start the dialogs that were asked for this frame, refusing a second one while
/// one is open.
fn open_file_dialogs(
    mut commands: Commands,
    mut requests: MessageReader<OpenFileDialog>,
    mut closed: MessageWriter<FileDialogClosed>,
    mut state: ResMut<FileDialogState>,
) {
    for request in requests.read() {
        if state.open {
            warn!(
                "a file dialog is already open; refusing the one for {}",
                request.purpose
            );
            closed.write(FileDialogClosed {
                purpose: request.purpose.clone(),
                outcome: FileDialogOutcome::Busy,
            });
            continue;
        }
        state.open = true;
        let directory = state
            .last_dir(&request.purpose)
            .map(Path::to_path_buf)
            .or_else(|| request.start_dir.clone());
        commands.spawn(FileDialogTask {
            purpose: request.purpose.clone(),
            task: spawn_dialog(request, directory),
        });
    }
}

/// Put one chooser on screen off the frame thread, yielding the path it picked.
fn spawn_dialog(request: &OpenFileDialog, directory: Option<PathBuf>) -> Task<Option<PathBuf>> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(&request.title);
    for filter in &request.filters {
        dialog = dialog.add_filter(&filter.label, &filter.extensions);
    }
    if let Some(directory) = directory {
        dialog = dialog.set_directory(directory);
    }
    let selection = request.selection;
    IoTaskPool::get().spawn(async move {
        let picked = match selection {
            FileDialogSelection::File => dialog.pick_file().await,
            FileDialogSelection::Folder => dialog.pick_folder().await,
        };
        picked.map(|handle| handle.path().to_path_buf())
    })
}

/// Poll the open chooser; when it closes, publish the answer, remember where it
/// picked from, and reopen the gate.
fn poll_file_dialogs(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut FileDialogTask)>,
    mut closed: MessageWriter<FileDialogClosed>,
    mut state: ResMut<FileDialogState>,
) {
    for (entity, mut pending) in &mut tasks {
        let Some(picked) = block_on(poll_once(&mut pending.task)) else {
            continue;
        };
        let outcome = match picked {
            Some(path) => {
                if let Some(directory) = path.parent() {
                    state
                        .last_dir
                        .insert(pending.purpose.clone(), directory.to_path_buf());
                }
                FileDialogOutcome::Picked(path)
            }
            None => FileDialogOutcome::Cancelled,
        };
        closed.write(FileDialogClosed {
            purpose: pending.purpose.clone(),
            outcome,
        });
        state.open = false;
        commands.entity(entity).despawn();
    }
}

/// Registers the file-open dialog service: the [`OpenFileDialog`] /
/// [`FileDialogClosed`] message pair and the systems that drive one.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileDialogPlugin;

impl Plugin for FileDialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FileDialogState>()
            .add_message::<OpenFileDialog>()
            .add_message::<FileDialogClosed>()
            .add_systems(Update, (open_file_dialogs, poll_file_dialogs).chain());
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{
        FileDialogClosed, FileDialogOutcome, FileDialogPlugin, FileDialogSelection,
        FileDialogState, OpenFileDialog,
    };

    /// A boxed error, so a test can `?` rather than reach for the `panic!` the
    /// workspace's lints (rightly) forbid.
    type TestError = Box<dyn core::error::Error>;

    /// A request naming a purpose, with no filters and no starting directory.
    fn request(purpose: &str) -> OpenFileDialog {
        OpenFileDialog {
            purpose: purpose.into(),
            title: "Pick a file".to_owned(),
            filters: Vec::new(),
            start_dir: None,
            selection: FileDialogSelection::File,
        }
    }

    /// An app with the plugin scheduled, so the systems run as they do in the
    /// viewer rather than being called by hand.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, FileDialogPlugin));
        app
    }

    /// The second request while one is open is answered `Busy`, not queued and
    /// not dropped — the caller always gets exactly one reply.
    #[test]
    fn a_second_request_is_refused_while_one_is_open() -> Result<(), TestError> {
        let mut app = app();
        // Stand in for a dialog already on screen: the gate is what the second
        // request sees, and opening a real chooser in a test is not on.
        app.world_mut().resource_mut::<FileDialogState>().open = true;
        app.world_mut().write_message(request("import-sky"));
        app.update();
        let replies: Vec<FileDialogClosed> = app
            .world_mut()
            .resource_mut::<Messages<FileDialogClosed>>()
            .drain()
            .collect();
        let reply = replies.first().ok_or("exactly one reply, and this is it")?;
        assert_eq!(replies.len(), 1, "exactly one reply");
        assert_eq!(&*reply.purpose, "import-sky", "tagged back to its asker");
        assert_eq!(reply.outcome, FileDialogOutcome::Busy, "refused, not shown");
        Ok(())
    }

    /// The remembered directory is per purpose, so two editors that import from
    /// sibling folders do not drag each other back and forth.
    #[test]
    fn the_last_directory_is_remembered_per_purpose() {
        let mut app = app();
        let mut state = app.world_mut().resource_mut::<FileDialogState>();
        state
            .last_dir
            .insert("import-sky".into(), PathBuf::from("/presets/skies"));
        state
            .last_dir
            .insert("import-water".into(), PathBuf::from("/presets/water"));
        let state = app.world().resource::<FileDialogState>();
        assert_eq!(
            state.last_dir("import-sky"),
            Some(std::path::Path::new("/presets/skies")),
            "the sky editor reopens in the skies"
        );
        assert_eq!(
            state.last_dir("import-water"),
            Some(std::path::Path::new("/presets/water")),
            "and the water editor in the water"
        );
        assert_eq!(
            state.last_dir("import-day"),
            None,
            "a purpose that has never picked has nowhere to reopen"
        );
    }

    /// With nothing asked for, nothing is opened and nothing is answered.
    #[test]
    fn an_idle_frame_opens_no_dialog() {
        let mut app = app();
        app.update();
        assert!(
            !app.world().resource::<FileDialogState>().is_open(),
            "no chooser without a request"
        );
        assert!(
            app.world()
                .resource::<Messages<FileDialogClosed>>()
                .is_empty(),
            "and no reply"
        );
    }
}
