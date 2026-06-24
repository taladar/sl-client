//! Inventory mutation over HTTP capabilities: the modern AIS3 (`InventoryAPIv3`)
//! REST surface and the `CreateInventoryCategory` capability.
//!
//! The legacy UDP inventory-mutation messages (`CreateInventoryFolder`,
//! `UpdateInventoryItem`, …) are handled in `sl-proto`; this module covers the
//! capability bodies and URL suffixes the modern path uses. AIS3 is served only
//! by Second Life (stock OpenSim ships no `InventoryAPIv3` cap), while
//! `CreateInventoryCategory` is served by both OpenSim and Second Life.
//!
//! AIS3 is a REST API rooted at the `InventoryAPIv3` capability URL; each
//! operation is an HTTP verb against a path suffix under it:
//!
//! - create a folder — `POST /category/<parent>?tid=<tid>` with the new
//!   category body;
//! - rename / re-type a folder — `PATCH /category/<id>` with the changed fields;
//! - move a folder or item — `PATCH /category/<id>` or `PATCH /item/<id>` with
//!   `{ parent_id }`;
//! - delete a folder — `DELETE /category/<id>`; empty it — `DELETE
//!   /category/<id>/children`;
//! - fetch a folder's children — `GET /category/<id>/children?depth=<n>`;
//! - update / delete / fetch an item — `PATCH` / `DELETE` / `GET /item/<id>`.
//!
//! Verbs, URL layout, and the `tid`/`depth` query parameters are cross-checked
//! against the Firestorm viewer's `indra/newview/llaisapi.cpp`.

use std::collections::HashMap;

use sl_types::key::{InventoryFolderKey, InventoryKey};
use uuid::Uuid;

use crate::WireError;
use crate::llsd::{Llsd, parse_llsd_xml, push_escaped};

/// The viewer's maximum AIS3 folder-fetch depth (`MAX_FOLDER_DEPTH_REQUEST`);
/// the grid caps deeper requests regardless.
pub const AIS_MAX_FOLDER_DEPTH: i32 = 50;

/// The URL suffix for creating a folder under `parent_id` via AIS3
/// (`POST /category/<parent>?tid=<tid>`). `tid` is a fresh transaction id the
/// reply echoes.
#[must_use]
pub fn ais_create_category_url(parent_id: InventoryFolderKey, tid: Uuid) -> String {
    format!("/category/{parent_id}?tid={tid}")
}

/// The URL suffix for a folder by id (`/category/<id>`), used by `PATCH`
/// (update / move) and `DELETE` (remove).
#[must_use]
pub fn ais_category_url(category_id: InventoryFolderKey) -> String {
    format!("/category/{category_id}")
}

/// The URL suffix for a folder's children (`/category/<id>/children`), used by
/// `GET ?depth=<n>` (fetch) and `DELETE` (purge / empty the folder).
#[must_use]
pub fn ais_category_children_url(category_id: InventoryFolderKey) -> String {
    format!("/category/{category_id}/children")
}

/// The URL suffix for fetching a folder's children to `depth`
/// (`GET /category/<id>/children?depth=<n>`).
#[must_use]
pub fn ais_category_children_fetch_url(category_id: InventoryFolderKey, depth: i32) -> String {
    let depth = depth.clamp(0, AIS_MAX_FOLDER_DEPTH);
    format!("/category/{category_id}/children?depth={depth}")
}

/// The URL suffix for an item by id (`/item/<id>`), used by `PATCH` (update /
/// move), `DELETE` (remove), and `GET` (fetch).
#[must_use]
pub fn ais_item_url(item_id: InventoryKey) -> String {
    format!("/item/{item_id}")
}

/// The AIS3 body for creating a new folder: `{ name, type }` (the `type` is the
/// folder's preferred `FolderType`, or `-1` for none).
#[must_use]
pub fn build_ais_create_category_body(folder_type: i32, name: &str) -> String {
    let mut out = String::from("<llsd><map><key>name</key><string>");
    push_escaped(&mut out, name);
    out.push_str("</string><key>type</key><integer>");
    out.push_str(&folder_type.to_string());
    out.push_str("</integer></map></llsd>");
    out
}

/// The AIS3 `PATCH` body renaming a folder: `{ name }`.
#[must_use]
pub fn build_ais_rename_category_body(name: &str) -> String {
    let mut out = String::from("<llsd><map><key>name</key><string>");
    push_escaped(&mut out, name);
    out.push_str("</string></map></llsd>");
    out
}

/// The AIS3 `PATCH` body re-parenting a folder or item: `{ parent_id }`.
#[must_use]
pub fn build_ais_move_body(parent_id: InventoryFolderKey) -> String {
    format!("<llsd><map><key>parent_id</key><uuid>{parent_id}</uuid></map></llsd>")
}

/// The AIS3 `PATCH` body updating an item's name and description:
/// `{ name, desc }`.
#[must_use]
pub fn build_ais_update_item_body(name: &str, description: &str) -> String {
    let mut out = String::from("<llsd><map><key>name</key><string>");
    push_escaped(&mut out, name);
    out.push_str("</string><key>desc</key><string>");
    push_escaped(&mut out, description);
    out.push_str("</string></map></llsd>");
    out
}

/// The `CreateInventoryCategory` capability body: `{ folder_id, parent_id, type,
/// name }`, where `folder_id` is the desired (client-chosen) id. The capability
/// replies synchronously with `{ folder_id, name, parent_id, type }`. Served by
/// both OpenSim and Second Life.
#[must_use]
pub fn build_create_inventory_category_request(
    folder_id: InventoryFolderKey,
    parent_id: InventoryFolderKey,
    folder_type: i32,
    name: &str,
) -> String {
    let mut out = format!(
        "<llsd><map><key>folder_id</key><uuid>{folder_id}</uuid><key>parent_id</key><uuid>{parent_id}</uuid><key>type</key><integer>{folder_type}</integer><key>name</key><string>"
    );
    push_escaped(&mut out, name);
    out.push_str("</string></map></llsd>");
    out
}

// ---------------------------------------------------------------------------
// Server (simulator / inventory-service) direction: parse the request URLs and
// bodies a client builds above, and build the responses it parses. The inverse
// of the builders, cross-checked against `llaisapi.cpp` the same way.
// ---------------------------------------------------------------------------

/// Splits an AIS3 URL suffix into its path and optional query string, dropping
/// the leading `/` (`"/category/<id>?tid=<t>"` → `("category/<id>", Some("tid=<t>"))`).
fn split_url_suffix(suffix: &str) -> (&str, Option<&str>) {
    let path = suffix.strip_prefix('/').unwrap_or(suffix);
    match path.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (path, None),
    }
}

/// Returns the value of query parameter `name` within a `key=value&…` query
/// string, if present.
fn query_param<'query>(query: &'query str, name: &str) -> Option<&'query str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

/// Parses the [`ais_create_category_url`] suffix back into its `(parent_id, tid)`
/// pair (`POST /category/<parent>?tid=<tid>`), or `None` if it does not match.
#[must_use]
pub fn parse_ais_create_category_url(suffix: &str) -> Option<(InventoryFolderKey, Uuid)> {
    let (path, query) = split_url_suffix(suffix);
    let parent = path.strip_prefix("category/")?;
    let parent = InventoryFolderKey::from(Uuid::parse_str(parent).ok()?);
    let tid = query_param(query?, "tid")?;
    let tid = Uuid::parse_str(tid).ok()?;
    Some((parent, tid))
}

/// Parses the [`ais_category_url`] suffix back into its folder id
/// (`/category/<id>`, the `PATCH`/`DELETE` target), or `None` if it does not
/// match. Rejects the `/children` sub-path so it is not confused with
/// [`parse_ais_category_children_url`].
#[must_use]
pub fn parse_ais_category_url(suffix: &str) -> Option<InventoryFolderKey> {
    let (path, _query) = split_url_suffix(suffix);
    let id = path.strip_prefix("category/")?;
    if id.contains('/') {
        return None;
    }
    Uuid::parse_str(id).ok().map(InventoryFolderKey::from)
}

/// Parses the [`ais_category_children_url`] suffix back into its folder id
/// (`/category/<id>/children`, the `GET`/`DELETE` children target), or `None`
/// if it does not match.
#[must_use]
pub fn parse_ais_category_children_url(suffix: &str) -> Option<InventoryFolderKey> {
    let (path, _query) = split_url_suffix(suffix);
    let id = path.strip_prefix("category/")?.strip_suffix("/children")?;
    Uuid::parse_str(id).ok().map(InventoryFolderKey::from)
}

/// Parses the [`ais_category_children_fetch_url`] suffix back into its
/// `(category_id, depth)` pair (`GET /category/<id>/children?depth=<n>`), or
/// `None` if it does not match. The depth is clamped to the AIS maximum, exactly
/// as the builder clamps it.
#[must_use]
pub fn parse_ais_category_children_fetch_url(suffix: &str) -> Option<(InventoryFolderKey, i32)> {
    let category = parse_ais_category_children_url(suffix)?;
    let (_path, query) = split_url_suffix(suffix);
    let depth = query_param(query?, "depth")?.parse::<i32>().ok()?;
    Some((category, depth.clamp(0, AIS_MAX_FOLDER_DEPTH)))
}

/// Parses the [`ais_item_url`] suffix back into its item id (`/item/<id>`, the
/// `PATCH`/`DELETE`/`GET` target), or `None` if it does not match.
#[must_use]
pub fn parse_ais_item_url(suffix: &str) -> Option<InventoryKey> {
    let (path, _query) = split_url_suffix(suffix);
    let id = path.strip_prefix("item/")?;
    if id.contains('/') {
        return None;
    }
    Uuid::parse_str(id).ok().map(InventoryKey::from)
}

/// A parsed AIS3 create-folder body (`{ name, type }`): the inverse of
/// [`build_ais_create_category_body`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AisCategoryCreate {
    /// The new folder's preferred `FolderType` (`-1` for none).
    pub folder_type: i32,
    /// The new folder's name.
    pub name: String,
}

/// Parses an AIS3 create-folder body (`{ name, type }`) into its fields, the
/// inverse of [`build_ais_create_category_body`]. Missing fields default to an
/// empty name and `-1` type, mirroring the lenient scalar parsing elsewhere.
///
/// # Errors
///
/// Returns [`WireError::MalformedField`] if the body is not well-formed XML or a
/// present field has the wrong LLSD kind.
pub fn parse_ais_create_category_body(xml: &str) -> Result<AisCategoryCreate, WireError> {
    let root = parse_llsd_xml(xml).map_err(|error| WireError::MalformedField {
        field: "AisCreateCategory",
        value: error.to_string(),
    })?;
    Ok(AisCategoryCreate {
        folder_type: root.field_i32("type", "type")?.unwrap_or(-1),
        name: llsd_string(&root, "name")?,
    })
}

/// Parses an AIS3 rename-folder body (`{ name }`) into the new name, the inverse
/// of [`build_ais_rename_category_body`].
///
/// # Errors
///
/// Returns [`WireError::MalformedField`] if the body is not well-formed XML or a
/// present field has the wrong LLSD kind.
pub fn parse_ais_rename_category_body(xml: &str) -> Result<String, WireError> {
    let root = parse_llsd_xml(xml).map_err(|error| WireError::MalformedField {
        field: "AisRenameCategory",
        value: error.to_string(),
    })?;
    llsd_string(&root, "name")
}

/// Parses an AIS3 re-parent body (`{ parent_id }`) into the new parent id, the
/// inverse of [`build_ais_move_body`]. A missing or malformed `parent_id`
/// yields the nil UUID.
///
/// # Errors
///
/// Returns [`WireError::MalformedField`] if the body is not well-formed XML or a
/// present field has the wrong LLSD kind.
pub fn parse_ais_move_body(xml: &str) -> Result<InventoryFolderKey, WireError> {
    let root = parse_llsd_xml(xml).map_err(|error| WireError::MalformedField {
        field: "AisMove",
        value: error.to_string(),
    })?;
    Ok(InventoryFolderKey::from(
        root.field_uuid("parent_id", "parent_id")?
            .unwrap_or_else(Uuid::nil),
    ))
}

/// A parsed AIS3 item-update body (`{ name, desc }`): the inverse of
/// [`build_ais_update_item_body`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AisItemUpdate {
    /// The item's new name.
    pub name: String,
    /// The item's new description.
    pub description: String,
}

/// Parses an AIS3 item-update body (`{ name, desc }`) into its fields, the
/// inverse of [`build_ais_update_item_body`].
///
/// # Errors
///
/// Returns [`WireError::MalformedField`] if the body is not well-formed XML or a
/// present field has the wrong LLSD kind.
pub fn parse_ais_update_item_body(xml: &str) -> Result<AisItemUpdate, WireError> {
    let root = parse_llsd_xml(xml).map_err(|error| WireError::MalformedField {
        field: "AisUpdateItem",
        value: error.to_string(),
    })?;
    Ok(AisItemUpdate {
        name: llsd_string(&root, "name")?,
        description: llsd_string(&root, "desc")?,
    })
}

/// A parsed `CreateInventoryCategory` capability request (`{ folder_id,
/// parent_id, type, name }`): the inverse of
/// [`build_create_inventory_category_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateInventoryCategoryRequest {
    /// The desired (client-chosen) id for the new folder.
    pub folder_id: InventoryFolderKey,
    /// The id of the folder to create it under.
    pub parent_id: InventoryFolderKey,
    /// The new folder's preferred `FolderType` (`-1` for none).
    pub folder_type: i32,
    /// The new folder's name.
    pub name: String,
}

/// Parses a `CreateInventoryCategory` capability request body into its fields,
/// the inverse of [`build_create_inventory_category_request`]. Served by both
/// OpenSim and Second Life.
///
/// The `folder_id` (the client-chosen id of the folder to create), `parent_id`
/// (the containing folder), and `name` are mandatory: OpenSim's handler reads
/// each unconditionally and rejects the request (HTTP 400) if any is absent or
/// mistyped (`BunchOfCaps.cs:1242-1258`), and Firestorm always sends all three
/// (`llinventorymodel.cpp:1181-1184`). The `folder_type` stays optional —
/// absence defaults to `-1` (`FolderType::None`), a legitimate "no preferred
/// type" value.
///
/// # Errors
///
/// Returns [`WireError::MissingField`] if `folder_id`, `parent_id`, or `name`
/// is absent, and [`WireError::MalformedField`] if the body is not well-formed
/// XML or a present field has the wrong LLSD kind.
pub fn parse_create_inventory_category_request(
    xml: &str,
) -> Result<CreateInventoryCategoryRequest, WireError> {
    let root = parse_llsd_xml(xml).map_err(|error| WireError::MalformedField {
        field: "CreateInventoryCategory",
        value: error.to_string(),
    })?;
    Ok(CreateInventoryCategoryRequest {
        folder_id: InventoryFolderKey::from(root.require_uuid("folder_id", "folder_id")?),
        parent_id: InventoryFolderKey::from(root.require_uuid("parent_id", "parent_id")?),
        folder_type: root.field_i32("type", "type")?.unwrap_or(-1),
        name: root.require_str("name", "name")?.to_owned(),
    })
}

/// Returns the string member `key` of `root`, or the empty string if absent.
///
/// # Errors
///
/// Returns [`WireError::MalformedField`] if `key` is present with a non-string
/// LLSD value.
fn llsd_string(root: &Llsd, key: &'static str) -> Result<String, WireError> {
    Ok(root.field_str(key, key)?.unwrap_or("").to_owned())
}

/// The set of inventory objects an AIS3 mutation reply reports as changed — the
/// "meta" block a viewer's `AISUpdate` applies to its inventory model. Every
/// field is optional (an empty list / map is simply omitted from the wire form),
/// so one struct serves create, move, rename, update, and delete replies.
///
/// Field names mirror the `_`-prefixed wire keys the Firestorm viewer reads in
/// `llaisapi.cpp` (`AISUpdate::parseMeta`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AisUpdate {
    /// `_created_categories`: folders created by the operation.
    pub created_categories: Vec<InventoryFolderKey>,
    /// `_created_items`: items created by the operation.
    pub created_items: Vec<InventoryKey>,
    /// `_updated_categories`: folders whose fields changed.
    pub updated_categories: Vec<InventoryFolderKey>,
    /// `_updated_category_versions`: new descendant-version per folder id.
    pub updated_category_versions: Vec<(InventoryFolderKey, i32)>,
    /// `_categories_removed`: folders deleted by the operation.
    pub categories_removed: Vec<InventoryFolderKey>,
    /// `_category_items_removed`: items removed when emptying a folder.
    pub category_items_removed: Vec<InventoryKey>,
    /// `_removed_items`: items deleted by the operation.
    pub removed_items: Vec<InventoryKey>,
    /// `_broken_links_removed`: broken inventory links the grid pruned.
    pub broken_links_removed: Vec<InventoryKey>,
}

/// Builds an AIS3 mutation-reply body from an [`AisUpdate`], the response a
/// simulator's inventory service returns for the `POST`/`PATCH`/`DELETE`
/// operations the client issues via the URL/body builders above. Only non-empty
/// fields are emitted, exactly as the grid omits empty change-sets; an
/// all-empty [`AisUpdate`] therefore yields an empty `{}` map. Built on
/// [`Llsd::to_llsd_xml`], so it round-trips through [`parse_llsd_xml`].
#[must_use]
pub fn build_ais_update_response(update: &AisUpdate) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    insert_uuid_array(
        &mut map,
        "_created_categories",
        update.created_categories.iter().map(|id| id.uuid()),
    );
    insert_uuid_array(
        &mut map,
        "_created_items",
        update.created_items.iter().map(|id| id.uuid()),
    );
    insert_uuid_array(
        &mut map,
        "_updated_categories",
        update.updated_categories.iter().map(|id| id.uuid()),
    );
    insert_uuid_array(
        &mut map,
        "_categories_removed",
        update.categories_removed.iter().map(|id| id.uuid()),
    );
    insert_uuid_array(
        &mut map,
        "_category_items_removed",
        update.category_items_removed.iter().map(|id| id.uuid()),
    );
    insert_uuid_array(
        &mut map,
        "_removed_items",
        update.removed_items.iter().map(|id| id.uuid()),
    );
    insert_uuid_array(
        &mut map,
        "_broken_links_removed",
        update.broken_links_removed.iter().map(|id| id.uuid()),
    );
    if !update.updated_category_versions.is_empty() {
        let versions = update
            .updated_category_versions
            .iter()
            .map(|(id, version)| (id.to_string(), Llsd::Integer(*version)))
            .collect();
        let _previous = map.insert("_updated_category_versions".to_owned(), Llsd::Map(versions));
    }
    Llsd::Map(map).to_llsd_xml()
}

/// Inserts `ids` under `key` as an LLSD array of UUIDs, skipping the key
/// entirely when the list is empty.
fn insert_uuid_array(map: &mut HashMap<String, Llsd>, key: &str, ids: impl Iterator<Item = Uuid>) {
    let array: Vec<Llsd> = ids.map(Llsd::Uuid).collect();
    if array.is_empty() {
        return;
    }
    let _previous = map.insert(key.to_owned(), Llsd::Array(array));
}

/// Builds the synchronous `CreateInventoryCategory` capability reply (`{
/// folder_id, name, parent_id, type }`), the response counterpart of
/// [`parse_create_inventory_category_request`]. Built on [`Llsd::to_llsd_xml`],
/// so it round-trips through [`parse_llsd_xml`]. Served by both OpenSim and
/// Second Life.
#[must_use]
pub fn build_create_inventory_category_response(
    folder_id: InventoryFolderKey,
    parent_id: InventoryFolderKey,
    folder_type: i32,
    name: &str,
) -> String {
    Llsd::Map(HashMap::from([
        ("folder_id".to_owned(), Llsd::Uuid(folder_id.uuid())),
        ("name".to_owned(), Llsd::String(name.to_owned())),
        ("parent_id".to_owned(), Llsd::Uuid(parent_id.uuid())),
        ("type".to_owned(), Llsd::Integer(folder_type)),
    ]))
    .to_llsd_xml()
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;
    use uuid::uuid;

    #[test]
    fn create_category_url_carries_parent_and_tid() {
        let parent = uuid!("11111111-1111-1111-1111-111111111111");
        let tid = uuid!("22222222-2222-2222-2222-222222222222");
        assert_eq!(
            ais_create_category_url(InventoryFolderKey::from(parent), tid),
            "/category/11111111-1111-1111-1111-111111111111?tid=22222222-2222-2222-2222-222222222222"
        );
    }

    #[test]
    fn children_fetch_url_clamps_depth() {
        let id = uuid!("33333333-3333-3333-3333-333333333333");
        assert_eq!(
            ais_category_children_fetch_url(InventoryFolderKey::from(id), 999),
            "/category/33333333-3333-3333-3333-333333333333/children?depth=50"
        );
        assert_eq!(
            ais_category_children_fetch_url(InventoryFolderKey::from(id), -5),
            "/category/33333333-3333-3333-3333-333333333333/children?depth=0"
        );
    }

    #[test]
    fn create_category_body_escapes_name() {
        assert_eq!(
            build_ais_create_category_body(8, "A & B"),
            "<llsd><map><key>name</key><string>A &amp; B</string><key>type</key><integer>8</integer></map></llsd>"
        );
    }

    #[test]
    fn move_body_carries_parent() {
        let parent = uuid!("44444444-4444-4444-4444-444444444444");
        assert_eq!(
            build_ais_move_body(InventoryFolderKey::from(parent)),
            "<llsd><map><key>parent_id</key><uuid>44444444-4444-4444-4444-444444444444</uuid></map></llsd>"
        );
    }

    #[test]
    fn create_inventory_category_request_has_all_fields() {
        let folder = uuid!("55555555-5555-5555-5555-555555555555");
        let parent = uuid!("66666666-6666-6666-6666-666666666666");
        assert_eq!(
            build_create_inventory_category_request(
                InventoryFolderKey::from(folder),
                InventoryFolderKey::from(parent),
                8,
                "Toys"
            ),
            "<llsd><map><key>folder_id</key><uuid>55555555-5555-5555-5555-555555555555</uuid>\
<key>parent_id</key><uuid>66666666-6666-6666-6666-666666666666</uuid>\
<key>type</key><integer>8</integer><key>name</key><string>Toys</string></map></llsd>"
        );
    }

    #[test]
    fn create_category_url_round_trips() {
        let parent = uuid!("11111111-1111-1111-1111-111111111111");
        let tid = uuid!("22222222-2222-2222-2222-222222222222");
        let parent = InventoryFolderKey::from(parent);
        let url = ais_create_category_url(parent, tid);
        assert_eq!(parse_ais_create_category_url(&url), Some((parent, tid)));
        // A bare category URL is not a create URL (no tid).
        assert_eq!(
            parse_ais_create_category_url(&ais_category_url(parent)),
            None
        );
    }

    #[test]
    fn category_and_item_urls_round_trip_and_stay_distinct() {
        let id = uuid!("33333333-3333-3333-3333-333333333333");
        let folder_id = InventoryFolderKey::from(id);
        let item_id = InventoryKey::from(id);
        assert_eq!(
            parse_ais_category_url(&ais_category_url(folder_id)),
            Some(folder_id)
        );
        assert_eq!(parse_ais_item_url(&ais_item_url(item_id)), Some(item_id));
        // The children sub-path must not parse as a plain category URL.
        assert_eq!(
            parse_ais_category_url(&ais_category_children_url(folder_id)),
            None
        );
        assert_eq!(
            parse_ais_category_children_url(&ais_category_children_url(folder_id)),
            Some(folder_id)
        );
        // …nor a plain category URL as a children URL.
        assert_eq!(
            parse_ais_category_children_url(&ais_category_url(folder_id)),
            None
        );
    }

    #[test]
    fn children_fetch_url_round_trips_clamped() {
        let id = InventoryFolderKey::from(uuid!("33333333-3333-3333-3333-333333333333"));
        let url = ais_category_children_fetch_url(id, 7);
        assert_eq!(parse_ais_category_children_fetch_url(&url), Some((id, 7)));
        // The builder clamps, so the parser sees (and re-clamps) the clamped value.
        let clamped = ais_category_children_fetch_url(id, 999);
        assert_eq!(
            parse_ais_category_children_fetch_url(&clamped),
            Some((id, AIS_MAX_FOLDER_DEPTH))
        );
    }

    #[test]
    fn create_category_body_round_trips() -> Result<(), String> {
        let body = build_ais_create_category_body(8, "A & B");
        assert_eq!(
            parse_ais_create_category_body(&body).map_err(|error| format!("{error:?}"))?,
            AisCategoryCreate {
                folder_type: 8,
                name: "A & B".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn rename_and_move_bodies_round_trip() -> Result<(), String> {
        let renamed = build_ais_rename_category_body("New <name>");
        assert_eq!(
            parse_ais_rename_category_body(&renamed).map_err(|error| format!("{error:?}"))?,
            "New <name>"
        );
        let parent = InventoryFolderKey::from(uuid!("44444444-4444-4444-4444-444444444444"));
        assert_eq!(
            parse_ais_move_body(&build_ais_move_body(parent))
                .map_err(|error| format!("{error:?}"))?,
            parent
        );
        Ok(())
    }

    #[test]
    fn update_item_body_round_trips() -> Result<(), String> {
        let body = build_ais_update_item_body("Hat", "a fine hat");
        assert_eq!(
            parse_ais_update_item_body(&body).map_err(|error| format!("{error:?}"))?,
            AisItemUpdate {
                name: "Hat".to_owned(),
                description: "a fine hat".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn create_inventory_category_round_trips() -> Result<(), String> {
        let folder = InventoryFolderKey::from(uuid!("55555555-5555-5555-5555-555555555555"));
        let parent = InventoryFolderKey::from(uuid!("66666666-6666-6666-6666-666666666666"));
        let request = build_create_inventory_category_request(folder, parent, 8, "Toys");
        assert_eq!(
            parse_create_inventory_category_request(&request)
                .map_err(|error| format!("{error:?}"))?,
            CreateInventoryCategoryRequest {
                folder_id: folder,
                parent_id: parent,
                folder_type: 8,
                name: "Toys".to_owned(),
            }
        );
        // The reply echoes the same fields.
        let response = build_create_inventory_category_response(folder, parent, 8, "Toys");
        let tree = parse_llsd_xml(&response).map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            tree.get("folder_id").and_then(Llsd::as_uuid),
            Some(folder.uuid())
        );
        assert_eq!(
            tree.get("parent_id").and_then(Llsd::as_uuid),
            Some(parent.uuid())
        );
        assert_eq!(tree.get("type").and_then(Llsd::as_i32), Some(8));
        assert_eq!(tree.get("name").and_then(Llsd::as_str), Some("Toys"));
        Ok(())
    }

    #[test]
    fn ais_update_response_emits_only_nonempty_fields() -> Result<(), String> {
        let created = InventoryFolderKey::from(uuid!("77777777-7777-7777-7777-777777777777"));
        let removed = InventoryKey::from(uuid!("88888888-8888-8888-8888-888888888888"));
        let update = AisUpdate {
            created_categories: vec![created],
            removed_items: vec![removed],
            updated_category_versions: vec![(created, 42)],
            ..AisUpdate::default()
        };
        let tree = parse_llsd_xml(&build_ais_update_response(&update))
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            tree.get("_created_categories")
                .and_then(Llsd::as_array)
                .and_then(|array| array.first())
                .and_then(Llsd::as_uuid),
            Some(created.uuid())
        );
        assert_eq!(
            tree.get("_removed_items")
                .and_then(Llsd::as_array)
                .and_then(|array| array.first())
                .and_then(Llsd::as_uuid),
            Some(removed.uuid())
        );
        assert_eq!(
            tree.get("_updated_category_versions")
                .and_then(|versions| versions.get(&created.to_string()))
                .and_then(Llsd::as_i32),
            Some(42)
        );
        // Empty change-sets are omitted entirely.
        assert!(tree.get("_created_items").is_none());
        assert!(tree.get("_broken_links_removed").is_none());
        Ok(())
    }

    #[test]
    fn empty_ais_update_is_an_empty_map() -> Result<(), String> {
        let tree = parse_llsd_xml(&build_ais_update_response(&AisUpdate::default()))
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(tree, Llsd::Map(HashMap::new()));
        Ok(())
    }
}
