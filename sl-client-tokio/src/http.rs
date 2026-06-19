//! Generic LLSD-over-HTTP capability helpers (GET/PUT/PATCH/DELETE).

use reqwest::Client as ReqwestClient;
use sl_proto::{Llsd, parse_llsd_xml};
use tokio::sync::mpsc;

/// GETs `url` and parses the LLSD-XML reply, returning `None` on any
/// transport/parse failure. Shared by the experience capability fetches.
pub(crate) async fn get_llsd(url: &str, http: &ReqwestClient) -> Option<Llsd> {
    let response = http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
        .await
        .ok()?;
    let text = response.text().await.ok()?;
    parse_llsd_xml(&text).ok()
}

/// GETs an experience capability URL and forwards its LLSD reply to `caps_tx`
/// tagged `cap`, for the session to decode in
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) into the
/// matching experience event.
pub(crate) async fn get_caps_llsd(
    url: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    if let Some(llsd) = get_llsd(&url, &http).await {
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// PUTs `body` to an experience capability URL (the `Allow`/`Block` preference
/// set) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) async fn put_caps_llsd(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .put(&cap_url)
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
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// Sends an HTTP PATCH of `body` to an AIS3 inventory capability URL (a folder /
/// item update or move) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) async fn patch_caps_llsd(
    cap_url: String,
    body: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .patch(&cap_url)
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
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}

/// Sends an HTTP DELETE to an experience capability URL (the `Forget`
/// preference) and forwards the LLSD reply to `caps_tx` tagged `cap`.
pub(crate) async fn delete_caps_llsd(
    cap_url: String,
    cap: &'static str,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Ok(response) = http
        .delete(&cap_url)
        .header("Accept", "application/llsd+xml")
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((cap.to_owned(), llsd)).await.ok();
    }
}
