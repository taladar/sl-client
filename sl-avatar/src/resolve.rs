//! Driver → driven propagation and avatar-sex resolution (P13.4).
//!
//! An `AvatarAppearance.visual_params` vector only carries the *transmitted*
//! params ([`VisualParams::transmitted`]). Two effects turn that partial vector
//! into the full set of weights the shape actually needs:
//!
//! - **Driver → driven propagation.** A `<param_driver>` param
//!   ([`ParamEffect::Driver`]) drives a list of other params over sub-ranges of
//!   its own weight (Firestorm `LLDriverParam::getDrivenWeight`). Every driven
//!   param in the standard `avatar_lad.xml` is *non-transmitted* (group 1 / 2),
//!   so a receiver never sees its value on the wire and must derive it from the
//!   (transmitted) driver. The classic example is the transmitted `male` driver,
//!   which drives the non-transmitted `Male_Skeleton` / `Male_Head` / … params
//!   that give a male avatar its proportions. A driven param that *is* somehow
//!   transmitted keeps its wire value (the sender already resolved it), so this
//!   pass only fills in non-transmitted driven params.
//! - **Sex.** The avatar's sex is decided by the transmitted [`male`](Self)
//!   param's weight (Firestorm: `getVisualParamWeight("male") > 0.5`). A param
//!   tagged `sex="male"` / `"female"` only takes effect on a matching avatar;
//!   otherwise it falls back to its default weight
//!   ([`ResolvedParams::effective_weight`], mirroring the `getSex() & avatar_sex`
//!   gate in `LLPolyMorphTarget::apply` / `LLPolySkeletalDistortion::apply`).
//!
//! [`ResolvedParams`] performs both, producing the param-id → weight lookup that
//! [`MorphWeights`](crate::MorphWeights) and
//! [`SkeletalDeformations`](crate::SkeletalDeformations) consume. Like the rest
//! of the crate it is I/O-free and Bevy-free.

use std::collections::HashMap;

use crate::params::{AppearanceValues, ParamEffect, ParamSex, VisualParam, VisualParams};

/// The name of the visual param whose weight decides the avatar's sex; the
/// reference viewer keys off `getVisualParamWeight("male")`.
const MALE_PARAM_NAME: &str = "male";

/// The `male`-param weight above which the avatar is treated as male
/// (Firestorm's `> 0.5f` test).
const MALE_THRESHOLD: f32 = 0.5;

/// The full set of resolved visual-param weights for one avatar: every param's
/// weight after filling in non-transmitted driven params from their drivers,
/// plus the avatar's resolved sex.
///
/// Built once per avatar from a [`VisualParams`] table and an appearance vector,
/// then queried by the morph and skeletal resolvers. A param's *raw* resolved
/// weight is available via [`weight`](Self::weight); its *effective* weight after
/// sex gating via [`effective_weight`](Self::effective_weight).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedParams {
    /// Resolved weight for every param in the table, keyed by param id.
    by_id: HashMap<i32, f32>,
    /// The avatar's resolved sex (never [`ParamSex::Both`]).
    avatar_sex: ParamSex,
}

impl ResolvedParams {
    /// Resolve the weights from a visual-param table and a raw wire
    /// `AvatarAppearance.visual_params` byte vector.
    #[must_use]
    pub fn from_appearance(params: &VisualParams, visual_params: &[u8]) -> Self {
        Self::from_values(params, &params.map_appearance(visual_params))
    }

    /// Resolve the weights from a visual-param table and already-mapped
    /// [`AppearanceValues`].
    ///
    /// Every param starts at its transmitted (dequantized) weight or, where the
    /// vector did not carry it, its default. Each driver then overwrites its
    /// non-transmitted driven params with the trapezoid-ramped
    /// `driven_weight`. Drivers are applied in ascending id order (the order
    /// [`VisualParams::all`] yields), so a driver driven by an earlier driver
    /// sees the updated value.
    #[must_use]
    pub fn from_values(params: &VisualParams, appearance: &AppearanceValues) -> Self {
        // Base weights: transmitted params from the wire, everything else at its
        // default.
        let mut by_id = HashMap::with_capacity(params.len());
        for param in params.all() {
            let weight = appearance.weight(param.id).unwrap_or(param.default);
            by_id.insert(param.id, weight);
        }

        // Driver → driven: fill in the non-transmitted driven params.
        for driver in params.all() {
            let ParamEffect::Driver(entries) = &driver.effect else {
                continue;
            };
            let input = by_id.get(&driver.id).copied().unwrap_or(driver.default);
            for entry in entries {
                let Some(driven) = params.get(entry.id) else {
                    continue;
                };
                // A transmitted driven param already carries the sender's
                // resolved value on the wire; only derive the rest.
                if driven.is_transmitted() {
                    continue;
                }
                by_id.insert(entry.id, driven_weight(driver, driven, entry, input));
            }
        }

        let avatar_sex = resolve_sex(params, &by_id);
        Self { by_id, avatar_sex }
    }

    /// The raw resolved weight for a given param id, if the param is in the
    /// table.
    #[must_use]
    pub fn weight(&self, id: i32) -> Option<f32> {
        self.by_id.get(&id).copied()
    }

    /// The avatar's resolved sex ([`ParamSex::Male`] or [`ParamSex::Female`]).
    #[must_use]
    pub const fn avatar_sex(&self) -> ParamSex {
        self.avatar_sex
    }

    /// The param's *effective* weight after sex gating: its resolved weight when
    /// the param applies to this avatar's sex (or to both), else its default —
    /// the `( getSex() & avatar_sex ) ? weight : default` gate the reference
    /// viewer applies before every morph / skeletal distortion.
    #[must_use]
    pub fn effective_weight(&self, param: &VisualParam) -> f32 {
        let raw = self.by_id.get(&param.id).copied().unwrap_or(param.default);
        match param.sex {
            ParamSex::Both => raw,
            other if other == self.avatar_sex => raw,
            _ => param.default,
        }
    }
}

/// Decide the avatar's sex from the resolved weight of the `male` param (default
/// [`ParamSex::Female`] when the table has no such param).
fn resolve_sex(params: &VisualParams, by_id: &HashMap<i32, f32>) -> ParamSex {
    for param in params.all() {
        if param.name.eq_ignore_ascii_case(MALE_PARAM_NAME) {
            let weight = by_id.get(&param.id).copied().unwrap_or(param.default);
            return if weight > MALE_THRESHOLD {
                ParamSex::Male
            } else {
                ParamSex::Female
            };
        }
    }
    ParamSex::Female
}

/// The driven param's weight for a given driver `input`, replicating Firestorm's
/// `LLDriverParam::getDrivenWeight`: a trapezoid ramp from the driven param's own
/// `[min, max]` over the driver sub-ranges `min1 < max1 <= max2 < min2`.
///
/// Below `min1` and above `min2` the driven param sits at its min (or max at the
/// extremes where the driver bottoms/tops out and the plateau is degenerate);
/// it ramps up over `[min1, max1]`, holds at max over `[max1, max2]`, and ramps
/// back down over `[max2, min2]`.
fn driven_weight(
    driver: &VisualParam,
    driven: &VisualParam,
    entry: &crate::params::DrivenParam,
    input: f32,
) -> f32 {
    let driven_min = driven.min;
    let driven_max = driven.max;
    let crate::params::DrivenParam {
        min1,
        max1,
        max2,
        min2,
        ..
    } = *entry;

    if input <= min1 {
        if near(min1, max1) && min1 <= driver.min {
            driven_max
        } else {
            driven_min
        }
    } else if input <= max1 {
        lerp(driven_min, driven_max, ramp(input, min1, max1))
    } else if input <= max2 {
        driven_max
    } else if input <= min2 {
        lerp(driven_max, driven_min, ramp(input, max2, min2))
    } else if max2 >= driver.max {
        driven_max
    } else {
        driven_min
    }
}

/// The fraction of the way `value` sits from `start` to `end`, guarding a
/// degenerate (zero-width) span so no division by zero occurs.
fn ramp(value: f32, start: f32, end: f32) -> f32 {
    let span = end - start;
    if span.abs() > f32::EPSILON {
        (value - start) / span
    } else {
        0.0
    }
}

/// Linear interpolation from `a` to `b` by `t`.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Whether two weights are equal within one float epsilon (the reference viewer
/// tests exact equality of the driver thresholds; the epsilon guards rounding).
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::ResolvedParams;
    use crate::params::{ParamSex, VisualParams};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A visual-param table exercising driver propagation and sex: a transmitted
    /// `male` driver (id 80) driving a non-transmitted `Male_Head` morph (id 40)
    /// and a transmitted-but-driven `Shared` morph (id 5) that must keep its wire
    /// value; a female-only morph (id 6); and a plain transmitted morph (id 1).
    /// Wire (transmitted, ascending id) order is [1, 5, 6, 80].
    const DRIVER_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="headMesh" lod="0" file_name="avatar_head.llm">
    <param id="1" group="0" name="Plain" value_min="0" value_max="1" value_default="0">
      <param_morph/>
    </param>
    <param id="5" group="0" name="Shared" value_min="0" value_max="1" value_default="0">
      <param_morph/>
    </param>
    <param id="6" group="0" name="FemOnly" sex="female" value_min="0" value_max="1" value_default="0">
      <param_morph/>
    </param>
    <param id="40" group="1" name="Male_Head" value_min="0" value_max="1" value_default="0">
      <param_morph/>
    </param>
  </mesh>
  <driver_parameters>
    <param id="80" group="0" name="male" value_min="0" value_max="1" value_default="0">
      <param_driver>
        <driven id="40"/>
        <driven id="5"/>
      </param_driver>
    </param>
  </driver_parameters>
</linden_avatar>"#;

    /// Compare two floats within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn male_driver_fills_non_transmitted_driven_param() -> Result<(), TestError> {
        let params = VisualParams::from_xml(DRIVER_LAD)?;
        // Wire order [1, 5, 6, 80]; drive `male` (id 80) to full.
        let resolved = ResolvedParams::from_appearance(&params, &[0, 0, 0, 255]);
        assert_eq!(resolved.avatar_sex(), ParamSex::Male);
        // Male_Head (id 40) is non-transmitted, driven by `male` -> its max (1.0).
        assert!(resolved.weight(40).is_some_and(|w| approx(w, 1.0)));
        Ok(())
    }

    #[test]
    fn transmitted_driven_param_keeps_its_wire_value() -> Result<(), TestError> {
        let params = VisualParams::from_xml(DRIVER_LAD)?;
        // Shared (id 5) is transmitted (slot 1) *and* driven by `male`. Its wire
        // byte 255 -> 1.0 must survive; the driver does not overwrite it even
        // though `male` is also full.
        let resolved = ResolvedParams::from_appearance(&params, &[0, 255, 0, 255]);
        assert!(resolved.weight(5).is_some_and(|w| approx(w, 1.0)));
        Ok(())
    }

    #[test]
    fn sex_gates_the_effective_weight() -> Result<(), TestError> {
        let params = VisualParams::from_xml(DRIVER_LAD)?;
        // Female avatar (male = 0): FemOnly (id 6) transmitted to full applies.
        let female = ResolvedParams::from_appearance(&params, &[0, 0, 255, 0]);
        assert_eq!(female.avatar_sex(), ParamSex::Female);
        let fem_only = params.get(6).ok_or("param 6")?;
        assert!(approx(female.effective_weight(fem_only), 1.0));

        // Male avatar (male = full): the same female-only param is gated back to
        // its default (0) even though its wire weight is full.
        let male = ResolvedParams::from_appearance(&params, &[0, 0, 255, 255]);
        assert_eq!(male.avatar_sex(), ParamSex::Male);
        assert!(approx(male.effective_weight(fem_only), 0.0));

        // A sexless param is never gated.
        let plain = params.get(1).ok_or("param 1")?;
        let resolved = ResolvedParams::from_appearance(&params, &[255, 0, 0, 255]);
        assert!(approx(resolved.effective_weight(plain), 1.0));
        Ok(())
    }

    #[test]
    fn absent_male_param_defaults_to_female() -> Result<(), TestError> {
        // A table with no `male` param resolves to female.
        let lad = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="headMesh" lod="0" file_name="avatar_head.llm">
    <param id="1" group="0" name="Plain" value_min="0" value_max="1"><param_morph/></param>
  </mesh>
</linden_avatar>"#;
        let params = VisualParams::from_xml(lad)?;
        let resolved = ResolvedParams::from_appearance(&params, &[128]);
        assert_eq!(resolved.avatar_sex(), ParamSex::Female);
        Ok(())
    }
}
