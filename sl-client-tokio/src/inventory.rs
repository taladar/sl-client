//! Inventory capability fetches (AIS / FetchInventory, GroupMemberData).

use reqwest::Client as ReqwestClient;
use sl_proto::{
    CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY, CAP_GROUP_MEMBER_DATA, Error as ProtoError, GroupKey,
    InventoryFolderKey, InventoryOwner, Llsd, Session, Uuid, build_fetch_inventory_request,
    build_group_member_data_request, parse_llsd_xml,
};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;

/// Issues a contents fetch for a single `folder_id`, automatically choosing the
/// modern CAPS inventory capability the region advertises and falling back to
/// the legacy UDP `FetchInventoryDescendents` when it does not (or when the
/// capability map is not yet known).
///
/// Second Life serves inventory only over CAPS — its UDP fetch goes
/// unanswered — while OpenSim still answers the UDP path, so routing here keeps
/// the explicit
/// ([`Command::RequestFolderContents`](sl_proto::Command::RequestFolderContents))
/// and on-demand (paging an unfetched folder) pulls grid-agnostic, mirroring the
/// background crawl's per-tree batch routing. The agent tree fetches over
/// `FetchInventoryDescendents2`, the Library tree over `FetchLibDescendents2`;
/// both decode to
/// [`Event::InventoryDescendents`](sl_proto::Event::InventoryDescendents). The
/// AIS3 (`InventoryAPIv3`) capability backs the inventory *mutation* commands;
/// the read path keeps the descendents semantics (folder versioning / `Loaded`
/// marking) that those caps provide.
///
/// # Errors
///
/// Propagates the UDP fallback's [`Error`](sl_proto::Error) (e.g. no circuit).
/// The CAPS path is fire-and-forget — its transport / parse failures surface as
/// a CAPS-failure diagnostic — so it returns `Ok`.
pub(crate) fn fetch_folder_contents(
    session: &mut Session,
    folder_id: InventoryFolderKey,
    caps: &HashMap<String, String>,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
    now: Instant,
) -> Result<(), ProtoError> {
    let route = if session.inventory_owner(folder_id) == Some(InventoryOwner::Library) {
        caps.get(CAP_FETCH_LIBRARY)
            .cloned()
            .zip(session.library_owner().map(|owner| owner.uuid()))
            .map(|(url, owner)| (url, owner, CAP_FETCH_LIBRARY))
    } else {
        caps.get(CAP_FETCH_INVENTORY)
            .cloned()
            .zip(session.agent_id().map(|owner| owner.uuid()))
            .map(|(url, owner)| (url, owner, CAP_FETCH_INVENTORY))
    };
    match route {
        Some((url, owner, response_cap)) => {
            tokio::spawn(fetch_inventory(
                url,
                owner,
                vec![folder_id],
                response_cap,
                http.clone(),
                caps_tx.clone(),
            ));
            // Mirror the UDP path's in-flight bookkeeping so the background crawl
            // does not re-pick this folder before its reply lands.
            session.mark_folder_fetching(folder_id);
            Ok(())
        }
        None => session.request_folder_contents(folder_id, now),
    }
}

/// POSTs a `FetchInventoryDescendents2` / `FetchLibDescendents2` request for
/// `folder_ids` (addressed to `owner_id` — the agent for its own inventory, the
/// Library owner for the shared Library) and forwards the LLSD response to
/// `caps_tx` tagged `response_cap` (`CAP_FETCH_INVENTORY` for the agent tree,
/// `CAP_FETCH_LIBRARY` for the Library), for the session to decode into
/// [`Event::InventoryDescendents`].
pub(crate) async fn fetch_inventory(
    cap_url: String,
    owner_id: Uuid,
    folder_ids: Vec<InventoryFolderKey>,
    response_cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_fetch_inventory_request(owner_id, &folder_ids);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((response_cap.to_owned(), llsd)).await.ok();
    }
}

/// POSTs the `GroupMemberData` capability for `group_id`, forwarding the decoded
/// LLSD roster back over `caps_tx` to be surfaced as an [`Event::GroupMembers`].
pub(crate) async fn fetch_group_members(
    cap_url: String,
    group_id: GroupKey,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_group_member_data_request(group_id.uuid());
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_GROUP_MEMBER_DATA.to_owned(), llsd))
            .await
            .ok();
    }
}
