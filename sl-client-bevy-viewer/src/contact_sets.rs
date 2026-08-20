//! **Contact sets** (`viewer-contact-sets`): named, coloured groups of
//! residents, kept entirely on this client.
//!
//! A contact set is not a Second Life group — nothing about it leaves the
//! machine. It is the user's own filing of the people they know: *Builders*,
//! *Sunday DJ crowd*, *the two friends I actually want to see at an event*. Each
//! set carries a colour, and the colour is the point: it is what lets a name in
//! a crowded radar, a chat line or a name tag say **which** of the user's
//! circles that person belongs to, without reading the name at all.
//!
//! # One store, one way in
//!
//! [`ContactSets`] is the whole model — the sets, their members, the per-account
//! file, and the by-agent index the tinting consumers read. Every surface that
//! changes something writes a [`RequestContactSet`] rather than touching the
//! sets ([`apply_contact_set_requests`] runs the guards), the same shape the
//! render exceptions ([`crate::avatar_render_settings`]) and the mute list
//! ([`crate::mutes`]) use. A refused request is logged with **why**
//! ([`ContactSetRefusal`]), and a refused *rename* additionally raises the
//! reference's own `RenameContactSetFailure` notification, since that one is a
//! user action that visibly did nothing.
//!
//! # The colour question: the smallest set wins
//!
//! A resident may be in several sets, so "what colour is this person" needs a
//! rule. [`ContactSets::color_of`] answers with the colour of the **smallest**
//! set they belong to — the reference's rule (`LGGContactSets::getFriendColor`),
//! and the one that reads right: the set with three people in it says more about
//! someone than the set with eighty. Ties break by set name, so the answer is
//! stable rather than hash-order.
//!
//! # Names
//!
//! A set outlives everyone's presence — most of its members are nowhere near you
//! most of the time — so, exactly as the render-exception store does, each
//! member's name is remembered as the surface that filed them knew it, and the
//! **live name cache** is read over it when it has an answer
//! ([`refresh_contact_set_names`]). A resolution never rewrites the stored name:
//! a grid answers an id it cannot resolve with a placeholder, and adopting that
//! would destroy the record of who was filed.
//!
//! # The file is the reference's shape
//!
//! The store is a per-account `contact_sets.json`, a sibling of the account
//! `settings.toml`, whose top level is keyed by **set name** with a `color` and
//! a `friends` map — the same layout Firestorm's `settings_friends_groups.xml`
//! uses, so a list exported from there ports across by transcription. The
//! reference's own internal keys (`globalSettings`, `extraAvs`, `Pseudonyms`)
//! are recognised and skipped rather than mistaken for sets; the member-name
//! memo above is our addition under [`NAMES_KEY`].
//!
//! Reference (Firestorm, read-only): `lggcontactsets`, `fspanelcontactsets`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use bevy::color::ColorToComponents as _;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{AgentKey, Uuid};
use tracing::{debug, info, warn};

use crate::avatars::AvatarState;
use crate::notifications::ShowNotification;
use crate::settings::ViewerSettings;

/// The per-account file the sets are stored in (a sibling of the account
/// `settings.toml`, like [`crate::avatar_render_settings`]'s exceptions). The
/// account directory is already per-grid and per-avatar, so the bare name
/// suffices.
const STORE_FILE: &str = "contact_sets.json";

/// Our own top-level key holding the remembered member names (`id → name`). The
/// reference has no such key — its list is names-free and leans on the name
/// cache — so it is written beside the sets rather than inside one, and a
/// reference file simply has none.
const NAMES_KEY: &str = "names";

/// The top-level keys the **reference** file uses for its own bookkeeping. They
/// are not sets: a file written by Firestorm carries them, and reading one of
/// them as a set would invent a set named `globalSettings`.
///
/// `globalSettings` (the fallback colour for everyone in no set), `extraAvs`
/// (which members are not friends) and `Pseudonyms` (per-avatar aliases) have no
/// consumer here yet, so they are skipped on the way in and not written back.
const REFERENCE_INTERNAL_KEYS: &[&str] = &["globalSettings", "extraAvs", "Pseudonyms"];

/// The names a set may not be given: our own file key, the reference's internal
/// keys, and the two pseudo-sets the panel's set list shows above the real ones.
/// Compared case-insensitively — a set called `all sets` would be just as
/// confusing as `All Sets`.
pub(crate) const RESERVED_SET_NAMES: &[&str] = &[
    NAMES_KEY,
    "globalSettings",
    "extraAvs",
    "Pseudonyms",
    ALL_SETS_LABEL,
    NO_SETS_LABEL,
];

/// The panel's "every set at once" pseudo-set, named as the reference names it.
pub(crate) const ALL_SETS_LABEL: &str = "All Sets";

/// The panel's "friends who are in no set" pseudo-set, named as the reference
/// names it.
pub(crate) const NO_SETS_LABEL: &str = "No Sets";

/// How often a member whose name has not resolved is chased up — a name request
/// is one batched round trip, and a name nobody can answer must not be asked for
/// every frame.
const NAME_REFRESH_SECONDS: f64 = 2.0;

/// The colours a newly created set is given, in order, cycling once they run
/// out.
///
/// A **deliberate divergence**: the reference starts every new set at its
/// default grey, which makes the one feature that distinguishes a set — its
/// colour — invisible until the user goes and picks one. Handing each new set a
/// distinct, legible colour means a set tints the moment it exists, and the
/// colour picker is then a correction rather than a required step.
const NEW_SET_COLORS: &[Color] = &[
    Color::srgb(0.42, 0.72, 1.00),
    Color::srgb(0.55, 0.85, 0.45),
    Color::srgb(1.00, 0.72, 0.35),
    Color::srgb(0.90, 0.55, 0.85),
    Color::srgb(1.00, 0.55, 0.50),
    Color::srgb(0.55, 0.85, 0.85),
];

// ---------------------------------------------------------------------------
// The model.
// ---------------------------------------------------------------------------

/// One contact set: a name, a colour, and the residents filed under it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContactSet {
    /// The set's name, as the user typed it — also its key in the file.
    name: String,
    /// The colour everything tinted by this set uses.
    color: Color,
    /// The residents in the set, ordered by id so the file is stable.
    members: BTreeSet<Uuid>,
}

impl ContactSet {
    /// The set's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The set's colour.
    pub(crate) const fn color(&self) -> Color {
        self.color
    }

    /// How many residents are in the set — the size the colour rule compares.
    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }

    /// The residents in the set.
    pub(crate) fn members(&self) -> impl Iterator<Item = AgentKey> + '_ {
        self.members.iter().copied().map(AgentKey::from)
    }
}

/// Why a [`RequestContactSet`] was refused — the guards, named so the caller can
/// react (the rename failure raises the reference's notification) and the tests
/// can assert on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactSetRefusal {
    /// The name was empty (or only whitespace).
    EmptyName,
    /// The name is one the file or the panel already means something by.
    ReservedName,
    /// A set of that name already exists.
    DuplicateName,
    /// The request named a set that does not exist.
    UnknownSet,
    /// The request named the null agent.
    NullAgent,
}

impl core::fmt::Display for ContactSetRefusal {
    /// Say what was refused, for a log line (and for the `?` in a test).
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let reason = match *self {
            Self::EmptyName => "a contact set needs a name",
            Self::ReservedName => "that name is reserved",
            Self::DuplicateName => "a contact set of that name already exists",
            Self::UnknownSet => "no such contact set",
            Self::NullAgent => "the null agent cannot be filed",
        };
        f.write_str(reason)
    }
}

impl core::error::Error for ContactSetRefusal {}

/// The contact sets: the sets, their by-agent index, the remembered names, and
/// the persistence.
#[derive(Resource, Debug, Default)]
pub(crate) struct ContactSets {
    /// The sets by name — a `BTreeMap`, so the panel's list and the file are
    /// both in name order without sorting either.
    sets: BTreeMap<String, ContactSet>,
    /// Which sets each resident is in, in name order — the index the colour rule
    /// and the tinting consumers read.
    by_agent: HashMap<AgentKey, Vec<String>>,
    /// The name each member was filed under, as the filing surface knew it.
    /// Persisted; never overwritten by a name resolution.
    names: HashMap<AgentKey, String>,
    /// What the live name cache last answered for a member, mirrored here by
    /// [`refresh_contact_set_names`] so a view rebuilds off one revision instead
    /// of polling the name cache per row. Session state: never persisted.
    live_names: HashMap<AgentKey, String>,
    /// Bumped on every change, so a view rebuilds exactly when something moved.
    revision: u64,
    /// How many sets have ever been created this session plus those loaded — the
    /// cursor into [`NEW_SET_COLORS`], so two sets made in a row differ.
    created: usize,
    /// The per-account store path, resolved at login; `None` until then (and
    /// when the platform has no per-avatar directory, disabling persistence).
    path: Option<PathBuf>,
    /// Whether the on-disk file has been read — a once-per-session load.
    loaded: bool,
    /// Whether the sets have changed since the last flush.
    dirty: bool,
}

impl ContactSets {
    // --- Reads ------------------------------------------------------------

    /// Every set, in name order.
    pub(crate) fn sets(&self) -> impl ExactSizeIterator<Item = &ContactSet> {
        self.sets.values()
    }

    /// How many sets there are.
    pub(crate) fn set_count(&self) -> usize {
        self.sets.len()
    }

    /// The set of that name, if it exists.
    pub(crate) fn set(&self, name: &str) -> Option<&ContactSet> {
        self.sets.get(name)
    }

    /// The model revision — a view stores the value it last built at and
    /// rebuilds when it advances.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// The sets `agent` is filed under, in name order (empty when none).
    pub(crate) fn sets_of(&self, agent: AgentKey) -> &[String] {
        self.by_agent.get(&agent).map_or(&[], Vec::as_slice)
    }

    /// Whether `agent` is in any set at all.
    pub(crate) fn is_filed(&self, agent: AgentKey) -> bool {
        self.by_agent.contains_key(&agent)
    }

    /// Everyone who is in at least one set, in id order.
    pub(crate) fn everyone_filed(&self) -> Vec<AgentKey> {
        let mut agents: Vec<AgentKey> = self.by_agent.keys().copied().collect();
        agents.sort_unstable_by_key(AgentKey::uuid);
        agents
    }

    /// The colour to tint `agent` with: the colour of the **smallest** set they
    /// belong to (the reference's rule — the more specific set says more about
    /// someone), ties broken by set name so the answer is stable. `None` when
    /// they are in no set, which is the caller's cue to leave its own colour
    /// alone.
    pub(crate) fn color_of(&self, agent: AgentKey) -> Option<Color> {
        self.sets_of(agent)
            .iter()
            .filter_map(|name| self.sets.get(name))
            .min_by_key(|set| (set.member_count(), set.name.clone()))
            .map(ContactSet::color)
    }

    /// The best name known for `agent`: what the live name cache answered, else
    /// the name they were filed under, else `None` (the caller shows the id).
    pub(crate) fn label_of(&self, agent: AgentKey) -> Option<&str> {
        self.live_names
            .get(&agent)
            .or_else(|| self.names.get(&agent))
            .map(String::as_str)
    }

    // --- Writes (guarded; the request system is the way in) ---------------

    /// Create an empty set named `name`, coloured from [`NEW_SET_COLORS`].
    pub(crate) fn create_set(&mut self, name: &str) -> Result<(), ContactSetRefusal> {
        let name = check_set_name(name)?;
        if self.sets.contains_key(&name) {
            return Err(ContactSetRefusal::DuplicateName);
        }
        let color = NEW_SET_COLORS
            .iter()
            .copied()
            .cycle()
            .nth(self.created)
            .unwrap_or(Color::WHITE);
        self.created = self.created.saturating_add(1);
        let _replaced = self.sets.insert(
            name.clone(),
            ContactSet {
                name,
                color,
                members: BTreeSet::new(),
            },
        );
        self.touch();
        Ok(())
    }

    /// Rename a set, keeping its colour and members. Renaming to the name it
    /// already has succeeds and changes nothing.
    pub(crate) fn rename_set(&mut self, from: &str, to: &str) -> Result<(), ContactSetRefusal> {
        let to = check_set_name(to)?;
        if !self.sets.contains_key(from) {
            return Err(ContactSetRefusal::UnknownSet);
        }
        if from == to {
            return Ok(());
        }
        if self.sets.contains_key(&to) {
            return Err(ContactSetRefusal::DuplicateName);
        }
        let Some(mut set) = self.sets.remove(from) else {
            return Err(ContactSetRefusal::UnknownSet);
        };
        set.name.clone_from(&to);
        let _replaced = self.sets.insert(to, set);
        self.touch();
        Ok(())
    }

    /// Delete a set. Its members keep whatever other sets they are in.
    pub(crate) fn remove_set(&mut self, name: &str) -> Result<(), ContactSetRefusal> {
        if self.sets.remove(name).is_none() {
            return Err(ContactSetRefusal::UnknownSet);
        }
        self.touch();
        Ok(())
    }

    /// Give a set a new colour.
    pub(crate) fn recolor_set(
        &mut self,
        name: &str,
        color: Color,
    ) -> Result<(), ContactSetRefusal> {
        let Some(set) = self.sets.get_mut(name) else {
            return Err(ContactSetRefusal::UnknownSet);
        };
        if set.color == color {
            return Ok(());
        }
        set.color = color;
        self.touch();
        Ok(())
    }

    /// File `agent` under a set, remembering `name` as what they were called.
    /// Filing someone already in the set only refreshes the remembered name.
    pub(crate) fn add_member(
        &mut self,
        set: &str,
        agent: AgentKey,
        name: &str,
    ) -> Result<(), ContactSetRefusal> {
        if agent.uuid().is_nil() {
            return Err(ContactSetRefusal::NullAgent);
        }
        let Some(target) = self.sets.get_mut(set) else {
            return Err(ContactSetRefusal::UnknownSet);
        };
        let added = target.members.insert(agent.uuid());
        let renamed = !name.is_empty() && self.names.get(&agent).map(String::as_str) != Some(name);
        if renamed {
            let _previous = self.names.insert(agent, name.to_owned());
        }
        if added || renamed {
            self.touch();
        }
        Ok(())
    }

    /// Take `agent` out of a set.
    pub(crate) fn remove_member(
        &mut self,
        set: &str,
        agent: AgentKey,
    ) -> Result<(), ContactSetRefusal> {
        let Some(target) = self.sets.get_mut(set) else {
            return Err(ContactSetRefusal::UnknownSet);
        };
        if target.members.remove(&agent.uuid()) {
            self.touch();
        }
        Ok(())
    }

    /// Move `agent` from one set to another. Both must exist, so a half-done
    /// move cannot leave them filed nowhere.
    pub(crate) fn move_member(
        &mut self,
        from: &str,
        to: &str,
        agent: AgentKey,
    ) -> Result<(), ContactSetRefusal> {
        if !self.sets.contains_key(from) || !self.sets.contains_key(to) {
            return Err(ContactSetRefusal::UnknownSet);
        }
        let name = self.names.get(&agent).cloned().unwrap_or_default();
        self.add_member(to, agent, &name)?;
        if from != to {
            self.remove_member(from, agent)?;
        }
        Ok(())
    }

    /// Mirror the live name cache's answer for a member, bumping the revision
    /// (so a view rebuilds) when it says something new. Not a change to the
    /// file: the stored name is the filing surface's and stays.
    fn note_live_name(&mut self, agent: AgentKey, name: &str) {
        if name.is_empty() || self.live_names.get(&agent).map(String::as_str) == Some(name) {
            return;
        }
        let _previous = self.live_names.insert(agent, name.to_owned());
        self.revision = self.revision.wrapping_add(1);
    }

    /// Record a change: re-index, bump the revision, and mark the file dirty.
    fn touch(&mut self) {
        self.dirty = true;
        self.reindex();
    }

    /// Rebuild the by-agent index (and drop the name memos of anyone no longer
    /// filed anywhere), then bump the revision.
    fn reindex(&mut self) {
        self.by_agent.clear();
        for set in self.sets.values() {
            for member in &set.members {
                self.by_agent
                    .entry(AgentKey::from(*member))
                    .or_default()
                    .push(set.name.clone());
            }
        }
        // The name memos exist to label filed people, so they follow the index.
        // Bound as locals so each `retain` is a disjoint field borrow rather
        // than a second borrow of the whole model.
        let filed = &self.by_agent;
        self.names.retain(|agent, _name| filed.contains_key(agent));
        self.live_names
            .retain(|agent, _name| filed.contains_key(agent));
        self.revision = self.revision.wrapping_add(1);
    }
}

/// The name a set may be given: trimmed, non-empty, and not one of the names the
/// file or the panel already means something by.
fn check_set_name(name: &str) -> Result<String, ContactSetRefusal> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ContactSetRefusal::EmptyName);
    }
    if RESERVED_SET_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(trimmed))
    {
        return Err(ContactSetRefusal::ReservedName);
    }
    Ok(trimmed.to_owned())
}

// ---------------------------------------------------------------------------
// Requests.
// ---------------------------------------------------------------------------

/// Ask for a change to the contact sets. The one way in: the panel, its
/// configuration floater, the add-to-set floater and the avatar pie all write
/// this, so the guards run once and in one place.
#[derive(Message, Debug, Clone)]
pub(crate) enum RequestContactSet {
    /// Create an empty set.
    Create {
        /// The name to give it.
        name: String,
    },
    /// Rename a set, keeping its colour and members.
    Rename {
        /// The set as it is named now.
        from: String,
        /// The name to give it.
        to: String,
    },
    /// Delete a set.
    Remove {
        /// The set to delete.
        name: String,
    },
    /// Give a set a new colour.
    Recolor {
        /// The set to recolour.
        name: String,
        /// Its new colour.
        color: Color,
    },
    /// File a resident under a set.
    Add {
        /// The set to file them under.
        set: String,
        /// The resident.
        agent: AgentKey,
        /// The best name the requesting surface knows for them (empty when it
        /// knows none — [`refresh_contact_set_names`] fills it in later).
        name: String,
    },
    /// Take a resident out of a set.
    RemoveMember {
        /// The set to take them out of.
        set: String,
        /// The resident.
        agent: AgentKey,
    },
    /// Move a resident from one set to another.
    Move {
        /// The set they are in now.
        from: String,
        /// The set to move them to.
        to: String,
        /// The resident.
        agent: AgentKey,
    },
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// Registers the contact-set store, its requests and its persistence. The panel
/// over it is [`crate::contact_sets_panel`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ContactSetsPlugin;

impl Plugin for ContactSetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContactSets>()
            .add_message::<RequestContactSet>()
            .add_systems(
                Update,
                (
                    load_contact_sets,
                    apply_contact_set_requests,
                    refresh_contact_set_names,
                    flush_contact_sets,
                )
                    .chain(),
            );
    }
}

/// Turn each [`RequestContactSet`] into a change to the sets, after the guards.
/// A refused rename raises the reference's `RenameContactSetFailure`, since a
/// rename is a user action whose refusal is otherwise invisible; the rest are
/// logged.
pub(crate) fn apply_contact_set_requests(
    mut requests: MessageReader<RequestContactSet>,
    mut sets: ResMut<ContactSets>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for request in requests.read() {
        let outcome = match request {
            RequestContactSet::Create { name } => sets.create_set(name),
            RequestContactSet::Rename { from, to } => {
                let outcome = sets.rename_set(from, to);
                if outcome.is_err() {
                    notifications.write(
                        ShowNotification::new("RenameContactSetFailure")
                            .arg("SET", from.clone())
                            .arg("NEW_NAME", to.clone()),
                    );
                }
                outcome
            }
            RequestContactSet::Remove { name } => sets.remove_set(name),
            RequestContactSet::Recolor { name, color } => sets.recolor_set(name, *color),
            RequestContactSet::Add { set, agent, name } => sets.add_member(set, *agent, name),
            RequestContactSet::RemoveMember { set, agent } => sets.remove_member(set, *agent),
            RequestContactSet::Move { from, to, agent } => sets.move_member(from, to, *agent),
        };
        match outcome {
            Ok(()) => debug!(?request, "contact sets changed"),
            Err(refusal) => debug!(?request, ?refusal, "refusing a contact-set change"),
        }
    }
}

/// Keep the name cache warm for every filed resident and mirror what it answers,
/// so a view reads live names off one revision. Throttled: a name nobody can
/// answer must not be re-requested every frame.
pub(crate) fn refresh_contact_set_names(
    mut sets: ResMut<ContactSets>,
    avatars: Option<ResMut<AvatarState>>,
    time: Res<Time>,
    mut last_run: Local<Option<f64>>,
) {
    let Some(mut avatars) = avatars else {
        return;
    };
    if sets.by_agent.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if last_run.is_some_and(|last| now - last < NAME_REFRESH_SECONDS) {
        return;
    }
    *last_run = Some(now);
    let agents: Vec<AgentKey> = sets.by_agent.keys().copied().collect();
    for agent in agents {
        let resolved = avatars
            .name_record(agent)
            .and_then(crate::avatars::NameRecord::preferred_name)
            .map(ToOwned::to_owned);
        match resolved {
            Some(name) => sets.note_live_name(agent, &name),
            // Nothing known yet: the request de-duplicates itself, so a name
            // that is simply slow is asked for once and a name nobody can
            // answer costs one request for the session.
            None => avatars.request_name(agent),
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence.
// ---------------------------------------------------------------------------

/// One set as it is written: the reference's `color` and `friends` members of a
/// set entry. `friends` is a map of id → `""` rather than a list because that is
/// what the reference writes (an LLSD map used as a set), and the point of the
/// layout is that a transcribed file loads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredSet {
    /// The set's colour as sRGB + alpha, the reference's `LLColor4` order.
    #[serde(default)]
    color: Option<[f32; 4]>,
    /// The members, as a map of id string → `""`.
    #[serde(default)]
    friends: BTreeMap<String, String>,
}

/// The colour a set was stored with, as the model's [`Color`].
fn color_from_stored(stored: [f32; 4]) -> Color {
    Color::Srgba(Srgba::from_f32_array(stored))
}

/// A set's colour as it is stored.
fn stored_color(color: Color) -> [f32; 4] {
    Srgba::from(color).to_f32_array()
}

/// Once the per-account directory resolves (post login), read the stored sets.
/// Runs once.
pub(crate) fn load_contact_sets(
    mut sets: ResMut<ContactSets>,
    settings: Option<Res<ViewerSettings>>,
) {
    if sets.loaded {
        return;
    }
    let Some(account_dir) = settings
        .as_deref()
        .filter(|settings| settings.account_loaded())
        .and_then(ViewerSettings::account_dir)
    else {
        return;
    };
    let path = account_dir.join(STORE_FILE);
    sets.loaded = true;
    let stored = read_store(&path);
    sets.path = Some(path);
    if stored.sets.is_empty() && stored.names.is_empty() {
        return;
    }
    // A load is not an edit — everything read is already on disk — but a set
    // made before the account directory resolved is, and must survive the load
    // and still be written.
    let pending = sets.dirty;
    for (name, set) in stored.sets {
        if sets.sets.contains_key(&name) {
            continue;
        }
        let _replaced = sets.sets.insert(name, set);
    }
    for (agent, name) in stored.names {
        let _previous = sets.names.entry(agent).or_insert(name);
    }
    sets.created = sets.created.saturating_add(sets.sets.len());
    sets.reindex();
    sets.dirty = pending;
}

/// What a read of the store yielded.
#[derive(Debug, Default)]
struct StoredContactSets {
    /// The sets by name.
    sets: BTreeMap<String, ContactSet>,
    /// The remembered member names.
    names: HashMap<AgentKey, String>,
}

/// Read the persisted sets from `path`, tolerating a missing file (the first-run
/// case) and a malformed one (logged, treated as empty — a corrupt store must
/// not abort login). A top-level key that is one of the reference's internal
/// ones, or whose value is not shaped like a set, is skipped rather than
/// failing the whole read: a file transcribed from Firestorm carries both.
fn read_store(path: &Path) -> StoredContactSets {
    if !path.exists() {
        return StoredContactSets::default();
    }
    let contents = match fs_err::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            warn!(path = %path.display(), %error, "could not read the contact sets");
            return StoredContactSets::default();
        }
    };
    let top: BTreeMap<String, serde_json::Value> = match serde_json::from_str(&contents) {
        Ok(top) => top,
        Err(error) => {
            warn!(path = %path.display(), %error, "malformed contact sets; ignoring");
            return StoredContactSets::default();
        }
    };
    let mut read = StoredContactSets::default();
    for (key, value) in top {
        if key == NAMES_KEY {
            read.names = read_names(&value);
            continue;
        }
        if REFERENCE_INTERNAL_KEYS.contains(&key.as_str()) {
            debug!(key, "skipping a reference-internal contact-sets key");
            continue;
        }
        match serde_json::from_value::<StoredSet>(value) {
            Ok(stored) => {
                let members: BTreeSet<Uuid> = stored
                    .friends
                    .keys()
                    .filter_map(|id| id.parse::<Uuid>().ok())
                    .collect();
                let color = stored.color.map_or(Color::WHITE, color_from_stored);
                let _replaced = read.sets.insert(
                    key.clone(),
                    ContactSet {
                        name: key,
                        color,
                        members,
                    },
                );
            }
            Err(error) => warn!(key, %error, "skipping a contact-sets entry that is not a set"),
        }
    }
    info!(count = read.sets.len(), path = %path.display(), "loaded the contact sets");
    read
}

/// Read the remembered member names, ignoring anything that is not an id → name
/// map.
fn read_names(value: &serde_json::Value) -> HashMap<AgentKey, String> {
    let Ok(names) = serde_json::from_value::<BTreeMap<String, String>>(value.clone()) else {
        warn!("skipping malformed contact-set member names");
        return HashMap::new();
    };
    names
        .into_iter()
        .filter_map(|(id, name)| id.parse::<Uuid>().ok().map(|id| (AgentKey::from(id), name)))
        .collect()
}

/// Write the sets when they have changed, once the path is known (best-effort —
/// a write failure is logged, never fatal).
pub(crate) fn flush_contact_sets(mut sets: ResMut<ContactSets>) {
    if !sets.dirty {
        return;
    }
    let Some(path) = sets.path.clone() else {
        return;
    };
    match serde_json::to_string_pretty(&store_value(&sets)) {
        Ok(contents) => {
            if let Err(error) = fs_err::write(&path, contents) {
                warn!(path = %path.display(), %error, "could not write the contact sets");
            } else {
                debug!(count = sets.sets.len(), "flushed the contact sets");
                sets.dirty = false;
            }
        }
        Err(error) => warn!(%error, "could not serialize the contact sets"),
    }
}

/// The sets as the file's top-level map: one entry per set plus the remembered
/// member names.
fn store_value(sets: &ContactSets) -> BTreeMap<String, serde_json::Value> {
    let mut top: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for set in sets.sets.values() {
        let stored = StoredSet {
            color: Some(stored_color(set.color)),
            friends: set
                .members
                .iter()
                .map(|member| (member.to_string(), String::new()))
                .collect(),
        };
        if let Ok(value) = serde_json::to_value(stored) {
            let _replaced = top.insert(set.name.clone(), value);
        }
    }
    let names: BTreeMap<String, String> = sets
        .names
        .iter()
        .map(|(agent, name)| (agent.uuid().to_string(), name.clone()))
        .collect();
    if let Ok(value) = serde_json::to_value(names) {
        let _replaced = top.insert(NAMES_KEY.to_owned(), value);
    }
    top
}

#[cfg(test)]
mod tests {
    use super::{
        ALL_SETS_LABEL, ContactSet, ContactSetRefusal, ContactSets, NAMES_KEY, StoredContactSets,
        color_from_stored, read_names, store_value, stored_color,
    };
    use bevy::prelude::Color;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{AgentKey, Uuid};

    /// An agent key from a small integer.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(Uuid::from_u128(id))
    }

    /// Whether `set` holds `who`.
    fn holds(set: &ContactSet, who: AgentKey) -> bool {
        set.members().any(|member| member == who)
    }

    /// A model with `count` sets named `Set N`.
    fn with_sets(count: usize) -> Result<ContactSets, ContactSetRefusal> {
        let mut sets = ContactSets::default();
        for index in 0..count {
            sets.create_set(&format!("Set {index}"))?;
        }
        Ok(sets)
    }

    /// Creating, renaming and deleting a set, and the names that are refused.
    #[test]
    fn set_lifecycle_and_name_guards() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        sets.create_set("  Builders  ")?;
        assert_eq!(sets.set_count(), 1);
        assert!(sets.set("Builders").is_some(), "the name is trimmed");

        assert_eq!(
            sets.create_set("Builders"),
            Err(ContactSetRefusal::DuplicateName)
        );
        assert_eq!(sets.create_set("   "), Err(ContactSetRefusal::EmptyName));
        assert_eq!(
            sets.create_set(ALL_SETS_LABEL),
            Err(ContactSetRefusal::ReservedName)
        );
        assert_eq!(
            sets.create_set("all sets"),
            Err(ContactSetRefusal::ReservedName),
            "a reserved name is reserved whatever its case"
        );
        assert_eq!(
            sets.create_set(NAMES_KEY),
            Err(ContactSetRefusal::ReservedName)
        );

        sets.add_member("Builders", agent(1), "Alpha Resident")?;
        sets.rename_set("Builders", "Makers")?;
        assert!(sets.set("Builders").is_none());
        let renamed = sets.set("Makers").ok_or(ContactSetRefusal::UnknownSet)?;
        assert_eq!(renamed.name(), "Makers");
        assert!(holds(renamed, agent(1)), "a rename keeps the members");
        assert_eq!(sets.sets_of(agent(1)), ["Makers".to_owned()]);

        // Renaming to the name it already has is a no-op, not a duplicate.
        sets.rename_set("Makers", "Makers")?;
        assert_eq!(
            sets.rename_set("Nobody", "Makers"),
            Err(ContactSetRefusal::UnknownSet)
        );

        sets.remove_set("Makers")?;
        assert_eq!(sets.set_count(), 0);
        assert!(
            sets.sets_of(agent(1)).is_empty(),
            "deleting a set unfiles its members"
        );
        assert_eq!(
            sets.remove_set("Makers"),
            Err(ContactSetRefusal::UnknownSet)
        );
        Ok(())
    }

    /// Filing, un-filing and moving residents, and the null-agent guard.
    #[test]
    fn membership_moves_between_sets() -> Result<(), ContactSetRefusal> {
        let mut sets = with_sets(2)?;
        sets.add_member("Set 0", agent(1), "Alpha Resident")?;
        sets.add_member("Set 1", agent(1), "")?;
        assert_eq!(
            sets.sets_of(agent(1)),
            ["Set 0".to_owned(), "Set 1".to_owned()],
            "a resident may be in several sets, listed in name order"
        );
        assert_eq!(
            sets.label_of(agent(1)),
            Some("Alpha Resident"),
            "filing again without a name keeps the one already remembered"
        );

        sets.move_member("Set 0", "Set 1", agent(1))?;
        assert_eq!(sets.sets_of(agent(1)), ["Set 1".to_owned()]);

        sets.remove_member("Set 1", agent(1))?;
        assert!(!sets.is_filed(agent(1)));
        assert_eq!(
            sets.label_of(agent(1)),
            None,
            "the name memo follows the filing"
        );

        assert_eq!(
            sets.add_member("Set 0", AgentKey::from(Uuid::nil()), ""),
            Err(ContactSetRefusal::NullAgent)
        );
        assert_eq!(
            sets.add_member("Nowhere", agent(2), ""),
            Err(ContactSetRefusal::UnknownSet)
        );
        assert_eq!(
            sets.move_member("Set 0", "Nowhere", agent(2)),
            Err(ContactSetRefusal::UnknownSet),
            "a move to a set that does not exist must not unfile them"
        );
        Ok(())
    }

    /// The colour rule: the smallest set a resident is in wins, ties break by
    /// set name, and someone in no set has no colour of their own.
    #[test]
    fn the_smallest_set_colours_a_resident() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        sets.create_set("Big")?;
        sets.create_set("Small")?;
        sets.recolor_set("Big", Color::srgb(1.0, 0.0, 0.0))?;
        sets.recolor_set("Small", Color::srgb(0.0, 1.0, 0.0))?;
        for id in 1..=4 {
            sets.add_member("Big", agent(id), "")?;
        }
        sets.add_member("Small", agent(1), "")?;
        assert_eq!(sets.color_of(agent(1)), Some(Color::srgb(0.0, 1.0, 0.0)));
        assert_eq!(sets.color_of(agent(2)), Some(Color::srgb(1.0, 0.0, 0.0)));
        assert_eq!(sets.color_of(agent(9)), None);

        // Equal sizes: the name decides, so the answer never depends on hash
        // order.
        sets.create_set("Alpha")?;
        sets.recolor_set("Alpha", Color::srgb(0.0, 0.0, 1.0))?;
        sets.add_member("Alpha", agent(5), "")?;
        sets.create_set("Zulu")?;
        sets.recolor_set("Zulu", Color::srgb(1.0, 1.0, 0.0))?;
        sets.add_member("Zulu", agent(5), "")?;
        assert_eq!(sets.color_of(agent(5)), Some(Color::srgb(0.0, 0.0, 1.0)));
        Ok(())
    }

    /// A new set is given a colour of its own, so it tints the moment it exists
    /// (our divergence from the reference's default grey), and two made in a row
    /// differ.
    #[test]
    fn new_sets_get_distinct_colors() -> Result<(), ContactSetRefusal> {
        let sets = with_sets(3)?;
        let colors: Vec<Color> = sets.sets().map(ContactSet::color).collect();
        assert_eq!(colors.len(), 3);
        for (index, color) in colors.iter().enumerate() {
            assert!(
                !colors
                    .iter()
                    .enumerate()
                    .any(|(other, seen)| other != index && seen == color),
                "two new sets share a colour"
            );
        }
        Ok(())
    }

    /// A resolved name is mirrored for a view to read by, never written into the
    /// filing — a grid answers an id it cannot resolve with a placeholder, and
    /// persisting that would destroy the record of who was filed.
    #[test]
    fn resolved_names_are_mirrored_not_stored() -> Result<(), ContactSetRefusal> {
        let mut sets = with_sets(1)?;
        sets.add_member("Set 0", agent(1), "Alpha Resident")?;
        let revision = sets.revision();
        sets.dirty = false;

        sets.note_live_name(agent(1), "Alpha Renamed");
        assert_eq!(sets.label_of(agent(1)), Some("Alpha Renamed"));
        assert_ne!(sets.revision(), revision);
        assert!(!sets.dirty, "a resolution is not a change to the file");

        let settled = sets.revision();
        sets.note_live_name(agent(1), "Alpha Renamed");
        sets.note_live_name(agent(1), "");
        assert_eq!(sets.revision(), settled, "no news is not a change");

        // The stored name is what the filing surface knew, and stays.
        let stored = store_value(&sets);
        let names = stored.get(NAMES_KEY).ok_or(ContactSetRefusal::UnknownSet)?;
        assert!(
            names.to_string().contains("Alpha Resident"),
            "the file keeps the filed name: {names}"
        );
        Ok(())
    }

    /// The file round-trips through the reference's own layout — a top level
    /// keyed by set name, each with a `color` and a `friends` map.
    #[test]
    fn the_file_round_trips_in_the_reference_layout() -> Result<(), Box<dyn core::error::Error>> {
        let mut sets = ContactSets::default();
        sets.create_set("Builders")?;
        sets.recolor_set("Builders", Color::srgb(0.25, 0.5, 0.75))?;
        sets.add_member("Builders", agent(1), "Alpha Resident")?;
        let json = serde_json::to_string(&store_value(&sets))?;
        assert!(json.contains("\"Builders\""), "keyed by set name: {json}");
        assert!(
            json.contains("\"friends\""),
            "reference members map: {json}"
        );

        let read: StoredContactSets = super::read_store(&write_temp(&json)?);
        let builders = read.sets.get("Builders").ok_or("the set survived")?;
        assert_eq!(builders.member_count(), 1);
        assert!(holds(builders, agent(1)));
        assert_eq!(
            read.names.get(&agent(1)).map(String::as_str),
            Some("Alpha Resident")
        );
        // The colour survives the sRGB round trip.
        let read_back = stored_color(builders.color());
        let written = stored_color(Color::srgb(0.25, 0.5, 0.75));
        assert!(
            read_back
                .iter()
                .zip(written.iter())
                .all(|(left, right)| (left - right).abs() < 1.0e-6),
            "the colour survives: {read_back:?} vs {written:?}"
        );
        Ok(())
    }

    /// Write `contents` to a temporary file and return its path.
    fn write_temp(contents: &str) -> Result<std::path::PathBuf, Box<dyn core::error::Error>> {
        let path =
            std::env::temp_dir().join(format!("sl-client-contact-sets-{}.json", contents.len()));
        fs_err::write(&path, contents)?;
        Ok(path)
    }

    /// A file written by the reference loads: its internal keys are skipped
    /// rather than read as sets, and an entry that is not shaped like a set is
    /// dropped instead of failing the whole read.
    #[test]
    fn a_reference_file_loads() -> Result<(), Box<dyn core::error::Error>> {
        let json = r#"{
            "globalSettings": {"defaultColor": [0.5, 0.5, 0.5, 1.0]},
            "extraAvs": {"00000000-0000-0000-0000-000000000002": ""},
            "Pseudonyms": {"00000000-0000-0000-0000-000000000002": "Nickname"},
            "Builders": {
                "color": [1.0, 0.0, 0.0, 1.0],
                "notify": true,
                "friends": {"00000000-0000-0000-0000-000000000001": ""}
            },
            "Broken": 7
        }"#;
        let read = super::read_store(&write_temp(json)?);
        assert_eq!(read.sets.len(), 1, "only the one real set");
        let builders = read.sets.get("Builders").ok_or("the set was read")?;
        assert!(holds(builders, agent(1)));
        assert_eq!(builders.color(), color_from_stored([1.0, 0.0, 0.0, 1.0]));
        Ok(())
    }

    /// Member names that are not an id → name map are ignored rather than
    /// failing the read.
    #[test]
    fn malformed_member_names_are_ignored() {
        assert!(read_names(&serde_json::Value::Bool(true)).is_empty());
    }
}
