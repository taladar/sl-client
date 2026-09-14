//! The **environment-pin registry**: which `SL_VIEWER_*` debug knobs currently
//! beat a registered, GUI-editable setting.
//!
//! The viewer carries a large family of `SL_VIEWER_*` environment knobs — A/B
//! levers for rendering work, seeds for the offline screenshot harness, logging
//! gates. A handful of them do not merely *log* something: they **pin** a value
//! that a preferences control is also bound to, and they win. The control then
//! still moves under the pointer and the store still records the edit, but the
//! frame never changes, because the per-frame apply pass reads the environment
//! value instead. Two authorities for one user-facing value, with no indication
//! which one is in charge.
//!
//! The project rule is that GUI options live in preferences, the CLI is for
//! non-GUI/start-up concerns, and environment variables are for source-level
//! debugging only. These knobs stay — they are the levers that isolate a
//! rendering defect, and a lever you have to rebuild to pull is no lever — but
//! they stop being *silent*:
//!
//! - every pin is announced at start-up with a `warn!` naming the variable, its
//!   value, and the setting it beats;
//! - the preferences control bound to a pinned setting is **disabled** and says
//!   which variable took it over, so the dead checkbox is visibly dead.
//!
//! A pin is recorded once, at start-up, by the module that owns the knob
//! (the render overrides, the shadow preferences, the skin and locale seeds)
//! into the [`EnvPinnedSettings`] resource. Nothing re-reads the environment
//! afterwards: the resource is the single answer to "is this setting under
//! external control", so a test states the situation by inserting the resource
//! rather than by mutating the process environment.

use std::collections::HashMap;

use bevy::prelude::*;
use tracing::{info, warn};

/// The prefix every viewer environment knob shares.
const ENV_PREFIX: &str = "SL_VIEWER_";

/// How long an environment pin holds its setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinKind {
    /// The knob is re-applied every frame, so the bound preference is dead for
    /// the whole session: an edit is stored and never takes effect.
    Live,
    /// The knob only **seeds** the start-up value. The bound preference still
    /// works — it simply is not what the viewer started with, so the control
    /// and the running viewer disagree until the control is touched.
    Seed,
}

impl PinKind {
    /// Whether a control bound to a setting pinned this way should be disabled:
    /// only a [`Live`](PinKind::Live) pin makes the control inert.
    #[must_use]
    pub const fn disables_control(self) -> bool {
        matches!(self, Self::Live)
    }
}

/// One environment variable holding one registered setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvPin {
    /// The name of the pinned setting in the store (e.g. `RenderTonemapType`).
    setting: String,
    /// The environment variable that pinned it (e.g. `SL_VIEWER_TONEMAP`).
    env: &'static str,
    /// The variable's value as the process received it. Empty when the knob is
    /// a bare presence check (`SL_VIEWER_DISABLE_GLOW=`).
    value: String,
    /// How long the pin holds.
    kind: PinKind,
}

impl EnvPin {
    /// The name of the pinned setting.
    #[must_use]
    pub fn setting(&self) -> &str {
        &self.setting
    }

    /// The environment variable that pinned it.
    #[must_use]
    pub const fn env(&self) -> &'static str {
        self.env
    }

    /// The variable's value, as the process received it.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// How long the pin holds.
    #[must_use]
    pub const fn kind(&self) -> PinKind {
        self.kind
    }
}

/// Every registered setting currently held by an environment knob, keyed by
/// setting name.
///
/// Absent or empty (the normal run) nothing is pinned and every preferences
/// control behaves exactly as before.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvPinnedSettings {
    /// Setting name → the knob holding it. One knob per setting: a setting two
    /// variables could pin would be a bug in the recording side, and the last
    /// recorded wins so the report at least names a real winner.
    pins: HashMap<String, EnvPin>,
}

impl EnvPinnedSettings {
    /// Record `setting` as pinned by `env`, whose value is `value`.
    pub fn pin(
        &mut self,
        setting: impl Into<String>,
        env: &'static str,
        value: impl Into<String>,
        kind: PinKind,
    ) {
        let setting = setting.into();
        self.pins.insert(
            setting.clone(),
            EnvPin {
                setting,
                env,
                value: value.into(),
                kind,
            },
        );
    }

    /// Record `setting` as pinned by `env` **if** `env` is set in the process
    /// environment at all; a no-op otherwise.
    ///
    /// Reading the environment is the caller's start-up concern: call this from
    /// the same start-up pass that builds the knob's own value, so the registry
    /// and the knob can never disagree about whether it was set.
    pub fn pin_if_set(&mut self, setting: &str, env: &'static str, kind: PinKind) {
        if let Ok(value) = std::env::var(env) {
            self.pin(setting, env, value, kind);
        } else if std::env::var_os(env).is_some() {
            // Set, but not UTF-8. It still pins; the value is not printable.
            self.pin(setting, env, String::new(), kind);
        }
    }

    /// The knob holding `setting`, if any.
    #[must_use]
    pub fn get(&self, setting: &str) -> Option<&EnvPin> {
        self.pins.get(setting)
    }

    /// Whether `setting` is held by a knob that makes its control inert.
    #[must_use]
    pub fn disables_control(&self, setting: &str) -> bool {
        self.get(setting)
            .is_some_and(|pin| pin.kind().disables_control())
    }

    /// Every recorded pin, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &EnvPin> {
        self.pins.values()
    }

    /// How many settings are pinned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pins.len()
    }

    /// Whether nothing is pinned (the normal run).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    /// Announce every pin: one `warn!` per setting a knob holds, naming the
    /// variable, its value and what it beat.
    ///
    /// A `warn!` rather than an `info!` on purpose — a knob that quietly
    /// disables a preference is exactly the thing a bug report needs to carry,
    /// and a run started from a stale shell should say so on its own.
    pub fn announce(&self) {
        // Sorted, so two runs of the same environment log the same lines.
        let mut pins: Vec<&EnvPin> = self.iter().collect();
        pins.sort_by(|left, right| left.setting.cmp(&right.setting));
        for pin in pins {
            match pin.kind {
                PinKind::Live => warn!(
                    env = pin.env,
                    value = pin.value.as_str(),
                    setting = pin.setting.as_str(),
                    "environment knob overrides a preference: the preferences \
                     control for this setting is disabled for this session",
                ),
                PinKind::Seed => warn!(
                    env = pin.env,
                    value = pin.value.as_str(),
                    setting = pin.setting.as_str(),
                    "environment knob seeded a preference: the viewer started \
                     with the environment value, not the stored one",
                ),
            }
        }
    }
}

/// Log every `SL_VIEWER_*` variable the process was started with.
///
/// The knobs are a debugging surface, not a documented one: most have no CLI
/// flag and no preference, so `--help` cannot list them and nothing in the UI
/// hints they exist. A single line per active knob at start-up makes a run
/// self-describing — the log that accompanies a bug report says which levers
/// were pulled, whether or not any of them pinned a setting.
///
/// Values are logged as received. These are debug knobs (booleans, counts,
/// paths and log gates); none carries a credential.
pub fn log_active_env_knobs() {
    let mut active: Vec<(String, String)> = std::env::vars()
        .filter(|(name, _)| name.starts_with(ENV_PREFIX))
        .collect();
    if active.is_empty() {
        return;
    }
    active.sort();
    info!(
        count = active.len(),
        "{ENV_PREFIX}* debug knobs are set for this run",
    );
    for (name, value) in active {
        info!(
            env = name.as_str(),
            value = value.as_str(),
            "debug knob set"
        );
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{EnvPin, EnvPinnedSettings, PinKind};

    /// The empty registry — the normal run — holds nothing and disables
    /// nothing.
    #[test]
    fn nothing_is_pinned_by_default() {
        let pins = EnvPinnedSettings::default();
        assert!(pins.is_empty());
        assert_eq!(pins.len(), 0);
        assert!(pins.get("RenderGlow").is_none());
        assert!(!pins.disables_control("RenderGlow"));
    }

    /// A live pin names its variable and value, and makes the bound control
    /// inert.
    #[test]
    fn a_live_pin_disables_its_control() {
        let mut pins = EnvPinnedSettings::default();
        pins.pin(
            "RenderTonemapType",
            "SL_VIEWER_TONEMAP",
            "none",
            PinKind::Live,
        );
        let pin = pins.get("RenderTonemapType");
        assert_eq!(pin.map(EnvPin::env), Some("SL_VIEWER_TONEMAP"));
        assert_eq!(pin.map(EnvPin::value), Some("none"));
        assert_eq!(pin.map(EnvPin::kind), Some(PinKind::Live));
        assert!(pins.disables_control("RenderTonemapType"));
    }

    /// A seed pin is reported, but the control still works — it only says the
    /// viewer started somewhere else.
    #[test]
    fn a_seed_pin_leaves_its_control_alive() {
        let mut pins = EnvPinnedSettings::default();
        pins.pin("UiSkin", "SL_VIEWER_SKIN", "azure", PinKind::Seed);
        assert_eq!(pins.len(), 1);
        assert!(pins.get("UiSkin").is_some());
        assert!(!pins.disables_control("UiSkin"));
    }

    /// Recording the same setting twice keeps one entry, so a report never
    /// claims two winners for one value.
    #[test]
    fn one_pin_per_setting() {
        let mut pins = EnvPinnedSettings::default();
        pins.pin("RenderGlow", "SL_VIEWER_DISABLE_GLOW", "", PinKind::Live);
        pins.pin("RenderGlow", "SL_VIEWER_DISABLE_GLOW", "1", PinKind::Live);
        assert_eq!(pins.len(), 1);
        assert_eq!(
            pins.get("RenderGlow").map(|pin| pin.value()),
            Some("1"),
            "the last recording wins",
        );
    }

    /// `announce` walks every pin without panicking on either kind (it is the
    /// one place a pin's fields are formatted).
    #[test]
    fn announcing_covers_both_kinds() {
        let mut pins = EnvPinnedSettings::default();
        pins.pin("RenderGlow", "SL_VIEWER_DISABLE_GLOW", "", PinKind::Live);
        pins.pin("UiLanguage", "SL_VIEWER_UI_LOCALE", "ja", PinKind::Seed);
        pins.announce();
        assert_eq!(pins.len(), 2);
    }
}
