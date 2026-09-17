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
    match post_cap_llsd(cap_url, body) {
        Some(llsd) => deliver(caps_tx, (cap.to_owned(), llsd)),
        None => report_caps_failure(caps_tx, cap),
    }
}

/// POSTs an LLSD `body` to `cap_url` and parses the LLSD reply, or `None` when
/// the client, the request, the body or its parse failed — which the caller
/// reports as a failed capability request.
pub(crate) fn post_cap_llsd(cap_url: &str, body: String) -> Option<Llsd> {
    let http = crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let text = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .ok()?
        .text()
        .ok()?;
    parse_llsd_xml(&text).ok()
}

/// POSTs a `VoiceSignalingRequest` (WebRTC ICE trickle). Fire-and-forget: the
/// simulator returns only an HTTP status, so there is no event to surface.
pub(crate) fn run_voice_signaling(cap_url: &str, body: String) {
    crate::http::post_llsd_oneway(cap_url, body, "a voice-signaling POST");
}
