//! Region identity, limits, chat and combat settings.

use super::{Maturity, ProductType};
use sl_types::lsl::Vector;
use sl_types::map::{GridCoordinates, RegionName};
use sl_types::money::LindenAmount;
use sl_wire::RegionHandle;
use uuid::Uuid;

/// A region's identity, maturity, and product type, parsed from `RegionHandshake`.
///
/// (Not `Eq`: `water_height` / `billable_factor` are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionIdentity {
    /// The region (simulator) name, or `None` when the grid sent an empty
    /// (unknown) name.
    pub sim_name: Option<RegionName>,
    /// The region's globally-unique id (`RegionID`, from the `RegionInfo2` block).
    pub region_id: Uuid,
    /// The region handle: its global south-west corner packed as
    /// `(global_x << 32) | global_y`, or `0` when not yet known. The
    /// `RegionHandshake` message does not itself carry the handle, so this is the
    /// handle the session has learned for the simulator — seeded from the login
    /// response's `region_x` / `region_y` for the start region, and otherwise from
    /// `EnableSimulator` / object updates.
    pub region_handle: RegionHandle,
    /// The region's grid coordinates (region index pair = the handle's global
    /// metres divided by 256), derived from [`Self::region_handle`]; `(0, 0)`
    /// when the handle is unknown.
    pub grid_coordinates: GridCoordinates,
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
    /// The region (simulator) name, or `None` when the grid sent an empty
    /// (unknown) name.
    pub sim_name: Option<RegionName>,
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
    pub price_per_meter: LindenAmount,
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

/// A single simulator performance/telemetry statistic id, as carried in a
/// `SimStats` `Stat` block's `StatID` field.
///
/// The known ids match the viewer's `ESimStatID`
/// (`indra/newview/llviewerstats.h`) and OpenSim's `StatsID`
/// (`OpenSim/Framework/SimStats.cs`); both agree on ids 0–40. Ids in the
/// 1000+ range are OpenSim-only extras. Any id the simulator sends that is not
/// in either table is preserved as [`SimStatId::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimStatId {
    /// Time dilation (0–1): the fraction of real time the physics simulation
    /// is keeping up with.
    TimeDilation,
    /// Simulator frame rate, in frames per second.
    SimFps,
    /// Physics-engine frame rate, in frames per second.
    PhysicsFps,
    /// Agent updates processed per second.
    AgentUpdatesPerSecond,
    /// Total time spent per frame, in milliseconds.
    FrameTimeMs,
    /// Time spent on networking per frame, in milliseconds.
    NetTimeMs,
    /// Time spent on miscellaneous "other" work per frame, in milliseconds.
    OtherTimeMs,
    /// Time spent on physics per frame, in milliseconds.
    PhysicsTimeMs,
    /// Time spent on agent processing per frame, in milliseconds.
    AgentTimeMs,
    /// Time spent on image/texture work per frame, in milliseconds.
    ImageTimeMs,
    /// Time spent running scripts per frame, in milliseconds.
    ScriptTimeMs,
    /// Total number of prims (tasks) in the region.
    TotalPrims,
    /// Number of active (physical/scripted) prims in the region.
    ActivePrims,
    /// Number of root (main) agents in the region.
    Agents,
    /// Number of child agents (neighbour-region presences) in the region.
    ChildAgents,
    /// Number of active scripts in the region.
    ActiveScripts,
    /// LSL script lines executed per second (deprecated; viewers ignore it).
    ScriptLinesPerSecond,
    /// Inbound packets per second.
    InPacketsPerSecond,
    /// Outbound packets per second.
    OutPacketsPerSecond,
    /// Number of pending asset downloads.
    PendingDownloads,
    /// Number of pending asset uploads.
    PendingUploads,
    /// Simulator virtual memory size, in kilobytes.
    VirtualSizeKb,
    /// Simulator resident memory size, in kilobytes.
    ResidentSizeKb,
    /// Number of pending local asset uploads.
    PendingLocalUploads,
    /// Total unacknowledged bytes in flight.
    UnackedBytes,
    /// Number of physics tasks pinned (non-physical static shapes).
    PhysicsPinnedTasks,
    /// Number of physics tasks at reduced level of detail.
    PhysicsLodTasks,
    /// Time spent in the physics step, in milliseconds.
    PhysicsStepMs,
    /// Time spent updating physics shapes, in milliseconds.
    PhysicsShapeUpdateMs,
    /// Time spent on other physics work, in milliseconds.
    PhysicsOtherMs,
    /// Physics-engine memory use, in bytes.
    PhysicsMemory,
    /// Script events processed per second.
    ScriptEventsPerSecond,
    /// Spare (idle) time per frame, in milliseconds.
    SimSpareTimeMs,
    /// Time spent sleeping per frame, in milliseconds.
    SimSleepTimeMs,
    /// Time spent in the I/O pump per frame, in milliseconds.
    IoPumpTimeMs,
    /// Percentage of scripts run this frame.
    PercentScriptsRun,
    /// Region idle flag (dataserver only).
    RegionIdle,
    /// Region idle-possible flag (dataserver only).
    RegionIdlePossible,
    /// Time spent in the pathfinding/AI step, in milliseconds.
    SimAiStepTimeMs,
    /// Skipped pathfinding silhouette steps per second.
    SkippedSilhouetteStepsPerSecond,
    /// Percentage of characters stepped by the pathfinding engine.
    PercentSteppedCharacters,
    /// OpenSim-only: internal LSL script lines per second.
    InternalScriptLinesPerSecond,
    /// OpenSim-only: secondary frame-dilation measure.
    FrameDilation,
    /// OpenSim-only: number of users currently logging in.
    UsersLoggingIn,
    /// OpenSim-only: total geometric (legacy) prims.
    TotalGeoPrims,
    /// OpenSim-only: total mesh objects.
    TotalMesh,
    /// OpenSim-only: number of script-engine threads.
    ScriptEngineThreadCount,
    /// OpenSim-only: number of NPCs in the region.
    Npcs,
    /// An id present in neither the viewer nor the OpenSim table; the raw value
    /// is preserved.
    Unknown(u32),
}

impl SimStatId {
    /// Classifies a raw `StatID` value from a `SimStats` `Stat` block.
    #[must_use]
    pub const fn from_id(id: u32) -> Self {
        match id {
            0 => Self::TimeDilation,
            1 => Self::SimFps,
            2 => Self::PhysicsFps,
            3 => Self::AgentUpdatesPerSecond,
            4 => Self::FrameTimeMs,
            5 => Self::NetTimeMs,
            6 => Self::OtherTimeMs,
            7 => Self::PhysicsTimeMs,
            8 => Self::AgentTimeMs,
            9 => Self::ImageTimeMs,
            10 => Self::ScriptTimeMs,
            11 => Self::TotalPrims,
            12 => Self::ActivePrims,
            13 => Self::Agents,
            14 => Self::ChildAgents,
            15 => Self::ActiveScripts,
            16 => Self::ScriptLinesPerSecond,
            17 => Self::InPacketsPerSecond,
            18 => Self::OutPacketsPerSecond,
            19 => Self::PendingDownloads,
            20 => Self::PendingUploads,
            21 => Self::VirtualSizeKb,
            22 => Self::ResidentSizeKb,
            23 => Self::PendingLocalUploads,
            24 => Self::UnackedBytes,
            25 => Self::PhysicsPinnedTasks,
            26 => Self::PhysicsLodTasks,
            27 => Self::PhysicsStepMs,
            28 => Self::PhysicsShapeUpdateMs,
            29 => Self::PhysicsOtherMs,
            30 => Self::PhysicsMemory,
            31 => Self::ScriptEventsPerSecond,
            32 => Self::SimSpareTimeMs,
            33 => Self::SimSleepTimeMs,
            34 => Self::IoPumpTimeMs,
            35 => Self::PercentScriptsRun,
            36 => Self::RegionIdle,
            37 => Self::RegionIdlePossible,
            38 => Self::SimAiStepTimeMs,
            39 => Self::SkippedSilhouetteStepsPerSecond,
            40 => Self::PercentSteppedCharacters,
            1000 => Self::InternalScriptLinesPerSecond,
            1001 => Self::FrameDilation,
            1002 => Self::UsersLoggingIn,
            1003 => Self::TotalGeoPrims,
            1004 => Self::TotalMesh,
            1005 => Self::ScriptEngineThreadCount,
            1006 => Self::Npcs,
            other => Self::Unknown(other),
        }
    }

    /// The raw `StatID` value this id corresponds to.
    #[must_use]
    pub const fn id(self) -> u32 {
        match self {
            Self::TimeDilation => 0,
            Self::SimFps => 1,
            Self::PhysicsFps => 2,
            Self::AgentUpdatesPerSecond => 3,
            Self::FrameTimeMs => 4,
            Self::NetTimeMs => 5,
            Self::OtherTimeMs => 6,
            Self::PhysicsTimeMs => 7,
            Self::AgentTimeMs => 8,
            Self::ImageTimeMs => 9,
            Self::ScriptTimeMs => 10,
            Self::TotalPrims => 11,
            Self::ActivePrims => 12,
            Self::Agents => 13,
            Self::ChildAgents => 14,
            Self::ActiveScripts => 15,
            Self::ScriptLinesPerSecond => 16,
            Self::InPacketsPerSecond => 17,
            Self::OutPacketsPerSecond => 18,
            Self::PendingDownloads => 19,
            Self::PendingUploads => 20,
            Self::VirtualSizeKb => 21,
            Self::ResidentSizeKb => 22,
            Self::PendingLocalUploads => 23,
            Self::UnackedBytes => 24,
            Self::PhysicsPinnedTasks => 25,
            Self::PhysicsLodTasks => 26,
            Self::PhysicsStepMs => 27,
            Self::PhysicsShapeUpdateMs => 28,
            Self::PhysicsOtherMs => 29,
            Self::PhysicsMemory => 30,
            Self::ScriptEventsPerSecond => 31,
            Self::SimSpareTimeMs => 32,
            Self::SimSleepTimeMs => 33,
            Self::IoPumpTimeMs => 34,
            Self::PercentScriptsRun => 35,
            Self::RegionIdle => 36,
            Self::RegionIdlePossible => 37,
            Self::SimAiStepTimeMs => 38,
            Self::SkippedSilhouetteStepsPerSecond => 39,
            Self::PercentSteppedCharacters => 40,
            Self::InternalScriptLinesPerSecond => 1000,
            Self::FrameDilation => 1001,
            Self::UsersLoggingIn => 1002,
            Self::TotalGeoPrims => 1003,
            Self::TotalMesh => 1004,
            Self::ScriptEngineThreadCount => 1005,
            Self::Npcs => 1006,
            Self::Unknown(id) => id,
        }
    }
}

/// A region's periodic performance telemetry, parsed from a `SimStats` message.
///
/// The simulator pushes one of these roughly once a second to every agent in
/// the region; the viewer feeds the [`stats`](Self::stats) into its statistics
/// bar. (Not `Eq`: the stat values are `f32`.)
#[derive(Debug, Clone, PartialEq)]
pub struct RegionStats {
    /// The region's grid coordinates (region index pair = global metres / 256),
    /// from the `Region` block's `RegionX` / `RegionY` (which carry the region's
    /// map-tile indices, not a local position).
    pub grid_coordinates: GridCoordinates,
    /// The raw 32-bit `RegionFlags` bitfield (decode with [`sl_wire::RegionFlags`]).
    pub region_flags: u32,
    /// The region's maximum object (prim/task) capacity.
    pub object_capacity: u32,
    /// The full 64-bit `RegionFlagsExtended` (from the `RegionInfo` block); falls
    /// back to the zero-extended 32-bit [`Self::region_flags`] when the grid
    /// sends no `RegionInfo` block (e.g. older simulators).
    pub region_flags_extended: u64,
    /// The individual statistics, each a `(stat id, value)` pair in the order the
    /// simulator sent them.
    pub stats: Vec<(SimStatId, f32)>,
}

/// The simulator's world time and sun state, parsed from a
/// `SimulatorViewerTimeMessage`.
///
/// The simulator pushes this so the viewer can resynchronise its day-cycle
/// clock and sun position. (Not `Eq`: the sun fields are `f32`/[`Vector`].)
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatorTime {
    /// Microseconds since the simulator started (its monotonic world clock).
    pub usec_since_start: u64,
    /// The length of a simulated day, in seconds.
    pub sec_per_day: u32,
    /// The length of a simulated year, in seconds.
    pub sec_per_year: u32,
    /// The sun's direction unit vector.
    pub sun_direction: Vector,
    /// The sun's phase angle along the day cycle, in radians.
    pub sun_phase: f32,
    /// The sun's angular velocity vector.
    pub sun_ang_velocity: Vector,
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

#[cfg(test)]
mod tests {
    use super::SimStatId;
    use pretty_assertions::{assert_eq, assert_ne};

    /// Every known stat id round-trips through its raw value.
    #[test]
    fn sim_stat_id_round_trips() {
        for id in (0..=40).chain(1000..=1006) {
            let classified = SimStatId::from_id(id);
            assert_ne!(classified, SimStatId::Unknown(id));
            assert_eq!(classified.id(), id);
        }
    }

    /// Ids in neither table are preserved as `Unknown`.
    #[test]
    fn sim_stat_id_unknown_preserves_raw_value() {
        assert_eq!(SimStatId::from_id(41), SimStatId::Unknown(41));
        assert_eq!(SimStatId::from_id(999), SimStatId::Unknown(999));
        assert_eq!(SimStatId::from_id(1007), SimStatId::Unknown(1007));
        assert_eq!(SimStatId::Unknown(12_345).id(), 12_345);
    }
}
