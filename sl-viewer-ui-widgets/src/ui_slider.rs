//! The **reusable slider widget**: the track box, the thumb inside it, and the
//! one system that puts the thumb where the value says.
//!
//! # Why this exists
//!
//! `bevy_ui_widgets` gives a slider its *behaviour* — its `Slider` takes the
//! presses and drags and writes [`SliderValue`] — and says in as many words that
//! drawing it is the stylist's job. So every panel that wanted one drew its own:
//! a bordered track node, an absolutely-positioned thumb inside it, a marker
//! component to find that thumb again, and a system computing
//! `fraction * (track - thumb)` to place it. Ten panels, ten copies, in seven
//! crates.
//!
//! They agreed on everything but their constants, and they were all wrong in the
//! same way: the thumb was given the track's **own** height, which is its
//! *border box*, while an absolutely-positioned child is placed inside the
//! border — so every thumb in the viewer hung one border-width past the bottom
//! of its track. One defect, ten places to fix it, and it survived because no
//! single place looked wrong (`viewer-ui-rows-shorter-than-their-text`).
//!
//! Here the thumb has **no height at all**: its block insets are both zero, so it
//! stretches to exactly the track's interior, and there is one place for that to
//! be true.
//!
//! # What a caller still owns
//!
//! The behaviour bundle (a plain `Slider` or a bound one), the `Name`, any
//! observer, and what the value *means*. [`spawn_slider`] returns the track, so
//! anything else goes on with an ordinary `insert` / `observe`.
//!
//! # The skin paints it
//!
//! The track wears [`SLIDER_CLASS`] and the thumb [`SLIDER_THUMB_CLASS`], so
//! `common.css` paints both from the `--track-bg`, `--control-border` and
//! `--slider-thumb` tokens and greys a refused one from `:disabled`. The colours
//! in [`SliderStyle`] are the **pre-load fallback** — what a headless harness,
//! which resolves no stylesheet, and the frame before the sheet lands measure —
//! the same arrangement as a `ButtonSpec`'s. Before the classes, every slider in
//! the viewer painted whatever colours its panel had picked, and no skin could
//! reach one.
//!
//! A **static** slider — a gallery specimen, a screenshot fixture — needs no
//! plugin: [`slider_thumb`] takes the fraction to draw at, so the thumb is in the
//! right place the moment it is spawned. [`SliderWidgetPlugin`] is what keeps a
//! *live* one in step with its value.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderThumb, SliderValue};
use bevy_flair::style::components::ClassList;

use sl_viewer_ui_core::ui::{LogicalInset, LogicalRect, resolve_logical_boxes};

/// The skin class on a slider's **track** — the node that carries the value,
/// the tab stop and, when refused, `InteractionDisabled`, so `:disabled` is
/// selected here and reaches the thumb as a descendant.
pub const SLIDER_CLASS: &str = "sk-slider";

/// The skin class on a slider's **thumb**.
pub const SLIDER_THUMB_CLASS: &str = "sk-slider-thumb";

/// How a slider is drawn: its geometry in **logical** pixels, and its colours.
///
/// The colours are the pre-load fallback: the skin paints the track and thumb
/// through [`SLIDER_CLASS`] and [`SLIDER_THUMB_CLASS`] (see the module docs).
///
/// A plain data struct rather than a builder: every field is load-bearing, a
/// slider with a default width would be a slider nobody sized, and the call
/// sites already had all seven values as constants of their own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderStyle {
    /// The track's width — the slider's whole extent along the inline axis.
    pub track_width: f32,
    /// The track's height, border included.
    pub track_height: f32,
    /// The track's border width, on every edge.
    pub border: f32,
    /// The track's border colour.
    pub border_color: Color,
    /// The track's fill, behind the thumb.
    pub track_fill: Color,
    /// The thumb's width. Its height is the track's interior, never a number.
    pub thumb_width: f32,
    /// The thumb's fill.
    pub thumb_fill: Color,
}

impl SliderStyle {
    /// How far the thumb travels: the track's width less the thumb's own, since
    /// the thumb has to stay inside the track at either end.
    ///
    /// The same number `bevy_ui_widgets`' core slider works out from the thumb's
    /// *measured* size when it converts a drag into a value, which is why the
    /// thumb must carry a real width and why the two agree.
    #[must_use]
    pub fn travel(&self) -> f32 {
        (self.track_width - self.thumb_width).max(0.0)
    }
}

/// The travel of the track this is on, logical px — what
/// [`place_slider_thumbs`] needs and the only thing it needs.
///
/// On the *track*, not the thumb: the track is what carries the value and the
/// range, so one lookup answers the whole question.
#[derive(Component, Debug, Clone, Copy)]
pub struct SliderTravel(f32);

/// Where in its range a slider sits, as 0…1.
///
/// Zero for an empty range rather than a division by it.
#[must_use]
pub fn slider_fraction(value: &SliderValue, range: &SliderRange) -> f32 {
    let span = range.span();
    if span > f32::EPSILON {
        ((value.0 - range.start()) / span).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The track's own bundle: the bordered box, its fallback colours and skin
/// class, its tab stop, and the travel its thumb will need.
///
/// `flex_shrink: 0.0` deliberately. The thumb is placed from `style`'s widths,
/// so a track squeezed by a crowded row would put its thumb somewhere the value
/// never said — a slider is a fixed-width control or it is lying.
#[must_use]
pub fn slider_track(style: SliderStyle, tab_index: i32) -> impl Bundle {
    (
        Node {
            width: Val::Px(style.track_width),
            height: Val::Px(style.track_height),
            border: UiRect::all(Val::Px(style.border)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(style.border_color),
        BackgroundColor(style.track_fill),
        ClassList::new_with_classes([SLIDER_CLASS]),
        TabIndex(tab_index),
        Pickable::default(),
        SliderTravel(style.travel()),
    )
}

/// The thumb's bundle, drawn at `fraction` (0…1) of its track's travel.
///
/// **No height**, which is the whole point of this widget existing: both block
/// insets are zero, so the thumb stretches to the track's interior. Handing it
/// the track's own height instead — as all ten hand-built sliders did — makes it
/// as tall as the *border box* while it sits inside the border, so it overhangs
/// the bottom by exactly the border.
#[must_use]
pub fn slider_thumb(style: SliderStyle, fraction: f32) -> impl Bundle {
    (
        SliderThumb,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(style.thumb_width),
            ..default()
        },
        LogicalInset(LogicalRect {
            inline_start: Val::Px(fraction.clamp(0.0, 1.0) * style.travel()),
            ..LogicalRect::ZERO
        }),
        BackgroundColor(style.thumb_fill),
        ClassList::new_with_classes([SLIDER_THUMB_CLASS]),
        // The press that starts a drag belongs to the track: it is what carries
        // the `Slider`, and it is what turns a pointer position into a value.
        Pickable::IGNORE,
    )
}

/// Spawn a whole slider under `parent`: the track carrying `behaviour`, with its
/// thumb inside it, drawn at `fraction`. Returns the **track**, which is the
/// entity every other component belongs on.
pub fn spawn_slider(
    commands: &mut Commands,
    parent: Entity,
    style: SliderStyle,
    tab_index: i32,
    fraction: f32,
    behaviour: impl Bundle,
) -> Entity {
    commands
        .spawn((behaviour, slider_track(style, tab_index), ChildOf(parent)))
        .with_child(slider_thumb(style, fraction))
        .id()
}

/// Slide every thumb to the value of the track it is on.
///
/// In `PostUpdate`, before the scaffold resolves logical boxes, rather than in
/// `Update` beside whatever wrote the value: this way it sees the *settled*
/// value of the frame whoever set it — a drag, a trackball, a setting reloaded
/// from the store — instead of each panel having to chain it after its own
/// writer and a new writer silently arriving a frame late.
pub fn place_slider_thumbs(
    sliders: Query<(&SliderValue, &SliderRange, &SliderTravel, &Children)>,
    mut thumbs: Query<&mut LogicalInset, With<SliderThumb>>,
) {
    for (value, range, travel, children) in &sliders {
        let offset = Val::Px(slider_fraction(value, range) * travel.0);
        for child in children {
            if let Ok(mut inset) = thumbs.get_mut(*child)
                && inset.0.inline_start != offset
            {
                // Only on a change: the inset is a logical box, and touching one
                // re-resolves it and dirties the node's layout. A slider nobody
                // is dragging should cost nothing.
                inset.0.inline_start = offset;
            }
        }
    }
}

/// Keeps every live slider's thumb in step with its value.
///
/// Idempotent to add, like the other widget plugins here: a panel that draws a
/// slider adds it and the first one to arrive wins, because the viewer adds its
/// windows one at a time and no plugin group owns them all.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliderWidgetPlugin;

impl Plugin for SliderWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            place_slider_thumbs.before(resolve_logical_boxes),
        );
    }
}

#[cfg(test)]
mod tests {
    //! Two things a layout sweep cannot tell you about this widget: that the
    //! thumb has a *size* at all, and that it is where the value says.
    //!
    //! The first matters because the thumb is deliberately given no height —
    //! a stretch that failed to stretch would be a zero-height node, invisible
    //! and passing every overflow check in the harness by being nothing.

    use super::{SLIDER_CLASS, SLIDER_THUMB_CLASS, SliderStyle, SliderWidgetPlugin, spawn_slider};
    use bevy::prelude::*;
    use bevy::ui_widgets::{Slider, SliderRange, SliderValue};
    use bevy_flair::style::components::ClassList;
    use pretty_assertions::assert_eq;
    use sl_viewer_testkit::{LayoutTest, TestError, settle, spawn_under_root};

    /// A slider with a 2 px border, so the interior is 4 px shorter than the
    /// track and the difference is visible in a whole number of pixels.
    const STYLE: SliderStyle = SliderStyle {
        track_width: 200.0,
        track_height: 16.0,
        border: 2.0,
        border_color: Color::WHITE,
        track_fill: Color::BLACK,
        thumb_width: 10.0,
        thumb_fill: Color::WHITE,
    };

    /// Spawn one slider at `value` over 0…100 and settle. Gives back the track
    /// and its thumb.
    fn laid_out(
        app: &mut App,
        value: f32,
        with_plugin: bool,
    ) -> Result<(Entity, Entity), TestError> {
        if with_plugin {
            app.add_plugins(SliderWidgetPlugin);
        }
        let row = spawn_under_root(app, (Node::default(), Name::new("row")));
        let mut commands = app.world_mut().commands();
        let track = spawn_slider(
            &mut commands,
            row,
            STYLE,
            0,
            0.0,
            (
                Slider::default(),
                SliderValue(value),
                SliderRange::new(0.0, 100.0),
            ),
        );
        app.world_mut().flush();
        settle(app);
        let thumb = *app
            .world()
            .entity(track)
            .get::<Children>()
            .and_then(|children| children.first())
            .ok_or("the track has no thumb")?;
        Ok((track, thumb))
    }

    /// The thumb fills the track's **interior** — the box inside its border —
    /// rather than being the track's own height, or nothing at all.
    #[test]
    fn a_thumb_fills_its_track_interior() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (track, thumb) = laid_out(&mut app, 0.0, false)?;
        let interior = app
            .world()
            .entity(track)
            .get::<ComputedNode>()
            .ok_or("the track has no layout")?
            .content_box()
            .size();
        let thumb_box = app
            .world()
            .entity(thumb)
            .get::<ComputedNode>()
            .ok_or("the thumb has no layout")?
            .size;
        // Bit-exact: the thumb is *stretched* to the interior, so it is that
        // number or the stretch did not happen.
        assert_eq!(
            thumb_box.y.to_bits(),
            interior.y.to_bits(),
            "the thumb is {} px tall in a {} px interior — a stretch that did not \
             stretch is an invisible widget every layout check would pass",
            thumb_box.y,
            interior.y,
        );
        assert_eq!(
            thumb_box.x.to_bits(),
            STYLE.thumb_width.to_bits(),
            "the thumb is {} px wide, not the {} it asked for",
            thumb_box.x,
            STYLE.thumb_width,
        );
        Ok(())
    }

    /// With the plugin, the thumb moves to where the value sits in its range.
    #[test]
    fn a_thumb_sits_where_the_value_says() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (_, thumb) = laid_out(&mut app, 25.0, true)?;
        let node = app
            .world()
            .entity(thumb)
            .get::<Node>()
            .ok_or("the thumb has no node")?;
        assert_eq!(
            node.left,
            Val::Px(0.25 * STYLE.travel()),
            "a quarter of the way along a 0…100 range is a quarter of the travel",
        );
        Ok(())
    }

    /// **The skin can reach a slider.** The track and the thumb each carry
    /// their class, so `common.css` paints them from tokens; before this every
    /// slider painted the colours its panel picked, and no skin could restyle
    /// one.
    #[test]
    fn a_slider_carries_the_classes_the_skin_paints() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (track, thumb) = laid_out(&mut app, 0.0, false)?;
        for (node, class) in [(track, SLIDER_CLASS), (thumb, SLIDER_THUMB_CLASS)] {
            assert!(
                app.world()
                    .entity(node)
                    .get::<ClassList>()
                    .is_some_and(|classes| classes.contains(class)),
                "a slider node without `{class}` is one no skin can paint"
            );
        }
        Ok(())
    }

    /// **A live slider moves under the pointer.** A drag from the thumb at the
    /// start of the track to three quarters along it carries the value there,
    /// through the real input and picking stack.
    #[test]
    fn a_drag_along_the_track_moves_the_value() -> Result<(), TestError> {
        use sl_viewer_testkit::interact::{self, InteractionTest};
        let mut app = InteractionTest::new().build();
        let (track, _thumb) = laid_out(&mut app, 0.0, true)?;
        // The headless slider only announces a move; its owner writes the
        // value back, as `settings_binding`'s observer does.
        app.world_mut()
            .entity_mut(track)
            .insert(Name::new("slider-under-test"))
            .observe(
                |change: On<bevy::ui_widgets::ValueChange<f32>>, mut commands: Commands| {
                    commands
                        .entity(change.source)
                        .insert(SliderValue(change.value));
                },
            );
        settle(&mut app);
        let centre = interact::centre_of(&mut app, "slider-under-test")
            .ok_or("the track has no position")?;
        let from = centre - Vec2::new(STYLE.track_width / 2.0 - STYLE.thumb_width / 2.0, 0.0);
        let to = centre + Vec2::new(STYLE.track_width / 4.0, 0.0);
        interact::drag(&mut app, from, to, 8, MouseButton::Left);
        settle(&mut app);
        let value = app
            .world()
            .get::<SliderValue>(track)
            .map(|value| value.0)
            .ok_or("no value")?;
        assert!(
            value > 50.0,
            "a drag three quarters along the track left the value at {value}"
        );
        Ok(())
    }
}
