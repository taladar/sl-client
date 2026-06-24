//! Blocking LLSD/byte HTTP capability helpers (GET/PUT/PATCH/DELETE).

use crate::EVENT_QUEUE_TIMEOUT;
use crate::caps::report_caps_failure;
use bevy::prelude::*;
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{
    CAP_LAND_RESOURCES, LAND_RESOURCE_DETAIL_TAG, LAND_RESOURCE_SUMMARY_TAG, Llsd, ParcelKey,
    build_land_resources_request, parse_land_resources_reply, parse_llsd_xml,
};

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

    if !urls.script_resource_summary.is_empty() {
        run_get_caps_llsd(
            &urls.script_resource_summary,
            LAND_RESOURCE_SUMMARY_TAG,
            caps_tx,
        );
    }
    if let Some(detail_url) = urls.script_resource_details {
        run_get_caps_llsd(&detail_url, LAND_RESOURCE_DETAIL_TAG, caps_tx);
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
