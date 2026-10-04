//! Inventory capability fetches (AIS / FetchInventory, GroupMemberData).

use crate::caps::deliver;
use crate::http::get_caps_llsd;
use reqwest::Client as ReqwestClient;
use sl_proto::{
    AIS3_FETCH_INVENTORY_TAG, AIS3_FETCH_LIBRARY_TAG, CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY,
    CAP_GROUP_MEMBER_DATA, CAP_INVENTORY_API_V3, CAP_LIBRARY_API_V3, Error as ProtoError, GroupKey,
    InventoryFolderKey, InventoryOwner, Llsd, Session, Uuid, ais_category_children_fetch_url,
    build_fetch_inventory_request, build_group_member_data_request, parse_llsd_xml,
};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;

/// Issues a contents fetch for a single `folder_id` over the most modern road
/// the region offers for the tree the folder belongs to: AIS3
/// (`GET <InventoryAPIv3>/category/<id>/children?depth=0`, `LibraryAPIv3` for
/// the Library — what the reference viewer reads both trees over whenever AIS
/// is available; Second Life serves it, stock OpenSim does not), else the
/// descendents capabilities (`FetchInventoryDescendents2` /
/// `FetchLibDescendents2`), else the legacy UDP `FetchInventoryDescendents`.
///
/// Every road decodes to
/// [`Event::InventoryDescendents`](sl_proto::Event::InventoryDescendents) and
/// marks the folder loaded at its version, so the explicit
/// ([`Command::RequestFolderContents`](sl_proto::Command::RequestFolderContents))
/// and on-demand (paging an unfetched folder) pulls stay grid-agnostic,
/// mirroring the background crawl's per-tree routing.
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
    let library = session.inventory_owner(folder_id) == Some(InventoryOwner::Library);
    let (ais_cap, ais_tag) = ais3_fetch_cap(library);
    if let Some(base) = caps.get(ais_cap) {
        let url = format!("{base}{}", ais_category_children_fetch_url(folder_id, 0));
        tokio::spawn(get_caps_llsd(url, ais_tag, http.clone(), caps_tx.clone()));
        session.mark_folder_fetching(folder_id, now);
        return Ok(());
    }
    let route = if library {
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
            // does not re-pick this folder before its reply lands — and so a POST
            // that errors out (logged by the fetch, which delivers nothing) releases the
            // in-flight slot again once its stall deadline passes.
            session.mark_folder_fetching(folder_id, now);
            Ok(())
        }
        None => session.request_folder_contents(folder_id, now),
    }
}

/// The AIS3 capability a folder of the agent's tree (`library == false`) or of
/// the Library is fetched over, and the tag its reply is forwarded under.
pub(crate) const fn ais3_fetch_cap(library: bool) -> (&'static str, &'static str) {
    if library {
        (CAP_LIBRARY_API_V3, AIS3_FETCH_LIBRARY_TAG)
    } else {
        (CAP_INVENTORY_API_V3, AIS3_FETCH_INVENTORY_TAG)
    }
}

/// GETs the AIS3 listing of each of `folder_ids` (`?depth=0`, one request per
/// folder, as the reference does) from the AIS3 capability at `base`, and
/// forwards every reply to `caps_tx` tagged `tag`
/// ([`AIS3_FETCH_INVENTORY_TAG`] / [`AIS3_FETCH_LIBRARY_TAG`]), for the session
/// to decode into [`Event::InventoryDescendents`]. A failed request is logged
/// and reported by [`get_caps_llsd`]; the folder's stall deadline then frees it
/// for the crawl to retry.
pub(crate) async fn fetch_ais3_folders(
    base: String,
    folder_ids: Vec<InventoryFolderKey>,
    tag: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    for folder in folder_ids {
        let url = format!("{base}{}", ais_category_children_fetch_url(folder, 0));
        get_caps_llsd(url, tag, http.clone(), caps_tx.clone()).await;
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
    // Every failure is logged: the folders stay `Fetching` until their stall
    // deadline frees them for the crawl to retry, and without a line here a
    // fetch that never works looks exactly like a slow one.
    let response = match http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(capability = response_cap, %error, "an inventory fetch POST failed");
            return;
        }
    };
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(capability = response_cap, %status, "an inventory fetch POST was rejected");
        return;
    }
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(capability = response_cap, %error, "an inventory fetch reply could not be read");
            return;
        }
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => deliver(&caps_tx, (response_cap.to_owned(), llsd)).await,
        Err(error) => {
            tracing::warn!(capability = response_cap, %error, "an inventory fetch reply did not parse");
        }
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
        deliver(&caps_tx, (CAP_GROUP_MEMBER_DATA.to_owned(), llsd)).await;
    }
}
