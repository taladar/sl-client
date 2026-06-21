//! Region identity, limits, chat and combat settings.

use super::{Maturity, ProductType};
use sl_wire::RegionHandle;
use uuid::Uuid;

/// A region's identity, maturity, and product type, parsed from `RegionHandshake`.
///
/// (Not `Eq`: `water_height` / `billable_factor` are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionIdentity {
    /// The region (simulator) name.
    pub sim_name: String,
    /// The region's globally-unique id (`RegionID`, from the `RegionInfo2` block).
    pub region_id: Uuid,
    /// The region handle: its global south-west corner packed as
    /// `(global_x << 32) | global_y`, or `0` when not yet known. The
    /// `RegionHandshake` message does not itself carry the handle, so this is the
    /// handle the session has learned for the simulator — seeded from the login
    /// response's `region_x` / `region_y` for the start region, and otherwise from
    /// `EnableSimulator` / object updates.
    pub region_handle: RegionHandle,
    /// The region's grid X coordinate (region index = the handle's global X metres
    /// divided by 256), derived from [`Self::region_handle`]; `0` when the handle
    /// is unknown.
    pub grid_x: u32,
    /// The region's grid Y coordinate; see [`Self::grid_x`].
    pub grid_y: u32,
    /// The raw 32-bit `RegionFlags` bitfield (decode with [`sl_wire::RegionFlags`]).
    pub region_flags: u32,
    /// The full 64-bit `RegionFlagsExtended` (from the `RegionInfo4` block); falls
    /// back to the zero-extended 32-bit [`Self::region_flags`] when the grid sends
    /// no `RegionInfo4` (e.g. OpenSim and older simulators).
    pub region_flags_extended: u64,
    /// The `RegionProtocols` capability bitfield (from `RegionInfo4`), or `0` when
    /// the grid sends no `RegionInfo4`.
    pub region_protocols: u64,
    /// The maturity / content rating.
    pub maturity: Maturity,
    /// The inferred product type.
    pub product: ProductType,
    /// The raw `ProductSKU` string (possibly empty, e.g. on OpenSim).
    pub product_sku: String,
    /// The raw `ProductName` string (possibly empty, e.g. on OpenSim).
    pub product_name: String,
    /// The simulator's advertised CPU class (`CPUClassID`, from the `RegionInfo3`
    /// block); a coarse performance tier. `0` when the grid does not provide it.
    pub cpu_class_id: i32,
    /// The simulator's CPU ratio — roughly how many regions share the host CPU
    /// (`CPURatio`, from the `RegionInfo3` block). `0` when not provided.
    pub cpu_ratio: i32,
    /// The region/estate owner's id.
    pub sim_owner: Uuid,
    /// Whether *this* agent is an estate manager for the region (gates estate UI).
    pub is_estate_manager: bool,
    /// The region's water height, in metres.
    pub water_height: f32,
    /// The billing factor applied to land tier in this region.
    pub billable_factor: f32,
}

/// A region's agent and object capacity plus estate/terrain/chat/combat settings,
/// parsed from `RegionInfo` (the reply to
/// [`Session::request_region_info`](crate::Session::request_region_info)).
///
/// (Not `Eq`: several fields are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionLimits {
    /// The region (simulator) name.
    pub sim_name: String,
    /// The maximum concurrent agents (prefers the 32-bit field, falling back to
    /// the legacy 8-bit `MaxAgents`).
    pub max_agents: u32,
    /// The hard agent cap, or `0` if the grid did not provide it (common for
    /// non-estate-managers on Second Life, and on OpenSim).
    pub hard_max_agents: u32,
    /// The hard region-wide object/prim cap, or `0` if not provided.
    pub hard_max_objects: u32,
    /// The raw 32-bit `RegionFlags` bitfield (decode with [`sl_wire::RegionFlags`]).
    pub region_flags: u32,
    /// The full 64-bit `RegionFlagsExtended` (from the `RegionInfo3` block); falls
    /// back to the zero-extended 32-bit [`Self::region_flags`] when the grid sends
    /// no `RegionInfo3`.
    pub region_flags_extended: u64,
    /// The maturity / content rating.
    pub maturity: Maturity,
    /// The estate this region belongs to.
    pub estate_id: u32,
    /// The parent estate (the "mainland" estate is `1`).
    pub parent_estate_id: u32,
    /// The region's water height, in metres.
    pub water_height: f32,
    /// The billing factor applied to land tier in this region.
    pub billable_factor: f32,
    /// The prim-allowance multiplier applied to parcel object limits.
    pub object_bonus_factor: f32,
    /// The maximum height a terrain edit may raise the ground above its baked
    /// value, in metres.
    pub terrain_raise_limit: f32,
    /// The maximum depth a terrain edit may lower the ground below its baked
    /// value, in metres.
    pub terrain_lower_limit: f32,
    /// The land price per square metre, in L$.
    pub price_per_meter: i32,
    /// The grid X this region redirects to, or `0` for none.
    pub redirect_grid_x: i32,
    /// The grid Y this region redirects to, or `0` for none.
    pub redirect_grid_y: i32,
    /// Whether the region uses the estate's sun position rather than its own.
    pub use_estate_sun: bool,
    /// The fixed sun hour (0–24) when [`Self::use_estate_sun`] is `false`; a
    /// negative value means the sun cycles normally.
    pub sun_hour: f32,
    /// The region's chat-range settings, present only when the grid sends a
    /// `RegionInfo5` block (newer Second Life; absent on OpenSim and older grids).
    pub chat_settings: Option<RegionChatSettings>,
    /// The region's combat/damage settings, present only when the grid sends a
    /// `CombatSettings` block.
    pub combat_settings: Option<RegionCombatSettings>,
}

/// A region's chat whisper/normal/shout ranges and offsets, parsed from a
/// `RegionInfo` `RegionInfo5` block.
///
/// (Not `Eq`: the ranges/offsets are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionChatSettings {
    /// The whisper audibility range, in metres.
    pub whisper_range: f32,
    /// The normal-chat audibility range, in metres.
    pub normal_range: f32,
    /// The shout audibility range, in metres.
    pub shout_range: f32,
    /// The whisper range offset.
    pub whisper_offset: f32,
    /// The normal-chat range offset.
    pub normal_offset: f32,
    /// The shout range offset.
    pub shout_offset: f32,
    /// The raw chat-behaviour flag bitfield.
    pub flags: u32,
}

/// A region's combat/damage settings, parsed from a `RegionInfo`
/// `CombatSettings` block.
///
/// (Not `Eq`: several fields are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionCombatSettings {
    /// The raw combat-behaviour flag bitfield.
    pub flags: u32,
    /// The on-death behaviour code.
    pub on_death: u8,
    /// The rate at which damage may be applied.
    pub damage_throttle: f32,
    /// The health regeneration rate.
    pub regeneration_rate: f32,
    /// The post-respawn invulnerability window, in seconds.
    pub invulnerability_time: f32,
    /// The maximum damage applied per hit.
    pub damage_limit: f32,
}
