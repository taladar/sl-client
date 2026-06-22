//! Inventory capability fetches (AIS / FetchInventory, GroupMemberData).

use reqwest::Client as ReqwestClient;
use sl_proto::{
    CAP_FETCH_INVENTORY, CAP_GROUP_MEMBER_DATA, GroupKey, InventoryFolderKey, Llsd, Uuid,
    build_fetch_inventory_request, build_group_member_data_request, parse_llsd_xml,
};
use tokio::sync::mpsc;

/// POSTs a `FetchInventoryDescendents2` request for `folder_ids` and forwards the
/// LLSD response to `caps_tx` tagged [`CAP_FETCH_INVENTORY`], for the session to
/// decode into [`Event::InventoryDescendents`].
pub(crate) async fn fetch_inventory(
    cap_url: String,
    owner_id: Uuid,
    folder_ids: Vec<InventoryFolderKey>,
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
        caps_tx
            .send((CAP_FETCH_INVENTORY.to_owned(), llsd))
            .await
            .ok();
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
