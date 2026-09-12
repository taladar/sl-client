//! Two-step NewFileAgentInventory / UploadBakedTexture asset upload.

use crate::caps::deliver;
use reqwest::Client as ReqwestClient;
use sl_proto::{
    AgentKey, Event, NewFileAgentInventoryRequest, ScriptCompileError, Uuid,
    parse_asset_upload_response, uploaded_inventory_item,
};
use tokio::sync::mpsc;

/// What an upload was asked to create, kept for the completion: the
/// `NewFileAgentInventory` request body plus the agent the created item belongs
/// to. `None` for the uploads that create no item — `UploadBakedTexture` and the
/// `Update*Inventory` saves onto an item that already exists.
pub(crate) type NewItemUpload = Option<(NewFileAgentInventoryRequest, AgentKey)>;

/// Runs the modern two-step CAPS asset upload: POST the LLSD `metadata` to the
/// capability `cap_url` to obtain an `uploader` URL, then POST the raw `data`
/// bytes there. Surfaces the outcome as [`Event::AssetUploaded`] on success or
/// [`Event::AssetUploadFailed`] on any failure. Shared by the
/// `NewFileAgentInventory`, `UploadBakedTexture`, and `Update*AgentInventory`
/// uploads, whose responses share the `{ state, uploader, new_asset,
/// new_inventory_item }` shape.
///
/// `creating` carries the request of an upload that creates an item, so the
/// completion can be turned into the item itself — no grid announces one.
/// `asked_about` is the item an **update** was asked to rewrite, which names the
/// completion when the grid does not (see [`named_item`]).
pub(crate) async fn run_caps_upload(
    cap_url: String,
    metadata: String,
    data: Vec<u8>,
    creating: NewItemUpload,
    asked_about: Option<Uuid>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = caps_upload_event(&cap_url, metadata, data, creating, asked_about, &http).await;
    deliver(&events, event).await;
}

/// **The item a save rewrote, named by whoever knows it.** An update
/// capability's completion is only obliged to carry the new *asset*: the item
/// already exists and the client is the one that named it, so a grid may echo it
/// (OpenSim's `UpdateItemAsset.cs` does) or say nothing at all (Second Life does
/// not, and the reference's update path —
/// `LLBufferedAssetUploadInfo::finishUpload` — reads the asset from the response
/// and takes the item from `getItemId()`, the id it sent).
///
/// So the completion is filled in from the request when the grid leaves it out:
/// without it, everything that correlates a save with the item it saved —
/// reporting the save, and rebinding the item to the asset it now holds — simply
/// never fires on the stricter grid.
const fn named_item(reported: Option<Uuid>, asked_about: Option<Uuid>) -> Option<Uuid> {
    // A nil id parses as `None` (as a genuinely item-less baked-texture
    // completion does), so this covers "echoed a nil" as well as "said nothing".
    match reported {
        Some(item) => Some(item),
        None => asked_about,
    }
}

/// Performs both steps of a CAPS asset upload and returns the resulting event.
pub(crate) async fn caps_upload_event(
    cap_url: &str,
    metadata: String,
    data: Vec<u8>,
    creating: NewItemUpload,
    asked_about: Option<Uuid>,
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
                new_inventory_item: named_item(response.new_inventory_item, asked_about),
                created: creating.and_then(|(request, owner)| {
                    uploaded_inventory_item(&request, &response, owner, unix_seconds())
                        .map(Box::new)
                }),
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

/// The current Unix time in seconds, for the creation date of an item the grid
/// creates but never dates (`LLResourceUploadInfo::finishUpload` stamps
/// `time_corrected()` for the same reason). Saturates rather than wrapping
/// outside the range the wire field can hold.
fn unix_seconds() -> i32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i32::try_from(since.as_secs()).unwrap_or(i32::MAX)
        })
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
    asked_about: Option<Uuid>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = script_upload_event(&cap_url, metadata, source, running, asked_about, &http).await;
    deliver(&events, event).await;
}

/// Performs both steps of a script upload and maps the completion to an event.
async fn script_upload_event(
    cap_url: &str,
    metadata: String,
    source: Vec<u8>,
    running: Option<bool>,
    asked_about: Option<Uuid>,
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
                // A script upload always replaces the source of an item that
                // already exists — `Command::UploadScript` names it — so unlike
                // `AssetUploaded` there is never an item to assemble here.
                new_asset: response.new_asset,
                new_inventory_item: named_item(response.new_inventory_item, asked_about),
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
        // Fire-and-forget by design (no event is surfaced), but a failed
        // snapshot upload still gets a line so a report that silently lost its
        // screenshot is not invisible.
        if let Err(reason) =
            caps_upload_step(&http, &uploader, "application/octet-stream", screenshot).await
        {
            tracing::warn!("abuse-report screenshot upload failed: {reason}");
        }
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
