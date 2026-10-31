//! Library tranche: vectors and rotations (`server-lsl-lib-math-rotations`).
//!
//! **Second Life** computes these the way Linden's `llmath` does (the code
//! published in 2007, with `normalize`'s later guard), in float arithmetic
//! with every operation rounded — fitted bit for bit to the
//! `math/rotation-bits` transcript on aditi, whose seven input pairs print
//! the exact mantissa of each result. Eight functions match all seven pairs
//! that way (`llVecMag`, `llVecNorm`, `llRot2Axis`, `llRot2Angle`,
//! `llRot2Fwd`, `llAxisAngle2Rot`, `llRotBetween`, `llAngleBetween`), and
//! `llRot2Euler` five of them, one unit in the last place off in the other
//! two. Where PyOptimizer (`lslbasefuncs.py`) computes in double and rounds
//! once, it is a unit or two off in the last place on most of those. Two
//! functions keep PyOptimizer's double form: `llEuler2Rot`, which no
//! arrangement of Linden's code tried reproduces exactly and whose double
//! form comes closest (within three units in the last place on every pair,
//! and the six printed decimals of `math/rotations` agree), and
//! `llAxes2Rot`, which no probe measured to the bit. The pieces of Linden's
//! code that Second Life uses differ in places from the code that was
//! published — an axis-angle rotation is not renormalised, a zero axis has no
//! direction and Euler angles carry no negative zero — and each such place is
//! named where it is used.
//!
//! **OpenSim** computes each in double by `LSL_Api.cs`'s formulas, which
//! agree with Second Life's for unit input and part over input that is not
//! (book chapter `gridspec/lsl.md`, § Rotations); its `LSL_Rotation` turns
//! `<0, 0, 0, 0>` into the identity as it is made.
//!
//! Every composition here is in Linden's order: `a * b` applies `a` first,
//! the reverse of glam's.

use sl_types::lsl::{Rotation, Vector};

use crate::library::{CallError, PureCtx};
use crate::num::{fsum, to_f32};

/// Linden's `llmath` in float arithmetic — the 2007 code, with the later
/// guard in `normalize` — what Second Life's library is built on.
mod linden {
    use sl_types::lsl::{Rotation, Vector};

    use crate::num::to_f32;

    /// `FP_MAG_THRESHOLD`: a length not above it has no direction.
    const MAG_THRESHOLD: f32 = 1e-7;

    /// `F_APPROXIMATELY_ZERO`.
    const APPROXIMATELY_ZERO: f32 = 0.000_01;

    /// `F_PI`.
    const PI: f32 = core::f32::consts::PI;

    /// `F_TWO_PI`.
    const TWO_PI: f32 = core::f32::consts::TAU;

    /// The zero vector.
    pub(super) const ZERO: Vector = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// The identity rotation.
    pub(super) const IDENTITY: Rotation = Rotation {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        s: 1.0,
    };

    /// `(F32)curve((F64)x)`: a C library function of a float, in double,
    /// rounded back.
    fn library(curve: fn(f64) -> f64, x: f32) -> f32 {
        to_f32(curve(f64::from(x)))
    }

    /// The dot product.
    pub(super) fn dot(a: &Vector, b: &Vector) -> f32 {
        a.x * b.x + a.y * b.y + a.z * b.z
    }

    /// The cross product.
    pub(super) fn cross(a: &Vector, b: &Vector) -> Vector {
        Vector {
            x: a.y * b.z - a.z * b.y,
            y: a.z * b.x - a.x * b.z,
            z: a.x * b.y - a.y * b.x,
        }
    }

    /// Each component times `factor`.
    fn scaled(v: &Vector, factor: f32) -> Vector {
        Vector {
            x: v.x * factor,
            y: v.y * factor,
            z: v.z * factor,
        }
    }

    /// The length (`LLVector3::magVec`).
    pub(super) fn length(v: &Vector) -> f32 {
        dot(v, v).sqrt()
    }

    /// `LLVector3::normVec`: the vector times the inverse of its length, and
    /// that length; the zero vector and 0 for a length not above the
    /// threshold (`llVecNorm(<0, 0, 1e-7>)` is `ZERO_VECTOR` on aditi).
    pub(super) fn normalised(v: &Vector) -> (Vector, f32) {
        let magnitude = length(v);
        if magnitude > MAG_THRESHOLD {
            (scaled(v, 1.0 / magnitude), magnitude)
        } else {
            (ZERO, 0.0)
        }
    }

    /// `LLQuaternion::normalize`: the rotation times the inverse of its
    /// length, unless that length is within a millionth of 1 (the guard
    /// Linden added to `normQuat` later — without it the x90 rotation's
    /// round trip through `llRot2Euler` gives 90.00001, where aditi prints
    /// 90); the identity for a length not above the threshold.
    pub(super) fn normalised_rotation(q: &Rotation) -> Rotation {
        /// `ONE_PART_IN_A_MILLION`.
        const NEAR_ONE: f32 = 0.000_001;
        let magnitude = (q.x * q.x + q.y * q.y + q.z * q.z + q.s * q.s).sqrt();
        if magnitude > MAG_THRESHOLD {
            if (1.0 - magnitude).abs() <= NEAR_ONE {
                return q.clone();
            }
            let inverse = 1.0 / magnitude;
            Rotation {
                x: q.x * inverse,
                y: q.y * inverse,
                z: q.z * inverse,
                s: q.s * inverse,
            }
        } else {
            IDENTITY
        }
    }

    /// The conjugate.
    pub(super) const fn conjugate(q: &Rotation) -> Rotation {
        Rotation {
            x: -q.x,
            y: -q.y,
            z: -q.z,
            s: q.s,
        }
    }

    /// `a * b`, applying `a` first; not renormalised.
    pub(super) fn compose(a: &Rotation, b: &Rotation) -> Rotation {
        Rotation {
            x: b.s * a.x + b.x * a.s + b.y * a.z - b.z * a.y,
            y: b.s * a.y + b.y * a.s + b.z * a.x - b.x * a.z,
            z: b.s * a.z + b.z * a.s + b.x * a.y - b.y * a.x,
            s: b.s * a.s - b.x * a.x - b.y * a.y - b.z * a.z,
        }
    }

    /// `LLQuaternion::getMatrix3`: the rows of the rotation's matrix — its
    /// forward, left and up axes.
    pub(super) fn matrix(q: &Rotation) -> [Vector; 3] {
        let (xx, xy, xz, xw) = (q.x * q.x, q.x * q.y, q.x * q.z, q.x * q.s);
        let (yy, yz, yw) = (q.y * q.y, q.y * q.z, q.y * q.s);
        let (zz, zw) = (q.z * q.z, q.z * q.s);
        [
            Vector {
                x: 1.0 - 2.0 * (yy + zz),
                y: 2.0 * (xy + zw),
                z: 2.0 * (xz - yw),
            },
            Vector {
                x: 2.0 * (xy - zw),
                y: 1.0 - 2.0 * (xx + zz),
                z: 2.0 * (yz + xw),
            },
            Vector {
                x: 2.0 * (xz + yw),
                y: 2.0 * (yz - xw),
                z: 1.0 - 2.0 * (xx + yy),
            },
        ]
    }

    /// `LLMatrix3::orthogonalize`: x normalised, y made perpendicular to it
    /// and normalised, z their cross product.
    pub(super) fn orthogonalised(rows: &[Vector; 3]) -> [Vector; 3] {
        let [x_row, y_row, _] = rows;
        let (x_axis, _) = normalised(x_row);
        let along = scaled(&x_axis, dot(&x_axis, y_row));
        let (y_axis, _) = normalised(&Vector {
            x: y_row.x - along.x,
            y: y_row.y - along.y,
            z: y_row.z - along.z,
        });
        let z_axis = cross(&x_axis, &y_axis);
        [x_axis, y_axis, z_axis]
    }

    /// `LLMatrix3::getEulerAngles`, in double: the pitch from the up row's
    /// x, then roll and yaw from the rest — or, at the gimbal (the pitch's
    /// cosine within 0.005 of zero), no roll and the yaw from the left row.
    pub(super) fn euler_angles(rows: &[Vector; 3]) -> Vector {
        let [forward, left, up] = rows;
        let angle_y = f64::from(up.x.clamp(-1.0, 1.0)).asin();
        let cy = angle_y.cos();
        let (angle_x, angle_z) = if cy.abs() > 0.005 {
            let x = f64::from(to_f32((-f64::from(up.y) / cy).atan2(f64::from(up.z) / cy)));
            let z = f64::from(to_f32(
                (-f64::from(left.x) / cy).atan2(f64::from(forward.x) / cy),
            ));
            (x, z)
        } else {
            (0.0, f64::from(forward.y).atan2(f64::from(left.y)))
        };
        Vector {
            x: to_f32(angle_x),
            y: to_f32(angle_y),
            z: to_f32(angle_z),
        }
    }

    /// The rotation by `angle` about `axis` (`LLQuaternion::setQuat` without
    /// its closing `normQuat`, which Second Life leaves out — all seven
    /// `math/rotation-bits` pairs), or the identity for an axis too short to
    /// normalise, as aditi answers a zero and a `1e-7` axis.
    pub(super) fn axis_angle(angle: f32, axis: &Vector) -> Rotation {
        let (axis, magnitude) = normalised(axis);
        if magnitude == 0.0 {
            return IDENTITY;
        }
        let half = angle * 0.5;
        let (c, s) = (library(f64::cos, half), library(f64::sin, half));
        Rotation {
            x: axis.x * s,
            y: axis.y * s,
            z: axis.z * s,
            s: c,
        }
    }

    /// The angle of a rotation, from 0 to π, and its axis — through `acos`
    /// of `s`, folded past π with the axis turned (`LLQuaternion::
    /// getAngleAxis`). An axis whose sine is below 0.0005 is the zero vector,
    /// a positive one: aditi answers `<0, 0, 0>` for the identity and its
    /// negation, where the published code multiplies the components by one.
    pub(super) fn angle_axis(q: &Rotation) -> (f32, Vector) {
        let cos_a = q.s.clamp(-1.0, 1.0);
        let sin_a = (1.0 - cos_a * cos_a).sqrt();
        let doubled = 2.0 * library(f64::acos, cos_a);
        let (angle, turned) = if doubled > PI {
            (TWO_PI - doubled, true)
        } else {
            (doubled, false)
        };
        if sin_a.abs() < 0.0005 {
            return (angle, ZERO);
        }
        let inverse = 1.0 / sin_a;
        let axis = if turned {
            Vector {
                x: -q.x * inverse,
                y: -q.y * inverse,
                z: -q.z * inverse,
            }
        } else {
            Vector {
                x: q.x * inverse,
                y: q.y * inverse,
                z: q.z * inverse,
            }
        };
        (angle, axis)
    }

    /// `LLQuaternion::shortestArc`: the rotation that turns `a`'s direction
    /// into `b`'s — the identity when either is (nearly) zero or they are
    /// parallel, a half turn about an axis perpendicular to `a` when they
    /// are opposite (about z when `a` lies along x), and otherwise the turn
    /// by `acos` of their dot product about their cross product, made by
    /// [`axis_angle`].
    pub(super) fn shortest_arc(a: &Vector, b: &Vector) -> Rotation {
        let (a, a_length) = normalised(a);
        let (b, b_length) = normalised(b);
        if a_length < APPROXIMATELY_ZERO || b_length < APPROXIMATELY_ZERO {
            return IDENTITY;
        }
        let axis = cross(&a, &b);
        let cos_theta = dot(&a, &b);
        let near = 1.0 - f64::from(APPROXIMATELY_ZERO);
        if f64::from(cos_theta) > near {
            return IDENTITY;
        }
        if f64::from(cos_theta) < -near {
            let projection = scaled(&a, a.x / dot(&a, &a));
            let (orthogonal, orthogonal_length) = normalised(&Vector {
                x: 1.0 - projection.x,
                y: -projection.y,
                z: -projection.z,
            });
            let orthogonal = if orthogonal_length < APPROXIMATELY_ZERO {
                Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                }
            } else {
                orthogonal
            };
            return Rotation {
                x: orthogonal.x,
                y: orthogonal.y,
                z: orthogonal.z,
                s: 0.0,
            };
        }
        axis_angle(library(f64::acos, cos_theta), &axis)
    }
}

/// A vector in double, for OpenSim's formulas and PyOptimizer's.
#[derive(Debug, Clone, Copy, PartialEq)]
struct D3 {
    /// The x component.
    x: f64,
    /// The y component.
    y: f64,
    /// The z component.
    z: f64,
}

impl D3 {
    /// The zero vector.
    const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// The float vector in double.
    fn of(vector: &Vector) -> Self {
        Self {
            x: f64::from(vector.x),
            y: f64::from(vector.y),
            z: f64::from(vector.z),
        }
    }

    /// Each component rounded to a float.
    const fn rounded(self) -> Vector {
        Vector {
            x: to_f32(self.x),
            y: to_f32(self.y),
            z: to_f32(self.z),
        }
    }

    /// Each component rounded to a float and back: OpenSim's `(float)` cast.
    fn float(self) -> Self {
        Self::of(&self.rounded())
    }

    /// The sum of the squares, added left to right (`LSL_Vector.MagSquare`).
    fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// The dot product, added left to right.
    fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// The cross product.
    fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Each component times `factor`.
    fn scaled(self, factor: f64) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        }
    }

    /// `LSL_Vector.Norm`: the vector times the inverse of its length, or the
    /// zero vector for a length of 0.
    fn normalised(self) -> Self {
        let length = self.length_squared().sqrt();
        if length > 0.0 {
            self.scaled(1.0 / length)
        } else {
            Self::ZERO
        }
    }
}

/// A rotation in double, for OpenSim's formulas and PyOptimizer's.
#[derive(Debug, Clone, Copy, PartialEq)]
struct D4 {
    /// The x component.
    x: f64,
    /// The y component.
    y: f64,
    /// The z component.
    z: f64,
    /// The s (real) component.
    s: f64,
}

impl D4 {
    /// The identity rotation.
    const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        s: 1.0,
    };

    /// The float rotation in double.
    fn of(rotation: &Rotation) -> Self {
        Self {
            x: f64::from(rotation.x),
            y: f64::from(rotation.y),
            z: f64::from(rotation.z),
            s: f64::from(rotation.s),
        }
    }

    /// The float rotation as OpenSim holds it: `<0, 0, 0, 0>` is made the
    /// identity (`LSL_Types.Quaternion`'s constructor), so its functions
    /// never see it — `llRot2Fwd(<0, 0, 0, 0>)` is `<1, 0, 0>` there too.
    fn opensim(rotation: &Rotation) -> Self {
        Self::of(rotation).or_identity()
    }

    /// Each component rounded to a float.
    const fn rounded(self) -> Rotation {
        Rotation {
            x: to_f32(self.x),
            y: to_f32(self.y),
            z: to_f32(self.z),
            s: to_f32(self.s),
        }
    }

    /// The rotation, or the identity for `<0, 0, 0, 0>`.
    fn or_identity(self) -> Self {
        if self.x == 0.0 && self.y == 0.0 && self.z == 0.0 && self.s == 0.0 {
            Self::IDENTITY
        } else {
            self
        }
    }

    /// The sum of the squares, added left to right.
    fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.s * self.s
    }

    /// Each component times `factor`.
    fn scaled(self, factor: f64) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
            s: self.s * factor,
        }
    }

    /// `LSL_Rotation.Normalize`: the unit rotation, or the identity for one
    /// whose squared length is below `float.Epsilon`.
    fn normalised(self) -> Self {
        /// `float.Epsilon`, the least positive float.
        const EPSILON: f64 = 1.401_298_464_324_817e-45;
        let squared = self.length_squared();
        if squared < EPSILON {
            Self::IDENTITY
        } else {
            self.scaled(1.0 / squared.sqrt())
        }
    }
}

/// `llVecMag`: the vector's length (`<3, 4, 0>` is 5) — in float on Second
/// Life, in double on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_vec_mag<C: PureCtx>(ctx: &mut C, vector: Vector) -> Result<f32, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        to_f32(D3::of(&vector).length_squared().sqrt())
    } else {
        linden::length(&vector)
    })
}

/// `llVecDist`: the distance between two points — the length of their
/// difference, in float on Second Life (`dist_vec`; not measured to the
/// bit, and `llVecMag`'s float form matches it), in double on OpenSim.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_vec_dist<C: PureCtx>(ctx: &mut C, a: Vector, b: Vector) -> Result<f32, CallError> {
    if ctx.flavour().is_opensim() {
        let (a, b) = (D3::of(&a), D3::of(&b));
        let difference = D3 {
            x: a.x - b.x,
            y: a.y - b.y,
            z: a.z - b.z,
        };
        return Ok(to_f32(difference.length_squared().sqrt()));
    }
    Ok(linden::length(&Vector {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }))
}

/// `llVecNorm`: the unit vector along the argument, `ZERO_VECTOR` for the
/// zero vector on every grid (`errors/math` case 15); a vector of length
/// `1e-7` is too short on Second Life (zero) and normalised on OpenSim
/// (`math/rotations`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_vec_norm<C: PureCtx>(ctx: &mut C, vector: Vector) -> Result<Vector, CallError> {
    Ok(if ctx.flavour().is_opensim() {
        D3::of(&vector).normalised().rounded()
    } else {
        linden::normalised(&vector).0
    })
}

/// `llEuler2Rot`: the rotation by `angles.x` about x, then `angles.y` about
/// y, then `angles.z` about z. On Second Life the half-angle product is
/// taken in double (PyOptimizer `llEuler2Rot`, see the module) and its sign
/// chosen so the largest of three diagonal terms of its matrix decides it, so
/// a turn of `<400°, −200°, 1000°>` comes out as aditi's `<−0.549, 0.747,
/// 0.153, 0.342>`; OpenSim leaves the product's sign, the negation of that
/// (`math/rotations`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_euler2_rot<C: PureCtx>(ctx: &mut C, angles: Vector) -> Result<Rotation, CallError> {
    let v = D3::of(&angles);
    let half = |angle: f64| ((angle * 0.5).cos(), (angle * 0.5).sin());
    let ((c0, s0), (c1, s1), (c2, s2)) = (half(v.x), half(v.y), half(v.z));
    if ctx.flavour().is_opensim() {
        // `LSL_Api.llEuler2Rot`'s names: its c1/s1 are x's, c2/s2 y's, c3/s3
        // z's.
        let (c1c2, s1s2, c1s2, s1c2) = (c0 * c1, s0 * s1, c0 * s1, s0 * c1);
        return Ok(D4 {
            x: s1c2 * c2 + c1s2 * s2,
            y: c1s2 * c2 - s1c2 * s2,
            z: s1s2 * c2 + c1c2 * s2,
            s: c1c2 * c2 - s1s2 * s2,
        }
        .rounded());
    }
    let rotation = D4 {
        x: s0 * c1 * c2 + c0 * s1 * s2,
        y: c0 * s1 * c2 - s0 * c1 * s2,
        z: c0 * c1 * s2 + s0 * s1 * c2,
        s: c0 * c1 * c2 - s0 * s1 * s2,
    }
    .rounded();
    let (c0, s0, c1, s1, c2, s2) = (
        v.x.cos(),
        v.x.sin(),
        v.y.cos(),
        v.y.sin(),
        v.z.cos(),
        v.z.sin(),
    );
    let d1 = c1 * c2;
    let d2 = c0 * c2 - s0 * s1 * s2;
    let d3 = c0 * c1;
    let decider = if d1 + d2 + d3 > 0.0 {
        rotation.s
    } else if d1 < d3 && d3 > d2 {
        rotation.z
    } else if d2 > d1 {
        rotation.y
    } else {
        rotation.x
    };
    Ok(if decider < 0.0 {
        Rotation {
            x: -rotation.x,
            y: -rotation.y,
            z: -rotation.z,
            s: -rotation.s,
        }
    } else {
        rotation
    })
}

/// `llRot2Euler`: the angles `llEuler2Rot` turns back into the rotation, in
/// radians. On Second Life Linden's way: the rotation normalised, its
/// matrix orthogonalised, the angles read off it — at the gimbal x is folded
/// into z, so `<10°, 90°, 20°>` comes back `<0, 90, 30>` (`math/rotations`).
/// A zero angle is always a positive one there (the identity, its negation
/// and `<0, 0, 0.707, −0.707>` print `0.000000` on aditi, where the matrix
/// formula gives `-0.0`). OpenSim's formula tests the pole against `0.4999`
/// of the squared length and arranges the same terms its own way.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot2_euler<C: PureCtx>(ctx: &mut C, rotation: Rotation) -> Result<Vector, CallError> {
    if ctx.flavour().is_opensim() {
        return Ok(opensim_rot2_euler(D4::opensim(&rotation)).rounded());
    }
    let rows = linden::matrix(&linden::normalised_rotation(&rotation));
    let angles = linden::euler_angles(&linden::orthogonalised(&rows));
    Ok(Vector {
        x: angles.x + 0.0,
        y: angles.y + 0.0,
        z: angles.z + 0.0,
    })
}

/// OpenSim's `llRot2Euler`, in double.
fn opensim_rot2_euler(q: D4) -> D3 {
    let sqw = q.s * q.s;
    let sqx = q.x * q.x;
    let sqy = q.z * q.z;
    let sqz = q.y * q.y;
    let unit = sqx + sqy + sqz + sqw;
    let test = q.x * q.z + q.y * q.s;
    if test > 0.4999 * unit {
        return D3 {
            x: 0.0,
            y: core::f64::consts::FRAC_PI_2,
            z: 2.0 * q.x.atan2(q.s),
        };
    }
    if test < -0.4999 * unit {
        return D3 {
            x: 0.0,
            y: -core::f64::consts::FRAC_PI_2,
            z: -2.0 * q.x.atan2(q.s),
        };
    }
    D3 {
        x: (2.0 * q.x * q.s - 2.0 * q.z * q.y).atan2(-sqx + sqy - sqz + sqw),
        y: (2.0 * test / unit).asin(),
        z: (2.0 * q.z * q.s - 2.0 * q.x * q.y).atan2(sqx - sqy - sqz + sqw),
    }
}

/// `llAxisAngle2Rot`: the rotation by `angle` radians about `axis`, which
/// is normalised first. On Second Life an axis too short to normalise turns
/// by nothing (the identity), and the result is not renormalised; OpenSim's
/// zero axis gives `<0, 0, 0, cos(angle / 2)>`, not a unit rotation, and
/// normalises a `1e-7` one (`math/rotations`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_axis_angle2_rot<C: PureCtx>(
    ctx: &mut C,
    axis: Vector,
    angle: f32,
) -> Result<Rotation, CallError> {
    if !ctx.flavour().is_opensim() {
        return Ok(linden::axis_angle(angle, &axis));
    }
    let angle = f64::from(angle);
    let axis = D3::of(&axis).normalised();
    let (c, s) = ((angle * 0.5).cos(), (angle * 0.5).sin());
    Ok(D4 {
        x: axis.x * s,
        y: axis.y * s,
        z: axis.z * s,
        s: c,
    }
    .rounded())
}

/// `llRot2Axis`: the rotation's axis, normalised, turned when its angle is
/// past π; the zero vector for the identity, its negation, `<0, 0, 0, 2>`
/// and `<0, 0, 0, 0>` on every grid (`math/rotations`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot2_axis<C: PureCtx>(ctx: &mut C, rotation: Rotation) -> Result<Vector, CallError> {
    if !ctx.flavour().is_opensim() {
        return Ok(linden::angle_axis(&linden::normalised_rotation(&rotation)).1);
    }
    let r = D4::opensim(&rotation).normalised();
    let s = (1.0 - r.s * r.s).sqrt();
    if s < 1e-8 {
        return Ok(D3::ZERO.rounded());
    }
    let inverse = if r.s < 0.0 { -1.0 / s } else { 1.0 / s };
    Ok(D3 {
        x: r.x * inverse,
        y: r.y * inverse,
        z: r.z * inverse,
    }
    .rounded())
}

/// `llRot2Angle`: the rotation's angle in radians, from 0 to π — 0 for the
/// identity, its negation, `<0, 0, 0, 2>` and `<0, 0, 0, 0>`
/// (`math/rotations`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot2_angle<C: PureCtx>(ctx: &mut C, rotation: Rotation) -> Result<f32, CallError> {
    if !ctx.flavour().is_opensim() {
        return Ok(linden::angle_axis(&linden::normalised_rotation(&rotation)).0);
    }
    let r = D4::opensim(&rotation).normalised();
    let angle = 2.0 * r.s.acos();
    Ok(to_f32(if angle > core::f64::consts::PI {
        core::f64::consts::TAU - angle
    } else {
        angle
    }))
}

/// `llAngleBetween`: the angle of the rotation from `a` to `b`, from 0 to
/// π; 0 between a rotation and its negation or a multiple of it, and against
/// `<0, 0, 0, 0>` (`math/rotations`). On Second Life the angle, as
/// `llRot2Angle` takes it, of `a` composed with the conjugate of `b`, each
/// normalised first and the product not.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_angle_between<C: PureCtx>(
    ctx: &mut C,
    a: Rotation,
    b: Rotation,
) -> Result<f32, CallError> {
    if !ctx.flavour().is_opensim() {
        let a = linden::normalised_rotation(&a);
        let b = linden::normalised_rotation(&b);
        return Ok(linden::angle_axis(&linden::compose(&a, &linden::conjugate(&b))).0);
    }
    let (a, b) = (D4::opensim(&a), D4::opensim(&b));
    let product = a.length_squared() * b.length_squared();
    if product == 0.0 {
        return Ok(0.0);
    }
    let dot = a.x * b.x + a.y * b.y + a.z * b.z + a.s * b.s;
    let quotient = (dot * dot) / product;
    if quotient >= 1.0 {
        return Ok(0.0);
    }
    Ok(to_f32((2.0 * quotient - 1.0).acos()))
}

/// OpenSim's rotation for its `llRot2Fwd` family: normalised when its
/// squared length is more than `0.000001` off 1.
fn opensim_axes_rotation(rotation: &Rotation) -> D4 {
    let r = D4::opensim(rotation);
    let squared = r.length_squared();
    if (1.0 - squared).abs() > 0.000_001 {
        r.scaled(1.0 / squared.sqrt())
    } else {
        r
    }
}

/// The rows of a rotation's matrix on Second Life: the rotation normalised
/// (the identity for `<0, 0, 0, 0>`), so one of any length gives unit axes.
fn sl_axes(rotation: &Rotation) -> [Vector; 3] {
    linden::matrix(&linden::normalised_rotation(rotation))
}

/// `llRot2Fwd`: the x axis turned by the rotation.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot2_fwd<C: PureCtx>(ctx: &mut C, rotation: Rotation) -> Result<Vector, CallError> {
    if ctx.flavour().is_opensim() {
        let r = opensim_axes_rotation(&rotation);
        return Ok(D3 {
            x: r.x * r.x - r.y * r.y - r.z * r.z + r.s * r.s,
            y: 2.0 * (r.x * r.y + r.z * r.s),
            z: 2.0 * (r.x * r.z - r.y * r.s),
        }
        .rounded());
    }
    let [forward, _, _] = sl_axes(&rotation);
    Ok(forward)
}

/// `llRot2Left`: the y axis turned by the rotation.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot2_left<C: PureCtx>(ctx: &mut C, rotation: Rotation) -> Result<Vector, CallError> {
    if ctx.flavour().is_opensim() {
        let r = opensim_axes_rotation(&rotation);
        return Ok(D3 {
            x: 2.0 * (r.x * r.y - r.z * r.s),
            y: -r.x * r.x + r.y * r.y - r.z * r.z + r.s * r.s,
            z: 2.0 * (r.x * r.s + r.y * r.z),
        }
        .rounded());
    }
    let [_, left, _] = sl_axes(&rotation);
    Ok(left)
}

/// `llRot2Up`: the z axis turned by the rotation.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot2_up<C: PureCtx>(ctx: &mut C, rotation: Rotation) -> Result<Vector, CallError> {
    if ctx.flavour().is_opensim() {
        let r = opensim_axes_rotation(&rotation);
        return Ok(D3 {
            x: 2.0 * (r.x * r.z + r.y * r.s),
            y: 2.0 * (-r.x * r.s + r.y * r.z),
            z: -r.x * r.x - r.y * r.y + r.z * r.z + r.s * r.s,
        }
        .rounded());
    }
    let [_, _, up] = sl_axes(&rotation);
    Ok(up)
}

/// `llRotBetween`: the shortest rotation that turns the direction of `a`
/// into the direction of `b`; the identity when either is zero or they are
/// parallel, and a half turn about an axis perpendicular to both when they
/// are opposite — x to −x is `<0, 0, 1, 0>`, z to −x `<0, −0.707, 0, 0.707>`
/// on every grid (`math/rotations`). Second Life's is Linden's
/// `shortestArc`; OpenSim's goes through `acos` of the dot product too, in
/// double.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_rot_between<C: PureCtx>(
    ctx: &mut C,
    a: Vector,
    b: Vector,
) -> Result<Rotation, CallError> {
    if !ctx.flavour().is_opensim() {
        return Ok(linden::shortest_arc(&a, &b));
    }
    Ok(opensim_rot_between(D3::of(&a), D3::of(&b)).rounded())
}

/// OpenSim's `llRotBetween`, in double.
fn opensim_rot_between(a: D3, b: D3) -> D4 {
    if a.length_squared() < 1e-12 || b.length_squared() < 1e-12 {
        return D4::IDENTITY;
    }
    let a = a.normalised();
    let b = b.normalised();
    let axis = a.cross(b);
    let cos_theta = a.dot(b);
    if cos_theta > 0.999_99 {
        return D4::IDENTITY;
    }
    if cos_theta < -0.999_99 {
        let along = a.scaled(a.x / a.dot(a));
        let mut orthogonal = D3 {
            x: 1.0 - along.x,
            y: -along.y,
            z: -along.z,
        };
        if orthogonal.length_squared() < 1e-12 {
            orthogonal = D3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            };
        }
        let orthogonal = orthogonal.float();
        return D4 {
            x: orthogonal.x,
            y: orthogonal.y,
            z: orthogonal.z,
            s: 0.0,
        };
    }
    // `(LSL_Float)Math.Acos(cos_theta) * 0.5f`: the half angle in double.
    let theta = cos_theta.acos() * 0.5;
    let axis = axis.normalised();
    let (s, t) = (theta.cos(), theta.sin());
    D4 {
        x: axis.x * t,
        y: axis.y * t,
        z: axis.z * t,
        s,
    }
}

/// `llAxes2Rot`: the rotation whose forward, left and up axes are the three
/// vectors. When the trace is positive the result is **not normalised** —
/// x of length 2 gives `<0, 0, 0, 1.118>`, skewed axes `<0, 0, −0.25, 1>` —
/// and otherwise it is (PyOptimizer `llAxes2Rot`, in double); three zero
/// vectors give `<1, 0, 0, 0>` on Second Life and `<0, 0, 0, 0.5>` on
/// OpenSim, whose trace test is `>= 0` and which never normalises
/// (`math/rotations`, `errors/math` case 15).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_axes2_rot<C: PureCtx>(
    ctx: &mut C,
    forward: Vector,
    left: Vector,
    up: Vector,
) -> Result<Rotation, CallError> {
    let (f, l, u) = (D3::of(&forward), D3::of(&left), D3::of(&up));
    if ctx.flavour().is_opensim() {
        return Ok(opensim_axes2_rot(f, l, u).rounded());
    }
    let trace = fsum(&[f.x, l.y, u.z]);
    if trace > 0.0 {
        let r = (1.0 + trace).sqrt();
        let s = 0.5 / r;
        return Ok(D4 {
            x: s * (l.z - u.y),
            y: s * (u.x - f.z),
            z: s * (f.y - l.x),
            s: r * 0.5,
        }
        .rounded());
    }
    let q = if l.y <= f.x && f.x >= u.z {
        let r = (1.0 + f.x - l.y - u.z).sqrt();
        let s = 0.5 / r;
        D4 {
            x: r * 0.5,
            y: s * (f.y + l.x),
            z: s * (u.x + f.z),
            s: s * (l.z - u.y),
        }
    } else if f.x <= l.y && l.y >= u.z {
        let r = (1.0 - f.x + l.y - u.z).sqrt();
        let s = 0.5 / r;
        D4 {
            x: s * (f.y + l.x),
            y: r * 0.5,
            z: s * (l.z + u.y),
            s: s * (u.x - f.z),
        }
    } else {
        let r = (1.0 - f.x - l.y + u.z).sqrt();
        let s = 0.5 / r;
        D4 {
            x: s * (u.x + f.z),
            y: s * (l.z + u.y),
            z: r * 0.5,
            s: s * (f.y - l.x),
        }
    };
    let q = q.or_identity();
    let length = fsum(&[q.x * q.x, q.y * q.y, q.z * q.z, q.s * q.s]).sqrt();
    Ok(q.scaled(1.0 / length).rounded())
}

/// OpenSim's `llAxes2Rot`, in double.
fn opensim_axes2_rot(f: D3, l: D3, u: D3) -> D4 {
    let trace = f.x + l.y + u.z + 1.0;
    if trace >= 1.0 {
        let s = 0.5 / trace.sqrt();
        return D4 {
            x: (l.z - u.y) * s,
            y: (u.x - f.z) * s,
            z: (f.y - l.x) * s,
            s: 0.25 / s,
        };
    }
    let max = if l.y > u.z { l.y } else { u.z };
    if max < f.x {
        let root = (f.x - (l.y + u.z) + 1.0).sqrt();
        let s = 0.5 / root;
        D4 {
            x: root * 0.5,
            y: (f.y + l.x) * s,
            z: (u.x + f.z) * s,
            s: (l.z - u.y) * s,
        }
    } else if l.y >= u.z {
        // `max == left.y`: left's y is the larger of the two, or equal.
        let root = (l.y - (u.z + f.x) + 1.0).sqrt();
        let s = 0.5 / root;
        D4 {
            x: (f.y + l.x) * s,
            y: root * 0.5,
            z: (l.z + u.y) * s,
            s: (u.x - f.z) * s,
        }
    } else {
        let root = (u.z - (f.x + l.y) + 1.0).sqrt();
        let s = 0.5 / root;
        D4 {
            x: (u.x + f.z) * s,
            y: (l.z + u.y) * s,
            z: root * 0.5,
            s: (f.y - l.x) * s,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::bare::sl;
    use crate::probe::Line;

    /// A float as `math/rotation-bits` prints it, `<sign><hi>:<lo>p<exp>`,
    /// read back.
    fn read_bits(text: &str) -> Result<f32, String> {
        if text == "0" {
            return Ok(0.0);
        }
        let (sign, rest) = text.split_at(1);
        let (mantissa, exponent) = rest.split_once('p').ok_or(text)?;
        let (hi, lo) = mantissa.split_once(':').ok_or(text)?;
        if lo != "0" {
            return Err(format!("{text} is not a float"));
        }
        let hi: i32 = hi.parse().map_err(|error| format!("{text}: {error}"))?;
        let exponent: i32 = exponent
            .parse()
            .map_err(|error| format!("{text}: {error}"))?;
        let magnitude = f64::from(hi) * 2.0_f64.powi(exponent);
        Ok(to_f32(if sign == "-" { -magnitude } else { magnitude }))
    }

    /// The fields `keys` of a line, read as floats.
    fn floats(line: &Line, keys: &[&str]) -> Result<Vec<f32>, String> {
        keys.iter()
            .map(|key| {
                line.field(key)
                    .ok_or_else(|| format!("no {key}"))
                    .and_then(read_bits)
            })
            .collect()
    }

    /// How many floats apart two floats of one sign are.
    fn ulps(a: f32, b: f32) -> u32 {
        a.to_bits().abs_diff(b.to_bits())
    }

    /// One library row: the record, what the function gave and what aditi
    /// printed.
    type Row = (String, Vec<f32>, Vec<f32>);

    /// Every library row of `math/rotation-bits` on aditi, by record, for
    /// each of the seven pairs.
    fn rows() -> Result<Vec<Row>, Box<dyn std::error::Error>> {
        let transcript = include_str!("../../probes/results/aditi/math/rotation-bits.txt");
        let lines: Vec<Line> = transcript
            .lines()
            .filter_map(|line| Line::parse(line).ok())
            .collect();
        let mut out = Vec::new();
        for case in 0..7 {
            let case_text = &case.to_string();
            let of = |rec: &'static str| {
                lines
                    .iter()
                    .filter(move |line| line.rec == rec && line.field("case") == Some(case_text))
            };
            let mut inputs = of("input-bits");
            let a = floats(inputs.next().ok_or("no a")?, &["x", "y", "z", "s"])?;
            let b = floats(inputs.next().ok_or("no b")?, &["x", "y", "z", "s"])?;
            let (&[ax, ay, az, as_], &[bx, by, bz, bs]) = (a.as_slice(), b.as_slice()) else {
                return Err("an operand is not four floats".into());
            };
            let (ra, rb) = (
                Rotation {
                    x: ax,
                    y: ay,
                    z: az,
                    s: as_,
                },
                Rotation {
                    x: bx,
                    y: by,
                    z: bz,
                    s: bs,
                },
            );
            let (v, w) = (
                Vector {
                    x: ax,
                    y: ay,
                    z: az,
                },
                Vector {
                    x: bx,
                    y: by,
                    z: bz,
                },
            );
            let vector = |v: Vector| vec![v.x, v.y, v.z];
            let rotation = |r: Rotation| vec![r.x, r.y, r.z, r.s];
            let ctx = &mut sl();
            let ours: [(&str, &[&str], Vec<f32>); 10] = [
                ("magnitude", &["value"], vec![ll_vec_mag(ctx, v.clone())?]),
                (
                    "normalise",
                    &["x", "y", "z"],
                    vector(ll_vec_norm(ctx, v.clone())?),
                ),
                (
                    "rot-between",
                    &["x", "y", "z", "s"],
                    rotation(ll_rot_between(ctx, v.clone(), w.clone())?),
                ),
                (
                    "angle-between",
                    &["value"],
                    vec![ll_angle_between(ctx, ra.clone(), rb.clone())?],
                ),
                (
                    "rot2euler",
                    &["x", "y", "z"],
                    vector(ll_rot2_euler(ctx, ra.clone())?),
                ),
                (
                    "euler2rot",
                    &["x", "y", "z", "s"],
                    rotation(ll_euler2_rot(ctx, v.clone())?),
                ),
                (
                    "axis-angle",
                    &["x", "y", "z", "s"],
                    rotation(ll_axis_angle2_rot(ctx, v.clone(), bs)?),
                ),
                (
                    "rot2axis",
                    &["x", "y", "z"],
                    vector(ll_rot2_axis(ctx, ra.clone())?),
                ),
                (
                    "rot2axis",
                    &["angle"],
                    vec![ll_rot2_angle(ctx, ra.clone())?],
                ),
                (
                    "rot2fwd",
                    &["x", "y", "z"],
                    vector(ll_rot2_fwd(ctx, ra.clone())?),
                ),
            ];
            for (rec, keys, got) in ours {
                let line = of(rec).next().ok_or_else(|| format!("no {rec} {case}"))?;
                out.push((rec.to_owned(), got, floats(line, keys)?));
            }
        }
        Ok(out)
    }

    #[test]
    fn the_rotation_library_is_aditis_to_the_bit() -> Result<(), Box<dyn std::error::Error>> {
        let mut exact = std::collections::BTreeMap::<String, u32>::new();
        for (rec, ours, aditi) in rows()? {
            let worst = ours
                .iter()
                .zip(&aditi)
                .map(|(a, b)| ulps(*a, *b))
                .max()
                .unwrap_or(0);
            // `llEuler2Rot` keeps PyOptimizer's double form, within three
            // units in the last place; `llRot2Euler` misses two pairs by one.
            let allowed = match rec.as_str() {
                "euler2rot" => 3,
                "rot2euler" => 1,
                _ => 0,
            };
            assert!(worst <= allowed, "{rec}: ours {ours:?}, aditi {aditi:?}");
            if worst == 0 {
                *exact.entry(rec).or_default() += 1;
            }
        }
        assert_eq!(exact.get("rot2euler"), Some(&5));
        for rec in [
            "magnitude",
            "normalise",
            "rot-between",
            "angle-between",
            "axis-angle",
            "rot2fwd",
        ] {
            assert_eq!(exact.get(rec), Some(&7), "{rec}");
        }
        // The axis and the angle of each pair.
        assert_eq!(exact.get("rot2axis"), Some(&14));
        Ok(())
    }

    #[test]
    fn euler_angles_round_trip_to_float_precision() -> Result<(), CallError> {
        let ctx = &mut sl();
        let mut worst = 0.0_f32;
        for x in (-170_i16..=170).step_by(17) {
            for y in (-80_i16..=80).step_by(16) {
                for z in (-170_i16..=170).step_by(17) {
                    let angles = Vector {
                        x: f32::from(x).to_radians(),
                        y: f32::from(y).to_radians(),
                        z: f32::from(z).to_radians(),
                    };
                    let rotation = ll_euler2_rot(ctx, angles.clone())?;
                    let back = ll_rot2_euler(ctx, rotation)?;
                    for (a, b) in [(angles.x, back.x), (angles.y, back.y), (angles.z, back.z)] {
                        worst = worst.max((a - b).abs());
                    }
                }
            }
        }
        assert!(worst < 1e-5, "worst {worst}");
        Ok(())
    }
}
