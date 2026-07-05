//! Evaluate the **tint colour** a wearable's visual params give one bake layer,
//! for the client-side baker (P15.2).
//!
//! A bake layer's colour comes either from a named **global colour**
//! (`skin_color` / `hair_color` / `eye_color`, tinting the skin / hair / eyes
//! base layers by the body-part's colour params) or from a layer's own inline
//! **colour params** (a clothing layer's red / green / blue tint). Both are the
//! reference viewer's `LLTexLayer::calculateTexLayerColor`: a running colour that
//! starts black-transparent and is combined param-by-param per each param's
//! [`ColorOp`], then clamped. Each param contributes its [`ColorRamp::net_color`](crate::params::ColorRamp::net_color)
//! at the wearable's stored weight (falling back to the param default).
//!
//! This is I/O- and Bevy-free like the rest of `sl-avatar`: it reads a
//! [`VisualParams`] table (already parsed from `avatar_lad.xml`) plus a closure
//! giving each param's weight (from the worn wearable asset), and returns a plain
//! linear-RGBA tint the `sl-bake` compositor multiplies a layer by.

use crate::params::{ColorOp, ParamEffect, VisualParams};

/// The colour params (in reference-viewer order) that make up each named
/// `<global_color>` in `avatar_lad.xml`. Hardcoded because only these three
/// exist and their membership is stable; the param ramps / operations themselves
/// still come from the parsed [`VisualParams`] table, so nothing about the actual
/// colours is duplicated here.
const GLOBAL_COLORS: [(&str, &[i32]); 3] = [
    // Pigment (111), Red Skin (110), Rainbow Color (108).
    ("skin_color", &[111, 110, 108]),
    // Blonde (114), Red (113), White (115), Rainbow (112).
    ("hair_color", &[114, 113, 115, 112]),
    // Eye Color (99), Eye Lightness (98).
    ("eye_color", &[99, 98]),
];

/// The param ids composing the named global colour, or `None` for an unknown
/// name.
#[must_use]
pub fn global_color_params(name: &str) -> Option<&'static [i32]> {
    GLOBAL_COLORS
        .iter()
        .find_map(|&(global, ids)| (global == name).then_some(ids))
}

/// The tint for a named global colour (`skin_color` / `hair_color` /
/// `eye_color`), or `None` for an unknown name. `weight_of` gives the wearable's
/// stored weight for a param id (the eval falls back to the param default when it
/// returns `None`). See [`combine_layer_color`].
#[must_use]
pub fn global_color(
    params: &VisualParams,
    name: &str,
    weight_of: impl Fn(i32) -> Option<f32>,
) -> Option<[f32; 4]> {
    let ids = global_color_params(name)?;
    Some(combine_layer_color(params, ids, weight_of))
}

/// Combine the colour params `ids` into one tint, replicating the reference
/// viewer's `LLTexLayer::calculateTexLayerColor`: start from transparent black
/// and fold each param's [`ColorRamp::net_color`](crate::params::ColorRamp::net_color) in by its [`ColorOp`]
/// (`Add` / `Multiply` / `Blend`), then clamp to `0.0..=1.0`.
///
/// A param that is absent from the table, or is not a colour param, is skipped.
/// `weight_of(id)` supplies the wearable's stored weight for a param; when it
/// returns `None` the param's default weight is used (matching a wearable that
/// omits a param). An empty `ids` yields opaque white (no tint).
#[must_use]
pub fn combine_layer_color(
    params: &VisualParams,
    ids: &[i32],
    weight_of: impl Fn(i32) -> Option<f32>,
) -> [f32; 4] {
    if ids.is_empty() {
        return [1.0, 1.0, 1.0, 1.0];
    }
    let mut net = [0.0_f32; 4];
    for &id in ids {
        let Some(param) = params.get(id) else {
            continue;
        };
        let ParamEffect::Color(ramp) = &param.effect else {
            continue;
        };
        let weight = weight_of(id).unwrap_or(param.default);
        let param_net = ramp.net_color(weight);
        net = match ramp.operation {
            ColorOp::Add => zip4(net, param_net, |a, b| a + b),
            ColorOp::Multiply => zip4(net, param_net, |a, b| a * b),
            ColorOp::Blend => zip4(net, param_net, |a, b| a + (b - a) * weight),
        };
    }
    zip4(net, net, |a, _b| a.clamp(0.0, 1.0))
}

/// Combine two RGBA quads channel-by-channel with `op` (avoiding variable
/// indexing so the strict lints stay happy).
fn zip4(a: [f32; 4], b: [f32; 4], op: impl Fn(f32, f32) -> f32) -> [f32; 4] {
    [
        op(a[0], b[0]),
        op(a[1], b[1]),
        op(a[2], b[2]),
        op(a[3], b[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::{combine_layer_color, global_color, global_color_params};
    use crate::params::VisualParams;
    use pretty_assertions::assert_eq;

    /// A boxed error so a test can `?` through `from_xml` without `expect`.
    type TestError = Box<dyn std::error::Error>;

    /// Assert two RGBA quads match within a small tolerance (avoids strict
    /// float comparison).
    fn assert_close(actual: [f32; 4], expected: [f32; 4]) {
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert!((a - e).abs() < 0.01, "{actual:?} != {expected:?}");
        }
    }

    /// A minimal `avatar_lad.xml` defining just the three colour params a
    /// two-param global colour needs, so the eval can be exercised without the
    /// full character file.
    const LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="1.0">
  <global_color name="test_color">
    <param id="900" group="0" name="Base" value_min="0" value_max="1" value_default="0.5">
      <param_color>
        <value color="0, 0, 0, 255" />
        <value color="200, 100, 50, 255" />
      </param_color>
    </param>
    <param id="901" group="0" name="Mul" value_min="0" value_max="1" value_default="1">
      <param_color operation="multiply">
        <value color="255, 255, 255, 255" />
      </param_color>
    </param>
  </global_color>
  <param id="910" group="0" name="RedCh" value_min="0" value_max="1" value_default="1">
    <param_color>
      <value color="0, 0, 0, 255" />
      <value color="255, 0, 0, 255" />
    </param_color>
  </param>
</linden_avatar>
"#;

    #[test]
    fn add_op_interpolates_ramp_by_weight() -> Result<(), TestError> {
        let params = VisualParams::from_xml(LAD)?;
        // Param 900 (add) at weight 0.5 → halfway 0..(200,100,50) = (100,50,25);
        // param 901 (multiply, single white stop) → *1 → unchanged.
        let tint = combine_layer_color(&params, &[900, 901], |_id| Some(0.5));
        // 100/255≈0.392, 50/255≈0.196, 25/255≈0.098.
        assert_close(tint, [0.392, 0.196, 0.098, 1.0]);
        Ok(())
    }

    #[test]
    fn missing_weight_uses_default() -> Result<(), TestError> {
        let params = VisualParams::from_xml(LAD)?;
        // No weights supplied → param 900 default 0.5 → same as an explicit 0.5.
        let with_default = combine_layer_color(&params, &[900], |_id| None);
        let explicit = combine_layer_color(&params, &[900], |_id| Some(0.5));
        assert_close(with_default, explicit);
        Ok(())
    }

    #[test]
    fn empty_ids_is_white() -> Result<(), TestError> {
        let params = VisualParams::from_xml(LAD)?;
        assert_close(
            combine_layer_color(&params, &[], |_id| None),
            [1.0, 1.0, 1.0, 1.0],
        );
        Ok(())
    }

    #[test]
    fn single_red_channel_param_at_full() -> Result<(), TestError> {
        let params = VisualParams::from_xml(LAD)?;
        // Param 910 at weight 1 → pure red (add from black).
        assert_close(
            combine_layer_color(&params, &[910], |_id| Some(1.0)),
            [1.0, 0.0, 0.0, 1.0],
        );
        Ok(())
    }

    #[test]
    fn global_color_membership() {
        assert_eq!(
            global_color_params("skin_color"),
            Some(&[111, 110, 108][..])
        );
        assert_eq!(
            global_color_params("hair_color"),
            Some(&[114, 113, 115, 112][..])
        );
        assert_eq!(global_color_params("eye_color"), Some(&[99, 98][..]));
        assert_eq!(global_color_params("nope"), None);
    }

    #[test]
    fn unknown_global_color_is_none() -> Result<(), TestError> {
        let params = VisualParams::from_xml(LAD)?;
        assert_eq!(global_color(&params, "nope", |_id| None), None);
        Ok(())
    }
}
