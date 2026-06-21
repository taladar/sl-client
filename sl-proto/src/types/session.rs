//! Session setup and control: login parameters, throttle, camera, transmit.

use std::net::SocketAddr;

use sl_types::lsl::Vector;
use sl_wire::LoginRequest;

/// The parameters needed to start a session: where to log in and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginParams {
    /// The XML-RPC login endpoint URL (e.g. `http://127.0.0.1:9000/`).
    pub login_uri: String,
    /// The login request to send.
    pub request: LoginRequest,
}

/// An HTTP request the driver must perform on the session's behalf: POST `body`
/// to `url` and feed the response back via
/// [`Session::handle_login_response`](crate::Session::handle_login_response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginHttpRequest {
    /// The URL to POST to.
    pub url: String,
    /// The XML-RPC request body.
    pub body: String,
    /// The `User-Agent` header to send, identifying the viewer by its channel
    /// and version (see [`LoginRequest::user_agent`](sl_wire::LoginRequest::user_agent)).
    pub user_agent: String,
}

/// How an outgoing message should be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    /// Send once, best-effort.
    Unreliable,
    /// Send reliably: track acknowledgement and retransmit until acked.
    Reliable,
}

/// Per-category bandwidth throttle, in **kilobits per second**, advertised to
/// the simulator with `AgentThrottle`. The seven categories partition the
/// simulator's UDP send budget; the simulator uses these caps to allocate
/// bandwidth across the traffic it pushes to the client.
///
/// Without an explicit throttle the simulator applies conservative defaults
/// that starve the bulk object / terrain / texture streams the world-rendering
/// features (object scene graph, terrain, textures) depend on. Set one with
/// [`Session::set_throttle`](crate::Session::set_throttle) after the circuit is
/// established; it is re-sent automatically on every region change.
///
/// The values are interpreted as a total bandwidth split: the sum across all
/// seven categories is the requested aggregate rate, which the simulator may
/// cap to its own configured maximum. Use [`Throttle::total`] to read the sum
/// and the [`Throttle::preset_300`] / [`Throttle::preset_500`] /
/// [`Throttle::preset_1000`] presets (named for their total kbps) as starting
/// points; they mirror the reference viewer's bandwidth tables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Throttle {
    /// Resent (reliable retransmit) traffic.
    pub resend: f32,
    /// Land/terrain layer (`LayerData`) traffic.
    pub land: f32,
    /// Wind layer traffic.
    pub wind: f32,
    /// Cloud layer traffic.
    pub cloud: f32,
    /// Task traffic: object updates (the scene graph).
    pub task: f32,
    /// Texture (image) traffic.
    pub texture: f32,
    /// Other asset traffic (sounds, animations, notecards, …).
    pub asset: f32,
}

impl Throttle {
    /// Builds a throttle from the seven per-category rates (kilobits per second),
    /// in wire order: resend, land, wind, cloud, task, texture, asset.
    #[must_use]
    pub const fn new(
        resend: f32,
        land: f32,
        wind: f32,
        cloud: f32,
        task: f32,
        texture: f32,
        asset: f32,
    ) -> Self {
        Self {
            resend,
            land,
            wind,
            cloud,
            task,
            texture,
            asset,
        }
    }

    /// The reference viewer's preset for a 300 kbps total bandwidth.
    #[must_use]
    pub const fn preset_300() -> Self {
        Self::new(30.0, 40.0, 9.0, 9.0, 86.0, 86.0, 40.0)
    }

    /// The reference viewer's preset for a 500 kbps total bandwidth.
    #[must_use]
    pub const fn preset_500() -> Self {
        Self::new(50.0, 70.0, 14.0, 14.0, 136.0, 136.0, 80.0)
    }

    /// The reference viewer's preset for a 1000 kbps total bandwidth.
    #[must_use]
    pub const fn preset_1000() -> Self {
        Self::new(100.0, 100.0, 20.0, 20.0, 310.0, 310.0, 140.0)
    }

    /// The total requested bandwidth (kilobits per second), the sum of all seven
    /// categories.
    #[must_use]
    pub fn total(&self) -> f32 {
        self.resend + self.land + self.wind + self.cloud + self.task + self.texture + self.asset
    }

    /// Rebuilds a throttle from the seven wire **bits per second** rates (in
    /// wire order: resend, land, wind, cloud, task, texture, asset), the exact
    /// inverse of [`Throttle::bits_per_second`]. Used by the simulator side to
    /// recover the client's requested per-category split from an inbound
    /// `AgentThrottle`.
    #[must_use]
    pub fn from_bits_per_second(rates: [f32; 7]) -> Self {
        // 1 kilobit = 1024 bits, matching the reference viewer's conversion.
        const KILOBIT: f32 = 1024.0;
        let [resend, land, wind, cloud, task, texture, asset] = rates;
        Self {
            resend: resend / KILOBIT,
            land: land / KILOBIT,
            wind: wind / KILOBIT,
            cloud: cloud / KILOBIT,
            task: task / KILOBIT,
            texture: texture / KILOBIT,
            asset: asset / KILOBIT,
        }
    }

    /// The seven category rates in wire order (resend, land, wind, cloud, task,
    /// texture, asset), converted to **bits per second** as the `AgentThrottle`
    /// wire encoding expects (the simulator divides by 8 to get bytes/second).
    #[must_use]
    pub fn bits_per_second(&self) -> [f32; 7] {
        // 1 kilobit = 1024 bits, matching the reference viewer's conversion.
        const KILOBIT: f32 = 1024.0;
        [
            self.resend * KILOBIT,
            self.land * KILOBIT,
            self.wind * KILOBIT,
            self.cloud * KILOBIT,
            self.task * KILOBIT,
            self.texture * KILOBIT,
            self.asset * KILOBIT,
        ]
    }
}

impl Default for Throttle {
    /// The 1000 kbps preset — a generous split suited to a client that wants the
    /// full object/terrain/texture firehose.
    fn default() -> Self {
        Self::preset_1000()
    }
}

/// The agent's camera viewpoint, advertised to the simulator in every
/// `AgentUpdate`.
///
/// The simulator uses the camera position and look direction (together with the
/// draw distance — see [`Session::set_draw_distance`](crate::Session::set_draw_distance))
/// to build the agent's **interest list**: which objects, avatars and regions it
/// streams, and how the per-category bandwidth (the throttle) is spent. So the
/// camera follows where the agent actually *looks*, not where it stands.
///
/// The three axes form a right-handed orthonormal frame in the SL convention
/// (`at × left = up`): `at_axis` is the forward look direction, `left_axis`
/// points to the camera's left, and `up_axis` is its up vector. Until a client
/// sets one with [`Session::set_camera`](crate::Session::set_camera), the
/// session advertises [`Camera::region_center`] — the historic region-centre
/// viewpoint looking along +X.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// The camera's region-local position (the eye point).
    pub center: Vector,
    /// The unit vector the camera looks along (forward / "at").
    pub at_axis: Vector,
    /// The camera's unit left vector.
    pub left_axis: Vector,
    /// The camera's unit up vector.
    pub up_axis: Vector,
}

/// Why [`Camera::new`] rejected a set of axes: they do not form a right-handed
/// orthonormal frame in the SL convention (`at × left = up`). The axes arrive as
/// `f32`, so every check allows a small tolerance rather than an exact match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CameraError {
    /// One of the three axes is not unit-length (within tolerance).
    #[error("camera axis is not unit-length")]
    NotUnitLength,
    /// Two of the axes are not mutually orthogonal (within tolerance).
    #[error("camera axes are not mutually orthogonal")]
    NotOrthogonal,
    /// The axes are orthonormal but not right-handed in the SL convention
    /// (`at × left` does not equal `up`).
    #[error("camera axes are not right-handed (at × left ≠ up)")]
    NotRightHanded,
}

/// Tolerance for the unit-length, orthogonality and right-handedness checks in
/// [`Camera::new`]; the axes are `f32`, so an exact comparison is never apt.
const AXIS_TOLERANCE: f32 = 1e-3;

impl Camera {
    /// Builds a camera from an explicit position and basis, validating that the
    /// three axes form a **right-handed orthonormal frame** in the SL convention
    /// (`at × left = up`): each axis unit-length, the three mutually orthogonal,
    /// and `at × left` equal to `up` — all within a small `f32` tolerance.
    ///
    /// Returns [`CameraError`] if the basis is degenerate. Use
    /// [`Camera::looking_at`] to derive a valid basis from a target point, or
    /// [`Camera::new_unchecked`] when the axes are already known-good (for
    /// example reconstructed from the wire).
    ///
    /// # Errors
    ///
    /// Returns [`CameraError::NotUnitLength`], [`CameraError::NotOrthogonal`] or
    /// [`CameraError::NotRightHanded`] for a basis that is not a right-handed
    /// orthonormal frame.
    pub fn new(
        center: Vector,
        at_axis: Vector,
        left_axis: Vector,
        up_axis: Vector,
    ) -> Result<Self, CameraError> {
        for axis in [&at_axis, &left_axis, &up_axis] {
            if (length(axis) - 1.0).abs() > AXIS_TOLERANCE {
                return Err(CameraError::NotUnitLength);
            }
        }
        if dot(&at_axis, &left_axis).abs() > AXIS_TOLERANCE
            || dot(&at_axis, &up_axis).abs() > AXIS_TOLERANCE
            || dot(&left_axis, &up_axis).abs() > AXIS_TOLERANCE
        {
            return Err(CameraError::NotOrthogonal);
        }
        let expected_up = cross(&at_axis, &left_axis);
        if length(&sub(&expected_up, &up_axis)) > AXIS_TOLERANCE {
            return Err(CameraError::NotRightHanded);
        }
        Ok(Self::new_unchecked(center, at_axis, left_axis, up_axis))
    }

    /// Builds a camera from an explicit position and basis **without validating**
    /// that the axes are orthonormal. The caller is responsible for the axes
    /// being unit-length and mutually orthogonal in the SL convention
    /// (`at × left = up`).
    ///
    /// This is the codec-boundary constructor: an inbound `AgentUpdate` carries
    /// whatever basis the peer sent, which must be reconstructed verbatim rather
    /// than rejected. For caller-supplied axes prefer the validating
    /// [`Camera::new`], or [`Camera::looking_at`] to derive the basis.
    #[must_use]
    pub const fn new_unchecked(
        center: Vector,
        at_axis: Vector,
        left_axis: Vector,
        up_axis: Vector,
    ) -> Self {
        Self {
            center,
            at_axis,
            left_axis,
            up_axis,
        }
    }

    /// The default camera advertised before any [`Session::set_camera`](crate::Session::set_camera):
    /// positioned at the centre of a standard 256 m region (128, 128, 30),
    /// looking along +X with the world-up basis. This is the viewpoint the
    /// session used unconditionally before camera control existed, so it keeps
    /// the interest list anchored at the region origin until a real viewpoint is
    /// supplied.
    #[must_use]
    pub const fn region_center() -> Self {
        Self::new_unchecked(
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        )
    }

    /// Builds a camera at `eye` looking toward `target`, deriving the
    /// orthonormal `at`/`left`/`up` basis with the world up vector (+Z), exactly
    /// as the reference viewer's `LLCoordFrame::lookAt` does
    /// (`left = up × at`, `up = at × left`).
    ///
    /// Degenerate inputs fall back gracefully: if `eye` and `target` coincide the
    /// camera looks along +X, and if the look direction is vertical (so the
    /// world-up cross product vanishes) the left axis defaults to +Y.
    #[must_use]
    pub fn looking_at(eye: Vector, target: Vector) -> Self {
        const FORWARD: Vector = Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        const SIDE: Vector = Vector {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        const WORLD_UP: Vector = Vector {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        let at = normalize(&sub(&target, &eye)).unwrap_or(FORWARD);
        let left = normalize(&cross(&WORLD_UP, &at)).unwrap_or(SIDE);
        // `at` and `left` are unit and orthogonal, so their cross product is
        // already unit-length — no further normalisation needed.
        let up = cross(&at, &left);
        Self::new_unchecked(eye, at, left, up)
    }
}

impl Default for Camera {
    /// [`Camera::region_center`] — the region-centre viewpoint used before any
    /// explicit camera was set.
    fn default() -> Self {
        Self::region_center()
    }
}

/// Vector difference `a - b`.
fn sub(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// The cross product `a × b`.
fn cross(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// The dot product `a · b`.
fn dot(a: &Vector, b: &Vector) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// The Euclidean length of `v`.
fn length(v: &Vector) -> f32 {
    dot(v, v).sqrt()
}

/// Normalises `v` to unit length, returning `None` if it is too short to give a
/// stable direction (so callers can substitute a sensible default axis).
fn normalize(v: &Vector) -> Option<Vector> {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if length < 1e-6 {
        return None;
    }
    Some(Vector {
        x: v.x / length,
        y: v.y / length,
        z: v.z / length,
    })
}

/// A datagram ready to be sent on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transmit {
    /// Where to send the datagram.
    pub destination: SocketAddr,
    /// The datagram bytes.
    pub payload: Vec<u8>,
}

/// Why a session became disconnected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// The login server rejected the credentials.
    LoginFailed {
        /// The machine-readable reason code.
        reason: String,
        /// The human-readable message.
        message: String,
    },
    /// No traffic was received within the inactivity budget.
    Timeout,
    /// A reliable handshake packet exhausted its retransmissions.
    HandshakeFailed,
    /// An unrecoverable wire-protocol error occurred.
    ProtocolError,
}

#[cfg(test)]
mod tests {
    use super::{Camera, CameraError, Vector, cross, dot};
    use pretty_assertions::assert_eq;

    fn is_unit(v: &Vector) -> bool {
        (dot(v, v) - 1.0).abs() < 1e-5
    }

    fn approx_eq(a: &Vector, b: &Vector) -> bool {
        (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5
    }

    #[test]
    fn looking_at_builds_right_handed_orthonormal_basis() {
        // An oblique look direction so all three axes are non-trivial.
        let eye = Vector {
            x: 10.0,
            y: 20.0,
            z: 5.0,
        };
        let target = Vector {
            x: 13.0,
            y: 24.0,
            z: 7.0,
        };
        let camera = Camera::looking_at(eye.clone(), target);
        // The eye point is kept verbatim.
        assert_eq!(camera.center, eye);
        // All three axes are unit length.
        assert!(is_unit(&camera.at_axis));
        assert!(is_unit(&camera.left_axis));
        assert!(is_unit(&camera.up_axis));
        // They are mutually orthogonal.
        assert!(dot(&camera.at_axis, &camera.left_axis).abs() < 1e-5);
        assert!(dot(&camera.at_axis, &camera.up_axis).abs() < 1e-5);
        assert!(dot(&camera.left_axis, &camera.up_axis).abs() < 1e-5);
        // Right-handed in the SL convention: at × left = up.
        assert!(approx_eq(
            &cross(&camera.at_axis, &camera.left_axis),
            &camera.up_axis
        ));
    }

    #[test]
    fn looking_straight_down_falls_back_gracefully() {
        // A vertical look direction makes `world_up × at` vanish; the left axis
        // must still come out unit-length (the +Y fallback) and the basis stay
        // orthonormal rather than producing NaNs.
        let eye = Vector {
            x: 0.0,
            y: 0.0,
            z: 10.0,
        };
        let target = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let camera = Camera::looking_at(eye, target);
        assert!(is_unit(&camera.at_axis));
        assert!(is_unit(&camera.left_axis));
        assert!(is_unit(&camera.up_axis));
        // Looking straight down: at = -Z.
        assert!(approx_eq(
            &camera.at_axis,
            &Vector {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            }
        ));
    }

    #[test]
    fn region_center_is_the_legacy_default() {
        // The default camera matches the historic hardcoded viewpoint: region
        // centre, looking along +X with the world-up basis.
        let camera = Camera::default();
        assert_eq!(camera, Camera::region_center());
        assert_eq!(
            camera.center,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            }
        );
        // Equivalent to looking from the centre toward +X.
        let looked = Camera::looking_at(
            camera.center.clone(),
            Vector {
                x: 129.0,
                y: 128.0,
                z: 30.0,
            },
        );
        assert!(approx_eq(&looked.at_axis, &camera.at_axis));
        assert!(approx_eq(&looked.left_axis, &camera.left_axis));
        assert!(approx_eq(&looked.up_axis, &camera.up_axis));
    }

    #[test]
    fn new_accepts_a_valid_orthonormal_basis() {
        // The basis `looking_at` derives must pass the validating constructor.
        let eye = Vector {
            x: 10.0,
            y: 20.0,
            z: 5.0,
        };
        let derived = Camera::looking_at(
            eye.clone(),
            Vector {
                x: 13.0,
                y: 24.0,
                z: 7.0,
            },
        );
        let built = Camera::new(
            eye,
            derived.at_axis.clone(),
            derived.left_axis.clone(),
            derived.up_axis.clone(),
        );
        assert_eq!(built, Ok(derived));
    }

    #[test]
    fn new_rejects_a_non_unit_axis() {
        let center = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        // `at` is twice unit length.
        let result = Camera::new(
            center,
            Vector {
                x: 2.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        );
        assert_eq!(result, Err(CameraError::NotUnitLength));
    }

    #[test]
    fn new_rejects_non_orthogonal_axes() {
        let center = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        // `at` and `left` are both unit but point the same way.
        let result = Camera::new(
            center,
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        );
        assert_eq!(result, Err(CameraError::NotOrthogonal));
    }

    #[test]
    fn new_rejects_a_left_handed_basis() {
        let center = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        // Orthonormal but left-handed: at × left = -up, not up.
        let result = Camera::new(
            center,
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            Vector {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
        );
        assert_eq!(result, Err(CameraError::NotRightHanded));
    }
}
