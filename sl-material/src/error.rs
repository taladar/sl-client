//! The error type [`parse_material_asset`](crate::parse_material_asset) returns.

use sl_llsd::LlsdError;

/// Why decoding an `AT_MATERIAL` asset failed.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_material::MaterialError`, where it reads clearly"
)]
#[derive(Debug, thiserror::Error)]
pub enum MaterialError {
    /// The LLSD envelope could not be parsed (neither a recognised binary nor
    /// XML LLSD serialization).
    #[error("material LLSD envelope did not parse: {0}")]
    Envelope(#[from] LlsdError),
    /// The envelope was valid LLSD but not a map with the expected `version` /
    /// `type` / `data` fields.
    #[error("material asset envelope is not a well-formed GLTF material wrapper")]
    MalformedEnvelope,
    /// The envelope's `version` field is not one this decoder accepts
    /// (`"1.0"` / `"1.1"`).
    #[error("unsupported material asset version {0:?}")]
    UnsupportedVersion(String),
    /// The envelope's `type` field is not `"GLTF 2.0"`.
    #[error("unsupported material asset type {0:?}")]
    UnsupportedType(String),
    /// The `data` glTF document did not parse as JSON.
    #[error("material glTF document did not parse: {0}")]
    Json(#[from] serde_json::Error),
    /// The glTF document carried no material at index 0.
    #[error("material glTF document contains no material")]
    NoMaterial,
}
