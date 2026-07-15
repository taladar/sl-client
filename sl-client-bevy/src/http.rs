//! Blocking LLSD/byte HTTP capability helpers (GET/PUT/PATCH/DELETE).

use crate::EVENT_QUEUE_TIMEOUT;
use crate::caps::report_caps_failure;
use crate::lsl_syntax_cache::LslSyntaxCache;
use bevy::prelude::*;
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{
    CAP_CHAT_SESSION_REQUEST, CAP_LAND_RESOURCES, CAP_LSL_SYNTAX, LAND_RESOURCE_DETAIL_TAG,
    LAND_RESOURCE_SUMMARY_TAG, LSL_SYNTAX_VERSION, Llsd, ParcelKey, Uuid,
    build_land_resources_request, parse_land_resources_reply, parse_llsd_xml,
};
use std::collections::HashMap;

/// GETs `url` and parses the LLSD-XML reply, returning `None` on any
/// transport/parse failure. Shared by the experience capability fetches.
pub(crate) fn blocking_get_llsd(url: &str) -> Option<Llsd> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let response = http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
        .ok()?;
    let text = response.text().ok()?;
    parse_llsd_xml(&text).ok()
}

/// POSTs `body` to a capability URL and ignores the reply (blocking) — a
/// fire-and-forget capability call where the simulator returns only an HTTP
/// status (e.g. the `SendUserReport` abuse-report cap). There is no event.
pub(crate) fn run_caps_oneway(cap_url: &str, body: String) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    http.post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .ok();
}

/// POSTs a `ChatSessionRequest` accept / decline `body` (blocking) and forwards
/// the LLSD reply to `caps_tx` tagged [`CAP_CHAT_SESSION_REQUEST`], stamping the
/// answered invitation's `session-id` + `from_group` into the reply map so the
/// session can route the accept roster to the right participants (the reply
/// carries no session id of its own). A non-map reply (decline ack / OpenSim's
/// stubbed `true`) carries no roster, so the fold is then a no-op. Mirrors the
/// tokio `post_chat_session_request`.
pub(crate) fn run_chat_session_request(
    cap_url: &str,
    body: String,
    session_id: Uuid,
    from_group: bool,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, CAP_CHAT_SESSION_REQUEST);
        return;
    };
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        report_caps_failure(caps_tx, CAP_CHAT_SESSION_REQUEST);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, CAP_CHAT_SESSION_REQUEST);
        return;
    };
    let Ok(reply) = parse_llsd_xml(&text) else {
        report_caps_failure(caps_tx, CAP_CHAT_SESSION_REQUEST);
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
        .ok();
}

/// GETs an experience capability URL and forwards its LLSD reply to `caps_tx`
/// tagged `cap`, for the session to decode in
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event).
pub(crate) fn run_get_caps_llsd(url: &str, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
    match blocking_get_llsd(url) {
        Some(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).ok();
        }
        None => report_caps_failure(caps_tx, cap),
    }
}

/// GETs the `LSLSyntax` capability (blocking), caches the raw document under
/// syntax `id`, and forwards its parsed LLSD to `caps_tx` tagged
/// [`CAP_LSL_SYNTAX`] for the session to decode into
/// [`SlSessionEvent::LslSyntax`](sl_proto::Event::LslSyntax). Mirrors the tokio
/// `fetch_lsl_syntax`: the raw XML is cached only when it declares the supported
/// schema version, while the LLSD is forwarded regardless (the session owns the
/// version gate).
pub(crate) fn run_fetch_lsl_syntax(
    url: &str,
    id: Uuid,
    cache: &LslSyntaxCache,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, CAP_LSL_SYNTAX);
        return;
    };
    let Ok(response) = http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
    else {
        report_caps_failure(caps_tx, CAP_LSL_SYNTAX);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, CAP_LSL_SYNTAX);
        return;
    };
    let Ok(llsd) = parse_llsd_xml(&text) else {
        report_caps_failure(caps_tx, CAP_LSL_SYNTAX);
        return;
    };
    if llsd
        .field_i32("llsd-lsl-syntax-version", "llsd-lsl-syntax-version")
        .ok()
        .flatten()
        == Some(LSL_SYNTAX_VERSION)
    {
        cache.store(id, &text);
    }
    caps_tx.send((CAP_LSL_SYNTAX.to_owned(), llsd)).ok();
}

/// Drives the two-step `LandResources` flow (blocking): POSTs `{ parcel_id }` to
/// the `LandResources` capability, forwards the follow-up-URL reply tagged
/// [`CAP_LAND_RESOURCES`], then GETs the `ScriptResourceSummary` and (when
/// present) `ScriptResourceDetails` follow-up URLs, forwarding each tagged
/// [`LAND_RESOURCE_SUMMARY_TAG`] / [`LAND_RESOURCE_DETAIL_TAG`] for the session to
/// decode into [`SlSessionEvent::LandResourcesUrls`](sl_proto::Event::LandResourcesUrls),
/// [`SlSessionEvent::LandResourceSummary`](sl_proto::Event::LandResourceSummary), and
/// [`SlSessionEvent::LandResourceDetail`](sl_proto::Event::LandResourceDetail).
pub(crate) fn run_land_resources(
    cap_url: &str,
    parcel_id: ParcelKey,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, CAP_LAND_RESOURCES);
        return;
    };
    let body = build_land_resources_request(parcel_id);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        report_caps_failure(caps_tx, CAP_LAND_RESOURCES);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, CAP_LAND_RESOURCES);
        return;
    };
    let Ok(reply) = parse_llsd_xml(&text) else {
        report_caps_failure(caps_tx, CAP_LAND_RESOURCES);
        return;
    };
    let Ok(urls) = parse_land_resources_reply(&reply) else {
        report_caps_failure(caps_tx, CAP_LAND_RESOURCES);
        return;
    };
    caps_tx.send((CAP_LAND_RESOURCES.to_owned(), reply)).ok();

    if let Some(summary) = urls.script_resource_summary {
        run_get_caps_llsd(summary.as_str(), LAND_RESOURCE_SUMMARY_TAG, caps_tx);
    }
    if let Some(detail_url) = urls.script_resource_details {
        run_get_caps_llsd(detail_url.as_str(), LAND_RESOURCE_DETAIL_TAG, caps_tx);
    }
}

/// PUTs `body` to an experience capability URL (the `Allow`/`Block` preference
/// set) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) fn run_put_caps_llsd(
    cap_url: &str,
    body: String,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    let Ok(response) = http
        .put(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).ok();
        }
        Err(_error) => report_caps_failure(caps_tx, cap),
    }
}

/// Sends an HTTP PATCH of `body` to an AIS3 inventory capability URL (a folder /
/// item update or move) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) fn run_patch_caps_llsd(
    cap_url: &str,
    body: String,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    let Ok(response) = http
        .patch(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).ok();
        }
        Err(_error) => report_caps_failure(caps_tx, cap),
    }
}

/// Sends an HTTP DELETE to an experience capability URL (the `Forget`
/// preference) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) fn run_delete_caps_llsd(
    cap_url: &str,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    let Ok(response) = http
        .delete(cap_url)
        .header("Accept", "application/llsd+xml")
        .send()
    else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => {
            caps_tx.send((cap.to_owned(), llsd)).ok();
        }
        Err(_error) => report_caps_failure(caps_tx, cap),
    }
}

/// Performs a blocking HTTP `GET`, returning the body bytes on a 2xx response,
/// or `None` on any network/HTTP failure. When `max_bytes` is `Some`, requests
/// only the first `max_bytes` via a `Range: bytes=0-(max_bytes-1)` header.
pub(crate) fn blocking_get_bytes(url: &str, max_bytes: Option<usize>) -> Option<Vec<u8>> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let mut request = http.get(url);
    if let Some(max) = max_bytes {
        request = request.header("Range", format!("bytes=0-{}", max.saturating_sub(1)));
    }
    let response = request.send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}

/// Performs a blocking HTTP `GET` for an inclusive `(start, end)` byte range via
/// a `Range: bytes=start-end` header, returning the body on a 2xx response.
pub(crate) fn blocking_get_range(url: &str, start: u32, end: u32) -> Option<Vec<u8>> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let response = http
        .get(url)
        .header("Range", format!("bytes={start}-{end}"))
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}
