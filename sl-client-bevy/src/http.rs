//! Blocking LLSD/byte HTTP capability helpers (GET/PUT/PATCH/DELETE).

use crate::caps::report_caps_failure;
use crate::lsl_syntax_cache::LslSyntaxCache;
use crate::retry::{MAX_TRANSIENT_RETRIES, is_transient_status, transient_backoff};
use crate::{EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use crossbeam_channel::Sender;
use sl_proto::{
    AVATAR_PICKER_SEARCH_TAG, CAP_CHAT_SESSION_REQUEST, CAP_LAND_RESOURCES, CAP_LSL_SYNTAX,
    CHAT_SESSION_FETCH_HISTORY_TAG, LAND_RESOURCE_DETAIL_TAG, LAND_RESOURCE_SUMMARY_TAG,
    LSL_SYNTAX_VERSION, Llsd, ParcelKey, Uuid, build_land_resources_request,
    parse_land_resources_reply, parse_llsd_xml,
};
use std::collections::HashMap;

/// GETs `url` and parses the LLSD-XML reply, retrying a **transient** answer
/// with the shared exponential backoff and returning `None` once every attempt
/// has failed. Shared by every one-shot capability GET. Mirrors the tokio
/// `get_llsd`; every caller runs on its own spawned thread, so the backoff
/// sleeps block nothing but the fetch.
///
/// `cap` names the capability for the log lines. The URL is deliberately **not**
/// logged: it carries the region's per-session cap token.
///
/// The retry is not a theoretical robustness knob. OpenSim's
/// `SimulatorFeaturesModule` answers `503 Service Unavailable` (plus
/// `Retry-After`) for as long as the requesting agent has no `ScenePresence` in
/// the scene — and both runtimes fire that GET the moment the capability map
/// lands, which is the same instant the deferred `CompleteAgentMovement` is
/// released. A one-shot fetch therefore loses that race on *every* login, which
/// is why the local grid never surfaced its feature flags (and so never fired
/// the `LSLSyntax` fetch keyed off them). The reference viewer retries the same
/// way: `LLViewerRegionImpl::requestSimulatorFeatureCoro` re-issues the GET on
/// any non-success status, up to 30 attempts.
///
/// A status that is *not* transient (a `404`, a `500`) is a real rejection, so
/// it fails the fetch immediately rather than spending the whole budget on an
/// answer that will not change.
pub(crate) fn blocking_get_llsd(url: &str, cap: &str) -> Option<Llsd> {
    let http = match crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    {
        Ok(http) => http,
        Err(error) => {
            tracing::warn!(capability = cap, %error, "could not build the HTTP client for a CAPS GET");
            return None;
        }
    };
    for attempt in 0..=MAX_TRANSIENT_RETRIES {
        match http
            .get(url)
            .header("Accept", "application/llsd+xml")
            .send()
        {
            Ok(response) if is_transient_status(response.status()) => {
                tracing::debug!(
                    capability = cap,
                    status = %response.status(),
                    attempt,
                    "a CAPS GET answered a transient status"
                );
            }
            Ok(response) if !response.status().is_success() => {
                tracing::warn!(
                    capability = cap,
                    status = %response.status(),
                    "a CAPS GET was rejected"
                );
                return None;
            }
            Ok(response) => {
                let text = match response.text() {
                    Ok(text) => text,
                    Err(error) => {
                        tracing::warn!(capability = cap, %error, "a CAPS GET reply could not be read");
                        return None;
                    }
                };
                return match parse_llsd_xml(&text) {
                    Ok(llsd) => Some(llsd),
                    Err(error) => {
                        tracing::warn!(capability = cap, %error, "a CAPS GET reply did not parse");
                        None
                    }
                };
            }
            Err(error) => {
                tracing::debug!(capability = cap, %error, attempt, "a CAPS GET failed");
            }
        }
        if attempt < MAX_TRANSIENT_RETRIES {
            std::thread::sleep(transient_backoff(attempt));
        }
    }
    tracing::warn!(capability = cap, "a CAPS GET failed every attempt");
    None
}

/// POSTs `body` to a capability URL and ignores the *reply* (blocking): the
/// shared body of every fire-and-forget capability call, where the simulator
/// answers with an HTTP status and nothing else.
///
/// `what` names the request family for the log. Because there is no event to
/// carry an outcome, a transport failure or a rejecting status is logged here
/// rather than discarded — that line is the only trace such a call leaves.
/// The capability URL is deliberately **not** logged: it carries the region's
/// per-session cap token.
pub(crate) fn post_llsd_oneway(cap_url: &str, body: String, what: &str) {
    let http = match crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    {
        Ok(http) => http,
        Err(error) => {
            tracing::warn!("could not build the HTTP client for {what}: {error}");
            return;
        }
    };
    match http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(status = %response.status(), "{what} was rejected");
        }
        Err(error) => tracing::warn!("{what} could not be sent: {error}"),
    }
}

/// POSTs `body` to a capability URL and ignores the reply (blocking) — a
/// fire-and-forget capability call where the simulator returns only an HTTP
/// status (e.g. the `SendUserReport` abuse-report cap). There is no event.
pub(crate) fn run_caps_oneway(cap_url: &str, body: String) {
    post_llsd_oneway(cap_url, body, "a fire-and-forget capability POST");
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
    deliver(
        caps_tx,
        (CAP_CHAT_SESSION_REQUEST.to_owned(), Llsd::Map(map)),
    );
}

/// POSTs a `ChatSessionRequest` `fetch history` `body` (blocking) and forwards
/// the reply to `caps_tx` tagged [`CHAT_SESSION_FETCH_HISTORY_TAG`] — the
/// synthetic routing tag, because the reply is a **bare LLSD array** (the
/// session's server-side backlog, oldest-first) that a plain
/// [`CAP_CHAT_SESSION_REQUEST`] tag would misroute into the roster decoder.
/// Like the roster path above, the reply carries no session identity of its
/// own, so it is wrapped as
/// `{ "history": <array>, "session-id": <uuid>, "from_group": <bool> }` for
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) to
/// rebuild the session kind. Mirrors the tokio
/// `post_chat_session_fetch_history`.
pub(crate) fn run_chat_session_fetch_history(
    cap_url: &str,
    body: String,
    session_id: Uuid,
    from_group: bool,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG);
        return;
    };
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        report_caps_failure(caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG);
        return;
    };
    let Ok(reply) = parse_llsd_xml(&text) else {
        report_caps_failure(caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG);
        return;
    };
    let mut map = HashMap::new();
    let _previous = map.insert("history".to_owned(), reply);
    let _previous = map.insert("session-id".to_owned(), Llsd::Uuid(session_id));
    let _previous = map.insert("from_group".to_owned(), Llsd::Boolean(from_group));
    deliver(
        caps_tx,
        (CHAT_SESSION_FETCH_HISTORY_TAG.to_owned(), Llsd::Map(map)),
    );
}

/// GETs an experience capability URL and forwards its LLSD reply to `caps_tx`
/// tagged `cap`, for the session to decode in
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event).
pub(crate) fn run_get_caps_llsd(url: &str, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
    match blocking_get_llsd(url, cap) {
        Some(llsd) => {
            deliver(caps_tx, (cap.to_owned(), llsd));
        }
        None => report_caps_failure(caps_tx, cap),
    }
}

/// GETs the `AvatarPickerSearch` capability (blocking) and forwards its reply to
/// `caps_tx` tagged [`AVATAR_PICKER_SEARCH_TAG`], stamping the caller's
/// `query_id` into the reply map — the HTTP path carries no `QueryID` of its
/// own, so without the stamp the answer could not be routed back to the search
/// that asked. Mirrors the tokio `get_avatar_picker_search`.
pub(crate) fn run_avatar_picker_search(
    url: &str,
    query_id: Uuid,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Some(reply) = blocking_get_llsd(url, AVATAR_PICKER_SEARCH_TAG) else {
        report_caps_failure(caps_tx, AVATAR_PICKER_SEARCH_TAG);
        return;
    };
    let mut map = match reply {
        Llsd::Map(map) => map,
        _other => HashMap::new(),
    };
    let _previous = map.insert("query-id".to_owned(), Llsd::Uuid(query_id));
    deliver(
        caps_tx,
        (AVATAR_PICKER_SEARCH_TAG.to_owned(), Llsd::Map(map)),
    );
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
    deliver(caps_tx, (CAP_LSL_SYNTAX.to_owned(), llsd));
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
    deliver(caps_tx, (CAP_LAND_RESOURCES.to_owned(), reply));

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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
            deliver(caps_tx, (cap.to_owned(), llsd));
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
            deliver(caps_tx, (cap.to_owned(), llsd));
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
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
            deliver(caps_tx, (cap.to_owned(), llsd));
        }
        Err(_error) => report_caps_failure(caps_tx, cap),
    }
}

/// Performs a blocking HTTP `GET`, returning the body bytes on a 2xx response,
/// or `None` on any network/HTTP failure. When `max_bytes` is `Some`, requests
/// only the first `max_bytes` via a `Range: bytes=0-(max_bytes-1)` header.
pub(crate) fn blocking_get_bytes(url: &str, max_bytes: Option<usize>) -> Option<Vec<u8>> {
    let http = crate::http_proxy::blocking_client_builder()
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
    let http = crate::http_proxy::blocking_client_builder()
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
