//! The linkset data store: a key/value store owned by a linkset, not by a
//! script — what `llLinksetDataWrite` and its family read and write.
//!
//! Every number and answer is the book's (chapter `gridspec/lsl.md`, § The
//! linkset data store), measured on aditi by the probe `data/linkset-data`.
//! OpenSim has no store: its engines compile none of the functions, so a host
//! for an OpenSim region never needs one.
//!
//! The store is the host's: it keeps one [`LinksetData`] per linkset, says
//! which linkset a script is in ([`Host::linkset`](crate::vm::Host::linkset)),
//! and lends the caller's store to the library
//! ([`Host::linkset_data`](crate::vm::Host::linkset_data)). So it survives a
//! script's reset, is shared by every script of the linkset and is another
//! object's own — none of which the runtime has to arrange. What the store
//! answers, and what each change raises, is here.
//!
//! **Passwords.** A pair written with a password is *protected*. The rule the
//! measurements fit is one comparison: a call may read, write or delete a
//! pair when the password it gives is the pair's — the empty password for an
//! ordinary pair. So `llLinksetDataRead` of a protected pair reads `""`,
//! `llLinksetDataReadProtected` of an ordinary pair with a password reads
//! `""` too, and a write that would give a pair a password, take it away or
//! change it is refused (`LINKSETDATA_EPROTECTED`).

use std::collections::BTreeMap;

/// The store's budget in bytes (`llLinksetDataAvailable` of an empty store).
pub const BUDGET: usize = 0x0002_0000;

/// What a protected pair costs over its name and value, whatever its
/// password's length (2 and 20 characters were measured).
pub const PROTECTED_COST: usize = 32;

/// `LINKSETDATA_OK`: the call did what it was asked.
pub const OK: i32 = 0;
/// `LINKSETDATA_EMEMORY`: the pair does not fit in what is left.
pub const EMEMORY: i32 = 1;
/// `LINKSETDATA_ENOKEY`: the name is empty.
pub const ENOKEY: i32 = 2;
/// `LINKSETDATA_EPROTECTED`: the password is not the pair's.
pub const EPROTECTED: i32 = 3;
/// `LINKSETDATA_NOTFOUND`: there is no pair of that name.
pub const NOTFOUND: i32 = 4;
/// `LINKSETDATA_NOUPDATE`: the pair has that value already.
pub const NOUPDATE: i32 = 5;

/// Which linkset a script is in, as the host mints it: two scripts share a
/// store exactly when the host gives them the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LinksetId(pub u64);

/// What a `linkset_data` event says happened — its `action` argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinksetAction {
    /// `LINKSETDATA_RESET` (0): `llLinksetDataReset` emptied the store.
    Reset,
    /// `LINKSETDATA_UPDATE` (1): a pair was written.
    Update,
    /// `LINKSETDATA_DELETE` (2): a pair was deleted, or written empty.
    Delete,
    /// `LINKSETDATA_MULTIDELETE` (3): `llLinksetDataDeleteFound` deleted
    /// pairs.
    MultiDelete,
}

impl LinksetAction {
    /// The `LINKSETDATA_*` constant the event carries.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::Reset => 0,
            Self::Update => 1,
            Self::Delete => 2,
            Self::MultiDelete => 3,
        }
    }
}

/// One change of a store, as the `linkset_data` event every script of the
/// linkset gets for it says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinksetChange {
    /// What happened.
    pub action: LinksetAction,
    /// The pair's name; for [`LinksetAction::MultiDelete`] the names deleted,
    /// in key order, comma-separated without spaces; empty for a reset.
    pub name: String,
    /// The value written — empty for a protected pair, a delete and a reset.
    pub value: String,
}

impl LinksetChange {
    /// A change of `action` to the pair `name` with `value`.
    fn new(action: LinksetAction, name: &str, value: &str) -> Self {
        Self {
            action,
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }
}

/// What a call that may change the store answered, and the change it made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer<T> {
    /// What the function returns.
    pub value: T,
    /// The change, which raises `linkset_data`; [`None`] when the store is as
    /// it was (a refused write, `LINKSETDATA_NOUPDATE`, a missing name).
    pub change: Option<LinksetChange>,
}

impl<T> Answer<T> {
    /// An answer that changed nothing.
    const fn unchanged(value: T) -> Self {
        Self {
            value,
            change: None,
        }
    }

    /// An answer that made `change`.
    const fn changed(value: T, change: LinksetChange) -> Self {
        Self {
            value,
            change: Some(change),
        }
    }
}

/// One pair's value and password (empty for an ordinary pair).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pair {
    /// The value; never empty, since writing one deletes the pair.
    value: String,
    /// The password; empty for an ordinary pair.
    password: String,
}

impl Pair {
    /// Whether the pair has a password.
    const fn is_protected(&self) -> bool {
        !self.password.is_empty()
    }
}

/// What the pair `name` = `value` costs, protected or not: its name and
/// value in UTF-8, and [`PROTECTED_COST`] more for a password.
const fn cost(name: &str, value: &str, protected: bool) -> usize {
    let bytes = name.len().saturating_add(value.len());
    if protected {
        bytes.saturating_add(PROTECTED_COST)
    } else {
        bytes
    }
}

/// A linkset's key/value store.
///
/// The keys are kept in code-point order — the order
/// `llLinksetDataListKeys` answers in (`K0,k1,k10,k2,…`), which is the order
/// of their UTF-8 bytes and so a [`BTreeMap`]'s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinksetData {
    /// The pairs, by name.
    pairs: BTreeMap<String, Pair>,
    /// What the pairs cost, in bytes.
    used: usize,
}

impl LinksetData {
    /// An empty store.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pairs: BTreeMap::new(),
            used: 0,
        }
    }

    /// `llLinksetDataAvailable`: the bytes left of [`BUDGET`].
    #[must_use]
    pub fn available(&self) -> i32 {
        i32::try_from(BUDGET.saturating_sub(self.used)).unwrap_or(i32::MAX)
    }

    /// `llLinksetDataCountKeys`.
    #[must_use]
    pub fn count_keys(&self) -> i32 {
        i32::try_from(self.pairs.len()).unwrap_or(i32::MAX)
    }

    /// `llLinksetDataRead` (`password` empty) and
    /// `llLinksetDataReadProtected`: the value, when `password` is the
    /// pair's; `""` otherwise and for a missing name.
    #[must_use]
    pub fn read(&self, name: &str, password: &str) -> String {
        self.pairs
            .get(name)
            .filter(|pair| pair.password == password)
            .map(|pair| pair.value.clone())
            .unwrap_or_default()
    }

    /// `llLinksetDataWrite` (`password` empty) and
    /// `llLinksetDataWriteProtected`.
    ///
    /// An empty name is `LINKSETDATA_ENOKEY`; a password that is not the
    /// pair's `LINKSETDATA_EPROTECTED` (so an ordinary pair cannot be
    /// protected, nor a protected one given a new password); an empty value
    /// deletes the pair (`LINKSETDATA_NOTFOUND` when there is none); the
    /// value the pair has already is `LINKSETDATA_NOUPDATE`; a pair that does
    /// not fit in what is left, counting the bytes it frees, is
    /// `LINKSETDATA_EMEMORY` — one of exactly the bytes left fits.
    pub fn write(&mut self, name: &str, value: &str, password: &str) -> Answer<i32> {
        if name.is_empty() {
            return Answer::unchanged(ENOKEY);
        }
        if value.is_empty() {
            return self.delete(name, password);
        }
        let old = match self.pairs.get(name) {
            Some(pair) if pair.password != password => return Answer::unchanged(EPROTECTED),
            Some(pair) if pair.value == value => return Answer::unchanged(NOUPDATE),
            Some(pair) => cost(name, &pair.value, pair.is_protected()),
            None => 0,
        };
        let protected = !password.is_empty();
        let new = cost(name, value, protected);
        let used = self.used.saturating_sub(old).saturating_add(new);
        if used > BUDGET {
            return Answer::unchanged(EMEMORY);
        }
        self.used = used;
        let _previous = self.pairs.insert(
            name.to_owned(),
            Pair {
                value: value.to_owned(),
                password: password.to_owned(),
            },
        );
        let shown = if protected { "" } else { value };
        Answer::changed(OK, LinksetChange::new(LinksetAction::Update, name, shown))
    }

    /// `llLinksetDataDelete` (`password` empty) and
    /// `llLinksetDataDeleteProtected`: `LINKSETDATA_NOTFOUND` for a missing
    /// name, `LINKSETDATA_EPROTECTED` when `password` is not the pair's —
    /// so `llLinksetDataDeleteProtected` of an ordinary pair is refused too.
    pub fn delete(&mut self, name: &str, password: &str) -> Answer<i32> {
        match self.pairs.get(name) {
            None => Answer::unchanged(NOTFOUND),
            Some(pair) if pair.password != password => Answer::unchanged(EPROTECTED),
            Some(_) => {
                self.remove(name);
                Answer::changed(OK, LinksetChange::new(LinksetAction::Delete, name, ""))
            }
        }
    }

    /// Remove the pair `name`, giving back what it cost.
    fn remove(&mut self, name: &str) {
        if let Some(pair) = self.pairs.remove(name) {
            self.used = self
                .used
                .saturating_sub(cost(name, &pair.value, pair.is_protected()));
        }
    }

    /// `llLinksetDataReset`: the store emptied. It raises a reset whether or
    /// not there was anything in it.
    pub fn reset(&mut self) -> Answer<()> {
        self.pairs.clear();
        self.used = 0;
        Answer::changed((), LinksetChange::new(LinksetAction::Reset, "", ""))
    }

    /// `llLinksetDataListKeys`: the names in key order from `start` (a
    /// negative start is 0, one past the end none), at most `count` of them —
    /// all for a count below 1.
    #[must_use]
    pub fn list_keys(&self, start: i32, count: i32) -> Vec<String> {
        window(self.pairs.keys().cloned(), start, count)
    }

    /// `llLinksetDataFindKeys`: the names `pattern` finds, windowed as
    /// [`Self::list_keys`]. An empty pattern, and one that is no regular
    /// expression, finds nothing — without an error.
    #[must_use]
    pub fn find_keys(&self, pattern: &str, start: i32, count: i32) -> Vec<String> {
        window(self.found(pattern).into_iter(), start, count)
    }

    /// `llLinksetDataCountFound`: how many names `pattern` finds.
    #[must_use]
    pub fn count_found(&self, pattern: &str) -> i32 {
        i32::try_from(self.found(pattern).len()).unwrap_or(i32::MAX)
    }

    /// `llLinksetDataDeleteFound`: delete every pair `pattern` finds that
    /// `password` may delete — an ordinary pair whatever the password, a
    /// protected one only with its own — and answer `[deleted, not
    /// deleted]`. The deleted names raise **one** `LINKSETDATA_MULTIDELETE`,
    /// comma-separated in key order; nothing deleted raises nothing.
    pub fn delete_found(&mut self, pattern: &str, password: &str) -> Answer<(i32, i32)> {
        let (deleted, kept): (Vec<String>, Vec<String>) =
            self.found(pattern).into_iter().partition(|name| {
                self.pairs
                    .get(name)
                    .is_some_and(|pair| !pair.is_protected() || pair.password == password)
            });
        for name in &deleted {
            self.remove(name);
        }
        let counts = (
            i32::try_from(deleted.len()).unwrap_or(i32::MAX),
            i32::try_from(kept.len()).unwrap_or(i32::MAX),
        );
        if deleted.is_empty() {
            return Answer::unchanged(counts);
        }
        Answer::changed(
            counts,
            LinksetChange::new(LinksetAction::MultiDelete, &deleted.join(","), ""),
        )
    }

    /// The names `pattern` finds anywhere in them, in key order.
    ///
    /// Second Life's matcher is not documented; `regex`'s syntax is close to
    /// the ECMAScript and Perl ones for everything the probe used (`^`, `$`,
    /// classes, a plain substring) and refuses what it does not support —
    /// backreferences, look-around — which then finds nothing, as an
    /// unbalanced `(` does on the grid.
    fn found(&self, pattern: &str) -> Vec<String> {
        if pattern.is_empty() {
            return Vec::new();
        }
        let Ok(matcher) = regex::Regex::new(pattern) else {
            return Vec::new();
        };
        self.pairs
            .keys()
            .filter(|name| matcher.is_match(name))
            .cloned()
            .collect()
    }
}

/// At most `count` of `names` from `start`, by the listing functions' rule: a
/// negative start is 0, a count below 1 is all of them.
fn window(names: impl Iterator<Item = String>, start: i32, count: i32) -> Vec<String> {
    let skip = usize::try_from(start).unwrap_or(0);
    let take = usize::try_from(count)
        .ok()
        .filter(|count| *count > 0)
        .unwrap_or(usize::MAX);
    names.skip(skip).take(take).collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::constant;
    use crate::value::Value;

    /// The return codes and actions are the table's `LINKSETDATA_*`.
    #[test]
    fn the_codes_are_the_tables() {
        let value = |name: &str| constant(name).map(|constant| constant.value.to_value());
        for (name, code) in [
            ("LINKSETDATA_OK", OK),
            ("LINKSETDATA_EMEMORY", EMEMORY),
            ("LINKSETDATA_ENOKEY", ENOKEY),
            ("LINKSETDATA_EPROTECTED", EPROTECTED),
            ("LINKSETDATA_NOTFOUND", NOTFOUND),
            ("LINKSETDATA_NOUPDATE", NOUPDATE),
            ("LINKSETDATA_RESET", LinksetAction::Reset.code()),
            ("LINKSETDATA_UPDATE", LinksetAction::Update.code()),
            ("LINKSETDATA_DELETE", LinksetAction::Delete.code()),
            ("LINKSETDATA_MULTIDELETE", LinksetAction::MultiDelete.code()),
        ] {
            assert_eq!(value(name), Some(Value::Integer(code)), "{name}");
        }
    }

    /// The write rows of `data/linkset-data` on aditi: each answer and what
    /// it cost.
    #[test]
    fn writes_answer_and_cost_as_measured() {
        let mut store = LinksetData::new();
        let mut before = store.available();
        let mut write = |store: &mut LinksetData, name: &str, value: &str, password: &str| {
            let code = store.write(name, value, password).value;
            let now = store.available();
            let spent = before.saturating_sub(now);
            before = now;
            (code, spent)
        };
        let e10 = "é".repeat(10);
        assert_eq!(store.available(), 0x0002_0000);
        assert_eq!(write(&mut store, "a", "1", ""), (OK, 2));
        assert_eq!(
            write(&mut store, "abcdefghij", &"x".repeat(100), ""),
            (OK, 110)
        );
        assert_eq!(write(&mut store, "a", "1", ""), (NOUPDATE, 0));
        assert_eq!(write(&mut store, "a", "22", ""), (OK, 1));
        assert_eq!(write(&mut store, "a", "3", ""), (OK, -1));
        assert_eq!(write(&mut store, "u", &e10, ""), (OK, 21));
        assert_eq!(write(&mut store, &e10, "v", ""), (OK, 21));
        assert_eq!(write(&mut store, "e", "", ""), (NOTFOUND, 0));
        assert_eq!(write(&mut store, "a", "", ""), (OK, -2));
        assert_eq!(write(&mut store, "", "x", ""), (ENOKEY, 0));
        assert_eq!(write(&mut store, &"n".repeat(1100), "x", ""), (OK, 1101));
        assert_eq!(store.read("a", ""), "");
        assert_eq!(store.count_keys(), 4);
        // Protected pairs: 32 bytes more, whatever the password's length.
        assert_eq!(write(&mut store, "p", "v", "pw"), (OK, 34));
        assert_eq!(write(&mut store, "p0", "v", ""), (OK, 3));
        assert_eq!(write(&mut store, "p20", "v", &"w".repeat(20)), (OK, 36));
        assert_eq!(store.read("p", ""), "");
        assert_eq!(store.read("p0", ""), "v");
        assert_eq!(store.read("p", "nope"), "");
        assert_eq!(store.read("p", "pw"), "v");
        assert_eq!(store.read("abcdefghij", "pw"), "");
        assert_eq!(store.read("zz", "pw"), "");
        assert_eq!(write(&mut store, "p", "new", ""), (EPROTECTED, 0));
        assert_eq!(write(&mut store, "p", "new", "nope"), (EPROTECTED, 0));
        assert_eq!(write(&mut store, "p", "new", "pw"), (OK, 2));
        assert_eq!(write(&mut store, "p", "newer", "other"), (EPROTECTED, 0));
        assert_eq!(store.read("p", "pw"), "new");
        assert_eq!(write(&mut store, "abcdefghij", "y", "pw"), (EPROTECTED, 0));
        assert_eq!(store.delete("p", "").value, EPROTECTED);
        assert_eq!(store.delete("p", "nope").value, EPROTECTED);
        assert_eq!(store.delete("p", "pw").value, OK);
        assert_eq!(store.delete("zz", "").value, NOTFOUND);
        assert_eq!(store.delete("u", "pw").value, EPROTECTED);
        assert_eq!(store.delete("u", "").value, OK);
        assert_eq!(store.available(), 129_801);
    }

    /// The listing and finding rows, and `llLinksetDataDeleteFound`'s.
    #[test]
    fn lists_finds_and_deletes_by_pattern() {
        let mut store = LinksetData::new();
        for name in ["k3", "k1", "k10", "K0", "k2"] {
            assert_eq!(store.write(name, "v", "").value, OK);
        }
        assert_eq!(store.write("k4", "v", "pw").value, OK);
        let joined = |names: Vec<String>| names.join(",");
        assert_eq!(joined(store.list_keys(0, 0)), "K0,k1,k10,k2,k3,k4");
        assert_eq!(joined(store.list_keys(0, 2)), "K0,k1");
        assert_eq!(joined(store.list_keys(2, 10)), "k10,k2,k3,k4");
        assert_eq!(joined(store.list_keys(10, 1)), "");
        assert_eq!(joined(store.list_keys(-2, 2)), "K0,k1");
        assert_eq!(joined(store.list_keys(0, -1)), "K0,k1,k10,k2,k3,k4");
        assert_eq!(joined(store.find_keys("^k[0-9]$", 0, 0)), "k1,k2,k3,k4");
        assert_eq!(store.count_found("^k[0-9]$"), 4);
        assert_eq!(joined(store.find_keys("k", 1, 2)), "k10,k2");
        assert_eq!(store.count_found("k"), 5);
        assert_eq!(joined(store.find_keys("^K", 0, 0)), "K0");
        assert_eq!(joined(store.find_keys("", 0, 0)), "");
        assert_eq!(store.count_found(""), 0);
        assert_eq!(joined(store.find_keys("(", 0, 0)), "");
        assert_eq!(store.count_found("("), 0);
        let before = store.available();
        let no_pass = store.delete_found("^k", "");
        assert_eq!(no_pass.value, (4, 1));
        assert_eq!(
            no_pass.change,
            Some(LinksetChange::new(
                LinksetAction::MultiDelete,
                "k1,k10,k2,k3",
                ""
            ))
        );
        assert_eq!(store.available().saturating_sub(before), 13);
        let pass = store.delete_found("^k", "pw");
        assert_eq!(pass.value, (1, 0));
        assert_eq!(
            pass.change,
            Some(LinksetChange::new(LinksetAction::MultiDelete, "k4", ""))
        );
        assert_eq!(store.delete_found("^zz", ""), Answer::unchanged((0, 0)));
        assert_eq!(store.delete_found("(", ""), Answer::unchanged((0, 0)));
        assert_eq!(store.count_keys(), 1);
    }

    /// Filled with pairs of 1000 characters, 130 fit and the 131st is
    /// refused; a pair of exactly the bytes left fits, and then nothing does.
    #[test]
    fn the_budget_fills_as_measured() {
        let mut store = LinksetData::new();
        let value = "f".repeat(1000);
        let mut writes = 0_u32;
        let mut code = OK;
        while code == OK && writes < 200 {
            code = store.write(&format!("f{writes}"), &value, "").value;
            writes = writes.saturating_add(1);
        }
        assert_eq!((writes, code, store.available()), (131, EMEMORY, 662));
        assert_eq!(store.count_keys(), 130);
        assert_eq!(store.write("z", &"z".repeat(661), "").value, OK);
        assert_eq!(store.available(), 0);
        assert_eq!(store.write("y", "1", "").value, EMEMORY);
        assert_eq!(
            store.reset(),
            Answer::changed((), LinksetChange::new(LinksetAction::Reset, "", ""))
        );
        assert_eq!((store.available(), store.count_keys()), (0x0002_0000, 0));
    }

    /// What each change raises: a protected value is not shown, a delete and
    /// an empty value say `LINKSETDATA_DELETE`, and nothing is raised for a
    /// refused write, `LINKSETDATA_NOUPDATE` or a missing name.
    #[test]
    fn changes_raise_what_the_event_says() {
        let mut store = LinksetData::new();
        assert_eq!(
            store.write("a", "1", "").change,
            Some(LinksetChange::new(LinksetAction::Update, "a", "1"))
        );
        assert_eq!(
            store.write("p", "secret", "pw").change,
            Some(LinksetChange::new(LinksetAction::Update, "p", ""))
        );
        assert_eq!(store.write("a", "1", "").change, None);
        assert_eq!(store.write("p", "x", "").change, None);
        assert_eq!(store.delete("zz", "").change, None);
        assert_eq!(
            store.write("a", "", "").change,
            Some(LinksetChange::new(LinksetAction::Delete, "a", ""))
        );
        assert_eq!(
            store.delete("p", "pw").change,
            Some(LinksetChange::new(LinksetAction::Delete, "p", ""))
        );
    }
}
