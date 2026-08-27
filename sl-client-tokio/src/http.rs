//! Generic LLSD-over-HTTP capability helpers (GET/PUT/PATCH/DELETE).

use reqwest::Client as ReqwestClient;
use sl_proto::{
    AVATAR_PICKER_SEARCH_TAG, CAP_CHAT_SESSION_REQUEST, CAP_LAND_RESOURCES, CAP_LSL_SYNTAX,
    CHAT_SESSION_FETCH_HISTORY_TAG, LAND_RESOURCE_DETAIL_TAG, LAND_RESOURCE_SUMMARY_TAG, Llsd,
    ParcelKey, build_land_resources_request, parse_land_resources_reply, parse_llsd_xml,
};
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::caps::{deliver, report_caps_failure};
use crate::lsl_syntax_cache::LslSyntaxCache;

/// POSTs `body` to a capability URL and ignores the *reply*: the shared body of
/// every fire-and-forget capability call, where the simulator answers with an
/// HTTP status and nothing else.
///
/// `what` names the request family for the log. Because there is no event to
/// carry an outcome, a transport failure or a rejecting status is logged here
/// rather than discarded — that line is the only trace such a call leaves. The
/// capability URL is deliberately **not** logged: it carries the region's
/// per-session cap token.
pub(crate) async fn post_llsd_oneway(
    cap_url: &str,
    body: String,
    http: &ReqwestClient,
    what: &str,
) {
    match http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(status = %response.status(), "{what} was rejected");
        }
        Err(error) => tracing::warn!("{what} could not be sent: {error}"),
    }
}

/// POSTs `body` to a capability URL and ignores the reply — a fire-and-forget
/// capability call where the simulator returns only an HTTP status (e.g. the
/// `SendUserReport` abuse-report cap). There is no event.
pub(crate) async fn post_caps_oneway(cap_url: String, body: String, http: ReqwestClient) {
    post_llsd_oneway(&cap_url, body, &http, "a fire-and-forget capability POST").await;
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
    deliver(
        &caps_tx,
        (CAP_CHAT_SESSION_REQUEST.to_owned(), Llsd::Map(map)),
    )
    .await;
}

/// POSTs a `ChatSessionRequest` `fetch history` `body` to the cap URL and
/// forwards the reply to `caps_tx` tagged
/// [`CHAT_SESSION_FETCH_HISTORY_TAG`] — the synthetic routing tag, because the
/// reply is a **bare LLSD array** (the session's server-side backlog,
/// oldest-first) that a plain [`CAP_CHAT_SESSION_REQUEST`] tag would misroute
/// into the roster decoder. Like the roster path above, the reply carries no
/// session identity of its own, so it is wrapped as
/// `{ "history": <array>, "session-id": <uuid>, "from_group": <bool> }` for
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) to
/// rebuild the session kind.
pub(crate) async fn post_chat_session_fetch_history(
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
        report_caps_failure(&caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG).await;
        return;
    };
    let Ok(text) = response.text().await else {
        report_caps_failure(&caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG).await;
        return;
    };
    let Ok(reply) = parse_llsd_xml(&text) else {
        report_caps_failure(&caps_tx, CHAT_SESSION_FETCH_HISTORY_TAG).await;
        return;
    };
    let mut map = HashMap::new();
    let _previous = map.insert("history".to_owned(), reply);
    let _previous = map.insert("session-id".to_owned(), Llsd::Uuid(session_id));
    let _previous = map.insert("from_group".to_owned(), Llsd::Boolean(from_group));
    deliver(
        &caps_tx,
        (CHAT_SESSION_FETCH_HISTORY_TAG.to_owned(), Llsd::Map(map)),
    )
    .await;
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
            deliver(&caps_tx, (cap.to_owned(), llsd)).await;
        }
        None => report_caps_failure(&caps_tx, cap).await,
    }
}

/// GETs the `AvatarPickerSearch` capability and forwards its reply to `caps_tx`
/// tagged [`AVATAR_PICKER_SEARCH_TAG`], stamping the caller's `query_id` into
/// the reply map — the HTTP path carries no `QueryID` of its own, so without the
/// stamp the answer could not be routed back to the search that asked. Mirrors
/// the bevy `run_avatar_picker_search`.
pub(crate) async fn get_avatar_picker_search(
    url: String,
    query_id: Uuid,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Some(reply) = get_llsd(&url, &http).await else {
        report_caps_failure(&caps_tx, AVATAR_PICKER_SEARCH_TAG).await;
        return;
    };
    let mut map = match reply {
        Llsd::Map(map) => map,
        _other => HashMap::new(),
    };
    let _previous = map.insert("query-id".to_owned(), Llsd::Uuid(query_id));
    deliver(
        &caps_tx,
        (AVATAR_PICKER_SEARCH_TAG.to_owned(), Llsd::Map(map)),
    )
    .await;
}

/// GETs the `LSLSyntax` capability, caches the raw document under syntax `id`,
/// and forwards its parsed LLSD to `caps_tx` tagged [`CAP_LSL_SYNTAX`] for
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) to decode
/// into [`Event::LslSyntax`](sl_proto::Event::LslSyntax).
///
/// The raw XML is cached only when it declares the schema version this client
/// supports (`llsd-lsl-syntax-version == 2`), so a document of an unknown version
/// — which the session will reject anyway — is never persisted. The parsed LLSD
/// is forwarded regardless; the session owns the version gate and logs the
/// rejection, keeping one decode path for both the fresh-fetch and cache-hit
/// cases.
pub(crate) async fn fetch_lsl_syntax(
    url: String,
    id: Uuid,
    cache: LslSyntaxCache,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .get(&url)
        .header("Accept", "application/llsd+xml")
        .send()
        .await
    else {
        report_caps_failure(&caps_tx, CAP_LSL_SYNTAX).await;
        return;
    };
    let Ok(text) = response.text().await else {
        report_caps_failure(&caps_tx, CAP_LSL_SYNTAX).await;
        return;
    };
    let Ok(llsd) = parse_llsd_xml(&text) else {
        report_caps_failure(&caps_tx, CAP_LSL_SYNTAX).await;
        return;
    };
    // Persist only a supported-version document (a cheap version-key check, not a
    // full decode — the session does that): caching an unsupported one would just
    // reproduce a reject on the next restart.
    if llsd
        .field_i32("llsd-lsl-syntax-version", "llsd-lsl-syntax-version")
        .ok()
        .flatten()
        == Some(sl_proto::LSL_SYNTAX_VERSION)
    {
        cache.store(id, &text);
    }
    deliver(&caps_tx, (CAP_LSL_SYNTAX.to_owned(), llsd)).await;
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
    deliver(&caps_tx, (CAP_LAND_RESOURCES.to_owned(), reply)).await;

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
            deliver(&caps_tx, (cap.to_owned(), llsd)).await;
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
            deliver(&caps_tx, (cap.to_owned(), llsd)).await;
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
            deliver(&caps_tx, (cap.to_owned(), llsd)).await;
        }
        Err(_error) => report_caps_failure(&caps_tx, cap).await,
    }
}
