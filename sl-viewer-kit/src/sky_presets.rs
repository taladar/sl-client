//! [`FixedSky`]: the four fixed times of day the **World ▸ Environment** menu
//! offers, over Linden's four canonical WindLight sky presets.
//!
//! The presets themselves — `A-6AM` / `A-12PM` / `A-6PM` / `A-12AM`, the legacy
//! → EEP conversion, and the day cycle that schedules all four — live in
//! `sl_proto::sky_presets` and are re-exported here, because a *grid* needs
//! them too: a region that wants to serve a day cycle with a day in it serves
//! these frames, and a comparison between two viewers rests on both of them
//! reading the same schedule off the wire.
//!
//! What stays here is the menu's own vocabulary: which preset each entry names,
//! where it sits in the day, and which modern EEP asset the reference loads for
//! it (`LLEnvironment::setEnvironment(ENV_LOCAL, …)` on the same four).

pub use sl_client_bevy::{
    MIDDAY, MIDNIGHT, PRESET_DAY_CYCLE_NAME, PRESET_DAY_KEYFRAMES, SUNRISE, SUNSET, SkyPreset,
    install_preset_day_cycle, preset_day_cycle, preset_sky_schedule, sky_settings_from,
};
use sl_client_bevy::{SkySettings, Uuid};

/// One of the four fixed times of day the World ▸ Environment menu offers —
/// the reference viewer's Sunrise / Midday / Sunset / Midnight presets.
///
/// Serialisable because the menu's pin is part of the personal environment the
/// account keeps across a relog (`EnvironmentPersistAcrossLogin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FixedSky {
    /// Linden's `A-6AM` (Sunrise).
    Sunrise,
    /// Linden's `A-12PM` (Midday).
    Midday,
    /// Linden's `A-6PM` (Sunset).
    Sunset,
    /// Linden's `A-12AM` (Midnight).
    Midnight,
}

impl FixedSky {
    /// The ported preset behind this fixed sky.
    const fn preset(self) -> &'static SkyPreset {
        match self {
            Self::Sunrise => &SUNRISE,
            Self::Midday => &MIDDAY,
            Self::Sunset => &SUNSET,
            Self::Midnight => &MIDNIGHT,
        }
    }

    /// This fixed sky as a renderable frame.
    #[must_use]
    pub fn settings(self) -> SkySettings {
        sky_settings_from(self.preset())
    }

    /// The frame name the fixed day cycle files this sky under.
    #[must_use]
    pub const fn frame_name(self) -> &'static str {
        self.preset().label
    }

    /// The normalised day-cycle position (`0.0..=1.0`) at which this time freezes
    /// the region's *own* EEP day cycle — the reference's canonical placement
    /// (`0.25` sunrise, `0.5` midday, `0.75` sunset, `0.0` midnight). Backs the
    /// World ▸ Environment **Day Cycle** presets.
    #[must_use]
    pub const fn day_position(self) -> f32 {
        match self {
            Self::Sunrise => 0.25,
            Self::Midday => 0.5,
            Self::Sunset => 0.75,
            Self::Midnight => 0.0,
        }
    }

    /// The reference viewer's `KNOWN_SKY_*` library sky asset for this time — the
    /// exact modern EEP sky Firestorm's World ▸ Environment preset loads
    /// (`LLEnvironment::KNOWN_SKY_{SUNRISE,MIDDAY,SUNSET,MIDNIGHT}`). Backs the
    /// **Modern** presets: fetching and rendering this asset gives byte-identical
    /// input to Firestorm, so a renderer comparison isolates the renderer.
    #[must_use]
    pub const fn modern_asset(self) -> Uuid {
        match self {
            Self::Sunrise => Uuid::from_u128(0x01e4_1537_ff51_2f1f_8ef7_17e4_df76_0bfb),
            Self::Midday => Uuid::from_u128(0xc462_26b4_0e43_5a56_9708_d27c_a1df_3292),
            Self::Sunset => Uuid::from_u128(0x084e_26cd_a900_28e8_08d0_64a9_de5c_15e2),
            Self::Midnight => Uuid::from_u128(0x8a01_b97a_cb20_c1ea_ac63_f7ea_84ad_0090),
        }
    }
}
