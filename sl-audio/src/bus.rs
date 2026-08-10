//! Audio buses: the fixed set of volume categories every source plays on, and
//! the [`BusLevel`] gain/mute state each carries.
//!
//! The mixer graph is one [`crate::mixer::Mixer`] with a [`Bus::Master`] volume
//! node feeding the device and one volume node per category feeding the master.
//! Every producer picks a [`Bus`] and never touches the device — the whole
//! point of the shared mixer (see the `viewer-audio-backend` roadmap task).
//!
//! These categories are exactly the ones the volume panel
//! (`viewer-volume-panel`) exposes, so that panel reads and writes [`BusLevel`]
//! rather than inventing a parallel notion of volume.

/// A named volume category in the mixer.
///
/// [`Bus::Master`] is the final gain before the output device; every other
/// variant is a category that feeds the master. The ordering follows the
/// volume panel: sound effects, ambient, UI, music, media, voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bus {
    /// The final gain applied to the whole mix before the output device. Muting
    /// this silences everything; every other bus feeds into it.
    Master,
    /// In-world spatial sound effects: `llTriggerSound`, looped and attached
    /// object sounds, collision sounds (`viewer-in-world-sounds`).
    Sfx,
    /// Ambient / environment sound such as wind (`viewer-ambient-wind-sound`).
    Ambient,
    /// The viewer's own 2-D feedback sounds: clicks, alerts, IM chimes
    /// (`viewer-ui-sound-effects`).
    Ui,
    /// The parcel radio / streaming-music stream (`viewer-streaming-audio`).
    Music,
    /// Media-on-a-prim and page audio: video and browser PCM
    /// (`viewer-video-playback`, `viewer-media-prim-browser`).
    Media,
    /// Voice chat (`viewer-voice-audio`).
    Voice,
}

impl Bus {
    /// Every bus, master first, in a fixed order suitable for building the
    /// graph and iterating the panel.
    pub const ALL: [Self; 7] = [
        Self::Master,
        Self::Sfx,
        Self::Ambient,
        Self::Ui,
        Self::Music,
        Self::Media,
        Self::Voice,
    ];

    /// The category buses (everything except [`Bus::Master`]), in panel order.
    pub const CATEGORIES: [Self; 6] = [
        Self::Sfx,
        Self::Ambient,
        Self::Ui,
        Self::Music,
        Self::Media,
        Self::Voice,
    ];

    /// A short stable identifier for the bus, used for settings keys and logs.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Sfx => "sfx",
            Self::Ambient => "ambient",
            Self::Ui => "ui",
            Self::Music => "music",
            Self::Media => "media",
            Self::Voice => "voice",
        }
    }
}

/// The gain and mute state of a single [`Bus`].
///
/// `gain` is a linear multiplier in `[0.0, 1.0]` (unity is `1.0`), matching a
/// percent slider divided by 100. **Mute is not "gain 0":** muting retains the
/// previous gain so unmuting restores it, and — crucially — muting must not
/// stop the sources feeding the bus, because SL's looped and attached sounds
/// have to stay time-coherent (they keep playing silently). The mixer honours
/// that by driving the *bus node's* gain to zero while leaving every source
/// running.
#[expect(
    clippy::module_name_repetitions,
    reason = "BusLevel is the public paired type of Bus, re-exported at the crate root"
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BusLevel {
    /// The retained linear gain in `[0.0, 1.0]`. This is the value a slider
    /// shows and the value restored on unmute; it is unaffected by muting.
    gain: f32,
    /// Whether the bus is muted. While muted the effective gain is zero but
    /// `gain` above is preserved.
    muted: bool,
}

impl Default for BusLevel {
    /// Unity gain, not muted.
    fn default() -> Self {
        Self {
            gain: 1.0,
            muted: false,
        }
    }
}

impl BusLevel {
    /// Construct a level from a linear gain (clamped to `[0.0, 1.0]`), not
    /// muted.
    #[must_use]
    pub const fn from_linear(gain: f32) -> Self {
        Self {
            gain: gain.clamp(0.0, 1.0),
            muted: false,
        }
    }

    /// Construct a level from a percent value in `[0.0, 100.0]`, not muted.
    #[must_use]
    pub fn from_percent(percent: f32) -> Self {
        Self::from_linear(percent / 100.0)
    }

    /// The retained linear gain in `[0.0, 1.0]`, independent of mute state.
    ///
    /// This is what a slider binds to; use [`BusLevel::effective_gain`] for the
    /// value actually applied to the audio.
    #[must_use]
    pub const fn gain(self) -> f32 {
        self.gain
    }

    /// Whether the bus is currently muted.
    #[must_use]
    pub const fn is_muted(self) -> bool {
        self.muted
    }

    /// The linear gain actually applied to the signal: zero while muted, the
    /// retained [`BusLevel::gain`] otherwise.
    #[must_use]
    pub const fn effective_gain(self) -> f32 {
        if self.muted { 0.0 } else { self.gain }
    }

    /// Set the retained linear gain (clamped to `[0.0, 1.0]`). Does not change
    /// the mute state.
    pub const fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(0.0, 1.0);
    }

    /// Set the mute state. Muting retains the current [`BusLevel::gain`] so
    /// unmuting restores it.
    pub const fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Toggle the mute state, returning the new value.
    pub const fn toggle_muted(&mut self) -> bool {
        self.muted = !self.muted;
        self.muted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn categories_exclude_master() {
        assert!(!Bus::CATEGORIES.contains(&Bus::Master));
        assert_eq!(Bus::CATEGORIES.len(), Bus::ALL.len() - 1);
    }

    #[test]
    fn all_keys_unique() {
        let mut keys: Vec<&str> = Bus::ALL.iter().map(|b| b.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Bus::ALL.len());
    }

    #[test]
    fn default_is_unity_unmuted() {
        let level = BusLevel::default();
        assert!((level.gain() - 1.0).abs() < 1e-6);
        assert!(!level.is_muted());
        assert!((level.effective_gain() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mute_retains_and_restores_level() {
        let mut level = BusLevel::from_percent(40.0);
        assert!((level.gain() - 0.4).abs() < 1e-6);

        level.set_muted(true);
        // Effective gain drops to zero...
        assert!(level.effective_gain().abs() < 1e-6);
        // ...but the retained gain is untouched.
        assert!((level.gain() - 0.4).abs() < 1e-6);

        level.set_muted(false);
        assert!((level.effective_gain() - 0.4).abs() < 1e-6);
    }

    #[test]
    fn set_gain_while_muted_keeps_silence_but_updates_retained() {
        let mut level = BusLevel::from_linear(0.5);
        level.set_muted(true);
        level.set_gain(0.8);
        assert!(level.effective_gain().abs() < 1e-6);
        assert!((level.gain() - 0.8).abs() < 1e-6);
        assert!(!level.toggle_muted());
        assert!((level.effective_gain() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn gain_is_clamped() {
        assert!((BusLevel::from_linear(2.0).gain() - 1.0).abs() < 1e-6);
        assert!(BusLevel::from_linear(-1.0).gain().abs() < 1e-6);
        let mut level = BusLevel::default();
        level.set_gain(5.0);
        assert!((level.gain() - 1.0).abs() < 1e-6);
    }
}
