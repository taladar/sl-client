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

    /// Serialize this asset back into the `LLWearable` text form, mirroring the
    /// reference viewer's `LLWearable::exportStream`: the version header, the
    /// name line, the permissions and sale-info blocks from `perms`, then the
    /// `type`, `parameters`, and `textures` sections. The output round-trips
    /// through [`parse`](Self::parse) (the appearance editor's Save path authors
    /// the edited asset with this).
    ///
    /// The permissions / sale-info blocks are not retained on parse (a bake does
    /// not need them), so they are supplied by the caller — the appearance editor
    /// takes them from the wearable's inventory item, as the reference does.
    #[must_use]
    pub fn to_text(&self, perms: &WearablePermissions) -> String {
        use std::fmt::Write as _;
        let mut text = String::new();
        let _written = writeln!(text, "LLWearable version {}", self.version);
        let _written = writeln!(text, "{}", self.name);
        let _written = writeln!(text);
        let _written = writeln!(text, "\tpermissions 0");
        let _written = writeln!(text, "\t{{");
        let _written = writeln!(text, "\t\tbase_mask\t{:08x}", perms.base_mask);
        let _written = writeln!(text, "\t\towner_mask\t{:08x}", perms.owner_mask);
        let _written = writeln!(text, "\t\tgroup_mask\t{:08x}", perms.group_mask);
        let _written = writeln!(text, "\t\teveryone_mask\t{:08x}", perms.everyone_mask);
        let _written = writeln!(text, "\t\tnext_owner_mask\t{:08x}", perms.next_owner_mask);
        let _written = writeln!(text, "\t\tcreator_id\t{}", perms.creator_id);
        let _written = writeln!(text, "\t\towner_id\t{}", perms.owner_id);
        let _written = writeln!(text, "\t\tlast_owner_id\t{}", perms.last_owner_id);
        let _written = writeln!(text, "\t\tgroup_id\t{}", perms.group_id);
        let _written = writeln!(text, "\t}}");
        let _written = writeln!(text, "\tsale_info\t0");
        let _written = writeln!(text, "\t{{");
        let _written = writeln!(text, "\t\tsale_type\t{}", perms.sale_type.wire());
        let _written = writeln!(text, "\t\tsale_price\t{}", perms.sale_price);
        let _written = writeln!(text, "\t}}");
        let _written = writeln!(text, "type {}", self.wearable_type.to_code());
        let _written = writeln!(text, "parameters {}", self.params.len());
        for (id, weight) in &self.params {
            let _written = writeln!(text, "{id} {weight}");
        }
        let _written = writeln!(text, "textures {}", self.textures.len());
        for (te, id) in &self.textures {
            let _written = writeln!(text, "{te} {id}");
        }
        text
    }
}

/// The permissions and sale-info a [`WearableAsset::to_text`] writes into the
/// `LLWearable` header blocks. These are not part of the bake-relevant state
/// [`WearableAsset`] retains, so the appearance editor supplies them from the
/// wearable's inventory item on Save (the reference viewer's
/// `LLWearable::exportStream` likewise takes the permissions from the item).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `WearablePermissions` reads clearly"
)]
pub struct WearablePermissions {
    /// The base permission mask.
    pub base_mask: u32,
    /// The owner permission mask.
    pub owner_mask: u32,
    /// The group permission mask.
    pub group_mask: u32,
    /// The everyone permission mask.
    pub everyone_mask: u32,
    /// The next-owner permission mask.
    pub next_owner_mask: u32,
    /// The asset creator.
    pub creator_id: Uuid,
    /// The current owner.
    pub owner_id: Uuid,
    /// The previous owner (nil if never transferred).
    pub last_owner_id: Uuid,
    /// The group the asset is shared with (nil if none).
    pub group_id: Uuid,
    /// How the wearable is offered for sale.
    pub sale_type: SaleType,
    /// The sale price in L$ (meaningful only when `sale_type` is not
    /// [`SaleType::Not`]).
    pub sale_price: i32,
}

/// How a wearable is offered for sale — the `sale_type` written into the
/// `sale_info` block, matching the reference viewer's `LLSaleInfo::EForSale`
/// keywords.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SaleType {
    /// Not for sale (`not`).
    #[default]
    Not,
    /// The original object is sold and transfers to the buyer (`orig`).
    Original,
    /// A copy is sold (`copy`).
    Copy,
    /// The object's contents are sold (`cntn`).
    Contents,
}

impl SaleType {
    /// The wire keyword for this sale type (the `sale_type` value).
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Not => "not",
            Self::Original => "orig",
            Self::Copy => "copy",
            Self::Contents => "cntn",
        }
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
    fn to_text_round_trips_through_parse() -> Result<(), TestError> {
        use super::{SaleType, WearablePermissions};
        let asset = WearableAsset::parse(SKIN)?;
        let perms = WearablePermissions {
            base_mask: 0x7fff_ffff,
            owner_mask: 0x7fff_ffff,
            group_mask: 0,
            everyone_mask: 0,
            next_owner_mask: 0x0008_e000,
            creator_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111")?,
            owner_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111")?,
            last_owner_id: Uuid::nil(),
            group_id: Uuid::nil(),
            sale_type: SaleType::Not,
            sale_price: 10,
        };
        let text = asset.to_text(&perms);
        let reparsed = WearableAsset::parse(&text)?;
        assert_eq!(reparsed, asset);
        Ok(())
    }

    #[test]
    fn to_text_preserves_edited_params_and_textures() -> Result<(), TestError> {
        use super::{SaleType, WearablePermissions};
        let mut asset = WearableAsset::parse(SKIN)?;
        // Edit a param weight and add a second texture layer, as the editor would.
        let _prev = asset.params.insert(108, 0.75);
        let new_tex = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")?;
        let _prev = asset.textures.insert(
            u32::try_from(avatar_texture::UPPER_BODYPAINT).unwrap_or_default(),
            new_tex,
        );
        let perms = WearablePermissions {
            base_mask: 0x7fff_ffff,
            owner_mask: 0x7fff_ffff,
            group_mask: 0,
            everyone_mask: 0,
            next_owner_mask: 0,
            creator_id: Uuid::nil(),
            owner_id: Uuid::nil(),
            last_owner_id: Uuid::nil(),
            group_id: Uuid::nil(),
            sale_type: SaleType::Copy,
            sale_price: 0,
        };
        let reparsed = WearableAsset::parse(&asset.to_text(&perms))?;
        assert_eq!(reparsed.params.get(&108), Some(&0.75));
        assert_eq!(
            reparsed.layer_texture(avatar_texture::UPPER_BODYPAINT),
            Some(new_tex)
        );
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
