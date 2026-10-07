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

use std::collections::{BTreeMap, HashMap};

use uuid::Uuid;

use crate::WireError;
use crate::llsd::{Llsd, LlsdError};

/// Which collision-shape types the simulator accepts for a prim's physics shape
/// (`PhysicsShapeTypes`). The viewer enables the corresponding entries in the
/// build tool's physics-shape dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimatedObjects {
    /// The maximum triangle count of a single animated object
    /// (`AnimatedObjectMaxTris`).
    pub max_tris: i32,
    /// The maximum number of animated objects an agent may attach
    /// (`MaxAgentAnimatedObjectAttachments`).
    pub max_agent_attachments: i32,
}

/// The menu entries a region asks the viewer to add (`menus`), by the menu they
/// go under.
///
/// OpenSim's `DynamicMenuModule` adds this map to every reply — with all five
/// menus empty on a stock region, since entries only come from a script
/// calling `osAddDynamicMenu`-style module commands. Each entry is its title,
/// which the module sends as both the key and the value. Second Life sends no
/// such key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DynamicMenus {
    /// Entries for the avatar menu (`agent`).
    pub agent: Vec<String>,
    /// Entries for the World menu (`world`).
    pub world: Vec<String>,
    /// Entries for the Build / Tools menu (`tools`).
    pub tools: Vec<String>,
    /// Entries for the Advanced menu (`advanced`).
    pub advanced: Vec<String>,
    /// Entries for the Admin menu (`admin`).
    pub admin: Vec<String>,
}

impl DynamicMenus {
    /// Decodes the `menus` map. A menu the map omits has no entries.
    fn from_llsd(map: &Llsd) -> Self {
        let titles = |menu: &str| -> Vec<String> {
            let mut titles: Vec<String> = match map.get(menu) {
                Some(Llsd::Map(entries)) => entries.keys().cloned().collect(),
                _ => Vec::new(),
            };
            titles.sort();
            titles
        };
        Self {
            agent: titles("agent"),
            world: titles("world"),
            tools: titles("tools"),
            advanced: titles("advanced"),
            admin: titles("admin"),
        }
    }

    /// Encodes the `menus` map — all five menus, each entry keyed and valued by
    /// its title, as `DynamicMenuModule` sends it.
    fn to_llsd(&self) -> Llsd {
        let menu = |titles: &[String]| -> Llsd {
            Llsd::Map(
                titles
                    .iter()
                    .map(|title| (title.clone(), Llsd::String(title.clone())))
                    .collect(),
            )
        };
        Llsd::Map(HashMap::from([
            ("agent".to_owned(), menu(&self.agent)),
            ("world".to_owned(), menu(&self.world)),
            ("tools".to_owned(), menu(&self.tools)),
            ("advanced".to_owned(), menu(&self.advanced)),
            ("admin".to_owned(), menu(&self.admin)),
        ]))
    }
}

/// The OpenSim-specific `OpenSimExtras` subtree of a `SimulatorFeatures` reply.
/// Second Life omits this map entirely; OpenSim grids fill in the subset their
/// configuration enables. Every field is an [`Option`]: [`None`] means the grid
/// did **not** advertise the key (so the caller applies its own default — e.g.
/// the 20/100/10 m chat ranges), distinct from a value the grid did send.
///
/// The fields are every key a stock OpenSim 0.9.3 standalone was measured
/// sending (`book/src/gridspec/region-arrival.md`), plus the grid-wide URLs a
/// configured grid adds; anything else rides in [`other`](Self::other).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OpenSimExtras {
    /// Whether the grid permits the "export" creator permission
    /// (`ExportSupported`). OpenSim sends it as the **string** `"true"` /
    /// `"false"`, which is how it is written back; a boolean decodes too.
    pub export_supported: Option<bool>,
    /// Whether the region accepts animation-set assets (`AnimationSet`).
    pub animation_set: Option<bool>,
    /// Whether the region accepts custom avatar skeletons (`AvatarSkeleton`).
    pub avatar_skeleton: Option<bool>,
    /// The grid's map-tile server base URL (`map-server-url`).
    pub map_server_url: Option<url::Url>,
    /// The grid's web search endpoint (`search-server-url`).
    pub search_server_url: Option<url::Url>,
    /// The grid's destination-guide URL (`destination-guide-url`).
    pub destination_guide_url: Option<url::Url>,
    /// The grid's avatar-picker URL (`avatar-picker-url`).
    pub avatar_picker_url: Option<url::Url>,
    /// The grid's HyperGrid URL prefix (`GridURL`).
    pub grid_url: Option<url::Url>,
    /// The other addresses the grid answers to, comma-separated
    /// (`GridURLAlias`). A grid with none sends the key with an empty string,
    /// which is `Some("")` here, not [`None`].
    pub grid_url_alias: Option<String>,
    /// The grid's display name (`GridName`).
    pub grid_name: Option<String>,
    /// The grid's short name (`GridNick`).
    pub grid_nick: Option<String>,
    /// The currency symbol the grid displays (`currency`), e.g. `"OS$"`.
    pub currency: Option<String>,
    /// The economy helper base URI (`currency-base-uri`): where the viewer's
    /// buy-L$ / buy-land flows POST their XML-RPC helper calls
    /// (the `economy_helper` builders such as [`build_currency_quote_request`](crate::build_currency_quote_request)).
    pub currency_base_uri: Option<url::Url>,
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
    /// The smallest physical-prim dimension the grid allows
    /// (`MinPhysPrimScale`).
    pub min_phys_prim_scale: Option<f32>,
    /// The largest physical-prim dimension the grid allows (`MaxPhysPrimScale`).
    pub max_phys_prim_scale: Option<f32>,
    /// The lowest altitude the region simulates, in metres (`MinSimHeight`).
    pub min_sim_height: Option<f32>,
    /// The highest altitude the region simulates, in metres (`MaxSimHeight`).
    pub max_sim_height: Option<f32>,
    /// The lowest terrain height the region stores, in metres (`MinHeightmap`).
    pub min_heightmap: Option<f32>,
    /// The highest terrain height the region stores, in metres
    /// (`MaxHeightmap`).
    pub max_heightmap: Option<f32>,
    /// The simulator's real frame rate (`SimulatorFPS`): the reciprocal of its
    /// frame time, 11 on a stock region.
    pub simulator_fps: Option<f32>,
    /// What the region multiplies its real frame rate by before reporting it in
    /// `SimStats` (`SimulatorFPSFactor`), so that a healthy region reads as
    /// the 55 a viewer expects.
    pub simulator_fps_factor: Option<f32>,
    /// The percentage of the nominal frame rate below which a viewer should
    /// show a warning (`SimulatorFPSWarnPercent`).
    pub simulator_fps_warn_percent: Option<i32>,
    /// The percentage of the nominal frame rate below which a viewer should
    /// show the rate as critical (`SimulatorFPSCritPercent`).
    pub simulator_fps_crit_percent: Option<i32>,
    /// Every key of the subtree the fields above do not carry: a key this
    /// decoder has no field for, or a URL key the grid sent empty. Kept so a
    /// caller can see — and a server can send — a subtree this type does not
    /// fully describe.
    pub other: BTreeMap<String, Llsd>,
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
///
/// The fields are every key Second Life (on aditi) and a stock OpenSim were
/// measured sending (`book/src/gridspec/region-arrival.md`); a key neither
/// sent then rides in [`other`](Self::other), and
/// [`advertised`](Self::advertised) lists the whole reply.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SimulatorFeatures {
    /// Whether rezzing mesh objects is permitted (`MeshRezEnabled`).
    pub mesh_rez_enabled: Option<bool>,
    /// Whether uploading mesh assets is permitted (`MeshUploadEnabled`).
    pub mesh_upload_enabled: Option<bool>,
    /// Whether the legacy mesh-xfer path is enabled (`MeshXferEnabled`).
    pub mesh_xfer_enabled: Option<bool>,
    /// Whether bakes-on-mesh is supported (`BakesOnMeshEnabled`).
    pub bakes_on_mesh_enabled: Option<bool>,
    /// Whether the region stores the agent's hover height
    /// (`AvatarHoverHeightEnabled`).
    pub avatar_hover_height_enabled: Option<bool>,
    /// Whether per-prim physics materials are supported (`PhysicsMaterialsEnabled`).
    pub physics_materials_enabled: Option<bool>,
    /// The accepted physics-shape types (`PhysicsShapeTypes`); [`None`] when the
    /// grid did not advertise the map.
    pub physics_shape_types: Option<PhysicsShapeTypes>,
    /// Animated-object (animesh) limits (`AnimatedObjects`); [`None`] when the
    /// grid did not advertise the map.
    pub animated_objects: Option<AnimatedObjects>,
    /// Whether the region's navmesh follows objects that move
    /// (`DynamicPathfindingEnabled`, Second Life).
    pub dynamic_pathfinding_enabled: Option<bool>,
    /// The maximum number of attachments an agent may wear (`MaxAgentAttachments`).
    pub max_agent_attachments: Option<i32>,
    /// The group cap of an account of no particular level (`MaxAgentGroups`,
    /// Second Life).
    pub max_agent_groups: Option<i32>,
    /// The free-account group cap (`MaxAgentGroupsBasic`).
    pub max_agent_groups_basic: Option<i32>,
    /// The premium-account group cap (`MaxAgentGroupsPremium`).
    pub max_agent_groups_premium: Option<i32>,
    /// How many agents and groups each estate access list holds
    /// (`MaxEstateAccessIds`, Second Life).
    pub max_estate_access_ids: Option<i32>,
    /// How many managers an estate may have (`MaxEstateManagers`, Second
    /// Life).
    pub max_estate_managers: Option<i32>,
    /// How many materials one `RenderMaterials` request may carry
    /// (`MaxMaterialsPerTransaction`).
    pub max_materials_per_transaction: Option<i32>,
    /// The maximum texture dimension the simulator serves
    /// (`MaxTextureResolution`, Second Life).
    pub max_texture_resolution: Option<i32>,
    /// How many `RenderMaterials` requests a second the region accepts
    /// (`RenderMaterialsCapability`) — a rate, despite the name, which both
    /// grids send as a **real**: 4 on Second Life, 3 on OpenSim. The reference
    /// viewer spaces its requests by it (`resetMaterialsCapThrottle`,
    /// `llviewerregion.cpp`) and assumes 1 when the key is absent or not a
    /// real.
    pub render_materials_capability: Option<f32>,
    /// Whether PBR (GLTF) terrain is enabled (`PBRTerrainEnabled`, Second Life).
    pub pbr_terrain_enabled: Option<bool>,
    /// Whether PBR terrain materials take texture transforms
    /// (`PBRTerrainTransformsEnabled`, Second Life).
    pub pbr_terrain_transforms_enabled: Option<bool>,
    /// Whether a PBR material may be picked from a swatch in the build tool
    /// (`PBRMaterialSwatchEnabled`, Second Life).
    pub pbr_material_swatch_enabled: Option<bool>,
    /// Whether GLTF scene objects are enabled (`GLTFEnabled`, Second Life).
    pub gltf_enabled: Option<bool>,
    /// Whether the region renders mirror probes (`MirrorsEnabled`, Second
    /// Life).
    pub mirrors_enabled: Option<bool>,
    /// Whether the region honours the no-modify bypass for texture and
    /// material overrides (`NoModBypassSupport`, Second Life).
    pub no_mod_bypass_support: Option<bool>,
    /// Whether the region runs Lua (SLua) scripts (`LuaScriptsEnabled`, Second
    /// Life) — a per-region flag: two aditi regions answered differently.
    pub lua_scripts_enabled: Option<bool>,
    /// The asset id of the LSL syntax definition for this simulator (`LSLSyntaxId`).
    pub lsl_syntax_id: Option<Uuid>,
    /// The version of that syntax definition (`LSLSyntaxVersion`, Second
    /// Life), e.g. `"0.6.13"`.
    pub lsl_syntax_version: Option<String>,
    /// How far, in metres, the region lets an object's extrapolated position
    /// drift before it sends an update (`DeadReckoningDistance`, Second Life).
    pub dead_reckoning_distance: Option<f32>,
    /// How long, in seconds, the region lets a viewer extrapolate an object
    /// without an update (`DeadReckoningTime`, Second Life).
    pub dead_reckoning_time: Option<f32>,
    /// The simulator host's name (`HostName`, Second Life).
    pub host_name: Option<String>,
    /// The voice backend the region speaks (`VoiceServerType`: `"webrtc"` or
    /// `"vivox"`). The viewer selects its spatial-voice module from this field
    /// (Firestorm `llvoiceclient.cpp`, `LLVoiceClient::onSimulatorFeaturesReceived`);
    /// [`None`] when the region did not advertise one (older grids — the viewer
    /// then falls back to the login response's `voice-config`).
    pub voice_server_type: Option<String>,
    /// The menu entries the region adds to the viewer (`menus`, OpenSim).
    pub menus: Option<DynamicMenus>,
    /// The OpenSim-only grid extras, or [`None`] on grids omitting them.
    pub open_sim_extras: Option<OpenSimExtras>,
    /// Every top-level key the fields above do not carry: a key this decoder
    /// has no field for. Kept so a caller can see — and a server can send — a
    /// reply this type does not fully describe.
    pub other: BTreeMap<String, Llsd>,
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

/// Reads a string field from an LLSD map as an owned [`String`]; an absent key
/// is [`None`], an empty string is `Some("")`.
fn map_text(map: &Llsd, key: &'static str) -> Result<Option<String>, WireError> {
    Ok(map.field_str(key, key)?.map(str::to_owned))
}

/// Reads a nested map member: [`None`] when absent or undefined, a hard error
/// when it is present but not a map.
fn map_member<'a>(body: &'a Llsd, key: &'static str) -> Result<Option<&'a Llsd>, WireError> {
    match body.get(key) {
        None | Some(Llsd::Undef) => Ok(None),
        Some(map @ Llsd::Map(_)) => Ok(Some(map)),
        Some(other) => Err(LlsdError::MalformedField {
            field: key,
            value: other.kind().to_owned(),
        }
        .into()),
    }
}

/// The members of `body` that `typed` — the same map re-encoded from its typed
/// fields — does not carry: what a decoder has to keep aside so that nothing a
/// grid sent is dropped.
///
/// Deriving it from the encoder rather than from a list of known keys means the
/// two cannot disagree about what a typed field covers.
fn untyped_members(body: &Llsd, typed: &HashMap<String, Llsd>) -> BTreeMap<String, Llsd> {
    match body {
        Llsd::Map(members) => members
            .iter()
            .filter(|(key, _value)| !typed.contains_key(*key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        _ => BTreeMap::new(),
    }
}

/// Lists `value`'s members under `prefix` into `out`, by dotted path and LLSD
/// kind, descending into nested maps.
fn list_members(prefix: &str, value: &Llsd, out: &mut BTreeMap<String, &'static str>) {
    if let Llsd::Map(members) = value {
        for (key, member) in members {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            list_members(&path, member, out);
            let _previous = out.insert(path, member.kind());
        }
    }
}

/// Reads OpenSim's `ExportSupported` flag, which the reference simulator encodes
/// two different ways depending on where the value originates. The Second
/// Life-style path (`SimulatorFeaturesModule`, `extrasMap["ExportSupported"] =
/// true`) sends a boolean, but the grid-wide extra-features path
/// (`GetGridExtraFeatures`, `extrasMap[key] = val`) injects it as the **string**
/// `"true"`/`"false"` — every grid extra is stored as a string there, and the
/// `GridService` default for the key is the literal string `"true"`. The second
/// path runs after the first and overwrites it, so the string is what a stock
/// region was measured sending. Accept either encoding (a boolean or integer
/// via [`Llsd::field_bool`], else a case-insensitive `"true"`/`"false"` string,
/// matching OpenSim's own `bool.TryParse(val, …)`) so a real OpenSim reply
/// parses. An absent key decodes to [`None`]; a present value of any other kind
/// is a hard error.
fn map_export_supported(map: &Llsd) -> Result<Option<bool>, WireError> {
    match map.get("ExportSupported") {
        None | Some(Llsd::Undef) => Ok(None),
        Some(Llsd::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(WireError::Llsd(LlsdError::MalformedField {
                field: "ExportSupported",
                value: format!("string {text:?}"),
            })),
        },
        Some(_) => Ok(map.field_bool("ExportSupported", "ExportSupported")?),
    }
}

impl OpenSimExtras {
    /// Decodes the `OpenSimExtras` map. An absent key decodes to [`None`] (the
    /// grid did not advertise it); a present key of the wrong LLSD kind is a hard
    /// error.
    fn from_llsd(map: &Llsd) -> Result<Self, WireError> {
        // A URL key decodes through the codec boundary helper: absent or empty is
        // the "not advertised" sentinel (`None`), a non-empty but unparsable value
        // is a hard error.
        let url_field = |key: &'static str| -> Result<Option<url::Url>, WireError> {
            crate::optional_url_from_wire(key, map.field_str(key, key)?.unwrap_or(""))
        };
        let mut extras = Self {
            export_supported: map_export_supported(map)?,
            animation_set: map.field_bool("AnimationSet", "AnimationSet")?,
            avatar_skeleton: map.field_bool("AvatarSkeleton", "AvatarSkeleton")?,
            map_server_url: url_field("map-server-url")?,
            search_server_url: url_field("search-server-url")?,
            destination_guide_url: url_field("destination-guide-url")?,
            avatar_picker_url: url_field("avatar-picker-url")?,
            grid_url: url_field("GridURL")?,
            grid_url_alias: map_text(map, "GridURLAlias")?,
            grid_name: map_text(map, "GridName")?,
            grid_nick: map_text(map, "GridNick")?,
            currency: map_text(map, "currency")?,
            currency_base_uri: url_field("currency-base-uri")?,
            say_range: map.field_i32("say-range", "say-range")?,
            shout_range: map.field_i32("shout-range", "shout-range")?,
            whisper_range: map.field_i32("whisper-range", "whisper-range")?,
            min_prim_scale: map.field_f32("MinPrimScale", "MinPrimScale")?,
            max_prim_scale: map.field_f32("MaxPrimScale", "MaxPrimScale")?,
            min_phys_prim_scale: map.field_f32("MinPhysPrimScale", "MinPhysPrimScale")?,
            max_phys_prim_scale: map.field_f32("MaxPhysPrimScale", "MaxPhysPrimScale")?,
            min_sim_height: map.field_f32("MinSimHeight", "MinSimHeight")?,
            max_sim_height: map.field_f32("MaxSimHeight", "MaxSimHeight")?,
            min_heightmap: map.field_f32("MinHeightmap", "MinHeightmap")?,
            max_heightmap: map.field_f32("MaxHeightmap", "MaxHeightmap")?,
            simulator_fps: map.field_f32("SimulatorFPS", "SimulatorFPS")?,
            simulator_fps_factor: map.field_f32("SimulatorFPSFactor", "SimulatorFPSFactor")?,
            simulator_fps_warn_percent: map
                .field_i32("SimulatorFPSWarnPercent", "SimulatorFPSWarnPercent")?,
            simulator_fps_crit_percent: map
                .field_i32("SimulatorFPSCritPercent", "SimulatorFPSCritPercent")?,
            other: BTreeMap::new(),
        };
        extras.other = untyped_members(map, &extras.typed_members());
        Ok(extras)
    }

    /// The subtree's typed fields as LLSD map members: only advertised (`Some`)
    /// keys, each in the LLSD kind OpenSim sends it in.
    fn typed_members(&self) -> HashMap<String, Llsd> {
        let mut map: HashMap<String, Llsd> = HashMap::new();
        let mut put = |key: &str, value: Option<Llsd>| {
            if let Some(value) = value {
                let _previous = map.insert(key.to_owned(), value);
            }
        };
        let url = |value: Option<&url::Url>| value.map(|url| Llsd::String(crate::url_to_wire(url)));
        let text = |value: Option<&String>| value.cloned().map(Llsd::String);
        let real = |value: Option<f32>| value.map(|value| Llsd::Real(f64::from(value)));
        // The string, not a boolean: see `map_export_supported`.
        put(
            "ExportSupported",
            self.export_supported
                .map(|value| Llsd::String(value.to_string())),
        );
        put("AnimationSet", self.animation_set.map(Llsd::Boolean));
        put("AvatarSkeleton", self.avatar_skeleton.map(Llsd::Boolean));
        put("map-server-url", url(self.map_server_url.as_ref()));
        put("search-server-url", url(self.search_server_url.as_ref()));
        put(
            "destination-guide-url",
            url(self.destination_guide_url.as_ref()),
        );
        put("avatar-picker-url", url(self.avatar_picker_url.as_ref()));
        put("GridURL", url(self.grid_url.as_ref()));
        put("GridURLAlias", text(self.grid_url_alias.as_ref()));
        put("GridName", text(self.grid_name.as_ref()));
        put("GridNick", text(self.grid_nick.as_ref()));
        put("currency", text(self.currency.as_ref()));
        put("currency-base-uri", url(self.currency_base_uri.as_ref()));
        put("say-range", self.say_range.map(Llsd::Integer));
        put("shout-range", self.shout_range.map(Llsd::Integer));
        put("whisper-range", self.whisper_range.map(Llsd::Integer));
        put("MinPrimScale", real(self.min_prim_scale));
        put("MaxPrimScale", real(self.max_prim_scale));
        put("MinPhysPrimScale", real(self.min_phys_prim_scale));
        put("MaxPhysPrimScale", real(self.max_phys_prim_scale));
        put("MinSimHeight", real(self.min_sim_height));
        put("MaxSimHeight", real(self.max_sim_height));
        put("MinHeightmap", real(self.min_heightmap));
        put("MaxHeightmap", real(self.max_heightmap));
        put("SimulatorFPS", real(self.simulator_fps));
        put("SimulatorFPSFactor", real(self.simulator_fps_factor));
        put(
            "SimulatorFPSWarnPercent",
            self.simulator_fps_warn_percent.map(Llsd::Integer),
        );
        put(
            "SimulatorFPSCritPercent",
            self.simulator_fps_crit_percent.map(Llsd::Integer),
        );
        map
    }

    /// Encodes this subtree as an `OpenSimExtras` LLSD map — the inverse of
    /// [`from_llsd`](Self::from_llsd). Only advertised (`Some`) keys are emitted,
    /// so a round-trip preserves the advertised-vs-absent distinction; the
    /// [`other`](Self::other) keys follow, a typed field winning a clash.
    #[must_use]
    fn to_llsd(&self) -> Llsd {
        let mut map = self.typed_members();
        for (key, value) in &self.other {
            let _existing = map.entry(key.clone()).or_insert_with(|| value.clone());
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
/// decodes to [`None`] when omitted (as it is on Second Life). A key no field
/// carries is kept in [`SimulatorFeatures::other`].
///
/// # Errors
/// Returns [`LlsdError::MalformedField`] if a decoded LLSD field is present but
/// of the wrong kind.
pub fn parse_simulator_features(body: &Llsd) -> Result<SimulatorFeatures, WireError> {
    let physics_shape_types = match map_member(body, "PhysicsShapeTypes")? {
        Some(map) => Some(PhysicsShapeTypes {
            convex: map_bool(map, "convex")?,
            none: map_bool(map, "none")?,
            prim: map_bool(map, "prim")?,
        }),
        None => None,
    };
    let animated_objects = match map_member(body, "AnimatedObjects")? {
        Some(map) => Some(AnimatedObjects {
            max_tris: map_int(map, "AnimatedObjectMaxTris")?,
            max_agent_attachments: map_int(map, "MaxAgentAnimatedObjectAttachments")?,
        }),
        None => None,
    };
    let open_sim_extras = match map_member(body, "OpenSimExtras")? {
        Some(map) => Some(OpenSimExtras::from_llsd(map)?),
        None => None,
    };
    let mut features = SimulatorFeatures {
        mesh_rez_enabled: body.field_bool("MeshRezEnabled", "MeshRezEnabled")?,
        mesh_upload_enabled: body.field_bool("MeshUploadEnabled", "MeshUploadEnabled")?,
        mesh_xfer_enabled: body.field_bool("MeshXferEnabled", "MeshXferEnabled")?,
        bakes_on_mesh_enabled: body.field_bool("BakesOnMeshEnabled", "BakesOnMeshEnabled")?,
        avatar_hover_height_enabled: body
            .field_bool("AvatarHoverHeightEnabled", "AvatarHoverHeightEnabled")?,
        physics_materials_enabled: body
            .field_bool("PhysicsMaterialsEnabled", "PhysicsMaterialsEnabled")?,
        physics_shape_types,
        animated_objects,
        dynamic_pathfinding_enabled: body
            .field_bool("DynamicPathfindingEnabled", "DynamicPathfindingEnabled")?,
        max_agent_attachments: body.field_i32("MaxAgentAttachments", "MaxAgentAttachments")?,
        max_agent_groups: body.field_i32("MaxAgentGroups", "MaxAgentGroups")?,
        max_agent_groups_basic: body.field_i32("MaxAgentGroupsBasic", "MaxAgentGroupsBasic")?,
        max_agent_groups_premium: body
            .field_i32("MaxAgentGroupsPremium", "MaxAgentGroupsPremium")?,
        max_estate_access_ids: body.field_i32("MaxEstateAccessIds", "MaxEstateAccessIds")?,
        max_estate_managers: body.field_i32("MaxEstateManagers", "MaxEstateManagers")?,
        max_materials_per_transaction: body
            .field_i32("MaxMaterialsPerTransaction", "MaxMaterialsPerTransaction")?,
        max_texture_resolution: body.field_i32("MaxTextureResolution", "MaxTextureResolution")?,
        render_materials_capability: body
            .field_f32("RenderMaterialsCapability", "RenderMaterialsCapability")?,
        pbr_terrain_enabled: body.field_bool("PBRTerrainEnabled", "PBRTerrainEnabled")?,
        pbr_terrain_transforms_enabled: body
            .field_bool("PBRTerrainTransformsEnabled", "PBRTerrainTransformsEnabled")?,
        pbr_material_swatch_enabled: body
            .field_bool("PBRMaterialSwatchEnabled", "PBRMaterialSwatchEnabled")?,
        gltf_enabled: body.field_bool("GLTFEnabled", "GLTFEnabled")?,
        mirrors_enabled: body.field_bool("MirrorsEnabled", "MirrorsEnabled")?,
        no_mod_bypass_support: body.field_bool("NoModBypassSupport", "NoModBypassSupport")?,
        lua_scripts_enabled: body.field_bool("LuaScriptsEnabled", "LuaScriptsEnabled")?,
        lsl_syntax_id: body.field_uuid("LSLSyntaxId", "LSLSyntaxId")?,
        lsl_syntax_version: map_text(body, "LSLSyntaxVersion")?,
        dead_reckoning_distance: body
            .field_f32("DeadReckoningDistance", "DeadReckoningDistance")?,
        dead_reckoning_time: body.field_f32("DeadReckoningTime", "DeadReckoningTime")?,
        host_name: map_text(body, "HostName")?,
        voice_server_type: map_text(body, "VoiceServerType")?,
        menus: map_member(body, "menus")?.map(DynamicMenus::from_llsd),
        open_sim_extras,
        other: BTreeMap::new(),
    };
    features.other = untyped_members(body, &features.typed_members());
    Ok(features)
}

// ---------------------------------------------------------------------------
// Server side — the inverse: the reply builder.
// ---------------------------------------------------------------------------

impl SimulatorFeatures {
    /// The reply's typed fields as LLSD map members: only advertised (`Some`)
    /// keys, each in the LLSD kind the live grids send it in.
    fn typed_members(&self) -> HashMap<String, Llsd> {
        let mut map: HashMap<String, Llsd> = HashMap::new();
        let mut put = |key: &str, value: Option<Llsd>| {
            if let Some(value) = value {
                let _previous = map.insert(key.to_owned(), value);
            }
        };
        let text = |value: Option<&String>| value.cloned().map(Llsd::String);
        let real = |value: Option<f32>| value.map(|value| Llsd::Real(f64::from(value)));
        put("MeshRezEnabled", self.mesh_rez_enabled.map(Llsd::Boolean));
        put(
            "MeshUploadEnabled",
            self.mesh_upload_enabled.map(Llsd::Boolean),
        );
        put("MeshXferEnabled", self.mesh_xfer_enabled.map(Llsd::Boolean));
        put(
            "BakesOnMeshEnabled",
            self.bakes_on_mesh_enabled.map(Llsd::Boolean),
        );
        put(
            "AvatarHoverHeightEnabled",
            self.avatar_hover_height_enabled.map(Llsd::Boolean),
        );
        put(
            "PhysicsMaterialsEnabled",
            self.physics_materials_enabled.map(Llsd::Boolean),
        );
        put(
            "PhysicsShapeTypes",
            self.physics_shape_types.map(|shapes| {
                Llsd::Map(HashMap::from([
                    ("convex".to_owned(), Llsd::Boolean(shapes.convex)),
                    ("none".to_owned(), Llsd::Boolean(shapes.none)),
                    ("prim".to_owned(), Llsd::Boolean(shapes.prim)),
                ]))
            }),
        );
        put(
            "AnimatedObjects",
            self.animated_objects.map(|animated| {
                Llsd::Map(HashMap::from([
                    (
                        "AnimatedObjectMaxTris".to_owned(),
                        Llsd::Integer(animated.max_tris),
                    ),
                    (
                        "MaxAgentAnimatedObjectAttachments".to_owned(),
                        Llsd::Integer(animated.max_agent_attachments),
                    ),
                ]))
            }),
        );
        put(
            "DynamicPathfindingEnabled",
            self.dynamic_pathfinding_enabled.map(Llsd::Boolean),
        );
        put(
            "MaxAgentAttachments",
            self.max_agent_attachments.map(Llsd::Integer),
        );
        put("MaxAgentGroups", self.max_agent_groups.map(Llsd::Integer));
        put(
            "MaxAgentGroupsBasic",
            self.max_agent_groups_basic.map(Llsd::Integer),
        );
        put(
            "MaxAgentGroupsPremium",
            self.max_agent_groups_premium.map(Llsd::Integer),
        );
        put(
            "MaxEstateAccessIds",
            self.max_estate_access_ids.map(Llsd::Integer),
        );
        put(
            "MaxEstateManagers",
            self.max_estate_managers.map(Llsd::Integer),
        );
        put(
            "MaxMaterialsPerTransaction",
            self.max_materials_per_transaction.map(Llsd::Integer),
        );
        put(
            "MaxTextureResolution",
            self.max_texture_resolution.map(Llsd::Integer),
        );
        put(
            "RenderMaterialsCapability",
            real(self.render_materials_capability),
        );
        put(
            "PBRTerrainEnabled",
            self.pbr_terrain_enabled.map(Llsd::Boolean),
        );
        put(
            "PBRTerrainTransformsEnabled",
            self.pbr_terrain_transforms_enabled.map(Llsd::Boolean),
        );
        put(
            "PBRMaterialSwatchEnabled",
            self.pbr_material_swatch_enabled.map(Llsd::Boolean),
        );
        put("GLTFEnabled", self.gltf_enabled.map(Llsd::Boolean));
        put("MirrorsEnabled", self.mirrors_enabled.map(Llsd::Boolean));
        put(
            "NoModBypassSupport",
            self.no_mod_bypass_support.map(Llsd::Boolean),
        );
        put(
            "LuaScriptsEnabled",
            self.lua_scripts_enabled.map(Llsd::Boolean),
        );
        put("LSLSyntaxId", self.lsl_syntax_id.map(Llsd::Uuid));
        put("LSLSyntaxVersion", text(self.lsl_syntax_version.as_ref()));
        put("DeadReckoningDistance", real(self.dead_reckoning_distance));
        put("DeadReckoningTime", real(self.dead_reckoning_time));
        put("HostName", text(self.host_name.as_ref()));
        put("VoiceServerType", text(self.voice_server_type.as_ref()));
        put("menus", self.menus.as_ref().map(DynamicMenus::to_llsd));
        put(
            "OpenSimExtras",
            self.open_sim_extras.as_ref().map(OpenSimExtras::to_llsd),
        );
        map
    }

    /// The whole reply as an LLSD map: the advertised typed fields, then the
    /// [`other`](Self::other) keys, a typed field winning a clash.
    #[must_use]
    pub fn to_llsd(&self) -> Llsd {
        let mut map = self.typed_members();
        for (key, value) in &self.other {
            let _existing = map.entry(key.clone()).or_insert_with(|| value.clone());
        }
        Llsd::Map(map)
    }

    /// Everything the reply advertises, by dotted path and LLSD kind — a nested
    /// map is listed itself (`OpenSimExtras` → `map`) and member by member
    /// (`OpenSimExtras.GridName` → `string`).
    ///
    /// This is the reply's shape with the values left out, which is the part of
    /// it a grid keeps constant from region to region: what a survey of a grid
    /// records, and what a caller prints to say what a region offers.
    #[must_use]
    pub fn advertised(&self) -> BTreeMap<String, &'static str> {
        let mut out = BTreeMap::new();
        list_members("", &self.to_llsd(), &mut out);
        out
    }
}

/// Builds a `SimulatorFeatures` GET reply from a [`SimulatorFeatures`] — the
/// inverse of [`parse_simulator_features`]. The `OpenSimExtras` map is emitted
/// only when [`open_sim_extras`](SimulatorFeatures::open_sim_extras) is present
/// (a Second Life-style reply leaves it [`None`]). Built on
/// [`Llsd::to_llsd_xml`], so it round-trips through
/// [`parse_simulator_features`].
#[must_use]
pub fn build_simulator_features_response(features: &SimulatorFeatures) -> String {
    features.to_llsd().to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        AnimatedObjects, DynamicMenus, OpenSimExtras, PhysicsShapeTypes, SimulatorFeatures,
        build_simulator_features_response, parse_simulator_features,
    };
    use crate::llsd::Llsd;
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
            voice_server_type: Some("webrtc".to_owned()),
            open_sim_extras: Some(OpenSimExtras {
                export_supported: Some(true),
                map_server_url: Some(
                    url::Url::parse("http://maps.example/").map_err(|e| e.to_string())?,
                ),
                search_server_url: Some(
                    url::Url::parse("http://search.example/").map_err(|e| e.to_string())?,
                ),
                destination_guide_url: Some(
                    url::Url::parse("http://guide.example/").map_err(|e| e.to_string())?,
                ),
                avatar_picker_url: Some(
                    url::Url::parse("http://picker.example/").map_err(|e| e.to_string())?,
                ),
                grid_url: Some(url::Url::parse("http://grid.example/").map_err(|e| e.to_string())?),
                currency: Some("OS$".to_owned()),
                currency_base_uri: Some(
                    url::Url::parse("http://economy.example/").map_err(|e| e.to_string())?,
                ),
                say_range: Some(20),
                shout_range: Some(100),
                whisper_range: Some(10),
                min_prim_scale: Some(0.01),
                max_prim_scale: Some(64.0),
                max_phys_prim_scale: Some(10.0),
                ..OpenSimExtras::default()
            }),
            ..SimulatorFeatures::default()
        };
        let xml = build_simulator_features_response(&features);
        let parsed =
            parse_simulator_features(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, features);
        Ok(())
    }

    /// OpenSim's grid-wide extra-features path injects `ExportSupported` into
    /// `OpenSimExtras` as a **string** (`GetGridExtraFeatures` stores every grid
    /// extra as a string; the `GridService` default is the literal `"true"`), so
    /// a real OpenSim reply carries `<string>true</string>` where the Second
    /// Life-style path would carry a boolean. The parser accepts the string form
    /// rather than rejecting the whole reply as malformed.
    #[test]
    fn export_supported_string_decodes() -> Result<(), String> {
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>MeshUploadEnabled</key><boolean>true</boolean>",
            "<key>OpenSimExtras</key><map>",
            "<key>ExportSupported</key><string>true</string>",
            "<key>currency</key><string>OS$</string></map>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let features = parse_simulator_features(&body).map_err(|error| format!("{error:?}"))?;
        let extras = features.open_sim_extras.ok_or("expected OpenSimExtras")?;
        assert_eq!(extras.export_supported, Some(true));
        assert_eq!(extras.currency.as_deref(), Some("OS$"));
        Ok(())
    }

    /// A `false` string is honoured too, and an unparsable string is a hard
    /// error rather than being silently coerced.
    #[test]
    fn export_supported_string_false_and_garbage() -> Result<(), String> {
        let false_body = parse_llsd_xml(concat!(
            "<llsd><map><key>OpenSimExtras</key><map>",
            "<key>ExportSupported</key><string>False</string></map></map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let features =
            parse_simulator_features(&false_body).map_err(|error| format!("{error:?}"))?;
        let extras = features.open_sim_extras.ok_or("expected OpenSimExtras")?;
        assert_eq!(extras.export_supported, Some(false));

        let garbage_body = parse_llsd_xml(concat!(
            "<llsd><map><key>OpenSimExtras</key><map>",
            "<key>ExportSupported</key><string>maybe</string></map></map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        match parse_simulator_features(&garbage_body) {
            Err(_) => {}
            Ok(_) => return Err("an unparsable ExportSupported string must be rejected".to_owned()),
        }
        Ok(())
    }

    /// The reply an aditi mainland region gave on 2026-10-07, verbatim but for
    /// the simulator host's name.
    const SECOND_LIFE_REPLY: &str = concat!(
        "<llsd><map>",
        "<key>AnimatedObjects</key><map>",
        "<key>AnimatedObjectMaxTris</key><integer>100000</integer>",
        "<key>MaxAgentAnimatedObjectAttachments</key><integer>1</integer></map>",
        "<key>AvatarHoverHeightEnabled</key><boolean>true</boolean>",
        "<key>BakesOnMeshEnabled</key><boolean>true</boolean>",
        "<key>DeadReckoningDistance</key><real>20</real>",
        "<key>DeadReckoningTime</key><real>1</real>",
        "<key>DynamicPathfindingEnabled</key><boolean>true</boolean>",
        "<key>GLTFEnabled</key><boolean>false</boolean>",
        "<key>HostName</key><string>simhost-0123456789abcdef0.aditi.secondlife.io</string>",
        "<key>LSLSyntaxId</key><uuid>af29bb2b-5b56-b15f-04ee-fa42372d11db</uuid>",
        "<key>LSLSyntaxVersion</key><string>0.6.13</string>",
        "<key>LuaScriptsEnabled</key><boolean>false</boolean>",
        "<key>MaxAgentAttachments</key><integer>38</integer>",
        "<key>MaxAgentGroups</key><integer>50</integer>",
        "<key>MaxAgentGroupsBasic</key><integer>42</integer>",
        "<key>MaxAgentGroupsPremium</key><integer>70</integer>",
        "<key>MaxEstateAccessIds</key><integer>750</integer>",
        "<key>MaxEstateManagers</key><integer>20</integer>",
        "<key>MaxMaterialsPerTransaction</key><integer>50</integer>",
        "<key>MaxTextureResolution</key><integer>2048</integer>",
        "<key>MeshRezEnabled</key><boolean>true</boolean>",
        "<key>MeshUploadEnabled</key><boolean>true</boolean>",
        "<key>MeshXferEnabled</key><boolean>true</boolean>",
        "<key>MirrorsEnabled</key><boolean>true</boolean>",
        "<key>NoModBypassSupport</key><boolean>true</boolean>",
        "<key>PBRMaterialSwatchEnabled</key><boolean>true</boolean>",
        "<key>PBRTerrainEnabled</key><boolean>true</boolean>",
        "<key>PBRTerrainTransformsEnabled</key><boolean>true</boolean>",
        "<key>PhysicsMaterialsEnabled</key><boolean>true</boolean>",
        "<key>PhysicsShapeTypes</key><map>",
        "<key>convex</key><boolean>true</boolean>",
        "<key>none</key><boolean>true</boolean>",
        "<key>prim</key><boolean>true</boolean></map>",
        "<key>RenderMaterialsCapability</key><real>4</real>",
        "<key>VoiceServerType</key><string>webrtc</string>",
        "</map></llsd>"
    );

    /// The reply the local OpenSim standalone gave on 2026-10-07, verbatim but
    /// for `GridURL`, which it sends without the trailing slash a parsed URL
    /// is written back with.
    const OPENSIM_REPLY: &str = concat!(
        "<llsd><map>",
        "<key>AnimatedObjects</key><map>",
        "<key>AnimatedObjectMaxTris</key><integer>150000</integer>",
        "<key>MaxAgentAnimatedObjectAttachments</key><integer>2</integer></map>",
        "<key>AvatarHoverHeightEnabled</key><boolean>true</boolean>",
        "<key>BakesOnMeshEnabled</key><boolean>true</boolean>",
        "<key>LSLSyntaxId</key><uuid>4b833b57-d52b-5503-85a4-76754ac3b8ff</uuid>",
        "<key>MaxAgentAttachments</key><integer>38</integer>",
        "<key>MaxAgentGroupsBasic</key><integer>60</integer>",
        "<key>MaxAgentGroupsPremium</key><integer>60</integer>",
        "<key>MaxMaterialsPerTransaction</key><integer>50</integer>",
        "<key>MeshRezEnabled</key><boolean>true</boolean>",
        "<key>MeshUploadEnabled</key><boolean>true</boolean>",
        "<key>MeshXferEnabled</key><boolean>true</boolean>",
        "<key>OpenSimExtras</key><map>",
        "<key>AnimationSet</key><boolean>true</boolean>",
        "<key>AvatarSkeleton</key><boolean>true</boolean>",
        "<key>ExportSupported</key><string>true</string>",
        "<key>GridName</key><string>the lost continent of hippo</string>",
        "<key>GridNick</key><string>hippogrid</string>",
        "<key>GridURL</key><string>http://127.0.0.1:9000/</string>",
        "<key>GridURLAlias</key><string></string>",
        "<key>MaxHeightmap</key><real>4000</real>",
        "<key>MaxPhysPrimScale</key><real>64</real>",
        "<key>MaxPrimScale</key><real>256</real>",
        "<key>MaxSimHeight</key><real>50000</real>",
        "<key>MinHeightmap</key><real>-100</real>",
        "<key>MinPhysPrimScale</key><real>0.009999999776482582</real>",
        "<key>MinPrimScale</key><real>0.0010000000474974513</real>",
        "<key>MinSimHeight</key><real>-100</real>",
        "<key>SimulatorFPS</key><real>11.001100540161133</real>",
        "<key>SimulatorFPSCritPercent</key><integer>40</integer>",
        "<key>SimulatorFPSFactor</key><real>4.999499797821045</real>",
        "<key>SimulatorFPSWarnPercent</key><integer>60</integer>",
        "<key>currency-base-uri</key><string>http://127.0.0.1:9000/</string>",
        "<key>map-server-url</key><string>http://127.0.0.1:9000/</string>",
        "<key>say-range</key><integer>20</integer>",
        "<key>shout-range</key><integer>100</integer>",
        "<key>whisper-range</key><integer>10</integer></map>",
        "<key>PhysicsMaterialsEnabled</key><boolean>true</boolean>",
        "<key>PhysicsShapeTypes</key><map>",
        "<key>convex</key><boolean>true</boolean>",
        "<key>none</key><boolean>true</boolean>",
        "<key>prim</key><boolean>true</boolean></map>",
        "<key>RenderMaterialsCapability</key><real>3</real>",
        "<key>menus</key><map>",
        "<key>admin</key><map /><key>advanced</key><map /><key>agent</key><map />",
        "<key>tools</key><map /><key>world</key><map /></map>",
        "</map></llsd>"
    );

    /// Every key Second Life was measured sending has a typed field, and the
    /// builder writes each back in the LLSD kind it arrived in: the decoded
    /// reply re-encodes to the very document that was parsed.
    #[test]
    fn second_life_reply_as_measured_is_fully_typed() -> Result<(), String> {
        let body = parse_llsd_xml(SECOND_LIFE_REPLY).map_err(|error| format!("{error:?}"))?;
        let features = parse_simulator_features(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(features.other, std::collections::BTreeMap::new());
        assert_eq!(features.open_sim_extras, None);
        assert_eq!(features.menus, None);
        assert_eq!(features.max_agent_groups, Some(50));
        assert_eq!(features.max_estate_access_ids, Some(750));
        assert_eq!(features.render_materials_capability, Some(4.0));
        assert_eq!(features.lsl_syntax_version.as_deref(), Some("0.6.13"));
        assert_eq!(features.gltf_enabled, Some(false));
        assert_eq!(features.to_llsd(), body);
        let advertised = features.advertised();
        assert_eq!(advertised.len(), 36);
        assert_eq!(advertised.get("RenderMaterialsCapability"), Some(&"real"));
        assert_eq!(advertised.get("PhysicsShapeTypes"), Some(&"map"));
        assert_eq!(advertised.get("PhysicsShapeTypes.convex"), Some(&"boolean"));
        Ok(())
    }

    /// The same for OpenSim's reply, whose `ExportSupported` is a string, whose
    /// `GridURLAlias` is present and empty, and whose `menus` holds five empty
    /// menus.
    #[test]
    fn opensim_reply_as_measured_is_fully_typed() -> Result<(), String> {
        let body = parse_llsd_xml(OPENSIM_REPLY).map_err(|error| format!("{error:?}"))?;
        let features = parse_simulator_features(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(features.other, std::collections::BTreeMap::new());
        assert_eq!(features.menus, Some(DynamicMenus::default()));
        let extras = features
            .open_sim_extras
            .as_ref()
            .ok_or("expected OpenSimExtras")?;
        assert_eq!(extras.other, std::collections::BTreeMap::new());
        assert_eq!(extras.export_supported, Some(true));
        assert_eq!(extras.grid_url_alias.as_deref(), Some(""));
        assert_eq!(extras.max_prim_scale, Some(256.0));
        assert_eq!(extras.simulator_fps_warn_percent, Some(60));
        assert_eq!(features.to_llsd(), body);
        let advertised = features.advertised();
        assert_eq!(
            advertised.get("OpenSimExtras.ExportSupported"),
            Some(&"string")
        );
        assert_eq!(advertised.get("menus.agent"), Some(&"map"));
        assert_eq!(advertised.len(), 50);
        Ok(())
    }

    /// A key no field carries survives the decode and is sent back, at the top
    /// level and inside the extras; a URL key sent empty is kept the same way.
    #[test]
    fn unknown_keys_are_kept_and_sent_back() -> Result<(), String> {
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>MeshRezEnabled</key><boolean>true</boolean>",
            "<key>SomethingNew</key><integer>7</integer>",
            "<key>OpenSimExtras</key><map>",
            "<key>camera-only-mode</key><string>true</string>",
            "<key>search-server-url</key><string></string></map>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let features = parse_simulator_features(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(features.other.get("SomethingNew"), Some(&Llsd::Integer(7)));
        assert_eq!(features.other.len(), 1);
        let extras = features
            .open_sim_extras
            .as_ref()
            .ok_or("expected OpenSimExtras")?;
        assert_eq!(extras.search_server_url, None);
        assert_eq!(
            extras.other.get("camera-only-mode"),
            Some(&Llsd::String("true".to_owned()))
        );
        assert_eq!(extras.other.len(), 2);
        assert_eq!(features.to_llsd(), body);
        Ok(())
    }

    /// The menu entries a region adds decode to their titles and encode back to
    /// the title-keyed maps `DynamicMenuModule` sends.
    #[test]
    fn dynamic_menu_entries_round_trip() -> Result<(), String> {
        let features = SimulatorFeatures {
            menus: Some(DynamicMenus {
                world: vec!["Region tour".to_owned(), "Welcome".to_owned()],
                ..DynamicMenus::default()
            }),
            ..SimulatorFeatures::default()
        };
        let xml = build_simulator_features_response(&features);
        let parsed =
            parse_simulator_features(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?)
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, features);
        Ok(())
    }
}
