//! The **`SimulatorFeatures`** capability: the region's feature/capability flags.
//!
//! On arriving in a region the viewer GETs the `SimulatorFeatures` capability to
//! learn what the simulator supports — whether mesh upload/rez is allowed, the
//! physics-shape types it accepts, attachment/group limits, the GLTF/PBR-terrain
//! switches, and (on OpenSim grids) a nested `OpenSimExtras` map of grid-specific
//! settings such as chat ranges, the currency symbol, and prim-scale limits.
//! There is no UDP equivalent; the feature set lives entirely behind this HTTP
//! capability and is surfaced at handshake.
//!
//! This module decodes that reply (client side) and builds it (server side). The
//! LLSD keys and their types are cross-checked against the Firestorm viewer's
//! `indra/newview/llviewerregion.cpp` (`setSimulatorFeatures`) and
//! `lfsimfeaturehandler.cpp`, and OpenSim's `SimulatorFeaturesModule.cs`.
//!
//! The capability is a single GET returning an LLSD map. Different grids
//! advertise different subsets (Second Life omits the `OpenSimExtras` subtree,
//! OpenSim omits the Second Life PBR switches), so every decoded field is an
//! [`Option`]: an absent key decodes to [`None`] — the grid did **not** advertise
//! it — distinct from a value it did send, so a caller can tell "advertised
//! disabled" from "not advertised at all".

use std::collections::HashMap;

use uuid::Uuid;

use crate::WireError;
use crate::llsd::Llsd;

/// Which collision-shape types the simulator accepts for a prim's physics shape
/// (`PhysicsShapeTypes`). The viewer enables the corresponding entries in the
/// build tool's physics-shape dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhysicsShapeTypes {
    /// The convex-hull shape is accepted (`convex`).
    pub convex: bool,
    /// The "none" (non-physical) shape is accepted (`none`).
    pub none: bool,
    /// The exact-prim shape is accepted (`prim`).
    pub prim: bool,
}

/// Animated-object (animesh) limits (`AnimatedObjects`): the triangle budget for
/// one animated object and how many animated objects an agent may wear at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnimatedObjects {
    /// The maximum triangle count of a single animated object
    /// (`AnimatedObjectMaxTris`).
    pub max_tris: i32,
    /// The maximum number of animated objects an agent may attach
    /// (`MaxAgentAnimatedObjectAttachments`).
    pub max_agent_attachments: i32,
}

/// The OpenSim-specific `OpenSimExtras` subtree of a `SimulatorFeatures` reply.
/// Second Life omits this map entirely; OpenSim grids fill in the subset their
/// configuration enables. Every field is an [`Option`]: [`None`] means the grid
/// did **not** advertise the key (so the caller applies its own default — e.g.
/// the 20/100/10 m chat ranges), distinct from a value the grid did send.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpenSimExtras {
    /// Whether the grid permits the "export" creator permission (`ExportSupported`).
    pub export_supported: Option<bool>,
    /// The grid's map-tile server base URL (`map-server-url`).
    pub map_server_url: Option<String>,
    /// The grid's web search endpoint (`search-server-url`).
    pub search_server_url: Option<String>,
    /// The grid's destination-guide URL (`destination-guide-url`).
    pub destination_guide_url: Option<String>,
    /// The grid's avatar-picker URL (`avatar-picker-url`).
    pub avatar_picker_url: Option<String>,
    /// The grid's HyperGrid URL prefix (`GridURL`).
    pub grid_url: Option<String>,
    /// The currency symbol the grid displays (`currency`), e.g. `"OS$"`.
    pub currency: Option<String>,
    /// The `llSay`/normal chat range in metres (`say-range`; viewer default 20).
    pub say_range: Option<i32>,
    /// The `llShout` range in metres (`shout-range`; viewer default 100).
    pub shout_range: Option<i32>,
    /// The `llWhisper` range in metres (`whisper-range`; viewer default 10).
    pub whisper_range: Option<i32>,
    /// The smallest prim dimension the grid allows (`MinPrimScale`).
    pub min_prim_scale: Option<f32>,
    /// The largest prim dimension the grid allows (`MaxPrimScale`).
    pub max_prim_scale: Option<f32>,
    /// The largest physical-prim dimension the grid allows (`MaxPhysPrimScale`).
    pub max_phys_prim_scale: Option<f32>,
}

/// The decoded `SimulatorFeatures` reply: the region's feature flags and limits.
///
/// A grid advertises only the subset its configuration enables, so every field
/// is an [`Option`]: [`None`] means the grid did **not** advertise the key
/// (feature not supported / limit unknown), distinct from a value it did send —
/// so a caller can tell "advertised disabled" (`Some(false)`) from "not
/// advertised at all" (`None`). The OpenSim-only grid extras live in
/// [`open_sim_extras`](Self::open_sim_extras), which is [`None`] on Second Life
/// (and any grid omitting the `OpenSimExtras` map).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SimulatorFeatures {
    /// Whether rezzing mesh objects is permitted (`MeshRezEnabled`).
    pub mesh_rez_enabled: Option<bool>,
    /// Whether uploading mesh assets is permitted (`MeshUploadEnabled`).
    pub mesh_upload_enabled: Option<bool>,
    /// Whether the legacy mesh-xfer path is enabled (`MeshXferEnabled`, OpenSim).
    pub mesh_xfer_enabled: Option<bool>,
    /// Whether bakes-on-mesh is supported (`BakesOnMeshEnabled`).
    pub bakes_on_mesh_enabled: Option<bool>,
    /// Whether per-prim physics materials are supported (`PhysicsMaterialsEnabled`).
    pub physics_materials_enabled: Option<bool>,
    /// The accepted physics-shape types (`PhysicsShapeTypes`); [`None`] when the
    /// grid did not advertise the map.
    pub physics_shape_types: Option<PhysicsShapeTypes>,
    /// Animated-object (animesh) limits (`AnimatedObjects`); [`None`] when the
    /// grid did not advertise the map.
    pub animated_objects: Option<AnimatedObjects>,
    /// The maximum number of attachments an agent may wear (`MaxAgentAttachments`).
    pub max_agent_attachments: Option<i32>,
    /// The free-account group cap (`MaxAgentGroupsBasic`, OpenSim).
    pub max_agent_groups_basic: Option<i32>,
    /// The premium-account group cap (`MaxAgentGroupsPremium`, OpenSim).
    pub max_agent_groups_premium: Option<i32>,
    /// The maximum texture dimension the simulator serves (`MaxTextureResolution`).
    pub max_texture_resolution: Option<i32>,
    /// Whether PBR (GLTF) terrain is enabled (`PBRTerrainEnabled`, Second Life).
    pub pbr_terrain_enabled: Option<bool>,
    /// Whether GLTF scene objects are enabled (`GLTFEnabled`, Second Life).
    pub gltf_enabled: Option<bool>,
    /// The asset id of the LSL syntax definition for this simulator (`LSLSyntaxId`).
    pub lsl_syntax_id: Option<Uuid>,
    /// The OpenSim-only grid extras, or [`None`] on grids omitting them.
    pub open_sim_extras: Option<OpenSimExtras>,
}

/// Reads a boolean field from an LLSD map, defaulting to `false` when absent. A
/// present key of the wrong LLSD kind is rejected (see [`Llsd::field_bool`]).
fn map_bool(map: &Llsd, key: &'static str) -> Result<bool, WireError> {
    Ok(map.field_bool(key, key)?.unwrap_or(false))
}

/// Reads an integer field from an LLSD map, defaulting to `0` when absent. A
/// present key of the wrong LLSD kind is rejected (see [`Llsd::field_i32`]).
fn map_int(map: &Llsd, key: &'static str) -> Result<i32, WireError> {
    Ok(map.field_i32(key, key)?.unwrap_or(0))
}

impl OpenSimExtras {
    /// Decodes the `OpenSimExtras` map. An absent key decodes to [`None`] (the
    /// grid did not advertise it); a present key of the wrong LLSD kind is a hard
    /// error.
    fn from_llsd(map: &Llsd) -> Result<Self, WireError> {
        Ok(Self {
            export_supported: map.field_bool("ExportSupported", "ExportSupported")?,
            map_server_url: map
                .field_str("map-server-url", "map-server-url")?
                .map(str::to_owned),
            search_server_url: map
                .field_str("search-server-url", "search-server-url")?
                .map(str::to_owned),
            destination_guide_url: map
                .field_str("destination-guide-url", "destination-guide-url")?
                .map(str::to_owned),
            avatar_picker_url: map
                .field_str("avatar-picker-url", "avatar-picker-url")?
                .map(str::to_owned),
            grid_url: map.field_str("GridURL", "GridURL")?.map(str::to_owned),
            currency: map.field_str("currency", "currency")?.map(str::to_owned),
            say_range: map.field_i32("say-range", "say-range")?,
            shout_range: map.field_i32("shout-range", "shout-range")?,
            whisper_range: map.field_i32("whisper-range", "whisper-range")?,
            min_prim_scale: map.field_f32("MinPrimScale", "MinPrimScale")?,
            max_prim_scale: map.field_f32("MaxPrimScale", "MaxPrimScale")?,
            max_phys_prim_scale: map.field_f32("MaxPhysPrimScale", "MaxPhysPrimScale")?,
        })
    }

    /// Encodes this subtree as an `OpenSimExtras` LLSD map — the inverse of
    /// [`from_llsd`](Self::from_llsd). Only advertised (`Some`) keys are emitted,
    /// so a round-trip preserves the advertised-vs-absent distinction.
    #[must_use]
    fn to_llsd(&self) -> Llsd {
        let mut map: HashMap<String, Llsd> = HashMap::new();
        let mut put = |key: &str, value: Llsd| {
            let _previous = map.insert(key.to_owned(), value);
        };
        if let Some(value) = self.export_supported {
            put("ExportSupported", Llsd::Boolean(value));
        }
        if let Some(value) = &self.map_server_url {
            put("map-server-url", Llsd::String(value.clone()));
        }
        if let Some(value) = &self.search_server_url {
            put("search-server-url", Llsd::String(value.clone()));
        }
        if let Some(value) = &self.destination_guide_url {
            put("destination-guide-url", Llsd::String(value.clone()));
        }
        if let Some(value) = &self.avatar_picker_url {
            put("avatar-picker-url", Llsd::String(value.clone()));
        }
        if let Some(value) = &self.grid_url {
            put("GridURL", Llsd::String(value.clone()));
        }
        if let Some(value) = &self.currency {
            put("currency", Llsd::String(value.clone()));
        }
        if let Some(value) = self.say_range {
            put("say-range", Llsd::Integer(value));
        }
        if let Some(value) = self.shout_range {
            put("shout-range", Llsd::Integer(value));
        }
        if let Some(value) = self.whisper_range {
            put("whisper-range", Llsd::Integer(value));
        }
        if let Some(value) = self.min_prim_scale {
            put("MinPrimScale", Llsd::Real(f64::from(value)));
        }
        if let Some(value) = self.max_prim_scale {
            put("MaxPrimScale", Llsd::Real(f64::from(value)));
        }
        if let Some(value) = self.max_phys_prim_scale {
            put("MaxPhysPrimScale", Llsd::Real(f64::from(value)));
        }
        Llsd::Map(map)
    }
}

// ---------------------------------------------------------------------------
// Client side — the reply parser.
// ---------------------------------------------------------------------------

/// Decodes a `SimulatorFeatures` GET reply into a [`SimulatorFeatures`]. Every
/// field is lenient: a grid advertises only the subset its configuration
/// enables, so absent keys take their defaults and the `OpenSimExtras` subtree
/// decodes to [`None`] when omitted (as it is on Second Life).
///
/// # Errors
/// Returns [`WireError::MalformedField`] if a decoded LLSD field is present but
/// of the wrong kind.
pub fn parse_simulator_features(body: &Llsd) -> Result<SimulatorFeatures, WireError> {
    let physics_shape_types = match body.get("PhysicsShapeTypes") {
        None | Some(Llsd::Undef) => None,
        Some(map @ Llsd::Map(_)) => Some(PhysicsShapeTypes {
            convex: map_bool(map, "convex")?,
            none: map_bool(map, "none")?,
            prim: map_bool(map, "prim")?,
        }),
        Some(other) => {
            return Err(WireError::MalformedField {
                field: "PhysicsShapeTypes",
                value: other.kind().to_owned(),
            });
        }
    };
    let animated_objects = match body.get("AnimatedObjects") {
        None | Some(Llsd::Undef) => None,
        Some(map @ Llsd::Map(_)) => Some(AnimatedObjects {
            max_tris: map_int(map, "AnimatedObjectMaxTris")?,
            max_agent_attachments: map_int(map, "MaxAgentAnimatedObjectAttachments")?,
        }),
        Some(other) => {
            return Err(WireError::MalformedField {
                field: "AnimatedObjects",
                value: other.kind().to_owned(),
            });
        }
    };
    Ok(SimulatorFeatures {
        mesh_rez_enabled: body.field_bool("MeshRezEnabled", "MeshRezEnabled")?,
        mesh_upload_enabled: body.field_bool("MeshUploadEnabled", "MeshUploadEnabled")?,
        mesh_xfer_enabled: body.field_bool("MeshXferEnabled", "MeshXferEnabled")?,
        bakes_on_mesh_enabled: body.field_bool("BakesOnMeshEnabled", "BakesOnMeshEnabled")?,
        physics_materials_enabled: body
            .field_bool("PhysicsMaterialsEnabled", "PhysicsMaterialsEnabled")?,
        physics_shape_types,
        animated_objects,
        max_agent_attachments: body.field_i32("MaxAgentAttachments", "MaxAgentAttachments")?,
        max_agent_groups_basic: body.field_i32("MaxAgentGroupsBasic", "MaxAgentGroupsBasic")?,
        max_agent_groups_premium: body
            .field_i32("MaxAgentGroupsPremium", "MaxAgentGroupsPremium")?,
        max_texture_resolution: body.field_i32("MaxTextureResolution", "MaxTextureResolution")?,
        pbr_terrain_enabled: body.field_bool("PBRTerrainEnabled", "PBRTerrainEnabled")?,
        gltf_enabled: body.field_bool("GLTFEnabled", "GLTFEnabled")?,
        lsl_syntax_id: body.field_uuid("LSLSyntaxId", "LSLSyntaxId")?,
        open_sim_extras: match body.get("OpenSimExtras") {
            None | Some(Llsd::Undef) => None,
            Some(map @ Llsd::Map(_)) => Some(OpenSimExtras::from_llsd(map)?),
            Some(other) => {
                return Err(WireError::MalformedField {
                    field: "OpenSimExtras",
                    value: other.kind().to_owned(),
                });
            }
        },
    })
}

// ---------------------------------------------------------------------------
// Server side — the inverse: the reply builder.
// ---------------------------------------------------------------------------

/// Builds a `SimulatorFeatures` GET reply from a [`SimulatorFeatures`] — the
/// inverse of [`parse_simulator_features`]. The `OpenSimExtras` map is emitted
/// only when [`open_sim_extras`](SimulatorFeatures::open_sim_extras) is present
/// (a Second Life-style reply leaves it [`None`]). Built on
/// [`Llsd::to_llsd_xml`], so it round-trips through
/// [`parse_simulator_features`].
#[must_use]
pub fn build_simulator_features_response(features: &SimulatorFeatures) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let mut put = |key: &str, value: Llsd| {
        let _previous = map.insert(key.to_owned(), value);
    };
    if let Some(value) = features.mesh_rez_enabled {
        put("MeshRezEnabled", Llsd::Boolean(value));
    }
    if let Some(value) = features.mesh_upload_enabled {
        put("MeshUploadEnabled", Llsd::Boolean(value));
    }
    if let Some(value) = features.mesh_xfer_enabled {
        put("MeshXferEnabled", Llsd::Boolean(value));
    }
    if let Some(value) = features.bakes_on_mesh_enabled {
        put("BakesOnMeshEnabled", Llsd::Boolean(value));
    }
    if let Some(value) = features.physics_materials_enabled {
        put("PhysicsMaterialsEnabled", Llsd::Boolean(value));
    }
    if let Some(shapes) = &features.physics_shape_types {
        put(
            "PhysicsShapeTypes",
            Llsd::Map(HashMap::from([
                ("convex".to_owned(), Llsd::Boolean(shapes.convex)),
                ("none".to_owned(), Llsd::Boolean(shapes.none)),
                ("prim".to_owned(), Llsd::Boolean(shapes.prim)),
            ])),
        );
    }
    if let Some(animated) = &features.animated_objects {
        put(
            "AnimatedObjects",
            Llsd::Map(HashMap::from([
                (
                    "AnimatedObjectMaxTris".to_owned(),
                    Llsd::Integer(animated.max_tris),
                ),
                (
                    "MaxAgentAnimatedObjectAttachments".to_owned(),
                    Llsd::Integer(animated.max_agent_attachments),
                ),
            ])),
        );
    }
    if let Some(value) = features.max_agent_attachments {
        put("MaxAgentAttachments", Llsd::Integer(value));
    }
    if let Some(value) = features.max_agent_groups_basic {
        put("MaxAgentGroupsBasic", Llsd::Integer(value));
    }
    if let Some(value) = features.max_agent_groups_premium {
        put("MaxAgentGroupsPremium", Llsd::Integer(value));
    }
    if let Some(value) = features.max_texture_resolution {
        put("MaxTextureResolution", Llsd::Integer(value));
    }
    if let Some(value) = features.pbr_terrain_enabled {
        put("PBRTerrainEnabled", Llsd::Boolean(value));
    }
    if let Some(value) = features.gltf_enabled {
        put("GLTFEnabled", Llsd::Boolean(value));
    }
    if let Some(value) = features.lsl_syntax_id {
        put("LSLSyntaxId", Llsd::Uuid(value));
    }
    if let Some(extras) = &features.open_sim_extras {
        put("OpenSimExtras", extras.to_llsd());
    }
    Llsd::Map(map).to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        AnimatedObjects, OpenSimExtras, PhysicsShapeTypes, SimulatorFeatures,
        build_simulator_features_response, parse_simulator_features,
    };
    use crate::llsd::parse_llsd_xml;

    /// A Second Life-style reply (PBR/GLTF on, no `OpenSimExtras`) decodes its
    /// flags, the nested physics-shape and animated-object maps, and leaves the
    /// grid extras absent.
    #[test]
    fn second_life_reply_decodes() -> Result<(), String> {
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>MeshUploadEnabled</key><boolean>true</boolean>",
            "<key>PBRTerrainEnabled</key><boolean>true</boolean>",
            "<key>GLTFEnabled</key><boolean>true</boolean>",
            "<key>MaxAgentAttachments</key><integer>38</integer>",
            "<key>MaxTextureResolution</key><integer>2048</integer>",
            "<key>PhysicsShapeTypes</key><map>",
            "<key>convex</key><boolean>true</boolean>",
            "<key>none</key><boolean>true</boolean>",
            "<key>prim</key><boolean>true</boolean></map>",
            "<key>AnimatedObjects</key><map>",
            "<key>AnimatedObjectMaxTris</key><integer>150000</integer>",
            "<key>MaxAgentAnimatedObjectAttachments</key><integer>2</integer></map>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let features = parse_simulator_features(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(features.mesh_upload_enabled, Some(true));
        assert_eq!(features.pbr_terrain_enabled, Some(true));
        assert_eq!(features.gltf_enabled, Some(true));
        // A flag the reply omits stays `None` (not advertised), distinct from
        // `Some(false)`.
        assert_eq!(features.mesh_rez_enabled, None);
        assert_eq!(features.max_agent_attachments, Some(38));
        assert_eq!(features.max_texture_resolution, Some(2048));
        let shapes = features.physics_shape_types.ok_or("expected shape types")?;
        assert!(shapes.prim);
        let animated = features
            .animated_objects
            .ok_or("expected animated objects")?;
        assert_eq!(animated.max_tris, 150_000);
        assert_eq!(animated.max_agent_attachments, 2);
        assert_eq!(features.open_sim_extras, None);
        Ok(())
    }

    /// A reply carrying the OpenSim grid extras round-trips through the server
    /// builder and the client parser, preserving the nested subtree.
    #[test]
    fn open_sim_features_round_trip() -> Result<(), String> {
        let features = SimulatorFeatures {
            mesh_rez_enabled: Some(true),
            mesh_upload_enabled: Some(true),
            mesh_xfer_enabled: Some(true),
            bakes_on_mesh_enabled: Some(true),
            physics_materials_enabled: Some(true),
            physics_shape_types: Some(PhysicsShapeTypes {
                convex: true,
                none: true,
                prim: false,
            }),
            animated_objects: Some(AnimatedObjects {
                max_tris: 50_000,
                max_agent_attachments: 1,
            }),
            max_agent_attachments: Some(38),
            max_agent_groups_basic: Some(42),
            max_agent_groups_premium: Some(60),
            max_texture_resolution: Some(1024),
            pbr_terrain_enabled: Some(false),
            gltf_enabled: Some(false),
            lsl_syntax_id: Some(
                Uuid::parse_str("11111111-1111-1111-1111-111111111111")
                    .map_err(|error| error.to_string())?,
            ),
            open_sim_extras: Some(OpenSimExtras {
                export_supported: Some(true),
                map_server_url: Some("http://maps.example/".to_owned()),
                search_server_url: Some("http://search.example/".to_owned()),
                destination_guide_url: Some("http://guide.example/".to_owned()),
                avatar_picker_url: Some("http://picker.example/".to_owned()),
                grid_url: Some("http://grid.example/".to_owned()),
                currency: Some("OS$".to_owned()),
                say_range: Some(20),
                shout_range: Some(100),
                whisper_range: Some(10),
                min_prim_scale: Some(0.01),
                max_prim_scale: Some(64.0),
                max_phys_prim_scale: Some(10.0),
            }),
        };
        let xml = build_simulator_features_response(&features);
        let parsed =
            parse_simulator_features(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, features);
        Ok(())
    }
}
