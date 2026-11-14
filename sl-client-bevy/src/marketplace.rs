//! Second Life Marketplace (SLM) DirectDelivery requests (blocking).
//!
//! The SLM API is plain JSON (the only non-LLSD HTTP transport), so
//! its replies do not ride the `(String, Llsd)` caps channel: each
//! request runs the pre-built [`MarketplaceRequest`] against the
//! `DirectDelivery` capability URL on its own thread and forwards the
//! fully-formed session event produced by the shared sans-I/O mapping
//! in `sl-proto` (`marketplace_reply_event` /
//! `marketplace_failure_event`) over `asset_tx` — the same
//! direct-event side channel the binary asset fetches and experience
//! fetches use.

use crate::{Caps, EVENT_QUEUE_TIMEOUT, NetOutbound, deliver, report};
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::Event as SessionEvent;
use sl_proto::{
    CAP_DIRECT_DELIVERY, MarketplaceBuildRequestError, MarketplaceMethod, MarketplaceOperation,
    MarketplaceRequest, marketplace_failure_event, marketplace_reply_event,
};

/// Dispatch one SLM command: spawn the HTTP request thread when the region's
/// `DirectDelivery` capability is present and the request body built,
/// otherwise send the per-operation failure event inline (viewer parity: the
/// reference viewer's empty-capability-URL path reports a connection failure
/// rather than dropping the command silently — this keeps the commands
/// observable on grids without the capability, e.g. OpenSim, and before the
/// region's capabilities have arrived at all).
///
/// The inline failure goes straight to the app side over `outbound`, since
/// before the capabilities arrive there is no asset channel to carry it.
pub(crate) fn dispatch_marketplace_request(
    caps: Option<&Caps>,
    operation: MarketplaceOperation,
    request: Result<MarketplaceRequest, MarketplaceBuildRequestError>,
    outbound: &Sender<NetOutbound>,
) {
    let target = caps.and_then(|caps| {
        caps.map
            .get(CAP_DIRECT_DELIVERY)
            .map(|url| (url.clone(), caps.asset_tx.clone()))
    });
    let failure_reason = match (target, request) {
        (Some((url, asset_tx)), Ok(request)) => {
            crate::log_context::spawn_thread(move || {
                run_marketplace_request(&url, operation, request, &asset_tx);
            });
            return;
        }
        (None, _) if caps.is_none() => "the region's capabilities have not arrived yet".to_owned(),
        (None, _) => "no DirectDelivery capability (not granted by this region/grid)".to_owned(),
        (_, Err(e)) => e.to_string(),
    };
    report(
        outbound,
        NetOutbound::Event(marketplace_failure_event(operation, failure_reason)),
    );
}

/// Run one SLM request against the `DirectDelivery` capability base
/// URL (blocking) and forward the resulting session event over
/// `asset_tx`.
///
/// The route path is appended to the capability URL verbatim
/// (reference-viewer `getSLMConnectURL` semantics). JSON `Accept` /
/// `Content-Type` headers are sent for every route except the
/// merchant probe, which sends neither — also reference-viewer
/// parity. (reqwest's default redirect policy may downgrade a
/// redirected POST/PUT to GET; the reference viewer only enables
/// redirect-following on the probe, so that is acceptable here.)
pub(crate) fn run_marketplace_request(
    cap_url: &str,
    operation: MarketplaceOperation,
    request: MarketplaceRequest,
    asset_tx: &Sender<SessionEvent>,
) {
    let event = perform_marketplace_request(cap_url, operation, request);
    deliver(asset_tx, event);
}

/// Perform the HTTP round-trip and map the outcome to a session
/// event.
fn perform_marketplace_request(
    cap_url: &str,
    operation: MarketplaceOperation,
    request: MarketplaceRequest,
) -> SessionEvent {
    let http = match ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    {
        Ok(http) => http,
        Err(e) => {
            return marketplace_failure_event(
                operation,
                format!("failed to build HTTP client: {e}"),
            );
        }
    };
    let url = format!("{cap_url}{}", request.path);
    let mut builder = match request.method {
        MarketplaceMethod::Get => http.get(&url),
        MarketplaceMethod::Post => http.post(&url),
        MarketplaceMethod::Put => http.put(&url),
        MarketplaceMethod::Delete => http.delete(&url),
    };
    if request.json_headers {
        builder = builder
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    match builder.send() {
        Ok(response) => {
            let status = response.status().as_u16();
            match response.text() {
                Ok(body) => marketplace_reply_event(operation, status, &body),
                Err(e) => marketplace_failure_event(
                    operation,
                    format!("failed to read SLM reply body: {e}"),
                ),
            }
        }
        Err(e) => marketplace_failure_event(operation, format!("SLM request failed: {e}")),
    }
}
