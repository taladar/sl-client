//! Inventory / group-member / appearance capability fetches.

use crate::{Caps, EVENT_QUEUE_TIMEOUT};
use bevy::prelude::*;
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{
    CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY, CAP_GROUP_MEMBER_DATA, CAP_UPDATE_AVATAR_APPEARANCE,
    GroupKey, InventoryFolderKey, InventoryOwner, Llsd, Session, Uuid,
    build_fetch_inventory_request, build_group_member_data_request,
    build_update_avatar_appearance_request, parse_llsd_xml,
};
use std::time::Instant;

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
/// [`SessionEvent::InventoryDescendents`](sl_proto::Event::InventoryDescendents).
/// The AIS3 (`InventoryAPIv3`) capability backs the inventory *mutation*
/// commands; the read path keeps the descendents semantics (folder versioning /
/// `Loaded` marking) that those caps provide.
pub(crate) fn fetch_folder_contents(
    session: &mut Session,
    folder_id: InventoryFolderKey,
    caps: Option<&Caps>,
    now: Instant,
) {
    let route = caps.and_then(|caps| {
        let (url, owner, response_cap) =
            if session.inventory_owner(folder_id) == Some(InventoryOwner::Library) {
                let url = caps.map.get(CAP_FETCH_LIBRARY).cloned()?;
                (url, session.library_owner()?.uuid(), CAP_FETCH_LIBRARY)
            } else {
                let url = caps.map.get(CAP_FETCH_INVENTORY).cloned()?;
                (url, session.agent_id()?.uuid(), CAP_FETCH_INVENTORY)
            };
        Some((url, owner, response_cap, caps.events_tx.clone()))
    });
    match route {
        Some((url, owner, response_cap, events_tx)) => {
            std::thread::spawn(move || {
                run_inventory_fetch(&url, owner, &[folder_id], response_cap, &events_tx);
            });
            // Mirror the UDP path's in-flight bookkeeping so the background crawl
            // does not re-pick this folder before its reply lands.
            session.mark_folder_fetching(folder_id);
        }
        None => {
            session.request_folder_contents(folder_id, now).ok();
        }
    }
}

/// POSTs a `FetchInventoryDescendents2` / `FetchLibDescendents2` request for
/// `folder_ids` (addressed to `owner_id` — the agent for its own inventory, the
/// Library owner for the shared Library) and forwards the LLSD response to
/// `caps_tx` tagged `response_cap` (`CAP_FETCH_INVENTORY` for the agent tree,
/// `CAP_FETCH_LIBRARY` for the Library), for the session to decode into
/// [`SlSessionEvent::InventoryDescendents`].
pub(crate) fn run_inventory_fetch(
    cap_url: &str,
    owner_id: Uuid,
    folder_ids: &[InventoryFolderKey],
    response_cap: &'static str,
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
        caps_tx.send((response_cap.to_owned(), llsd)).ok();
    }
}

/// POSTs a `GroupMemberData` request for `group_id` and forwards the LLSD roster
/// response to `caps_tx` tagged [`CAP_GROUP_MEMBER_DATA`], for the session to
/// decode into [`SlSessionEvent::GroupMembers`].
pub(crate) fn run_group_members_fetch(
    cap_url: &str,
    group_id: GroupKey,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_group_member_data_request(group_id.uuid());
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
