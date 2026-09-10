//! Two-step asset upload over NewFileAgentInventory / UploadBakedTexture.

use crate::{Caps, EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use sl_proto::Event as SessionEvent;
use sl_proto::{
    AgentKey, AssetType, AssetUploadResponse, CAP_NEW_FILE_AGENT_INVENTORY, InventoryType,
    NewFileAgentInventoryRequest, ScriptCompileError, build_new_file_agent_inventory_request,
    parse_asset_upload_response, uploaded_inventory_item,
};

/// What an upload was asked to create, kept for the completion: the
/// `NewFileAgentInventory` request body plus the agent the created item belongs
/// to. `None` for the uploads that create no item — `UploadBakedTexture` and the
/// `Update*Inventory` saves onto an item that already exists.
pub(crate) type NewItemUpload = Option<(NewFileAgentInventoryRequest, AgentKey)>;

/// Spawns the modern `NewFileAgentInventory` two-step CAPS upload on a background
/// thread, emitting [`SlSessionEvent::AssetUploaded`] /
/// [`SlSessionEvent::AssetUploadFailed`] over the asset channel. Emits a failure
/// immediately if the capability is unavailable (a class that cannot be
/// uploaded at all has no CAPS type names — see [`upload_type_names`]).
///
/// `owner` is the uploading agent, kept — with the request — for the completion:
/// this is the one upload that *creates* an inventory item, and no grid
/// announces the item it created, so the completion is what the item is built
/// from. A session with no agent id yet cannot name a creator, and the
/// completion then carries no item.
pub(crate) fn spawn_new_file_upload(
    caps: Option<&Caps>,
    owner: Option<AgentKey>,
    request: &NewFileAgentInventoryRequest,
    data: Vec<u8>,
) {
    let Some(caps) = caps else {
        return;
    };
    let Some(url) = caps.map.get(CAP_NEW_FILE_AGENT_INVENTORY).cloned() else {
        let asset_tx = caps.asset_tx.clone();
        deliver(
            &asset_tx,
            SessionEvent::AssetUploadFailed {
                reason: "NewFileAgentInventory capability not available".to_owned(),
            },
        );
        return;
    };
    let body = build_new_file_agent_inventory_request(request);
    let creating = owner.map(|owner| (request.clone(), owner));
    let asset_tx = caps.asset_tx.clone();
    std::thread::spawn(move || {
        let event = run_caps_upload(&url, body, data, creating);
        deliver(&asset_tx, event);
    });
}

/// The CAPS type names a `NewFileAgentInventory` request needs, or `None` when
/// either class has none and the pair therefore cannot be uploaded at all.
pub(crate) fn upload_type_names(
    asset_type: AssetType,
    inventory_type: InventoryType,
) -> Option<(&'static str, &'static str)> {
    asset_type.caps_asset_name().zip(inventory_type.caps_name())
}

/// Emits an [`SlSessionEvent::AssetUploadFailed`] over the asset channel naming a
/// capability that is not available on the current region.
pub(crate) fn emit_upload_unavailable(caps: Option<&Caps>, cap: &str) {
    emit_upload_failure(caps, format!("{cap} capability not available"));
}

/// Emits an [`SlSessionEvent::AssetUploadFailed`] over the asset channel with the
/// given reason (a no-op if no capabilities are established yet).
pub(crate) fn emit_upload_failure(caps: Option<&Caps>, reason: String) {
    if let Some(caps) = caps {
        deliver(&caps.asset_tx, SessionEvent::AssetUploadFailed { reason });
    }
}

/// Runs both steps of a modern CAPS asset upload synchronously (on the calling
/// background thread): POST the LLSD `metadata` to `cap_url` for an `uploader`
/// URL, then POST the raw `data` bytes there. Returns
/// [`SlSessionEvent::AssetUploaded`] on success or
/// [`SlSessionEvent::AssetUploadFailed`] on any failure.
///
/// `creating` carries the request of an upload that creates an item, so the
/// completion can be turned into the item itself — no grid announces one.
pub(crate) fn run_caps_upload(
    cap_url: &str,
    metadata: String,
    data: Vec<u8>,
    creating: NewItemUpload,
) -> SessionEvent {
    // Step 1: POST the metadata, expecting an `uploader` URL back.
    let uploader = match caps_upload_step(cap_url, "application/llsd+xml", metadata.into_bytes()) {
        Ok(response) => match response.uploader {
            Some(url) => url,
            None => {
                return SessionEvent::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("upload metadata rejected (state {})", response.state)
                    }),
                };
            }
        },
        Err(reason) => return SessionEvent::AssetUploadFailed { reason },
    };
    // Step 2: POST the raw asset bytes to the uploader URL.
    match caps_upload_step(&uploader, "application/octet-stream", data) {
        Ok(response) => match response.new_asset {
            Some(new_asset) => SessionEvent::AssetUploaded {
                new_asset,
                new_inventory_item: response.new_inventory_item,
                created: creating.and_then(|(request, owner)| {
                    uploaded_inventory_item(&request, &response, owner, unix_seconds())
                        .map(Box::new)
                }),
            },
            None => SessionEvent::AssetUploadFailed {
                reason: response.error.unwrap_or_else(|| {
                    format!("upload did not complete (state {})", response.state)
                }),
            },
        },
        Err(reason) => SessionEvent::AssetUploadFailed { reason },
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

/// Runs a two-step **script** upload (`UpdateScriptAgent`/`UpdateScriptTask`)
/// synchronously and returns the simulator's compile result as
/// [`SessionEvent::ScriptUploaded`]. A transport-level failure (missing uploader,
/// HTTP/parse error, or a bare error completion) returns
/// [`SessionEvent::AssetUploadFailed`]. `running` is the requested run state
/// echoed for a task-inventory upload (`None` for agent inventory).
pub(crate) fn run_script_upload(
    cap_url: &str,
    metadata: String,
    source: Vec<u8>,
    running: Option<bool>,
) -> SessionEvent {
    // Step 1: POST the metadata (ids + compile target), get an uploader.
    let uploader = match caps_upload_step(cap_url, "application/llsd+xml", metadata.into_bytes()) {
        Ok(response) => match response.uploader {
            Some(url) => url,
            None => {
                return SessionEvent::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("script upload metadata rejected (state {})", response.state)
                    }),
                };
            }
        },
        Err(reason) => return SessionEvent::AssetUploadFailed { reason },
    };
    // Step 2: POST the raw source; the completion carries the compile result.
    match caps_upload_step(&uploader, "application/octet-stream", source) {
        Ok(response) => {
            if response.compiled.is_none() && response.new_asset.is_none() {
                return SessionEvent::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("script upload did not complete (state {})", response.state)
                    }),
                };
            }
            SessionEvent::ScriptUploaded {
                new_asset: response.new_asset,
                new_inventory_item: response.new_inventory_item,
                compiled: response.compiled.unwrap_or(true),
                errors: response
                    .errors
                    .iter()
                    .map(|error| ScriptCompileError::parse(error))
                    .collect(),
                running,
            }
        }
        Err(reason) => SessionEvent::AssetUploadFailed { reason },
    }
}

/// Files an abuse report bearing a snapshot over the
/// `SendUserReportWithScreenshot` capability (blocking, on the calling
/// background thread): a two-step upload that POSTs the report's LLSD body
/// (`report_body`) to `cap_url` for an `uploader` URL, then POSTs the snapshot's
/// JPEG-2000 bytes (`screenshot`) there. Fire-and-forget like the no-screenshot
/// `SendUserReport` path — the report's outcome is not surfaced as an event
/// (mirroring the viewer, which discards the result in
/// `LLARScreenShotUploader::finishUpload`).
pub(crate) fn run_report_screenshot_upload(
    cap_url: &str,
    report_body: String,
    screenshot: Vec<u8>,
) {
    let Ok(response) = caps_upload_step(cap_url, "application/llsd+xml", report_body.into_bytes())
    else {
        return;
    };
    if let Some(uploader) = response.uploader {
        // Fire-and-forget by design (no event is surfaced), but a failed
        // snapshot upload still gets a line so a report that silently lost its
        // screenshot is not invisible.
        if let Err(reason) = caps_upload_step(&uploader, "application/octet-stream", screenshot) {
            tracing::warn!("abuse-report screenshot upload failed: {reason}");
        }
    }
}

/// POSTs one step of a CAPS upload (blocking) and parses the LLSD response,
/// returning the parsed [`AssetUploadResponse`] or a failure reason.
pub(crate) fn caps_upload_step(
    url: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<AssetUploadResponse, String> {
    let http = crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .map_err(|error| format!("HTTP client build failed: {error}"))?;
    let response = http
        .post(url)
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .map_err(|error| format!("upload request failed: {error}"))?;
    let text = response
        .text()
        .map_err(|error| format!("upload response read failed: {error}"))?;
    parse_asset_upload_response(&text)
        .map_err(|error| format!("upload response parse failed: {error}"))
}
