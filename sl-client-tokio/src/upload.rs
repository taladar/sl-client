//! Two-step NewFileAgentInventory / UploadBakedTexture asset upload.

use reqwest::Client as ReqwestClient;
use sl_proto::{Event, ScriptCompileError, parse_asset_upload_response};
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

/// Runs a two-step **script** upload (`UpdateScriptAgent` / `UpdateScriptTask`)
/// and surfaces the simulator's compile result as [`Event::ScriptUploaded`]. A
/// transport-level failure (missing uploader, HTTP/parse error, or a bare error
/// completion) surfaces as [`Event::AssetUploadFailed`] instead. `running` is the
/// requested run state echoed back for a task-inventory upload (`None` for agent
/// inventory).
pub(crate) async fn run_script_upload(
    cap_url: String,
    metadata: String,
    source: Vec<u8>,
    running: Option<bool>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = script_upload_event(&cap_url, metadata, source, running, &http).await;
    events.send(event).await.ok();
}

/// Performs both steps of a script upload and maps the completion to an event.
async fn script_upload_event(
    cap_url: &str,
    metadata: String,
    source: Vec<u8>,
    running: Option<bool>,
    http: &ReqwestClient,
) -> Event {
    // Step 1: POST the metadata (item/task ids + compile target), get an uploader.
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
                        format!("script upload metadata rejected (state {})", response.state)
                    }),
                };
            }
        },
        Err(reason) => return Event::AssetUploadFailed { reason },
    };
    // Step 2: POST the raw source; the completion carries the compile result.
    match caps_upload_step(http, &uploader, "application/octet-stream", source).await {
        Ok(response) => {
            // A completion with neither a compile result nor a stored asset is a
            // transport/permission error, not a (failed) compile.
            if response.compiled.is_none() && response.new_asset.is_none() {
                return Event::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("script upload did not complete (state {})", response.state)
                    }),
                };
            }
            Event::ScriptUploaded {
                new_asset: response.new_asset,
                new_inventory_item: response.new_inventory_item,
                // A grid that completed but omitted `compiled` is treated as a
                // clean compile.
                compiled: response.compiled.unwrap_or(true),
                errors: response
                    .errors
                    .iter()
                    .map(|error| ScriptCompileError::parse(error))
                    .collect(),
                running,
            }
        }
        Err(reason) => Event::AssetUploadFailed { reason },
    }
}

/// Files an abuse report bearing a snapshot over the
/// `SendUserReportWithScreenshot` capability: a two-step upload that POSTs the
/// report's LLSD body (`report_body`) to `cap_url` for an `uploader` URL, then
/// POSTs the snapshot's JPEG-2000 bytes (`screenshot`) there. Fire-and-forget
/// like the no-screenshot `SendUserReport` path — the report's outcome is not
/// surfaced as an event (mirroring the viewer, which discards the result in
/// `LLARScreenShotUploader::finishUpload`).
pub(crate) async fn run_report_screenshot_upload(
    cap_url: String,
    report_body: String,
    screenshot: Vec<u8>,
    http: ReqwestClient,
) {
    let Ok(response) = caps_upload_step(
        &http,
        &cap_url,
        "application/llsd+xml",
        report_body.into_bytes(),
    )
    .await
    else {
        return;
    };
    if let Some(uploader) = response.uploader {
        caps_upload_step(&http, &uploader, "application/octet-stream", screenshot)
            .await
            .ok();
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
