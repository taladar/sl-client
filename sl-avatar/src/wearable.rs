//! Parse the legacy **wearable asset** text format (`LLWearable`) into the layer
//! texture ids and visual-param weights a client-side bake needs.
//!
//! A body-part or clothing inventory item points at a wearable *asset*: a short
//! text file the reference viewer reads in `LLWearable::importStream`. It names
//! the wearable [`type`](WearableType), a table of visual-param weights (which
//! colour / shape the wearable), and a table of per-layer texture ids keyed by
//! avatar `TextureEntry` slot (see [`sl_proto::avatar_texture`]). The
//! client-side baker (P15.2) fetches each worn wearable asset, parses it here,
//! and feeds the resulting layer texture ids + weights into the `sl-bake`
//! compositor.
//!
//! The format is line-oriented:
//!
//! ```text
//! LLWearable version 22
//! New Skin
//! <description line, may be empty>
//! permissions 0
//! { … }
//! sale_info 0
//! { … }
//! type 1
//! parameters 3
//! 111 0.5
//! 108 0
//! 110 0
//! textures 1
//! 0 5748decc-f629-461c-9a36-a35a221fe21f
//! ```
//!
//! We do not need the permissions / sale-info blocks, so — after reading the
//! version and name — the parser scans for the `type`, `parameters`, and
//! `textures` markers (whose keywords never appear inside those blocks) and
//! reads the counted rows that follow each, mirroring the reference viewer's
//! `getNextPopulatedLine` (blank lines are skipped between rows).

use std::collections::BTreeMap;

use sl_proto::Uuid;
use sl_proto::WearableType;
use sl_proto::avatar_texture;

/// An error parsing a [`WearableAsset`] from its text form.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `WearableError` reads clearly"
)]
pub enum WearableError {
    /// The asset did not begin with a `LLWearable version <n>` header.
    #[error("missing or malformed `LLWearable version` header")]
    BadHeader,
    /// The `type <n>` line was missing or not a number.
    #[error("missing or malformed `type` line")]
    BadType,
    /// A `parameters <n>` / `textures <n>` count header was malformed.
    #[error("malformed `{section}` count header")]
    BadCount {
        /// Which section header was malformed (`parameters` or `textures`).
        section: &'static str,
    },
    /// A `parameters` row was not an `<id> <weight>` pair.
    #[error("malformed parameter row: {row:?}")]
    BadParameter {
        /// The offending row text.
        row: String,
    },
    /// A `textures` row was not a `<te-index> <uuid>` pair, or the UUID / index
    /// was invalid.
    #[error("malformed texture row: {row:?}")]
    BadTexture {
        /// The offending row text.
        row: String,
    },
    /// The asset ended before a counted section had all its rows.
    #[error("unexpected end of wearable asset while reading `{section}`")]
    Truncated {
        /// Which section ran out of rows.
        section: &'static str,
    },
}

/// A parsed wearable asset: its [`type`](WearableType), the visual-param weights
/// it carries, and its per-layer texture ids keyed by avatar `TextureEntry` slot
/// (an [`avatar_texture`] layer index).
///
/// Only the fields a client-side bake needs are kept; permissions, sale info,
/// name, and description are parsed past but not retained (except the name, kept
/// for logging).
#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `WearableAsset` reads clearly"
)]
pub struct WearableAsset {
    /// The asset-definition version from the header (`LLWearable version <n>`).
    pub version: i32,
    /// The wearable's display name (the line after the version header).
    pub name: String,
    /// Which wearable slot this asset is (`type <n>`).
    pub wearable_type: WearableType,
    /// The visual-param weights, keyed by param id — the raw weights the asset
    /// stored (a colour param's tint input, a shape param's morph weight, …).
    pub params: BTreeMap<i32, f32>,
    /// The per-layer texture ids, keyed by avatar `TextureEntry` slot index (an
    /// [`avatar_texture`] layer constant); a nil id means "no texture".
    pub textures: BTreeMap<u32, Uuid>,
}

impl WearableAsset {
    /// Parse a wearable asset from its text form.
    ///
    /// # Errors
    ///
    /// Returns a [`WearableError`] if the header, type, a section count, or a
    /// parameter / texture row is missing or malformed.
    pub fn parse(text: &str) -> Result<Self, WearableError> {
        let mut lines = text.lines();

        // Header: `LLWearable version <n>` (the first populated line).
        let header = next_populated(&mut lines).ok_or(WearableError::BadHeader)?;
        let version = header
            .trim()
            .strip_prefix("LLWearable version ")
            .and_then(|rest| rest.trim().parse::<i32>().ok())
            .ok_or(WearableError::BadHeader)?;

        // Name is the very next line (may be empty); the reference viewer reads it
        // with a plain getline, so it is not skipped even when blank.
        let name = lines.next().unwrap_or_default().trim().to_owned();

        // Scan for the `type`, `parameters`, and `textures` markers. Their
        // keywords never appear inside the permissions / sale-info blocks, so a
        // forward scan is unambiguous.
        let mut wearable_type: Option<WearableType> = None;
        let mut params = BTreeMap::new();
        let mut textures = BTreeMap::new();

        while let Some(line) = next_populated(&mut lines) {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("type ") {
                let code = rest
                    .trim()
                    .parse::<u8>()
                    .map_err(|_ignored| WearableError::BadType)?;
                wearable_type = Some(WearableType::from_code(code));
            } else if let Some(rest) = line.strip_prefix("parameters ") {
                let count = parse_count(rest, "parameters")?;
                for _ in 0..count {
                    let row = next_populated(&mut lines).ok_or(WearableError::Truncated {
                        section: "parameters",
                    })?;
                    let (id, weight) = parse_parameter(row)?;
                    let _prev = params.insert(id, weight);
                }
            } else if let Some(rest) = line.strip_prefix("textures ") {
                let count = parse_count(rest, "textures")?;
                for _ in 0..count {
                    let row = next_populated(&mut lines).ok_or(WearableError::Truncated {
                        section: "textures",
                    })?;
                    let (te, id) = parse_texture(row)?;
                    let _prev = textures.insert(te, id);
                }
            }
        }

        let wearable_type = wearable_type.ok_or(WearableError::BadType)?;
        Ok(Self {
            version,
            name,
            wearable_type,
            params,
            textures,
        })
    }

    /// The texture id at avatar `TextureEntry` layer `slot`, if the asset carries
    /// a non-nil one there. `None` for an absent or nil (no-texture) slot.
    #[must_use]
    pub fn layer_texture(&self, slot: usize) -> Option<Uuid> {
        let slot = u32::try_from(slot).ok()?;
        self.textures.get(&slot).copied().filter(|id| !id.is_nil())
    }

    /// Whether this asset supplies the layer texture for avatar `TextureEntry`
    /// `slot` *and* the slot's canonical wearable type matches this asset (so a
    /// mislabeled texture on the wrong wearable is ignored). Used by the baker to
    /// pick the wearable feeding each bake layer.
    #[must_use]
    pub fn supplies_layer(&self, slot: usize) -> bool {
        avatar_texture::layer_wearable_type(slot) == Some(self.wearable_type)
            && self.layer_texture(slot).is_some()
    }
}

/// The next non-blank line from `lines`, mirroring the reference viewer's
/// `getNextPopulatedLine` (skip lines that are empty once trimmed).
fn next_populated<'text>(lines: &mut std::str::Lines<'text>) -> Option<&'text str> {
    lines.by_ref().find(|line| !line.trim().is_empty())
}

/// Parse a section-count header suffix (the text after `parameters `/`textures `).
fn parse_count(rest: &str, section: &'static str) -> Result<u32, WearableError> {
    rest.trim()
        .parse::<u32>()
        .map_err(|_ignored| WearableError::BadCount { section })
}

/// Parse a `parameters` row into its `(id, weight)`.
fn parse_parameter(row: &str) -> Result<(i32, f32), WearableError> {
    let mut parts = row.split_whitespace();
    let bad = || WearableError::BadParameter {
        row: row.to_owned(),
    };
    let id = parts
        .next()
        .and_then(|p| p.parse::<i32>().ok())
        .ok_or_else(bad)?;
    let weight = parts
        .next()
        .and_then(|p| p.parse::<f32>().ok())
        .ok_or_else(bad)?;
    Ok((id, weight))
}

/// Parse a `textures` row into its `(te-index, uuid)`.
fn parse_texture(row: &str) -> Result<(u32, Uuid), WearableError> {
    let mut parts = row.split_whitespace();
    let bad = || WearableError::BadTexture {
        row: row.to_owned(),
    };
    let te = parts
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .ok_or_else(bad)?;
    let id = parts
        .next()
        .and_then(|p| Uuid::parse_str(p).ok())
        .ok_or_else(bad)?;
    Ok((te, id))
}

#[cfg(test)]
mod tests {
    use super::{WearableAsset, WearableError};
    use pretty_assertions::assert_eq;
    use sl_proto::Uuid;
    use sl_proto::WearableType;
    use sl_proto::avatar_texture;

    /// A boxed error so a test can `?` through parsing without `expect`.
    type TestError = Box<dyn std::error::Error>;

    /// A realistic skin (body-part) wearable asset with permissions / sale-info
    /// blocks, a few colour params, and one head-bodypaint texture.
    const SKIN: &str = "LLWearable version 22\n\
        My Skin\n\
        \n\
        \tpermissions 0\n\
        \t{\n\
        \t\tbase_mask\t7fffffff\n\
        \t\towner_mask\t7fffffff\n\
        \t\tcreator_id\t11111111-1111-1111-1111-111111111111\n\
        \t}\n\
        \tsale_info 0\n\
        \t{\n\
        \t\tsale_type\tnot\n\
        \t\tsale_price\t0\n\
        \t}\n\
        type 1\n\
        parameters 3\n\
        111 0.5\n\
        108 0\n\
        110 0.25\n\
        textures 1\n\
        0 5748decc-f629-461c-9a36-a35a221fe21f\n";

    #[test]
    fn parses_skin_type_params_and_textures() -> Result<(), TestError> {
        let asset = WearableAsset::parse(SKIN)?;
        assert_eq!(asset.version, 22);
        assert_eq!(asset.name, "My Skin");
        assert_eq!(asset.wearable_type, WearableType::Skin);
        assert_eq!(asset.params.get(&111), Some(&0.5));
        assert_eq!(asset.params.get(&108), Some(&0.0));
        assert_eq!(asset.params.get(&110), Some(&0.25));
        assert_eq!(
            asset.layer_texture(avatar_texture::HEAD_BODYPAINT),
            Some(Uuid::parse_str("5748decc-f629-461c-9a36-a35a221fe21f")?)
        );
        // The skin supplies the head-bodypaint layer (right type + a texture).
        assert!(asset.supplies_layer(avatar_texture::HEAD_BODYPAINT));
        // It does not supply a shirt layer (wrong wearable type).
        assert!(!asset.supplies_layer(avatar_texture::UPPER_SHIRT));
        Ok(())
    }

    #[test]
    fn empty_name_and_no_textures() -> Result<(), TestError> {
        let text = "LLWearable version 22\n\
            \n\
            type 5\n\
            parameters 0\n\
            textures 0\n";
        let asset = WearableAsset::parse(text)?;
        assert_eq!(asset.name, "");
        assert_eq!(asset.wearable_type, WearableType::Pants);
        assert!(asset.params.is_empty());
        assert!(asset.textures.is_empty());
        assert_eq!(asset.layer_texture(avatar_texture::LOWER_PANTS), None);
        Ok(())
    }

    #[test]
    fn nil_texture_is_not_supplied() -> Result<(), TestError> {
        let text = "LLWearable version 22\n\
            Alpha\n\
            type 13\n\
            parameters 0\n\
            textures 1\n\
            23 00000000-0000-0000-0000-000000000000\n";
        let asset = WearableAsset::parse(text)?;
        assert_eq!(asset.wearable_type, WearableType::Alpha);
        assert_eq!(asset.layer_texture(avatar_texture::HEAD_ALPHA), None);
        assert!(!asset.supplies_layer(avatar_texture::HEAD_ALPHA));
        Ok(())
    }

    #[test]
    fn bad_header_is_rejected() {
        assert_eq!(
            WearableAsset::parse("not a wearable\n"),
            Err(WearableError::BadHeader)
        );
    }

    #[test]
    fn missing_type_is_rejected() {
        let text = "LLWearable version 22\nName\nparameters 0\ntextures 0\n";
        assert_eq!(WearableAsset::parse(text), Err(WearableError::BadType));
    }

    #[test]
    fn truncated_parameters_is_rejected() {
        let text = "LLWearable version 22\nName\ntype 1\nparameters 2\n111 0.5\n";
        assert_eq!(
            WearableAsset::parse(text),
            Err(WearableError::Truncated {
                section: "parameters"
            })
        );
    }
}
