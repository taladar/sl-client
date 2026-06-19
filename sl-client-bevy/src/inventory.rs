//! Inventory / group-member / appearance capability fetches.

use crate::EVENT_QUEUE_TIMEOUT;
use bevy::prelude::*;
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{
    CAP_FETCH_INVENTORY, CAP_GROUP_MEMBER_DATA, CAP_UPDATE_AVATAR_APPEARANCE, Llsd, Uuid,
    build_fetch_inventory_request, build_group_member_data_request,
    build_update_avatar_appearance_request, parse_llsd_xml,
};

/// POSTs a `FetchInventoryDescendents2` request for `folder_ids` and forwards the
/// LLSD response to `caps_tx` tagged [`CAP_FETCH_INVENTORY`], for the session to
/// decode into [`SlSessionEvent::InventoryDescendents`].
pub(crate) fn run_inventory_fetch(
    cap_url: &str,
    owner_id: Uuid,
    folder_ids: &[Uuid],
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_fetch_inventory_request(owner_id, folder_ids);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_FETCH_INVENTORY.to_owned(), llsd)).ok();
    }
}

/// POSTs a `GroupMemberData` request for `group_id` and forwards the LLSD roster
/// response to `caps_tx` tagged [`CAP_GROUP_MEMBER_DATA`], for the session to
/// decode into [`SlSessionEvent::GroupMembers`].
pub(crate) fn run_group_members_fetch(
    cap_url: &str,
    group_id: Uuid,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_group_member_data_request(group_id);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_GROUP_MEMBER_DATA.to_owned(), llsd)).ok();
    }
}

/// POSTs an `UpdateAvatarAppearance` request for `cof_version` (the modern
/// Second Life server-side bake) and forwards the LLSD reply to `caps_tx` tagged
/// [`CAP_UPDATE_AVATAR_APPEARANCE`], for the session to surface as a
/// [`SlSessionEvent::ServerAppearanceUpdate`]. The baked appearance itself
/// arrives separately over UDP as a [`SlSessionEvent::AvatarAppearance`].
pub(crate) fn run_server_appearance_update(
    cap_url: &str,
    cof_version: i32,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_update_avatar_appearance_request(cof_version);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_UPDATE_AVATAR_APPEARANCE.to_owned(), llsd))
            .ok();
    }
}
