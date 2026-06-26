//! Session setup and control: login parameters, throttle, camera, transmit.

use std::net::SocketAddr;

use sl_types::lsl::Vector;
use sl_wire::LoginRequest;

/// The parameters needed to start a session: where to log in and with what.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginParams {
    /// The XML-RPC login endpoint URL (e.g. `http://127.0.0.1:9000/`).
    pub login_uri: url::Url,
    /// The login request to send.
    pub request: LoginRequest,
}

/// An HTTP request the driver must perform on the session's behalf: POST `body`
/// to `url` and feed the response back via
/// [`Session::handle_login_response`](crate::Session::handle_login_response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginHttpRequest {
    /// The URL to POST to.
    pub url: url::Url,
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

/// Whether the avatar runs or walks for ground movement, carried by
/// [`Command::SetAlwaysRun`](crate::Command::SetAlwaysRun) and its matching
/// [`ServerEvent::SetAlwaysRun`](crate::ServerEvent::SetAlwaysRun). A named
/// intent enum in place of the bare `always_run: bool` of the `SetAlwaysRun`
/// message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementMode {
    /// Walk for ground movement (the `always_run` wire flag is clear).
    Walk,
    /// Always run for ground movement (the `always_run` wire flag is set).
    AlwaysRun,
}

impl MovementMode {
    /// Whether this mode sets the `always_run` wire flag: `true` for
    /// [`AlwaysRun`](Self::AlwaysRun), `false` for [`Walk`](Self::Walk).
    #[must_use]
    pub const fn is_always_run(self) -> bool {
        matches!(self, Self::AlwaysRun)
    }

    /// The mode for an `always_run` flag: [`AlwaysRun`](Self::AlwaysRun) when
    /// set, [`Walk`](Self::Walk) when clear.
    #[must_use]
    pub const fn from_always_run_flag(always_run: bool) -> Self {
        if always_run {
            Self::AlwaysRun
        } else {
            Self::Walk
        }
    }
}

/// Which start-location slot a `SetStartLocationRequest` records, mirroring the
/// reference viewer's `EStartLocation`. The everyday case is
/// [`Home`](Self::Home) — "set home to here" stores the accompanying region
/// position and look-at as the agent's home — but the message can target any
/// named slot.
///
/// Distinct from the login [`StartLocation`](crate::StartLocation), which is the
/// SLURL-style `start=` login parameter (`last` / `home` / `uri:Region&x&y&z`):
/// that enum names *where to log in*, whereas this one is the wire `LocationID`
/// of the request that *records* a home/last slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartLocationSlot {
    /// The agent's last-location slot (`START_LOCATION_ID_LAST`).
    Last,
    /// The agent's home slot (`START_LOCATION_ID_HOME`) — "set home to here".
    Home,
    /// A direct coordinate start (`START_LOCATION_ID_DIRECT`).
    Direct,
    /// A parcel start point (`START_LOCATION_ID_PARCEL`).
    Parcel,
    /// A telehub start point (`START_LOCATION_ID_TELEHUB`).
    Telehub,
    /// A SLURL-resolved start (`START_LOCATION_ID_URL`).
    Url,
}

impl StartLocationSlot {
    /// The wire `LocationID` value (the reference viewer's `EStartLocation`
    /// ordinal).
    #[must_use]
    pub const fn to_code(self) -> u32 {
        match self {
            Self::Last => 0,
            Self::Home => 1,
            Self::Direct => 2,
            Self::Parcel => 3,
            Self::Telehub => 4,
            Self::Url => 5,
        }
    }

    /// Classifies a `SetStartLocationRequest` `LocationID`, returning `None` for
    /// an unrecognised code.
    #[must_use]
    pub const fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Last),
            1 => Some(Self::Home),
            2 => Some(Self::Direct),
            3 => Some(Self::Parcel),
            4 => Some(Self::Telehub),
            5 => Some(Self::Url),
            _ => None,
        }
    }
}

/// A non-negative, finite bandwidth rate in **kilobits per second**, used for
/// the seven per-category rates of a [`Throttle`].
///
/// Constructing one through [`Kilobits::new`] guarantees the value is a real
/// number — not NaN, not infinite, and not negative — so a [`Throttle`] built
/// from `Kilobits` can never advertise a nonsensical bandwidth, and the
/// invariant holds for the life of the value (there is no way to mutate it into
/// an invalid state). Use [`Kilobits::new_unchecked`] only at the codec
/// boundary, where an inbound `AgentThrottle` must be reconstructed verbatim
/// from whatever the peer sent.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Kilobits(f32);

impl Kilobits {
    /// A zero bandwidth rate.
    pub const ZERO: Self = Self(0.0);

    /// Validates `rate` (kilobits per second) and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`ThrottleError::NotFinite`] for a NaN or infinite `rate`, or
    /// [`ThrottleError::Negative`] for a `rate` below zero.
    pub fn new(rate: f32) -> Result<Self, ThrottleError> {
        if !rate.is_finite() {
            return Err(ThrottleError::NotFinite);
        }
        if rate < 0.0 {
            return Err(ThrottleError::Negative);
        }
        Ok(Self(rate))
    }

    /// Wraps `rate` (kilobits per second) **without validation**.
    ///
    /// This is the codec-boundary constructor: an inbound `AgentThrottle` carries
    /// whatever per-category rates the peer sent, which must be reconstructed
    /// verbatim rather than rejected. For caller-supplied rates prefer the
    /// validating [`Kilobits::new`].
    #[must_use]
    pub const fn new_unchecked(rate: f32) -> Self {
        Self(rate)
    }

    /// The wrapped rate, in kilobits per second.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Why a [`Kilobits`] rate (and therefore a [`Throttle`]) was rejected: a
/// per-category bandwidth must be a finite, non-negative number of kilobits per
/// second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ThrottleError {
    /// The rate was NaN or infinite.
    #[error("bandwidth rate is not finite (NaN or infinite)")]
    NotFinite,
    /// The rate was negative.
    #[error("bandwidth rate is negative")]
    Negative,
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
///
/// The seven fields are private and validated on construction, so a throttle
/// can never hold a NaN, infinite, or negative rate. Build a custom split with
/// [`Throttle::builder`] (named per-category setters, which avoid the
/// transposition hazard of [`Throttle::new`]'s seven positional rates) and read
/// the categories back with the [`Throttle::resend`] … [`Throttle::asset`]
/// accessors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Throttle {
    /// Resent (reliable retransmit) traffic.
    resend: Kilobits,
    /// Land/terrain layer (`LayerData`) traffic.
    land: Kilobits,
    /// Wind layer traffic.
    wind: Kilobits,
    /// Cloud layer traffic.
    cloud: Kilobits,
    /// Task traffic: object updates (the scene graph).
    task: Kilobits,
    /// Texture (image) traffic.
    texture: Kilobits,
    /// Other asset traffic (sounds, animations, notecards, …).
    asset: Kilobits,
}

impl Throttle {
    /// Builds a throttle from the seven per-category rates (kilobits per second),
    /// in wire order: resend, land, wind, cloud, task, texture, asset.
    ///
    /// Because the seven positional arguments share one type and a fixed order
    /// they are easy to transpose; prefer [`Throttle::builder`] (named setters)
    /// or a preset ([`Throttle::preset_1000`] …) when the call is not obviously
    /// correct.
    ///
    /// # Errors
    ///
    /// Returns [`ThrottleError`] if any rate is NaN, infinite, or negative (see
    /// [`Kilobits::new`]).
    pub fn new(
        resend: f32,
        land: f32,
        wind: f32,
        cloud: f32,
        task: f32,
        texture: f32,
        asset: f32,
    ) -> Result<Self, ThrottleError> {
        Ok(Self {
            resend: Kilobits::new(resend)?,
            land: Kilobits::new(land)?,
            wind: Kilobits::new(wind)?,
            cloud: Kilobits::new(cloud)?,
            task: Kilobits::new(task)?,
            texture: Kilobits::new(texture)?,
            asset: Kilobits::new(asset)?,
        })
    }

    /// Builds a throttle from the seven per-category rates (kilobits per second,
    /// wire order) **without validation**.
    ///
    /// This is the codec-boundary / preset constructor: it wraps each rate with
    /// [`Kilobits::new_unchecked`], so an inbound `AgentThrottle` is
    /// reconstructed verbatim. For caller-supplied rates prefer the validating
    /// [`Throttle::new`] or [`Throttle::builder`].
    #[must_use]
    pub const fn new_unchecked(
        resend: f32,
        land: f32,
        wind: f32,
        cloud: f32,
        task: f32,
        texture: f32,
        asset: f32,
    ) -> Self {
        Self {
            resend: Kilobits::new_unchecked(resend),
            land: Kilobits::new_unchecked(land),
            wind: Kilobits::new_unchecked(wind),
            cloud: Kilobits::new_unchecked(cloud),
            task: Kilobits::new_unchecked(task),
            texture: Kilobits::new_unchecked(texture),
            asset: Kilobits::new_unchecked(asset),
        }
    }

    /// A [`ThrottleBuilder`] with every category at [`Kilobits::ZERO`]; set the
    /// categories you need by name and call [`ThrottleBuilder::build`].
    #[must_use]
    pub const fn builder() -> ThrottleBuilder {
        ThrottleBuilder::new()
    }

    /// The resend (reliable retransmit) rate.
    #[must_use]
    pub const fn resend(&self) -> Kilobits {
        self.resend
    }

    /// The land/terrain layer (`LayerData`) rate.
    #[must_use]
    pub const fn land(&self) -> Kilobits {
        self.land
    }

    /// The wind layer rate.
    #[must_use]
    pub const fn wind(&self) -> Kilobits {
        self.wind
    }

    /// The cloud layer rate.
    #[must_use]
    pub const fn cloud(&self) -> Kilobits {
        self.cloud
    }

    /// The task (object-update / scene-graph) rate.
    #[must_use]
    pub const fn task(&self) -> Kilobits {
        self.task
    }

    /// The texture (image) rate.
    #[must_use]
    pub const fn texture(&self) -> Kilobits {
        self.texture
    }

    /// The other-asset (sounds, animations, notecards, …) rate.
    #[must_use]
    pub const fn asset(&self) -> Kilobits {
        self.asset
    }

    /// The reference viewer's preset for a 300 kbps total bandwidth.
    #[must_use]
    pub const fn preset_300() -> Self {
        Self::new_unchecked(30.0, 40.0, 9.0, 9.0, 86.0, 86.0, 40.0)
    }

    /// The reference viewer's preset for a 500 kbps total bandwidth.
    #[must_use]
    pub const fn preset_500() -> Self {
        Self::new_unchecked(50.0, 70.0, 14.0, 14.0, 136.0, 136.0, 80.0)
    }

    /// The reference viewer's preset for a 1000 kbps total bandwidth.
    #[must_use]
    pub const fn preset_1000() -> Self {
        Self::new_unchecked(100.0, 100.0, 20.0, 20.0, 310.0, 310.0, 140.0)
    }

    /// The total requested bandwidth (kilobits per second), the sum of all seven
    /// categories.
    #[must_use]
    pub fn total(&self) -> f32 {
        self.resend.get()
            + self.land.get()
            + self.wind.get()
            + self.cloud.get()
            + self.task.get()
            + self.texture.get()
            + self.asset.get()
    }

    /// Rebuilds a throttle from the seven wire **bits per second** rates (in
    /// wire order: resend, land, wind, cloud, task, texture, asset), the exact
    /// inverse of [`Throttle::bits_per_second`]. Used by the simulator side to
    /// recover the client's requested per-category split from an inbound
    /// `AgentThrottle`.
    ///
    /// The peer's rates are accepted verbatim (via [`Kilobits::new_unchecked`]),
    /// not validated — this is the wire-decode boundary.
    #[must_use]
    pub fn from_bits_per_second(rates: [f32; 7]) -> Self {
        // 1 kilobit = 1024 bits, matching the reference viewer's conversion.
        const KILOBIT: f32 = 1024.0;
        let [resend, land, wind, cloud, task, texture, asset] = rates;
        Self {
            resend: Kilobits::new_unchecked(resend / KILOBIT),
            land: Kilobits::new_unchecked(land / KILOBIT),
            wind: Kilobits::new_unchecked(wind / KILOBIT),
            cloud: Kilobits::new_unchecked(cloud / KILOBIT),
            task: Kilobits::new_unchecked(task / KILOBIT),
            texture: Kilobits::new_unchecked(texture / KILOBIT),
            asset: Kilobits::new_unchecked(asset / KILOBIT),
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
            self.resend.get() * KILOBIT,
            self.land.get() * KILOBIT,
            self.wind.get() * KILOBIT,
            self.cloud.get() * KILOBIT,
            self.task.get() * KILOBIT,
            self.texture.get() * KILOBIT,
            self.asset.get() * KILOBIT,
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

/// A builder for [`Throttle`] with named per-category setters, avoiding the
/// transposition hazard of [`Throttle::new`]'s seven positional rates. Every
/// category defaults to [`Kilobits::ZERO`]; set the ones you need and call
/// [`ThrottleBuilder::build`].
///
/// Each setter takes an already-validated [`Kilobits`], so the build is
/// infallible and `const`-friendly; validate caller input once with
/// [`Kilobits::new`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThrottleBuilder {
    /// Resent (reliable retransmit) traffic.
    resend: Kilobits,
    /// Land/terrain layer (`LayerData`) traffic.
    land: Kilobits,
    /// Wind layer traffic.
    wind: Kilobits,
    /// Cloud layer traffic.
    cloud: Kilobits,
    /// Task traffic: object updates (the scene graph).
    task: Kilobits,
    /// Texture (image) traffic.
    texture: Kilobits,
    /// Other asset traffic (sounds, animations, notecards, …).
    asset: Kilobits,
}

impl ThrottleBuilder {
    /// A builder with every category at [`Kilobits::ZERO`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            resend: Kilobits::ZERO,
            land: Kilobits::ZERO,
            wind: Kilobits::ZERO,
            cloud: Kilobits::ZERO,
            task: Kilobits::ZERO,
            texture: Kilobits::ZERO,
            asset: Kilobits::ZERO,
        }
    }

    /// Sets the resend (reliable retransmit) rate.
    #[must_use]
    pub const fn resend(mut self, rate: Kilobits) -> Self {
        self.resend = rate;
        self
    }

    /// Sets the land/terrain layer (`LayerData`) rate.
    #[must_use]
    pub const fn land(mut self, rate: Kilobits) -> Self {
        self.land = rate;
        self
    }

    /// Sets the wind layer rate.
    #[must_use]
    pub const fn wind(mut self, rate: Kilobits) -> Self {
        self.wind = rate;
        self
    }

    /// Sets the cloud layer rate.
    #[must_use]
    pub const fn cloud(mut self, rate: Kilobits) -> Self {
        self.cloud = rate;
        self
    }

    /// Sets the task (object-update / scene-graph) rate.
    #[must_use]
    pub const fn task(mut self, rate: Kilobits) -> Self {
        self.task = rate;
        self
    }

    /// Sets the texture (image) rate.
    #[must_use]
    pub const fn texture(mut self, rate: Kilobits) -> Self {
        self.texture = rate;
        self
    }

    /// Sets the other-asset (sounds, animations, notecards, …) rate.
    #[must_use]
    pub const fn asset(mut self, rate: Kilobits) -> Self {
        self.asset = rate;
        self
    }

    /// Builds the [`Throttle`] from the configured per-category rates.
    #[must_use]
    pub const fn build(self) -> Throttle {
        Throttle {
            resend: self.resend,
            land: self.land,
            wind: self.wind,
            cloud: self.cloud,
            task: self.task,
            texture: self.texture,
            asset: self.asset,
        }
    }
}

impl Default for ThrottleBuilder {
    /// A builder with every category at [`Kilobits::ZERO`].
    fn default() -> Self {
        Self::new()
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
#[non_exhaustive]
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
#[non_exhaustive]
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
    /// The simulator forced a logout with a `KickUser` message (e.g. the same
    /// account logged in elsewhere). Paired with an [`Event::Kicked`] carrying
    /// the full [`Kick`] details.
    ///
    /// [`Event::Kicked`]: crate::types::Event::Kicked
    /// [`Kick`]: crate::types::Kick
    Kicked {
        /// The human-readable reason the simulator gave for the kick.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{Camera, CameraError, Kilobits, Throttle, ThrottleError, Vector, cross, dot};
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

    #[test]
    fn throttle_new_matches_the_raw_field_layout() -> Result<(), ThrottleError> {
        // The validating `new` keeps the seven rates in wire order, readable back
        // through the accessors bit-identically to the values passed in.
        let throttle = Throttle::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0)?;
        assert_eq!(throttle.resend(), Kilobits::new_unchecked(1.0));
        assert_eq!(throttle.land(), Kilobits::new_unchecked(2.0));
        assert_eq!(throttle.wind(), Kilobits::new_unchecked(3.0));
        assert_eq!(throttle.cloud(), Kilobits::new_unchecked(4.0));
        assert_eq!(throttle.task(), Kilobits::new_unchecked(5.0));
        assert_eq!(throttle.texture(), Kilobits::new_unchecked(6.0));
        assert_eq!(throttle.asset(), Kilobits::new_unchecked(7.0));
        // total() sums the seven categories (wrapped so the float comparison
        // runs through `Kilobits`' derived equality rather than a bare `==`).
        assert_eq!(
            Kilobits::new_unchecked(throttle.total()),
            Kilobits::new_unchecked(28.0)
        );
        Ok(())
    }

    #[test]
    fn throttle_builder_matches_positional_new() {
        // The named-setter builder produces the same throttle as the positional
        // constructor — the whole point is to avoid transposing the order.
        let built = Throttle::builder()
            .resend(Kilobits::new_unchecked(30.0))
            .land(Kilobits::new_unchecked(40.0))
            .wind(Kilobits::new_unchecked(9.0))
            .cloud(Kilobits::new_unchecked(9.0))
            .task(Kilobits::new_unchecked(86.0))
            .texture(Kilobits::new_unchecked(86.0))
            .asset(Kilobits::new_unchecked(40.0))
            .build();
        assert_eq!(built, Throttle::preset_300());
        // An unset builder category defaults to zero.
        let partial = Throttle::builder()
            .task(Kilobits::new_unchecked(100.0))
            .build();
        assert_eq!(partial.resend(), Kilobits::ZERO);
        assert_eq!(partial.task(), Kilobits::new_unchecked(100.0));
    }

    #[test]
    fn throttle_bits_per_second_round_trips() {
        // bits_per_second and from_bits_per_second are exact inverses, so a
        // throttle survives an encode/decode round trip bit-identically.
        let throttle = Throttle::preset_500();
        let restored = Throttle::from_bits_per_second(throttle.bits_per_second());
        assert_eq!(restored, throttle);
    }

    #[test]
    fn kilobits_new_rejects_invalid_rates() {
        assert_eq!(Kilobits::new(-1.0), Err(ThrottleError::Negative));
        assert_eq!(Kilobits::new(f32::NAN), Err(ThrottleError::NotFinite));
        assert_eq!(Kilobits::new(f32::INFINITY), Err(ThrottleError::NotFinite));
        assert_eq!(Kilobits::new(0.0), Ok(Kilobits::ZERO));
        assert_eq!(Kilobits::new(42.0), Ok(Kilobits::new_unchecked(42.0)));
    }

    #[test]
    fn throttle_new_rejects_a_negative_category() {
        // A single bad rate fails the whole construction.
        assert_eq!(
            Throttle::new(1.0, 2.0, 3.0, -4.0, 5.0, 6.0, 7.0),
            Err(ThrottleError::Negative)
        );
    }

    #[test]
    fn movement_mode_maps_to_always_run_flag() {
        use super::MovementMode;
        assert!(MovementMode::AlwaysRun.is_always_run());
        assert!(!MovementMode::Walk.is_always_run());
        assert_eq!(
            MovementMode::from_always_run_flag(true),
            MovementMode::AlwaysRun
        );
        assert_eq!(
            MovementMode::from_always_run_flag(false),
            MovementMode::Walk
        );
        // Round-trips bit-identically to the raw flag in both directions.
        for mode in [MovementMode::Walk, MovementMode::AlwaysRun] {
            assert_eq!(
                MovementMode::from_always_run_flag(mode.is_always_run()),
                mode
            );
        }
    }

    #[test]
    fn start_location_slot_round_trips_its_code() {
        use super::StartLocationSlot;
        for location in [
            StartLocationSlot::Last,
            StartLocationSlot::Home,
            StartLocationSlot::Direct,
            StartLocationSlot::Parcel,
            StartLocationSlot::Telehub,
            StartLocationSlot::Url,
        ] {
            assert_eq!(
                StartLocationSlot::from_code(location.to_code()),
                Some(location)
            );
        }
        assert_eq!(StartLocationSlot::Home.to_code(), 1);
        assert_eq!(StartLocationSlot::from_code(99), None);
    }
}
