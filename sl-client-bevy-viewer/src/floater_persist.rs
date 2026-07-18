//! **Per-user floater geometry persistence** (`viewer-ui-floater-persist-geometry`):
//! a floater remembers where it was, how big it was, whether it was minimized /
//! docked, and whether it was open — across sessions, per avatar.
//!
//! This is the reference viewer's model, ported onto our own store: every
//! floater's rect is a saved control keyed by the floater's name
//! (`LLFloater::storeRectControl` / `applyRectControl`, plus the `save_visibility`
//! control), so any floater gets remembered geometry **for free** just by having
//! a [`Floater::id`](crate::floater::Floater) — the inventory window today, every
//! future floater tomorrow.
//!
//! # Where it is stored, and why per avatar
//!
//! The persisted values live in the [`Account`](sl_settings::Scope::Account)
//! scope of [`ViewerSettings`] — the per-(grid, avatar-name) settings file
//! ([`crate::settings`]). The reference keeps floater rects in the *global*
//! `gSavedSettings`; we deliberately key them per avatar instead, so two
//! characters on one machine keep their own window layouts.
//!
//! Four settings per floater, grouped under `[floater]` in the file:
//!
//! ```toml
//! [floater]
//! # Window rectangle (logical px [left, top, right, bottom]); ...
//! inventory_rect = [20, 60, 320, 460]
//! # Whether the window is open
//! inventory_visible = true
//! # Whether collapsed to its title bar
//! inventory_minimized = false
//! # Whether docked into its host
//! inventory_docked = false
//! ```
//!
//! # The four-stage lifecycle
//!
//! - **Register** ([`register_floater_settings`]) — the frame a floater is
//!   spawned, declare its four settings (with the spec geometry as the default).
//!   This runs before login, so the account file that loads later is coerced to
//!   the declared types.
//! - **Seed** ([`seed_floaters_from_settings`]) — once the account scope is
//!   loaded (post login), apply any *stored* value back onto the floater, keyed
//!   by [`SettingsStore::is_overridden`](sl_settings::SettingsStore::is_overridden)
//!   so a floater with nothing saved keeps its `FloaterSpec` default. The
//!   manager's own [`clamp_floaters_on_screen`](crate::floater) then recovers a
//!   rect saved on a larger monitor: a window restored off-screen on a smaller
//!   display is pulled back so its title bar stays reachable.
//! - **Persist** ([`persist_floater_changes`]) — on any move / resize / minimize
//!   / dock / open / close, write the floater's geometry back into the store and
//!   mark it dirty.
//! - **Flush** ([`flush_floater_settings`]) — write the store to disk at most
//!   once every [`FLUSH_INTERVAL_SECS`] while dirty, so a crash mid-session loses
//!   at most that window of adjustments rather than the whole session (the clean
//!   logout save in [`crate::session`] flushes the rest).
//!
//! Reference (Firestorm, read-only): `indra/llui/llfloater.cpp`
//! (`storeRectControl` / `applyRectControl` / `storeVisibilityControl`).

use bevy::prelude::*;

use sl_settings::SettingValue;

use crate::floater::{Floater, FloaterCommand, FloaterGeometry, FloaterOp};
use crate::settings::{ViewerSettings, load_account_settings};
use crate::ui::UiPanelShown;

/// The persisted-file section every floater's settings are grouped under
/// (`[floater]`). The per-floater id in each name keeps the flat keys distinct.
const FLOATER_SECTION: &[&str] = &["floater"];

/// How long a change may sit unsaved before the store is flushed to disk, in
/// seconds. Long enough that a burst of drags coalesces into one write, short
/// enough that a crash never costs a whole session's worth of window tuning.
const FLUSH_INTERVAL_SECS: f32 = 30.0;

/// Stored pixel coordinates are clamped to the `i16` range on the way in and out
/// of the `i32` rect. No display is anywhere near ±32 k logical pixels, so the
/// clamp only ever fires on a corrupt value — and staying inside `i16` keeps the
/// `f32`↔`i32` conversion provably lossless and non-truncating.
const STORED_PX_MIN: f32 = -32768.0;
/// The upper `i16` clamp bound; see [`STORED_PX_MIN`].
const STORED_PX_MAX: f32 = 32767.0;

/// The plugin that wires the four persistence systems and their debounce state.
///
/// Every system tolerates a missing [`ViewerSettings`] (an app without the
/// settings store, e.g. the gallery) by early-returning, so adding the plugin is
/// always safe.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FloaterPersistPlugin;

impl Plugin for FloaterPersistPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloaterPersistDirty>().add_systems(
            Update,
            (
                register_floater_settings,
                // Seed only after the account scope is in, so a stored rect is
                // actually present to read.
                seed_floaters_from_settings.after(load_account_settings),
                persist_floater_changes,
                flush_floater_settings,
            )
                .chain(),
        );
    }
}

/// The debounce clock: the elapsed-time of the first change not yet flushed to
/// disk, or `None` when everything is saved.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct FloaterPersistDirty {
    /// When the oldest unsaved change happened ([`Time::elapsed_secs`]).
    since: Option<f32>,
}

/// Marks a floater whose stored geometry has already been applied, so seeding
/// runs exactly once and the persistence write-back only fires for real user
/// changes after the seed (not for the seed itself).
#[derive(Component, Debug, Clone, Copy)]
struct FloaterSeeded;

/// The query filter [`persist_floater_changes`] runs on: a seeded floater whose
/// geometry *or* open state changed this frame. Aliased to keep the system
/// signature readable (and clear of `clippy::type_complexity`).
type ChangedSeededFloater = (
    With<FloaterSeeded>,
    Or<(Changed<Floater>, Changed<UiPanelShown>)>,
);

/// The `[floater]` setting name for a floater's remembered rectangle.
fn rect_key(id: &str) -> String {
    format!("{id}_rect")
}

/// The setting name for whether a floater is open.
fn visible_key(id: &str) -> String {
    format!("{id}_visible")
}

/// The setting name for whether a floater is minimized.
fn minimized_key(id: &str) -> String {
    format!("{id}_minimized")
}

/// The setting name for whether a floater is docked.
fn docked_key(id: &str) -> String {
    format!("{id}_docked")
}

/// Encode a floater's geometry as the `[left, top, right, bottom]` rect stored on
/// disk, in logical pixels.
///
/// `right`/`bottom` carry the content size as `left + width` / `top + height`; a
/// content-driven floater (no explicit size) stores a zero-extent rect
/// (`right == left`), which [`decode_rect`] reads back as "size unset".
fn encode_rect(geometry: FloaterGeometry) -> [i32; 4] {
    let (width, height) = match geometry.content_size {
        Some(size) => (size.x, size.y),
        None => (0.0, 0.0),
    };
    [
        round_px(geometry.position.x),
        round_px(geometry.position.y),
        round_px(geometry.position.x + width),
        round_px(geometry.position.y + height),
    ]
}

/// Decode a stored `[left, top, right, bottom]` rect back into a position and an
/// optional content size (the inverse of [`encode_rect`]).
///
/// A non-positive extent means the floater was content-driven when saved, so the
/// size is left unset and only the position is restored.
fn decode_rect(rect: [i32; 4]) -> (Vec2, Option<Vec2>) {
    let [left, top, right, bottom] = rect;
    let (left_px, top_px) = (px_to_f32(left), px_to_f32(top));
    let width = px_to_f32(right) - left_px;
    let height = px_to_f32(bottom) - top_px;
    let content_size = if width > 0.0 && height > 0.0 {
        Some(Vec2::new(width, height))
    } else {
        None
    };
    (Vec2::new(left_px, top_px), content_size)
}

/// Round a logical-pixel coordinate to the nearest whole `i32`, clamped to the
/// `i16` range so the conversion can neither truncate nor wrap.
const fn round_px(value: f32) -> i32 {
    let clamped = value.round().clamp(STORED_PX_MIN, STORED_PX_MAX);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "clamped to the i16 range just above — far inside i32, and integral after round()"
    )]
    let out = clamped as i32;
    out
}

/// Convert a stored pixel coordinate back to `f32`, clamped to the `i16` range so
/// the `i16`→`f32` widening is provably lossless.
fn px_to_f32(value: i32) -> f32 {
    let clamped = value.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
    f32::from(i16::try_from(clamped).unwrap_or(0))
}

/// **Register** each newly-spawned floater's four settings, with the spec
/// geometry as the declared default.
///
/// Runs the frame a floater is added — well before login — so the account file
/// loaded at login is coerced against these declarations. Idempotent per floater
/// (the `Added` filter fires once); a duplicate id is logged and swallowed by
/// [`ViewerSettings::register_in`].
fn register_floater_settings(
    settings: Option<ResMut<ViewerSettings>>,
    floaters: Query<(&Floater, &UiPanelShown), Added<Floater>>,
) {
    let Some(mut settings) = settings else {
        return;
    };
    for (floater, shown) in &floaters {
        let id = floater.id;
        let geometry = floater.geometry();
        settings.register_in(
            FLOATER_SECTION,
            &rect_key(id),
            SettingValue::Rect(encode_rect(geometry)),
            "Window rectangle (logical px [left, top, right, bottom]); a zero-width rect is a \
             content-sized window",
        );
        settings.register_in(
            FLOATER_SECTION,
            &visible_key(id),
            SettingValue::Bool(shown.0),
            "Whether the window is open",
        );
        settings.register_in(
            FLOATER_SECTION,
            &minimized_key(id),
            SettingValue::Bool(geometry.minimized),
            "Whether the window is collapsed to its title bar",
        );
        settings.register_in(
            FLOATER_SECTION,
            &docked_key(id),
            SettingValue::Bool(geometry.docked),
            "Whether the window is docked into its host",
        );
    }
}

/// **Seed** every not-yet-seeded floater from its stored geometry, once the
/// account scope is loaded.
///
/// Each aspect is applied only when the store actually holds a value for it
/// (`is_overridden`), so a floater with nothing saved keeps its `FloaterSpec`
/// default. Docking is requested through the manager's command path; the on-screen
/// clamp (a manager system) then rescues a rect saved on a larger display.
fn seed_floaters_from_settings(
    settings: Option<Res<ViewerSettings>>,
    mut floaters: Query<(Entity, &mut Floater, &mut UiPanelShown), Without<FloaterSeeded>>,
    mut commands: Commands,
    mut floater_commands: MessageWriter<FloaterCommand>,
) {
    let Some(settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    let store = settings.store();
    for (entity, mut floater, mut shown) in &mut floaters {
        let id = floater.id;
        let mut geometry = floater.geometry();
        if store.is_overridden(&rect_key(id))
            && let Ok(rect) = store.get_rect(&rect_key(id))
        {
            let (position, content_size) = decode_rect(rect);
            geometry.position = position;
            geometry.content_size = content_size;
        }
        if store.is_overridden(&minimized_key(id))
            && let Ok(minimized) = store.get_bool(&minimized_key(id))
        {
            geometry.minimized = minimized;
        }
        floater.restore_geometry(geometry);

        let mut want_dock = false;
        if store.is_overridden(&docked_key(id))
            && let Ok(docked) = store.get_bool(&docked_key(id))
        {
            want_dock = docked;
        }
        if store.is_overridden(&visible_key(id))
            && let Ok(visible) = store.get_bool(&visible_key(id))
        {
            shown.0 = visible;
        }
        if want_dock {
            // Toggling a fresh (free) floater docks it into the default host.
            floater_commands.write(FloaterCommand {
                floater: entity,
                op: FloaterOp::ToggleDock,
            });
        }
        commands.entity(entity).insert(FloaterSeeded);
    }
}

/// **Persist** a seeded floater's geometry into the store whenever it moves,
/// resizes, minimizes, docks, opens or closes, and mark the store dirty for the
/// next flush.
///
/// Gated on [`FloaterSeeded`] so the write-back reflects real user changes, not
/// the seed itself, and never runs before the stored values have been applied.
fn persist_floater_changes(
    settings: Option<ResMut<ViewerSettings>>,
    floaters: Query<(&Floater, &UiPanelShown), ChangedSeededFloater>,
    mut dirty: ResMut<FloaterPersistDirty>,
    time: Res<Time>,
) {
    let Some(mut settings) = settings else {
        return;
    };
    let mut any = false;
    for (floater, shown) in &floaters {
        let id = floater.id;
        let geometry = floater.geometry();
        settings.set_account(&rect_key(id), SettingValue::Rect(encode_rect(geometry)));
        settings.set_account(&visible_key(id), SettingValue::Bool(shown.0));
        settings.set_account(&minimized_key(id), SettingValue::Bool(geometry.minimized));
        settings.set_account(&docked_key(id), SettingValue::Bool(geometry.docked));
        any = true;
    }
    if any && dirty.since.is_none() {
        dirty.since = Some(time.elapsed_secs());
    }
}

/// **Flush** the store to disk once a dirty change has aged past
/// [`FLUSH_INTERVAL_SECS`], then clear the dirty clock.
fn flush_floater_settings(
    settings: Option<Res<ViewerSettings>>,
    mut dirty: ResMut<FloaterPersistDirty>,
    time: Res<Time>,
) {
    let Some(settings) = settings else {
        return;
    };
    let Some(since) = dirty.since else {
        return;
    };
    if time.elapsed_secs() - since >= FLUSH_INTERVAL_SECS {
        settings.save();
        dirty.since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_rect, encode_rect, px_to_f32, round_px};
    use crate::floater::FloaterGeometry;
    use bevy::prelude::Vec2;
    use pretty_assertions::assert_eq;

    /// A sized floater's rect round-trips: position and content size come back
    /// unchanged (to the pixel).
    #[test]
    fn a_sized_floater_rect_round_trips() {
        let geometry = FloaterGeometry {
            position: Vec2::new(20.0, 60.0),
            content_size: Some(Vec2::new(300.0, 400.0)),
            minimized: false,
            docked: false,
        };
        let rect = encode_rect(geometry);
        assert_eq!(rect, [20, 60, 320, 460], "right/bottom carry the size");
        let (position, content_size) = decode_rect(rect);
        assert_eq!(position, Vec2::new(20.0, 60.0));
        assert_eq!(content_size, Some(Vec2::new(300.0, 400.0)));
    }

    /// A content-driven floater (no explicit size) stores a zero-extent rect and
    /// decodes back to "size unset", preserving only the position.
    #[test]
    fn a_content_driven_floater_keeps_no_size() {
        let geometry = FloaterGeometry {
            position: Vec2::new(120.0, 80.0),
            content_size: None,
            minimized: false,
            docked: false,
        };
        let rect = encode_rect(geometry);
        assert_eq!(rect, [120, 80, 120, 80], "a zero-extent rect");
        let (position, content_size) = decode_rect(rect);
        assert_eq!(position, Vec2::new(120.0, 80.0));
        assert_eq!(content_size, None, "no size is restored");
    }

    /// A rect saved off the top-left (a negative offset from a larger monitor)
    /// still decodes to the same negative position — recovery on a smaller display
    /// is the manager's on-screen clamp's job, not the codec's.
    #[test]
    fn a_negative_offset_survives_the_codec() {
        let geometry = FloaterGeometry {
            position: Vec2::new(-40.0, -10.0),
            content_size: Some(Vec2::new(200.0, 150.0)),
            minimized: false,
            docked: false,
        };
        let (position, content_size) = decode_rect(encode_rect(geometry));
        assert_eq!(position, Vec2::new(-40.0, -10.0));
        assert_eq!(content_size, Some(Vec2::new(200.0, 150.0)));
    }

    /// A wild coordinate is clamped to the `i16` range rather than wrapping or
    /// truncating, on the way both in and out of the stored rect.
    #[expect(
        clippy::float_cmp,
        reason = "the clamp yields an exact i16 bound, asserted exactly"
    )]
    #[test]
    fn a_wild_coordinate_clamps() {
        assert_eq!(round_px(1.0e9), i32::from(i16::MAX), "clamped on encode");
        assert_eq!(round_px(-1.0e9), i32::from(i16::MIN));
        assert_eq!(
            px_to_f32(1_000_000),
            f32::from(i16::MAX),
            "clamped on decode"
        );
    }
}
