//! Voice capability provisioning and signaling.

use crate::EVENT_QUEUE_TIMEOUT;
use crate::caps::report_caps_failure;
use bevy::prelude::*;
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
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
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        report_caps_failure(caps_tx, cap);
        return;
    };
    let Ok(response) = http
        .post(cap_url)
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

/// POSTs a `VoiceSignalingRequest` (WebRTC ICE trickle). Fire-and-forget: the
/// simulator returns only an HTTP status, so there is no event to surface.
pub(crate) fn run_voice_signaling(cap_url: &str, body: String) {
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
