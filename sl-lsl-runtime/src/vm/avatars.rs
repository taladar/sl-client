//! What a script does to and about avatars (`server-lsl-lib-avatar-control`):
//! the records a host lends of an avatar, and each grid's rules for what it
//! sends when a script takes an avatar's controls or sets its camera, and how
//! often a held key raises `control`.
//!
//! The host keeps the avatars — where they are, what they play, what they
//! hold down — and sends the viewer what the library decided; the shapes of
//! those messages and the event's timing are measured
//! (`book/src/gridspec/lsl.md` § Animations, sitting, controls and camera) and
//! live here so a test host and the fake grid send alike.

use sl_types::lsl::Vector;

use crate::vm::EngineFlavour;

/// An avatar in the region, as the library reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    /// Its legacy name, `First Last`.
    pub legacy_name: String,
    /// Its display name.
    pub display_name: String,
    /// Its user name, `first.last` (`first` for a `Resident`).
    pub username: String,
    /// Its size (`llGetAgentSize`).
    pub size: Vector,
    /// Its `AGENT_*` bits (`llGetAgentInfo`).
    pub info: i32,
    /// The language its viewer reports (`llGetAgentLanguage`).
    pub language: String,
    /// Its movement state as `llGetAnimation` names it: `Standing`,
    /// `Walking`, `Sitting`, `Flying`, …
    pub state: String,
    /// The animations playing on it, by asset key.
    pub animations: Vec<String>,
}

/// What `llRequestAgentData` reads of any avatar of the grid.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentRecord {
    /// Whether it is logged in.
    pub online: bool,
    /// Its legacy name.
    pub legacy_name: String,
    /// Its display name.
    pub display_name: String,
    /// Its user name.
    pub username: String,
    /// When it was born, `YYYY-MM-DD`.
    pub born: String,
    /// Its payment information bits (`PAYMENT_INFO_*`).
    pub payment: i32,
}

/// A `ScriptControlChange` a grid sends the avatar's viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlChange {
    /// Whether the controls are taken (`true`) or released.
    pub take: bool,
    /// The control bits, as sent.
    pub controls: u32,
    /// Whether the viewer is to act on them too.
    pub pass_to_agent: bool,
}

/// The controls a script holds, as the last `ScriptControlChange` it caused
/// stated them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// The control bits as sent.
    pub controls: u32,
    /// Whether they were passed on.
    pub pass_to_agent: bool,
}

/// `CONTROL_FWD`'s nudge counterpart and its five siblings: Second Life takes
/// the nudge of every direction a script takes.
fn with_nudges(controls: u32) -> u32 {
    (0..6_u32).fold(controls, |bits, direction| {
        if controls & (1 << direction) != 0 {
            bits | (1 << direction.saturating_add(19))
        } else {
            bits
        }
    })
}

/// What `flavour`'s grid sends when a script takes `controls` (`accept`,
/// `pass_on`) holding `held` before, and what it holds after.
///
/// Second Life releases what was held (everything, the first time, passed
/// to nobody) and takes the controls with their nudges — or, not accepting
/// them, everything else, passed on. OpenSim releases the controls twice,
/// once passed on and once not, and takes them as given.
#[must_use]
pub fn take(
    flavour: EngineFlavour,
    held: Option<Held>,
    controls: i32,
    accept: bool,
    pass_on: bool,
) -> (Vec<ControlChange>, Held) {
    let bits = u32::from_ne_bytes(controls.to_ne_bytes());
    if flavour.is_opensim() {
        let changes = vec![
            ControlChange {
                take: false,
                controls: bits,
                pass_to_agent: false,
            },
            ControlChange {
                take: false,
                controls: bits,
                pass_to_agent: true,
            },
            ControlChange {
                take: true,
                controls: bits,
                pass_to_agent: pass_on,
            },
        ];
        return (
            changes,
            Held {
                controls: bits,
                pass_to_agent: pass_on,
            },
        );
    }
    let released = held.map_or(
        ControlChange {
            take: false,
            controls: u32::MAX,
            pass_to_agent: false,
        },
        |held| ControlChange {
            take: false,
            controls: held.controls,
            pass_to_agent: held.pass_to_agent,
        },
    );
    let taken = if accept {
        with_nudges(bits)
    } else {
        !with_nudges(bits)
    };
    let now = Held {
        controls: taken,
        pass_to_agent: pass_on,
    };
    (
        vec![
            released,
            ControlChange {
                take: true,
                controls: taken,
                pass_to_agent: pass_on,
            },
        ],
        now,
    )
}

/// What `flavour`'s grid sends when a script holding `held` releases its
/// controls: Second Life releases what was taken; OpenSim sends nothing.
#[must_use]
pub fn release(flavour: EngineFlavour, held: Held) -> Vec<ControlChange> {
    if flavour.is_opensim() {
        return Vec::new();
    }
    vec![ControlChange {
        take: false,
        controls: held.controls,
        pass_to_agent: held.pass_to_agent,
    }]
}

/// A value of a follow-camera rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraRule {
    /// The `FollowCamProperty` id.
    pub property: i32,
    /// Its value.
    pub value: f32,
}

/// The follow-camera properties `flavour`'s grid sends for a script's
/// `llSetCameraParams` rules, already read into `(rule, x, y, z)` (a scalar
/// in `x`): a vector rule as its three components (`CAMERA_FOCUS_OFFSET` 1
/// as 2, 3, 4; `CAMERA_FOCUS` 17 as 18, 19, 20; `CAMERA_POSITION` 13 as 14,
/// 15, 16), no value clamped; Second Life in the script's order, OpenSim by
/// property.
#[must_use]
pub fn camera_rules(flavour: EngineFlavour, rules: &[(i32, f32, f32, f32)]) -> Vec<CameraRule> {
    let mut out = Vec::new();
    for &(rule, x, y, z) in rules {
        if matches!(rule, 1 | 13 | 17) {
            for (offset, value) in [(1, x), (2, y), (3, z)] {
                out.push(CameraRule {
                    property: rule.saturating_add(offset),
                    value,
                });
            }
        } else {
            out.push(CameraRule {
                property: rule,
                value: x,
            });
        }
    }
    if flavour.is_opensim() {
        out.sort_by_key(|rule| rule.property);
    }
    out
}

/// What a script that took controls hears of the keys held: the controls it
/// took, what was held at the last tick, and how many repeats it has had of
/// the present hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Controls {
    /// The controls the script takes events of.
    pub taken: i32,
    /// What was held at the last tick, of those.
    pub level: i32,
    /// The ticks since the hold last changed.
    pub held_for: u64,
}

impl Controls {
    /// The keys held at a tick, `held`: the `control(level, edge)` to raise,
    /// if any. A change raises one at once; an unchanged hold repeats on
    /// Second Life every second tick (21 in a second's hold, `avatar/controls`)
    /// and on OpenSim once, the tick after the change.
    pub fn tick(&mut self, flavour: EngineFlavour, held: i32) -> Option<(i32, i32)> {
        let level = held & self.taken;
        let edge = level ^ self.level;
        self.level = level;
        if edge != 0 {
            self.held_for = 0;
            return Some((level, edge));
        }
        self.held_for = self.held_for.saturating_add(1);
        if level == 0 {
            return None;
        }
        let repeats = if flavour.is_opensim() {
            self.held_for == 1
        } else {
            self.held_for.is_multiple_of(2)
        };
        repeats.then_some((level, 0))
    }
}

/// The default animation of the movement state `state` (`llGetAnimation`'s
/// name), as Second Life's `llGetAnimationOverride` names it when nothing
/// overrides it.
#[must_use]
pub fn default_animation(state: &str) -> &'static str {
    match state {
        "Crouching" => "crouch",
        "CrouchWalking" => "crouchwalk",
        "Falling Down" => "falldown",
        "Flying" => "fly",
        "FlyingSlow" => "flyslow",
        "Hovering" => "hover",
        "Hovering Down" => "hover_down",
        "Hovering Up" => "hover_up",
        "Jumping" => "jump",
        "Landing" => "land",
        "PreJumping" => "prejump",
        "Running" => "run",
        "Sitting" => "sit",
        "Sitting on Ground" => "sit_ground_constrained",
        "Soft Landing" => "soft_land",
        "Standing Up" => "standup",
        "Striding" => "stride",
        "Swimming" => "swim",
        "Turning Left" => "turnleft",
        "Turning Right" => "turnright",
        "Walking" => "walk",
        _ => "stand",
    }
}

/// The movement states an animation override can name.
pub const OVERRIDE_STATES: &[&str] = &[
    "Crouching",
    "CrouchWalking",
    "Falling Down",
    "Flying",
    "FlyingSlow",
    "Hovering",
    "Hovering Down",
    "Hovering Up",
    "Jumping",
    "Landing",
    "PreJumping",
    "Running",
    "Sitting",
    "Sitting on Ground",
    "Standing",
    "Standing Up",
    "Striding",
    "Soft Landing",
    "Taking Off",
    "Turning Left",
    "Turning Right",
    "Walking",
];

/// Where a scripted teleport sends an avatar.
#[derive(Debug, Clone, PartialEq)]
pub enum Destination {
    /// Home (`llTeleportAgentHome`).
    Home,
    /// A landmark of the caller's prim, by its asset (`llTeleportAgent`
    /// with a landmark's name).
    Landmark {
        /// The landmark's asset key.
        asset: String,
        /// The way the avatar looks on arrival.
        look_at: Vector,
    },
    /// A place in the caller's region (`llTeleportAgent` with an empty
    /// landmark).
    Local {
        /// The position.
        position: Vector,
        /// The way the avatar looks on arrival.
        look_at: Vector,
    },
    /// A place on the grid (`llTeleportAgentGlobalCoords`).
    Global {
        /// The region's corner, in metres.
        corner: (f64, f64),
        /// The position in it.
        position: Vector,
        /// The way the avatar looks on arrival.
        look_at: Vector,
    },
}
