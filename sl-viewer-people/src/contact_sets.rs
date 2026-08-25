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
//! `ContactSets` is the whole model — the sets, their members, the per-account
//! file, and the by-agent index the tinting consumers read. Every surface that
//! changes something writes a `RequestContactSet` rather than touching the
//! sets (`apply_contact_set_requests` runs the guards), the same shape the
//! render exceptions ([`crate::avatar_render_settings`]) and the mute list
//! ([`crate::mutes`]) use. A refused request is logged with **why**
//! (`ContactSetRefusal`), and a refused *rename* additionally raises the
//! reference's own `RenameContactSetFailure` notification, since that one is a
//! user action that visibly did nothing.
//!
//! # The colour question: the smallest set wins
//!
//! A resident may be in several sets, so "what colour is this person" needs a
//! rule. `ContactSets::color_of` answers with the colour of the **smallest**
//! set they belong to — the reference's rule (`LGGContactSets::getFriendColor`),
//! and the one that reads right: the set with three people in it says more about
//! someone than the set with eighty. Ties break by set name, so the answer is
//! stable rather than hash-order.
//!
//! # Aliases: the other half of the feature
//!
//! The same store carries the reference's **pseudonyms**
//! (`viewer-contact-set-pseudonyms`): a name the user gives one resident, and
//! its special case, **display-name removal** (show me this person's legacy name
//! and not the display name they chose). Those are per resident rather than per
//! set, and they are not a panel feature: `apply_name_aliases` mirrors them
//! into the [name cache](crate::world_api::NameRecord) as a
//! [`NameAlias`], so every surface that resolves a
//! name — tags, the radar, tooltips, chat — shows the alias without knowing that
//! contact sets exist.
//!
//! An alias is shown in the reference's **quoted** form (`'Nickname'`), which is
//! the whole reason it can be trusted: a name in quotes is visibly the user's
//! own, never something the grid answered. Nothing aliased is ever written back
//! over the filed-name memo below, and a wire action still carries the grid's
//! name ([`crate::world_api::AvatarState::name_of`]).
//!
//! # What a set *does*, beyond its colour
//!
//! Three per-set behaviours ride along with the colour
//! (`viewer-contact-set-presence-extras`), each the reference's own field in the
//! same file:
//!
//! - **Notify** — announce this set's members coming and going even when the
//!   global friend online / offline notice is off (`ContactSets::notifies`,
//!   read by [`crate::people`]).
//! - **Sort by online status** — list this set online-first in the panel.
//! - **Per-set autoresponses** — a canned reply of this set's own for each of
//!   the three answering modes (`SetAutoresponseMode`), consulted by
//!   [`crate::presence`] *before* the global reply, so "my partner gets a
//!   different Unavailable message" works.
//!
//! Someone may be in several sets, so the reply lookup needs the same rule the
//! colour does, and uses it: `ContactSets::autoresponse_for` answers with the
//! **smallest** set that has one, ties broken by name.
//!
//! # Names
//!
//! A set outlives everyone's presence — most of its members are nowhere near you
//! most of the time — so, exactly as the render-exception store does, each
//! member's name is remembered as the surface that filed them knew it, and the
//! **live name cache** is read over it when it has an answer
//! (`refresh_contact_set_names`). A resolution never rewrites the stored name:
//! a grid answers an id it cannot resolve with a placeholder, and adopting that
//! would destroy the record of who was filed.
//!
//! # The file is the reference's shape
//!
//! The store is a per-account `contact_sets.json`, a sibling of the account
//! `settings.toml`, whose top level is keyed by **set name** with a `color` and
//! a `friends` map — the same layout Firestorm's `settings_friends_groups.xml`
//! uses, so a list exported from there ports across by transcription. The
//! reference's own internal keys (`globalSettings`, `extraAvs`) are recognised
//! and skipped rather than mistaken for sets, its `PSEUDONYMS_KEY` map is read
//! and written as the reference writes it, and the member-name memo above is our
//! addition under `NAMES_KEY`.
//!
//! Reference (Firestorm, read-only): `lggcontactsets`, `fspanelcontactsets`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use bevy::color::ColorToComponents as _;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{AgentKey, Uuid};
use tracing::{debug, info, warn};

use crate::notifications::ShowNotification;
use crate::settings::ViewerSettings;
use crate::world_api::FriendsModel;
use crate::world_api::{AvatarState, NameAlias};

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

/// The reference's own top-level key holding the per-resident aliases
/// (`id → alias`), read and written exactly as Firestorm writes it, so an alias
/// list transcribed from there works and one written here still does after a
/// trip through the reference. It doubles as the panel's third pseudo-set — the
/// listing of everyone who has an alias — which is why it is a reserved set
/// name.
pub(crate) const PSEUDONYMS_KEY: &str = "Pseudonyms";

/// The alias text the reference stores to mean "**remove** this resident's
/// display name" rather than "call them this" (`lggcontactsets`'s
/// `CS_PSEUDONYM`). It is a stored value rather than a flag because that is what
/// the file format has room for — one string per resident.
const DISPLAY_NAME_REMOVED: &str = "--- ---";

/// The top-level keys the **reference** file uses for its own bookkeeping. They
/// are not sets: a file written by Firestorm carries them, and reading one of
/// them as a set would invent a set named `globalSettings`.
///
/// `globalSettings` (the fallback colour for everyone in no set) and `extraAvs`
/// (which members are not friends) have no consumer here yet, so they are
/// skipped on the way in and not written back. (`PSEUDONYMS_KEY` used to be
/// one of them and is now read.)
const REFERENCE_INTERNAL_KEYS: &[&str] = &["globalSettings", "extraAvs"];

/// The names a set may not be given: our own file key, the reference's internal
/// keys, and the pseudo-sets the panel's set list shows above the real ones.
/// Compared case-insensitively — a set called `all sets` would be just as
/// confusing as `All Sets`.
pub(crate) const RESERVED_SET_NAMES: &[&str] = &[
    NAMES_KEY,
    "globalSettings",
    "extraAvs",
    PSEUDONYMS_KEY,
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

/// Which canned reply a per-set autoresponse overrides — the reference's
/// `ContactSetAutoresponseMode`. There is deliberately no entry for the *away*
/// or *blocked* replies: the reference has none either, and both are statements
/// about the user rather than about the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetAutoresponseMode {
    /// The Do Not Disturb (*Unavailable*) reply.
    Busy,
    /// The autorespond reply.
    Autorespond,
    /// The autorespond-to-non-friends reply.
    NonFriends,
}

/// One per-set canned reply: whether it overrides the global one at all, and
/// the text. Both are stored even when the override is off, so switching it back
/// on does not lose what was typed (the reference stores them the same way).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SetAutoresponse {
    /// Whether this set's text is used in place of the global reply.
    enabled: bool,
    /// The reply text.
    text: String,
}

impl SetAutoresponse {
    /// Whether this set's text overrides the global reply.
    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    /// The reply text, whether or not the override is on.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The text to actually answer with, or `None` when the override is off or
    /// blank — a blank override would silence the reply rather than change it,
    /// which is not what "use a custom reply for this set" means.
    fn override_text(&self) -> Option<&str> {
        (self.enabled && !self.text.is_empty()).then_some(self.text.as_str())
    }
}

/// One contact set: a name, a colour, the residents filed under it, and the
/// three behaviours the set carries.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContactSet {
    /// The set's name, as the user typed it — also its key in the file.
    name: String,
    /// The colour everything tinted by this set uses.
    color: Color,
    /// The residents in the set, ordered by id so the file is stable.
    members: BTreeSet<Uuid>,
    /// Whether this set's members are announced coming and going even when the
    /// global friend notice is off (the reference's `notify`).
    notify: bool,
    /// Whether the panel lists this set online-first (the reference's
    /// `sort_by_online_status`).
    sort_by_online_status: bool,
    /// This set's override of the Do Not Disturb reply.
    busy_reply: SetAutoresponse,
    /// This set's override of the autorespond reply.
    autorespond_reply: SetAutoresponse,
    /// This set's override of the autorespond-to-non-friends reply.
    non_friends_reply: SetAutoresponse,
}

impl ContactSet {
    /// An empty set of that name and colour, with every behaviour off — the
    /// shape both a fresh set and a read entry start from.
    fn new(name: String, color: Color) -> Self {
        Self {
            name,
            color,
            members: BTreeSet::new(),
            notify: false,
            sort_by_online_status: false,
            busy_reply: SetAutoresponse::default(),
            autorespond_reply: SetAutoresponse::default(),
            non_friends_reply: SetAutoresponse::default(),
        }
    }

    /// The set's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The set's colour.
    pub(crate) const fn color(&self) -> Color {
        self.color
    }

    /// Whether this set announces its members coming and going.
    pub(crate) const fn notify(&self) -> bool {
        self.notify
    }

    /// Whether the panel lists this set online-first.
    pub(crate) const fn sorts_by_online_status(&self) -> bool {
        self.sort_by_online_status
    }

    /// This set's override of one canned reply.
    pub(crate) const fn autoresponse(&self, mode: SetAutoresponseMode) -> &SetAutoresponse {
        match mode {
            SetAutoresponseMode::Busy => &self.busy_reply,
            SetAutoresponseMode::Autorespond => &self.autorespond_reply,
            SetAutoresponseMode::NonFriends => &self.non_friends_reply,
        }
    }

    /// This set's override of one canned reply, to write.
    const fn autoresponse_mut(&mut self, mode: SetAutoresponseMode) -> &mut SetAutoresponse {
        match mode {
            SetAutoresponseMode::Busy => &mut self.busy_reply,
            SetAutoresponseMode::Autorespond => &mut self.autorespond_reply,
            SetAutoresponseMode::NonFriends => &mut self.non_friends_reply,
        }
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

/// Why a `RequestContactSet` was refused — the guards, named so the caller can
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
    /// The user's alias per resident, as the reference stores it: the alias
    /// text, or [`DISPLAY_NAME_REMOVED`] for "show their legacy name only".
    /// Persisted under `PSEUDONYMS_KEY`. Independent of set membership — one
    /// may alias someone filed nowhere.
    pseudonyms: HashMap<AgentKey, String>,
    /// What the live name cache last answered for a member, mirrored here by
    /// `refresh_contact_set_names` so a view rebuilds off one revision instead
    /// of polling the name cache per row. Session state: never persisted.
    live_names: HashMap<AgentKey, String>,
    /// Bumped on every change, so a view rebuilds exactly when something moved.
    revision: u64,
    /// Bumped only when an **alias** moved. [`revision`](Self::revision) also
    /// advances when a member's name resolves, which is far too often to rebuild
    /// every transcript for; the alias mirror and the surfaces that re-read a
    /// drawn name gate on this instead.
    alias_revision: u64,
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

    /// The **alias** revision, advancing only when someone's alias was given,
    /// changed or cleared — what a surface that redraws whole panes of names
    /// (the chat transcript) gates on.
    pub(crate) const fn alias_revision(&self) -> u64 {
        self.alias_revision
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

    /// Whether any set `agent` is filed under asks for their comings and goings
    /// to be announced — the reference's `notifyForFriend`, read by the friend
    /// online / offline notice ([`crate::people`]) as the *second* way a notice
    /// can be enabled, beside the global toggle.
    pub(crate) fn notifies(&self, agent: AgentKey) -> bool {
        self.sets_of(agent)
            .iter()
            .filter_map(|name| self.sets.get(name))
            .any(ContactSet::notify)
    }

    /// The reply `agent`'s sets answer with in `mode`, if one of them overrides
    /// the global text: the **smallest** such set's, ties broken by set name —
    /// the same rule (and the same reason) as [`Self::color_of`], and the
    /// reference's own in `getAutoresponseForFriend`.
    pub(crate) fn autoresponse_for(
        &self,
        agent: AgentKey,
        mode: SetAutoresponseMode,
    ) -> Option<&str> {
        self.sets_of(agent)
            .iter()
            .filter_map(|name| self.sets.get(name))
            .filter(|set| set.autoresponse(mode).override_text().is_some())
            .min_by_key(|set| (set.member_count(), set.name.clone()))
            .and_then(|set| set.autoresponse(mode).override_text())
    }

    /// The best name known for `agent`: what the live name cache answered, else
    /// the name they were filed under, else `None` (the caller shows the id).
    ///
    /// Always the **grid's** answer — this is the name a filing remembers and a
    /// wire action carries. [`Self::shown_label_of`] is the one to draw.
    pub(crate) fn label_of(&self, agent: AgentKey) -> Option<&str> {
        self.live_names
            .get(&agent)
            .or_else(|| self.names.get(&agent))
            .map(String::as_str)
    }

    /// The label to **show** for `agent`: their alias when they have one, else
    /// [`Self::label_of`]. Display-name removal changes nothing here — the names
    /// this store knows are legacy names already.
    pub(crate) fn shown_label_of(&self, agent: AgentKey) -> Option<String> {
        self.shown_alias_of(agent)
            .or_else(|| self.label_of(agent).map(ToOwned::to_owned))
    }

    /// The text to show **in place of** `agent`'s name, for a surface that
    /// already holds a legacy name for them (a chat line's speaker, a friends
    /// row): the quoted alias, or `None` — display-name removal asks for the
    /// legacy name, which is the one such a surface already has.
    pub(crate) fn shown_alias_of(&self, agent: AgentKey) -> Option<String> {
        match self.alias_of(agent) {
            Some(NameAlias::Pseudonym(shown)) => Some(shown),
            Some(NameAlias::LegacyOnly) | None => None,
        }
    }

    /// The user's alias for `agent`, in the form the name cache takes.
    pub(crate) fn alias_of(&self, agent: AgentKey) -> Option<NameAlias> {
        let stored = self.pseudonyms.get(&agent)?;
        if stored == DISPLAY_NAME_REMOVED {
            return Some(NameAlias::LegacyOnly);
        }
        Some(NameAlias::Pseudonym(quoted(stored)))
    }

    /// Whether `agent` has any alias at all (either kind) — what the panel's
    /// *Rem Alias…* button acts on.
    pub(crate) fn has_alias(&self, agent: AgentKey) -> bool {
        self.pseudonyms.contains_key(&agent)
    }

    /// Whether `agent`'s display name is the one being suppressed, so the
    /// button that would suppress it again is not offered.
    pub(crate) fn has_display_name_removed(&self, agent: AgentKey) -> bool {
        self.pseudonyms
            .get(&agent)
            .is_some_and(|stored| stored == DISPLAY_NAME_REMOVED)
    }

    /// Everyone the user has given an alias, in id order — the population of the
    /// panel's *Pseudonyms* pseudo-set.
    pub(crate) fn everyone_aliased(&self) -> Vec<AgentKey> {
        let mut agents: Vec<AgentKey> = self.pseudonyms.keys().copied().collect();
        agents.sort_unstable_by_key(AgentKey::uuid);
        agents
    }

    /// Every alias, as the name cache takes them — what
    /// `apply_name_aliases` mirrors.
    fn aliases(&self) -> HashMap<AgentKey, NameAlias> {
        self.pseudonyms
            .keys()
            .filter_map(|agent| self.alias_of(*agent).map(|alias| (*agent, alias)))
            .collect()
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
        let _replaced = self.sets.insert(name.clone(), ContactSet::new(name, color));
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

    /// Turn a set's online / offline announcement on or off.
    pub(crate) fn set_notify(&mut self, name: &str, notify: bool) -> Result<(), ContactSetRefusal> {
        let Some(set) = self.sets.get_mut(name) else {
            return Err(ContactSetRefusal::UnknownSet);
        };
        if set.notify == notify {
            return Ok(());
        }
        set.notify = notify;
        self.touch();
        Ok(())
    }

    /// Turn a set's online-first panel ordering on or off.
    pub(crate) fn set_sort_by_online_status(
        &mut self,
        name: &str,
        sort: bool,
    ) -> Result<(), ContactSetRefusal> {
        let Some(set) = self.sets.get_mut(name) else {
            return Err(ContactSetRefusal::UnknownSet);
        };
        if set.sort_by_online_status == sort {
            return Ok(());
        }
        set.sort_by_online_status = sort;
        self.touch();
        Ok(())
    }

    /// Set a set's override of one canned reply. The text is kept whether or not
    /// the override is on, so turning it back on restores what was typed.
    pub(crate) fn set_autoresponse(
        &mut self,
        name: &str,
        mode: SetAutoresponseMode,
        enabled: bool,
        text: &str,
    ) -> Result<(), ContactSetRefusal> {
        let Some(set) = self.sets.get_mut(name) else {
            return Err(ContactSetRefusal::UnknownSet);
        };
        let reply = set.autoresponse_mut(mode);
        if reply.enabled == enabled && reply.text == text {
            return Ok(());
        }
        reply.enabled = enabled;
        text.clone_into(&mut reply.text);
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

    /// Give `agent` an alias — the name the user would rather see for them,
    /// remembering `name` as what the grid calls them so an aliased person who
    /// is in no set is still identifiable in the panel.
    pub(crate) fn set_pseudonym(
        &mut self,
        agent: AgentKey,
        alias: &str,
        name: &str,
    ) -> Result<(), ContactSetRefusal> {
        if agent.uuid().is_nil() {
            return Err(ContactSetRefusal::NullAgent);
        }
        let alias = alias.trim();
        if alias.is_empty() {
            return Err(ContactSetRefusal::EmptyName);
        }
        // The removal marker is a stored value, not a name anyone may type: an
        // alias of "--- ---" would silently mean something else entirely.
        if alias == DISPLAY_NAME_REMOVED {
            return Err(ContactSetRefusal::ReservedName);
        }
        self.store_pseudonym(agent, alias.to_owned(), name);
        Ok(())
    }

    /// Suppress `agent`'s chosen display name, leaving their legacy name — the
    /// reference's `removeDisplayName`, stored as the marker alias.
    pub(crate) fn remove_display_name(
        &mut self,
        agent: AgentKey,
        name: &str,
    ) -> Result<(), ContactSetRefusal> {
        if agent.uuid().is_nil() {
            return Err(ContactSetRefusal::NullAgent);
        }
        self.store_pseudonym(agent, DISPLAY_NAME_REMOVED.to_owned(), name);
        Ok(())
    }

    /// Drop `agent`'s alias, whichever kind it was: they go back to being called
    /// what the grid calls them.
    pub(crate) fn clear_pseudonym(&mut self, agent: AgentKey) -> Result<(), ContactSetRefusal> {
        if self.pseudonyms.remove(&agent).is_some() {
            self.alias_revision = self.alias_revision.wrapping_add(1);
            self.touch();
        }
        Ok(())
    }

    /// Record an alias and the grid name that went with it.
    fn store_pseudonym(&mut self, agent: AgentKey, stored: String, name: &str) {
        let changed = self.pseudonyms.get(&agent) != Some(&stored);
        if changed {
            let _previous = self.pseudonyms.insert(agent, stored);
            self.alias_revision = self.alias_revision.wrapping_add(1);
        }
        let renamed = !name.is_empty() && self.names.get(&agent).map(String::as_str) != Some(name);
        if renamed {
            let _previous = self.names.insert(agent, name.to_owned());
        }
        if changed || renamed {
            self.touch();
        }
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
        // The name memos exist to label people the store knows — filed in a set
        // or given an alias — so they follow both. Bound as locals so each
        // `retain` is a disjoint field borrow rather than a second borrow of the
        // whole model.
        let filed = &self.by_agent;
        let aliased = &self.pseudonyms;
        let known = |agent: &AgentKey| filed.contains_key(agent) || aliased.contains_key(agent);
        self.names.retain(|agent, _name| known(agent));
        self.live_names.retain(|agent, _name| known(agent));
        self.revision = self.revision.wrapping_add(1);
    }
}

/// An alias as it is shown: in single quotes, the reference's own form
/// (`LGGContactSets::getPseudonym`). The quotes are the feature's honesty — a
/// name in quotes is visibly the user's own and never the grid's answer.
fn quoted(alias: &str) -> String {
    format!("'{alias}'")
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
    /// Turn a set's online / offline announcement on or off.
    SetNotify {
        /// The set.
        name: String,
        /// Whether its members' comings and goings are announced.
        notify: bool,
    },
    /// Turn a set's online-first panel ordering on or off.
    SetSortByOnlineStatus {
        /// The set.
        name: String,
        /// Whether the panel lists it online-first.
        sort: bool,
    },
    /// Set a set's override of one canned reply.
    SetAutoresponse {
        /// The set.
        name: String,
        /// Which reply is overridden.
        mode: SetAutoresponseMode,
        /// Whether the override is on.
        enabled: bool,
        /// The reply text (kept even while the override is off).
        text: String,
    },
    /// File a resident under a set.
    Add {
        /// The set to file them under.
        set: String,
        /// The resident.
        agent: AgentKey,
        /// The best name the requesting surface knows for them (empty when it
        /// knows none — `refresh_contact_set_names` fills it in later).
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
    /// Give a resident an alias, shown in place of their name everywhere.
    SetPseudonym {
        /// The resident.
        agent: AgentKey,
        /// The alias, as the user typed it (quoted only when shown).
        alias: String,
        /// The best name the requesting surface knows for them, so an aliased
        /// person is still identifiable (empty when it knows none).
        name: String,
    },
    /// Show a resident's legacy name only, suppressing the display name they
    /// chose.
    RemoveDisplayName {
        /// The resident.
        agent: AgentKey,
        /// The best name the requesting surface knows for them.
        name: String,
    },
    /// Drop a resident's alias, whichever kind it was.
    ClearPseudonym {
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
pub struct ContactSetsPlugin;

impl Plugin for ContactSetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContactSets>()
            .add_message::<RequestContactSet>()
            .add_systems(
                Update,
                (
                    load_contact_sets,
                    apply_contact_set_requests,
                    apply_name_aliases,
                    refresh_contact_set_names,
                    flush_contact_sets,
                )
                    .chain(),
            );
    }
}

/// Turn each `RequestContactSet` into a change to the sets, after the guards.
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
            RequestContactSet::SetNotify { name, notify } => sets.set_notify(name, *notify),
            RequestContactSet::SetSortByOnlineStatus { name, sort } => {
                sets.set_sort_by_online_status(name, *sort)
            }
            RequestContactSet::SetAutoresponse {
                name,
                mode,
                enabled,
                text,
            } => sets.set_autoresponse(name, *mode, *enabled, text),
            RequestContactSet::Add { set, agent, name } => sets.add_member(set, *agent, name),
            RequestContactSet::RemoveMember { set, agent } => sets.remove_member(set, *agent),
            RequestContactSet::Move { from, to, agent } => sets.move_member(from, to, *agent),
            RequestContactSet::SetPseudonym { agent, alias, name } => {
                sets.set_pseudonym(*agent, alias, name)
            }
            RequestContactSet::RemoveDisplayName { agent, name } => {
                sets.remove_display_name(*agent, name)
            }
            RequestContactSet::ClearPseudonym { agent } => sets.clear_pseudonym(*agent),
        };
        match outcome {
            Ok(()) => debug!(?request, "contact sets changed"),
            Err(refusal) => debug!(?request, ?refusal, "refusing a contact-set change"),
        }
    }
}

/// Mirror the user's aliases into the [name cache](AvatarState), so every
/// surface that draws a name shows them. The **one** hook the feature has: it
/// runs when the store moves, and the cache folds an alias into a record it
/// creates later, so an avatar first seen after the alias was given is aliased
/// too.
pub(crate) fn apply_name_aliases(
    sets: Res<ContactSets>,
    avatars: Option<ResMut<AvatarState>>,
    friends: Option<ResMut<FriendsModel>>,
    mut mirrored: Local<Option<u64>>,
) {
    if *mirrored == Some(sets.alias_revision()) {
        return;
    }
    let aliases = sets.aliases();
    if let Some(mut avatars) = avatars {
        avatars.set_name_aliases(aliases.clone());
        // Only once the name cache has it: it is the store every drawn name
        // reads, and a half-applied alias would show in one place and not
        // another.
        *mirrored = Some(sets.alias_revision());
    }
    // The friends list keeps its own resolved names (it lists people who are
    // nowhere near the viewer), so the alias is mirrored there too.
    if let Some(mut friends) = friends {
        friends.set_name_aliases(
            aliases
                .into_iter()
                .filter_map(|(agent, alias)| match alias {
                    NameAlias::Pseudonym(shown) => Some((agent, shown)),
                    // A friends-list row shows a legacy name already, which is
                    // exactly what display-name removal asks for.
                    NameAlias::LegacyOnly => None,
                })
                .collect(),
        );
    }
}

/// Keep the name cache warm for every resident the store knows — filed in a set
/// or given an alias — and mirror what it answers, so a view reads live names
/// off one revision. Throttled: a name nobody can answer must not be
/// re-requested every frame.
pub(crate) fn refresh_contact_set_names(
    mut sets: ResMut<ContactSets>,
    avatars: Option<ResMut<AvatarState>>,
    time: Res<Time>,
    mut last_run: Local<Option<f64>>,
) {
    let Some(mut avatars) = avatars else {
        return;
    };
    if sets.by_agent.is_empty() && sets.pseudonyms.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if last_run.is_some_and(|last| now - last < NAME_REFRESH_SECONDS) {
        return;
    }
    *last_run = Some(now);
    let agents: Vec<AgentKey> = sets
        .by_agent
        .keys()
        .chain(
            sets.pseudonyms
                .keys()
                .filter(|agent| !sets.by_agent.contains_key(*agent)),
        )
        .copied()
        .collect();
    for agent in agents {
        // The **grid's** answer, not the shown one: mirroring an alias here
        // would file the user's own name for someone as what they are called.
        let resolved = avatars
            .name_record(agent)
            .and_then(crate::world_api::NameRecord::grid_name)
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

/// One set as it is written: the reference's own members of a set entry, name
/// for name. `friends` is a map of id → `""` rather than a list because that is
/// what the reference writes (an LLSD map used as a set), and the point of the
/// layout is that a transcribed file loads.
///
/// Every field is `#[serde(default)]` because a set written by an older build
/// (or hand-transcribed) has only some of them, and a missing behaviour means
/// "off" rather than a failed read.
#[expect(
    clippy::struct_excessive_bools,
    reason = "this struct is the file format, not a model: one field per key the reference \
              writes into a set entry. Folding the five flags into enums or sub-structs would \
              change the on-disk layout, which is the whole point of the type — a set \
              configured in Firestorm has to load here and back"
)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredSet {
    /// The set's colour as sRGB + alpha, the reference's `LLColor4` order.
    #[serde(default)]
    color: Option<[f32; 4]>,
    /// The members, as a map of id string → `""`.
    #[serde(default)]
    friends: BTreeMap<String, String>,
    /// Whether the set announces its members coming and going.
    #[serde(default)]
    notify: bool,
    /// Whether the panel lists the set online-first.
    #[serde(default)]
    sort_by_online_status: bool,
    /// Whether the Do Not Disturb reply is overridden for this set.
    #[serde(default)]
    autoresponse_busy_enabled: bool,
    /// This set's Do Not Disturb reply.
    #[serde(default)]
    autoresponse_busy: String,
    /// Whether the autorespond reply is overridden for this set.
    #[serde(default)]
    autoresponse_mode_enabled: bool,
    /// This set's autorespond reply.
    #[serde(default)]
    autoresponse_mode: String,
    /// Whether the autorespond-to-non-friends reply is overridden for this set.
    #[serde(default)]
    autoresponse_nonfriends_enabled: bool,
    /// This set's autorespond-to-non-friends reply.
    #[serde(default)]
    autoresponse_nonfriends: String,
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
    if stored.sets.is_empty() && stored.names.is_empty() && stored.pseudonyms.is_empty() {
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
    let aliased = !stored.pseudonyms.is_empty();
    for (agent, alias) in stored.pseudonyms {
        let _previous = sets.pseudonyms.entry(agent).or_insert(alias);
    }
    if aliased {
        // A load is the first time the aliases exist this session, so the mirror
        // has to run — the name cache is empty of them.
        sets.alias_revision = sets.alias_revision.wrapping_add(1);
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
    /// The user's aliases, as stored (the marker included).
    pseudonyms: HashMap<AgentKey, String>,
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
        if key == PSEUDONYMS_KEY {
            read.pseudonyms = read_names(&value);
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
                let mut set = ContactSet::new(key.clone(), color);
                set.members = members;
                set.notify = stored.notify;
                set.sort_by_online_status = stored.sort_by_online_status;
                set.busy_reply = SetAutoresponse {
                    enabled: stored.autoresponse_busy_enabled,
                    text: stored.autoresponse_busy,
                };
                set.autorespond_reply = SetAutoresponse {
                    enabled: stored.autoresponse_mode_enabled,
                    text: stored.autoresponse_mode,
                };
                set.non_friends_reply = SetAutoresponse {
                    enabled: stored.autoresponse_nonfriends_enabled,
                    text: stored.autoresponse_nonfriends,
                };
                let _replaced = read.sets.insert(key, set);
            }
            Err(error) => warn!(key, %error, "skipping a contact-sets entry that is not a set"),
        }
    }
    info!(count = read.sets.len(), path = %path.display(), "loaded the contact sets");
    read
}

/// Read an id → text map (the remembered member names, and the aliases beside
/// them), ignoring anything that is not one.
fn read_names(value: &serde_json::Value) -> HashMap<AgentKey, String> {
    let Ok(names) = serde_json::from_value::<BTreeMap<String, String>>(value.clone()) else {
        warn!("skipping a malformed contact-set id → name map");
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
            notify: set.notify,
            sort_by_online_status: set.sort_by_online_status,
            autoresponse_busy_enabled: set.busy_reply.enabled,
            autoresponse_busy: set.busy_reply.text.clone(),
            autoresponse_mode_enabled: set.autorespond_reply.enabled,
            autoresponse_mode: set.autorespond_reply.text.clone(),
            autoresponse_nonfriends_enabled: set.non_friends_reply.enabled,
            autoresponse_nonfriends: set.non_friends_reply.text.clone(),
        };
        if let Ok(value) = serde_json::to_value(stored) {
            let _replaced = top.insert(set.name.clone(), value);
        }
    }
    insert_name_map(&mut top, NAMES_KEY, &sets.names);
    if !sets.pseudonyms.is_empty() {
        insert_name_map(&mut top, PSEUDONYMS_KEY, &sets.pseudonyms);
    }
    top
}

/// Write one id → text map into the file's top level under `key`.
fn insert_name_map(
    top: &mut BTreeMap<String, serde_json::Value>,
    key: &str,
    names: &HashMap<AgentKey, String>,
) {
    let stored: BTreeMap<String, String> = names
        .iter()
        .map(|(agent, name)| (agent.uuid().to_string(), name.clone()))
        .collect();
    if let Ok(value) = serde_json::to_value(stored) {
        let _replaced = top.insert(key.to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ALL_SETS_LABEL, ContactSet, ContactSetRefusal, ContactSets, NAMES_KEY, NameAlias,
        SetAutoresponseMode, StoredContactSets, color_from_stored, read_names, store_value,
        stored_color,
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
    /// rather than read as sets (its aliases being read *as* aliases), and an
    /// entry that is not shaped like a set is dropped instead of failing the
    /// whole read.
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
        assert_eq!(
            read.pseudonyms.get(&agent(2)).map(String::as_str),
            Some("Nickname"),
            "an alias filed in Firestorm is an alias here"
        );
        Ok(())
    }

    /// Member names that are not an id → name map are ignored rather than
    /// failing the read.
    #[test]
    fn malformed_member_names_are_ignored() {
        assert!(read_names(&serde_json::Value::Bool(true)).is_empty());
    }

    /// A per-set reply overrides the global one only when it is switched on and
    /// non-blank, and the **smallest** set carrying one wins — the same rule the
    /// colour uses, and the reference's own in `getAutoresponseForFriend`.
    #[test]
    fn the_smallest_set_with_a_reply_answers() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        sets.create_set("Big")?;
        sets.create_set("Small")?;
        for id in 1..=4 {
            sets.add_member("Big", agent(id), "")?;
        }
        sets.add_member("Small", agent(1), "")?;

        // Nothing configured: no override at all.
        assert_eq!(
            sets.autoresponse_for(agent(1), SetAutoresponseMode::Busy),
            None
        );

        sets.set_autoresponse("Big", SetAutoresponseMode::Busy, true, "from the big set")?;
        assert_eq!(
            sets.autoresponse_for(agent(1), SetAutoresponseMode::Busy),
            Some("from the big set"),
            "the only set with a reply answers, whatever its size"
        );

        sets.set_autoresponse(
            "Small",
            SetAutoresponseMode::Busy,
            true,
            "from the small set",
        )?;
        assert_eq!(
            sets.autoresponse_for(agent(1), SetAutoresponseMode::Busy),
            Some("from the small set"),
            "the smaller set is the more specific answer"
        );
        assert_eq!(
            sets.autoresponse_for(agent(2), SetAutoresponseMode::Busy),
            Some("from the big set"),
            "someone only in the big set still gets its reply"
        );

        // A mode with no override falls through to the global reply (`None`).
        assert_eq!(
            sets.autoresponse_for(agent(1), SetAutoresponseMode::Autorespond),
            None
        );

        // Switched off, or blank, is not an override — the global reply stands
        // rather than the sender hearing nothing.
        sets.set_autoresponse(
            "Small",
            SetAutoresponseMode::Busy,
            false,
            "from the small set",
        )?;
        assert_eq!(
            sets.autoresponse_for(agent(1), SetAutoresponseMode::Busy),
            Some("from the big set"),
            "an override switched off falls back to the next set"
        );
        assert_eq!(
            sets.set("Small").map(|set| set
                .autoresponse(SetAutoresponseMode::Busy)
                .text()
                .to_owned()),
            Some("from the small set".to_owned()),
            "the text is kept, so switching it back on restores what was typed"
        );
        sets.set_autoresponse("Big", SetAutoresponseMode::Busy, true, "")?;
        assert_eq!(
            sets.autoresponse_for(agent(1), SetAutoresponseMode::Busy),
            None,
            "a blank override is not a reply"
        );
        Ok(())
    }

    /// Any set the resident is in asking to notify is enough — the reference's
    /// `notifyForFriend`, which is an *or* across their sets.
    #[test]
    fn one_notifying_set_is_enough() -> Result<(), ContactSetRefusal> {
        let mut sets = with_sets(2)?;
        sets.add_member("Set 0", agent(1), "")?;
        sets.add_member("Set 1", agent(1), "")?;
        sets.add_member("Set 0", agent(2), "")?;
        assert!(!sets.notifies(agent(1)), "off by default");

        sets.set_notify("Set 1", true)?;
        assert!(sets.notifies(agent(1)));
        assert!(
            !sets.notifies(agent(2)),
            "someone in the other set alone is not announced"
        );
        assert!(!sets.notifies(agent(9)), "nor is someone in no set");

        sets.set_notify("Set 1", false)?;
        assert!(!sets.notifies(agent(1)));
        assert_eq!(
            sets.set_notify("Nowhere", true),
            Err(ContactSetRefusal::UnknownSet)
        );
        Ok(())
    }

    /// The three behaviours round-trip through the reference's own field names,
    /// and a set written without them (an older file, or one transcribed by
    /// hand) reads as all-off rather than failing.
    #[test]
    fn the_behaviours_round_trip_in_the_reference_layout() -> Result<(), Box<dyn core::error::Error>>
    {
        let mut sets = ContactSets::default();
        sets.create_set("Builders")?;
        sets.set_notify("Builders", true)?;
        sets.set_sort_by_online_status("Builders", true)?;
        sets.set_autoresponse("Builders", SetAutoresponseMode::Busy, true, "busy text")?;
        sets.set_autoresponse(
            "Builders",
            SetAutoresponseMode::Autorespond,
            false,
            "mode text",
        )?;
        sets.set_autoresponse(
            "Builders",
            SetAutoresponseMode::NonFriends,
            true,
            "non-friends text",
        )?;
        let json = serde_json::to_string(&store_value(&sets))?;
        for key in [
            "\"notify\"",
            "\"sort_by_online_status\"",
            "\"autoresponse_busy_enabled\"",
            "\"autoresponse_busy\"",
            "\"autoresponse_mode\"",
            "\"autoresponse_nonfriends\"",
        ] {
            assert!(
                json.contains(key),
                "the reference key {key} is written: {json}"
            );
        }

        let read = super::read_store(&write_temp(&json)?);
        let builders = read.sets.get("Builders").ok_or("the set survived")?;
        assert!(builders.notify());
        assert!(builders.sorts_by_online_status());
        let busy = builders.autoresponse(SetAutoresponseMode::Busy);
        assert!(busy.enabled());
        assert_eq!(busy.text(), "busy text");
        let mode = builders.autoresponse(SetAutoresponseMode::Autorespond);
        assert!(
            !mode.enabled(),
            "the switch survives independently of the text"
        );
        assert_eq!(mode.text(), "mode text");
        assert_eq!(
            builders
                .autoresponse(SetAutoresponseMode::NonFriends)
                .text(),
            "non-friends text"
        );

        // A set entry with none of the behaviours (what an older file holds) is
        // all-off, not a failed read.
        let bare = super::read_store(&write_temp(
            r#"{"Bare": {"color": [1.0, 1.0, 1.0, 1.0], "friends": {}}}"#,
        )?);
        let bare = bare.sets.get("Bare").ok_or("the bare set was read")?;
        assert!(!bare.notify());
        assert!(!bare.sorts_by_online_status());
        assert!(!bare.autoresponse(SetAutoresponseMode::Busy).enabled());
        Ok(())
    }

    /// An alias is shown quoted (never mistakable for the grid's answer), is
    /// per resident rather than per set, and replaces itself rather than
    /// stacking.
    #[test]
    fn an_alias_is_quoted_and_replaces_itself() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        sets.set_pseudonym(agent(1), "  Neighbour  ", "Alpha Resident")?;
        assert_eq!(
            sets.alias_of(agent(1)),
            Some(NameAlias::Pseudonym("'Neighbour'".to_owned())),
            "the alias is trimmed and quoted"
        );
        assert_eq!(
            sets.shown_alias_of(agent(1)).as_deref(),
            Some("'Neighbour'")
        );
        assert_eq!(
            sets.shown_label_of(agent(1)).as_deref(),
            Some("'Neighbour'"),
            "the panel lists an aliased person under the alias"
        );
        assert!(sets.has_alias(agent(1)));
        assert!(!sets.has_display_name_removed(agent(1)));
        assert_eq!(
            sets.label_of(agent(1)),
            Some("Alpha Resident"),
            "the grid's name is remembered beside the alias, not replaced by it"
        );

        sets.set_pseudonym(agent(1), "Neighbour Two", "")?;
        assert_eq!(
            sets.shown_alias_of(agent(1)).as_deref(),
            Some("'Neighbour Two'")
        );
        assert_eq!(sets.everyone_aliased(), [agent(1)]);

        sets.clear_pseudonym(agent(1))?;
        assert_eq!(sets.alias_of(agent(1)), None);
        assert!(sets.everyone_aliased().is_empty());
        assert_eq!(
            sets.label_of(agent(1)),
            None,
            "the name memo follows the alias, as it follows a filing"
        );
        Ok(())
    }

    /// Display-name removal is the reference's marker alias: it asks for the
    /// legacy name rather than naming anyone, so it is *an* alias but not one to
    /// show, and the marker itself may not be typed as one.
    #[test]
    fn display_name_removal_is_a_marker_not_a_name() -> Result<(), ContactSetRefusal> {
        let mut sets = ContactSets::default();
        sets.remove_display_name(agent(1), "Alpha Resident")?;
        assert_eq!(sets.alias_of(agent(1)), Some(NameAlias::LegacyOnly));
        assert!(sets.has_alias(agent(1)));
        assert!(sets.has_display_name_removed(agent(1)));
        assert_eq!(
            sets.shown_alias_of(agent(1)),
            None,
            "there is no alias text to show — the legacy name is the point"
        );
        assert_eq!(
            sets.shown_label_of(agent(1)).as_deref(),
            Some("Alpha Resident")
        );

        assert_eq!(
            sets.set_pseudonym(agent(2), super::DISPLAY_NAME_REMOVED, ""),
            Err(ContactSetRefusal::ReservedName),
            "the marker means something else entirely and may not be typed"
        );
        assert_eq!(
            sets.set_pseudonym(agent(2), "   ", ""),
            Err(ContactSetRefusal::EmptyName)
        );
        assert_eq!(
            sets.set_pseudonym(AgentKey::from(Uuid::nil()), "Nobody", ""),
            Err(ContactSetRefusal::NullAgent)
        );
        Ok(())
    }

    /// An alias moves only the alias revision, so the surfaces that redraw whole
    /// panes of names are not rebuilt every time a member's name resolves.
    #[test]
    fn only_an_alias_moves_the_alias_revision() -> Result<(), ContactSetRefusal> {
        let mut sets = with_sets(1)?;
        sets.add_member("Set 0", agent(1), "Alpha Resident")?;
        let aliases = sets.alias_revision();
        sets.note_live_name(agent(1), "Alpha Renamed");
        assert_eq!(
            sets.alias_revision(),
            aliases,
            "a resolved name is not an alias change"
        );

        sets.set_pseudonym(agent(1), "Neighbour", "")?;
        assert_ne!(sets.alias_revision(), aliases);
        let after = sets.alias_revision();
        sets.set_pseudonym(agent(1), "Neighbour", "")?;
        assert_eq!(sets.alias_revision(), after, "no news is not a change");
        Ok(())
    }

    /// The aliases round-trip through the reference's own `Pseudonyms` key —
    /// unquoted, since the quotes are how one is *shown*, not how it is stored.
    #[test]
    fn aliases_round_trip_through_the_reference_key() -> Result<(), Box<dyn core::error::Error>> {
        let mut sets = ContactSets::default();
        sets.set_pseudonym(agent(1), "Neighbour", "Alpha Resident")?;
        sets.remove_display_name(agent(2), "Beta Resident")?;
        let json = serde_json::to_string(&store_value(&sets))?;
        assert!(json.contains("\"Pseudonyms\""), "the reference key: {json}");
        assert!(
            json.contains("\"Neighbour\""),
            "stored as typed, not quoted: {json}"
        );

        let read = super::read_store(&write_temp(&json)?);
        assert_eq!(
            read.pseudonyms.get(&agent(1)).map(String::as_str),
            Some("Neighbour")
        );
        assert_eq!(
            read.pseudonyms.get(&agent(2)).map(String::as_str),
            Some(super::DISPLAY_NAME_REMOVED),
            "display-name removal survives as the marker it is"
        );
        Ok(())
    }
}
