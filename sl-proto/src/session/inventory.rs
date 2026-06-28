//! The held inventory model.
//!
//! Folds the loose folder/item caches that used to live directly on
//! [`Session`](crate::Session) into one value that owns the folder store, the
//! item store, the inventory roots, each folder's contents *fetch state*, and an
//! incrementally-maintained **parent→children index**, so listing a folder's
//! children is O(children) rather than a scan of the whole tree.
//!
//! The model holds **both** trees: the agent's own mutable inventory and the
//! read-only shared Library. Each folder records the [`InventoryOwner`] it
//! belongs to so the two trees share one store but stay queryable (and
//! cacheable) apart. Only structure/metadata lives here — asset bytes (textures,
//! meshes, notecard/script contents) are out of scope.
//!
//! Each folder is one [`FolderEntry`] that owns its payload **and** its
//! bookkeeping (owner, fetch state, child index) in a single value keyed by
//! folder key, so the two can never desync. The payload is an
//! `Option<`[`InventoryFolder`]`>`: a folder may be *known to exist* — named as a
//! child in another folder's listing, or fetched in its own right — before its
//! own metadata has arrived, in which case the entry holds the bookkeeping with a
//! `None` payload until the metadata lands. Items carry no such bookkeeping (no
//! fetch state, no children), so they stay a plain payload map. The stores are
//! keyed by the typed [`InventoryFolderKey`] / [`InventoryKey`] (never a bare
//! `Uuid`).

use std::collections::{BTreeMap, BTreeSet};

use sl_types::key::{InventoryFolderKey, InventoryKey};

use crate::bookkeeping_ids::InventoryCallbackId;
use crate::types::{InventoryFolder, InventoryItem, optional_key_from_wire};

/// Which of the two inventory trees an entry belongs to: the agent's own
/// mutable inventory, or the read-only shared Library. The two trees share one
/// held model but stay queryable apart (and are persisted to separate caches).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InventoryOwner {
    /// The agent's own inventory tree.
    Agent,
    /// The read-only shared Library tree.
    Library,
}

/// The fetch state of a folder's *contents* (its immediate children), tracked
/// separately from [`InventoryFolder::version`] — which records the folder's
/// authoritative version even while its contents are unfetched: a skeleton
/// folder carries a known version but [`Unknown`](Self::Unknown) contents until
/// it is fetched in its own right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    /// Known to exist (from the login skeleton, or named as a child in some other
    /// folder's descendents reply) but its own contents have not been fetched.
    Unknown,
    /// A descendents request for this folder is in flight.
    Fetching,
    /// The folder's contents are present, fetched at this version.
    Loaded {
        /// The folder version the contents were fetched at.
        version: i32,
    },
}

/// A folder and everything the model tracks about it: the [`InventoryFolder`]
/// payload, the tree it belongs to, its contents' [`FolderState`], and the
/// parent→children index. The payload and bookkeeping live in one value so they
/// cannot desync.
///
/// The payload is optional: a folder can be *known to exist* (linked as some
/// item's or folder's parent, or fetched in its own right) before its own
/// metadata arrives, and the entry then holds the bookkeeping with a `None`
/// payload until the metadata lands. Such a payload-less folder is reported as
/// absent by [`Inventory::folder`] / [`Inventory::folders_iter`] but its state
/// and child index are still tracked.
#[derive(Debug)]
struct FolderEntry {
    /// The folder payload, or `None` while the folder is known to exist but its
    /// metadata has not yet been received.
    folder: Option<InventoryFolder>,
    /// The tree (agent or library) this folder belongs to.
    owner: InventoryOwner,
    /// The fetch state of this folder's contents.
    state: FolderState,
    /// The keys of this folder's immediate sub-folders.
    child_folders: BTreeSet<InventoryFolderKey>,
    /// The keys of the items filed directly in this folder.
    child_items: BTreeSet<InventoryKey>,
}

/// The held inventory model — both the agent tree and the Library tree.
#[derive(Debug)]
pub(crate) struct Inventory {
    /// The folder entries (payload + bookkeeping), keyed by folder key.
    folders: BTreeMap<InventoryFolderKey, FolderEntry>,
    /// The item payloads, keyed by item key. Items carry no per-item bookkeeping,
    /// so they need no entry wrapper.
    items: BTreeMap<InventoryKey, InventoryItem>,
    /// The agent inventory root ("My Inventory") folder, from the login response,
    /// or `None` before login.
    agent_root: Option<InventoryFolderKey>,
    /// The shared Library root folder, from the login response, or `None`.
    library_root: Option<InventoryFolderKey>,
    /// A monotonic counter for the async `CallbackID` of inventory create/update
    /// requests (never zero), echoed back by the simulator so a client can
    /// correlate a reply with its request.
    next_callback: InventoryCallbackId,
}

impl Inventory {
    /// An empty inventory model: no folders, no items, both roots unknown.
    pub(crate) const fn new() -> Self {
        Self {
            folders: BTreeMap::new(),
            items: BTreeMap::new(),
            agent_root: None,
            library_root: None,
            next_callback: InventoryCallbackId(1),
        }
    }

    // ---- roots --------------------------------------------------------------

    /// The agent inventory root folder, if known.
    pub(crate) const fn agent_root(&self) -> Option<InventoryFolderKey> {
        self.agent_root
    }

    /// The shared Library root folder, if known.
    pub(crate) const fn library_root(&self) -> Option<InventoryFolderKey> {
        self.library_root
    }

    /// Records the agent inventory root (from the login response).
    pub(crate) const fn set_agent_root(&mut self, root: Option<InventoryFolderKey>) {
        self.agent_root = root;
    }

    /// Records the shared Library root (from the login response).
    pub(crate) const fn set_library_root(&mut self, root: Option<InventoryFolderKey>) {
        self.library_root = root;
    }

    // ---- reads --------------------------------------------------------------

    /// All folders whose metadata is present, in key order (payload-less
    /// known-but-unfetched folders are skipped).
    pub(crate) fn folders_iter(&self) -> impl Iterator<Item = &InventoryFolder> {
        self.folders
            .values()
            .filter_map(|entry| entry.folder.as_ref())
    }

    /// All cached item payloads, in key order.
    pub(crate) fn items_iter(&self) -> impl Iterator<Item = &InventoryItem> {
        self.items.values()
    }

    /// A folder payload by key, if its metadata is present.
    pub(crate) fn folder(&self, folder: InventoryFolderKey) -> Option<&InventoryFolder> {
        self.folders
            .get(&folder)
            .and_then(|entry| entry.folder.as_ref())
    }

    /// An item payload by key, if cached.
    pub(crate) fn item(&self, item: InventoryKey) -> Option<&InventoryItem> {
        self.items.get(&item)
    }

    /// A known folder's contents [`FolderState`], or `None` if the folder is not
    /// in the model at all. A known-but-unfetched folder (no payload yet) still
    /// has a state.
    pub(crate) fn folder_state(&self, folder: InventoryFolderKey) -> Option<FolderState> {
        self.folders.get(&folder).map(|entry| entry.state)
    }

    /// The tree (agent or library) a known folder belongs to, or `None`.
    pub(crate) fn folder_owner(&self, folder: InventoryFolderKey) -> Option<InventoryOwner> {
        self.folders.get(&folder).map(|entry| entry.owner)
    }

    /// The immediate children of `folder` — its sub-folder payloads and the item
    /// payloads filed directly in it — resolved O(children) through the index.
    /// Children whose metadata has not yet arrived are skipped.
    pub(crate) fn children(
        &self,
        folder: InventoryFolderKey,
    ) -> (Vec<&InventoryFolder>, Vec<&InventoryItem>) {
        let Some(entry) = self.folders.get(&folder) else {
            return (Vec::new(), Vec::new());
        };
        let folders = entry
            .child_folders
            .iter()
            .filter_map(|child| {
                self.folders
                    .get(child)
                    .and_then(|child| child.folder.as_ref())
            })
            .collect();
        let items = entry
            .child_items
            .iter()
            .filter_map(|child| self.items.get(child))
            .collect();
        (folders, items)
    }

    /// Allocates the next async inventory `CallbackID` (never zero).
    pub(crate) fn next_callback(&mut self) -> InventoryCallbackId {
        let id = self.next_callback;
        self.next_callback = InventoryCallbackId(self.next_callback.get().wrapping_add(1).max(1));
        id
    }

    // ---- folds (index- and state-maintaining) -------------------------------

    /// Ensures an entry exists for `folder`, creating a payload-less `Unknown`
    /// one under `owner` if absent; returns a mutable reference. This is how a
    /// folder becomes *known to exist* before its own metadata arrives — e.g.
    /// when one of its children is folded first, or a descendents reply for it
    /// lands before its skeleton entry.
    fn entry(&mut self, folder: InventoryFolderKey, owner: InventoryOwner) -> &mut FolderEntry {
        self.folders.entry(folder).or_insert_with(|| FolderEntry {
            folder: None,
            owner,
            state: FolderState::Unknown,
            child_folders: BTreeSet::new(),
            child_items: BTreeSet::new(),
        })
    }

    /// Inserts or updates a folder's metadata under `owner`, maintaining the
    /// child index (relinking it if its parent changed) and preserving an
    /// existing fetch state. A `version` of `0` (as carried by a descendents
    /// reply's sub-folders, which omit it) does not clobber a known version. A
    /// child folded before its parent links correctly: the parent's entry is
    /// created payload-less and filled when its own metadata arrives.
    pub(crate) fn cache_folder(&mut self, mut folder: InventoryFolder, owner: InventoryOwner) {
        let key = folder.folder_id;
        let old_parent = self
            .folders
            .get(&key)
            .and_then(|entry| entry.folder.as_ref())
            .and_then(|existing| existing.parent_id);
        let new_parent = folder.parent_id;
        if folder.version == 0
            && let Some(existing) = self
                .folders
                .get(&key)
                .and_then(|entry| entry.folder.as_ref())
        {
            folder.version = existing.version;
        }
        if old_parent != new_parent {
            if let Some(old) = old_parent
                && let Some(parent) = self.folders.get_mut(&old)
            {
                parent.child_folders.remove(&key);
            }
            if let Some(new) = new_parent {
                self.entry(new, owner).child_folders.insert(key);
            }
        }
        self.entry(key, owner).folder = Some(folder);
    }

    /// Inserts or updates an item payload under `owner`, maintaining the child
    /// index (relinking it if its containing folder changed). A reference to an
    /// as-yet-unknown containing folder creates that folder's entry payload-less.
    pub(crate) fn cache_item(&mut self, item: InventoryItem, owner: InventoryOwner) {
        let key = item.item_id;
        let new_folder = item.folder_id;
        let old_folder = self.items.get(&key).map(|existing| existing.folder_id);
        if old_folder != Some(new_folder) {
            if let Some(old) = old_folder
                && let Some(folder) = self.folders.get_mut(&old)
            {
                folder.child_items.remove(&key);
            }
            self.entry(new_folder, owner).child_items.insert(key);
        }
        self.items.insert(key, item);
    }

    /// Marks a folder's contents as fetched at `version` — the authoritative
    /// version from its own descendents reply — and writes that version into the
    /// folder payload if present. The folder need not have its own metadata yet
    /// (a descendents reply can precede the skeleton entry): the state is recorded
    /// on a payload-less entry, created under `owner` if absent. The sub-folders
    /// carried in that reply stay `Unknown` (their own `version 0` is not
    /// authoritative); only the fetched folder becomes
    /// [`Loaded`](FolderState::Loaded).
    pub(crate) fn mark_folder_loaded(
        &mut self,
        folder: InventoryFolderKey,
        version: i32,
        owner: InventoryOwner,
    ) {
        let entry = self.entry(folder, owner);
        entry.state = FolderState::Loaded { version };
        if let Some(payload) = entry.folder.as_mut() {
            payload.version = version;
        }
    }

    /// Re-parents a known folder under `new_parent`, moving it in the child index
    /// (unlink from the old parent, link under the new if present). Used by the
    /// in-place `MoveInventoryFolder` mutation. A no-op for a folder whose
    /// metadata is not present (nothing to re-parent).
    pub(crate) fn reparent_folder(
        &mut self,
        folder: InventoryFolderKey,
        new_parent: InventoryFolderKey,
    ) {
        let old_parent = match self
            .folders
            .get_mut(&folder)
            .and_then(|entry| entry.folder.as_mut())
        {
            Some(payload) => {
                let old = payload.parent_id;
                payload.parent_id = optional_key_from_wire(new_parent.uuid());
                old
            }
            None => return,
        };
        if old_parent == Some(new_parent) {
            return;
        }
        if let Some(old) = old_parent
            && let Some(parent) = self.folders.get_mut(&old)
        {
            parent.child_folders.remove(&folder);
        }
        if let Some(parent) = self.folders.get_mut(&new_parent) {
            parent.child_folders.insert(folder);
        }
    }

    /// Moves a known item into `new_folder` (optionally renaming it — an empty
    /// `new_name` keeps the current name), updating the child index. Used by the
    /// in-place `MoveInventoryItem` mutation.
    pub(crate) fn move_item(
        &mut self,
        item: InventoryKey,
        new_folder: InventoryFolderKey,
        new_name: &str,
    ) {
        let old_folder = match self.items.get_mut(&item) {
            Some(payload) => {
                let old = payload.folder_id;
                payload.folder_id = new_folder;
                if !new_name.is_empty() {
                    new_name.clone_into(&mut payload.name);
                }
                old
            }
            None => return,
        };
        if old_folder == new_folder {
            return;
        }
        if let Some(folder) = self.folders.get_mut(&old_folder) {
            folder.child_items.remove(&item);
        }
        self.entry(new_folder, InventoryOwner::Agent)
            .child_items
            .insert(item);
    }

    /// Overwrites the flags of a known item (the `ChangeInventoryItemFlags`
    /// mutation), leaving the index untouched (flags do not affect parentage).
    pub(crate) fn set_item_flags(&mut self, item: InventoryKey, flags: u32) {
        if let Some(payload) = self.items.get_mut(&item) {
            payload.flags = flags;
        }
    }

    /// Recursively drops a folder's descendents — its items and its sub-folders
    /// (whole subtrees) — leaving the folder's own (now-empty) entry in place.
    pub(crate) fn purge_descendents(&mut self, folder: InventoryFolderKey) {
        let Some((child_folders, child_items)) = self
            .folders
            .get(&folder)
            .map(|entry| (entry.child_folders.clone(), entry.child_items.clone()))
        else {
            return;
        };
        for item in &child_items {
            self.items.remove(item);
        }
        for sub in &child_folders {
            self.purge_descendents(*sub);
            self.folders.remove(sub);
        }
        if let Some(entry) = self.folders.get_mut(&folder) {
            entry.child_items.clear();
            entry.child_folders.clear();
        }
    }

    /// Removes a folder and its descendents, unlinking it from its parent's
    /// child set.
    pub(crate) fn remove_folder(&mut self, folder: InventoryFolderKey) {
        self.purge_descendents(folder);
        let parent = self
            .folders
            .get(&folder)
            .and_then(|entry| entry.folder.as_ref())
            .and_then(|payload| payload.parent_id);
        self.folders.remove(&folder);
        if let Some(parent) = parent
            && let Some(parent) = self.folders.get_mut(&parent)
        {
            parent.child_folders.remove(&folder);
        }
    }

    /// Removes an item, unlinking it from its containing folder's child set.
    pub(crate) fn remove_item(&mut self, item: InventoryKey) {
        if let Some(payload) = self.items.remove(&item)
            && let Some(folder) = self.folders.get_mut(&payload.folder_id)
        {
            folder.child_items.remove(&item);
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, OwnerKey};
    use sl_wire::Permissions5;
    use uuid::Uuid;

    use super::{FolderState, Inventory, InventoryOwner};
    use crate::types::{InventoryFolder, InventoryItem};

    /// A folder key from a small constant.
    fn fk(id: u128) -> InventoryFolderKey {
        InventoryFolderKey::from(Uuid::from_u128(id))
    }

    /// An item key from a small constant.
    fn ik(id: u128) -> InventoryKey {
        InventoryKey::from(Uuid::from_u128(id))
    }

    /// A skeleton-style folder under `parent` (`None` ⇒ root) at `version`.
    fn folder(id: u128, parent: Option<u128>, version: i32) -> InventoryFolder {
        InventoryFolder {
            folder_id: fk(id),
            parent_id: parent.map(fk),
            name: format!("folder-{id}"),
            folder_type: -1,
            version,
        }
    }

    /// A minimal item filed in `folder`; only the id, folder, and name carry
    /// meaningful values (the rest are nil/zero — the index logic ignores them).
    fn item(id: u128, folder: u128) -> InventoryItem {
        InventoryItem {
            item_id: ik(id),
            folder_id: fk(folder),
            name: format!("item-{id}"),
            description: String::new(),
            asset_id: Uuid::nil(),
            item_type: 0,
            inv_type: 0,
            flags: 0,
            sale_type: 0,
            sale_price: None,
            creation_date: 0,
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(1))),
            last_owner_id: Uuid::nil(),
            creator_id: AgentKey::from(Uuid::from_u128(1)),
            group: None,
            permissions: Permissions5::empty(),
        }
    }

    /// The child folder ids of `parent`, as raw `u128`s, for terse assertions.
    fn child_folder_ids(inv: &Inventory, parent: u128) -> Vec<u128> {
        inv.children(fk(parent))
            .0
            .iter()
            .map(|folder| folder.folder_id.uuid().as_u128())
            .collect()
    }

    /// The child item ids of `parent`, as raw `u128`s.
    fn child_item_ids(inv: &Inventory, parent: u128) -> Vec<u128> {
        inv.children(fk(parent))
            .1
            .iter()
            .map(|item| item.item_id.uuid().as_u128())
            .collect()
    }

    /// A freshly-seeded folder is `Unknown`, indexed under its parent, and keeps
    /// the authoritative version it was cached with; a later child fold carrying
    /// `version 0` does not clobber that version or change the `Unknown` state.
    #[test]
    fn seed_marks_unknown_and_preserves_version() {
        let mut inv = Inventory::new();
        inv.cache_folder(folder(0xF0, None, 5), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF1, Some(0xF0), 2), InventoryOwner::Agent);

        assert_eq!(inv.folder_state(fk(0xF0)), Some(FolderState::Unknown));
        assert_eq!(inv.folder_state(fk(0xF1)), Some(FolderState::Unknown));
        assert_eq!(inv.folder_owner(fk(0xF1)), Some(InventoryOwner::Agent));
        assert_eq!(child_folder_ids(&inv, 0xF0), vec![0xF1]);

        // A `version 0` re-fold (as a descendents reply lists sub-folders) keeps
        // both the stored version and the `Unknown` state.
        inv.cache_folder(folder(0xF1, Some(0xF0), 0), InventoryOwner::Agent);
        assert_eq!(inv.folder(fk(0xF1)).map(|f| f.version), Some(2));
        assert_eq!(inv.folder_state(fk(0xF1)), Some(FolderState::Unknown));
    }

    /// A child folded **before** its parent still links correctly: the parent's
    /// entry is created payload-less (reported absent until its metadata lands)
    /// but its child index is already populated.
    #[test]
    fn child_before_parent_links_via_placeholder() {
        let mut inv = Inventory::new();
        // Fold the child first; its parent 0xF0 is unknown so far.
        inv.cache_folder(folder(0xF1, Some(0xF0), 1), InventoryOwner::Agent);
        assert_eq!(inv.folder(fk(0xF0)), None); // parent metadata not here yet…
        assert_eq!(inv.folder_state(fk(0xF0)), Some(FolderState::Unknown)); // …but it's known
        assert_eq!(child_folder_ids(&inv, 0xF0), vec![0xF1]); // …and indexed

        // The parent's own metadata lands; the index is unchanged, payload appears.
        inv.cache_folder(folder(0xF0, None, 1), InventoryOwner::Agent);
        assert!(inv.folder(fk(0xF0)).is_some());
        assert_eq!(child_folder_ids(&inv, 0xF0), vec![0xF1]);
    }

    /// `mark_folder_loaded` flips a folder to `Loaded` at the reply version and
    /// writes that version into the payload, while its children stay `Unknown`.
    /// It works even for a folder whose own metadata has not arrived (a
    /// descendents reply preceding the skeleton entry).
    #[test]
    fn mark_loaded_sets_state_and_version() {
        let mut inv = Inventory::new();
        inv.cache_folder(folder(0xF0, None, 5), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF1, Some(0xF0), 0), InventoryOwner::Agent);
        inv.cache_item(item(0xD1, 0xF0), InventoryOwner::Agent);
        inv.mark_folder_loaded(fk(0xF0), 7, InventoryOwner::Agent);

        assert_eq!(
            inv.folder_state(fk(0xF0)),
            Some(FolderState::Loaded { version: 7 })
        );
        assert_eq!(inv.folder(fk(0xF0)).map(|f| f.version), Some(7));
        assert_eq!(inv.folder_state(fk(0xF1)), Some(FolderState::Unknown));
        assert_eq!(child_folder_ids(&inv, 0xF0), vec![0xF1]);
        assert_eq!(child_item_ids(&inv, 0xF0), vec![0xD1]);

        // Loading a folder we have no metadata for yet records the state on a
        // payload-less entry (the descendents-before-skeleton case).
        inv.mark_folder_loaded(fk(0xBEEF), 3, InventoryOwner::Agent);
        assert_eq!(
            inv.folder_state(fk(0xBEEF)),
            Some(FolderState::Loaded { version: 3 })
        );
        assert_eq!(inv.folder(fk(0xBEEF)), None);
    }

    /// Re-parenting a folder and moving an item both relink the child index:
    /// the entry leaves its old parent's set and joins the new one.
    #[test]
    fn reparent_and_move_relink_index() {
        let mut inv = Inventory::new();
        inv.cache_folder(folder(0xF0, None, 1), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF1, Some(0xF0), 1), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF2, Some(0xF0), 1), InventoryOwner::Agent);
        inv.cache_item(item(0xD1, 0xF1), InventoryOwner::Agent);

        // Re-parent F2 under F1.
        inv.reparent_folder(fk(0xF2), fk(0xF1));
        assert_eq!(child_folder_ids(&inv, 0xF0), vec![0xF1]);
        assert_eq!(child_folder_ids(&inv, 0xF1), vec![0xF2]);
        assert_eq!(
            inv.folder(fk(0xF2)).and_then(|f| f.parent_id),
            Some(fk(0xF1))
        );

        // Move the item from F1 to F2, renaming it.
        inv.move_item(ik(0xD1), fk(0xF2), "renamed");
        assert!(child_item_ids(&inv, 0xF1).is_empty());
        assert_eq!(child_item_ids(&inv, 0xF2), vec![0xD1]);
        assert_eq!(
            inv.item(ik(0xD1)).map(|i| i.name.clone()),
            Some("renamed".to_owned())
        );
    }

    /// Removing a folder drops it and its whole subtree and unlinks it from its
    /// parent; removing an item unlinks it from its folder.
    #[test]
    fn remove_drops_subtree_and_unlinks() {
        let mut inv = Inventory::new();
        inv.cache_folder(folder(0xF0, None, 1), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF1, Some(0xF0), 1), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF2, Some(0xF1), 1), InventoryOwner::Agent);
        inv.cache_item(item(0xD1, 0xF2), InventoryOwner::Agent);
        inv.cache_item(item(0xD2, 0xF0), InventoryOwner::Agent);

        // Removing F1 takes F2 and the item beneath it with it.
        inv.remove_folder(fk(0xF1));
        assert!(child_folder_ids(&inv, 0xF0).is_empty());
        assert_eq!(inv.folder(fk(0xF2)), None);
        assert_eq!(inv.item(ik(0xD1)), None);
        assert_eq!(inv.folder_state(fk(0xF1)), None);

        // The unrelated item under the root survives, then unlinks on removal.
        assert_eq!(child_item_ids(&inv, 0xF0), vec![0xD2]);
        inv.remove_item(ik(0xD2));
        assert!(child_item_ids(&inv, 0xF0).is_empty());
        assert_eq!(inv.item(ik(0xD2)), None);
    }

    /// Allocated callback ids are monotonic and never zero (wrapping past
    /// `u32::MAX` skips zero).
    #[test]
    fn callback_ids_are_monotonic_nonzero() {
        let mut inv = Inventory::new();
        let first = inv.next_callback();
        let second = inv.next_callback();
        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
    }
}
