//! Experiences: what a script compiled into one may ask of an avatar once,
//! for every script of the experience, and the key-value store the
//! experience keeps on the grid (`llRequestExperiencePermissions`,
//! `llCreateKeyValue` and their family).
//!
//! The experience is the grid's, never the script's: which experience a
//! script was compiled into, whether it may run where the script is, which
//! avatars have allowed or blocked it and the key-value store are all the
//! host's ([`Host::experience`](crate::vm::Host::experience) and the methods
//! beside it). What the runtime keeps is the script's own share — the grant
//! an answer gives it, which lapses like any other — and the rules the
//! library answers by.
//!
//! **What is measured.** None of our avatars can save a script into an
//! experience (creating one needs a Premium account), so what aditi was
//! measured answering is a script in **no** experience (book chapter
//! `gridspec/lsl.md`, § Experiences and the key-value store; probes
//! `experience/no-experience`, `experience/sit-on-link` and
//! `experience/unserved`): every request denied with
//! [`ExperienceError::NoExperience`], every key-value call answered `"0,5"`,
//! the error texts, the details of a public experience and of an id no
//! experience has. A script *in* an experience is modelled on Linden Lab's
//! documentation instead — the tooltips of the library table it serves and
//! the wiki's pages for each function — and every such rule says so where it
//! is written, so it can be held to aditi once an avatar can save into an
//! experience (`gridspec-lsl-in-experience`).

use std::collections::BTreeMap;

use crate::value::NULL_KEY;
use crate::vm::permissions::{
    PERMISSION_ATTACH, PERMISSION_CONTROL_CAMERA, PERMISSION_TAKE_CONTROLS, PERMISSION_TELEPORT,
    PERMISSION_TRACK_CAMERA, PERMISSION_TRIGGER_ANIMATION,
};

/// What an experience grant holds: the permissions
/// `llRequestExperiencePermissions` stands for (the wiki's list —
/// unmeasured; the library table's tooltip names none).
pub const EXPERIENCE_PERMISSIONS: i32 = PERMISSION_TAKE_CONTROLS
    | PERMISSION_TRIGGER_ANIMATION
    | PERMISSION_ATTACH
    | PERMISSION_TRACK_CAMERA
    | PERMISSION_CONTROL_CAMERA
    | PERMISSION_TELEPORT;

/// The most bytes a key of the key-value store may have (the wiki's
/// specification since 2016; unmeasured).
pub const MAX_KEY_BYTES: usize = 1011;

/// The most bytes a value of the key-value store may have (the wiki's
/// specification since 2016, for Mono and LSO alike; unmeasured).
pub const MAX_VALUE_BYTES: usize = 4095;

/// The store's quota in bytes, as `llDataSizeKeyValue` answers it: the
/// wiki's 128 MiB (unmeasured).
pub const QUOTA_BYTES: usize = 128 * 1024 * 1024;

/// The longest answer `llKeysKeyValue` gives: the flag and 4096 characters of
/// keys (the wiki; unmeasured).
pub const MAX_KEYS_ANSWER: usize = 4097;

/// An experience error, as `experience_permissions_denied`, a key-value
/// answer and an experience's state carry it — the `XP_ERROR_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExperienceError {
    /// `XP_ERROR_NONE` (0).
    None,
    /// `XP_ERROR_THROTTLED` (1): too many recent calls.
    Throttled,
    /// `XP_ERROR_EXPERIENCES_DISABLED` (2): the region allows no experience.
    ExperiencesDisabled,
    /// `XP_ERROR_INVALID_PARAMETERS` (3): a string too big for the store.
    InvalidParameters,
    /// `XP_ERROR_NOT_PERMITTED` (4): the avatar blocked the experience or
    /// refused the request.
    NotPermitted,
    /// `XP_ERROR_NO_EXPERIENCE` (5): the script is in no experience.
    NoExperience,
    /// `XP_ERROR_NOT_FOUND` (6): the simulator could not check the
    /// experience.
    NotFound,
    /// `XP_ERROR_INVALID_EXPERIENCE` (7): no experience has the id.
    InvalidExperience,
    /// `XP_ERROR_EXPERIENCE_DISABLED` (8): its owner disabled it.
    ExperienceDisabled,
    /// `XP_ERROR_EXPERIENCE_SUSPENDED` (9): Linden Lab suspended it.
    ExperienceSuspended,
    /// `XP_ERROR_UNKNOWN_ERROR` (10).
    Unknown,
    /// `XP_ERROR_QUOTA_EXCEEDED` (11): the store is full.
    QuotaExceeded,
    /// `XP_ERROR_STORE_DISABLED` (12): the region's store is off.
    StoreDisabled,
    /// `XP_ERROR_STORAGE_EXCEPTION` (13): the store failed, a key created
    /// twice, or one deleted that is not there.
    StorageException,
    /// `XP_ERROR_KEY_NOT_FOUND` (14).
    KeyNotFound,
    /// `XP_ERROR_RETRY_UPDATE` (15): a checked update met another value.
    RetryUpdate,
    /// `XP_ERROR_MATURITY_EXCEEDED` (16): its rating is above the region's.
    MaturityExceeded,
    /// `XP_ERROR_NOT_PERMITTED_LAND` (17): the land does not allow it.
    NotPermittedLand,
    /// `XP_ERROR_REQUEST_PERM_TIMEOUT` (18): nobody answered the request.
    RequestTimedOut,
}

impl ExperienceError {
    /// Every error, by code.
    const ALL: [Self; 19] = [
        Self::None,
        Self::Throttled,
        Self::ExperiencesDisabled,
        Self::InvalidParameters,
        Self::NotPermitted,
        Self::NoExperience,
        Self::NotFound,
        Self::InvalidExperience,
        Self::ExperienceDisabled,
        Self::ExperienceSuspended,
        Self::Unknown,
        Self::QuotaExceeded,
        Self::StoreDisabled,
        Self::StorageException,
        Self::KeyNotFound,
        Self::RetryUpdate,
        Self::MaturityExceeded,
        Self::NotPermittedLand,
        Self::RequestTimedOut,
    ];

    /// The `XP_ERROR_*` value.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Throttled => 1,
            Self::ExperiencesDisabled => 2,
            Self::InvalidParameters => 3,
            Self::NotPermitted => 4,
            Self::NoExperience => 5,
            Self::NotFound => 6,
            Self::InvalidExperience => 7,
            Self::ExperienceDisabled => 8,
            Self::ExperienceSuspended => 9,
            Self::Unknown => 10,
            Self::QuotaExceeded => 11,
            Self::StoreDisabled => 12,
            Self::StorageException => 13,
            Self::KeyNotFound => 14,
            Self::RetryUpdate => 15,
            Self::MaturityExceeded => 16,
            Self::NotPermittedLand => 17,
            Self::RequestTimedOut => 18,
        }
    }

    /// The error of `code`, if one has it.
    #[must_use]
    pub fn from_code(code: i32) -> Option<Self> {
        usize::try_from(code)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
    }

    /// What `llGetExperienceErrorMessage` says of it, in aditi's words
    /// (`experience/no-experience`).
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::None => "no error",
            Self::Throttled => "exceeded throttle",
            Self::ExperiencesDisabled => "experiences are disabled",
            Self::InvalidParameters => "invalid parameters",
            Self::NotPermitted => "operation not permitted",
            Self::NoExperience => "script not associated with an experience",
            Self::NotFound => "not found",
            Self::InvalidExperience => "invalid experience",
            Self::ExperienceDisabled => "experience is disabled",
            Self::ExperienceSuspended => "experience is suspended",
            Self::Unknown => "unknown error",
            Self::QuotaExceeded => "experience data quota exceeded",
            Self::StoreDisabled => "key-value store is disabled",
            Self::StorageException => "key-value store communication failed",
            Self::KeyNotFound => "key doesn't exist",
            Self::RetryUpdate => "retry update",
            Self::MaturityExceeded => "experience content rating too high",
            Self::NotPermittedLand => "not allowed to run in current location",
            Self::RequestTimedOut => "experience permissions request timed out",
        }
    }
}

/// What `llGetExperienceErrorMessage` says of a code no error has: not code
/// 10's text, which the library table's tooltip promises
/// (`experience/no-experience`, −1, 19 and 20).
pub const UNKNOWN_ERROR_ID: &str = "unknown error id";

/// The text `llGetExperienceErrorMessage` answers for `code`.
#[must_use]
pub fn error_message(code: i32) -> &'static str {
    ExperienceError::from_code(code).map_or(UNKNOWN_ERROR_ID, ExperienceError::message)
}

/// The experience a script was compiled into, as the host knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptExperience {
    /// The experience's key.
    pub key: String,
    /// Whether it may run where the script is: [`Ok`], or the error that
    /// stops it — [`ExperienceError::InvalidExperience`],
    /// [`ExperienceError::ExperienceDisabled`],
    /// [`ExperienceError::ExperienceSuspended`],
    /// [`ExperienceError::ExperiencesDisabled`],
    /// [`ExperienceError::MaturityExceeded`] or
    /// [`ExperienceError::NotPermittedLand`].
    pub standing: Result<(), ExperienceError>,
}

/// An avatar's answer to an experience, kept by the grid for every script of
/// it (the agent's allowed and blocked lists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Admission {
    /// The avatar allowed it: a request is granted at once.
    Allowed,
    /// The avatar blocked it: a request is denied at once.
    Blocked,
    /// The avatar has said neither: a request puts the question.
    Undecided,
}

/// What `llGetExperienceDetails` reads of an experience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceDetails {
    /// Its name.
    pub name: String,
    /// Its owner's key.
    pub owner: String,
    /// Its key.
    pub id: String,
    /// Its state, an error ([`ExperienceError::None`] for one in order).
    pub state: ExperienceError,
    /// The group it belongs to, `NULL_KEY` for none.
    pub group: String,
}

impl ExperienceDetails {
    /// What Second Life answers, once it has looked, for `id` when no
    /// experience has it: no name, `NULL_KEY` owner and group, the id asked
    /// and the state [`ExperienceError::InvalidExperience`]
    /// (`experience/no-experience`).
    #[must_use]
    pub fn invalid(id: &str) -> Self {
        Self {
            name: String::new(),
            owner: NULL_KEY.to_owned(),
            id: id.to_owned(),
            state: ExperienceError::InvalidExperience,
            group: NULL_KEY.to_owned(),
        }
    }
}

/// An experience question a script put to an avatar, for the host to show:
/// a `ScriptQuestion` naming the experience, whose answer the host keeps for
/// every script of it and reports through
/// [`Engine::answer_experience`](crate::vm::Engine::answer_experience).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceQuestion {
    /// The avatar asked.
    pub agent: String,
    /// The experience's key.
    pub experience: String,
    /// The permissions the question stands for, [`EXPERIENCE_PERMISSIONS`].
    pub mask: i32,
}

/// Why the host did not sit an avatar `llSitOnLink` asked it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SitRefusal {
    /// `SIT_NO_SIT_TARGET` (−3): no free sit target in the linkset.
    NoSitTarget,
    /// `SIT_INVALID_AGENT` (−4): no such avatar in the region, or not over
    /// land that allows the experience.
    InvalidAgent,
    /// `SIT_NO_ACCESS` (−6): the avatar may not enter the prim's parcel.
    NoAccess,
    /// `SIT_INVALID_OBJECT` (−7): the object cannot be sat on (an
    /// attachment).
    InvalidObject,
}

impl SitRefusal {
    /// The `SIT_*` value `llSitOnLink` returns.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::NoSitTarget => -3,
            Self::InvalidAgent => -4,
            Self::NoAccess => -6,
            Self::InvalidObject => -7,
        }
    }
}

/// `SIT_OK`: the avatar sits.
pub const SIT_OK: i32 = 1;
/// `SIT_NOT_EXPERIENCE`: the script is in no experience that may run here.
pub const SIT_NOT_EXPERIENCE: i32 = -1;
/// `SIT_NO_EXPERIENCE_PERMISSION`: the avatar has not allowed the
/// experience.
pub const SIT_NO_EXPERIENCE_PERMISSION: i32 = -2;
/// `SIT_INVALID_LINK`: no prim, or a link constant naming several.
pub const SIT_INVALID_LINK: i32 = -5;

/// An experience's key-value store, kept by the grid for the experience — so
/// every object in it shares one, and it outlives any of them.
///
/// Each answer is the `dataserver` string Linden Lab documents: `1` and what
/// the call reads, or `0` and an [`ExperienceError`] code. None of it is
/// measured (see the module documentation).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyValueStore {
    /// The pairs, in key order: the order `llKeysKeyValue` lists them in,
    /// which Linden Lab promises only stays put while no key comes or goes.
    pairs: BTreeMap<String, String>,
}

/// A refusal of the store, as a `dataserver` answer.
fn refused(error: ExperienceError) -> String {
    format!("0,{}", error.code())
}

impl KeyValueStore {
    /// An empty store.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pairs: BTreeMap::new(),
        }
    }

    /// The bytes the pairs take: every key and every value.
    #[must_use]
    pub fn used(&self) -> usize {
        self.pairs
            .iter()
            .map(|(key, value)| key.len().saturating_add(value.len()))
            .sum()
    }

    /// The pair `key`'s value, if there is one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.get(key).map(String::as_str)
    }

    /// Whether `key` and `value` fit the limits and, written over what `key`
    /// holds now, the quota.
    fn admits(&self, key: &str, value: &str) -> Result<(), ExperienceError> {
        if key.len() > MAX_KEY_BYTES || value.len() > MAX_VALUE_BYTES {
            return Err(ExperienceError::InvalidParameters);
        }
        let old = self
            .pairs
            .get(key)
            .map_or(0, |old| key.len().saturating_add(old.len()));
        let new = key.len().saturating_add(value.len());
        if self.used().saturating_sub(old).saturating_add(new) > QUOTA_BYTES {
            return Err(ExperienceError::QuotaExceeded);
        }
        Ok(())
    }

    /// Write `key` as `value` once [`Self::admits`] has.
    fn write(&mut self, key: String, value: String) -> String {
        let answer = format!("1,{value}");
        let _old = self.pairs.insert(key, value);
        answer
    }

    /// `llCreateKeyValue`: a key that is there already is
    /// [`ExperienceError::StorageException`].
    pub fn create(&mut self, key: String, value: String) -> String {
        if self.pairs.contains_key(&key) {
            return refused(ExperienceError::StorageException);
        }
        match self.admits(&key, &value) {
            Ok(()) => self.write(key, value),
            Err(error) => refused(error),
        }
    }

    /// `llReadKeyValue`: a key that is not there is
    /// [`ExperienceError::KeyNotFound`].
    #[must_use]
    pub fn read(&self, key: &str) -> String {
        self.pairs.get(key).map_or_else(
            || refused(ExperienceError::KeyNotFound),
            |value| format!("1,{value}"),
        )
    }

    /// `llUpdateKeyValue`: write `key` as `value`, creating it when it is not
    /// there; `checked`, only while it holds `original` —
    /// [`ExperienceError::RetryUpdate`] otherwise, and for a key that is not
    /// there, which holds nothing to match.
    pub fn update(&mut self, key: String, value: String, checked: bool, original: &str) -> String {
        if checked && self.pairs.get(&key).is_none_or(|held| held != original) {
            return refused(ExperienceError::RetryUpdate);
        }
        match self.admits(&key, &value) {
            Ok(()) => self.write(key, value),
            Err(error) => refused(error),
        }
    }

    /// `llDeleteKeyValue`: a key that is not there is
    /// [`ExperienceError::StorageException`], as the wiki says, not
    /// [`ExperienceError::KeyNotFound`].
    pub fn delete(&mut self, key: &str) -> String {
        match self.pairs.remove(key) {
            Some(_) => "1".to_owned(),
            None => refused(ExperienceError::StorageException),
        }
    }

    /// `llKeyCountKeyValue`.
    #[must_use]
    pub fn count(&self) -> String {
        format!("1,{}", self.pairs.len())
    }

    /// `llKeysKeyValue`: up to `count` keys from the `first`th, cut where the
    /// answer would pass [`MAX_KEYS_ANSWER`] characters; a `first` past the
    /// last key is [`ExperienceError::KeyNotFound`].
    #[must_use]
    pub fn keys(&self, first: i32, count: i32) -> String {
        let Ok(first) = usize::try_from(first) else {
            return refused(ExperienceError::KeyNotFound);
        };
        if first >= self.pairs.len() {
            return refused(ExperienceError::KeyNotFound);
        }
        let mut answer = "1".to_owned();
        let count = usize::try_from(count).unwrap_or(0);
        for key in self.pairs.keys().skip(first).take(count) {
            if answer.len().saturating_add(1).saturating_add(key.len()) > MAX_KEYS_ANSWER {
                break;
            }
            answer.push(',');
            answer.push_str(key);
        }
        answer
    }

    /// `llDataSizeKeyValue`: the bytes used and the quota.
    #[must_use]
    pub fn size(&self) -> String {
        format!("1,{},{QUOTA_BYTES}", self.used())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn every_code_has_aditis_text_and_any_other_is_an_unknown_id() {
        assert_eq!(error_message(0), "no error");
        assert_eq!(error_message(5), "script not associated with an experience");
        assert_eq!(
            error_message(18),
            "experience permissions request timed out"
        );
        for code in [-1, 19, 20, i32::MAX] {
            assert_eq!(error_message(code), UNKNOWN_ERROR_ID, "{code}");
        }
        for (code, error) in (0..).zip(ExperienceError::ALL) {
            assert_eq!(error.code(), code);
        }
    }

    #[test]
    fn the_store_creates_reads_updates_and_deletes_as_documented() {
        let mut store = KeyValueStore::new();
        assert_eq!(store.read("a"), "0,14");
        assert_eq!(
            store.create("a".to_owned(), "one,two".to_owned()),
            "1,one,two"
        );
        assert_eq!(store.create("a".to_owned(), "again".to_owned()), "0,13");
        assert_eq!(store.read("a"), "1,one,two");
        assert_eq!(
            store.update("a".to_owned(), "three".to_owned(), true, "nope"),
            "0,15"
        );
        assert_eq!(
            store.update("a".to_owned(), "three".to_owned(), true, "one,two"),
            "1,three"
        );
        assert_eq!(
            store.update("b".to_owned(), "new".to_owned(), true, ""),
            "0,15"
        );
        assert_eq!(
            store.update("b".to_owned(), "new".to_owned(), false, ""),
            "1,new"
        );
        assert_eq!(store.count(), "1,2");
        assert_eq!(store.size(), format!("1,10,{QUOTA_BYTES}"));
        assert_eq!(store.delete("a"), "1");
        assert_eq!(store.delete("a"), "0,13");
        assert_eq!(store.count(), "1,1");
    }

    #[test]
    fn keys_are_listed_from_an_index_and_past_the_end_is_not_found() {
        let mut store = KeyValueStore::new();
        for key in ["c", "a", "b"] {
            let _answer = store.create(key.to_owned(), String::new());
        }
        assert_eq!(store.keys(0, 10), "1,a,b,c");
        assert_eq!(store.keys(1, 1), "1,b");
        assert_eq!(store.keys(3, 1), "0,14");
        assert_eq!(store.keys(-1, 1), "0,14");
    }

    #[test]
    fn a_pair_too_big_is_invalid_parameters() {
        let mut store = KeyValueStore::new();
        assert_eq!(
            store.create("k".repeat(MAX_KEY_BYTES + 1), String::new()),
            "0,3"
        );
        assert_eq!(
            store.create("k".to_owned(), "v".repeat(MAX_VALUE_BYTES + 1)),
            "0,3"
        );
        assert_eq!(
            store
                .create("k".to_owned(), "v".repeat(MAX_VALUE_BYTES))
                .len(),
            MAX_VALUE_BYTES + 2
        );
    }
}
