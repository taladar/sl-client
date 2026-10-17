//! Blocking LLSD/byte HTTP capability helpers (GET/PUT/PATCH/DELETE).

use crate::caps::report_caps_failure;
use crate::{EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use crossbeam_channel::Sender;
use sl_client_common::lsl_syntax_cache::LslSyntaxCache;
use sl_client_common::retry::{MAX_TRANSIENT_RETRIES, is_transient_status, transient_backoff};
use sl_proto::{
    AVATAR_PICKER_SEARCH_TAG, CAP_CHAT_SESSION_REQUEST, CAP_GET_METADATA, CAP_GROUP_API_V1,
    CAP_LAND_RESOURCES, CAP_LSL_SYNTAX, CAP_REMOTE_PARCEL_REQUEST, CAP_REQUEST_TASK_INVENTORY,
    CAP_SEARCH_STAT_REQUEST, CAP_UPLOAD_AGENT_PROFILE_IMAGE, CHAT_SESSION_FETCH_HISTORY_TAG,
    ClassifiedKey, EXPERIENCE_QUERY_TAG, InventoryKey, LAND_RESOURCE_DETAIL_TAG,
    LAND_RESOURCE_SUMMARY_TAG, LSL_SYNTAX_VERSION, Llsd, ObjectKey, ParcelKey, ProfileImage,
    RemoteParcelRequest, TASK_INVENTORY_FAILED_TAG, TASK_INVENTORY_UNCHANGED_TAG, Uuid,
    build_get_metadata_request, build_land_resources_request, build_remote_parcel_request,
    parse_land_resources_reply, parse_llsd_xml, profile_image_upload_body,
    profile_image_upload_outcome, search_stat_request_body, stamp_get_metadata_reply,
    stamp_remote_parcel_request, stamp_search_stat_answer, stamp_task_inventory_cap_reply,
    task_inventory_cap_url, task_inventory_failed_body, task_inventory_unchanged_body,
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

/// Answers a friendship offer that was kept over the `AcceptFriendship`
/// (`accept`: a POST with an empty LLSD body) or `DeclineFriendship` (a DELETE)
/// capability (blocking), at `url` from [`sl_proto::friendship_answer_url`].
/// The grid answers `{ "success": bool }`; there is no event to carry it, so a
/// failed request, a rejecting status or `success` false is logged. Mirrors
/// the tokio `answer_friendship`; the URL is not logged.
pub(crate) fn answer_friendship(url: &str, accept: bool) {
    let what = if accept {
        "the AcceptFriendship request"
    } else {
        "the DeclineFriendship request"
    };
    let Some(http) = answer_client(what) else {
        return;
    };
    let request = if accept {
        http.post(url)
            .header("Content-Type", "application/llsd+xml")
            .body(Llsd::Undef.to_llsd_xml())
    } else {
        http.delete(url)
    };
    report_answer(what, request);
}

/// POSTs `{group}` to the `AcceptGroupInvite` or `DeclineGroupInvite`
/// capability `url` (blocking), the answer to a group invitation that was
/// kept, and logs an answer the grid did not take.
pub(crate) fn answer_group_invitation(url: &str, body: String, accept: bool) {
    let what = if accept {
        "the AcceptGroupInvite request"
    } else {
        "the DeclineGroupInvite request"
    };
    let Some(http) = answer_client(what) else {
        return;
    };
    let request = http
        .post(url)
        .header("Content-Type", "application/llsd+xml")
        .body(body);
    report_answer(what, request);
}

/// The blocking HTTP client an answer is sent with, or `None` (logged) where
/// it cannot be built.
fn answer_client(what: &str) -> Option<reqwest::blocking::Client> {
    match crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    {
        Ok(http) => Some(http),
        Err(error) => {
            tracing::warn!("could not build the HTTP client for {what}: {error}");
            None
        }
    }
}

/// Sends `request`, an answer to an offer or an invitation, and logs it where
/// the grid did not answer `{ success: true }`.
fn report_answer(what: &str, request: reqwest::blocking::RequestBuilder) {
    match request.header("Accept", "application/llsd+xml").send() {
        Ok(response) if response.status().is_success() => {
            let text = response.text().unwrap_or_default();
            let success = parse_llsd_xml(&text)
                .ok()
                .and_then(|reply| reply.get("success").and_then(Llsd::as_bool));
            if success != Some(true) {
                tracing::warn!("{what} was not taken: {text}");
            }
        }
        Ok(response) => tracing::warn!(status = %response.status(), "{what} was rejected"),
        Err(error) => tracing::warn!("{what} could not be sent: {error}"),
    }
}

/// Changes a group's ban list over the `GroupAPIv1` capability `url` (the
/// group's `?group_id=` URL, blocking) where `change` is a POST body, then
/// reads the list and forwards it over `caps_tx` tagged [`CAP_GROUP_API_V1`],
/// as the reference re-reads it after a change.
pub(crate) fn group_bans(url: &str, change: Option<String>, caps_tx: &Sender<(String, Llsd)>) {
    let Some(http) = answer_client("a group's ban list") else {
        return;
    };
    if let Some(body) = change {
        match http
            .post(url)
            .header("Content-Type", "application/llsd+xml")
            .header("Accept", "application/llsd+xml")
            .body(body)
            .send()
        {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                tracing::warn!(status = %response.status(), "a group ban change was rejected");
            }
            Err(error) => tracing::warn!("a group ban change could not be sent: {error}"),
        }
    }
    match http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
    {
        Ok(response) if response.status().is_success() => {
            let text = response.text().unwrap_or_default();
            match parse_llsd_xml(&text) {
                Ok(llsd) => deliver(caps_tx, (CAP_GROUP_API_V1.to_owned(), llsd)),
                Err(error) => tracing::warn!("a group's ban list did not parse: {error}"),
            }
        }
        Ok(response) => {
            tracing::warn!(status = %response.status(), "a group's ban list was refused");
        }
        Err(error) => tracing::warn!("a group's ban list could not be read: {error}"),
    }
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

/// POSTs `body` to the `cap` capability URL (blocking), ignoring the reply as
/// the reference viewer does, but reporting a POST that could not be sent or
/// was rejected as a failure of `cap` (over `caps_tx`), so an edit the grid
/// refused is not mistaken for one it took. Mirrors the tokio
/// `post_caps_reporting`.
pub(crate) fn run_caps_reporting(
    cap_url: &str,
    body: String,
    cap: &str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let http = match crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    {
        Ok(http) => http,
        Err(error) => {
            tracing::warn!("could not build the HTTP client for the {cap} POST: {error}");
            report_caps_failure(caps_tx, cap);
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
            tracing::warn!(status = %response.status(), "the {cap} POST was rejected");
            report_caps_failure(caps_tx, cap);
        }
        Err(error) => {
            tracing::warn!("the {cap} POST could not be sent: {error}");
            report_caps_failure(caps_tx, cap);
        }
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
    let status = response.status();
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, CAP_CHAT_SESSION_REQUEST);
        return;
    };
    if !status.is_success() {
        tracing::warn!(
            capability = CAP_CHAT_SESSION_REQUEST,
            %status,
            body = text.chars().take(200).collect::<String>(),
            "ChatSessionRequest refused the request"
        );
    }
    let Ok(reply) = parse_llsd_xml(&text) else {
        tracing::warn!(
            capability = CAP_CHAT_SESSION_REQUEST,
            %status,
            body = text.chars().take(200).collect::<String>(),
            "ChatSessionRequest answered with something that is not LLSD"
        );
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
    let status = response.status();
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG);
        return;
    };
    if !status.is_success() {
        tracing::warn!(
            capability = CHAT_SESSION_FETCH_HISTORY_TAG,
            %status,
            body = text.chars().take(200).collect::<String>(),
            "ChatSessionRequest refused the request"
        );
    }
    let Ok(reply) = parse_llsd_xml(&text) else {
        tracing::warn!(
            capability = CHAT_SESSION_FETCH_HISTORY_TAG,
            %status,
            body = text.chars().take(200).collect::<String>(),
            "ChatSessionRequest answered with something that is not LLSD"
        );
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

/// GETs the `ExperienceQuery` capability (blocking) and forwards its reply to
/// `caps_tx` tagged [`EXPERIENCE_QUERY_TAG`], stamping the queried `parcel_id`
/// into the reply map — the answer names only experiences, so without the stamp
/// it could not be told from one about a parcel the agent has already walked
/// off. Mirrors the tokio `get_experience_query`.
pub(crate) fn run_experience_query(url: &str, parcel_id: i32, caps_tx: &Sender<(String, Llsd)>) {
    let Some(reply) = blocking_get_llsd(url, EXPERIENCE_QUERY_TAG) else {
        report_caps_failure(caps_tx, EXPERIENCE_QUERY_TAG);
        return;
    };
    let mut map = match reply {
        Llsd::Map(map) => map,
        _other => HashMap::new(),
    };
    let _previous = map.insert("parcelid".to_owned(), Llsd::Integer(parcel_id));
    deliver(caps_tx, (EXPERIENCE_QUERY_TAG.to_owned(), Llsd::Map(map)));
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

/// POSTs the `RemoteParcelRequest` capability (blocking) and forwards its reply
/// to `caps_tx` tagged [`CAP_REMOTE_PARCEL_REQUEST`], stamping the `request` it
/// answers into the reply map.
///
/// The grid answers a bare `{ parcel_id }`, which names neither the location nor
/// the region asked about; two resolves in flight would be indistinguishable,
/// and handing a caller the wrong parcel is worse than handing it none. The
/// capability is a per-request POST, so this thread **holds** the question
/// across it and [`stamp_remote_parcel_request`] writes it back into the answer
/// — the same trick `run_avatar_picker_search` plays with its query id. Mirrors
/// the tokio `post_remote_parcel_request`.
pub(crate) fn run_remote_parcel_request(
    cap_url: &str,
    request: RemoteParcelRequest,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let body =
        build_remote_parcel_request(request.location, request.region_id, request.region_handle);
    let Ok(http) = crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, CAP_REMOTE_PARCEL_REQUEST);
        return;
    };
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        report_caps_failure(caps_tx, CAP_REMOTE_PARCEL_REQUEST);
        return;
    };
    let Ok(text) = response.text() else {
        report_caps_failure(caps_tx, CAP_REMOTE_PARCEL_REQUEST);
        return;
    };
    let Ok(reply) = parse_llsd_xml(&text) else {
        report_caps_failure(caps_tx, CAP_REMOTE_PARCEL_REQUEST);
        return;
    };
    deliver(
        caps_tx,
        (
            CAP_REMOTE_PARCEL_REQUEST.to_owned(),
            stamp_remote_parcel_request(reply, &request),
        ),
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

/// Runs an `ExtEnvironment` PUT or DELETE (`change`, which delivers its reply
/// to the sender it is given) and forwards that reply to `caps_tx` — or, when
/// the grid took the change without saying what the land has now, GETs
/// `refetch_url` and forwards that instead. OpenSim answers a set with a bare
/// `{success: true}`; a caller is owed the stored settings on either grid.
/// Mirrors the tokio `settle_environment_change`.
pub(crate) fn run_environment_change(
    change: impl FnOnce(&Sender<(String, Llsd)>),
    refetch_url: &str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let (reply_tx, reply_rx) = crossbeam_channel::unbounded();
    change(&reply_tx);
    drop(reply_tx);
    for (message, body) in reply_rx.try_iter() {
        if message == sl_proto::CAP_EXT_ENVIRONMENT
            && sl_proto::environment_reply_needs_refetch(&body)
        {
            run_get_caps_llsd(refetch_url, sl_proto::CAP_EXT_ENVIRONMENT, caps_tx);
        } else {
            deliver(caps_tx, (message, body));
        }
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

/// GETs a prim's contents over the `RequestTaskInventory` capability
/// (blocking) and forwards the listing to `caps_tx` tagged
/// [`CAP_REQUEST_TASK_INVENTORY`], stamped with the prim asked about (the
/// answer does not name it). A `304`, the grid's "the listing you hold is
/// current", is forwarded under [`TASK_INVENTORY_UNCHANGED_TAG`] with the
/// serial the request named. Mirrors the tokio `get_task_inventory`.
///
/// A transient status is retried with the shared backoff, as every one-shot
/// capability GET is; any other failure is forwarded under
/// [`TASK_INVENTORY_FAILED_TAG`], on which the session reads the prim over UDP
/// instead.
pub(crate) fn run_task_inventory(
    base: &str,
    task: ObjectKey,
    held_serial: Option<i16>,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let url = task_inventory_cap_url(base, task, held_serial);
    let Ok(http) = crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        let failed = task_inventory_failed_body(task, held_serial);
        deliver(caps_tx, (TASK_INVENTORY_FAILED_TAG.to_owned(), failed));
        return;
    };
    for attempt in 0..=MAX_TRANSIENT_RETRIES {
        match http
            .get(&url)
            .header("Accept", "application/llsd+xml")
            .send()
        {
            Ok(response) if response.status() == reqwest::StatusCode::NOT_MODIFIED => {
                if let Some(serial) = held_serial {
                    let body = task_inventory_unchanged_body(task, serial);
                    deliver(caps_tx, (TASK_INVENTORY_UNCHANGED_TAG.to_owned(), body));
                    return;
                }
                tracing::warn!("a task inventory GET that named no serial was answered 304");
                break;
            }
            Ok(response) if is_transient_status(response.status()) => {
                tracing::debug!(
                    status = %response.status(),
                    attempt,
                    "a task inventory GET answered a transient status"
                );
            }
            Ok(response) if !response.status().is_success() => {
                tracing::warn!(status = %response.status(), "a task inventory GET was rejected");
                break;
            }
            Ok(response) => {
                let listing = response
                    .text()
                    .ok()
                    .and_then(|text| parse_llsd_xml(&text).ok());
                if let Some(listing) = listing {
                    let stamped = stamp_task_inventory_cap_reply(listing, task);
                    deliver(caps_tx, (CAP_REQUEST_TASK_INVENTORY.to_owned(), stamped));
                    return;
                }
                tracing::warn!("a task inventory GET's answer could not be read");
                break;
            }
            Err(error) => {
                tracing::debug!(%error, attempt, "a task inventory GET failed");
            }
        }
        if attempt < MAX_TRANSIENT_RETRIES {
            std::thread::sleep(transient_backoff(attempt));
        }
    }
    let failed = task_inventory_failed_body(task, held_serial);
    deliver(caps_tx, (TASK_INVENTORY_FAILED_TAG.to_owned(), failed));
}

/// POSTs a `GetMetadata` request for the script `item` in the prim `task`
/// (blocking) and forwards the answer to `caps_tx` tagged
/// [`CAP_GET_METADATA`], stamped with the two ids (the answer names
/// neither). Mirrors the tokio `post_script_metadata`.
pub(crate) fn run_script_metadata(
    cap_url: &str,
    task: ObjectKey,
    item: InventoryKey,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let body = build_get_metadata_request(task, item);
    match crate::voice::post_cap_llsd(cap_url, body, CAP_GET_METADATA) {
        Some(reply) => {
            let stamped = stamp_get_metadata_reply(reply, task, item);
            deliver(caps_tx, (CAP_GET_METADATA.to_owned(), stamped));
        }
        None => report_caps_failure(caps_tx, CAP_GET_METADATA),
    }
}

/// How many times a profile image upload is tried, each time with a fresh
/// uploader, as the reference does (`post_profile_image`).
const PROFILE_IMAGE_ATTEMPTS: u32 = 3;

/// PUTs `body` to an `AgentProfile` URL (blocking) — the agent's own
/// profile, or its notes about another avatar — and logs an answer the grid
/// did not take. Mirrors the tokio `put_agent_profile`.
pub(crate) fn put_agent_profile(url: &str, body: String) {
    let Some(http) = answer_client("a profile write") else {
        return;
    };
    match http
        .put(url)
        .header("Content-Type", "application/llsd+xml")
        .header("Accept", "application/llsd+xml")
        .body(body)
        .send()
    {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(status = %response.status(), "a profile write was rejected");
        }
        Err(error) => tracing::warn!("a profile write could not be sent: {error}"),
    }
}

/// POSTs a display-name change to the `SetDisplayName` capability `url`
/// (blocking). The grid answers over the event queue, so only a request it did
/// not take is logged. Mirrors the tokio `set_display_name`.
pub(crate) fn set_display_name(url: &str, body: String) {
    let Some(http) = answer_client("a display-name change") else {
        return;
    };
    match http
        .post(url)
        .header("Content-Type", "application/llsd+xml")
        .header("Accept", "application/llsd+xml")
        .header("Accept-Language", "en")
        .body(body)
        .send()
    {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(status = %response.status(), "a display-name change was rejected");
        }
        Err(error) => tracing::warn!("a display-name change could not be sent: {error}"),
    }
}

/// Reads `classified`'s click counts over the `SearchStatRequest` capability
/// `url` (blocking) and forwards the stamped answer over `caps_tx`. Mirrors
/// the tokio `search_stat`.
pub(crate) fn search_stat(url: &str, classified: ClassifiedKey, caps_tx: &Sender<(String, Llsd)>) {
    let Some(http) = answer_client("a classified's click counts") else {
        return;
    };
    match http
        .post(url)
        .header("Content-Type", "application/llsd+xml")
        .header("Accept", "application/llsd+xml")
        .body(search_stat_request_body(classified))
        .send()
    {
        Ok(response) if response.status().is_success() => {
            let text = response.text().unwrap_or_default();
            match parse_llsd_xml(&text) {
                Ok(answer) => deliver(
                    caps_tx,
                    (
                        CAP_SEARCH_STAT_REQUEST.to_owned(),
                        stamp_search_stat_answer(answer, classified),
                    ),
                ),
                Err(error) => tracing::warn!("a classified's click counts did not parse: {error}"),
            }
        }
        Ok(response) => {
            tracing::warn!(status = %response.status(), "a classified's click counts were refused");
        }
        Err(error) => tracing::warn!("a classified's click counts could not be read: {error}"),
    }
}

/// Uploads `j2c` as the profile image `image` over the
/// `UploadAgentProfileImage` capability `url` (blocking) — `None` where the
/// region has none — and hands the outcome over `caps_tx`. Mirrors the tokio
/// `upload_profile_image`.
pub(crate) fn upload_profile_image(
    url: Option<&str>,
    image: ProfileImage,
    j2c: &[u8],
    caps_tx: &Sender<(String, Llsd)>,
) {
    let asset = match (url, answer_client("a profile image upload")) {
        (Some(url), Some(http)) => (1..=PROFILE_IMAGE_ATTEMPTS).find_map(|attempt| {
            profile_image_attempt(&http, url, image, j2c)
                .inspect_err(|why| tracing::warn!(attempt, "a profile image upload failed: {why}"))
                .ok()
        }),
        (None, _) => {
            tracing::warn!("the region has no UploadAgentProfileImage capability");
            None
        }
        (Some(_url), None) => None,
    };
    deliver(
        caps_tx,
        (
            CAP_UPLOAD_AGENT_PROFILE_IMAGE.to_owned(),
            profile_image_upload_outcome(image, asset),
        ),
    );
}

/// One attempt at a profile image upload (blocking): ask for an uploader,
/// POST the image to it, and read the asset it made.
fn profile_image_attempt(
    http: &reqwest::blocking::Client,
    url: &str,
    image: ProfileImage,
    j2c: &[u8],
) -> Result<Uuid, String> {
    let first = http
        .post(url)
        .header("Content-Type", "application/llsd+xml")
        .header("Accept", "application/llsd+xml")
        .body(profile_image_upload_body(image))
        .send()
        .map_err(|error| error.to_string())?;
    if !first.status().is_success() {
        return Err(format!("the uploader request answered {}", first.status()));
    }
    let answer = parse_llsd_xml(&first.text().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let uploader = answer
        .get("uploader")
        .and_then(Llsd::as_str)
        .ok_or("the uploader request named no uploader")?
        .to_owned();
    let second = http
        .post(&uploader)
        .header("Content-Type", "application/jp2")
        .header("Accept", "application/llsd+xml")
        .body(j2c.to_vec())
        .send()
        .map_err(|error| error.to_string())?;
    if !second.status().is_success() {
        return Err(format!("the upload answered {}", second.status()));
    }
    let answer = parse_llsd_xml(&second.text().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    if answer.get("state").and_then(Llsd::as_str) != Some("complete") {
        return Err("the upload did not complete".to_owned());
    }
    answer
        .get("new_asset")
        .and_then(|asset| match asset {
            Llsd::Uuid(id) => Some(*id),
            Llsd::String(text) => Uuid::parse_str(text).ok(),
            _ => None,
        })
        .ok_or_else(|| "the upload named no asset".to_owned())
}
