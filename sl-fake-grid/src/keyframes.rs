//! Keyframed motion (`llSetKeyframedMotion`): the region plays an object's
//! frames, moving and turning it a tick at a time.
//!
//! A frame moves the object by its translation and turns it by its rotation
//! over its time, relative to where the frame found it — each tick adds its
//! share, so a motion moved aside by `llSetPos` on OpenSim goes on from
//! there (`physics/keyframed`), and a stop leaves the object where it is.
//! Reverse plays the frames backwards, each move undone and each turn
//! conjugated (OpenSim's `KeyframeMotion.GetNextList`); a loop starts again
//! where it ended; a ping-pong turns round at each end. A frame shorter than
//! a tick — OpenSim plays one of 0.05 s or −1 s as given — is done in one.

use sl_lsl_runtime::vm::physics::{KeyframeMode, Keyframes};
use sl_types::lsl::{Rotation, Vector};

/// The identity rotation.
const IDENTITY: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// Whether a player is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Run {
    /// It plays.
    Playing,
    /// It holds where it is, to go on from there.
    Paused,
    /// It ended or was stopped; a play starts it again from its first
    /// frame, where the object is.
    Stopped,
}

/// An object's keyframed motion as it plays.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Player {
    /// The motion.
    motion: Keyframes,
    /// Whether it goes.
    pub(crate) run: Run,
    /// The frame it is in, in playing order.
    frame: usize,
    /// How far through that frame, seconds.
    into: f32,
    /// Whether this pass plays the frames backwards.
    backwards: bool,
    /// Seconds to wait before the first frame starts.
    pub(crate) wait: f32,
}

/// What a tick of a player does to its object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Moved {
    /// How far it moves, region axes.
    pub(crate) by: Vector,
    /// How it turns, applied after its rotation (`rotation * turn`).
    pub(crate) turn: Rotation,
    /// Whether the motion ended in this tick.
    pub(crate) ended: bool,
}

/// `a + b`.
fn plus(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
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

/// The share `t` (0 to 1) of the turn `to` from the identity, along the
/// shorter arc.
fn share_of(to: &Rotation, t: f32) -> Rotation {
    let dot = to.s;
    let (to, dot) = if dot < 0.0 {
        (
            Rotation {
                x: -to.x,
                y: -to.y,
                z: -to.z,
                s: -to.s,
            },
            -dot,
        )
    } else {
        (to.clone(), dot)
    };
    let angle = dot.clamp(-1.0, 1.0).acos();
    let sine = angle.sin();
    if sine.abs() < 1e-6 {
        return normalised(&Rotation {
            x: to.x * t,
            y: to.y * t,
            z: to.z * t,
            s: (1.0 - t).mul_add(1.0, t * to.s),
        });
    }
    let from_share = ((1.0 - t) * angle).sin() / sine;
    let to_share = (t * angle).sin() / sine;
    normalised(&Rotation {
        x: to.x * to_share,
        y: to.y * to_share,
        z: to.z * to_share,
        s: from_share + to.s * to_share,
    })
}

/// `r` at unit length.
fn normalised(r: &Rotation) -> Rotation {
    let length =
        r.s.mul_add(r.s, r.z.mul_add(r.z, r.x.mul_add(r.x, r.y * r.y)))
            .sqrt();
    if length == 0.0 {
        return IDENTITY;
    }
    Rotation {
        x: r.x / length,
        y: r.y / length,
        z: r.z / length,
        s: r.s / length,
    }
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

impl Player {
    /// `motion`, playing from its first frame after `wait` seconds.
    pub(crate) const fn new(motion: Keyframes, wait: f32) -> Self {
        let backwards = matches!(motion.mode, KeyframeMode::Reverse);
        Self {
            motion,
            run: Run::Playing,
            frame: 0,
            into: 0.0,
            backwards,
            wait,
        }
    }

    /// Whether the motion has no frames: OpenSim keeps a motion whose only
    /// frame was cut short, and it never plays.
    pub(crate) const fn is_empty(&self) -> bool {
        self.motion.frames.is_empty()
    }

    /// Plays again from the first frame, where the object is now.
    pub(crate) const fn restart(&mut self) {
        self.frame = 0;
        self.into = 0.0;
        self.backwards = matches!(self.motion.mode, KeyframeMode::Reverse);
        self.run = Run::Playing;
    }

    /// The frame being played: its move, its turn and its time, as this pass
    /// plays it.
    fn current(&self) -> Option<(Vector, Rotation, f32)> {
        let count = self.motion.frames.len();
        let index = if self.backwards {
            count.checked_sub(self.frame.checked_add(1)?)?
        } else {
            self.frame
        };
        let frame = self.motion.frames.get(index)?;
        let translation = frame.translation.clone().unwrap_or(ZERO);
        let rotation = frame.rotation.clone().unwrap_or(IDENTITY);
        Some(if self.backwards {
            (
                scaled(&translation, -1.0),
                conjugate(&rotation),
                frame.seconds,
            )
        } else {
            (translation, rotation, frame.seconds)
        })
    }

    /// One tick of `dt` seconds: what it does to the object, [`None`] when it
    /// does not play.
    pub(crate) fn tick(&mut self, dt: f32) -> Option<Moved> {
        if self.run != Run::Playing {
            return None;
        }
        let mut left = dt;
        if self.wait > 0.0 {
            let waited = self.wait.min(left);
            self.wait -= waited;
            left -= waited;
            if left <= 0.0 {
                return Some(Moved {
                    by: ZERO,
                    turn: IDENTITY,
                    ended: false,
                });
            }
        }
        let mut moved = Moved {
            by: ZERO,
            turn: IDENTITY,
            ended: false,
        };
        // What was left when the pass began: a pass that spends none of it
        // (frames of no time, looping) is not run again in this tick.
        let mut pass_began = f32::INFINITY;
        loop {
            if self.frame >= self.motion.frames.len() {
                if !self.next_pass() {
                    self.run = Run::Stopped;
                    moved.ended = true;
                    break;
                }
                if left >= pass_began {
                    break;
                }
                pass_began = left;
            }
            let Some((translation, rotation, seconds)) = self.current() else {
                break;
            };
            let (from, to, spent) = if seconds <= 0.0 {
                (0.0, 1.0, 0.0)
            } else {
                if left <= 0.0 {
                    break;
                }
                let spent = left.min(seconds - self.into);
                (self.into / seconds, (self.into + spent) / seconds, spent)
            };
            moved.by = plus(&moved.by, &scaled(&translation, to - from));
            let step = crate::object_edits::multiply(
                &share_of(&rotation, to),
                &conjugate(&share_of(&rotation, from)),
            );
            moved.turn = crate::object_edits::multiply(&step, &moved.turn);
            left -= spent;
            self.into += spent;
            if seconds <= 0.0 || self.into >= seconds {
                self.frame = self.frame.saturating_add(1);
                self.into = 0.0;
            } else {
                break;
            }
        }
        Some(moved)
    }

    /// Starts the next pass of a loop or a ping-pong; `false` for a motion
    /// that plays once.
    const fn next_pass(&mut self) -> bool {
        match self.motion.mode {
            KeyframeMode::Forward | KeyframeMode::Reverse => false,
            KeyframeMode::Loop => {
                self.frame = 0;
                self.into = 0.0;
                true
            }
            KeyframeMode::PingPong => {
                self.frame = 0;
                self.into = 0.0;
                self.backwards = !self.backwards;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_lsl_runtime::vm::physics::Keyframe;

    use super::*;

    /// One frame half a metre up in a second.
    fn up(mode: KeyframeMode) -> Keyframes {
        Keyframes {
            frames: vec![Keyframe {
                translation: Some(Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.5,
                }),
                rotation: Some(IDENTITY),
                seconds: 1.0,
            }],
            mode,
        }
    }

    /// Runs `player` for `ticks` ticks of a tenth of a second, the height
    /// gained and whether it ended.
    fn run(player: &mut Player, ticks: usize) -> (f32, bool) {
        let mut height = 0.0;
        let mut ended = false;
        for _tick in 0..ticks {
            if let Some(moved) = player.tick(0.1) {
                height += moved.by.z;
                ended |= moved.ended;
            }
        }
        (height, ended)
    }

    #[test]
    fn a_frame_moves_its_translation_over_its_time_then_ends() {
        let mut player = Player::new(up(KeyframeMode::Forward), 0.0);
        let (height, ended) = run(&mut player, 5);
        assert!((height - 0.25).abs() < 1e-4, "{height}");
        assert!(!ended);
        let (height, ended) = run(&mut player, 10);
        assert!((height - 0.25).abs() < 1e-4, "{height}");
        assert!(ended);
        assert_eq!(player.run, Run::Stopped);
    }

    #[test]
    fn reverse_goes_down_and_a_frame_of_no_time_is_done_at_once() {
        let mut player = Player::new(up(KeyframeMode::Reverse), 0.0);
        let (height, ended) = run(&mut player, 15);
        assert!((height + 0.5).abs() < 1e-4, "{height}");
        assert!(ended);
        let mut motion = up(KeyframeMode::Forward);
        if let Some(frame) = motion.frames.first_mut() {
            frame.seconds = -1.0;
        }
        let mut player = Player::new(motion, 0.0);
        let (height, ended) = run(&mut player, 1);
        assert!((height - 0.5).abs() < 1e-4, "{height}");
        assert!(ended);
    }

    #[test]
    fn a_loop_goes_on_and_a_wait_holds_the_start() {
        let mut player = Player::new(up(KeyframeMode::Loop), 0.0);
        let (height, ended) = run(&mut player, 25);
        assert!((height - 1.25).abs() < 1e-3, "{height}");
        assert!(!ended);
        let mut late = Player::new(up(KeyframeMode::Forward), 0.5);
        let (height, _ended) = run(&mut late, 10);
        assert!((height - 0.25).abs() < 1e-3, "{height}");
    }
}
