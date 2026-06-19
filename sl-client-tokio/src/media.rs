//! Object-media capability fetch and update.

use reqwest::Client as ReqwestClient;
use sl_proto::{CAP_OBJECT_MEDIA, Llsd, Uuid, build_object_media_get_request, parse_llsd_xml};
use tokio::sync::mpsc;

/// POSTs an `ObjectMedia` GET for `object_id`, forwarding the decoded LLSD
/// response back over `caps_tx` to be surfaced as an [`Event::ObjectMedia`].
pub(crate) async fn fetch_object_media(
    cap_url: String,
    object_id: Uuid,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_object_media_get_request(object_id);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_OBJECT_MEDIA.to_owned(), llsd)).await.ok();
    }
}

/// POSTs an `ObjectMedia` UPDATE (or, with `navigate`, an `ObjectMediaNavigate`)
/// to set the per-face media of `object_id`. Both are fire-and-forget: the
/// simulator advances the object's media version rather than replying with
/// media, so there is no event to surface — a client re-fetches with
/// [`Command::RequestObjectMedia`] to observe the change.
pub(crate) async fn post_object_media(cap_url: String, body: String, http: ReqwestClient) {
    http.post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
        .ok();
}
