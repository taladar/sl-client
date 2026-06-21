//! Simulator notification payloads: the "mean collision" records carried by a
//! `MeanCollisionAlert`. The plain alert strings (`AlertMessage` /
//! `AgentAlertMessage`), the agent health (`HealthMessage`) and the camera
//! collision plane (`CameraConstraint`) are simple enough to live inline on the
//! [`Event`](crate::Event) variants; the structured alert key/parameters reuse
//! [`AlertInfo`](crate::AlertInfo).

use uuid::Uuid;

/// The kind of a "mean collision" reported by a `MeanCollisionAlert`: how one
/// avatar (the [`perp`](MeanCollision::perp)) collided with another (the
/// [`victim`](MeanCollision::victim)). The numeric values match the viewer's
/// `EMeanCollisionType` (`mean_collision_data.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeanCollisionType {
    /// `MEAN_INVALID` (`0`) — an unset / placeholder type.
    Invalid,
    /// `MEAN_BUMP` (`1`) — the perpetrator physically bumped the victim.
    Bump,
    /// `MEAN_LLPUSHOBJECT` (`2`) — the perpetrator `llPushObject`-ed the victim.
    PushObject,
    /// `MEAN_SELECTED_OBJECT_COLLIDE` (`3`) — the perpetrator dragged a selected
    /// object into the victim.
    SelectedObjectCollide,
    /// `MEAN_SCRIPTED_OBJECT_COLLIDE` (`4`) — the perpetrator hit the victim with
    /// a scripted object.
    ScriptedObjectCollide,
    /// `MEAN_PHYSICAL_OBJECT_COLLIDE` (`5`) — the perpetrator hit the victim with
    /// a physical object.
    PhysicalObjectCollide,
    /// An unrecognised collision-type value, preserved verbatim (includes the
    /// viewer's `MEAN_EOF` sentinel `6`).
    Unknown(u8),
}

impl MeanCollisionType {
    /// Classifies a `MeanCollision.Type` wire value.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            1 => Self::Bump,
            2 => Self::PushObject,
            3 => Self::SelectedObjectCollide,
            4 => Self::ScriptedObjectCollide,
            5 => Self::PhysicalObjectCollide,
            other => Self::Unknown(other),
        }
    }

    /// The wire value for this collision type.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Invalid => 0,
            Self::Bump => 1,
            Self::PushObject => 2,
            Self::SelectedObjectCollide => 3,
            Self::ScriptedObjectCollide => 4,
            Self::PhysicalObjectCollide => 5,
            Self::Unknown(other) => other,
        }
    }
}

/// One "mean collision" record from a `MeanCollisionAlert`: the simulator's
/// accounting of an avatar-on-avatar collision (the data behind the viewer's
/// "Bumps, Pushes & Hits" panel). Surfaced as part of
/// [`Event::MeanCollisionAlert`](crate::Event::MeanCollisionAlert).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeanCollision {
    /// The avatar that was collided with.
    pub victim: Uuid,
    /// The avatar (or the owner of the object) that caused the collision.
    pub perp: Uuid,
    /// When the collision happened, as a Unix timestamp (`time_t`, seconds).
    pub time: u32,
    /// The collision magnitude (velocity or total force, depending on
    /// [`collision_type`](MeanCollision::collision_type)).
    pub magnitude: f32,
    /// How the collision occurred.
    pub collision_type: MeanCollisionType,
}
