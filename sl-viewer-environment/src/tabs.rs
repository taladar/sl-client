//! **Which knob is on which tab**, for the whole crate.
//!
//! Two windows draw a sky frame's knobs — the fixed sky editor over an
//! inventory asset, and the day-cycle editor over the keyframe it has selected —
//! and a third draws a water frame's. They are the reference's *same* panels
//! (`panel_settings_sky_atmos.xml` and friends are shared verbatim between
//! `llfloaterfixedenvironment` and `llfloatereditextdaycycle`), so the pages are
//! one table here rather than a copy per window.
//!
//! The table is what makes "every knob is on exactly one tab" checkable. Both
//! ways of getting it wrong are silent: a knob on no tab is a value nobody can
//! edit, and one on two tabs is two controls fighting over a field with only the
//! last re-seed deciding which of them is right.

use crate::knobs::{AimKnobs, ColorKnob, SkyKnob, TextureKnob, WaterKnob};

/// One tab of an editor: its label and the knobs on it.
#[derive(Debug, Clone, Copy)]
pub struct TabPage {
    /// The tab label's Fluent key.
    pub label: &'static str,
    /// The tab's short name, for the element ids of the nodes on it.
    pub slug: &'static str,
    /// The colour swatches, in the first column.
    pub colors: &'static [ColorKnob],
    /// The texture swatches, under them.
    pub textures: &'static [TextureKnob],
    /// The sky sliders, split over the other two columns (empty on a water
    /// page).
    pub sky: &'static [SkyKnob],
    /// The water sliders (empty on a sky page).
    pub water: &'static [WaterKnob],
    /// The trackballs, one per slider column, above the sliders in it. Only
    /// the sun-and-moon page has any: a trackball is a second way to drive two
    /// knobs that are already on the page, so it belongs on the page they are
    /// on and nowhere else.
    pub aims: &'static [AimKnobs],
}

/// The sky pages, in the reference's order.
pub const SKY_TABS: &[TabPage] = &[
    TabPage {
        label: "settings-editor-tab-atmosphere",
        slug: "atmosphere",
        colors: &[
            ColorKnob::Ambient,
            ColorKnob::BlueHorizon,
            ColorKnob::BlueDensity,
        ],
        textures: &[],
        sky: &[
            SkyKnob::HazeHorizon,
            SkyKnob::HazeDensity,
            SkyKnob::MoistureLevel,
            SkyKnob::DropletRadius,
            SkyKnob::IceLevel,
            SkyKnob::DensityMultiplier,
            SkyKnob::DistanceMultiplier,
            SkyKnob::MaxAltitude,
            SkyKnob::ProbeAmbiance,
            SkyKnob::Gamma,
        ],
        water: &[],
        aims: &[],
    },
    TabPage {
        label: "settings-editor-tab-clouds",
        slug: "clouds",
        colors: &[ColorKnob::CloudColor],
        textures: &[TextureKnob::CloudImage],
        sky: &[
            SkyKnob::CloudCoverage,
            SkyKnob::CloudScale,
            SkyKnob::CloudVariance,
            SkyKnob::CloudScrollX,
            SkyKnob::CloudScrollY,
            SkyKnob::CloudDensityX,
            SkyKnob::CloudDensityY,
            SkyKnob::CloudDensityD,
            SkyKnob::CloudDetailX,
            SkyKnob::CloudDetailY,
            SkyKnob::CloudDetailD,
        ],
        water: &[],
        aims: &[],
    },
    TabPage {
        label: "settings-editor-tab-sun-moon",
        slug: "sun-moon",
        colors: &[ColorKnob::SunColor],
        textures: &[
            TextureKnob::SunImage,
            TextureKnob::MoonImage,
            TextureKnob::BloomImage,
            TextureKnob::HaloImage,
            TextureKnob::RainbowImage,
        ],
        sky: &[
            SkyKnob::SunAzimuth,
            SkyKnob::SunElevation,
            SkyKnob::SunScale,
            SkyKnob::GlowFocus,
            SkyKnob::GlowSize,
            SkyKnob::StarBrightness,
            SkyKnob::MoonAzimuth,
            SkyKnob::MoonElevation,
            SkyKnob::MoonScale,
            SkyKnob::MoonBrightness,
            SkyKnob::SunArcRadians,
        ],
        water: &[],
        // The reference's own sun-and-moon panel opens with the two
        // trackballs, one per body, above the angle spinners they share.
        aims: &[AimKnobs::SUN, AimKnobs::MOON],
    },
    TabPage {
        label: "settings-editor-tab-density",
        slug: "density",
        colors: &[],
        textures: &[],
        sky: &[
            SkyKnob::RayleighExpTerm,
            SkyKnob::RayleighExpScale,
            SkyKnob::RayleighLinear,
            SkyKnob::RayleighConstant,
            SkyKnob::RayleighWidth,
            SkyKnob::MieExpTerm,
            SkyKnob::MieExpScale,
            SkyKnob::MieLinear,
            SkyKnob::MieConstant,
            SkyKnob::MieAnisotropy,
            SkyKnob::MieWidth,
            SkyKnob::AbsorptionExpTerm,
            SkyKnob::AbsorptionExpScale,
            SkyKnob::AbsorptionLinear,
            SkyKnob::AbsorptionConstant,
            SkyKnob::AbsorptionWidth,
            // The atmosphere's geometry, which the same scattering model reads
            // and the reference's panel does not offer. Their ranges are its
            // validator's.
            SkyKnob::PlanetRadius,
            SkyKnob::SkyBottomRadius,
            SkyKnob::SkyTopRadius,
        ],
        water: &[],
        aims: &[],
    },
];

/// The water page. The reference's water panel is a single page too, and every
/// water knob fits on it.
pub const WATER_TABS: &[TabPage] = &[TabPage {
    label: "settings-editor-tab-water",
    slug: "water",
    colors: &[ColorKnob::WaterFogColor],
    textures: &[
        TextureKnob::WaterNormalMap,
        TextureKnob::WaterTransparentTexture,
    ],
    sky: &[],
    water: &[
        WaterKnob::FogDensity,
        WaterKnob::UnderwaterModifier,
        WaterKnob::FresnelScale,
        WaterKnob::FresnelOffset,
        WaterKnob::NormalScaleX,
        WaterKnob::NormalScaleY,
        WaterKnob::NormalScaleZ,
        WaterKnob::ScaleAbove,
        WaterKnob::ScaleBelow,
        WaterKnob::BlurMultiplier,
        WaterKnob::LargeWaveX,
        WaterKnob::LargeWaveY,
        WaterKnob::SmallWaveX,
        WaterKnob::SmallWaveY,
    ],
    aims: &[],
}];

#[cfg(test)]
mod tests {
    use super::{SKY_TABS, WATER_TABS};
    use crate::knobs::{AimKnobs, ColorKnob, SkyKnob, TextureKnob, WaterKnob};
    use pretty_assertions::assert_eq;

    /// **Every knob is on exactly one tab.** The knob tables and the tab tables
    /// are two lists that have to agree, and the failure is silent in both
    /// directions: a knob missing from every tab is a value nobody can edit, and
    /// one on two tabs is two controls writing the same field with only the last
    /// re-seed deciding which is right.
    #[test]
    fn every_knob_is_on_exactly_one_tab() {
        let sky: Vec<SkyKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.sky.iter().copied())
            .collect();
        assert_eq!(
            sky.len(),
            SkyKnob::ALL.len(),
            "a sky knob is missing or twice over"
        );
        for knob in SkyKnob::ALL {
            assert_eq!(
                sky.iter().filter(|shown| *shown == knob).count(),
                1,
                "{knob:?} is not on exactly one tab"
            );
        }

        let water: Vec<WaterKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.water.iter().copied())
            .collect();
        assert_eq!(water.len(), WaterKnob::ALL.len());
        for knob in WaterKnob::ALL {
            assert_eq!(water.iter().filter(|shown| *shown == knob).count(), 1);
        }

        let colors: Vec<ColorKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.colors.iter().copied())
            .collect();
        assert_eq!(colors.len(), ColorKnob::ALL.len());
        for knob in ColorKnob::ALL {
            assert_eq!(colors.iter().filter(|shown| *shown == knob).count(), 1);
        }

        let textures: Vec<TextureKnob> = SKY_TABS
            .iter()
            .chain(WATER_TABS)
            .flat_map(|page| page.textures.iter().copied())
            .collect();
        assert_eq!(textures.len(), TextureKnob::ALL.len());
        for knob in TextureKnob::ALL {
            assert_eq!(textures.iter().filter(|shown| *shown == knob).count(), 1);
        }
    }

    /// **A trackball is on the page its own two knobs are on**, once, and every
    /// body has one.
    ///
    /// A trackball writes two knobs that already have sliders. Put it on a page
    /// those sliders are not on and it silently drives controls the user cannot
    /// see beside it; put it on two pages and two controls fight over one body.
    /// Neither is visible by reading one table.
    #[test]
    fn every_trackball_is_on_the_page_its_knobs_are() {
        let pages: Vec<&super::TabPage> = SKY_TABS.iter().chain(WATER_TABS).collect();
        for pair in AimKnobs::ALL {
            let shown: Vec<&super::TabPage> = pages
                .iter()
                .copied()
                .filter(|page| page.aims.contains(pair))
                .collect();
            assert_eq!(shown.len(), 1, "{:?} is not on exactly one page", pair.body);
            for page in shown {
                assert!(
                    page.sky.contains(&pair.azimuth) && page.sky.contains(&pair.elevation),
                    "{} holds a {:?} trackball but not its two sliders",
                    page.label,
                    pair.body
                );
            }
        }
    }

    /// **A page shows one kind of slider.** The sky pages hold sky knobs and the
    /// water page water ones; a knob on the wrong page would spawn a control
    /// whose write-back looks in a frame the window's session never holds, and
    /// do nothing at all.
    #[test]
    fn a_page_holds_only_its_own_kind_of_knob() {
        for page in SKY_TABS {
            assert!(page.water.is_empty(), "{} shows water knobs", page.label);
        }
        for page in WATER_TABS {
            assert!(page.sky.is_empty(), "{} shows sky knobs", page.label);
        }
    }
}
