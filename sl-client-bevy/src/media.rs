//! Object-media capability fetch and update.

use crate::{EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use crossbeam_channel::Sender;
use sl_proto::{CAP_OBJECT_MEDIA, Llsd, ObjectKey, build_object_media_get_request, parse_llsd_xml};

/// POSTs an `ObjectMedia` GET for `object_id` and forwards the decoded LLSD
/// response to `caps_tx` tagged [`CAP_OBJECT_MEDIA`], for the session to surface
/// as a [`SlSessionEvent::ObjectMedia`].
pub(crate) fn run_object_media_fetch(
    cap_url: &str,
    object_id: ObjectKey,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = crate::http_proxy::blocking_client_builder()
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
        deliver(caps_tx, (CAP_OBJECT_MEDIA.to_owned(), llsd));
    }
}

/// POSTs a pre-built LLSD-XML `body` to a capability `cap_url`, fire-and-forget:
/// no reply is awaited. Used where the simulator acts on the POST and surfaces
/// the result out-of-band rather than in the POST response — an `ObjectMedia`
/// UPDATE / `ObjectMediaNavigate` advances the object's media version (observed
/// by re-fetching with [`Command::RequestObjectMedia`]), and a
/// `CopyInventoryFromNotecard` copy arrives over the normal inventory-update
/// stream.
pub(crate) fn post_caps_llsd_oneway(cap_url: &str, body: String) {
    crate::http::post_llsd_oneway(cap_url, body, "an out-of-band capability POST");
}
