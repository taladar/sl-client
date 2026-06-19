//! Object-media capability fetch and update.

use crate::EVENT_QUEUE_TIMEOUT;
use bevy::prelude::*;
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{CAP_OBJECT_MEDIA, Llsd, Uuid, build_object_media_get_request, parse_llsd_xml};

/// POSTs an `ObjectMedia` GET for `object_id` and forwards the decoded LLSD
/// response to `caps_tx` tagged [`CAP_OBJECT_MEDIA`], for the session to surface
/// as a [`SlSessionEvent::ObjectMedia`].
pub(crate) fn run_object_media_fetch(
    cap_url: &str,
    object_id: Uuid,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_object_media_get_request(object_id);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_OBJECT_MEDIA.to_owned(), llsd)).ok();
    }
}

/// POSTs a pre-built `ObjectMedia` UPDATE or `ObjectMediaNavigate` `body` to
/// `cap_url`. Fire-and-forget: the simulator advances the object's media version
/// rather than replying with media, so a client re-fetches with
/// [`Command::RequestObjectMedia`] to observe the change.
pub(crate) fn run_object_media_post(cap_url: &str, body: String) {
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
