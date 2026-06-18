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

use uuid::Uuid;

use crate::llsd::push_escaped;

/// The viewer's maximum AIS3 folder-fetch depth (`MAX_FOLDER_DEPTH_REQUEST`);
/// the grid caps deeper requests regardless.
pub const AIS_MAX_FOLDER_DEPTH: i32 = 50;

/// The URL suffix for creating a folder under `parent_id` via AIS3
/// (`POST /category/<parent>?tid=<tid>`). `tid` is a fresh transaction id the
/// reply echoes.
#[must_use]
pub fn ais_create_category_url(parent_id: Uuid, tid: Uuid) -> String {
    format!("/category/{parent_id}?tid={tid}")
}

/// The URL suffix for a folder by id (`/category/<id>`), used by `PATCH`
/// (update / move) and `DELETE` (remove).
#[must_use]
pub fn ais_category_url(category_id: Uuid) -> String {
    format!("/category/{category_id}")
}

/// The URL suffix for a folder's children (`/category/<id>/children`), used by
/// `GET ?depth=<n>` (fetch) and `DELETE` (purge / empty the folder).
#[must_use]
pub fn ais_category_children_url(category_id: Uuid) -> String {
    format!("/category/{category_id}/children")
}

/// The URL suffix for fetching a folder's children to `depth`
/// (`GET /category/<id>/children?depth=<n>`).
#[must_use]
pub fn ais_category_children_fetch_url(category_id: Uuid, depth: i32) -> String {
    let depth = depth.clamp(0, AIS_MAX_FOLDER_DEPTH);
    format!("/category/{category_id}/children?depth={depth}")
}

/// The URL suffix for an item by id (`/item/<id>`), used by `PATCH` (update /
/// move), `DELETE` (remove), and `GET` (fetch).
#[must_use]
pub fn ais_item_url(item_id: Uuid) -> String {
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
pub fn build_ais_move_body(parent_id: Uuid) -> String {
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
    folder_id: Uuid,
    parent_id: Uuid,
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
            ais_create_category_url(parent, tid),
            "/category/11111111-1111-1111-1111-111111111111?tid=22222222-2222-2222-2222-222222222222"
        );
    }

    #[test]
    fn children_fetch_url_clamps_depth() {
        let id = uuid!("33333333-3333-3333-3333-333333333333");
        assert_eq!(
            ais_category_children_fetch_url(id, 999),
            "/category/33333333-3333-3333-3333-333333333333/children?depth=50"
        );
        assert_eq!(
            ais_category_children_fetch_url(id, -5),
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
            build_ais_move_body(parent),
            "<llsd><map><key>parent_id</key><uuid>44444444-4444-4444-4444-444444444444</uuid></map></llsd>"
        );
    }

    #[test]
    fn create_inventory_category_request_has_all_fields() {
        let folder = uuid!("55555555-5555-5555-5555-555555555555");
        let parent = uuid!("66666666-6666-6666-6666-666666666666");
        assert_eq!(
            build_create_inventory_category_request(folder, parent, 8, "Toys"),
            "<llsd><map><key>folder_id</key><uuid>55555555-5555-5555-5555-555555555555</uuid>\
<key>parent_id</key><uuid>66666666-6666-6666-6666-666666666666</uuid>\
<key>type</key><integer>8</integer><key>name</key><string>Toys</string></map></llsd>"
        );
    }
}
