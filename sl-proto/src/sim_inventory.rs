//! The in-memory inventory tree the server-side inventory capabilities serve
//! from.
//!
//! [`SimSession`](crate::SimSession) holds two of these — the agent's
//! inventory and the read-only shared Library — as driver-populated serving
//! stores (the `display_names` stance): the authoritative grid inventory
//! database is out of scope, but unlike the purely-read stores the AIS3
//! mutations **do** apply to this fixture, so a follow-up fetch observes the
//! create/rename/move/delete a test (or the fake grid) just performed, and
//! every affected folder's `version` bumps exactly as the real service's
//! `_updated_category_versions` reports it.
//!
//! Determinism: both maps are `BTreeMap`s, listings sort with an id
//! tie-break, and ids are caller-supplied (the dispatch layer mints them from
//! [`SimSession`](crate::SimSession)'s deterministic serial) — no clock, no
//! RNG.

use std::collections::BTreeMap;

use sl_types::key::{InventoryFolderKey, InventoryKey};
use sl_wire::AisUpdate;

use crate::types::{
    ASSET_CODE_LINK, ASSET_CODE_LINK_FOLDER, Event, InventoryFolder, InventoryItem,
};

/// Why an inventory-tree mutation was rejected; the dispatch layer maps the
/// variants to HTTP statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SimInventoryError {
    /// The folder or item the operation addresses does not exist (HTTP 404).
    UnknownTarget,
    /// The requested new parent is unknown, or the move would place a folder
    /// under itself / one of its own descendants (HTTP 400).
    InvalidParent,
}

/// An in-memory inventory tree: folders and items keyed by id, each item's
/// [`folder_id`](InventoryItem::folder_id) / folder's
/// [`parent_id`](InventoryFolder::parent_id) forming the hierarchy.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SimInventoryTree {
    /// Every folder, keyed by its id.
    folders: BTreeMap<InventoryFolderKey, InventoryFolder>,
    /// Every item, keyed by its id.
    items: BTreeMap<InventoryKey, InventoryItem>,
}

impl SimInventoryTree {
    /// Inserts (or replaces) a folder — the driver/test population API. The
    /// hierarchy is whatever the driver builds; no parent check is applied
    /// here so roots (`parent_id: None`) and pre-linked subtrees can be
    /// loaded in any order.
    pub fn insert_folder(&mut self, folder: InventoryFolder) {
        self.folders.insert(folder.folder_id, folder);
    }

    /// Inserts (or replaces) an item — the driver/test population API.
    pub fn insert_item(&mut self, item: InventoryItem) {
        self.items.insert(item.item_id, item);
    }

    /// The folder named by `id`, if the tree holds it.
    #[must_use]
    pub fn folder(&self, id: InventoryFolderKey) -> Option<&InventoryFolder> {
        self.folders.get(&id)
    }

    /// The item named by `id`, if the tree holds it.
    #[must_use]
    pub fn item(&self, id: InventoryKey) -> Option<&InventoryItem> {
        self.items.get(&id)
    }

    /// Every folder in the tree, in id order — e.g. for deriving a login
    /// response's inventory skeleton from a fixture tree.
    pub fn folders(&self) -> impl Iterator<Item = &InventoryFolder> {
        self.folders.values()
    }

    /// The direct child folders of `folder_id`, sorted by name with an id
    /// tie-break (the deterministic listing order).
    fn child_folders(&self, folder_id: InventoryFolderKey) -> Vec<InventoryFolder> {
        let mut children: Vec<InventoryFolder> = self
            .folders
            .values()
            .filter(|folder| folder.parent_id == Some(folder_id))
            .cloned()
            .collect();
        children.sort_by(|a, b| a.name.cmp(&b.name).then(a.folder_id.cmp(&b.folder_id)));
        children
    }

    /// The items directly inside `folder_id`, sorted per `sort_order`
    /// (`0` = by name, otherwise by creation date), with an id tie-break.
    fn child_items(&self, folder_id: InventoryFolderKey, sort_order: i32) -> Vec<InventoryItem> {
        let mut children: Vec<InventoryItem> = self
            .items
            .values()
            .filter(|item| item.folder_id == folder_id)
            .cloned()
            .collect();
        if sort_order == 0 {
            children.sort_by(|a, b| a.name.cmp(&b.name).then(a.item_id.cmp(&b.item_id)));
        } else {
            children.sort_by(|a, b| {
                a.creation_date
                    .cmp(&b.creation_date)
                    .then(a.item_id.cmp(&b.item_id))
            });
        }
        children
    }

    /// The folder the account keeps for `folder_type`, if it has one.
    ///
    /// Ties are broken by id so the answer is stable: a well-formed account has
    /// exactly one folder per system type, but nothing in the tree enforces
    /// that and a fixture may seed two.
    pub(crate) fn folder_of_type(&self, folder_type: i8) -> Option<&InventoryFolder> {
        self.folders
            .values()
            .filter(|folder| folder.folder_type == folder_type)
            .min_by_key(|folder| folder.folder_id)
    }

    /// The **link items** directly inside `folder_id`, sorted by name, or
    /// `None` when the folder is unknown.
    ///
    /// This is what AIS3's `/links` fetches answer. Links are the items whose
    /// asset code is [`ASSET_CODE_LINK`] or [`ASSET_CODE_LINK_FOLDER`];
    /// anything else in the folder is not part of the answer, which is why
    /// this is not just [`Self::child_items`].
    pub(crate) fn child_links(&self, folder_id: InventoryFolderKey) -> Option<Vec<InventoryItem>> {
        if !self.folders.contains_key(&folder_id) {
            return None;
        }
        Some(
            self.child_items(folder_id, 0)
                .into_iter()
                .filter(|item| {
                    let code = i32::from(item.item_type);
                    code == ASSET_CODE_LINK || code == ASSET_CODE_LINK_FOLDER
                })
                .collect(),
        )
    }

    /// Serves one `FetchInventoryDescendents2` folder entry: the direct
    /// children of `folder_id` as an
    /// [`Event::InventoryDescendents`], honouring the request's
    /// `fetch_folders` / `fetch_items` / `sort_order` fields. The
    /// `descendents` count always reports the full direct-child total,
    /// regardless of which halves were requested (matching OpenSim's
    /// handler). `None` when the folder is unknown — the batch fetch skips
    /// it tolerantly.
    pub(crate) fn descendents(
        &self,
        folder_id: InventoryFolderKey,
        fetch_folders: bool,
        fetch_items: bool,
        sort_order: i32,
    ) -> Option<Event> {
        let folder = self.folders.get(&folder_id)?;
        let all_folders = self.child_folders(folder_id);
        let all_items = self.child_items(folder_id, sort_order);
        let descendents =
            i32::try_from(all_folders.len().saturating_add(all_items.len())).unwrap_or(i32::MAX);
        Some(Event::InventoryDescendents {
            folder_id,
            version: folder.version,
            descendents,
            folders: if fetch_folders {
                all_folders
            } else {
                Vec::new()
            },
            items: if fetch_items { all_items } else { Vec::new() },
        })
    }

    /// The descendants of `folder_id` down to `depth` levels, flattened
    /// (folders and items separately; a folder's items count as level-1
    /// children alongside its sub-folders, so `depth == 0` lists nothing).
    /// `None` when the folder is unknown.
    pub(crate) fn children_to_depth(
        &self,
        folder_id: InventoryFolderKey,
        depth: i32,
    ) -> Option<(Vec<InventoryFolder>, Vec<InventoryItem>)> {
        if !self.folders.contains_key(&folder_id) {
            return None;
        }
        let mut folders = Vec::new();
        let mut items = Vec::new();
        let mut frontier = vec![folder_id];
        let mut remaining = depth;
        while remaining > 0 && !frontier.is_empty() {
            let mut next_frontier = Vec::new();
            for parent in frontier {
                let child_folders = self.child_folders(parent);
                next_frontier.extend(child_folders.iter().map(|folder| folder.folder_id));
                folders.extend(child_folders);
                items.extend(self.child_items(parent, 0));
            }
            frontier = next_frontier;
            remaining = remaining.saturating_sub(1);
        }
        Some((folders, items))
    }

    /// Whether `candidate` is `ancestor` itself or lies anywhere under it —
    /// the cycle check for [`move_category`](Self::move_category).
    fn is_self_or_descendant(
        &self,
        candidate: InventoryFolderKey,
        ancestor: InventoryFolderKey,
    ) -> bool {
        let mut cursor = Some(candidate);
        while let Some(id) = cursor {
            if id == ancestor {
                return true;
            }
            cursor = self.folders.get(&id).and_then(|folder| folder.parent_id);
        }
        false
    }

    /// Bumps `folder_id`'s version and records the new value in `update`'s
    /// `_updated_category_versions`. Unknown folders are ignored (a mutation
    /// only bumps folders it already verified exist).
    fn bump_version(&mut self, folder_id: InventoryFolderKey, update: &mut AisUpdate) {
        if let Some(folder) = self.folders.get_mut(&folder_id) {
            folder.version = folder.version.saturating_add(1);
            update
                .updated_category_versions
                .push((folder_id, folder.version));
        }
    }

    /// Creates `folder` (an AIS3 `POST /category/<parent>` — the caller has
    /// already minted its id and set its `parent_id`), bumping the parent's
    /// version.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the parent folder does not
    /// exist.
    pub(crate) fn create_category(
        &mut self,
        folder: InventoryFolder,
    ) -> Result<AisUpdate, SimInventoryError> {
        let parent_id = folder.parent_id.ok_or(SimInventoryError::UnknownTarget)?;
        if !self.folders.contains_key(&parent_id) {
            return Err(SimInventoryError::UnknownTarget);
        }
        let mut update = AisUpdate {
            created_categories: vec![folder.folder_id],
            ..AisUpdate::default()
        };
        self.folders.insert(folder.folder_id, folder);
        self.bump_version(parent_id, &mut update);
        Ok(update)
    }

    /// Creates link items under `parent` (an AIS3 `POST /category/<parent>`
    /// carrying a `links` array — the caller has already minted the item ids
    /// and set each link's `folder_id`), bumping the parent's version.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the parent folder does not
    /// exist.
    pub(crate) fn create_links(
        &mut self,
        parent: InventoryFolderKey,
        links: Vec<InventoryItem>,
    ) -> Result<AisUpdate, SimInventoryError> {
        if !self.folders.contains_key(&parent) {
            return Err(SimInventoryError::UnknownTarget);
        }
        let mut update = AisUpdate {
            created_items: links.iter().map(|link| link.item_id).collect(),
            ..AisUpdate::default()
        };
        for link in links {
            self.items.insert(link.item_id, link);
        }
        self.bump_version(parent, &mut update);
        Ok(update)
    }

    /// Renames folder `id` (an AIS3 `PATCH /category/<id>` with `{ name }`),
    /// bumping its own version.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist.
    pub(crate) fn rename_category(
        &mut self,
        id: InventoryFolderKey,
        name: String,
    ) -> Result<AisUpdate, SimInventoryError> {
        let folder = self
            .folders
            .get_mut(&id)
            .ok_or(SimInventoryError::UnknownTarget)?;
        folder.name = name;
        let mut update = AisUpdate {
            updated_categories: vec![id],
            ..AisUpdate::default()
        };
        self.bump_version(id, &mut update);
        Ok(update)
    }

    /// Re-parents folder `id` under `new_parent` (an AIS3 `PATCH
    /// /category/<id>` with `{ parent_id }`), bumping both the old and the
    /// new parent's versions.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist;
    /// [`SimInventoryError::InvalidParent`] when the new parent is unknown or
    /// the move would place the folder under itself or one of its own
    /// descendants.
    pub(crate) fn move_category(
        &mut self,
        id: InventoryFolderKey,
        new_parent: InventoryFolderKey,
    ) -> Result<AisUpdate, SimInventoryError> {
        if !self.folders.contains_key(&id) {
            return Err(SimInventoryError::UnknownTarget);
        }
        if !self.folders.contains_key(&new_parent) || self.is_self_or_descendant(new_parent, id) {
            return Err(SimInventoryError::InvalidParent);
        }
        let old_parent = self.folders.get(&id).and_then(|folder| folder.parent_id);
        if let Some(folder) = self.folders.get_mut(&id) {
            folder.parent_id = Some(new_parent);
        }
        let mut update = AisUpdate {
            updated_categories: vec![id],
            ..AisUpdate::default()
        };
        if let Some(old_parent) = old_parent {
            self.bump_version(old_parent, &mut update);
        }
        self.bump_version(new_parent, &mut update);
        Ok(update)
    }

    /// Re-parents item `id` into `new_parent` (an AIS3 `PATCH /item/<id>`
    /// with `{ parent_id }`), bumping both containing folders' versions.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the item does not exist;
    /// [`SimInventoryError::InvalidParent`] when the new parent is unknown.
    pub(crate) fn move_item(
        &mut self,
        id: InventoryKey,
        new_parent: InventoryFolderKey,
    ) -> Result<AisUpdate, SimInventoryError> {
        if !self.items.contains_key(&id) {
            return Err(SimInventoryError::UnknownTarget);
        }
        if !self.folders.contains_key(&new_parent) {
            return Err(SimInventoryError::InvalidParent);
        }
        let old_parent = self.items.get(&id).map(|item| item.folder_id);
        if let Some(item) = self.items.get_mut(&id) {
            item.folder_id = new_parent;
        }
        let mut update = AisUpdate::default();
        if let Some(old_parent) = old_parent {
            self.bump_version(old_parent, &mut update);
        }
        self.bump_version(new_parent, &mut update);
        Ok(update)
    }

    /// Updates item `id`'s name and description (an AIS3 `PATCH /item/<id>`
    /// with `{ name, desc }`), bumping the containing folder's version.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the item does not exist.
    pub(crate) fn update_item(
        &mut self,
        id: InventoryKey,
        name: String,
        description: String,
    ) -> Result<AisUpdate, SimInventoryError> {
        let item = self
            .items
            .get_mut(&id)
            .ok_or(SimInventoryError::UnknownTarget)?;
        item.name = name;
        item.description = description;
        let folder_id = item.folder_id;
        let mut update = AisUpdate::default();
        self.bump_version(folder_id, &mut update);
        Ok(update)
    }

    /// Collects the folder ids of the subtree rooted at `id` (including `id`
    /// itself), in breadth-first order.
    fn subtree_folder_ids(&self, id: InventoryFolderKey) -> Vec<InventoryFolderKey> {
        let mut collected = vec![id];
        let mut cursor = 0;
        while let Some(parent) = collected.get(cursor).copied() {
            collected.extend(
                self.folders
                    .values()
                    .filter(|folder| folder.parent_id == Some(parent))
                    .map(|folder| folder.folder_id),
            );
            cursor = cursor.saturating_add(1);
        }
        collected
    }

    /// Removes the folders named by `ids` and every item inside any of them,
    /// returning the removed item ids (sorted by the `BTreeMap` retain
    /// order, i.e. ascending id).
    fn remove_folders_and_contents(&mut self, ids: &[InventoryFolderKey]) -> Vec<InventoryKey> {
        for id in ids {
            self.folders.remove(id);
        }
        let mut removed_items = Vec::new();
        self.items.retain(|item_id, item| {
            if ids.contains(&item.folder_id) {
                removed_items.push(*item_id);
                false
            } else {
                true
            }
        });
        removed_items
    }

    /// Removes folder `id` and its whole subtree (an AIS3 `DELETE
    /// /category/<id>`), bumping the parent's version. The reply's
    /// `_categories_removed` lists the entire removed subtree and
    /// `_category_items_removed` every item that was inside it.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist.
    pub(crate) fn remove_category(
        &mut self,
        id: InventoryFolderKey,
    ) -> Result<AisUpdate, SimInventoryError> {
        let folder = self
            .folders
            .get(&id)
            .ok_or(SimInventoryError::UnknownTarget)?;
        let parent_id = folder.parent_id;
        let removed_folders = self.subtree_folder_ids(id);
        let removed_items = self.remove_folders_and_contents(&removed_folders);
        let mut update = AisUpdate {
            categories_removed: removed_folders,
            category_items_removed: removed_items,
            ..AisUpdate::default()
        };
        if let Some(parent_id) = parent_id {
            self.bump_version(parent_id, &mut update);
        }
        Ok(update)
    }

    /// Empties folder `id` (an AIS3 `DELETE /category/<id>/children`): every
    /// child folder subtree and direct item is removed, the folder itself
    /// stays, and its own version bumps.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the folder does not exist.
    pub(crate) fn purge_category(
        &mut self,
        id: InventoryFolderKey,
    ) -> Result<AisUpdate, SimInventoryError> {
        if !self.folders.contains_key(&id) {
            return Err(SimInventoryError::UnknownTarget);
        }
        let mut removed_folders = Vec::new();
        for child in self.child_folders(id) {
            removed_folders.extend(self.subtree_folder_ids(child.folder_id));
        }
        // The item sweep below covers both the removed subtrees and the
        // folder's own direct items; only the subtree folder records are
        // deleted (the purged folder itself survives).
        for folder_id in &removed_folders {
            self.folders.remove(folder_id);
        }
        let mut removed_items = Vec::new();
        self.items.retain(|item_id, item| {
            if item.folder_id == id || removed_folders.contains(&item.folder_id) {
                removed_items.push(*item_id);
                false
            } else {
                true
            }
        });
        let mut update = AisUpdate {
            categories_removed: removed_folders,
            category_items_removed: removed_items,
            ..AisUpdate::default()
        };
        self.bump_version(id, &mut update);
        Ok(update)
    }

    /// Removes item `id` (an AIS3 `DELETE /item/<id>`), bumping the
    /// containing folder's version.
    ///
    /// # Errors
    ///
    /// [`SimInventoryError::UnknownTarget`] when the item does not exist.
    pub(crate) fn remove_item(&mut self, id: InventoryKey) -> Result<AisUpdate, SimInventoryError> {
        let item = self
            .items
            .remove(&id)
            .ok_or(SimInventoryError::UnknownTarget)?;
        let mut update = AisUpdate {
            removed_items: vec![id],
            ..AisUpdate::default()
        };
        self.bump_version(item.folder_id, &mut update);
        Ok(update)
    }
}
