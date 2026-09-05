//! The viewer's persistent settings store (`viewer-ui-settings-store` wired into
//! the app): a thin Bevy resource over [`sl_settings::SettingsStore`], the
//! reference viewer's `gSavedSettings` counterpart.
//!
//! Only the settings the current features need are registered here; more join as
//! their features land. The file is TOML: each override is a commented
//! `name = value` line grouped into a `[section]` table.
//!
//! Two scopes are persisted, mirroring the reference viewer's `gSavedSettings` /
//! `gSavedPerAccountSettings` split:
//!
//! - The [`Global`](Scope::Global) scope loads from and saves to the platform
//!   config directory's `viewer-settings.toml` (`paths`).
//! - The [`Account`](Scope::Account) scope is per-avatar: once the agent UUID is
//!   known at login, [`load_account_settings`] resolves the avatar's directory
//!   (keyed by grid + avatar name, with rename discovery — [`sl_account_dirs`])
//!   and loads its `settings.toml`. It resolves over the global scope.
//!
//! Both scopes are written whenever a setting changes ([`flush_settings`], one
//! coalesced write on the [`IoTaskPool`], at most one
//! in flight) and again at process exit (`save_settings_on_exit`, synchronous),
//! so a tuned value (e.g. a SpaceNavigator sensitivity) survives a restart.
//!
//! [`keys`] holds the setting **names** that both the feature owning a setting
//! and the preferences panel drawing a control for it have to agree on — the
//! layer beneath both, so neither has to depend on the other for a string.

pub mod keys;

use core::sync::atomic::{AtomicBool, Ordering};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on};
use sl_client_bevy::SlIdentity;
use sl_settings::{Scope, SettingValue, SettingsStore};
use tracing::{info, warn};

/// The account-scope settings filename within a per-avatar account directory.
const ACCOUNT_SETTINGS_FILE: &str = "settings.toml";

/// The per-avatar account identity resolved from the credentials before login
/// (grid + readable avatar name) and the accounts root, used to locate the
/// account settings directory once the agent UUID is known at login.
#[derive(Debug, Resource, Clone)]
pub struct AccountContext {
    /// The accounts root for settings (`<config>/accounts`), or `None` when the
    /// platform has no config directory (per-avatar settings are then disabled).
    pub accounts_base: Option<PathBuf>,
    /// The grid segment (from `sl_account_dirs::grid_dir_name`).
    pub grid: String,
    /// The readable avatar segment (from `sl_account_dirs::avatar_dir_name`).
    pub avatar: String,
}

/// The viewer's settings store, a Bevy resource.
#[derive(Debug, Resource)]
pub struct ViewerSettings {
    /// The underlying typed store (declared defaults + global/account overrides).
    store: SettingsStore,
    /// Where the global scope is persisted (the platform config directory).
    global_path: PathBuf,
    /// Where the account scope is persisted, once resolved at login; `None`
    /// until then (and when the platform has no per-avatar directory).
    account_path: Option<PathBuf>,
    /// Whether an override changed since the last write started.
    ///
    /// An [`AtomicBool`] rather than a plain field because the in-session save
    /// call sites hold the resource immutably ([`save_async`](Self::save_async)
    /// takes `&self`), and because it is cleared from the flush system while a
    /// write it started is still running.
    dirty: AtomicBool,
    /// The write in flight, if any.
    ///
    /// Holding it is what serializes the writes: [`flush_settings`] starts no
    /// second write while this is occupied, so two serializations can never
    /// land out of order. `None` between writes, and in a test app that drives
    /// the store directly.
    writing: Mutex<Option<Task<()>>>,
}

impl ViewerSettings {
    /// A read-only view of the store, for consumers reading their settings.
    #[must_use]
    pub const fn store(&self) -> &SettingsStore {
        &self.store
    }

    /// Register a setting's declared default (name → value + comment) under a
    /// section, logging and swallowing the (only-on-duplicate) error so a double
    /// registration can never abort startup.
    fn declare(&mut self, section: &[&str], name: &str, value: SettingValue, comment: &str) {
        if let Err(error) = self.store.register_in(section, name, value, comment) {
            warn!("settings: could not register {name}: {error}");
        }
    }

    /// Register a setting grouped under a `[section]` of the persisted file
    /// (e.g. `["spacenav", "flycam"]` → `[spacenav.flycam]`), the pub(crate)
    /// entry a feature module calls from [`FromWorld`]. Pass an empty section to
    /// place the setting at the document root.
    pub fn register_in(
        &mut self,
        section: &[&str],
        name: &str,
        value: SettingValue,
        comment: &str,
    ) {
        self.declare(section, name, value, comment);
    }

    /// Register a persisted setting grouped under a `[section]` that the raw
    /// debug-settings editor **skips** — mechanical UI state (window geometry,
    /// tab splits, table sort orders) that is saved and restored but is not a
    /// knob anyone debugs by hand. The floater / table persistence layers call
    /// this instead of [`register_in`](Self::register_in).
    pub fn register_hidden_in(
        &mut self,
        section: &[&str],
        name: &str,
        value: SettingValue,
        comment: &str,
    ) {
        if let Err(error) = self.store.register_hidden_in(section, name, value, comment) {
            warn!("settings: could not register {name}: {error}");
        }
    }

    /// Register a runtime-only setting whose overrides are never persisted (the
    /// reference viewer's transient debug settings). The two-way binding demo
    /// (`settings_binding`) uses this so its scratch values write no junk
    /// to the user's config.
    pub fn register_transient(&mut self, name: &str, value: SettingValue, comment: &str) {
        if let Err(error) = self.store.register_transient(name, value, comment) {
            warn!("settings: could not register {name}: {error}");
        }
    }

    /// Replace a registered setting's declared default (see
    /// [`SettingsStore::set_default`](sl_settings::SettingsStore::set_default)),
    /// logging and swallowing a (wrong-type or unregistered) error so a bad
    /// dynamic default can never abort a frame. The skin-colour bridge
    /// (`skin_colors`) feeds the active skin's palette in here.
    pub fn set_default(&mut self, name: &str, value: SettingValue) {
        if let Err(error) = self.store.set_default(name, value) {
            warn!("settings: could not set default for {name}: {error}");
        }
    }

    /// Write a value to the per-avatar [`Account`](Scope::Account) scope,
    /// logging and swallowing a (wrong-type or unregistered) error so a bad write
    /// can never abort a frame. The floater-geometry persistence
    /// (`floater_persist`) writes each floater's remembered rect /
    /// visibility here as it changes.
    pub fn set_account(&mut self, name: &str, value: SettingValue) {
        self.set(Scope::Account, name, value);
    }

    /// Write a value to a chosen override scope, logging and swallowing a
    /// (wrong-type or unregistered) error so a bad write can never abort a
    /// frame. The two-way widget binding (`settings_binding`) writes a
    /// user edit here, at the binding's declared scope.
    pub fn set(&mut self, scope: Scope, name: &str, value: SettingValue) {
        if let Err(error) = self.store.set(scope, name, value) {
            warn!("settings: could not set {name}: {error}");
        }
    }

    /// Drop a setting's override in one scope, reverting it to the layer below
    /// (see [`SettingsStore::reset`](sl_settings::SettingsStore::reset)). Returns
    /// whether an override was actually present. A bound "reset to default"
    /// control (`settings_binding`) calls this.
    pub fn reset(&mut self, scope: Scope, name: &str) -> bool {
        self.store.reset(scope, name)
    }

    /// Whether the per-avatar account scope has been resolved and loaded (post
    /// login). Consumers that seed themselves from a saved *account* value wait
    /// for this, since the account overrides are not in the store until then.
    #[must_use]
    pub const fn account_loaded(&self) -> bool {
        self.account_path.is_some()
    }

    /// The per-avatar account directory (the parent of the account
    /// `settings.toml`), once resolved at login — where a sibling per-account file
    /// (the persistent-notification store, `notification_persist`) lives.
    /// `None` until login, and when the platform has no per-avatar directory.
    #[must_use]
    pub fn account_dir(&self) -> Option<&Path> {
        self.account_path.as_deref().and_then(Path::parent)
    }

    /// Load the persisted global overrides, if the file exists — a missing file is
    /// the common first-run case and not an error.
    fn load_global(&mut self) {
        if !Path::new(&self.global_path).exists() {
            return;
        }
        match self.store.load_scope(Scope::Global, &self.global_path) {
            Ok(_loaded) => info!("settings: loaded {}", self.global_path.display()),
            Err(error) => warn!(
                "settings: could not load {}: {error}",
                self.global_path.display()
            ),
        }
    }

    /// Load the per-avatar account overrides from `account_dir/settings.toml`,
    /// recording the path so they are saved back there on logout. A missing file
    /// is the first-run case for that avatar and not an error.
    fn load_account(&mut self, account_dir: &Path) {
        let path = account_dir.join(ACCOUNT_SETTINGS_FILE);
        match self.store.load_scope(Scope::Account, &path) {
            Ok(_loaded) => info!("settings: loaded account scope {}", path.display()),
            Err(error) => warn!("settings: could not load {}: {error}", path.display()),
        }
        self.account_path = Some(path);
    }

    /// Save the global (and, once resolved, account) overrides to disk **now**,
    /// on the calling thread (best-effort; a failure is logged, not fatal).
    ///
    /// Waits for any write [`flush_settings`] has in flight first, so this — the
    /// newest state, serialized last — is what ends up on disk rather than an
    /// older serialization completing after it. The exit path
    /// (`save_settings_on_exit`) is the caller: at process exit there is no next
    /// frame to flush from, so the write has to be finished before returning.
    pub fn save(&self) {
        self.wait_for_write_in_flight();
        // Everything asked for is about to be on disk, so nothing is left for a
        // flush to redo — and a `save` that is not the exit save (a test's)
        // leaves the store in the same clean state a flush would.
        self.dirty.store(false, Ordering::Relaxed);
        for (path, text) in self.pending_writes() {
            write_scope_file(&path, &text);
        }
    }

    /// Ask for both scopes to be written, without blocking the frame on disk
    /// I/O: this only marks the store dirty, and [`flush_settings`] serializes
    /// and writes it on the [`IoTaskPool`] at the end of the frame.
    ///
    /// Every in-session persistence path calls this (floater geometry flush,
    /// table column widths, preferences apply, list sort changes, the people
    /// and derender lists). It is deliberately *not* a spawn-and-detach per
    /// call site: ten of them fire within a few frames of each other, two
    /// detached writes have no ordering guarantee, and an older serialization
    /// landing last silently undoes a newer one. Marking dirty instead makes a
    /// burst coalesce into one write of the newest state.
    pub fn save_async(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// The files to write and the text to write to them: the global scope
    /// always, the account scope once login has resolved its path.
    ///
    /// Serializing is cheap and stays on the frame thread; only the disk write
    /// moves to the [`IoTaskPool`].
    ///
    /// An *empty* path means no persistence rather than a file called nothing —
    /// that is what the `_for_test` constructors hand out, and what a platform
    /// with no config directory leaves the account scope as.
    fn pending_writes(&self) -> Vec<(PathBuf, String)> {
        let mut writes = Vec::new();
        for (path, scope) in [
            (Some(&self.global_path), Scope::Global),
            (self.account_path.as_ref(), Scope::Account),
        ] {
            if let Some(path) = path.filter(|path| !path.as_os_str().is_empty()) {
                writes.push((path.clone(), self.store.serialize_scope(scope)));
            }
        }
        writes
    }

    /// Block until the write [`flush_settings`] has in flight (if any) is done.
    ///
    /// Only the exit path uses this: everywhere else a flush that finds a write
    /// running returns and tries again next frame instead of blocking.
    fn wait_for_write_in_flight(&self) {
        let writing = lock(&self.writing).take();
        if let Some(writing) = writing {
            block_on(writing);
        }
    }

    /// One turn of the in-session write loop — see [`flush_settings`], the
    /// system that calls this every frame. A method as well as a system so a
    /// test can drive it without a [`World`].
    fn flush(&self) {
        if self.write_in_flight() {
            return;
        }
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        // Serialized now, on the calling (frame) thread, so the text written is
        // the state as of this frame; `dirty` is already cleared, so a change
        // made while the write runs re-dirties the store and is written by the
        // next flush.
        let writes = self.pending_writes();
        *lock(&self.writing) = Some(IoTaskPool::get().spawn(async move {
            for (path, text) in writes {
                write_scope_file(&path, &text);
            }
        }));
    }

    /// Whether a write is in flight, for the flush loop and its tests.
    fn write_in_flight(&self) -> bool {
        let mut finished = None;
        let running = {
            let mut guard = lock(&self.writing);
            match guard.take() {
                // Reap a finished write so the next flush is free to start one.
                Some(writing) if writing.is_finished() => {
                    finished = Some(writing);
                    false
                }
                Some(writing) => {
                    *guard = Some(writing);
                    true
                }
                None => false,
            }
        };
        // Outside the lock, and only for a task that has already finished, so
        // this never blocks the frame or holds the slot while it waits.
        if let Some(writing) = finished {
            block_on(writing);
        }
        running
    }

    /// Build a store-backed resource with no persistence paths, for unit tests
    /// that drive the store directly (e.g. the two-way binding tests) without
    /// touching the filesystem.
    ///
    /// Not `#[cfg(test)]`: the viewer's own tests build settings this way, and
    /// a `cfg(test)` item is not compiled when a *dependent* crate runs its
    /// tests. The `_for_test` name is the documentation — nothing in a running
    /// viewer should call it.
    #[must_use]
    pub const fn from_store_for_test(store: SettingsStore) -> Self {
        Self {
            store,
            global_path: PathBuf::new(),
            account_path: None,
            dirty: AtomicBool::new(false),
            writing: Mutex::new(None),
        }
    }

    /// Build a store-backed resource that really writes, to paths a test
    /// chooses — the one thing [`from_store_for_test`](Self::from_store_for_test)
    /// deliberately cannot do, since its empty paths mean "no persistence".
    ///
    /// For the tests that are *about* the write: this crate's own, and the
    /// exit-save system's in `sl-viewer-world-view`, which has to drive a real
    /// [`App`] to prove the save happens before the app stops. An absent
    /// `account` is the pre-login state.
    ///
    /// Not `#[cfg(test)]`: a `cfg(test)` item is not compiled when a *dependent*
    /// crate runs its tests. The `_for_test` name is the documentation —
    /// nothing in a running viewer should call it.
    #[must_use]
    pub const fn persisting_to_for_test(
        store: SettingsStore,
        global: PathBuf,
        account: Option<PathBuf>,
    ) -> Self {
        Self {
            store,
            global_path: global,
            account_path: account,
            dirty: AtomicBool::new(false),
            writing: Mutex::new(None),
        }
    }

    /// Pretend the account scope has loaded (an empty in-memory path), so a
    /// test can exercise a post-login gate ([`Self::account_loaded`], the
    /// preferences floater's account guard) without a filesystem.
    ///
    /// Not `#[cfg(test)]`: the viewer's own tests build settings this way, and
    /// a `cfg(test)` item is not compiled when a *dependent* crate runs its
    /// tests. The `_for_test` name is the documentation — nothing in a running
    /// viewer should call it.
    pub fn mark_account_loaded_for_test(&mut self) {
        self.account_path = Some(PathBuf::new());
    }

    /// Build the store with no registrations and no file load — the shared
    /// starting point for [`load_with`](Self::load_with) and for a test that
    /// wants to inspect what a given set of registrars declares.
    fn empty(global_path: PathBuf) -> Self {
        Self {
            store: SettingsStore::new(),
            global_path,
            account_path: None,
            dirty: AtomicBool::new(false),
            writing: Mutex::new(None),
        }
    }

    /// Build the store, run every `registrars` entry, and load any saved global
    /// overrides. The account scope loads later, at login
    /// ([`load_account_settings`]).
    ///
    /// The registrar list is supplied by the caller rather than named here,
    /// because this store cannot know the features that use it without
    /// depending on all of them. The viewer passes its full list; see
    /// `REGISTRARS` in the binary crate.
    ///
    /// Not a [`FromWorld`] initializer, because pre-app code (the login-request
    /// construction in `run_viewer`, which needs the stored start location
    /// before the Bevy [`World`] exists) reads the same store.
    pub fn load_with(registrars: &[fn(&mut Self)]) -> Self {
        let mut settings = Self::empty(sl_viewer_platform::paths::global_settings_file());
        settings.run_registrars(registrars);
        settings.load_global();
        settings
    }

    /// Run each registrar against this store, in the order given.
    pub fn run_registrars(&mut self, registrars: &[fn(&mut Self)]) {
        for register in registrars {
            register(self);
        }
    }

    /// A store with `registrars` applied and nothing loaded from disk, so a
    /// test can compare the declared surface without touching the filesystem.
    /// Not `#[cfg(test)]`: the viewer's own tests build settings this way,
    /// and a `cfg(test)` item is not compiled when a *dependent* crate runs
    /// its tests. The `_for_test` name is the documentation — nothing in a
    /// running viewer should call it.
    pub fn declared_for_test(registrars: &[fn(&mut Self)]) -> Self {
        let mut settings = Self::empty(PathBuf::new());
        settings.run_registrars(registrars);
        settings
    }
}

/// Write one scope's serialized TOML to `path`, logging (never hiding) a
/// failure — a settings file that could not be saved is not worth aborting a
/// frame or an exit over, but it must not pass silently either.
///
/// The parent directory is created first: on a first run that never reaches a
/// login, nothing else has made the platform config directory yet, and a write
/// into a directory that does not exist is how a first session's preferences
/// were quietly lost.
fn write_scope_file(path: &Path, text: &str) {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(error) = fs_err::create_dir_all(parent)
    {
        warn!(
            "settings: could not create {} for {}: {error}",
            parent.display(),
            path.display()
        );
        return;
    }
    match sl_settings::atomic_file::write_atomically(path, text) {
        Ok(()) => info!("settings: saved {}", path.display()),
        Err(error) => warn!("settings: could not save {}: {error}", path.display()),
    }
}

/// Lock a mutex, taking a poisoned lock's contents rather than propagating the
/// panic: the guarded value is a write handle, and a panic elsewhere is no
/// reason to stop saving the user's settings.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Write the settings files when an override has changed, at most one write in
/// flight at a time.
///
/// Serializing is cheap and stays on the frame thread; the write itself goes to
/// the [`IoTaskPool`], because a synchronous whole-file write inside `Update` is
/// a frame hitch on any disk that stalls.
///
/// **At most one write is in flight.** [`save_async`](ViewerSettings::save_async)
/// used to spawn a detached task per call site, and ten of them fire within a
/// few frames of each other (a preferences apply, a floater geometry flush, a
/// table's column widths); two detached writes have no ordering guarantee, so an
/// older serialization could land last and undo a newer one. A flush that finds
/// a write still running simply leaves the store dirty and tries again next
/// frame, which serializes the writes and coalesces a burst into one.
pub fn flush_settings(settings: Res<ViewerSettings>) {
    settings.flush();
}

/// Once the agent UUID is known (post-login), resolve the per-avatar account
/// directory — keyed by grid + avatar name, renaming it in place if the UUID
/// shows a name change — and load its account-scope settings. Runs every frame
/// but does its work exactly once (guarded on `account_path` being unset).
pub fn load_account_settings(
    mut settings: ResMut<ViewerSettings>,
    context: Res<AccountContext>,
    identity: Res<SlIdentity>,
) {
    // Already loaded, not logged in yet, or no per-avatar directory available.
    if settings.account_path.is_some() {
        return;
    }
    let Some(agent) = identity.agent_id else {
        return;
    };
    let Some(base) = context.accounts_base.clone() else {
        return;
    };
    match sl_account_dirs::reconcile_account_dir(
        &base,
        &context.grid,
        &context.avatar,
        agent.uuid(),
    ) {
        Ok(dir) => settings.load_account(&dir),
        Err(error) => warn!(
            "settings: could not resolve account directory under {}: {error}",
            base.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{ACCOUNT_SETTINGS_FILE, ViewerSettings};
    use bevy::tasks::{IoTaskPool, TaskPool};
    use core::sync::atomic::{AtomicBool, Ordering};
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Every [`SettingValue`] variant, each under its own name, so a round trip
    /// through the saved file covers the whole type surface rather than the two
    /// or three types a feature happens to use today.
    fn one_of_every_variant() -> Vec<(&'static str, SettingValue)> {
        vec![
            ("ABool", SettingValue::Bool(true)),
            ("AnI32", SettingValue::I32(-7)),
            ("AU32", SettingValue::U32(4_000_000_000)),
            ("AnF32", SettingValue::F32(0.25)),
            ("AString", SettingValue::String("a value".to_owned())),
            ("AColor3", SettingValue::Color3([0.1, 0.2, 0.3])),
            ("AColor4", SettingValue::Color4([0.1, 0.2, 0.3, 0.5])),
            ("AVec3", SettingValue::Vec3([1.0, 2.0, 3.0])),
            ("AVec3d", SettingValue::Vec3d([1.5, 2.5, 3.5])),
            ("ARect", SettingValue::Rect([1, 2, 3, 4])),
        ]
    }

    /// A store declaring [`one_of_every_variant`] with placeholder defaults, so
    /// each name is registered with the right type before a value is written.
    fn store_of_every_variant() -> Result<SettingsStore, TestError> {
        let mut store = SettingsStore::new();
        for (name, value) in one_of_every_variant() {
            // The default is the same *type* but not the same value, so a
            // round-trip assertion cannot pass by falling back to the default.
            let default = match value {
                SettingValue::Bool(_) => SettingValue::Bool(false),
                SettingValue::I32(_) => SettingValue::I32(0),
                SettingValue::U32(_) => SettingValue::U32(0),
                SettingValue::F32(_) => SettingValue::F32(0.0),
                SettingValue::String(_) => SettingValue::String(String::new()),
                SettingValue::Color3(_) => SettingValue::Color3([0.0; 3]),
                SettingValue::Color4(_) => SettingValue::Color4([0.0; 4]),
                SettingValue::Vec3(_) => SettingValue::Vec3([0.0; 3]),
                SettingValue::Vec3d(_) => SettingValue::Vec3d([0.0; 3]),
                SettingValue::Rect(_) => SettingValue::Rect([0; 4]),
            };
            store.register_in(&["round", "trip"], name, default, "a test setting")?;
        }
        Ok(store)
    }

    /// A unique throwaway directory under the system temp dir (the crate has no
    /// `tempfile` dependency; this mirrors sl-settings' test helper).
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

    /// The `IoTaskPool` the flush spawns onto. Shared by every test in this
    /// binary, so it is initialised at most once.
    fn io_pool() {
        IoTaskPool::get_or_init(TaskPool::new);
    }

    /// How long a held write dawdles after it is released, in yields.
    ///
    /// Enough that a caller which does *not* wait for it reliably loses the
    /// race to it. Nothing in the fixed code depends on this number — waiting
    /// makes the outcome certain either way — it exists so the test would have
    /// failed against the code that did not wait.
    const LATE_WRITE_YIELDS: u32 = 1000;

    /// Occupy the store's write slot with a task that does not finish until the
    /// returned flag is set, so a test can say exactly when the write in flight
    /// completes instead of hoping a disk is slow.
    ///
    /// `then`, if given, is written once released — the older serialization
    /// whose landing after a newer one is the defect under test.
    fn held_write(settings: &ViewerSettings, then: Option<(PathBuf, String)>) -> Arc<AtomicBool> {
        let gate = Arc::new(AtomicBool::new(false));
        let waiting = Arc::clone(&gate);
        *super::lock(&settings.writing) = Some(IoTaskPool::get().spawn(async move {
            while !waiting.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            if let Some((path, text)) = then {
                for _ in 0..LATE_WRITE_YIELDS {
                    std::thread::yield_now();
                }
                super::write_scope_file(&path, &text);
            }
        }));
        gate
    }

    /// Read a saved scope back into a fresh store, so an assertion sees what a
    /// *next session* would load rather than what this one holds in memory.
    fn reload(path: &Path) -> Result<SettingsStore, TestError> {
        let mut store = store_of_every_variant()?;
        store.load_scope(Scope::Global, path)?;
        Ok(store)
    }

    /// **Every value type survives the trip to disk and back.**
    ///
    /// The crate had no tests at all, so nothing pinned that a saved setting can
    /// be read again — the one property the whole persistence path exists for.
    /// Asserted per variant because the file format is per type: a rectangle and
    /// a 3-vector of `f64` are both arrays of numbers on the way out, and only a
    /// declared type tells them apart on the way back in.
    #[test]
    fn every_value_type_round_trips_through_the_saved_file() -> Result<(), TestError> {
        let dir = tempdir("round-trip")?;
        let path = dir.join("viewer-settings.toml");
        let mut store = store_of_every_variant()?;
        for (name, value) in one_of_every_variant() {
            store.set(Scope::Global, name, value)?;
        }

        ViewerSettings::persisting_to_for_test(store, path.clone(), None).save();

        let reloaded = reload(&path)?;
        for (name, value) in one_of_every_variant() {
            assert_eq!(
                reloaded.get(name),
                Some(&value),
                "{name} did not survive the round trip"
            );
        }
        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **Both scopes are written, and only the resolved ones.**
    ///
    /// The account file exists only after login resolves its path; before that
    /// the global file is still saved on its own, and nothing is written to a
    /// path that is not there.
    #[test]
    fn a_save_writes_the_global_scope_and_the_account_scope_once_resolved() -> Result<(), TestError>
    {
        let dir = tempdir("scopes")?;
        let global = dir.join("viewer-settings.toml");
        let account = dir.join("accounts").join(ACCOUNT_SETTINGS_FILE);
        let mut store = store_of_every_variant()?;
        store.set(Scope::Global, "AnI32", SettingValue::I32(1))?;
        store.set(Scope::Account, "AnI32", SettingValue::I32(2))?;

        // Before login: the account path is unresolved, so only the global file
        // is written.
        ViewerSettings::persisting_to_for_test(store_of_every_variant()?, global.clone(), None)
            .save();
        assert!(global.exists(), "the global scope was not saved");
        assert!(!account.exists(), "an unresolved account scope was written");

        // After login: both, and the account file lands in a directory the save
        // itself creates.
        ViewerSettings::persisting_to_for_test(store, global.clone(), Some(account.clone())).save();
        assert_eq!(
            reload(&global)?.get_i32("AnI32").ok(),
            Some(1),
            "the global file holds the account value"
        );
        let mut account_store = store_of_every_variant()?;
        account_store.load_scope(Scope::Account, &account)?;
        assert_eq!(account_store.get_i32("AnI32").ok(), Some(2));

        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **A save asked for in-session is one write, however many call sites ask.**
    ///
    /// This is the defect the task records: ten call sites each spawned a
    /// detached write, and two detached writes have no ordering guarantee, so an
    /// older serialization could land last. Asserted as the mechanism that
    /// prevents it — a flush that finds a write in flight starts no second one
    /// and leaves the store dirty, so the writes serialize and a burst coalesces
    /// into a single write of the newest state.
    #[test]
    fn a_burst_of_saves_coalesces_into_one_serialized_write() -> Result<(), TestError> {
        io_pool();
        let dir = tempdir("coalesce")?;
        let path = dir.join("viewer-settings.toml");
        let mut store = store_of_every_variant()?;
        store.set(Scope::Global, "AnI32", SettingValue::I32(1))?;
        let settings = ViewerSettings::persisting_to_for_test(store, path.clone(), None);

        // Nothing has changed: a flush writes nothing at all.
        settings.flush();
        assert!(!settings.write_in_flight(), "a clean store started a write");
        assert!(!path.exists(), "a clean store wrote a file");

        // A write that will not finish until this test says so, so what follows
        // is about the rule and not about how fast a disk is.
        let gate = held_write(&settings, None);

        // Two call sites asking within the same frame, and a flush that finds
        // the write running: nothing is started, and nothing is lost either —
        // the store stays dirty so the next flush writes the newest state.
        settings.save_async();
        settings.save_async();
        settings.flush();
        assert!(
            settings.dirty.load(Ordering::Relaxed),
            "a flush that could not write dropped the work"
        );
        assert!(
            !path.exists(),
            "a second write landed while one was running"
        );

        // Once it finishes, the next flush writes — once, for all three asks.
        gate.store(true, Ordering::Relaxed);
        settings.wait_for_write_in_flight();
        settings.flush();
        settings.wait_for_write_in_flight();
        assert_eq!(reload(&path)?.get_i32("AnI32").ok(), Some(1));
        assert!(
            !settings.dirty.load(Ordering::Relaxed),
            "the write left the store dirty"
        );

        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **The exit save is what actually reaches the disk.**
    ///
    /// The exit path used to be a synchronous save fired the frame a *logout was
    /// requested*, with detached writes still able to complete after it: a
    /// setting changed during the grace period was lost, and an older
    /// serialization could overwrite the newer one. `save` now waits for the
    /// flush in flight and writes last, so the file holds the state as of the
    /// exit — asserted with a value changed *after* the in-session flush that
    /// preceded it.
    #[test]
    fn the_exit_save_writes_the_state_as_of_the_exit() -> Result<(), TestError> {
        io_pool();
        let dir = tempdir("exit")?;
        let path = dir.join("viewer-settings.toml");
        let mut store = store_of_every_variant()?;
        store.set(Scope::Global, "AnI32", SettingValue::I32(1))?;
        let mut settings = ViewerSettings::persisting_to_for_test(store, path.clone(), None);

        // An in-session flush has started a write of the older value, and it is
        // held until this test releases it — the race the old code lost.
        let older = settings
            .pending_writes()
            .into_iter()
            .next()
            .ok_or("the global scope has nothing to write")?;
        let gate = held_write(&settings, Some(older));

        // The user changes the setting again, then quits.
        settings.set(Scope::Global, "AnI32", SettingValue::I32(2));
        gate.store(true, Ordering::Relaxed);
        settings.save();

        assert!(
            !settings.write_in_flight(),
            "the exit save returned with a write still running"
        );
        assert_eq!(
            reload(&path)?.get_i32("AnI32").ok(),
            Some(2),
            "an older serialization landed after the exit save"
        );
        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **A first run creates the directory it saves into.**
    ///
    /// Nothing else makes the platform config directory before a login does, so
    /// a session that quits at the login screen used to write into a directory
    /// that was not there — and lose everything it had been asked to remember.
    #[test]
    fn a_save_creates_the_directory_it_writes_into() -> Result<(), TestError> {
        let dir = tempdir("first-run")?;
        let path = dir.join("never").join("made").join("viewer-settings.toml");
        let mut store = store_of_every_variant()?;
        store.set(Scope::Global, "ABool", SettingValue::Bool(true))?;

        ViewerSettings::persisting_to_for_test(store, path.clone(), None).save();

        assert_eq!(reload(&path)?.get_bool("ABool").ok(), Some(true));
        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **An empty path means no persistence, not a file called nothing.**
    ///
    /// The `_for_test` constructors and a platform with no config directory both
    /// leave a path empty; a save must then write nowhere rather than fail
    /// repeatedly against a nameless file.
    #[test]
    fn an_empty_path_writes_nothing() -> Result<(), TestError> {
        let mut settings = ViewerSettings::from_store_for_test(store_of_every_variant()?);
        settings.mark_account_loaded_for_test();
        settings.set(Scope::Global, "ABool", SettingValue::Bool(true));

        assert!(settings.pending_writes().is_empty());
        // And it is still a save that reports nothing left to do.
        settings.save_async();
        settings.save();
        assert!(!settings.dirty.load(Ordering::Relaxed));
        Ok(())
    }
}
