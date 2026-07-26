//! Render-materials capability fetch and ModifyMaterialParams post.

use crate::EVENT_QUEUE_TIMEOUT;
use bevy::prelude::*;
use crossbeam_channel::Sender;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::Event as SessionEvent;
use sl_proto::{
    CAP_MODIFY_MATERIAL_PARAMS, Llsd, Uuid, build_render_materials_request, parse_llsd_xml,
    parse_render_materials_response,
};

/// POSTs a `RenderMaterials` request for `material_ids` (the zipped binary-LLSD
/// form) and forwards the decoded legacy materials to `asset_tx` as a
/// [`SlSessionEvent::RenderMaterials`]. Best-effort: a transport or decode
/// failure yields an empty list.
pub(crate) fn run_render_materials_fetch(
    cap_url: &str,
    material_ids: Vec<Uuid>,
    asset_tx: &Sender<SessionEvent>,
) {
    let materials = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()
        .and_then(|http| {
            let body = build_render_materials_request(&material_ids);
            http.post(cap_url)
                .header("Content-Type", "application/llsd+xml")
                .body(body)
                .send()
                .ok()
        })
        .and_then(|response| response.text().ok())
        .map(|text| parse_render_materials_response(&text))
        .unwrap_or_default();
    asset_tx.send(SessionEvent::RenderMaterials(materials)).ok();
}

/// PUTs a `RenderMaterials` request that sets (or clears) legacy materials on
/// object faces (the zipped `FullMaterialsPerFace` form). Fire-and-forget for the
/// **payload**: the simulator assigns the material id and echoes it on the affected
/// faces' `TextureEntry` (an `ObjectImage` update), so there is no reply to forward
/// (the reference viewer's `onPutResponse` is likewise a no-op). The response
/// **status** is still logged, though — a non-2xx (a reverse proxy 500 / 502 / 503,
/// or a cap that rejected the body) is otherwise silent and is exactly what makes an
/// edit look like it "did nothing", so it is surfaced at `warn`.
pub(crate) fn run_set_render_materials(cap_url: &str, body: String) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    match http
        .put(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                tracing::debug!("RenderMaterials PUT succeeded: {status}");
            } else {
                // A short snippet of the error body (the proxy's HTML page, or the
                // cap's message) — enough to tell a "503 Service Unavailable" apart
                // from a body the cap rejected, without dumping a whole page.
                let snippet: String = response
                    .text()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(200)
                    .collect();
                tracing::warn!("RenderMaterials PUT failed: {status}: {snippet}");
            }
        }
        Err(error) => tracing::warn!("RenderMaterials PUT transport error: {error}"),
    }
}

/// POSTs a `ModifyMaterialParams` request and forwards the `{ success, message }`
/// reply to `caps_tx` tagged [`CAP_MODIFY_MATERIAL_PARAMS`], for the session to
/// surface as a [`SlSessionEvent::MaterialParamsResult`].
pub(crate) fn run_modify_material_params(
    cap_url: &str,
    body: String,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
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
        caps_tx
            .send((CAP_MODIFY_MATERIAL_PARAMS.to_owned(), llsd))
            .ok();
    }
}
