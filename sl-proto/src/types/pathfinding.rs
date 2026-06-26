//! Pathfinding agent-state and navmesh-status carriers
//! (`AgentStateUpdate`, `NavMeshStatusUpdate`).
//!
//! These arrive over the CAPS event queue, not LLUDP. Second Life pushes them
//! to keep a viewer's pathfinding UI current — whether the agent is currently
//! allowed to rebake the region's navmesh, and how the region's navmesh bake is
//! progressing. OpenSim emits neither, so they only ever appear against a real
//! Linden Lab simulator. They surface as typed [`Event`](super::Event)s instead
//! of being dropped to a `Diagnostic::UnknownCapsEvent`.

use uuid::Uuid;

/// The build state of a region's navmesh, reported by a `NavMeshStatusUpdate`.
///
/// The navmesh is the navigation surface the simulator bakes from the region's
/// pathfinding-relevant geometry; it is rebuilt whenever that geometry changes.
/// The states mirror the reference viewer's `LLPathfindingNavMeshStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavMeshBuildStatus {
    /// The navmesh is dirty and a rebuild is queued but has not started.
    Pending,
    /// The simulator is currently baking the navmesh.
    Building,
    /// The navmesh is up to date.
    Complete,
    /// The navmesh became dirty again while a rebuild was already pending.
    Repending,
}

impl NavMeshBuildStatus {
    /// Parses the wire `status` token. Mirrors the reference viewer, which maps
    /// any unrecognised value (and a missing field) to
    /// [`Complete`](Self::Complete).
    #[must_use]
    pub fn from_wire(status: &str) -> Self {
        match status {
            "pending" => Self::Pending,
            "building" => Self::Building,
            "repending" => Self::Repending,
            _ => Self::Complete,
        }
    }

    /// The wire `status` token for this state — the inverse of
    /// [`from_wire`](Self::from_wire). Round-trips for every variant.
    #[must_use]
    pub const fn to_wire(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Building => "building",
            Self::Complete => "complete",
            Self::Repending => "repending",
        }
    }
}

/// A region's navmesh build status, parsed from a `NavMeshStatusUpdate` CAPS
/// event-queue push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavMeshStatus {
    /// The region the navmesh belongs to. A region id stays a raw [`Uuid`]
    /// throughout this crate (there is no dedicated region-key newtype).
    pub region_id: Uuid,
    /// The navmesh version; bumped each time the region's navmesh is rebaked,
    /// so a client can tell whether its cached navmesh is current.
    pub version: u32,
    /// The current build state.
    pub status: NavMeshBuildStatus,
}

#[cfg(test)]
mod tests {
    use super::NavMeshBuildStatus;
    use pretty_assertions::assert_eq;

    /// Every documented status token maps to its variant.
    #[test]
    fn nav_mesh_status_known_tokens() {
        assert_eq!(
            NavMeshBuildStatus::from_wire("pending"),
            NavMeshBuildStatus::Pending
        );
        assert_eq!(
            NavMeshBuildStatus::from_wire("building"),
            NavMeshBuildStatus::Building
        );
        assert_eq!(
            NavMeshBuildStatus::from_wire("complete"),
            NavMeshBuildStatus::Complete
        );
        assert_eq!(
            NavMeshBuildStatus::from_wire("repending"),
            NavMeshBuildStatus::Repending
        );
    }

    /// Every variant's `to_wire` token round-trips back through `from_wire`.
    #[test]
    fn nav_mesh_status_to_wire_round_trips() {
        for status in [
            NavMeshBuildStatus::Pending,
            NavMeshBuildStatus::Building,
            NavMeshBuildStatus::Complete,
            NavMeshBuildStatus::Repending,
        ] {
            assert_eq!(NavMeshBuildStatus::from_wire(status.to_wire()), status);
        }
    }

    /// An unrecognised (or empty) token falls back to `Complete`, as the
    /// reference viewer does.
    #[test]
    fn nav_mesh_status_unknown_token_defaults_to_complete() {
        assert_eq!(
            NavMeshBuildStatus::from_wire("frobnicate"),
            NavMeshBuildStatus::Complete
        );
        assert_eq!(
            NavMeshBuildStatus::from_wire(""),
            NavMeshBuildStatus::Complete
        );
    }
}
