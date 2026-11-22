//! What a [`CalculatedClip`] leaves visible, as a screen rectangle.

use bevy::prelude::*;

/// The screen rectangle `clip` leaves visible, in physical pixels.
///
/// Since Bevy 0.20 a clip is a list of rectangles, each in its clipping node's
/// local space. A viewer's clipping ancestors are unrotated, so each maps to an
/// axis-aligned screen rectangle and the visible part is their intersection. A
/// node nothing clips keeps the whole plane; a fully clipped one gets an empty
/// rectangle.
#[must_use]
pub fn visible_rect(clip: &CalculatedClip) -> Rect {
    let Some(rects) = clip.rects() else {
        return Rect::from_corners(Vec2::ZERO, Vec2::ZERO);
    };
    let everywhere = Rect {
        min: Vec2::splat(f32::MIN),
        max: Vec2::splat(f32::MAX),
    };
    rects.iter().fold(everywhere, |visible, clip_rect| {
        // Per axis rather than `transform_point2`: an axis that does not clip
        // is infinite, and the zero off-diagonal times infinity would be NaN.
        let to_world = clip_rect.world_to_clip_local.inverse();
        let scale = Vec2::new(to_world.matrix2.x_axis.x, to_world.matrix2.y_axis.y);
        visible.intersect(Rect::from_corners(
            clip_rect.rect.min.mul_add(scale, to_world.translation),
            clip_rect.rect.max.mul_add(scale, to_world.translation),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::visible_rect;
    use bevy::math::Affine2;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// A clip rect in a node's local space, `to_local` moving the screen into it
    /// (the negated position of the node's centre).
    fn at(to_local: Vec2, half: Vec2) -> CalculatedClipRect {
        CalculatedClipRect {
            rect: Rect::from_center_half_size(Vec2::ZERO, half),
            world_to_clip_local: Affine2::from_translation(to_local),
        }
    }

    #[test]
    fn nested_clips_intersect_on_screen() {
        let clip = CalculatedClip::Rects(
            vec![
                at(Vec2::new(-50.0, -50.0), Vec2::splat(50.0)),
                at(Vec2::new(-100.0, -50.0), Vec2::splat(25.0)),
            ]
            .into(),
        );
        assert_eq!(visible_rect(&clip), Rect::new(75.0, 25.0, 100.0, 75.0));
    }

    #[test]
    fn an_axis_that_does_not_clip_stays_open() {
        let clip = CalculatedClip::Rects(
            vec![CalculatedClipRect {
                rect: Rect {
                    min: Vec2::new(f32::NEG_INFINITY, -25.0),
                    max: Vec2::new(f32::INFINITY, 25.0),
                },
                world_to_clip_local: Affine2::from_translation(-Vec2::new(100.0, 25.0)),
            }]
            .into(),
        );
        let visible = visible_rect(&clip);
        assert_eq!((visible.min.y, visible.max.y), (0.0, 50.0));
        assert!(visible.contains(Vec2::new(5000.0, 10.0)));
        assert!(!visible.contains(Vec2::new(5000.0, 120.0)));
    }

    #[test]
    fn a_fully_clipped_node_shows_nothing() {
        assert!(visible_rect(&CalculatedClip::FullyClipped).is_empty());
    }
}
