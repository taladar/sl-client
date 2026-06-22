//! Two-step asset upload over NewFileAgentInventory / UploadBakedTexture.

use crate::{Caps, EVENT_QUEUE_TIMEOUT};
use bevy::prelude::*;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::Event as SessionEvent;
use sl_proto::{
    AssetType, AssetUploadResponse, CAP_NEW_FILE_AGENT_INVENTORY, InventoryFolderKey,
    InventoryType, build_new_file_agent_inventory_request, parse_asset_upload_response,
};

/// Spawns the modern `NewFileAgentInventory` two-step CAPS upload on a background
/// thread, emitting [`SlSessionEvent::AssetUploaded`] /
/// [`SlSessionEvent::AssetUploadFailed`] over the asset channel. Emits a failure
/// immediately if the asset/inventory type is not uploadable or the capability
/// is unavailable.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the flat NewFileAgentInventory upload command fields"
)]
pub(crate) fn spawn_new_file_upload(
    caps: Option<&Caps>,
    folder_id: InventoryFolderKey,
    asset_type: AssetType,
    inventory_type: InventoryType,
    name: &str,
    description: &str,
    next_owner_mask: u32,
    group_mask: u32,
    everyone_mask: u32,
    expected_upload_cost: i32,
    data: Vec<u8>,
) {
    let (Some(asset_name), Some(inv_name)) =
        (asset_type.caps_asset_name(), inventory_type.caps_name())
    else {
        emit_upload_failure(caps, "asset/inventory type is not uploadable".to_owned());
        return;
    };
    let Some(caps) = caps else {
        return;
    };
    let Some(url) = caps.map.get(CAP_NEW_FILE_AGENT_INVENTORY).cloned() else {
        let asset_tx = caps.asset_tx.clone();
        asset_tx
            .send(SessionEvent::AssetUploadFailed {
                reason: "NewFileAgentInventory capability not available".to_owned(),
            })
            .ok();
        return;
    };
    let body = build_new_file_agent_inventory_request(
        folder_id,
        asset_name,
        inv_name,
        name,
        description,
        next_owner_mask,
        group_mask,
        everyone_mask,
        expected_upload_cost,
    );
    let asset_tx = caps.asset_tx.clone();
    std::thread::spawn(move || {
        let event = run_caps_upload(&url, body, data);
        asset_tx.send(event).ok();
    });
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
        caps.asset_tx
            .send(SessionEvent::AssetUploadFailed { reason })
            .ok();
    }
}

/// Runs both steps of a modern CAPS asset upload synchronously (on the calling
/// background thread): POST the LLSD `metadata` to `cap_url` for an `uploader`
/// URL, then POST the raw `data` bytes there. Returns
/// [`SlSessionEvent::AssetUploaded`] on success or
/// [`SlSessionEvent::AssetUploadFailed`] on any failure.
pub(crate) fn run_caps_upload(cap_url: &str, metadata: String, data: Vec<u8>) -> SessionEvent {
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
        caps_upload_step(&uploader, "application/octet-stream", screenshot).ok();
    }
}

/// POSTs one step of a CAPS upload (blocking) and parses the LLSD response,
/// returning the parsed [`AssetUploadResponse`] or a failure reason.
pub(crate) fn caps_upload_step(
    url: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<AssetUploadResponse, String> {
    let http = ReqwestBlockingClient::builder()
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
