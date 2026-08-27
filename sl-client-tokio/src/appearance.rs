//! Server-side appearance update capability.

use reqwest::Client as ReqwestClient;
use sl_proto::{
    CAP_UPDATE_AVATAR_APPEARANCE, Llsd, build_update_avatar_appearance_request, parse_llsd_xml,
};
use tokio::sync::mpsc;

use crate::caps::{deliver, report_caps_failure};

/// POSTs the `UpdateAvatarAppearance` capability for `cof_version` (the modern
/// Second Life server-side bake), forwarding the LLSD reply back over `caps_tx`
/// to be surfaced as an [`Event::ServerAppearanceUpdate`]. The baked appearance
/// itself arrives separately as a UDP [`Event::AvatarAppearance`].
///
/// Every failure path — the POST, reading the body, parsing the LLSD — reports
/// the capability as failed rather than returning silently, so a bake request
/// that never got its reply surfaces as a
/// [`Diagnostic::ExpectedReplyMissing`](sl_proto::Diagnostic::ExpectedReplyMissing)
/// instead of the appearance simply never updating.
pub(crate) async fn request_server_appearance_update(
    cap_url: String,
    cof_version: i32,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_update_avatar_appearance_request(cof_version);
    let response = match http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!("the server appearance update POST failed: {error}");
            report_caps_failure(&caps_tx, CAP_UPDATE_AVATAR_APPEARANCE).await;
            return;
        }
    };
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!("the server appearance update reply could not be read: {error}");
            report_caps_failure(&caps_tx, CAP_UPDATE_AVATAR_APPEARANCE).await;
            return;
        }
    };
    match parse_llsd_xml(&text) {
        Ok(llsd) => {
            deliver(&caps_tx, (CAP_UPDATE_AVATAR_APPEARANCE.to_owned(), llsd)).await;
        }
        Err(error) => {
            tracing::warn!("the server appearance update reply did not parse: {error}");
            report_caps_failure(&caps_tx, CAP_UPDATE_AVATAR_APPEARANCE).await;
        }
    }
}
