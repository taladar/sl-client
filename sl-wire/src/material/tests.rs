//! Round-trip tests for the material codecs.

use crate::llsd::Llsd;

use base64::Engine as _;
use pretty_assertions::assert_eq;

use super::{
    FaceMaterialPut, GltfMaterialOverride, LegacyMaterial, MaterialOverrideUpdate,
    RenderMaterialEntry, build_gltf_material_override, build_modify_material_params_request,
    build_modify_material_params_response, build_render_materials_put_request,
    build_render_materials_request, build_render_materials_response, parse_gltf_material_override,
    parse_modify_material_params_request, parse_render_materials_put_request,
    parse_render_materials_request, parse_render_materials_response, read_binary_value,
    write_binary_value,
};
use crate::field::Reader;
use sl_types::key::TextureKey;
use uuid::Uuid;

/// A binary-LLSD value round-trips through the writer and reader.
#[test]
fn binary_llsd_round_trip() -> Result<(), String> {
    let id = Uuid::from_u128(0x1234_5678_9abc_def0_1122_3344_5566_7788);
    let mut map = std::collections::HashMap::new();
    map.insert("ID".to_owned(), Llsd::Binary(id.as_bytes().to_vec()));
    map.insert("n".to_owned(), Llsd::Integer(-7));
    map.insert("r".to_owned(), Llsd::Real(1.5));
    let value = Llsd::Array(vec![Llsd::Map(map)]);

    let mut bytes = Vec::new();
    write_binary_value(&value, &mut bytes);
    let mut reader = Reader::new(&bytes);
    let decoded = read_binary_value(&mut reader).ok_or("decode failed")?;
    assert_eq!(decoded, value);
    Ok(())
}

/// A `RenderMaterials` request round-trips through the response parser
/// (build → would-be server echo of the same zipped form → parse).
#[test]
fn render_materials_zip_round_trip() -> Result<(), String> {
    // Build the request body for one material id and confirm it is the
    // zipped `{Zipped}` envelope; then feed an equivalent response (an
    // array of {ID, Material}) through the parser.
    let id = Uuid::from_u128(0x00ab_cdef_0011_2233_4455_6677_8899_aabb);
    let request = build_render_materials_request(&[id]);
    assert!(request.contains("<key>Zipped</key>"));

    // Construct a response: zip a binary-LLSD array of one entry.
    let mut material = std::collections::HashMap::new();
    material.insert("NormOffsetX".to_owned(), Llsd::Integer(5000));
    material.insert("SpecExp".to_owned(), Llsd::Integer(51));
    material.insert(
        "SpecColor".to_owned(),
        Llsd::Array(vec![
            Llsd::Integer(10),
            Llsd::Integer(20),
            Llsd::Integer(30),
            Llsd::Integer(255),
        ]),
    );
    let mut entry = std::collections::HashMap::new();
    entry.insert("ID".to_owned(), Llsd::Binary(id.as_bytes().to_vec()));
    entry.insert("Material".to_owned(), Llsd::Map(material));
    let array = Llsd::Array(vec![Llsd::Map(entry)]);
    let mut binary = Vec::new();
    write_binary_value(&array, &mut binary);
    let zipped = miniz_oxide::deflate::compress_to_vec_zlib(&binary, 6);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&zipped);
    let response = format!("<llsd><map><key>Zipped</key><binary>{encoded}</binary></map></llsd>");

    let entries = parse_render_materials_response(&response);
    assert_eq!(entries.len(), 1);
    let decoded = entries.first().ok_or("no entry")?;
    assert_eq!(decoded.material_id, id);
    assert!((decoded.material.normal_offset.0 - 0.5).abs() < f32::EPSILON);
    assert_eq!(decoded.material.specular_exponent, 51);
    assert_eq!(decoded.material.specular_color, [10, 20, 30, 255]);
    Ok(())
}

/// The GLTF override envelope decodes its id and faces, leaving the per-face
/// override documents as raw notation bytes.
#[test]
fn gltf_override_envelope() -> Result<(), String> {
    let payload = b"{'id':i42,'te':[i0,i3],'od':[{'bc':[r1,r1,r1,r1]},{'mf':r0.5}]}";
    let decoded = parse_gltf_material_override(payload).ok_or("decode failed")?;
    assert_eq!(
        decoded,
        GltfMaterialOverride {
            local_id: crate::RegionLocalObjectId(42),
            faces: vec![0, 3],
            overrides: vec![b"{'bc':[r1,r1,r1,r1]}".to_vec(), b"{'mf':r0.5}".to_vec(),],
        }
    );
    Ok(())
}

/// `ModifyMaterialParams` serializes object id, side and the opaque JSON.
#[test]
fn modify_material_params_body() {
    let object_id =
        sl_types::key::ObjectKey::from(Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10));
    let object_raw = object_id.uuid();
    let asset_id = Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888);
    let body = build_modify_material_params_request(&[MaterialOverrideUpdate {
        object_id,
        side: -1,
        gltf_json: Some("{\"a\":1}".to_owned()),
        asset_id: Some(asset_id),
    }]);
    assert!(body.contains(&format!("<uuid>{object_raw}</uuid>")));
    assert!(body.contains("<key>side</key><integer>-1</integer>"));
    assert!(body.contains("<key>gltf_json</key><string>{&quot;a&quot;:1}</string>"));
    assert!(body.contains(&format!("<key>asset_id</key><uuid>{asset_id}</uuid>")));
}

/// The GLTF override builder produces a payload the parser reads back equal
/// (the server-side inverse of `parse_gltf_material_override`).
#[test]
fn gltf_override_round_trip() -> Result<(), String> {
    let original = GltfMaterialOverride {
        local_id: crate::RegionLocalObjectId(42),
        faces: vec![0, 3],
        overrides: vec![b"{'bc':[r1,r1,r1,r1]}".to_vec(), b"{'mf':r0.5}".to_vec()],
    };
    let payload = build_gltf_material_override(&original);
    let decoded = parse_gltf_material_override(&payload).ok_or("decode failed")?;
    assert_eq!(decoded, original);
    Ok(())
}

/// A `ModifyMaterialParams` body built by the client parses back to the same
/// updates on the server side.
#[test]
fn modify_material_params_round_trip() -> Result<(), String> {
    let updates = vec![
        MaterialOverrideUpdate {
            object_id: sl_types::key::ObjectKey::from(Uuid::from_u128(
                0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10,
            )),
            side: -1,
            gltf_json: Some("{\"a\":1}".to_owned()),
            asset_id: Some(Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888)),
        },
        MaterialOverrideUpdate {
            object_id: sl_types::key::ObjectKey::from(Uuid::from_u128(
                0x00ab_cdef_0011_2233_4455_6677_8899_aabb,
            )),
            side: 2,
            gltf_json: None,
            asset_id: None,
        },
    ];
    let body = build_modify_material_params_request(&updates);
    let parsed =
        parse_modify_material_params_request(&body).map_err(|error| format!("{error:?}"))?;
    assert_eq!(parsed, updates);
    Ok(())
}

/// A `RenderMaterials` response built from entries parses back to the same
/// entries (the server-side inverse of `parse_render_materials_response`).
#[test]
fn render_materials_response_round_trip() -> Result<(), String> {
    let entry = RenderMaterialEntry {
        material_id: Uuid::from_u128(0x00ab_cdef_0011_2233_4455_6677_8899_aabb),
        material: LegacyMaterial {
            normal_map: TextureKey::from(Uuid::from_u128(0x1234)),
            normal_offset: (0.5, -0.25),
            normal_repeat: (2.0, 4.0),
            normal_rotation: 1.5,
            specular_map: TextureKey::from(Uuid::from_u128(0x5678)),
            specular_offset: (0.1, 0.2),
            specular_repeat: (1.0, 1.0),
            specular_rotation: 0.0,
            specular_color: [10, 20, 30, 255],
            specular_exponent: 51,
            environment_intensity: 7,
            diffuse_alpha_mode: 1,
            alpha_mask_cutoff: 128,
        },
    };
    let response = build_render_materials_response(std::slice::from_ref(&entry));
    let entries = parse_render_materials_response(&response);
    assert_eq!(entries.len(), 1);
    let decoded = entries.first().ok_or("no entry")?;
    assert_eq!(decoded, &entry);
    Ok(())
}

/// A `RenderMaterials` POST **request** built by the client parses back to the
/// queried ids on the server side (the inverse of the id-list builder).
#[test]
fn render_materials_request_round_trip() {
    let ids = vec![
        Uuid::from_u128(0x00ab_cdef_0011_2233_4455_6677_8899_aabb),
        Uuid::from_u128(0x1122_3344_5566_7788_99aa_bbcc_ddee_ff00),
    ];
    let body = build_render_materials_request(&ids);
    assert_eq!(parse_render_materials_request(&body), ids);
    // An empty "fetch all" body (no `Zipped`) parses to no ids.
    assert!(parse_render_materials_request("<llsd><map /></llsd>").is_empty());
}

/// A `RenderMaterials` PUT built by the client parses back to the same face
/// assignments on the server side (cleared faces included).
#[test]
fn render_materials_put_request_round_trip() {
    let updates = vec![
        FaceMaterialPut {
            local_id: 0x0089_aabb,
            face: 3,
            material: Some(LegacyMaterial {
                normal_map: TextureKey::from(Uuid::from_u128(0x1234)),
                normal_offset: (0.5, -0.25),
                normal_repeat: (2.0, 4.0),
                normal_rotation: 1.5,
                specular_map: TextureKey::from(Uuid::from_u128(0x5678)),
                specular_offset: (0.1, 0.2),
                specular_repeat: (1.0, 1.0),
                specular_rotation: 0.0,
                specular_color: [10, 20, 30, 255],
                specular_exponent: 51,
                environment_intensity: 7,
                diffuse_alpha_mode: 2,
                alpha_mask_cutoff: 128,
            }),
        },
        FaceMaterialPut {
            local_id: 0x0089_aabb,
            face: 4,
            material: None,
        },
    ];
    let body = build_render_materials_put_request(&updates);
    assert_eq!(parse_render_materials_put_request(&body), updates);
}

/// The `ModifyMaterialParams` `{ success, message }` response builder emits a
/// body the (inline, client-side) reader decodes — spot-checked structurally.
#[test]
fn modify_material_params_response_body() {
    let ok = build_modify_material_params_response(true, "");
    assert!(ok.contains("<key>success</key><boolean>1</boolean>"));
    assert!(ok.contains("<key>message</key><string></string>"));
    let err = build_modify_material_params_response(false, "no perms");
    assert!(err.contains("<key>success</key><boolean>0</boolean>"));
    assert!(err.contains("<key>message</key><string>no perms</string>"));
}

/// A `RenderMaterials` PUT body carries `FullMaterialsPerFace` with the face,
/// local id and the material's LLSD map (and omits `Material` for a cleared
/// face) — decoded straight from the zipped binary the builder emits.
#[test]
fn render_materials_put_round_trip() -> Result<(), String> {
    let material = LegacyMaterial {
        normal_map: TextureKey::from(Uuid::from_u128(0x1234)),
        normal_offset: (0.5, -0.25),
        normal_repeat: (2.0, 4.0),
        normal_rotation: 1.5,
        specular_map: TextureKey::from(Uuid::from_u128(0x5678)),
        specular_offset: (0.1, 0.2),
        specular_repeat: (1.0, 1.0),
        specular_rotation: 0.0,
        specular_color: [10, 20, 30, 255],
        specular_exponent: 51,
        environment_intensity: 7,
        diffuse_alpha_mode: 2,
        alpha_mask_cutoff: 128,
    };
    let updates = vec![
        FaceMaterialPut {
            local_id: 0x0089_aabb,
            face: 3,
            material: Some(material),
        },
        FaceMaterialPut {
            local_id: 0x0089_aabb,
            face: 4,
            material: None,
        },
    ];
    let body = build_render_materials_put_request(&updates);
    assert!(body.contains("<key>Zipped</key>"));

    // Decode the zipped binary and confirm the FullMaterialsPerFace structure.
    let root = crate::llsd::parse_llsd_xml(&body).map_err(|error| format!("{error:?}"))?;
    let Llsd::Map(map) = root else {
        return Err("root not a map".to_owned());
    };
    let Some(Llsd::Binary(zipped)) = map.get("Zipped") else {
        return Err("no Zipped binary".to_owned());
    };
    let unzipped = miniz_oxide::inflate::decompress_to_vec_zlib(zipped)
        .map_err(|error| format!("{error:?}"))?;
    let mut reader = Reader::new(&unzipped);
    let value = read_binary_value(&mut reader).ok_or("decode failed")?;
    let Llsd::Map(top) = value else {
        return Err("payload not a map".to_owned());
    };
    let Some(Llsd::Array(faces)) = top.get("FullMaterialsPerFace") else {
        return Err("no FullMaterialsPerFace".to_owned());
    };
    assert_eq!(faces.len(), 2);
    let Some(Llsd::Map(first)) = faces.first() else {
        return Err("first face not a map".to_owned());
    };
    assert_eq!(first.get("Face"), Some(&Llsd::Integer(3)));
    assert_eq!(first.get("ID"), Some(&Llsd::Integer(0x0089_aabb)));
    let Some(Llsd::Map(material_map)) = first.get("Material") else {
        return Err("first face has no Material".to_owned());
    };
    assert_eq!(material_map.get("SpecExp"), Some(&Llsd::Integer(51)));
    assert_eq!(
        material_map.get("DiffuseAlphaMode"),
        Some(&Llsd::Integer(2))
    );
    // The cleared face omits Material entirely.
    let Some(Llsd::Map(second)) = faces.get(1) else {
        return Err("second face not a map".to_owned());
    };
    assert_eq!(second.get("Face"), Some(&Llsd::Integer(4)));
    assert!(!second.contains_key("Material"));
    Ok(())
}
