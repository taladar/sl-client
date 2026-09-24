//! **Hold to repeat**: a button that keeps firing while it is held.
//!
//! The reference's step buttons — a scrollbar's arrows, a tab strip's
//! previous / next — are `LLButton`s with a `mouse_held_callback`: the action
//! once on the press, then again and again once the button has been held past
//! its `held_down_delay` (0.5 s in `widgets/button.xml`), until it is let go.
//!
//! Here that is a component, not a widget. A `bevy_ui_widgets` [`Button`]
//! with [`ActivateOnPress`] already fires [`Activate`] on the press and keeps
//! [`Pressed`] on itself until the release, a drag's end or a cancel — the
//! three ways a hold stops. [`HoldToRepeat`] adds the rest: while `Pressed`
//! stays past the delay, `repeat_held_buttons` triggers `Activate` again at
//! a fixed rate. So the button's own observer is the whole of its behaviour,
//! and it cannot tell a repeat from a press — which is the point: the step is
//! written once.
//!
//! The repeat is a fixed rate (`REPEAT_INTERVAL`) where the reference
//! repeats once per frame, so a list does not scroll faster on a faster
//! machine, and at most once per frame, so a long frame never throws a list
//! several steps at once.
//!
//! [`Button`]: bevy::ui_widgets::Button

use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Activate;

#[cfg(doc)]
use bevy::ui_widgets::ActivateOnPress;

/// How long a button must be held before it starts repeating, in seconds — the
/// reference button's `held_down_delay`.
const HOLD_DELAY: f64 = 0.5;

/// The time between two repeats of a held button, in seconds. The reference
/// repeats once per frame; a fixed rate is the same feel at 20 frames a second
/// and does not run away at 144.
const REPEAT_INTERVAL: f64 = 0.05;

/// On a [`Button`](bevy::ui_widgets::Button) with
/// [`ActivateOnPress`]: fire [`Activate`] again for as long as it is held.
///
/// The fields are the hold's own clock, reset whenever [`Pressed`] arrives, so
/// a spawner inserts `HoldToRepeat::default()` and never touches it again.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct HoldToRepeat {
    /// [`Time<Real>`]'s elapsed seconds when the current hold began.
    pressed_at: f64,
    /// How many repeats the current hold has fired.
    repeats: u32,
}

impl HoldToRepeat {
    /// When the next repeat is due, in [`Time<Real>`] elapsed seconds: the
    /// hold delay, then one interval per repeat already fired.
    fn next_repeat_at(self) -> f64 {
        self.pressed_at + HOLD_DELAY + f64::from(self.repeats) * REPEAT_INTERVAL
    }
}

/// The plugin that makes [`HoldToRepeat`] repeat. Without it a held button
/// still fires once on the press — the observer needs nothing registered —
/// and simply never repeats.
///
/// Added by every plugin whose widgets use it (the scrollbar's, the tab
/// strip's) when it is not already there, so no host has to know it exists.
#[derive(Debug, Clone, Copy, Default)]
pub struct HoldRepeatPlugin;

impl Plugin for HoldRepeatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, repeat_held_buttons);
    }
}

/// Add [`HoldRepeatPlugin`] to `app` unless something already has — the one
/// line a widget plugin that relies on it calls from its own `build`.
pub fn ensure_hold_repeat(app: &mut App) {
    if !app.is_plugin_added::<HoldRepeatPlugin>() {
        app.add_plugins(HoldRepeatPlugin);
    }
}

/// Start the clock on a button that has just been pressed, and re-fire
/// [`Activate`] on one that has been held long enough for its next repeat.
///
/// The clock is optional: this rides in with every widget plugin that spawns
/// a scrollbar, and an app built without `TimePlugin` — a headless test of a
/// panel that has nothing to hold — must not fail parameter validation on it.
/// With no clock nothing repeats, which is what such an app wants anyway.
fn repeat_held_buttons(
    mut commands: Commands,
    time: Option<Res<Time<Real>>>,
    mut held: Query<(Entity, Ref<Pressed>, &mut HoldToRepeat)>,
) {
    let Some(time) = time else {
        return;
    };
    let now = time.elapsed_secs_f64();
    for (entity, pressed, mut hold) in &mut held {
        if pressed.is_added() {
            *hold = HoldToRepeat {
                pressed_at: now,
                repeats: 0,
            };
            continue;
        }
        if hold.next_repeat_at() <= now {
            hold.repeats = hold.repeats.saturating_add(1);
            commands.trigger(Activate {
                entity,
                button: Some(PointerButton::Primary),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HOLD_DELAY, HoldToRepeat, REPEAT_INTERVAL};
    use pretty_assertions::assert_eq;

    /// A held button waits out the delay, then repeats once per interval.
    #[expect(
        clippy::float_cmp,
        reason = "the schedule is sums of representable constants, asserted exactly"
    )]
    #[test]
    fn a_hold_waits_the_delay_then_repeats_per_interval() {
        let hold = HoldToRepeat {
            pressed_at: 10.0,
            repeats: 0,
        };
        assert_eq!(hold.next_repeat_at(), 10.0 + HOLD_DELAY);
        let later = HoldToRepeat { repeats: 3, ..hold };
        assert_eq!(
            later.next_repeat_at(),
            10.0 + HOLD_DELAY + 3.0 * REPEAT_INTERVAL
        );
    }
}
