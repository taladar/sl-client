//! The render **context matrix**'s vocabulary (`viewer-render-context-matrix`):
//! what a registered subject *is* to the pixel oracle, and the axes along which
//! it can be captured.
//!
//! A subject is captured on its own once (the matrix's R0 tier) and then under
//! **contexts** — an eye under or over the sea, a time of day, a mirror beside
//! it, an occluder in front or a wall behind. Each axis is one slot of
//! [`ContextSet`], so a matrix cell holds at most one value per axis, and the
//! staging that realises a value is written once for every subject rather than
//! once per scene. A new subject lists the axes that mean something for it in
//! `RenderScene::applies`; a new axis is a slot here, a stage builder in the
//! viewer's `render_stage.rs` and a rule in its `expectation()`.

use bevy::prelude::*;

/// The marker colour a subject paints itself in, so the oracle can tell its
/// pixels from an actor's by channel alone. The viewer's `pixel_oracle::Marker`
/// carries the same four, with the thresholds that classify them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectMarker {
    /// `(0.9, 0.1, 0.1)` — the colour the matrix's actors wear, so a red
    /// subject is read against blue instead.
    Red,
    /// `(0.1, 0.9, 0.1)`.
    Green,
    /// `(0.1, 0.1, 0.9)`.
    Blue,
    /// `(0.9, 0.9, 0.1)`.
    Yellow,
}

/// One point on a subject's visible surface the oracle samples on its own, in
/// the subject's **local** Second Life metres, with the outward normal of the
/// face it lies on.
///
/// A subject with no points is read by its silhouette as a whole. Points are for
/// a subject whose parts answer different questions — the translucent box rack,
/// whose bands either side of the waterline are each a cell — and the normal is
/// what lets a rule see that a face is edge-on to the eye, where a sample is a
/// smear of the face and its neighbour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubjectPoint {
    /// What the point is, for a failure line: `"sunk above"`.
    pub label: &'static str,
    /// Where it is, relative to the subject's origin.
    pub at: Vec3,
    /// The outward normal of the face it lies on.
    pub normal: Vec3,
}

/// What a registered subject is, to the matrix: how to recognise its pixels,
/// and the two material facts the expectation rules turn on.
///
/// Its **bounds** are deliberately not declared: the matrix measures them from
/// the geometry tier's CPU tessellation of the same scene, as R0 always has, so
/// a declaration can never drift from the geometry it describes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubjectSignature {
    /// The colour it paints itself in, if it is a marker subject. `None` reads
    /// the subject by difference instead: the frame with it against the same
    /// frame with it hidden.
    pub marker: Option<SubjectMarker>,
    /// Points sampled one by one. Empty reads the silhouette as a whole.
    pub points: &'static [SubjectPoint],
    /// Whether it is see-through, so a wall behind it must show through.
    pub translucent: bool,
    /// Whether it lights itself, so its own brightness need not follow the time
    /// of day the way a lit surface's must.
    pub emissive: bool,
}

/// One context axis: what a subject declares in `RenderScene::applies`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Where the eye is relative to the sea — [`Eye`].
    Eye,
    /// What time of day lights it — [`TimeOfDay`].
    Time,
    /// How it is seen — [`Viewpoint`].
    Viewpoint,
    /// What stands in front of or behind it — [`Layering`].
    Layering,
}

impl Axis {
    /// The axis's name, for a failure line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Eye => "eye",
            Self::Time => "time",
            Self::Viewpoint => "viewpoint",
            Self::Layering => "layering",
        }
    }
}

/// Where the eye stands relative to the sea, with the subject centred on the
/// waterline.
///
/// The offsets are the translucency matrix's own, which is what lets the
/// translucent box rack reproduce that matrix's verdicts from the axis rather
/// than from six hand-staged scenes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eye {
    /// Three metres under the surface.
    Under,
    /// Five centimetres over it — not *on* it: an eye exactly at the level is a
    /// water plane edge-on through the frame's middle, which answers nothing.
    Grazing,
    /// Six metres over it.
    Over,
}

impl Eye {
    /// Every eye.
    pub const ALL: [Self; 3] = [Self::Under, Self::Grazing, Self::Over];

    /// The eye's height over the water level, in metres.
    #[must_use]
    pub const fn offset(self) -> f32 {
        match self {
            Self::Under => -3.0,
            Self::Grazing => 0.05,
            Self::Over => 6.0,
        }
    }

    /// The eye's name, for a failure line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Under => "under",
            Self::Grazing => "grazing",
            Self::Over => "over",
        }
    }
}

/// The time of day the real sky stack is pinned to: one of the four anchors the
/// sky scenes render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    /// The sun three degrees up in the east.
    Sunrise,
    /// The sun eighty degrees up.
    Midday,
    /// The sun three degrees up in the west.
    Sunset,
    /// The sun down, the moon up.
    Midnight,
}

impl TimeOfDay {
    /// Every anchor.
    pub const ALL: [Self; 4] = [Self::Sunrise, Self::Midday, Self::Sunset, Self::Midnight];

    /// The anchor's name, for a failure line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sunrise => "sunrise",
            Self::Midday => "midday",
            Self::Sunset => "sunset",
            Self::Midnight => "midnight",
        }
    }
}

/// How the subject is seen, beyond directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewpoint {
    /// In a mirror: a hero-probe sphere beside it, which must reflect it.
    InMirror,
}

impl Viewpoint {
    /// Every viewpoint.
    pub const ALL: [Self; 1] = [Self::InMirror];

    /// The viewpoint's name, for a failure line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InMirror => "in-mirror",
        }
    }
}

/// What kind of actor a [`Layering`] cell stands beside the subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// An opaque red wall.
    OpaquePrim,
    /// A half-transparent red wall.
    TranslucentPrim,
}

impl ActorKind {
    /// The actor's name, for a failure line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OpaquePrim => "opaque",
            Self::TranslucentPrim => "translucent",
        }
    }
}

/// A red actor placed relative to the subject and the eye.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layering {
    /// Between the eye and the subject.
    InFrontOf(ActorKind),
    /// Beyond the subject, filling what is behind it.
    Behind(ActorKind),
}

impl Layering {
    /// Every layering.
    pub const ALL: [Self; 4] = [
        Self::InFrontOf(ActorKind::OpaquePrim),
        Self::InFrontOf(ActorKind::TranslucentPrim),
        Self::Behind(ActorKind::OpaquePrim),
        Self::Behind(ActorKind::TranslucentPrim),
    ];

    /// The layering's name, for a failure line.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::InFrontOf(kind) => format!("{} in front", kind.name()),
            Self::Behind(kind) => format!("{} behind", kind.name()),
        }
    }
}

/// A render feature switched on for a cell that the bare rig leaves off.
///
/// Not an axis a subject declares: a toggle only means something beside the
/// context it acts in (fog with an eye under the sea), so toggles appear in the
/// curated pairs, each beside the same cell untoggled — and the two frames must
/// differ, or the toggle did nothing and the cell proved nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    /// The underwater fog post-process.
    UnderwaterFog,
}

impl Toggle {
    /// The toggle's name, for a failure line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::UnderwaterFog => "underwater fog",
        }
    }
}

/// One cell's contexts: at most one value per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContextSet {
    /// Where the eye is relative to the sea; `None` stages no sea.
    pub eye: Option<Eye>,
    /// The sky's time of day; `None` stages no sky.
    pub time: Option<TimeOfDay>,
    /// How the subject is seen; `None` is directly.
    pub viewpoint: Option<Viewpoint>,
    /// The actor beside it; `None` stages none.
    pub layering: Option<Layering>,
    /// A render feature switched on; `None` is the bare rig.
    pub toggle: Option<Toggle>,
}

impl ContextSet {
    /// No contexts: the subject on the bare rig, as R0 captures it.
    pub const NONE: Self = Self {
        eye: None,
        time: None,
        viewpoint: None,
        layering: None,
        toggle: None,
    };

    /// Whether no context is set.
    #[must_use]
    pub fn is_bare(self) -> bool {
        self == Self::NONE
    }

    /// The cell's name, for a failure line: its set values joined, or `bare`.
    #[must_use]
    pub fn name(self) -> String {
        let mut parts = Vec::new();
        if let Some(eye) = self.eye {
            parts.push(format!("eye {}", eye.name()));
        }
        if let Some(time) = self.time {
            parts.push(time.name().to_owned());
        }
        if let Some(viewpoint) = self.viewpoint {
            parts.push(viewpoint.name().to_owned());
        }
        if let Some(layering) = self.layering {
            parts.push(layering.name());
        }
        if let Some(toggle) = self.toggle {
            parts.push(toggle.name().to_owned());
        }
        if parts.is_empty() {
            "bare".to_owned()
        } else {
            parts.join(" + ")
        }
    }

    /// Every single-axis cell of `axis` — the R1 sweep's cells for one axis.
    #[must_use]
    pub fn along(axis: Axis) -> Vec<Self> {
        match axis {
            Axis::Eye => Eye::ALL
                .into_iter()
                .map(|eye| Self {
                    eye: Some(eye),
                    ..Self::NONE
                })
                .collect(),
            Axis::Time => TimeOfDay::ALL
                .into_iter()
                .map(|time| Self {
                    time: Some(time),
                    ..Self::NONE
                })
                .collect(),
            Axis::Viewpoint => Viewpoint::ALL
                .into_iter()
                .map(|viewpoint| Self {
                    viewpoint: Some(viewpoint),
                    ..Self::NONE
                })
                .collect(),
            Axis::Layering => Layering::ALL
                .into_iter()
                .map(|layering| Self {
                    layering: Some(layering),
                    ..Self::NONE
                })
                .collect(),
        }
    }

    /// This cell with `other`'s set values laid over it — how a pair of axes is
    /// one cell. `None` when both set the same axis, which is not one cell.
    #[must_use]
    pub fn with(self, other: Self) -> Option<Self> {
        /// One slot of the merge: either side, never both.
        const fn slot<T: Copy>(a: Option<T>, b: Option<T>) -> Result<Option<T>, ()> {
            match (a, b) {
                (Some(_), Some(_)) => Err(()),
                (value @ Some(_), None) | (None, value) => Ok(value),
            }
        }
        Some(Self {
            eye: slot(self.eye, other.eye).ok()?,
            time: slot(self.time, other.time).ok()?,
            viewpoint: slot(self.viewpoint, other.viewpoint).ok()?,
            layering: slot(self.layering, other.layering).ok()?,
            toggle: slot(self.toggle, other.toggle).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Axis, ContextSet, Eye, TimeOfDay};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_pair_of_axes_merges_into_one_cell() {
        let eye = ContextSet {
            eye: Some(Eye::Under),
            ..ContextSet::NONE
        };
        let time = ContextSet {
            time: Some(TimeOfDay::Midnight),
            ..ContextSet::NONE
        };
        assert_eq!(
            eye.with(time),
            Some(ContextSet {
                eye: Some(Eye::Under),
                time: Some(TimeOfDay::Midnight),
                ..ContextSet::NONE
            })
        );
        assert_eq!(eye.with(eye), None, "one axis set twice is not one cell");
    }

    #[test]
    fn each_axis_yields_one_cell_per_value() {
        assert_eq!(ContextSet::along(Axis::Eye).len(), 3);
        assert_eq!(ContextSet::along(Axis::Time).len(), 4);
        assert_eq!(ContextSet::along(Axis::Viewpoint).len(), 1);
        assert_eq!(ContextSet::along(Axis::Layering).len(), 4);
        assert!(
            ContextSet::along(Axis::Layering)
                .iter()
                .all(|cell| cell.layering.is_some() && cell.eye.is_none())
        );
    }

    #[test]
    fn a_cell_names_its_contexts() {
        assert_eq!(ContextSet::NONE.name(), "bare");
        let cell = ContextSet {
            eye: Some(Eye::Over),
            time: Some(TimeOfDay::Sunset),
            ..ContextSet::NONE
        };
        assert_eq!(cell.name(), "eye over + sunset");
    }
}
