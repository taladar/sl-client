//! The app's one double-click window.
//!
//! Every surface that tells a double click from two single clicks by timing
//! the presses itself — an inventory row, a radar row, a friend, a group, a
//! world click — asks this window how far apart two presses may be. It is the
//! picking plugin's [`PickingSettings::multi_click_interval`], the same one
//! picking counts [`Pointer<Press>::count`](bevy::picking::events::Press)
//! with, so a list and a widget never disagree about one gesture.
//!
//! It is also what the synthetic input pins while it plays a click
//! ([`crate::synthetic_input`]): a double click played two frames apart is
//! one however long those frames took, which a constant measured on the frame
//! clock is not — at a few frames a second each frame can advance the clock by
//! a quarter of a second, and a 0.35 s window then misses a double click that
//! is two frames long.

use bevy::ecs::system::SystemParam;
use bevy::picking::PickingSettings;
use bevy::prelude::*;

/// The window, in seconds, in a host with no picking settings to say (a
/// headless fixture): picking's own default.
pub const FALLBACK_DOUBLE_CLICK_SECS: f64 = 0.5;

/// The app's double-click window, read from its picking settings, and the
/// clock presses are timed on. See the [module documentation](self).
#[derive(Debug, SystemParam)]
pub struct DoubleClickWindow<'w> {
    /// The clock presses are timed on: the app's [`Time`], whose elapsed
    /// seconds every tracker stores.
    time: Option<Res<'w, Time>>,
    /// The picking settings whose multi-click interval is the window. Absent
    /// in a host without picking, which then uses
    /// [`FALLBACK_DOUBLE_CLICK_SECS`].
    picking: Option<Res<'w, PickingSettings>>,
}

impl DoubleClickWindow<'_> {
    /// The time, in seconds since startup, to stamp a press with — zero in a
    /// host with no clock, where nothing is ever a double click's second half
    /// by timing alone.
    #[must_use]
    pub fn now_secs(&self) -> f64 {
        self.time
            .as_ref()
            .map_or(0.0, |time| time.elapsed_secs_f64())
    }

    /// How far apart, in seconds, two presses may be and still be one double
    /// click.
    #[must_use]
    pub fn interval_secs(&self) -> f64 {
        self.picking
            .as_ref()
            .map_or(FALLBACK_DOUBLE_CLICK_SECS, |picking| {
                picking.multi_click_interval.as_secs_f64()
            })
    }

    /// Whether a press `elapsed_secs` after the one before completes a double
    /// click with it.
    #[must_use]
    pub fn holds(&self, elapsed_secs: f64) -> bool {
        elapsed_secs <= self.interval_secs()
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use bevy::ecs::system::SystemState;
    use bevy::picking::PickingSettings;
    use bevy::prelude::*;

    use super::{DoubleClickWindow, FALLBACK_DOUBLE_CLICK_SECS};

    /// The window is the picking settings' multi-click interval — so a pinned
    /// interval (the synthetic input's two-frame double click) holds however
    /// long the frames took — and picking's own default without them.
    #[test]
    fn the_window_is_the_picking_interval() -> Result<(), Box<dyn core::error::Error>> {
        let mut world = World::new();
        let mut state = SystemState::<DoubleClickWindow>::new(&mut world);
        {
            let window = state.get(&world)?;
            assert!((window.interval_secs() - FALLBACK_DOUBLE_CLICK_SECS).abs() < f64::EPSILON);
            assert!(window.holds(0.45));
            assert!(!window.holds(0.6));
            assert!(
                window.now_secs().abs() < f64::EPSILON,
                "no clock reads zero"
            );
        }

        world.insert_resource(PickingSettings {
            multi_click_interval: Duration::MAX,
            ..PickingSettings::default()
        });
        world.insert_resource(Time::<()>::default());
        let mut state = SystemState::<DoubleClickWindow>::new(&mut world);
        let window = state.get(&world)?;
        assert!(
            window.holds(0.5),
            "two starved frames apart is still one double click"
        );
        assert!(window.holds(3600.0));
        Ok(())
    }
}
