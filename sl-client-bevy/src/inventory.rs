//! Inventory / group-member / appearance capability fetches.

use crate::http::run_get_caps_llsd;
use crate::{Caps, EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use crossbeam_channel::Sender;
use sl_proto::{
    AIS3_FETCH_INVENTORY_TAG, AIS3_FETCH_LIBRARY_TAG, CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY,
    CAP_GROUP_MEMBER_DATA, CAP_INCREMENT_COF_VERSION, CAP_INVENTORY_API_V3, CAP_LIBRARY_API_V3,
    CAP_UPDATE_AVATAR_APPEARANCE, Error, GroupKey, InventoryFolderKey, InventoryOwner, Llsd,
    Session, Uuid, ais_category_children_fetch_url, build_fetch_inventory_request,
    build_group_member_data_request, build_update_avatar_appearance_request, parse_llsd_xml,
};
use std::time::Instant;

/// Issues a contents fetch for a single `folder_id` over the most modern road
/// the region offers for the tree the folder belongs to:
///
/// 1. **AIS3** — `GET <InventoryAPIv3>/category/<id>/children?depth=0` for the
///    agent's tree, `LibraryAPIv3` for the Library — what the reference viewer
///    reads both trees over whenever AIS is available
///    (`LLInventoryModelBackgroundFetch::bulkFetchViaAis`); Second Life serves
///    it, stock OpenSim does not;
/// 2. the descendents capabilities, `FetchInventoryDescendents2` /
///    `FetchLibDescendents2` (OpenSim);
/// 3. the legacy UDP `FetchInventoryDescendents` when no capability is known.
///
/// Every road decodes to
/// [`SessionEvent::InventoryDescendents`](sl_proto::Event::InventoryDescendents)
/// and marks the folder loaded at its version, so the explicit
/// ([`Command::RequestFolderContents`](sl_proto::Command::RequestFolderContents))
/// and on-demand (paging an unfetched folder) pulls stay grid-agnostic,
/// mirroring the background crawl's per-tree routing.
///
/// # Errors
///
/// Returns the [`Error`] of the UDP fallback's send when no inventory
/// capability is known and the request could not be put on the wire (the CAPS
/// route cannot fail here — it hands the fetch to a worker thread).
pub(crate) fn fetch_folder_contents(
    session: &mut Session,
    folder_id: InventoryFolderKey,
    caps: Option<&Caps>,
    now: Instant,
) -> Result<(), Error> {
    let library = session.inventory_owner(folder_id) == Some(InventoryOwner::Library);
    if let Some((base, tag, events_tx)) = caps.and_then(|caps| {
        let (cap, tag) = ais3_fetch_cap(library);
        Some((caps.map.get(cap).cloned()?, tag, caps.events_tx.clone()))
    }) {
        let url = format!("{base}{}", ais_category_children_fetch_url(folder_id, 0));
        crate::log_context::spawn_thread(move || run_get_caps_llsd(&url, tag, &events_tx));
        session.mark_folder_fetching(folder_id, now);
        return Ok(());
    }
    let route = caps.and_then(|caps| {
        let (url, owner, response_cap) = if library {
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
            crate::log_context::spawn_thread(move || {
                run_inventory_fetch(&url, owner, &[folder_id], response_cap, &events_tx);
            });
            // Mirror the UDP path's in-flight bookkeeping so the background crawl
            // does not re-pick this folder before its reply lands — and so a POST
            // that errors out (logged by the fetch, which delivers nothing) releases the
            // in-flight slot again once its stall deadline passes.
            session.mark_folder_fetching(folder_id, now);
        }
        None => {
            session.request_folder_contents(folder_id, now)?;
        }
    }
    Ok(())
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
/// to decode into [`SlSessionEvent::InventoryDescendents`]. A failed request is
/// logged and reported as the folder's fetch failing; the folder's stall
/// deadline then frees it for the crawl to retry.
pub(crate) fn run_ais3_folder_fetches(
    base: &str,
    folder_ids: &[InventoryFolderKey],
    tag: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    for folder in folder_ids {
        let url = format!("{base}{}", ais_category_children_fetch_url(*folder, 0));
        run_get_caps_llsd(&url, tag, caps_tx);
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_fetch_inventory_request(owner_id, folder_ids);
    // Every failure is logged: the folders stay `Fetching` until their stall
    // deadline frees them for the crawl to retry, and without a line here a
    // fetch that never works looks exactly like a slow one.
    let response = match http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
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
    let text = match response.text() {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(capability = response_cap, %error, "an inventory fetch reply could not be read");
            return;
        }
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => deliver(caps_tx, (response_cap.to_owned(), llsd)),
        Err(error) => {
            tracing::warn!(capability = response_cap, %error, "an inventory fetch reply did not parse");
        }
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
        deliver(caps_tx, (CAP_GROUP_MEMBER_DATA.to_owned(), llsd));
    }
}

/// GETs the `IncrementCOFVersion` capability (bump the agent's Current Outfit
/// Folder version on the grid) and forwards the LLSD reply to `caps_tx` tagged
/// [`CAP_INCREMENT_COF_VERSION`], for the session to surface as a
/// [`SlSessionEvent::CofVersionIncremented`].
///
/// A failed request is forwarded too, as an undefined body — surfaced as a reply
/// with no version — rather than as a bare caps failure: the caller retries the
/// increment, as the reference does, and can only do so if it hears that this
/// one did not land. Mirrors the tokio `increment_cof_version`.
pub(crate) fn run_increment_cof_version(cap_url: &str, caps_tx: &Sender<(String, Llsd)>) {
    let reply =
        crate::http::blocking_get_llsd(cap_url, CAP_INCREMENT_COF_VERSION).unwrap_or_else(|| {
            tracing::warn!("the Current Outfit Folder version increment failed");
            Llsd::Undef
        });
    deliver(caps_tx, (CAP_INCREMENT_COF_VERSION.to_owned(), reply));
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
        deliver(caps_tx, (CAP_UPDATE_AVATAR_APPEARANCE.to_owned(), llsd));
    }
}
