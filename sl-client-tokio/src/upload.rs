//! Two-step NewFileAgentInventory / UploadBakedTexture asset upload.

use reqwest::Client as ReqwestClient;
use sl_proto::{Event, parse_asset_upload_response};
use tokio::sync::mpsc;

/// Runs the modern two-step CAPS asset upload: POST the LLSD `metadata` to the
/// capability `cap_url` to obtain an `uploader` URL, then POST the raw `data`
/// bytes there. Surfaces the outcome as [`Event::AssetUploaded`] on success or
/// [`Event::AssetUploadFailed`] on any failure. Shared by the
/// `NewFileAgentInventory`, `UploadBakedTexture`, and `Update*AgentInventory`
/// uploads, whose responses share the `{ state, uploader, new_asset,
/// new_inventory_item }` shape.
pub(crate) async fn run_caps_upload(
    cap_url: String,
    metadata: String,
    data: Vec<u8>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = caps_upload_event(&cap_url, metadata, data, &http).await;
    events.send(event).await.ok();
}

/// Performs both steps of a CAPS asset upload and returns the resulting event.
pub(crate) async fn caps_upload_event(
    cap_url: &str,
    metadata: String,
    data: Vec<u8>,
    http: &ReqwestClient,
) -> Event {
    // Step 1: POST the metadata, expecting an `uploader` URL back.
    let uploader = match caps_upload_step(
        http,
        cap_url,
        "application/llsd+xml",
        metadata.into_bytes(),
    )
    .await
    {
        Ok(response) => match response.uploader {
            Some(url) => url,
            None => {
                return Event::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("upload metadata rejected (state {})", response.state)
                    }),
                };
            }
        },
        Err(reason) => return Event::AssetUploadFailed { reason },
    };
    // Step 2: POST the raw asset bytes to the uploader URL.
    match caps_upload_step(http, &uploader, "application/octet-stream", data).await {
        Ok(response) => match response.new_asset {
            Some(new_asset) => Event::AssetUploaded {
                new_asset,
                new_inventory_item: response.new_inventory_item,
            },
            None => Event::AssetUploadFailed {
                reason: response.error.unwrap_or_else(|| {
                    format!("upload did not complete (state {})", response.state)
                }),
            },
        },
        Err(reason) => Event::AssetUploadFailed { reason },
    }
}

/// POSTs one step of a CAPS upload and parses the LLSD response, returning the
/// parsed [`AssetUploadResponse`] or a human-readable failure reason.
pub(crate) async fn caps_upload_step(
    http: &ReqwestClient,
    url: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<sl_proto::AssetUploadResponse, String> {
    let response = http
        .post(url)
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .await
        .map_err(|error| format!("upload request failed: {error}"))?;
    let text = response
        .text()
        .await
        .map_err(|error| format!("upload response read failed: {error}"))?;
    parse_asset_upload_response(&text)
        .map_err(|error| format!("upload response parse failed: {error}"))
}
