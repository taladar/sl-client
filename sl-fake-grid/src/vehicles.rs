//! Vehicles: the parameter model `llSetVehicleType`, `llSetVehicle*Param`
//! and the flags set, and what it does to a physical object each tick.
//!
//! The model is OpenSim's (`ubOde/ODEDynamics.cs`), ported onto the fake
//! grid's kinematic solver ([`crate::physics`]): the type presets, the
//! clamps each parameter gets, and the linear and angular motors with their
//! decay, the friction timescales, hover and buoyancy, linear and angular
//! deflection, vertical attraction and banking, each a correction of the
//! velocity or the spin over its timescale. Both grids measured the same
//! limits (`physics/vehicles`): the linear motor is clamped at 100 m/s (98
//! m/s on aditi and 97 on OpenSim after half a second towards 100) and the
//! angular motor at 4π rad/s (12.4 and 12.3 after a second). What OpenSim's
//! model reads that the fake grid has not got — the camera of a mouselook
//! steer, the body's own inertia tensor — it does without: a mouselook
//! vehicle steers as one with no camera, and torques turn the object as a
//! unit inertia would.

use sl_types::lsl::{Rotation, Vector};

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

/// The fastest the linear motor may be asked to go, metres a second.
const LINEAR_MOTOR_CAP: f32 = 100.0;

/// The fastest the angular motor may be asked to turn, radians a second
/// (4π, as both grids clamp it).
const ANGULAR_MOTOR_CAP: f32 = 12.566;

/// `VEHICLE_TYPE_*`.
mod kind {
    /// `VEHICLE_TYPE_SLED`.
    pub(super) const SLED: i32 = 1;
    /// `VEHICLE_TYPE_CAR`.
    pub(super) const CAR: i32 = 2;
    /// `VEHICLE_TYPE_BOAT`.
    pub(super) const BOAT: i32 = 3;
    /// `VEHICLE_TYPE_AIRPLANE`.
    pub(super) const AIRPLANE: i32 = 4;
    /// `VEHICLE_TYPE_BALLOON`.
    pub(super) const BALLOON: i32 = 5;
}

/// `VEHICLE_*` parameter codes.
mod param {
    /// `VEHICLE_LINEAR_FRICTION_TIMESCALE`.
    pub(super) const LINEAR_FRICTION_TIMESCALE: i32 = 16;
    /// `VEHICLE_ANGULAR_FRICTION_TIMESCALE`.
    pub(super) const ANGULAR_FRICTION_TIMESCALE: i32 = 17;
    /// `VEHICLE_LINEAR_MOTOR_DIRECTION`.
    pub(super) const LINEAR_MOTOR_DIRECTION: i32 = 18;
    /// `VEHICLE_ANGULAR_MOTOR_DIRECTION`.
    pub(super) const ANGULAR_MOTOR_DIRECTION: i32 = 19;
    /// `VEHICLE_LINEAR_MOTOR_OFFSET`.
    pub(super) const LINEAR_MOTOR_OFFSET: i32 = 20;
    /// `VEHICLE_HOVER_HEIGHT`.
    pub(super) const HOVER_HEIGHT: i32 = 24;
    /// `VEHICLE_HOVER_EFFICIENCY`.
    pub(super) const HOVER_EFFICIENCY: i32 = 25;
    /// `VEHICLE_HOVER_TIMESCALE`.
    pub(super) const HOVER_TIMESCALE: i32 = 26;
    /// `VEHICLE_BUOYANCY`.
    pub(super) const BUOYANCY: i32 = 27;
    /// `VEHICLE_LINEAR_DEFLECTION_EFFICIENCY`.
    pub(super) const LINEAR_DEFLECTION_EFFICIENCY: i32 = 28;
    /// `VEHICLE_LINEAR_DEFLECTION_TIMESCALE`.
    pub(super) const LINEAR_DEFLECTION_TIMESCALE: i32 = 29;
    /// `VEHICLE_LINEAR_MOTOR_TIMESCALE`.
    pub(super) const LINEAR_MOTOR_TIMESCALE: i32 = 30;
    /// `VEHICLE_LINEAR_MOTOR_DECAY_TIMESCALE`.
    pub(super) const LINEAR_MOTOR_DECAY_TIMESCALE: i32 = 31;
    /// `VEHICLE_ANGULAR_DEFLECTION_EFFICIENCY`.
    pub(super) const ANGULAR_DEFLECTION_EFFICIENCY: i32 = 32;
    /// `VEHICLE_ANGULAR_DEFLECTION_TIMESCALE`.
    pub(super) const ANGULAR_DEFLECTION_TIMESCALE: i32 = 33;
    /// `VEHICLE_ANGULAR_MOTOR_TIMESCALE`.
    pub(super) const ANGULAR_MOTOR_TIMESCALE: i32 = 34;
    /// `VEHICLE_ANGULAR_MOTOR_DECAY_TIMESCALE`.
    pub(super) const ANGULAR_MOTOR_DECAY_TIMESCALE: i32 = 35;
    /// `VEHICLE_VERTICAL_ATTRACTION_EFFICIENCY`.
    pub(super) const VERTICAL_ATTRACTION_EFFICIENCY: i32 = 36;
    /// `VEHICLE_VERTICAL_ATTRACTION_TIMESCALE`.
    pub(super) const VERTICAL_ATTRACTION_TIMESCALE: i32 = 37;
    /// `VEHICLE_BANKING_EFFICIENCY`.
    pub(super) const BANKING_EFFICIENCY: i32 = 38;
    /// `VEHICLE_BANKING_MIX`.
    pub(super) const BANKING_MIX: i32 = 39;
    /// `VEHICLE_BANKING_TIMESCALE`.
    pub(super) const BANKING_TIMESCALE: i32 = 40;
    /// `VEHICLE_REFERENCE_FRAME`.
    pub(super) const REFERENCE_FRAME: i32 = 44;
}

/// `VEHICLE_FLAG_*`.
mod flag {
    /// `VEHICLE_FLAG_NO_DEFLECTION_UP`.
    pub(super) const NO_DEFLECTION_UP: i32 = 0x1;
    /// `VEHICLE_FLAG_LIMIT_ROLL_ONLY`.
    pub(super) const LIMIT_ROLL_ONLY: i32 = 0x2;
    /// `VEHICLE_FLAG_HOVER_WATER_ONLY`.
    pub(super) const HOVER_WATER_ONLY: i32 = 0x4;
    /// `VEHICLE_FLAG_HOVER_TERRAIN_ONLY`.
    pub(super) const HOVER_TERRAIN_ONLY: i32 = 0x8;
    /// `VEHICLE_FLAG_HOVER_GLOBAL_HEIGHT`.
    pub(super) const HOVER_GLOBAL_HEIGHT: i32 = 0x10;
    /// `VEHICLE_FLAG_HOVER_UP_ONLY`.
    pub(super) const HOVER_UP_ONLY: i32 = 0x20;
    /// `VEHICLE_FLAG_LIMIT_MOTOR_UP`.
    pub(super) const LIMIT_MOTOR_UP: i32 = 0x40;
    /// `VEHICLE_FLAG_MOUSELOOK_STEER`.
    pub(super) const MOUSELOOK_STEER: i32 = 0x80;
    /// `VEHICLE_FLAG_MOUSELOOK_BANK`.
    pub(super) const MOUSELOOK_BANK: i32 = 0x100;
    /// `VEHICLE_FLAG_CAMERA_DECOUPLED`.
    pub(super) const CAMERA_DECOUPLED: i32 = 0x200;
}

/// A vehicle's parameters and its motors' state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Vehicle {
    /// The type, `VEHICLE_TYPE_NONE` for no vehicle.
    kind: i32,
    /// The flags.
    flags: i32,
    /// The reference frame.
    reference: Rotation,
    /// The linear motor's velocity, vehicle frame.
    linear_motor: Vector,
    /// How fast it pushes, seconds.
    linear_motor_timescale: f32,
    /// How fast its push decays, in ticks.
    linear_motor_decay: f32,
    /// Where it pushes from.
    linear_motor_offset: Vector,
    /// The linear friction on each axis, seconds.
    linear_friction: Vector,
    /// The angular motor's spin, vehicle frame.
    angular_motor: Vector,
    /// How fast it turns, seconds.
    angular_motor_timescale: f32,
    /// How fast its turn decays, in ticks.
    angular_motor_decay: f32,
    /// The angular friction on each axis, seconds.
    angular_friction: Vector,
    /// Angular deflection.
    angular_deflection: (f32, f32),
    /// Linear deflection.
    linear_deflection: (f32, f32),
    /// Banking: efficiency, mix, timescale.
    banking: (f32, f32, f32),
    /// Hover: height, efficiency, timescale.
    hover: (f32, f32, f32),
    /// The share of gravity it does not feel.
    buoyancy: f32,
    /// Vertical attraction: efficiency, timescale.
    vertical: (f32, f32),
    /// The linear motor's strength now, decaying from 1.
    linear_effect: f32,
    /// Its decay per tick.
    linear_decay: f32,
    /// The angular motor's strength now.
    angular_effect: f32,
    /// Its decay per tick.
    angular_decay: f32,
}

/// What the vehicle model reads of the object and its region each tick.
pub(crate) struct Input<'a> {
    /// The object's rotation.
    pub(crate) rotation: &'a Rotation,
    /// Its root's position.
    pub(crate) position: &'a Vector,
    /// Its velocity.
    pub(crate) velocity: &'a Vector,
    /// Its spin.
    pub(crate) omega: &'a Vector,
    /// The ground's height at a point.
    pub(crate) ground: &'a dyn Fn(f32, f32) -> f32,
    /// The water's height.
    pub(crate) water: f32,
    /// The gravity multiplier of its material.
    pub(crate) gravity_multiplier: f32,
}

/// What the vehicle does to the object this tick: accelerations, region
/// axes, gravity included.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Output {
    /// The linear acceleration, metres a second squared.
    pub(crate) linear: Vector,
    /// The angular acceleration, radians a second squared.
    pub(crate) angular: Vector,
}

/// `a + b`.
fn plus(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// `a - b`.
fn minus(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// `v × k`.
fn scaled(v: &Vector, k: f32) -> Vector {
    Vector {
        x: v.x * k,
        y: v.y * k,
        z: v.z * k,
    }
}

/// The length of `v`.
fn length(v: &Vector) -> f32 {
    v.z.mul_add(v.z, v.x.mul_add(v.x, v.y * v.y)).sqrt()
}

/// The conjugate of `r`.
const fn conjugate(r: &Rotation) -> Rotation {
    Rotation {
        x: -r.x,
        y: -r.y,
        z: -r.z,
        s: r.s,
    }
}

/// `v` turned by `r`.
fn turned(v: &Vector, r: &Rotation) -> Vector {
    crate::object_edits::rotate(r, v)
}

/// `v` shortened to `cap` where it is longer.
fn capped(v: Vector, cap: f32) -> Vector {
    let len = length(&v);
    if len > cap { scaled(&v, cap / len) } else { v }
}

/// The vehicle's x axis, region frame (`ODEDynamics.Xrot`).
fn x_axis(r: &Rotation) -> Vector {
    turned(
        &Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        r,
    )
}

/// The vehicle's z axis, region frame (`ODEDynamics.Zrot`).
fn z_axis(r: &Rotation) -> Vector {
    turned(
        &Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
        r,
    )
}

/// The roll and pitch of `r` (`ODEDynamics.GetRollPitch`).
fn roll_pitch(r: &Rotation) -> (f32, f32) {
    let half_pi = core::f32::consts::FRAC_PI_2;
    let zx = r.x.mul_add(r.z, r.y * r.s);
    if zx < -0.499_99 {
        (0.0, -half_pi)
    } else if zx > 0.499_99 {
        (0.0, half_pi)
    } else {
        let pitch = (2.0 * zx).asin();
        let minus_zy = r.x.mul_add(r.s, -(r.y * r.z));
        let zz = r.z.mul_add(r.z, r.s * r.s) - 0.5;
        (minus_zy.atan2(zz), pitch)
    }
}

impl Vehicle {
    /// No vehicle: OpenSim's `TYPE_NONE` defaults.
    pub(crate) const fn none(tick: f32) -> Self {
        Self {
            kind: 0,
            flags: 0,
            reference: IDENTITY,
            linear_motor: ZERO,
            linear_motor_timescale: 1000.0,
            linear_motor_decay: 120.0 / tick,
            linear_motor_offset: ZERO,
            linear_friction: Vector {
                x: 1000.0,
                y: 1000.0,
                z: 1000.0,
            },
            angular_motor: ZERO,
            angular_motor_timescale: 1000.0,
            angular_motor_decay: 1000.0 / tick,
            angular_friction: Vector {
                x: 1000.0,
                y: 1000.0,
                z: 1000.0,
            },
            angular_deflection: (0.0, 1000.0),
            linear_deflection: (0.0, 1000.0),
            banking: (0.0, 1.0, 1000.0),
            hover: (0.0, 1.0, 1000.0),
            buoyancy: 0.0,
            vertical: (0.0, 1000.0),
            linear_effect: 0.0,
            linear_decay: 0.0,
            angular_effect: 0.0,
            angular_decay: 0.0,
        }
    }

    /// Whether this is a vehicle at all.
    pub(crate) const fn active(&self) -> bool {
        self.kind != 0
    }

    /// `llSetVehicleType`: the type's presets (`ProcessTypeChange`); a
    /// number no type has is no vehicle. `tick` is the physics step,
    /// seconds.
    pub(crate) fn set_type(&mut self, kind: i32, tick: f32) {
        let mut vehicle = Self::none(tick);
        let v = |x: f32, y: f32, z: f32| Vector { x, y, z };
        let keep_flags = self.flags;
        match kind {
            kind::SLED => {
                vehicle.linear_friction = v(30.0, 1.0, 1000.0);
                vehicle.linear_motor_decay = 120.0 / tick;
                vehicle.angular_motor_decay = 120.0 / tick;
                vehicle.hover = (0.0, 1.0, 10.0);
                vehicle.linear_deflection = (1.0, 1.0);
                vehicle.angular_deflection = (0.0, 10.0);
                vehicle.vertical = (1.0, 1000.0);
                vehicle.banking = (0.0, 1.0, 10.0);
                vehicle.flags = (keep_flags
                    & !(flag::HOVER_WATER_ONLY
                        | flag::HOVER_TERRAIN_ONLY
                        | flag::HOVER_GLOBAL_HEIGHT
                        | flag::HOVER_UP_ONLY))
                    | flag::NO_DEFLECTION_UP
                    | flag::LIMIT_ROLL_ONLY
                    | flag::LIMIT_MOTOR_UP;
            }
            kind::CAR => {
                vehicle.linear_friction = v(100.0, 2.0, 1000.0);
                vehicle.linear_motor_timescale = 1.0;
                vehicle.linear_motor_decay = 60.0 / tick;
                vehicle.angular_motor_timescale = 1.0;
                vehicle.angular_motor_decay = 0.8 / tick;
                vehicle.hover = (0.0, 0.0, 1000.0);
                vehicle.linear_deflection = (1.0, 2.0);
                vehicle.angular_deflection = (0.0, 10.0);
                vehicle.vertical = (1.0, 10.0);
                vehicle.banking = (-0.2, 1.0, 1.0);
                vehicle.flags = (keep_flags
                    & !(flag::HOVER_WATER_ONLY
                        | flag::HOVER_TERRAIN_ONLY
                        | flag::HOVER_GLOBAL_HEIGHT))
                    | flag::NO_DEFLECTION_UP
                    | flag::LIMIT_ROLL_ONLY
                    | flag::LIMIT_MOTOR_UP
                    | flag::HOVER_UP_ONLY;
            }
            kind::BOAT => {
                vehicle.linear_friction = v(10.0, 3.0, 2.0);
                vehicle.angular_friction = v(10.0, 10.0, 10.0);
                vehicle.linear_motor_timescale = 5.0;
                vehicle.linear_motor_decay = 60.0 / tick;
                vehicle.angular_motor_timescale = 4.0;
                vehicle.angular_motor_decay = 4.0 / tick;
                vehicle.hover = (0.0, 0.5, 2.0);
                vehicle.buoyancy = 1.0;
                vehicle.linear_deflection = (0.5, 3.0);
                vehicle.angular_deflection = (0.5, 5.0);
                vehicle.vertical = (0.5, 5.0);
                vehicle.banking = (-0.3, 0.8, 1.0);
                vehicle.flags = (keep_flags
                    & !(flag::HOVER_TERRAIN_ONLY
                        | flag::HOVER_GLOBAL_HEIGHT
                        | flag::HOVER_UP_ONLY))
                    | flag::NO_DEFLECTION_UP
                    | flag::LIMIT_MOTOR_UP
                    | flag::HOVER_UP_ONLY
                    | flag::HOVER_WATER_ONLY;
            }
            kind::AIRPLANE => {
                vehicle.linear_friction = v(200.0, 10.0, 5.0);
                vehicle.angular_friction = v(20.0, 20.0, 20.0);
                vehicle.linear_motor_timescale = 2.0;
                vehicle.linear_motor_decay = 60.0 / tick;
                vehicle.angular_motor_timescale = 4.0;
                vehicle.angular_motor_decay = 8.0 / tick;
                vehicle.hover = (0.0, 0.5, 1000.0);
                vehicle.linear_deflection = (0.5, 0.5);
                vehicle.angular_deflection = (1.0, 2.0);
                vehicle.vertical = (0.9, 2.0);
                vehicle.banking = (1.0, 0.7, 2.0);
                vehicle.flags = (keep_flags
                    & !(flag::HOVER_WATER_ONLY
                        | flag::HOVER_TERRAIN_ONLY
                        | flag::HOVER_GLOBAL_HEIGHT
                        | flag::HOVER_UP_ONLY
                        | flag::NO_DEFLECTION_UP
                        | flag::LIMIT_MOTOR_UP))
                    | flag::LIMIT_ROLL_ONLY;
            }
            kind::BALLOON => {
                vehicle.linear_friction = v(5.0, 5.0, 5.0);
                vehicle.angular_friction = v(10.0, 10.0, 10.0);
                vehicle.linear_motor_timescale = 5.0;
                vehicle.linear_motor_decay = 60.0 / tick;
                vehicle.angular_motor_timescale = 6.0;
                vehicle.angular_motor_decay = 10.0 / tick;
                vehicle.hover = (5.0, 0.8, 10.0);
                vehicle.buoyancy = 1.0;
                vehicle.linear_deflection = (0.0, 5.0 / tick);
                vehicle.angular_deflection = (0.0, 5.0);
                vehicle.vertical = (1.0, 1000.0);
                vehicle.banking = (0.0, 0.7, 5.0);
                vehicle.flags = keep_flags
                    & !(flag::HOVER_WATER_ONLY
                        | flag::HOVER_TERRAIN_ONLY
                        | flag::HOVER_UP_ONLY
                        | flag::NO_DEFLECTION_UP
                        | flag::LIMIT_MOTOR_UP
                        | flag::LIMIT_ROLL_ONLY
                        | flag::HOVER_GLOBAL_HEIGHT);
            }
            _ => {
                *self = vehicle;
                return;
            }
        }
        vehicle.kind = kind;
        vehicle.flags &= !(flag::MOUSELOOK_STEER | flag::MOUSELOOK_BANK | flag::CAMERA_DECOUPLED);
        vehicle.linear_decay = 1.0 - 1.0 / vehicle.linear_motor_decay;
        vehicle.angular_decay = 1.0 - 1.0 / vehicle.angular_motor_decay;
        *self = vehicle;
    }

    /// `llSetVehicleFloatParam` (`ProcessFloatVehicleParam`): a vector
    /// parameter is set on all three axes. A value that is no number is
    /// ignored.
    pub(crate) fn set_float(&mut self, code: i32, value: f32, tick: f32) {
        if !value.is_finite() {
            return;
        }
        let at_least = value.max(tick);
        let unit = value.clamp(0.0, 1.0);
        match code {
            param::ANGULAR_DEFLECTION_EFFICIENCY => self.angular_deflection.0 = unit,
            param::ANGULAR_DEFLECTION_TIMESCALE => self.angular_deflection.1 = at_least,
            param::ANGULAR_MOTOR_DECAY_TIMESCALE => {
                self.angular_motor_decay = at_least.min(120.0) / tick;
                self.angular_decay = 1.0 - 1.0 / self.angular_motor_decay;
            }
            param::ANGULAR_MOTOR_TIMESCALE => self.angular_motor_timescale = at_least,
            param::BANKING_EFFICIENCY => self.banking.0 = value.clamp(-1.0, 1.0),
            param::BANKING_MIX => self.banking.1 = unit,
            param::BANKING_TIMESCALE => self.banking.2 = at_least,
            param::BUOYANCY => self.buoyancy = value.clamp(-1.0, 1.0),
            param::HOVER_EFFICIENCY => self.hover.1 = unit,
            param::HOVER_HEIGHT => self.hover.0 = value,
            param::HOVER_TIMESCALE => self.hover.2 = at_least,
            param::LINEAR_DEFLECTION_EFFICIENCY => self.linear_deflection.0 = unit,
            param::LINEAR_DEFLECTION_TIMESCALE => self.linear_deflection.1 = at_least,
            param::LINEAR_MOTOR_DECAY_TIMESCALE => {
                self.linear_motor_decay = (0.2 + at_least.min(120.0)) / tick;
                self.linear_decay = 1.0 - 1.0 / self.linear_motor_decay;
            }
            param::LINEAR_MOTOR_TIMESCALE => self.linear_motor_timescale = at_least,
            param::VERTICAL_ATTRACTION_EFFICIENCY => self.vertical.0 = unit,
            param::VERTICAL_ATTRACTION_TIMESCALE => self.vertical.1 = at_least,
            param::LINEAR_FRICTION_TIMESCALE
            | param::ANGULAR_FRICTION_TIMESCALE
            | param::LINEAR_MOTOR_DIRECTION
            | param::ANGULAR_MOTOR_DIRECTION
            | param::LINEAR_MOTOR_OFFSET => self.set_vector(
                code,
                &Vector {
                    x: value,
                    y: value,
                    z: value,
                },
                tick,
            ),
            _ => {}
        }
    }

    /// `llSetVehicleVectorParam` (`ProcessVectorVehicleParam`).
    pub(crate) fn set_vector(&mut self, code: i32, value: &Vector, tick: f32) {
        if !(value.x.is_finite() && value.y.is_finite() && value.z.is_finite()) {
            return;
        }
        let at_least = |v: &Vector| Vector {
            x: v.x.max(tick),
            y: v.y.max(tick),
            z: v.z.max(tick),
        };
        match code {
            param::ANGULAR_FRICTION_TIMESCALE => self.angular_friction = at_least(value),
            param::LINEAR_FRICTION_TIMESCALE => self.linear_friction = at_least(value),
            param::ANGULAR_MOTOR_DIRECTION => {
                self.angular_motor = capped(value.clone(), ANGULAR_MOTOR_CAP);
                self.angular_effect = 1.0;
                self.angular_decay = 1.0 - 1.0 / self.angular_motor_decay;
            }
            param::LINEAR_MOTOR_DIRECTION => {
                self.linear_motor = capped(value.clone(), LINEAR_MOTOR_CAP);
                self.linear_effect = 1.0;
                self.linear_decay = 1.0 - 1.0 / self.linear_motor_decay;
            }
            param::LINEAR_MOTOR_OFFSET => {
                self.linear_motor_offset = capped(value.clone(), LINEAR_MOTOR_CAP);
            }
            _ => {}
        }
    }

    /// `llSetVehicleRotationParam`: the reference frame.
    pub(crate) fn set_rotation(&mut self, code: i32, value: &Rotation) {
        if code == param::REFERENCE_FRAME {
            self.reference = value.clone();
        }
    }

    /// `llSetVehicleFlags` (`remove` false) or `llRemoveVehicleFlags`.
    pub(crate) const fn set_flags(&mut self, flags: i32, remove: bool) {
        if remove {
            self.flags &= !flags;
        } else {
            self.flags |= flags;
        }
    }

    /// One tick of `dt` seconds of the vehicle on the object as `input`
    /// reads it (`ODEDynamics.Step`), gravity included.
    pub(crate) fn step(&mut self, input: &Input<'_>, dt: f32) -> Output {
        let gravity = -crate::physics::GRAVITY;
        let inv_dt = 1.0 / dt;
        // The vehicle's frame: the object's rotation, then the reference.
        let frame = crate::object_edits::multiply(&self.reference, input.rotation);
        let inverse = conjugate(&frame);
        let local_velocity = turned(input.velocity, &inverse);
        let local_omega = turned(input.omega, &inverse);
        let mut force = ZERO;
        let mut torque = ZERO;
        let mut damp_z = 0.0;

        if self.linear_effect > 0.01 && self.linear_motor_timescale < 1000.0 {
            let error = scaled(
                &minus(&self.linear_motor, &local_velocity),
                self.linear_effect / self.linear_motor_timescale,
            );
            let mut world = turned(&error, &frame);
            if self.flags & flag::LIMIT_MOTOR_UP != 0 {
                world.z = 0.0;
            }
            force = plus(&force, &world);
            self.linear_effect *= self.linear_decay;
        } else {
            self.linear_effect = 0.0;
        }

        let (hover_height, hover_efficiency, hover_timescale) = self.hover;
        if hover_timescale < 300.0 {
            let x = input.position.x;
            let y = input.position.y;
            let z = input.position.z - 0.21;
            let terrain = (input.ground)(x, y);
            let mut error = hover_height - z;
            if self.flags & flag::HOVER_GLOBAL_HEIGHT == 0 {
                error += if self.flags & flag::HOVER_WATER_ONLY != 0 {
                    input.water
                } else if self.flags & flag::HOVER_TERRAIN_ONLY != 0 {
                    terrain
                } else {
                    terrain.max(input.water)
                };
            } else if terrain > hover_height {
                error = terrain - z;
            }
            if self.flags & flag::HOVER_UP_ONLY == 0 || error > -0.1 {
                let damping = hover_efficiency * inv_dt;
                force.z += error * (1.0 + damping) / hover_timescale;
                damp_z = -damping * input.velocity.z;
                force.z += gravity * input.gravity_multiplier * (1.0 - self.buoyancy);
            } else {
                force.z += gravity;
            }
        } else {
            force.z += gravity * input.gravity_multiplier * (1.0 - self.buoyancy);
        }

        let (deflection, deflection_timescale) = self.linear_deflection;
        if deflection > 0.0 {
            let speed = length(input.velocity);
            if speed > 0.01 {
                let at = scaled(&x_axis(&frame), speed);
                let towards = minus(&at, input.velocity);
                let away = minus(&scaled(&at, -1.0), input.velocity);
                let (near, far) = (
                    towards.z.mul_add(
                        towards.z,
                        towards.x.mul_add(towards.x, towards.y * towards.y),
                    ),
                    away.z
                        .mul_add(away.z, away.x.mul_add(away.x, away.y * away.y)),
                );
                if near > 0.01 || far > 0.01 {
                    let error = if near < far { towards } else { away };
                    let correction = scaled(&error, deflection / deflection_timescale);
                    force.x += correction.x;
                    force.y += correction.y;
                    if self.flags & flag::NO_DEFLECTION_UP == 0 {
                        force.z += correction.z;
                    }
                }
            }
        }

        if local_velocity != ZERO {
            let friction = Vector {
                x: -local_velocity.x / self.linear_friction.x,
                y: -local_velocity.y / self.linear_friction.y,
                z: -local_velocity.z / self.linear_friction.z,
            };
            let mut world = turned(&friction, &frame);
            if damp_z != 0.0 && damp_z.abs() > world.z.abs() {
                world.z = damp_z;
            }
            force = plus(&force, &world);
        }

        let (attraction, attraction_timescale) = self.vertical;
        let (damp_x, damp_y, damp_z_angular, power);
        if attraction_timescale < 300.0 {
            let half_pi = core::f32::consts::FRAC_PI_2;
            let pi = core::f32::consts::PI;
            let strength = inv_dt / attraction_timescale / attraction_timescale;
            let (mut roll, pitch) = roll_pitch(&inverse);
            if roll > half_pi {
                roll = pi - roll;
            } else if roll < -half_pi {
                roll = -pi - roll;
            }
            let effective_roll = (1.0 - (pitch / half_pi).powi(2)) * roll;
            torque.x += effective_roll * strength;
            if self.flags & flag::LIMIT_ROLL_ONLY == 0 {
                let effective_pitch = (1.0 - (roll / half_pi).powi(2)) * pitch;
                torque.y += effective_pitch * strength;
            }
            let base_damp = 0.5 * attraction * inv_dt;
            power = 0.8_f32.mul_add(-attraction, 1.0);
            let (efficiency, mix, timescale) = self.banking;
            if efficiency != 0.0 && effective_roll.abs() > 0.01 {
                let mut bank = effective_roll * efficiency;
                if mix != 0.0 {
                    let share = (local_velocity.x.abs() / 10.0).min(1.0);
                    let factor = (share - 1.0).mul_add(mix, 1.0);
                    bank *= if local_velocity.x >= 0.0 {
                        factor
                    } else {
                        -factor
                    };
                }
                bank /= timescale;
                torque = plus(&torque, &scaled(&z_axis(&inverse), bank));
                damp_z_angular = efficiency.abs() / timescale;
                damp_y = damp_z_angular;
            } else {
                damp_z_angular = 1.0 / self.angular_friction.z;
                damp_y = base_damp;
            }
            damp_x = base_damp;
        } else {
            power = 1.0;
            damp_x = 1.0 / self.angular_friction.x;
            damp_y = 1.0 / self.angular_friction.y;
            damp_z_angular = 1.0 / self.angular_friction.z;
        }

        if self.angular_effect > 0.01 && self.angular_motor_timescale < 1000.0 {
            let error = scaled(
                &minus(&self.angular_motor, &local_omega),
                self.angular_effect / self.angular_motor_timescale,
            );
            torque.x += error.x * power;
            torque.y += error.y * power;
            torque.z += error.z;
            self.angular_effect *= self.angular_decay;
        } else {
            self.angular_effect = 0.0;
        }

        let (deflection, deflection_timescale) = self.angular_deflection;
        if deflection > 0.0 {
            let direction = if local_velocity.x > 0.01 {
                local_velocity.clone()
            } else if local_velocity.x < -0.01 {
                scaled(&local_velocity, -1.0)
            } else {
                Vector {
                    x: 0.01,
                    ..local_velocity.clone()
                }
            };
            let strength = deflection / deflection_timescale;
            if direction.z.abs() > 0.01 {
                torque.y += -direction.z.atan2(direction.x) * strength;
            }
            if direction.y.abs() > 0.01 {
                torque.z += direction.y.atan2(direction.x) * strength;
            }
        }

        if local_omega != ZERO {
            torque.x -= local_omega.x * damp_x;
            torque.y -= local_omega.y * damp_y;
            torque.z -= local_omega.z * damp_z_angular;
        }

        // The torque, in the vehicle's frame, turned into the region's.
        let angular = turned(&torque, &frame);
        Output {
            linear: force,
            angular,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// The fake grid's Second Life tick, 45 a second.
    const TICK: f32 = 1.0 / 45.0;

    /// A level object at rest, its region flat at 20 m and its water at 20.
    fn input<'a>(
        rotation: &'a Rotation,
        velocity: &'a Vector,
        omega: &'a Vector,
        ground: &'a dyn Fn(f32, f32) -> f32,
    ) -> Input<'a> {
        Input {
            rotation,
            position: &Vector {
                x: 128.0,
                y: 128.0,
                z: 21.0,
            },
            velocity,
            omega,
            ground,
            water: 20.0,
            gravity_multiplier: 1.0,
        }
    }

    #[test]
    fn a_motor_past_100_is_clamped_and_its_speed_approached() {
        let mut vehicle = Vehicle::none(TICK);
        vehicle.set_type(kind::BALLOON, TICK);
        vehicle.set_float(param::BUOYANCY, 1.0, TICK);
        vehicle.set_float(param::HOVER_EFFICIENCY, 0.0, TICK);
        vehicle.set_float(param::HOVER_TIMESCALE, 1000.0, TICK);
        vehicle.set_vector(
            param::LINEAR_MOTOR_DIRECTION,
            &Vector {
                x: 0.0,
                y: 0.0,
                z: 1000.0,
            },
            TICK,
        );
        vehicle.set_float(param::LINEAR_MOTOR_TIMESCALE, 0.1, TICK);
        let flat = |_x: f32, _y: f32| 20.0;
        let mut velocity = ZERO;
        for _tick in 0..22 {
            let out = vehicle.step(&input(&IDENTITY, &velocity, &ZERO, &flat), TICK);
            velocity = plus(&velocity, &scaled(&out.linear, TICK));
        }
        // Half a second towards 100 m/s, balloon friction pulling back.
        assert!(velocity.z > 80.0 && velocity.z < 100.0, "{velocity:?}");
    }

    #[test]
    fn an_angular_motor_is_clamped_at_four_pi() {
        let mut vehicle = Vehicle::none(TICK);
        vehicle.set_type(kind::BALLOON, TICK);
        vehicle.set_vector(
            param::ANGULAR_MOTOR_DIRECTION,
            &Vector {
                x: 0.0,
                y: 0.0,
                z: 100.0,
            },
            TICK,
        );
        vehicle.set_float(param::ANGULAR_MOTOR_TIMESCALE, 0.1, TICK);
        let flat = |_x: f32, _y: f32| 20.0;
        let mut omega = ZERO;
        for _tick in 0..45 {
            let out = vehicle.step(&input(&IDENTITY, &ZERO, &omega, &flat), TICK);
            omega = plus(&omega, &scaled(&out.angular, TICK));
        }
        assert!(
            omega.z > 10.0 && omega.z < 4.0 * core::f32::consts::PI,
            "{omega:?}"
        );
    }

    #[test]
    fn no_vehicle_is_gravity_alone() {
        let mut vehicle = Vehicle::none(TICK);
        vehicle.set_type(99, TICK);
        assert!(!vehicle.active());
        let flat = |_x: f32, _y: f32| 20.0;
        let out = vehicle.step(&input(&IDENTITY, &ZERO, &ZERO, &flat), TICK);
        assert_eq!(out.linear.z.to_bits(), (-crate::physics::GRAVITY).to_bits());
    }
}
