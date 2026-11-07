//! What a script senses of the region (`server-lsl-lib-detection-sensors`):
//! the objects and avatars a sweep weighs, and the surfaces a ray crosses.
//! Which of them a script is told of is the runtime's (`vm::sensing`); this
//! only says what is there.
//!
//! A ray meets a prim as its box — its scale, turned as it is turned — and an
//! avatar as a box of an average avatar's size about its position, which is
//! exact for the cubes, walls and floors scripted content casts at and an
//! approximation of a sphere or a body; the ground by its heightfield.

use sl_lsl_runtime::vm::CallerId;
use sl_lsl_runtime::vm::sensing::{HitKind, RayHit, Sensed};
use sl_proto::{Object, RegionLocalObjectId, prim_flags};
use sl_types::lsl::{Rotation, Vector};

use super::RegionHost;

/// Half an avatar's box, in metres: an average avatar's width, depth and
/// height halved.
const AVATAR_HALF: Vector = Vector {
    x: 0.225,
    y: 0.3,
    z: 0.95,
};

/// The edge of a region, in metres.
const REGION_EDGE: f32 = 256.0;

/// How finely a ray samples the ground, in metres.
const GROUND_STEP: f32 = 0.1;

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// The identity rotation.
const IDENTITY: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// `a - b`.
fn sub(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// `a + b * t`.
const fn along(a: &Vector, b: &Vector, t: f32) -> Vector {
    Vector {
        x: b.x.mul_add(t, a.x),
        y: b.y.mul_add(t, a.y),
        z: b.z.mul_add(t, a.z),
    }
}

/// `v` turned by `r`.
fn rotate(v: &Vector, r: &Rotation) -> Vector {
    // v' = v + 2s(u × v) + 2u × (u × v), u the rotation's vector part.
    let (ux, uy, uz, s) = (r.x, r.y, r.z, r.s);
    let cx = uy * v.z - uz * v.y;
    let cy = uz * v.x - ux * v.z;
    let cz = ux * v.y - uy * v.x;
    let ccx = uy * cz - uz * cy;
    let ccy = uz * cx - ux * cz;
    let ccz = ux * cy - uy * cx;
    Vector {
        x: 2.0f32.mul_add(s.mul_add(cx, ccx), v.x),
        y: 2.0f32.mul_add(s.mul_add(cy, ccy), v.y),
        z: 2.0f32.mul_add(s.mul_add(cz, ccz), v.z),
    }
}

/// `r` undone.
const fn inverse(r: &Rotation) -> Rotation {
    Rotation {
        x: -r.x,
        y: -r.y,
        z: -r.z,
        s: r.s,
    }
}

/// `local` then `root`: a child's turn in the region (Linden's
/// `local * root`).
fn then(local: &Rotation, root: &Rotation) -> Rotation {
    let (a, b) = (root, local);
    Rotation {
        x: a.s * b.x + a.x * b.s + a.y * b.z - a.z * b.y,
        y: a.s * b.y - a.x * b.z + a.y * b.s + a.z * b.x,
        z: a.s * b.z + a.x * b.y - a.y * b.x + a.z * b.s,
        s: a.s * b.s - a.x * b.x - a.y * b.y - a.z * b.z,
    }
}

/// Where a segment meets a box: the distance along it (0 to 1), the point's
/// normal in the box's frame, and whether the segment starts inside.
fn box_hit(start: &Vector, delta: &Vector, half: &Vector) -> Option<(f32, Vector, bool)> {
    let mut enter = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    let mut enter_axis = (0_usize, 0.0_f32);
    let mut exit_axis = (0_usize, 0.0_f32);
    for (axis, (origin, direction, extent)) in [
        (start.x, delta.x, half.x),
        (start.y, delta.y, half.y),
        (start.z, delta.z, half.z),
    ]
    .into_iter()
    .enumerate()
    {
        if direction.abs() < f32::EPSILON {
            if origin.abs() > extent {
                return None;
            }
            continue;
        }
        let (near, far) = (
            (-extent - origin) / direction,
            (extent - origin) / direction,
        );
        let (low, high, sign) = if near < far {
            (near, far, -1.0)
        } else {
            (far, near, 1.0)
        };
        if low > enter {
            enter = low;
            enter_axis = (axis, sign);
        }
        if high < exit {
            exit = high;
            exit_axis = (axis, -sign);
        }
    }
    if enter > exit || exit < 0.0 || enter > 1.0 {
        return None;
    }
    let normal_of = |(axis, sign): (usize, f32)| match axis {
        0 => Vector {
            x: sign,
            y: 0.0,
            z: 0.0,
        },
        1 => Vector {
            x: 0.0,
            y: sign,
            z: 0.0,
        },
        _ => Vector {
            x: 0.0,
            y: 0.0,
            z: sign,
        },
    };
    if enter >= 0.0 {
        return Some((enter, normal_of(enter_axis), false));
    }
    (exit <= 1.0).then(|| (exit, normal_of(exit_axis), true))
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's methods are split by library tranche across the `scripts` modules"
)]
impl RegionHost<'_> {
    /// A prim's centre and turn in the region.
    fn placed(&self, prim: &Object) -> (Vector, Rotation) {
        if prim.parent_id.0 == 0 {
            return (prim.motion.position.clone(), prim.motion.rotation.clone());
        }
        let Some(root) = self.world.objects.get(prim.parent_id) else {
            return (prim.motion.position.clone(), prim.motion.rotation.clone());
        };
        let offset = rotate(&prim.motion.position, &root.motion.rotation);
        (
            along(&root.motion.position, &offset, 1.0),
            then(&prim.motion.rotation, &root.motion.rotation),
        )
    }

    /// Whether a script has an instance in the linkset whose root is `root`.
    fn scripted(&self, root: RegionLocalObjectId) -> bool {
        std::iter::once(root)
            .chain(crate::link_sets::children_in_order(self.world, root))
            .filter_map(|local_id| self.world.objects.get(local_id))
            .any(|prim| {
                !self
                    .world
                    .script_instances
                    .scripts_in(prim.full_id)
                    .is_empty()
            })
    }

    /// Every object (by its root, worn ones left out) and avatar of the
    /// region, as a sweep weighs it.
    pub(super) fn sensed(&self) -> Vec<Sensed> {
        let mut sensed: Vec<Sensed> = self
            .world
            .objects
            .iter()
            .filter(|object| object.parent_id.0 == 0)
            .filter(|object| object.pcode == sl_proto::pcode::PRIMITIVE)
            .filter(|object| self.world.objects.attached_to(object.local_id).is_none())
            .map(|object| Sensed {
                key: object.full_id.uuid().to_string(),
                name: self
                    .world
                    .properties_of(object.local_id)
                    .map(|properties| properties.name)
                    .unwrap_or_default(),
                username: String::new(),
                owner: object.owner_id.to_string(),
                group: self
                    .world
                    .properties_of(object.local_id)
                    .and_then(|properties| properties.group)
                    .map_or_else(
                        || uuid::Uuid::nil().to_string(),
                        |group| group.uuid().to_string(),
                    ),
                position: object.motion.position.clone(),
                rotation: object.motion.rotation.clone(),
                velocity: object.motion.velocity.clone(),
                agent: false,
                physical: object.update_flags & prim_flags::USE_PHYSICS != 0,
                scripted: self.scripted(object.local_id),
            })
            .collect();
        for present in self.present {
            let agent = present.agent.uuid();
            let (first, last) = self.name_of(agent);
            let username = if last.eq_ignore_ascii_case("Resident") {
                first.to_lowercase()
            } else {
                format!("{}.{}", first.to_lowercase(), last.to_lowercase())
            };
            let worn_scripted = self
                .world
                .objects
                .worn_by(present.agent)
                .into_iter()
                .any(|root| self.scripted(root));
            sensed.push(Sensed {
                key: agent.to_string(),
                name: format!("{first} {last}"),
                username,
                owner: agent.to_string(),
                group: uuid::Uuid::nil().to_string(),
                position: present.position.clone(),
                rotation: IDENTITY,
                velocity: ZERO,
                agent: true,
                physical: true,
                scripted: worn_scripted,
            });
        }
        sensed
    }

    /// Every surface the segment from `start` to `end` crosses, cut where
    /// it leaves the region, nearest the start first.
    pub(super) fn ray_hits(&self, caller: CallerId, start: &Vector, end: &Vector) -> Vec<RayHit> {
        let mut delta = sub(end, start);
        // Cut where the segment leaves the region.
        let mut reach = 1.0_f32;
        for (origin, direction) in [(start.x, delta.x), (start.y, delta.y)] {
            if direction > 0.0 {
                reach = reach.min((REGION_EDGE - origin) / direction);
            } else if direction < 0.0 {
                reach = reach.min(-origin / direction);
            }
        }
        let reach = reach.clamp(0.0, 1.0);
        delta = Vector {
            x: delta.x * reach,
            y: delta.y * reach,
            z: delta.z * reach,
        };
        let host_prim = self.origin(caller).map(|origin| origin.prim);
        let mut hits: Vec<(f32, RayHit)> = Vec::new();
        for prim in self.world.objects.iter() {
            if prim.pcode != sl_proto::pcode::PRIMITIVE {
                continue;
            }
            let (centre, turn) = self.placed(prim);
            let back = inverse(&turn);
            let local_start = rotate(&sub(start, &centre), &back);
            let local_delta = rotate(&delta, &back);
            let half = Vector {
                x: prim.scale.x / 2.0,
                y: prim.scale.y / 2.0,
                z: prim.scale.z / 2.0,
            };
            let Some((t, normal, inside)) = box_hit(&local_start, &local_delta, &half) else {
                continue;
            };
            let root_id = self.world.root_of(prim.local_id);
            let root = self.world.objects.get(root_id).unwrap_or(prim);
            let children: Vec<RegionLocalObjectId> =
                crate::link_sets::children_in_order(self.world, root_id);
            let link = if children.is_empty() {
                0
            } else if root.local_id == prim.local_id {
                1
            } else {
                children
                    .iter()
                    .position(|child| *child == prim.local_id)
                    .and_then(|at| i32::try_from(at).ok())
                    .map_or(0, |at| at.saturating_add(2))
            };
            hits.push((
                t,
                RayHit {
                    kind: HitKind::Prim {
                        physical: root.update_flags & prim_flags::USE_PHYSICS != 0,
                        phantom: root.update_flags & prim_flags::PHANTOM != 0,
                    },
                    key: prim.full_id.uuid().to_string(),
                    root: root.full_id.uuid().to_string(),
                    link,
                    position: along(start, &delta, t),
                    normal: rotate(&normal, &turn),
                    from_inside: inside,
                    host: host_prim == Some(prim.full_id),
                },
            ));
        }
        for present in self.present {
            let local_start = sub(start, &present.position);
            let Some((t, normal, inside)) = box_hit(&local_start, &delta, &AVATAR_HALF) else {
                continue;
            };
            let key = present.agent.uuid().to_string();
            hits.push((
                t,
                RayHit {
                    kind: HitKind::Agent,
                    key: key.clone(),
                    root: key,
                    link: 0,
                    position: along(start, &delta, t),
                    normal,
                    from_inside: inside,
                    host: false,
                },
            ));
        }
        if let Some((t, normal)) = self.ground_hit(start, &delta) {
            let none = uuid::Uuid::nil().to_string();
            hits.push((
                t,
                RayHit {
                    kind: HitKind::Land,
                    key: none.clone(),
                    root: none,
                    link: 0,
                    position: along(start, &delta, t),
                    normal,
                    from_inside: false,
                    host: false,
                },
            ));
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        hits.into_iter().map(|(_, hit)| hit).collect()
    }

    /// The ground's height at `(x, y)`.
    fn ground(&self, x: f32, y: f32) -> f32 {
        self.core
            .region(self.index)
            .map_or(0.0, |region| region.config.terrain.height_at(x, y))
    }

    /// Where the segment first goes into the ground, and the ground's normal
    /// there: sampled every [`GROUND_STEP`] and halved down to a millimetre.
    fn ground_hit(&self, start: &Vector, delta: &Vector) -> Option<(f32, Vector)> {
        let length = (delta.x * delta.x + delta.y * delta.y + delta.z * delta.z).sqrt();
        if length <= 0.0 {
            return None;
        }
        let above = |t: f32| {
            let at = along(start, delta, t);
            at.z - self.ground(at.x, at.y)
        };
        if above(0.0) < 0.0 {
            return None;
        }
        let steps = (length / GROUND_STEP).ceil().max(1.0);
        let mut previous = 0.0_f32;
        let mut index = 1.0_f32;
        while index <= steps {
            let t = (index / steps).min(1.0);
            if above(t) < 0.0 {
                let (mut low, mut high) = (previous, t);
                while (high - low) * length > 0.001 {
                    let middle = f32::midpoint(low, high);
                    if above(middle) < 0.0 {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                let at = along(start, delta, high);
                let dx = self.ground(at.x + 0.5, at.y) - self.ground(at.x - 0.5, at.y);
                let dy = self.ground(at.x, at.y + 0.5) - self.ground(at.x, at.y - 0.5);
                let norm = (dx * dx + dy * dy + 1.0).sqrt();
                return Some((
                    high,
                    Vector {
                        x: -dx / norm,
                        y: -dy / norm,
                        z: 1.0 / norm,
                    },
                ));
            }
            previous = t;
            index += 1.0;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::lsl::Vector;

    use super::box_hit;

    /// A segment along x into a half-metre cube 5 m away enters its near
    /// face, the normal pointing back; one from inside leaves the far face.
    #[test]
    fn a_segment_meets_a_box_where_it_enters_or_leaves_it() {
        let half = Vector {
            x: 0.25,
            y: 0.25,
            z: 0.25,
        };
        let delta = Vector {
            x: 10.0,
            y: 0.0,
            z: 0.0,
        };
        let start = Vector {
            x: -5.0,
            y: 0.0,
            z: 0.0,
        };
        let (t, normal, inside) = box_hit(&start, &delta, &half).unwrap_or_default_hit();
        assert!((t * 10.0 - 4.75).abs() < 1e-5);
        assert_eq!((normal.x, inside), (-1.0, false));
        let inside_start = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let (t, normal, inside) = box_hit(&inside_start, &delta, &half).unwrap_or_default_hit();
        assert!((t * 10.0 - 0.25).abs() < 1e-5);
        assert_eq!((normal.x, inside), (1.0, true));
    }

    /// A test's default for no hit: never at the distances asserted.
    trait OrDefault {
        /// The hit, or one no assertion above accepts.
        fn unwrap_or_default_hit(self) -> (f32, Vector, bool);
    }

    impl OrDefault for Option<(f32, Vector, bool)> {
        fn unwrap_or_default_hit(self) -> (f32, Vector, bool) {
            self.unwrap_or((
                f32::NAN,
                Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                false,
            ))
        }
    }
}
