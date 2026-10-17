//! Voice capability provisioning and signaling.

use reqwest::Client as ReqwestClient;
use sl_proto::{Llsd, parse_llsd_xml};
use tokio::sync::mpsc;

use crate::caps::{deliver, report_caps_failure};

/// POSTs a voice-signalling capability (`ProvisionVoiceAccountRequest` or
/// `ParcelVoiceInfoRequest`) carrying the prepared `body`, forwarding the LLSD
/// reply back over `caps_tx` tagged with `cap` so the session decodes it into
/// the matching event ([`Event::VoiceAccountProvisioned`] /
/// [`Event::ParcelVoiceInfo`]). Only the grid signalling is handled here; the
/// audio session is out of scope.
pub(crate) async fn post_voice_cap(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    match post_cap_llsd(&cap_url, body, cap, &http).await {
        Some(llsd) => deliver(&caps_tx, (cap.to_owned(), llsd)).await,
        None => report_caps_failure(&caps_tx, cap).await,
    }
}

/// POSTs an LLSD `body` to the `cap` capability at `cap_url` and parses the
/// LLSD reply, or `None` when the request could not be sent, the grid refused
/// it, or the reply could not be read or parsed — which the caller reports as
/// a failed capability request. Each of those is logged with the status and, for
/// a refusal, the body the grid sent back: a refused `UpdateExperience` or
/// `RegionExperiences` POST says why only there (the URL is not logged — it
/// carries the capability's secret).
pub(crate) async fn post_cap_llsd(
    cap_url: &str,
    body: String,
    cap: &str,
    http: &ReqwestClient,
) -> Option<Llsd> {
    let response = match http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(capability = cap, %error, "a CAPS POST could not be sent");
            return None;
        }
    };
    let status = response.status();
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(capability = cap, %status, %error, "a CAPS POST reply could not be read");
            return None;
        }
    };
    if !status.is_success() {
        tracing::warn!(capability = cap, %status, body = %text, "a CAPS POST was refused");
        return None;
    }
    match parse_llsd_xml(&text) {
        Ok(llsd) => Some(llsd),
        Err(error) => {
            tracing::warn!(capability = cap, %error, "a CAPS POST reply did not parse");
            None
        }
    }
}

/// POSTs a `VoiceSignalingRequest` (WebRTC ICE trickle). Fire-and-forget: the
/// simulator returns only an HTTP status, so there is no event to surface.
pub(crate) async fn post_voice_signaling(cap_url: String, body: String, http: ReqwestClient) {
    crate::http::post_llsd_oneway(&cap_url, body, &http, "a voice-signaling POST").await;
}
