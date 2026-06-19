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
