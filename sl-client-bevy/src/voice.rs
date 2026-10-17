//! Voice capability provisioning and signaling.

use crate::caps::report_caps_failure;
use crate::{EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use crossbeam_channel::Sender;
use sl_proto::{Llsd, parse_llsd_xml};

/// POSTs a voice-signalling capability (`ProvisionVoiceAccountRequest` or
/// `ParcelVoiceInfoRequest`) carrying the prepared `body` and forwards the LLSD
/// reply to `caps_tx` tagged with `cap`, for the session to surface as the
/// matching event ([`SlSessionEvent::VoiceAccountProvisioned`] /
/// [`SlSessionEvent::ParcelVoiceInfo`]). Only the grid signalling is handled;
/// the audio session is out of scope.
pub(crate) fn run_voice_cap(
    cap_url: &str,
    body: String,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    match post_cap_llsd(cap_url, body, cap) {
        Some(llsd) => deliver(caps_tx, (cap.to_owned(), llsd)),
        None => report_caps_failure(caps_tx, cap),
    }
}

/// POSTs an LLSD `body` to the `cap` capability at `cap_url` and parses the
/// LLSD reply, or `None` when the client could not be built, the request could
/// not be sent, the grid refused it, or the reply could not be read or parsed —
/// which the caller reports as a failed capability request. Each of those is
/// logged with the status and, for a refusal, the body the grid sent back: a
/// refused `UpdateExperience` or `RegionExperiences` POST says why only there
/// (the URL is not logged — it carries the capability's secret).
pub(crate) fn post_cap_llsd(cap_url: &str, body: String, cap: &str) -> Option<Llsd> {
    let http = match crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    {
        Ok(http) => http,
        Err(error) => {
            tracing::warn!(capability = cap, %error, "could not build the HTTP client for a CAPS POST");
            return None;
        }
    };
    let response = match http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(capability = cap, %error, "a CAPS POST could not be sent");
            return None;
        }
    };
    let status = response.status();
    let text = match response.text() {
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
pub(crate) fn run_voice_signaling(cap_url: &str, body: String) {
    crate::http::post_llsd_oneway(cap_url, body, "a voice-signaling POST");
}
