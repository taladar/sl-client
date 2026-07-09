//! A pure (sans-I/O) decoder for Second Life / OpenSim **GLTF 2.0 (PBR) render
//! materials** — the `AT_MATERIAL` asset a face references through its
//! `LLRenderMaterialParams` entry ([`sl_proto::RenderMaterialRef`]).
//!
//! It is the material counterpart of `sl-mesh` / `sl-texture`: those decode a
//! mesh / texture asset; this decodes a material asset into a renderer-agnostic
//! [`GltfMaterial`] the viewer maps onto its PBR material (a Bevy
//! `StandardMaterial`), fetching each referenced texture through the ordinary
//! texture pipeline. See the crate `README.md` for the asset format.
//!
//! The reference implementation is Firestorm's `LLGLTFMaterial`
//! (`indra/llprimitive/llgltfmaterial.cpp`).
//!
//! [`sl_proto::RenderMaterialRef`]: https://github.com/taladar/sl-client
//!
//! # Example
//!
//! ```
//! # use sl_material::{parse_gltf_material_document, GltfAlphaMode};
//! let json = r#"{
//!   "materials": [{ "alphaMode": "BLEND", "doubleSided": true }]
//! }"#;
//! let material = parse_gltf_material_document(json).unwrap();
//! assert_eq!(material.alpha_mode, GltfAlphaMode::Blend);
//! assert!(material.double_sided);
//! ```

pub mod decode;
pub mod error;
pub mod overrides;
pub mod types;

pub use decode::{parse_gltf_material_document, parse_material_asset};
pub use error::MaterialError;
pub use overrides::{
    MaterialOverride, TextureOverride, TextureTransformOverride, parse_material_override,
};
pub use types::{GltfAlphaMode, GltfMaterial, GltfTexture, GltfTextureTransform};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;
    use sl_llsd::Llsd;
    use sl_types::key::TextureKey;
    use uuid::Uuid;

    use super::{GltfAlphaMode, MaterialError, parse_gltf_material_document, parse_material_asset};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A base-colour texture UUID used across the tests.
    const BASE_TEX: Uuid = Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888);
    /// A normal-map texture UUID used across the tests.
    const NORMAL_TEX: Uuid = Uuid::from_u128(0x9999_aaaa_bbbb_cccc_dddd_eeee_ffff_0000);

    /// Assert two floats are equal within a small tolerance (the glTF factors do
    /// not all round-trip exactly, so an exact `==` would be brittle and trips
    /// `clippy::float_cmp`).
    fn approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual} != {expected} (within 1e-5)"
        );
    }

    /// Assert two float slices are element-wise equal within tolerance.
    fn approx_slice(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "length mismatch");
        for (got, want) in actual.iter().zip(expected) {
            approx(*got, *want);
        }
    }

    /// A glTF material document exercising factors, two textures, a texture
    /// transform on the base colour, an alpha mode, and the double-sided flag.
    fn sample_gltf() -> String {
        format!(
            r#"{{
              "images": [{{ "uri": "{BASE_TEX}" }}, {{ "uri": "{NORMAL_TEX}" }}],
              "textures": [{{ "source": 0 }}, {{ "source": 1 }}],
              "materials": [{{
                "pbrMetallicRoughness": {{
                  "baseColorFactor": [0.5, 0.25, 0.125, 0.8],
                  "baseColorTexture": {{
                    "index": 0,
                    "extensions": {{
                      "KHR_texture_transform": {{
                        "offset": [0.1, 0.2],
                        "scale": [2.0, 3.0],
                        "rotation": 1.5
                      }}
                    }}
                  }},
                  "metallicFactor": 0.4,
                  "roughnessFactor": 0.6
                }},
                "normalTexture": {{ "index": 1 }},
                "emissiveFactor": [0.9, 0.8, 0.7],
                "alphaMode": "MASK",
                "alphaCutoff": 0.3,
                "doubleSided": true
              }}]
            }}"#
        )
    }

    /// The glTF document decodes into every field, resolving both texture slots
    /// through the `textures` → `images` indirection and reading the base
    /// colour's `KHR_texture_transform`.
    #[test]
    fn decodes_full_material() -> Result<(), TestError> {
        let material = parse_gltf_material_document(&sample_gltf())?;
        approx_slice(&material.base_color, &[0.5, 0.25, 0.125, 0.8]);
        approx(material.metallic_factor, 0.4);
        approx(material.roughness_factor, 0.6);
        approx_slice(&material.emissive_factor, &[0.9, 0.8, 0.7]);
        assert_eq!(material.alpha_mode, GltfAlphaMode::Mask);
        approx(material.alpha_cutoff, 0.3);
        assert!(material.double_sided);

        let base = material.base_color_texture.ok_or("base texture missing")?;
        assert_eq!(base.id, TextureKey::from(BASE_TEX));
        approx_slice(&base.transform.offset, &[0.1, 0.2]);
        approx_slice(&base.transform.scale, &[2.0, 3.0]);
        approx(base.transform.rotation, 1.5);

        let normal = material.normal_texture.ok_or("normal texture missing")?;
        assert_eq!(normal.id, TextureKey::from(NORMAL_TEX));
        // A texture without the extension keeps the identity transform.
        approx_slice(&normal.transform.offset, &[0.0, 0.0]);
        approx_slice(&normal.transform.scale, &[1.0, 1.0]);
        approx(normal.transform.rotation, 0.0);

        // Slots the document omits stay empty.
        assert!(material.metallic_roughness_texture.is_none());
        assert!(material.emissive_texture.is_none());
        Ok(())
    }

    /// An empty material takes every glTF default.
    #[test]
    fn empty_material_uses_defaults() -> Result<(), TestError> {
        let material = parse_gltf_material_document(r#"{ "materials": [{}] }"#)?;
        approx_slice(&material.base_color, &[1.0, 1.0, 1.0, 1.0]);
        approx(material.metallic_factor, 1.0);
        approx(material.roughness_factor, 1.0);
        approx_slice(&material.emissive_factor, &[0.0, 0.0, 0.0]);
        assert_eq!(material.alpha_mode, GltfAlphaMode::Opaque);
        approx(material.alpha_cutoff, 0.5);
        assert!(!material.double_sided);
        assert!(material.base_color_texture.is_none());
        Ok(())
    }

    /// A document with no material is a decode error, not a silent default.
    #[test]
    fn missing_material_errors() {
        assert!(matches!(
            parse_gltf_material_document(r#"{ "materials": [] }"#),
            Err(MaterialError::NoMaterial)
        ));
    }

    /// Build the LLSD envelope map (`{ version, type, data }`) wrapping a glTF
    /// document.
    fn envelope(version: &str, data: &str) -> Llsd {
        let mut map = HashMap::new();
        let _prev = map.insert("version".to_owned(), Llsd::String(version.to_owned()));
        let _prev = map.insert("type".to_owned(), Llsd::String("GLTF 2.0".to_owned()));
        let _prev = map.insert("data".to_owned(), Llsd::String(data.to_owned()));
        Llsd::Map(map)
    }

    /// A binary-LLSD envelope (as Second Life serializes the asset) decodes,
    /// whether or not it carries the `<? LLSD/Binary ?>` header the fetch returns.
    #[test]
    fn decodes_binary_envelope_with_and_without_header() -> Result<(), TestError> {
        let binary = envelope("1.1", &sample_gltf()).to_llsd_binary();

        let headerless = parse_material_asset(&binary)?;
        assert_eq!(headerless.alpha_mode, GltfAlphaMode::Mask);

        let mut with_header = b"<? LLSD/Binary ?>\n".to_vec();
        with_header.extend_from_slice(&binary);
        let headered = parse_material_asset(&with_header)?;
        approx_slice(&headered.base_color, &[0.5, 0.25, 0.125, 0.8]);
        Ok(())
    }

    /// An XML-LLSD envelope decodes too (the serialization is auto-detected).
    #[test]
    fn decodes_xml_envelope() -> Result<(), TestError> {
        let xml = envelope("1.0", &sample_gltf()).to_llsd_xml();
        let material = parse_material_asset(xml.as_bytes())?;
        assert!(material.double_sided);
        Ok(())
    }

    /// A rejected version and a wrong asset type each surface as an error rather
    /// than a bogus material.
    #[test]
    fn rejects_bad_envelope_metadata() {
        let bad_version = envelope("9.9", &sample_gltf()).to_llsd_binary();
        assert!(matches!(
            parse_material_asset(&bad_version),
            Err(MaterialError::UnsupportedVersion(_))
        ));

        let mut map = HashMap::new();
        let _prev = map.insert("version".to_owned(), Llsd::String("1.1".to_owned()));
        let _prev = map.insert("type".to_owned(), Llsd::String("NOT GLTF".to_owned()));
        let _prev = map.insert("data".to_owned(), Llsd::String(sample_gltf()));
        let wrong_type = Llsd::Map(map).to_llsd_binary();
        assert!(matches!(
            parse_material_asset(&wrong_type),
            Err(MaterialError::UnsupportedType(_))
        ));
    }
}
