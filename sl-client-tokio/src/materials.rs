//! Render-materials capability fetch and ModifyMaterialParams post.

use reqwest::Client as ReqwestClient;
use sl_proto::{
    CAP_MODIFY_MATERIAL_PARAMS, Event, Llsd, Uuid, build_render_materials_request, parse_llsd_xml,
    parse_render_materials_response,
};
use tokio::sync::mpsc;

/// POSTs a `RenderMaterials` request for `material_ids` (the zipped binary-LLSD
/// form), decoding the zipped reply into the legacy materials and surfacing them
/// as an [`Event::RenderMaterials`]. Best-effort: a transport or decode failure
/// yields an empty list.
pub(crate) async fn fetch_render_materials(
    cap_url: String,
    material_ids: Vec<Uuid>,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let body = build_render_materials_request(&material_ids);
    let materials = match http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    {
        Ok(response) => match response.text().await {
            Ok(text) => parse_render_materials_response(&text),
            Err(_error) => Vec::new(),
        },
        Err(_error) => Vec::new(),
    };
    events.send(Event::RenderMaterials(materials)).await.ok();
}

/// POSTs a `ModifyMaterialParams` request, forwarding the `{ success, message }`
/// reply back over `caps_tx` to be surfaced as an [`Event::MaterialParamsResult`].
pub(crate) async fn post_modify_material_params(
    cap_url: String,
    body: String,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
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
        caps_tx
            .send((CAP_MODIFY_MATERIAL_PARAMS.to_owned(), llsd))
            .await
            .ok();
    }
}
