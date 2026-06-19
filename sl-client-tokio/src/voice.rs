//! Voice capability provisioning and signaling.

use reqwest::Client as ReqwestClient;
use sl_proto::{Llsd, parse_llsd_xml};
use tokio::sync::mpsc;

use crate::caps::report_caps_failure;

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
    let Ok(response) = http
        .post(&cap_url)
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

/// POSTs a `VoiceSignalingRequest` (WebRTC ICE trickle). Fire-and-forget: the
/// simulator returns only an HTTP status, so there is no event to surface.
pub(crate) async fn post_voice_signaling(cap_url: String, body: String, http: ReqwestClient) {
    http.post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
        .ok();
}
