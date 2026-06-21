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
//! The capability is a single GET returning an LLSD map; absent keys take their
//! defaults (different grids advertise different subsets — Second Life omits the
//! `OpenSimExtras` subtree, OpenSim omits the Second Life PBR switches).

use std::collections::HashMap;

use uuid::Uuid;

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
/// configuration enables, so every field is lenient (absent → default).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpenSimExtras {
    /// Whether the grid permits the "export" creator permission (`ExportSupported`).
    pub export_supported: bool,
    /// The grid's map-tile server base URL (`map-server-url`).
    pub map_server_url: String,
    /// The grid's web search endpoint (`search-server-url`).
    pub search_server_url: String,
    /// The grid's destination-guide URL (`destination-guide-url`).
    pub destination_guide_url: String,
    /// The grid's avatar-picker URL (`avatar-picker-url`).
    pub avatar_picker_url: String,
    /// The grid's HyperGrid URL prefix (`GridURL`).
    pub grid_url: String,
    /// The currency symbol the grid displays (`currency`), e.g. `"OS$"`.
    pub currency: String,
    /// The `llSay`/normal chat range in metres (`say-range`, default 20).
    pub say_range: i32,
    /// The `llShout` range in metres (`shout-range`, default 100).
    pub shout_range: i32,
    /// The `llWhisper` range in metres (`whisper-range`, default 10).
    pub whisper_range: i32,
    /// The smallest prim dimension the grid allows (`MinPrimScale`).
    pub min_prim_scale: f32,
    /// The largest prim dimension the grid allows (`MaxPrimScale`).
    pub max_prim_scale: f32,
    /// The largest physical-prim dimension the grid allows (`MaxPhysPrimScale`).
    pub max_phys_prim_scale: f32,
}

/// The decoded `SimulatorFeatures` reply: the region's feature flags and limits.
///
/// Every field is lenient — a grid advertises only the subset its configuration
/// enables, so absent keys decode to their defaults. The OpenSim-only grid
/// extras live in [`open_sim_extras`](Self::open_sim_extras), which is [`None`]
/// on Second Life (and any grid omitting the `OpenSimExtras` map).
#[derive(Debug, Clone, PartialEq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the SimulatorFeatures LLSD reply, which is a flat flag map"
)]
pub struct SimulatorFeatures {
    /// Whether rezzing mesh objects is permitted (`MeshRezEnabled`).
    pub mesh_rez_enabled: bool,
    /// Whether uploading mesh assets is permitted (`MeshUploadEnabled`).
    pub mesh_upload_enabled: bool,
    /// Whether the legacy mesh-xfer path is enabled (`MeshXferEnabled`, OpenSim).
    pub mesh_xfer_enabled: bool,
    /// Whether bakes-on-mesh is supported (`BakesOnMeshEnabled`).
    pub bakes_on_mesh_enabled: bool,
    /// Whether per-prim physics materials are supported (`PhysicsMaterialsEnabled`).
    pub physics_materials_enabled: bool,
    /// The accepted physics-shape types (`PhysicsShapeTypes`).
    pub physics_shape_types: PhysicsShapeTypes,
    /// Animated-object (animesh) limits (`AnimatedObjects`).
    pub animated_objects: AnimatedObjects,
    /// The maximum number of attachments an agent may wear (`MaxAgentAttachments`).
    pub max_agent_attachments: i32,
    /// The free-account group cap (`MaxAgentGroupsBasic`, OpenSim).
    pub max_agent_groups_basic: i32,
    /// The premium-account group cap (`MaxAgentGroupsPremium`, OpenSim).
    pub max_agent_groups_premium: i32,
    /// The maximum texture dimension the simulator serves (`MaxTextureResolution`).
    pub max_texture_resolution: i32,
    /// Whether PBR (GLTF) terrain is enabled (`PBRTerrainEnabled`, Second Life).
    pub pbr_terrain_enabled: bool,
    /// Whether GLTF scene objects are enabled (`GLTFEnabled`, Second Life).
    pub gltf_enabled: bool,
    /// The asset id of the LSL syntax definition for this simulator (`LSLSyntaxId`).
    pub lsl_syntax_id: Uuid,
    /// The OpenSim-only grid extras, or [`None`] on grids omitting them.
    pub open_sim_extras: Option<OpenSimExtras>,
}

/// Reads a boolean field from an LLSD map, defaulting to `false` when absent.
fn map_bool(map: &Llsd, key: &str) -> bool {
    map.get(key).and_then(Llsd::as_bool).unwrap_or(false)
}

/// Reads an integer field from an LLSD map, defaulting to `0` when absent.
fn map_int(map: &Llsd, key: &str) -> i32 {
    map.get(key).and_then(Llsd::as_i32).unwrap_or(0)
}

/// Reads a real field from an LLSD map as `f32`, defaulting to `0.0` when absent.
fn map_f32(map: &Llsd, key: &str) -> f32 {
    map.get(key).and_then(Llsd::as_f32).unwrap_or(0.0)
}

/// Reads a string field from an LLSD map, defaulting to empty when absent.
fn map_string(map: &Llsd, key: &str) -> String {
    map.get(key)
        .and_then(Llsd::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl OpenSimExtras {
    /// Decodes the `OpenSimExtras` map. Absent keys take their defaults.
    #[must_use]
    fn from_llsd(map: &Llsd) -> Self {
        Self {
            export_supported: map_bool(map, "ExportSupported"),
            map_server_url: map_string(map, "map-server-url"),
            search_server_url: map_string(map, "search-server-url"),
            destination_guide_url: map_string(map, "destination-guide-url"),
            avatar_picker_url: map_string(map, "avatar-picker-url"),
            grid_url: map_string(map, "GridURL"),
            currency: map_string(map, "currency"),
            say_range: map_int(map, "say-range"),
            shout_range: map_int(map, "shout-range"),
            whisper_range: map_int(map, "whisper-range"),
            min_prim_scale: map_f32(map, "MinPrimScale"),
            max_prim_scale: map_f32(map, "MaxPrimScale"),
            max_phys_prim_scale: map_f32(map, "MaxPhysPrimScale"),
        }
    }

    /// Encodes this subtree as an `OpenSimExtras` LLSD map — the inverse of
    /// [`from_llsd`](Self::from_llsd).
    #[must_use]
    fn to_llsd(&self) -> Llsd {
        Llsd::Map(HashMap::from([
            (
                "ExportSupported".to_owned(),
                Llsd::Boolean(self.export_supported),
            ),
            (
                "map-server-url".to_owned(),
                Llsd::String(self.map_server_url.clone()),
            ),
            (
                "search-server-url".to_owned(),
                Llsd::String(self.search_server_url.clone()),
            ),
            (
                "destination-guide-url".to_owned(),
                Llsd::String(self.destination_guide_url.clone()),
            ),
            (
                "avatar-picker-url".to_owned(),
                Llsd::String(self.avatar_picker_url.clone()),
            ),
            ("GridURL".to_owned(), Llsd::String(self.grid_url.clone())),
            ("currency".to_owned(), Llsd::String(self.currency.clone())),
            ("say-range".to_owned(), Llsd::Integer(self.say_range)),
            ("shout-range".to_owned(), Llsd::Integer(self.shout_range)),
            (
                "whisper-range".to_owned(),
                Llsd::Integer(self.whisper_range),
            ),
            (
                "MinPrimScale".to_owned(),
                Llsd::Real(f64::from(self.min_prim_scale)),
            ),
            (
                "MaxPrimScale".to_owned(),
                Llsd::Real(f64::from(self.max_prim_scale)),
            ),
            (
                "MaxPhysPrimScale".to_owned(),
                Llsd::Real(f64::from(self.max_phys_prim_scale)),
            ),
        ]))
    }
}

// ---------------------------------------------------------------------------
// Client side — the reply parser.
// ---------------------------------------------------------------------------

/// Decodes a `SimulatorFeatures` GET reply into a [`SimulatorFeatures`]. Every
/// field is lenient: a grid advertises only the subset its configuration
/// enables, so absent keys take their defaults and the `OpenSimExtras` subtree
/// decodes to [`None`] when omitted (as it is on Second Life).
#[must_use]
pub fn parse_simulator_features(body: &Llsd) -> SimulatorFeatures {
    let physics_shape_types = body
        .get("PhysicsShapeTypes")
        .map(|map| PhysicsShapeTypes {
            convex: map_bool(map, "convex"),
            none: map_bool(map, "none"),
            prim: map_bool(map, "prim"),
        })
        .unwrap_or_default();
    let animated_objects = body
        .get("AnimatedObjects")
        .map(|map| AnimatedObjects {
            max_tris: map_int(map, "AnimatedObjectMaxTris"),
            max_agent_attachments: map_int(map, "MaxAgentAnimatedObjectAttachments"),
        })
        .unwrap_or_default();
    SimulatorFeatures {
        mesh_rez_enabled: map_bool(body, "MeshRezEnabled"),
        mesh_upload_enabled: map_bool(body, "MeshUploadEnabled"),
        mesh_xfer_enabled: map_bool(body, "MeshXferEnabled"),
        bakes_on_mesh_enabled: map_bool(body, "BakesOnMeshEnabled"),
        physics_materials_enabled: map_bool(body, "PhysicsMaterialsEnabled"),
        physics_shape_types,
        animated_objects,
        max_agent_attachments: map_int(body, "MaxAgentAttachments"),
        max_agent_groups_basic: map_int(body, "MaxAgentGroupsBasic"),
        max_agent_groups_premium: map_int(body, "MaxAgentGroupsPremium"),
        max_texture_resolution: map_int(body, "MaxTextureResolution"),
        pbr_terrain_enabled: map_bool(body, "PBRTerrainEnabled"),
        gltf_enabled: map_bool(body, "GLTFEnabled"),
        lsl_syntax_id: body
            .get("LSLSyntaxId")
            .and_then(Llsd::as_uuid)
            .unwrap_or_else(Uuid::nil),
        open_sim_extras: body.get("OpenSimExtras").map(OpenSimExtras::from_llsd),
    }
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
    put("MeshRezEnabled", Llsd::Boolean(features.mesh_rez_enabled));
    put(
        "MeshUploadEnabled",
        Llsd::Boolean(features.mesh_upload_enabled),
    );
    put("MeshXferEnabled", Llsd::Boolean(features.mesh_xfer_enabled));
    put(
        "BakesOnMeshEnabled",
        Llsd::Boolean(features.bakes_on_mesh_enabled),
    );
    put(
        "PhysicsMaterialsEnabled",
        Llsd::Boolean(features.physics_materials_enabled),
    );
    put(
        "PhysicsShapeTypes",
        Llsd::Map(HashMap::from([
            (
                "convex".to_owned(),
                Llsd::Boolean(features.physics_shape_types.convex),
            ),
            (
                "none".to_owned(),
                Llsd::Boolean(features.physics_shape_types.none),
            ),
            (
                "prim".to_owned(),
                Llsd::Boolean(features.physics_shape_types.prim),
            ),
        ])),
    );
    put(
        "AnimatedObjects",
        Llsd::Map(HashMap::from([
            (
                "AnimatedObjectMaxTris".to_owned(),
                Llsd::Integer(features.animated_objects.max_tris),
            ),
            (
                "MaxAgentAnimatedObjectAttachments".to_owned(),
                Llsd::Integer(features.animated_objects.max_agent_attachments),
            ),
        ])),
    );
    put(
        "MaxAgentAttachments",
        Llsd::Integer(features.max_agent_attachments),
    );
    put(
        "MaxAgentGroupsBasic",
        Llsd::Integer(features.max_agent_groups_basic),
    );
    put(
        "MaxAgentGroupsPremium",
        Llsd::Integer(features.max_agent_groups_premium),
    );
    put(
        "MaxTextureResolution",
        Llsd::Integer(features.max_texture_resolution),
    );
    put(
        "PBRTerrainEnabled",
        Llsd::Boolean(features.pbr_terrain_enabled),
    );
    put("GLTFEnabled", Llsd::Boolean(features.gltf_enabled));
    put("LSLSyntaxId", Llsd::Uuid(features.lsl_syntax_id));
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
        let features = parse_simulator_features(&body);
        assert!(features.mesh_upload_enabled);
        assert!(features.pbr_terrain_enabled);
        assert!(features.gltf_enabled);
        assert_eq!(features.max_agent_attachments, 38);
        assert_eq!(features.max_texture_resolution, 2048);
        assert!(features.physics_shape_types.prim);
        assert_eq!(features.animated_objects.max_tris, 150_000);
        assert_eq!(features.animated_objects.max_agent_attachments, 2);
        assert_eq!(features.open_sim_extras, None);
        Ok(())
    }

    /// A reply carrying the OpenSim grid extras round-trips through the server
    /// builder and the client parser, preserving the nested subtree.
    #[test]
    fn open_sim_features_round_trip() -> Result<(), String> {
        let features = SimulatorFeatures {
            mesh_rez_enabled: true,
            mesh_upload_enabled: true,
            mesh_xfer_enabled: true,
            bakes_on_mesh_enabled: true,
            physics_materials_enabled: true,
            physics_shape_types: PhysicsShapeTypes {
                convex: true,
                none: true,
                prim: false,
            },
            animated_objects: AnimatedObjects {
                max_tris: 50_000,
                max_agent_attachments: 1,
            },
            max_agent_attachments: 38,
            max_agent_groups_basic: 42,
            max_agent_groups_premium: 60,
            max_texture_resolution: 1024,
            pbr_terrain_enabled: false,
            gltf_enabled: false,
            lsl_syntax_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111")
                .map_err(|error| error.to_string())?,
            open_sim_extras: Some(OpenSimExtras {
                export_supported: true,
                map_server_url: "http://maps.example/".to_owned(),
                search_server_url: "http://search.example/".to_owned(),
                destination_guide_url: "http://guide.example/".to_owned(),
                avatar_picker_url: "http://picker.example/".to_owned(),
                grid_url: "http://grid.example/".to_owned(),
                currency: "OS$".to_owned(),
                say_range: 20,
                shout_range: 100,
                whisper_range: 10,
                min_prim_scale: 0.01,
                max_prim_scale: 64.0,
                max_phys_prim_scale: 10.0,
            }),
        };
        let xml = build_simulator_features_response(&features);
        let parsed =
            parse_simulator_features(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?);
        assert_eq!(parsed, features);
        Ok(())
    }
}
