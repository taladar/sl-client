//! Generic LLSD-over-HTTP capability helpers (GET/PUT/PATCH/DELETE).

use reqwest::Client as ReqwestClient;
use sl_proto::{
    CAP_CHAT_SESSION_REQUEST, CAP_LAND_RESOURCES, LAND_RESOURCE_DETAIL_TAG,
    LAND_RESOURCE_SUMMARY_TAG, Llsd, ParcelKey, build_land_resources_request,
    parse_land_resources_reply, parse_llsd_xml,
};
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::caps::report_caps_failure;

/// POSTs `body` to a capability URL and ignores the reply — a fire-and-forget
/// capability call where the simulator returns only an HTTP status (e.g. the
/// `SendUserReport` abuse-report cap). Errors are swallowed; there is no event.
pub(crate) async fn post_caps_oneway(cap_url: String, body: String, http: ReqwestClient) {
    http.post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
        .ok();
}

/// POSTs a `ChatSessionRequest` accept / decline `body` to the cap URL and
/// forwards the LLSD reply to `caps_tx` tagged [`CAP_CHAT_SESSION_REQUEST`]. The
/// accept reply is the session's current agent roster, but it carries no session
/// id of its own (the viewer correlates it to the request it issued), so this
/// stamps the `session-id` + `from_group` of the answered invitation into the
/// reply map before forwarding — that is how
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) routes the
/// roster into the right session's participants. A non-map reply (the decline
/// acknowledgement, or OpenSim's stubbed `<llsd>true</llsd>`) carries no roster,
/// so only the stamped session context is forwarded and the fold is a no-op.
pub(crate) async fn post_chat_session_request(
    cap_url: String,
    body: String,
    session_id: Uuid,
    from_group: bool,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        report_caps_failure(&caps_tx, CAP_CHAT_SESSION_REQUEST).await;
        return;
    };
    let Ok(text) = response.text().await else {
        report_caps_failure(&caps_tx, CAP_CHAT_SESSION_REQUEST).await;
        return;
    };
    let Ok(reply) = parse_llsd_xml(&text) else {
        report_caps_failure(&caps_tx, CAP_CHAT_SESSION_REQUEST).await;
        return;
    };
    let mut map = match reply {
        Llsd::Map(map) => map,
        _ => HashMap::new(),
    };
    let _previous = map.insert("session-id".to_owned(), Llsd::Uuid(session_id));
    let _previous = map.insert("from_group".to_owned(), Llsd::Boolean(from_group));
    caps_tx
        .send((CAP_CHAT_SESSION_REQUEST.to_owned(), Llsd::Map(map)))
        .await
        .ok();
}

/// GETs `url` and parses the LLSD-XML reply, returning `None` on any
/// transport/parse failure. Shared by the experience capability fetches.
pub(crate) async fn get_llsd(url: &str, http: &ReqwestClient) -> Option<Llsd> {
    let response = http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
        .await
        .ok()?;
    let text = response.text().await.ok()?;
    parse_llsd_xml(&text).ok()
}

/// GETs an experience capability URL and forwards its LLSD reply to `caps_tx`
/// tagged `cap`, for the session to decode in
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) into the
/// matching experience event.
pub(crate) async fn get_caps_llsd(
    url: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    match get_llsd(&url, &http).await {
        Some(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).await.ok();
        }
        None => report_caps_failure(&caps_tx, cap).await,
    }
}

/// Drives the two-step `LandResources` flow: POSTs `{ parcel_id }` to the
/// `LandResources` capability, forwards the follow-up-URL reply tagged
/// [`CAP_LAND_RESOURCES`], then GETs the `ScriptResourceSummary` and (when
/// present) `ScriptResourceDetails` follow-up URLs, forwarding each tagged
/// [`LAND_RESOURCE_SUMMARY_TAG`] / [`LAND_RESOURCE_DETAIL_TAG`] for the session to
/// decode into [`Event::LandResourcesUrls`](sl_proto::Event::LandResourcesUrls),
/// [`Event::LandResourceSummary`](sl_proto::Event::LandResourceSummary), and
/// [`Event::LandResourceDetail`](sl_proto::Event::LandResourceDetail).
pub(crate) async fn fetch_land_resources(
    cap_url: String,
    parcel_id: ParcelKey,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_land_resources_request(parcel_id);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        report_caps_failure(&caps_tx, CAP_LAND_RESOURCES).await;
        return;
    };
    let Ok(text) = response.text().await else {
        report_caps_failure(&caps_tx, CAP_LAND_RESOURCES).await;
        return;
    };
    let Ok(reply) = parse_llsd_xml(&text) else {
        report_caps_failure(&caps_tx, CAP_LAND_RESOURCES).await;
        return;
    };
    let Ok(urls) = parse_land_resources_reply(&reply) else {
        report_caps_failure(&caps_tx, CAP_LAND_RESOURCES).await;
        return;
    };
    caps_tx
        .send((CAP_LAND_RESOURCES.to_owned(), reply))
        .await
        .ok();

    if let Some(summary) = urls.script_resource_summary {
        get_caps_llsd(
            summary.to_string(),
            LAND_RESOURCE_SUMMARY_TAG,
            http.clone(),
            caps_tx.clone(),
        )
        .await;
    }
    if let Some(detail_url) = urls.script_resource_details {
        get_caps_llsd(
            detail_url.to_string(),
            LAND_RESOURCE_DETAIL_TAG,
            http,
            caps_tx,
        )
        .await;
    }
}

/// PUTs `body` to an experience capability URL (the `Allow`/`Block` preference
/// set) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) async fn put_caps_llsd(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .put(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        report_caps_failure(&caps_tx, cap).await;
        return;
    };
    let Ok(text) = response.text().await else {
        report_caps_failure(&caps_tx, cap).await;
        return;
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).await.ok();
        }
        Err(_error) => report_caps_failure(&caps_tx, cap).await,
    }
}

/// Sends an HTTP PATCH of `body` to an AIS3 inventory capability URL (a folder /
/// item update or move) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) async fn patch_caps_llsd(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .patch(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        report_caps_failure(&caps_tx, cap).await;
        return;
    };
    let Ok(text) = response.text().await else {
        report_caps_failure(&caps_tx, cap).await;
        return;
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).await.ok();
        }
        Err(_error) => report_caps_failure(&caps_tx, cap).await,
    }
}

/// Sends an HTTP DELETE to an experience capability URL (the `Forget`
/// preference) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) async fn delete_caps_llsd(
    cap_url: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .delete(&cap_url)
        .header("Accept", "application/llsd+xml")
        .send()
        .await
    else {
        report_caps_failure(&caps_tx, cap).await;
        return;
    };
    let Ok(text) = response.text().await else {
        report_caps_failure(&caps_tx, cap).await;
        return;
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).await.ok();
        }
        Err(_error) => report_caps_failure(&caps_tx, cap).await,
    }
}
