//! The landmark asset body: the tiny text record behind an inventory landmark
//! (`AssetType::Landmark`) that names where a teleport goes.
//!
//! Two on-wire versions exist (`LLLandmark::constructFromString`,
//! `indra/llinventory/lllandmark.cpp`):
//!
//! ```text
//! Landmark version 2
//! region_id <uuid>
//! local_pos <x> <y> <z>
//! ```
//!
//! is what every modern grid writes — the destination is a region id plus a
//! region-local position, resolved to a handle at teleport time — while the
//! original
//!
//! ```text
//! Landmark version 1
//! position <x> <y> <z>
//! ```
//!
//! carried a global position only. Both decode; only version 2 is written.

use sl_types::map::{GlobalCoordinates, RegionCoordinates};
use uuid::Uuid;

use crate::error::WireError;

/// The version line of the landmark body written by [`landmark_to_wire`].
const VERSION_2_HEADER: &str = "Landmark version 2";

/// A decoded landmark asset.
#[derive(Debug, Clone, PartialEq)]
pub enum LandmarkAsset {
    /// A version-2 landmark: a region id and a region-local position.
    Regional {
        /// The destination region's id.
        region_id: Uuid,
        /// The region-local landing position.
        position: RegionCoordinates,
    },
    /// A version-1 landmark: a global position only (legacy assets).
    Global(GlobalCoordinates),
}

impl LandmarkAsset {
    /// The region-local landing position, for either version: a global
    /// landmark's position folded into its region (`None` when the global
    /// position does not split into a grid cell, e.g. a negative coordinate).
    #[must_use]
    pub fn local_position(&self) -> Option<RegionCoordinates> {
        match self {
            Self::Regional { position, .. } => Some(*position),
            Self::Global(global) => global.split().map(|(_, region)| region),
        }
    }
}

/// The fault for a missing or malformed line.
fn invalid(field: &'static str, value: &str) -> WireError {
    WireError::InvalidScalar {
        field,
        value: value.to_owned(),
    }
}

/// Parses three whitespace-separated numbers after a keyword.
fn parse_triple<T: std::str::FromStr>(
    rest: &str,
    field: &'static str,
) -> Result<[T; 3], WireError> {
    let mut parts = rest.split_whitespace();
    let mut next = || {
        parts
            .next()
            .and_then(|part| part.parse::<T>().ok())
            .ok_or_else(|| invalid(field, rest))
    };
    Ok([next()?, next()?, next()?])
}

/// Decodes a landmark asset body (either version).
///
/// # Errors
///
/// Returns [`WireError::InvalidScalar`] naming the line that is missing or
/// malformed (`version`, `region_id`, `local_pos`, `position`), and
/// [`WireError::UnsupportedLandmarkVersion`] for a version other than 1 or 2.
pub fn parse_landmark(text: &str) -> Result<LandmarkAsset, WireError> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let version = lines
        .next()
        .and_then(|line| line.strip_prefix("Landmark version "))
        .and_then(|rest| rest.trim().parse::<u32>().ok())
        .ok_or_else(|| invalid("version", text))?;
    match version {
        1 => {
            let rest = lines
                .next()
                .and_then(|line| line.strip_prefix("position "))
                .ok_or_else(|| invalid("position", text))?;
            let [x, y, z] = parse_triple::<f64>(rest, "position")?;
            Ok(LandmarkAsset::Global(GlobalCoordinates::new(x, y, z)))
        }
        2 => {
            let mut region_id = None;
            let mut position = None;
            for line in lines {
                if let Some(rest) = line.strip_prefix("region_id ") {
                    let id = rest
                        .trim()
                        .parse::<Uuid>()
                        .ok()
                        .filter(|id| !id.is_nil())
                        .ok_or_else(|| invalid("region_id", rest))?;
                    region_id = Some(id);
                } else if let Some(rest) = line.strip_prefix("local_pos ") {
                    let [x, y, z] = parse_triple::<f32>(rest, "local_pos")?;
                    position = Some(RegionCoordinates::new(x, y, z));
                }
            }
            Ok(LandmarkAsset::Regional {
                region_id: region_id.ok_or_else(|| invalid("region_id", text))?,
                position: position.ok_or_else(|| invalid("local_pos", text))?,
            })
        }
        other => Err(WireError::UnsupportedLandmarkVersion { version: other }),
    }
}

/// Encodes a version-2 landmark body (the only version modern grids write).
/// The reference writer is `LLLandmark` / the simulator's landmark creation:
/// `Landmark version 2\nregion_id <uuid>\nlocal_pos <x> <y> <z>\n`.
#[must_use]
pub fn landmark_to_wire(region_id: Uuid, position: RegionCoordinates) -> String {
    format!(
        "{VERSION_2_HEADER}\nregion_id {region_id}\nlocal_pos {} {} {}\n",
        position.x(),
        position.y(),
        position.z()
    )
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn version_2_round_trips() -> Result<(), WireError> {
        let region_id = Uuid::from_u128(0x3b6b_7c62_8f8f_4e34_9c1a_79c2_e2ba_0fd1);
        let position = RegionCoordinates::new(128.5, 64.25, 22.0);
        let body = landmark_to_wire(region_id, position);
        assert_eq!(
            body,
            "Landmark version 2\nregion_id 3b6b7c62-8f8f-4e34-9c1a-79c2e2ba0fd1\nlocal_pos 128.5 64.25 22\n"
        );
        let parsed = parse_landmark(&body)?;
        assert_eq!(
            parsed,
            LandmarkAsset::Regional {
                region_id,
                position
            }
        );
        assert_eq!(parsed.local_position(), Some(position));
        Ok(())
    }

    #[test]
    fn version_1_decodes_a_global_position() -> Result<(), WireError> {
        let parsed = parse_landmark("Landmark version 1\nposition 256128.5 256064.25 22\n")?;
        assert_eq!(
            parsed,
            LandmarkAsset::Global(GlobalCoordinates::new(256_128.5, 256_064.25, 22.0))
        );
        // The global position folds into its region.
        assert_eq!(
            parsed.local_position(),
            Some(RegionCoordinates::new(128.5, 64.25, 22.0))
        );
        Ok(())
    }

    #[test]
    fn malformed_bodies_name_the_bad_line() {
        assert!(matches!(
            parse_landmark(""),
            Err(WireError::InvalidScalar {
                field: "version",
                ..
            })
        ));
        assert!(matches!(
            parse_landmark("Landmark version 2\n"),
            Err(WireError::InvalidScalar {
                field: "region_id",
                ..
            })
        ));
        assert!(matches!(
            parse_landmark(&format!("Landmark version 2\nregion_id {}\n", Uuid::nil())),
            Err(WireError::InvalidScalar {
                field: "region_id",
                ..
            })
        ));
        assert!(matches!(
            parse_landmark(&format!(
                "Landmark version 2\nregion_id {}\nlocal_pos 1 2\n",
                Uuid::from_u128(7)
            )),
            Err(WireError::InvalidScalar {
                field: "local_pos",
                ..
            })
        ));
        assert!(matches!(
            parse_landmark("Landmark version 3\n"),
            Err(WireError::UnsupportedLandmarkVersion { version: 3 })
        ));
    }
}
