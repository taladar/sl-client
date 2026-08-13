//! Second Life Marketplace (SLM) DirectDelivery requests.
//!
//! The SLM API is plain JSON (the only non-LLSD HTTP transport), so
//! its replies do not ride the `caps_tx` LLSD channel: each request
//! runs the pre-built [`MarketplaceRequest`] against the
//! `DirectDelivery` capability URL and forwards the fully-formed
//! [`Event`] produced by the shared sans-I/O mapping in `sl-proto`
//! (`marketplace_reply_event` / `marketplace_failure_event`) — the
//! same direct-event pattern the experience fetchers use.

use reqwest::Client as ReqwestClient;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use sl_proto::{
    Event, MarketplaceBuildRequestError, MarketplaceMethod, MarketplaceOperation,
    MarketplaceRequest, marketplace_failure_event, marketplace_reply_event,
};
use tokio::sync::mpsc;

/// Dispatch one SLM command: spawn the HTTP request when the
/// `DirectDelivery` capability is present and the request body built,
/// otherwise spawn a send of the per-operation failure event (viewer
/// parity: the reference viewer's empty-capability-URL path reports a
/// connection failure rather than dropping the command silently —
/// this keeps the commands observable on grids without the
/// capability, e.g. OpenSim).
pub(crate) fn dispatch_marketplace_request(
    cap_url: Option<String>,
    operation: MarketplaceOperation,
    request: Result<MarketplaceRequest, MarketplaceBuildRequestError>,
    http: &ReqwestClient,
    events: &mpsc::Sender<Event>,
) {
    let failure_reason = match (cap_url, request) {
        (Some(url), Ok(request)) => {
            tokio::spawn(run_marketplace_request(
                url,
                operation,
                request,
                http.clone(),
                events.clone(),
            ));
            return;
        }
        (None, _) => "no DirectDelivery capability (not granted by this region/grid)".to_owned(),
        (_, Err(e)) => e.to_string(),
    };
    let events = events.clone();
    tokio::spawn(async move {
        events
            .send(marketplace_failure_event(operation, failure_reason))
            .await
            .ok();
    });
}

/// Run one SLM request against the `DirectDelivery` capability base
/// URL and forward the resulting [`Event`] over `events`.
///
/// The route path is appended to the capability URL verbatim
/// (reference-viewer `getSLMConnectURL` semantics). JSON `Accept` /
/// `Content-Type` headers are sent for every route except the
/// merchant probe, which sends neither — also reference-viewer
/// parity. (reqwest's default redirect policy may downgrade a
/// redirected POST/PUT to GET; the reference viewer only enables
/// redirect-following on the probe, so that is acceptable here.)
pub(crate) async fn run_marketplace_request(
    cap_url: String,
    operation: MarketplaceOperation,
    request: MarketplaceRequest,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let url = format!("{cap_url}{}", request.path);
    let mut builder = match request.method {
        MarketplaceMethod::Get => http.get(&url),
        MarketplaceMethod::Post => http.post(&url),
        MarketplaceMethod::Put => http.put(&url),
        MarketplaceMethod::Delete => http.delete(&url),
    };
    if request.json_headers {
        builder = builder
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json");
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let event = match builder.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            match response.text().await {
                Ok(body) => marketplace_reply_event(operation, status, &body),
                Err(e) => marketplace_failure_event(
                    operation,
                    format!("failed to read SLM reply body: {e}"),
                ),
            }
        }
        Err(e) => marketplace_failure_event(operation, format!("SLM request failed: {e}")),
    };
    events.send(event).await.ok();
}
