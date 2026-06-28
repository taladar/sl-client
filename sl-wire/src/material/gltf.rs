//! GLTF (PBR) material overrides and the `ModifyMaterialParams` set request.

use super::{GltfMaterialOverride, MaterialOverrideUpdate};
use crate::WireError;
use crate::llsd::{Llsd, Scan, parse_llsd_xml, push_escaped};

// ---------------------------------------------------------------------------
// GLTF material override (GenericStreamingMessage method 0x4175)
// ---------------------------------------------------------------------------

/// Decodes a GLTF material-override `GenericStreamingMessage` payload (notation
/// LLSD `{ "id": <local id>, "te": [faces…], "od": [overrides…] }`) into a
/// [`GltfMaterialOverride`].
///
/// The per-face override documents (`od`) are returned as their raw notation
/// bytes rather than parsed — only the envelope (object id and affected faces)
/// is interpreted. Returns `None` if the payload is not the expected map.
#[must_use]
pub fn parse_gltf_material_override(data: &[u8]) -> Option<GltfMaterialOverride> {
    let mut scan = Scan::new(data);
    scan.expect(b'{')?;
    let mut local_id: Option<i64> = None;
    let mut faces_raw: Vec<i64> = Vec::new();
    let mut overrides: Vec<Vec<u8>> = Vec::new();
    loop {
        scan.skip_ws_sep();
        match scan.peek()? {
            b'}' => {
                scan.bump();
                break;
            }
            b'\'' | b'"' => {}
            _ => return None,
        }
        let key = scan.read_quoted_string()?;
        scan.skip_ws_sep();
        scan.expect(b':')?;
        match key.as_str() {
            "id" => local_id = Some(scan.read_integer()?),
            "te" => faces_raw = scan.read_integer_array()?,
            "od" => overrides = scan.read_raw_array()?,
            _ => {
                scan.skip_value()?;
            }
        }
    }
    let local_id = u32::try_from(local_id?).ok()?;
    let faces = faces_raw
        .into_iter()
        .filter_map(|value| u8::try_from(value).ok())
        .collect();
    Some(GltfMaterialOverride {
        local_id: crate::RegionLocalObjectId(local_id),
        faces,
        overrides,
    })
}

// ---------------------------------------------------------------------------
// ModifyMaterialParams (set GLTF material on object faces)
// ---------------------------------------------------------------------------

/// Builds the LLSD-XML body for a `ModifyMaterialParams` capability POST: an
/// array of per-face material assignments. Each entry carries `object_id` and
/// `side`, plus the supplied `gltf_json` (verbatim) and/or `asset_id`.
#[must_use]
pub fn build_modify_material_params_request(updates: &[MaterialOverrideUpdate]) -> String {
    let mut out = String::from("<llsd><array>");
    for update in updates {
        out.push_str("<map><key>object_id</key><uuid>");
        out.push_str(&update.object_id.uuid().to_string());
        out.push_str("</uuid><key>side</key><integer>");
        out.push_str(&update.side.to_string());
        out.push_str("</integer>");
        if let Some(json) = &update.gltf_json {
            out.push_str("<key>gltf_json</key><string>");
            push_escaped(&mut out, json);
            out.push_str("</string>");
        }
        if let Some(asset_id) = update.asset_id {
            out.push_str("<key>asset_id</key><uuid>");
            out.push_str(&asset_id.to_string());
            out.push_str("</uuid>");
        }
        out.push_str("</map>");
    }
    out.push_str("</array></llsd>");
    out
}

/// Builds a GLTF material-override `GenericStreamingMessage` payload — the
/// inverse of `parse_gltf_material_override`.
///
/// Emits the notation-LLSD map
/// `{ 'id': <local id>, 'te': [faces…], 'od': [overrides…] }` the simulator
/// pushes to viewers (the message's method id is
/// [`GLTF_MATERIAL_OVERRIDE_METHOD`](crate::GLTF_MATERIAL_OVERRIDE_METHOD)). The
/// per-face override documents are written verbatim from
/// [`GltfMaterialOverride::overrides`] (they are raw notation-LLSD bytes the
/// caller supplies — this layer does not build GLTF material documents),
/// positionally correlated with the face list.
#[must_use]
pub fn build_gltf_material_override(material_override: &GltfMaterialOverride) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'{');
    out.extend_from_slice(b"'id':i");
    out.extend_from_slice(material_override.local_id.to_string().as_bytes());
    out.extend_from_slice(b",'te':[");
    for (index, face) in material_override.faces.iter().enumerate() {
        if index != 0 {
            out.push(b',');
        }
        out.push(b'i');
        out.extend_from_slice(face.to_string().as_bytes());
    }
    out.extend_from_slice(b"],'od':[");
    for (index, document) in material_override.overrides.iter().enumerate() {
        if index != 0 {
            out.push(b',');
        }
        out.extend_from_slice(document);
    }
    out.extend_from_slice(b"]}");
    out
}

/// Parses a `ModifyMaterialParams` capability request body — the inverse of
/// [`build_modify_material_params_request`].
///
/// Reads the `<llsd><array>` of per-face assignments back into
/// [`MaterialOverrideUpdate`]s. Each entry must carry an `object_id`; `side`
/// defaults to `-1` (all faces) if absent, and the optional `gltf_json` /
/// `asset_id` fields are surfaced only when present. Malformed entries (no
/// `object_id`) are skipped.
///
/// # Errors
///
/// Returns the [`roxmltree`] error if `xml` is not well-formed.
pub fn parse_modify_material_params_request(
    xml: &str,
) -> Result<Vec<MaterialOverrideUpdate>, roxmltree::Error> {
    let root = parse_llsd_xml(xml)?;
    let Some(array) = root.as_array() else {
        return Ok(Vec::new());
    };
    Ok(array
        .iter()
        .filter_map(|item| modify_material_update(item).ok().flatten())
        .collect())
}

/// Decodes one `ModifyMaterialParams` array entry into a
/// [`MaterialOverrideUpdate`], or `None` if it lacks an `object_id`.
fn modify_material_update(item: &Llsd) -> Result<Option<MaterialOverrideUpdate>, WireError> {
    let Some(raw_object_id) = item.field_uuid("object_id", "object_id")? else {
        return Ok(None);
    };
    let object_id = sl_types::key::ObjectKey::from(raw_object_id);
    let side = item.field_i32("side", "side")?.unwrap_or(-1);
    let gltf_json = item.field_str("gltf_json", "gltf_json")?.map(str::to_owned);
    let asset_id = item.field_uuid("asset_id", "asset_id")?;
    Ok(Some(MaterialOverrideUpdate {
        object_id,
        side,
        gltf_json,
        asset_id,
    }))
}
