//! The rules behind the Features tab's **Reflection Probe** and **Animated
//! Mesh** blocks (`viewer-build-probe-animesh-controls`), kept apart from the
//! widgets in [`crate::edit_params`] so they can be tested without a window.
//!
//! - A probe edit is one [`ProbeChange`] applied to the object's current
//!   [`ReflectionProbe`] ([`apply_probe_change`]) — the reference's
//!   `LLPanelVolume::onCommitProbe`, which reads every control back but only
//!   ever moves the one that was touched. A mirror is always a box: picking a
//!   mirror update type sets the box bit too.
//! - Whether a linkset may be made an animated object
//!   ([`animesh_allowed`]) is the reference's `canBeAnimatedObject` (its
//!   triangle estimate within the region's `AnimatedObjectMaxTris`) plus the
//!   attachment cap a worn one must leave room under
//!   (`canAttachMoreAnimatedObjects`). The caps come from the login's benefits
//!   package and the region's simulator features ([`AnimeshLimits`]).
//!
//! Reference (Firestorm, read-only): `llpanelvolume.cpp`
//! (`getState`, `sendIsReflectionProbe`, `onCommitProbe`,
//! `onCommitAnimatedMeshCheckbox`), `llvovolume.cpp`
//! (`canBeAnimatedObject`), `llagentbenefits.cpp` (`getAnimatedObjectLimit`).

use bevy::prelude::*;
use sl_client_bevy::{ReflectionProbe, ReflectionProbeFlags, SlEvent, SlSessionEvent};

/// The largest probe ambiance (`REFLECTION_PROBE_MAX_AMBIANCE`).
pub(crate) const PROBE_MAX_AMBIANCE: f32 = 100.0;

/// The largest probe near-clip distance, metres
/// (`REFLECTION_PROBE_MAX_CLIP_DISTANCE`).
pub(crate) const PROBE_MAX_NEAR_CLIP: f32 = 1024.0;

/// The `ExtendedMesh` flag that makes a linkset an animated object
/// (`LLExtendedMeshParams::ANIMATED_MESH_ENABLED_FLAG`).
pub(crate) const ANIMATED_MESH_ENABLED_FLAG: u32 = 0x1;

/// What a newly made probe carries — `LLReflectionProbeParams`' defaults: no
/// ambiance, no near clip, a static sphere. The box bit is added for a prim on
/// a linear path when it is made one (Firestorm keeps a box a box).
pub(crate) const PROBE_DEFAULTS: ReflectionProbe = ReflectionProbe {
    ambiance: 0.0,
    clip_distance: 0.0,
    flags: ReflectionProbeFlags::empty(),
};

/// The Probe Volume combo's entries, in the reference's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeVolume {
    /// A spherical influence volume.
    Sphere,
    /// A box influence volume (`FLAG_BOX_VOLUME`).
    Box,
}

impl ProbeVolume {
    /// Every entry, in combo order.
    pub(crate) const ALL: [Self; 2] = [Self::Sphere, Self::Box];

    /// The volume a probe's flags name.
    pub(crate) const fn of(flags: ReflectionProbeFlags) -> Self {
        if flags.contains(ReflectionProbeFlags::BOX_VOLUME) {
            Self::Box
        } else {
            Self::Sphere
        }
    }

    /// The entry's Fluent key.
    pub(crate) const fn label_key(self) -> &'static str {
        match self {
            Self::Sphere => "build-probe-volume-sphere",
            Self::Box => "build-probe-volume-box",
        }
    }
}

/// The Probe Update combo's entries, in the reference's order: how often the
/// probe re-captures and whether it is a realtime mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeUpdate {
    /// Captured rarely, without avatars.
    Static,
    /// Captured more often, with avatars (`FLAG_DYNAMIC`).
    Dynamic,
    /// A realtime mirror of the environment (`FLAG_MIRROR`).
    Mirror,
    /// A realtime mirror of everything, avatars included (`FLAG_DYNAMIC |
    /// FLAG_MIRROR`).
    DynamicMirror,
}

impl ProbeUpdate {
    /// Every entry, in combo order.
    pub(crate) const ALL: [Self; 4] = [
        Self::Static,
        Self::Dynamic,
        Self::Mirror,
        Self::DynamicMirror,
    ];

    /// The entry a probe's flags name.
    pub(crate) const fn of(flags: ReflectionProbeFlags) -> Self {
        match (
            flags.contains(ReflectionProbeFlags::DYNAMIC),
            flags.contains(ReflectionProbeFlags::MIRROR),
        ) {
            (false, false) => Self::Static,
            (true, false) => Self::Dynamic,
            (false, true) => Self::Mirror,
            (true, true) => Self::DynamicMirror,
        }
    }

    /// Whether the entry renders avatars into the probe.
    const fn dynamic(self) -> bool {
        matches!(self, Self::Dynamic | Self::DynamicMirror)
    }

    /// Whether the entry is a realtime mirror.
    pub(crate) const fn mirror(self) -> bool {
        matches!(self, Self::Mirror | Self::DynamicMirror)
    }

    /// The entry's Fluent key.
    pub(crate) const fn label_key(self) -> &'static str {
        match self {
            Self::Static => "build-probe-update-static",
            Self::Dynamic => "build-probe-update-dynamic",
            Self::Mirror => "build-probe-update-mirror",
            Self::DynamicMirror => "build-probe-update-dynamic-mirror",
        }
    }
}

/// One touched probe control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ProbeChange {
    /// The Ambiance spinner.
    Ambiance(f32),
    /// The Near Clip spinner.
    NearClip(f32),
    /// The Probe Volume combo.
    Volume(ProbeVolume),
    /// The Probe Update combo (shown while mirrors are on).
    Update(ProbeUpdate),
    /// The Dynamic checkbox (shown while mirrors are off, which leaves the
    /// mirror bit as it was).
    Dynamic(bool),
}

/// `flags` with `bit` set or cleared.
fn with_bit(
    flags: ReflectionProbeFlags,
    bit: ReflectionProbeFlags,
    on: bool,
) -> ReflectionProbeFlags {
    let others = ReflectionProbeFlags::from_bits(flags.bits() & !bit.bits());
    if on { others | bit } else { others }
}

/// `current` with `change` applied, as the reference's `onCommitProbe` leaves
/// it: the spinners clamped to their ranges, and the box bit set whenever the
/// probe is a mirror (`is_box = shape_type == "Box" || is_mirror`). Every bit
/// the viewer does not name is kept.
pub(crate) fn apply_probe_change(current: ReflectionProbe, change: ProbeChange) -> ReflectionProbe {
    let mut probe = current;
    match change {
        ProbeChange::Ambiance(value) => probe.ambiance = value.clamp(0.0, PROBE_MAX_AMBIANCE),
        ProbeChange::NearClip(value) => probe.clip_distance = value.clamp(0.0, PROBE_MAX_NEAR_CLIP),
        ProbeChange::Volume(volume) => {
            probe.flags = with_bit(
                probe.flags,
                ReflectionProbeFlags::BOX_VOLUME,
                volume == ProbeVolume::Box,
            );
        }
        ProbeChange::Update(update) => {
            probe.flags = with_bit(probe.flags, ReflectionProbeFlags::DYNAMIC, update.dynamic());
            probe.flags = with_bit(probe.flags, ReflectionProbeFlags::MIRROR, update.mirror());
        }
        ProbeChange::Dynamic(on) => {
            probe.flags = with_bit(probe.flags, ReflectionProbeFlags::DYNAMIC, on);
        }
    }
    if probe.flags.contains(ReflectionProbeFlags::MIRROR) {
        probe.flags |= ReflectionProbeFlags::BOX_VOLUME;
    }
    probe
}

/// What decides whether a linkset may be made an animated object, beyond the
/// selection itself being one editable linkset root.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AnimeshFacts {
    /// The linkset's estimated triangle count: the sum of each mesh prim's
    /// finest-level estimate (a plain prim and a mesh whose header is not in
    /// yet count nothing, as in the reference).
    pub(crate) est_triangles: f32,
    /// Whether the linkset already is an animated object.
    pub(crate) animated: bool,
    /// For a worn linkset, how many animated attachments its wearer has on;
    /// `None` for one standing in the world.
    pub(crate) worn_animated: Option<usize>,
}

/// Whether the Animated Mesh checkbox is live for a linkset described by
/// `facts` under `limits` — the reference's `canBeAnimatedObject` and, for a
/// worn linkset not animated yet, `canAttachMoreAnimatedObjects`. Taking the
/// flag *off* is never refused by the attachment cap.
pub(crate) fn animesh_allowed(facts: AnimeshFacts, limits: &AnimeshLimits) -> bool {
    let max_triangles = limits.max_triangles();
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "a triangle cap is a small integer compared with an approximate estimate"
    )]
    let within_budget = facts.est_triangles >= 0.0 && facts.est_triangles <= max_triangles as f32;
    if !within_budget {
        return false;
    }
    match facts.worn_animated {
        Some(worn) if !facts.animated => {
            let limit = usize::try_from(limits.attachment_limit()).unwrap_or(0);
            worn.saturating_add(1) <= limit
        }
        _standing_or_already_animated => true,
    }
}

/// The animated-object caps the viewer was told: the account's from its login
/// benefits package (Second Life) and the agent region's from its simulator
/// features. Either may be missing, which caps at nothing — the reference's
/// `0` when a feature map lacks the entry.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AnimeshLimits {
    /// The account's `animated_object_limit`, when the login sent a benefits
    /// package.
    account_limit: Option<i32>,
    /// The region's `AnimatedObjectMaxTris`.
    region_max_triangles: Option<i32>,
    /// The region's `MaxAgentAnimatedObjectAttachments`.
    region_attachment_limit: Option<i32>,
}

impl AnimeshLimits {
    /// The region's triangle cap for one animated object.
    const fn max_triangles(&self) -> i32 {
        match self.region_max_triangles {
            Some(max) => max,
            None => 0,
        }
    }

    /// How many animated objects the agent may wear: the benefits package's
    /// where the login sent one, else the region's (Firestorm's OpenSim
    /// fallback in `getAnimatedObjectLimit`).
    const fn attachment_limit(&self) -> i32 {
        match (self.account_limit, self.region_attachment_limit) {
            (Some(limit), _) | (None, Some(limit)) => limit,
            (None, None) => 0,
        }
    }
}

/// Follow the login's benefits package and the agent region's simulator
/// features into [`AnimeshLimits`].
pub(crate) fn ingest_animesh_limits(
    mut events: MessageReader<SlEvent>,
    mut limits: ResMut<AnimeshLimits>,
) {
    for event in events.read() {
        let mut next = *limits;
        match &event.0 {
            SlSessionEvent::Account(account) => {
                next.account_limit = account
                    .benefits
                    .as_ref()
                    .map(|benefits| benefits.animated_object_limit);
            }
            SlSessionEvent::SimulatorFeatures(features) => {
                next.region_max_triangles =
                    features.animated_objects.map(|animesh| animesh.max_tris);
                next.region_attachment_limit = features
                    .animated_objects
                    .map(|animesh| animesh.max_agent_attachments);
            }
            _other => {}
        }
        if next != *limits {
            *limits = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnimeshFacts, AnimeshLimits, PROBE_DEFAULTS, ProbeChange, ProbeUpdate, ProbeVolume,
        animesh_allowed, apply_probe_change,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::ReflectionProbeFlags;

    /// Each update-type entry sets exactly its own pair of bits, a mirror
    /// entry forces a box volume, and reading the flags back names the entry
    /// that set them.
    #[test]
    fn an_update_type_sets_its_bits_and_a_mirror_is_a_box() {
        for update in ProbeUpdate::ALL {
            let probe = apply_probe_change(PROBE_DEFAULTS, ProbeChange::Update(update));
            assert_eq!(ProbeUpdate::of(probe.flags), update);
            assert_eq!(
                ProbeVolume::of(probe.flags) == ProbeVolume::Box,
                update.mirror(),
                "{update:?} on a sphere",
            );
        }
        // Back to static from a mirror: the box it was forced to stays.
        let mirror = apply_probe_change(PROBE_DEFAULTS, ProbeChange::Update(ProbeUpdate::Mirror));
        let static_again = apply_probe_change(mirror, ProbeChange::Update(ProbeUpdate::Static));
        assert_eq!(static_again.flags, ReflectionProbeFlags::BOX_VOLUME);
    }

    /// A mirror cannot be made a sphere, while an ordinary probe can; the
    /// Dynamic checkbox leaves the mirror bit alone; an unnamed bit survives
    /// every edit.
    #[test]
    fn the_volume_and_dynamic_controls_keep_what_they_do_not_name() {
        let mirror = apply_probe_change(
            PROBE_DEFAULTS,
            ProbeChange::Update(ProbeUpdate::DynamicMirror),
        );
        let still_box = apply_probe_change(mirror, ProbeChange::Volume(ProbeVolume::Sphere));
        assert_eq!(ProbeVolume::of(still_box.flags), ProbeVolume::Box);

        let boxed = apply_probe_change(PROBE_DEFAULTS, ProbeChange::Volume(ProbeVolume::Box));
        let sphere = apply_probe_change(boxed, ProbeChange::Volume(ProbeVolume::Sphere));
        assert_eq!(sphere.flags, ReflectionProbeFlags::empty());

        let static_mirror = apply_probe_change(mirror, ProbeChange::Dynamic(false));
        assert_eq!(ProbeUpdate::of(static_mirror.flags), ProbeUpdate::Mirror);

        let unknown = ReflectionProbeFlags::from_bits(0x80);
        let marked = sl_client_bevy::ReflectionProbe {
            flags: unknown,
            ..PROBE_DEFAULTS
        };
        let edited = apply_probe_change(marked, ProbeChange::Update(ProbeUpdate::Dynamic));
        assert!(edited.flags.contains(unknown), "{edited:?}");
    }

    /// The spinners clamp to the reference's ranges.
    #[test]
    fn the_spinners_clamp() {
        let probe = apply_probe_change(PROBE_DEFAULTS, ProbeChange::Ambiance(250.0));
        assert!((probe.ambiance - 100.0).abs() < f32::EPSILON);
        let probe = apply_probe_change(probe, ProbeChange::NearClip(-3.0));
        assert!(probe.clip_distance.abs() < f32::EPSILON);
        let probe = apply_probe_change(probe, ProbeChange::NearClip(5000.0));
        assert!((probe.clip_distance - 1024.0).abs() < f32::EPSILON);
    }

    /// The triangle budget: a plain prim (no estimate) fits under any cap,
    /// even none; a mesh over the region's cap, or anything mesh on a region
    /// that states no cap, does not.
    #[test]
    fn the_triangle_budget_gates_the_checkbox() {
        let standing = |est_triangles| AnimeshFacts {
            est_triangles,
            animated: false,
            worn_animated: None,
        };
        let uncapped = AnimeshLimits::default();
        assert!(animesh_allowed(standing(0.0), &uncapped));
        assert!(!animesh_allowed(standing(10.0), &uncapped));
        let capped = AnimeshLimits {
            region_max_triangles: Some(100_000),
            ..AnimeshLimits::default()
        };
        assert!(animesh_allowed(standing(99_999.0), &capped));
        assert!(!animesh_allowed(standing(100_001.0), &capped));
    }

    /// A worn linkset needs room under the attachment cap to become animated —
    /// the account's on Second Life, the region's where the login sent no
    /// package — but can always stop being animated.
    #[test]
    fn the_attachment_cap_gates_only_turning_it_on() {
        let worn = |worn, animated| AnimeshFacts {
            est_triangles: 0.0,
            animated,
            worn_animated: Some(worn),
        };
        let account = AnimeshLimits {
            account_limit: Some(2),
            region_attachment_limit: Some(9),
            ..AnimeshLimits::default()
        };
        assert!(animesh_allowed(worn(1, false), &account));
        assert!(!animesh_allowed(worn(2, false), &account));
        assert!(animesh_allowed(worn(2, true), &account));
        let region = AnimeshLimits {
            region_attachment_limit: Some(1),
            ..AnimeshLimits::default()
        };
        assert!(animesh_allowed(worn(0, false), &region));
        assert!(!animesh_allowed(worn(1, false), &region));
    }
}
