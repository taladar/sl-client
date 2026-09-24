//! The shared **scrollbar** widget (`viewer-skin-scrollbar-shape`).
//!
//! Six places in the viewer drew a scrollbar, and they drew it six ways: the
//! windowed list its own two rectangles, the tab strip, a filling tab page, the
//! UI gallery, the avatar profile's group list and the About box each a
//! `bevy_ui_widgets` [`Scrollbar`] with the thumb as its only child — two of
//! them in colour constants no skin could reach, and the other three with a
//! thumb class no stylesheet ever resolved (see the thumb's spawn). Every one
//! was a bar and nothing else.
//!
//! The reference's (`llscrollbar.cpp`) is four parts — a decrement button, a
//! groove, a thumb and an increment button — and every skin dresses all four.
//! This module is that shape, once:
//!
//! ```text
//! frame (.sk-scrollbar-vertical)   — the column the caller places
//! ├─ arrow (.sk-scrollbar-arrow)   — ▲, steps toward the start
//! ├─ groove (.sk-scrollbar-track)  — flex-grows into what the arrows leave
//! │  └─ thumb (.sk-scrollbar-thumb)
//! └─ arrow (.sk-scrollbar-arrow)   — ▼, steps toward the end
//! ```
//!
//! # The arrows are always there, and the skin decides whether they show
//!
//! Both ends are spawned on every bar with `display: none`, and
//! `common.css`'s `.sk-scrollbar-arrow` rule sets `display` from the
//! `--scrollbar-arrows` token. The flat skins say `none`, which keeps the bar
//! they have always had; a classic skin says `flex` and gets the ends, with no
//! Rust involved and no setting to plumb.
//!
//! That works because **the ends take their length out of the groove, not out
//! of the content.** The thumb is sized and placed against the groove's own
//! measured length — bevy's [`Scrollbar`] does that for a container, and
//! `virtual_list`'s driver does the same for a list — so a groove that lost
//! two arrows' worth of length to a skin still maps its whole travel onto the
//! whole scroll range. The scroll maths never learns the arrows exist.
//!
//! # A press steps once, and holding it repeats
//!
//! Each arrow is `bevy_ui_widgets`' own [`Button`] with [`ActivateOnPress`] and
//! [`HoldToRepeat`] — the reference's `LLButton` with a `mouse_held_callback`
//! — so the step lands on the press and repeats while the arrow is held.
//!
//! One step is a **row** for a list (the reference's scroll list steps by one
//! item) and `CONTAINER_STEP` pixels for anything else (its scroll
//! container's `VERTICAL_MULTIPLE`).
//!
//! # Vertical and horizontal
//!
//! [`spawn_scrollbar`] is the vertical bar, for a list or a container;
//! [`spawn_horizontal_scrollbar`] the horizontal one, for a container only —
//! a windowed list has one axis. A horizontal bar is **physical**: it maps
//! `ScrollPosition.x`, which is physical, and its thumb is placed from the
//! groove's left edge, so its ◀ stays on the left under a right-to-left locale
//! (`keep_horizontal_bars_physical`) rather than mirroring into an arrow
//! that points one way and scrolls the other.
//!
//! # Hidden while there is nothing to scroll
//!
//! Every bar is shown only while its target overflows on the bar's axis — the
//! reference's scroll containers hide a bar they do not need, and a bar whose
//! thumb fills its whole groove says nothing but "this could scroll". A
//! container's bar is also hidden while the container itself is `Hidden` (a
//! tab page that is not the selected one), so it never floats over the page
//! that is.
//!
//! It is `Visibility`, not `Display`: the bar keeps its space, so the content
//! never reflows by a bar's width as it crosses the threshold (and so cannot
//! flicker across it). The windowed list's bar is the exception, because its
//! rows reclaim the space and nothing reflows (`virtual_list`).
//!
//! That is not optional, so it cannot depend on a host remembering a plugin:
//! [`ScrollbarWidgetPlugin`] is added by every widget plugin that spawns a bar
//! ([`ensure_scrollbar_widget`]) — the windowed list's and the tab strip's —
//! and by any host that builds a bar of its own.
//!
//! Nor do the arrows take focus: the reference gives them `tab_stop(false)`,
//! and the list or page they scroll is the thing the keyboard reaches.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::ui_widgets::{
    Activate, ActivateOnPress, Button, ControlOrientation, Scrollbar, ScrollbarThumb,
};
use bevy_flair::style::components::{ClassList, PseudoElementsSupport, Styled};

use crate::hold_repeat::{HoldToRepeat, ensure_hold_repeat};
use crate::skin::{
    SCROLLBAR_ARROW_CLASS, SCROLLBAR_ARROW_DOWN_CLASS, SCROLLBAR_ARROW_GLYPH_CLASS,
    SCROLLBAR_ARROW_LEFT_CLASS, SCROLLBAR_ARROW_RIGHT_CLASS, SCROLLBAR_ARROW_UP_CLASS,
    SCROLLBAR_CORNER_CLASS, SCROLLBAR_HORIZONTAL_CLASS, SCROLLBAR_THUMB_CLASS,
    SCROLLBAR_TRACK_CLASS, SCROLLBAR_VERTICAL_CLASS,
};
use crate::skin_palette::SkinPalette;
use crate::ui::UiDirection;
use crate::ui_font::UiFont;
use crate::virtual_list::{VirtualList, spawn_list_groove};

/// The bar's thickness, in logical pixels, before a stylesheet says otherwise
/// — `--scrollbar-thickness` in every shipped skin, and what a headless world
/// with no stylesheet lays out at.
///
/// Public because a list's rows stop short of a visible bar by its width, and
/// a consumer laying anything out *beside* them reads that width through
/// [`VirtualList::scrollbar_inset`] — which is this until the bar has been
/// measured.
pub const SCROLLBAR_THICKNESS: f32 = 10.0;

/// The thumb's shortest length, in logical pixels, so it stays grabbable on a
/// very long list.
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;

/// How far one arrow step moves a scrolling **container**, in logical pixels —
/// the reference scroll container's `VERTICAL_MULTIPLE`. A list steps by one
/// row instead.
const CONTAINER_STEP: f32 = 16.0;

/// The arrow glyph's size against the bar's thickness, before a stylesheet
/// sizes it — the checkbox tick's proportion.
const ARROW_GLYPH_SCALE: f32 = 0.8;

/// What a scrollbar scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTarget {
    /// A node that scrolls its own overflow through [`ScrollPosition`] — a page,
    /// a panel, a list of real children. The groove is `bevy_ui_widgets`'
    /// [`Scrollbar`].
    Container(Entity),
    /// A windowed list's viewport, carrying [`VirtualList`], which owns its own
    /// offset. The groove is driven by `virtual_list`.
    List(Entity),
}

/// The frame of a scrollbar: the column (or row) holding the two arrows and
/// the groove, and the entity a caller places, hides and names.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarFrame {
    /// What the bar scrolls.
    pub target: ScrollTarget,
    /// Which axis it scrolls.
    pub orientation: ControlOrientation,
}

/// One arrow end of a scrollbar.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct ScrollArrow {
    /// What the arrow scrolls.
    target: ScrollTarget,
    /// Which axis it steps.
    orientation: ControlOrientation,
    /// Whether it steps toward the end (`▼` / `▶`) or the start (`▲` / `◀`).
    toward_end: bool,
}

/// The scrollbar's runtime half: hiding a container's bar while there is
/// nothing to scroll, the arrows' hold-to-repeat, and the horizontal bars'
/// physical order. Add it with [`ensure_scrollbar_widget`], never directly:
/// several widget plugins need it, and a plugin added twice is a panic.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollbarWidgetPlugin;

impl Plugin for ScrollbarWidgetPlugin {
    fn build(&self, app: &mut App) {
        ensure_hold_repeat(app);
        app.add_systems(Update, keep_horizontal_bars_physical)
            // After layout, because it reads the container's *measured*
            // overflow — the tab strip's controls' shape.
            .add_systems(PostUpdate, hide_idle_scrollbars.after(UiSystems::Layout));
    }
}

/// Add [`ScrollbarWidgetPlugin`] to `app` unless something already has — the
/// one line a plugin whose widgets spawn a bar calls from its own `build`, so
/// no host can end up with bars that never hide.
pub fn ensure_scrollbar_widget(app: &mut App) {
    if !app.is_plugin_added::<ScrollbarWidgetPlugin>() {
        app.add_plugins(ScrollbarWidgetPlugin);
    }
}

/// Spawn a **vertical** scrollbar for `target` under `parent`, and return its
/// frame.
///
/// `frame` is where the caller says where the bar goes — absolutely pinned to
/// a viewport's edge, a flex sibling beside a page, a grid cell's trailing
/// edge. The widget owns the rest of it: the direction its parts stack in,
/// its thickness, that it never shrinks, and that it shows only while there is
/// something to scroll (see the [module docs](self)).
///
/// `name` names the frame; the parts are named after it (`{name}:thumb`,
/// `{name}:track`, `{name}:up`, `{name}:down`) so a test can aim at each.
pub fn spawn_scrollbar(
    commands: &mut Commands,
    parent: Entity,
    target: ScrollTarget,
    frame: Node,
    name: &str,
) -> Entity {
    spawn_bar(
        commands,
        parent,
        target,
        ControlOrientation::Vertical,
        frame,
        name,
    )
}

/// Spawn a **horizontal** scrollbar for a scrolling `container` under
/// `parent`, and return its frame — [`spawn_scrollbar`] turned on its side,
/// with the ends named `{name}:left` / `{name}:right`.
pub fn spawn_horizontal_scrollbar(
    commands: &mut Commands,
    parent: Entity,
    container: Entity,
    frame: Node,
    name: &str,
) -> Entity {
    spawn_bar(
        commands,
        parent,
        ScrollTarget::Container(container),
        ControlOrientation::Horizontal,
        frame,
        name,
    )
}

/// The square where a vertical and a horizontal bar meet, for a surface that
/// scrolls both ways — the bundle to spawn under `parent`, below the vertical
/// bar and beside the horizontal one. The reference leaves the same square;
/// without it one bar's end runs under the other's.
///
/// It takes the groove's colour and the bars' thickness from the skin, so it
/// stays square whatever `--scrollbar-thickness` says.
#[must_use]
pub fn scrollbar_corner(parent: Entity) -> impl Bundle {
    (
        Node {
            width: Val::Px(SCROLLBAR_THICKNESS),
            height: Val::Px(SCROLLBAR_THICKNESS),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(SkinPalette::default().track_bg),
        ClassList::new_with_classes([SCROLLBAR_CORNER_CLASS]),
        Name::new("scrollbar-corner"),
        ChildOf(parent),
    )
}

/// The one spawner behind both orientations.
fn spawn_bar(
    commands: &mut Commands,
    parent: Entity,
    target: ScrollTarget,
    orientation: ControlOrientation,
    frame: Node,
    name: &str,
) -> Entity {
    let palette = SkinPalette::default();
    let vertical = orientation == ControlOrientation::Vertical;
    let (frame_node, frame_class, ends) = if vertical {
        (
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(SCROLLBAR_THICKNESS),
                flex_shrink: 0.0,
                ..frame
            },
            SCROLLBAR_VERTICAL_CLASS,
            [
                ("up", SCROLLBAR_ARROW_UP_CLASS),
                ("down", SCROLLBAR_ARROW_DOWN_CLASS),
            ],
        )
    } else {
        (
            Node {
                flex_direction: FlexDirection::Row,
                height: Val::Px(SCROLLBAR_THICKNESS),
                flex_shrink: 0.0,
                ..frame
            },
            SCROLLBAR_HORIZONTAL_CLASS,
            [
                ("left", SCROLLBAR_ARROW_LEFT_CLASS),
                ("right", SCROLLBAR_ARROW_RIGHT_CLASS),
            ],
        )
    };
    let frame = commands
        .spawn((
            frame_node,
            // Hidden until the first layout says there is something to scroll,
            // so a bar never flashes up on a panel that fits.
            Visibility::Hidden,
            ClassList::new_with_classes([frame_class]),
            ScrollbarFrame {
                target,
                orientation,
            },
            Name::new(name.to_owned()),
            ChildOf(parent),
        ))
        .id();
    let [(start_name, start_class), (end_name, end_class)] = ends;
    let arrow = ScrollArrow {
        target,
        orientation,
        toward_end: false,
    };
    spawn_scroll_arrow(
        commands,
        frame,
        arrow,
        start_class,
        &format!("{name}:{start_name}"),
    );
    let groove_node = if vertical {
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            width: Val::Percent(100.0),
            ..default()
        }
    } else {
        Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            height: Val::Percent(100.0),
            ..default()
        }
    };
    let groove = commands
        .spawn((
            groove_node,
            BackgroundColor(palette.track_bg),
            ClassList::new_with_classes([SCROLLBAR_TRACK_CLASS]),
            Name::new(format!("{name}:track")),
            ChildOf(frame),
        ))
        .id();
    match target {
        ScrollTarget::Container(container) => {
            commands.entity(groove).insert(Scrollbar {
                target: container,
                orientation,
                min_thumb_length: SCROLLBAR_MIN_THUMB,
            });
            commands.spawn((
                ScrollbarThumb::default(),
                // `bevy_flair` makes `Styled` a required component of `Node`
                // and `TextSpan`, and of nothing else — and this thumb has no
                // `Node` (bevy lays it out by hand, after layout), so without
                // this it is never styled at all: its class names a colour no
                // skin can reach. Every container scrollbar in the viewer drew
                // the fallback thumb for exactly that reason until the skin
                // test for this widget asked what colour it was.
                Styled::default(),
                BackgroundColor(palette.scrollbar_thumb),
                ClassList::new_with_classes([SCROLLBAR_THUMB_CLASS]),
                Name::new(format!("{name}:thumb")),
                ChildOf(groove),
            ));
        }
        ScrollTarget::List(viewport) => {
            spawn_list_groove(commands, groove, viewport, &format!("{name}:thumb"));
        }
    }
    spawn_scroll_arrow(
        commands,
        frame,
        ScrollArrow {
            toward_end: true,
            ..arrow
        },
        end_class,
        &format!("{name}:{end_name}"),
    );
    frame
}

/// Spawn one arrow end: a square button, hidden until a skin shows it, holding
/// an empty glyph host whose `::before` the stylesheet fills — so which arrow
/// a skin draws is its own choice, as the checkbox's tick is.
fn spawn_scroll_arrow(
    commands: &mut Commands,
    frame: Entity,
    arrow: ScrollArrow,
    direction_class: &'static str,
    name: &str,
) {
    let (width, height) = if arrow.orientation == ControlOrientation::Vertical {
        (Val::Percent(100.0), Val::Px(SCROLLBAR_THICKNESS))
    } else {
        (Val::Px(SCROLLBAR_THICKNESS), Val::Percent(100.0))
    };
    let entity = commands
        .spawn((
            Node {
                width,
                height,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                overflow: Overflow::clip(),
                // Off until a skin's `--scrollbar-arrows` says otherwise. The
                // stylesheet's resting rule writes this same property, so there
                // is a baseline for it to come back to.
                display: Display::None,
                ..default()
            },
            BackgroundColor(SkinPalette::default().control_bg),
            ClassList::new_with_classes([SCROLLBAR_ARROW_CLASS, direction_class]),
            Button,
            ActivateOnPress,
            HoldToRepeat::default(),
            arrow,
            Name::new(name.to_owned()),
            ChildOf(frame),
        ))
        .observe(on_scroll_arrow)
        .id();
    commands.spawn((
        Text::default(),
        PseudoElementsSupport,
        UiFont::Sans.at(SCROLLBAR_THICKNESS * ARROW_GLYPH_SCALE),
        ClassList::new_with_classes([SCROLLBAR_ARROW_GLYPH_CLASS]),
        Pickable::IGNORE,
        ChildOf(entity),
    ));
}

/// The two kinds of thing an arrow can move, as one parameter.
#[derive(SystemParam)]
struct ScrollTargets<'w, 's> {
    /// Windowed lists, which own their offset.
    lists: Query<'w, 's, &'static mut VirtualList>,
    /// Scrolling containers, and the measured size their range comes from.
    containers: Query<'w, 's, (&'static mut ScrollPosition, &'static ComputedNode)>,
}

impl ScrollTargets<'_, '_> {
    /// Move the arrow's target one step along its axis.
    fn step(&mut self, arrow: ScrollArrow) {
        let sign = if arrow.toward_end { 1.0 } else { -1.0 };
        match arrow.target {
            ScrollTarget::List(viewport) => {
                if let Ok(mut list) = self.lists.get_mut(viewport) {
                    // `scroll_by` floors at the top, and the layout pass clamps
                    // the far end against the live viewport, as for the wheel.
                    let row = list.row_height;
                    list.scroll_by(sign * row);
                }
            }
            ScrollTarget::Container(container) => {
                if let Ok((mut position, computed)) = self.containers.get_mut(container) {
                    let range = axis_range(computed, arrow.orientation);
                    let offset = along(position.0, arrow.orientation);
                    let next = step_container(offset, sign * CONTAINER_STEP, range);
                    match arrow.orientation {
                        ControlOrientation::Horizontal => position.x = next,
                        ControlOrientation::Vertical => position.y = next,
                    }
                }
            }
        }
    }
}

/// `value`'s component along the axis `orientation` scrolls.
const fn along(value: Vec2, orientation: ControlOrientation) -> f32 {
    match orientation {
        ControlOrientation::Horizontal => value.x,
        ControlOrientation::Vertical => value.y,
    }
}

/// How far a container can scroll along `orientation`'s axis, in logical
/// pixels — the range `bevy_ui_widgets`' own [`Scrollbar`] drags within.
fn axis_range(computed: &ComputedNode, orientation: ControlOrientation) -> f32 {
    let visible = (along(computed.size(), orientation)
        - along(computed.scrollbar_size, orientation))
        * computed.inverse_scale_factor();
    let content = along(computed.content_size(), orientation) * computed.inverse_scale_factor();
    (content - visible).max(0.0)
}

/// A container's offset after a step of `delta`, clamped to `range`.
///
/// Clamped at **both** ends, not only at the start. `bevy_ui` clamps what it
/// *draws* and leaves the stored [`ScrollPosition`] as written, so an offset
/// pushed past the end banks slack that the next several steps back spend
/// doing nothing visible — the gallery's page found that the hard way.
fn step_container(offset: f32, delta: f32, range: f32) -> f32 {
    (offset + delta).clamp(0.0, range)
}

/// An arrow was pressed, or is being held: step once.
fn on_scroll_arrow(
    activate: On<Activate>,
    arrows: Query<&ScrollArrow>,
    mut targets: ScrollTargets,
) {
    if let Ok(arrow) = arrows.get(activate.entity) {
        targets.step(*arrow);
    }
}

/// Keep every horizontal bar's parts in **physical** order — ◀, groove, ▶
/// from left to right — under either reading direction. The scaffold writes
/// the live direction onto every node, which would flow a plain row right to
/// left under RTL; a reversed row under RTL flows left to right again.
fn keep_horizontal_bars_physical(
    direction: Option<Res<UiDirection>>,
    mut frames: Query<(&ScrollbarFrame, &mut Node)>,
) {
    let rtl = direction.is_some_and(|direction| direction.is_rtl());
    let wanted = if rtl {
        FlexDirection::RowReverse
    } else {
        FlexDirection::Row
    };
    for (frame, mut node) in &mut frames {
        if frame.orientation == ControlOrientation::Horizontal && node.flex_direction != wanted {
            node.flex_direction = wanted;
        }
    }
}

/// Show each container's bar exactly while the container overflows on the
/// bar's axis and is not itself hidden. (A windowed list's bar is
/// `virtual_list`'s to show.)
///
/// "Itself hidden" is the container's own `Visibility`, not the inherited
/// one: a tab page that is not the selected one is `Hidden` directly, and
/// reading only that keeps this meaning the same in a headless world with no
/// visibility pass.
fn hide_idle_scrollbars(
    containers: Query<(&ComputedNode, Option<&Visibility>), Without<ScrollbarFrame>>,
    mut frames: Query<(&ScrollbarFrame, &mut Visibility)>,
) {
    for (frame, mut visibility) in &mut frames {
        let ScrollTarget::Container(container) = frame.target else {
            continue;
        };
        let Ok((computed, own)) = containers.get(container) else {
            continue;
        };
        let hidden = own.is_some_and(|own| *own == Visibility::Hidden);
        let wanted = if !hidden && axis_range(computed, frame.orientation) > 0.5 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CONTAINER_STEP, axis_range, step_container};
    use bevy::prelude::*;
    use bevy::ui_widgets::ControlOrientation::{Horizontal, Vertical};
    use pretty_assertions::assert_eq;

    /// A container step is clamped to the real range at both ends — the far
    /// end too, because `bevy_ui` leaves the stored offset as written — and
    /// the range is the one of the axis asked about.
    #[expect(
        clippy::float_cmp,
        reason = "the clamp returns its exact bounds or an exact sum"
    )]
    #[test]
    fn a_container_step_stays_inside_the_range() {
        // A 200x100 page over 260x250 of content: 60 across, 150 down.
        let computed = ComputedNode {
            size: Vec2::new(200.0, 100.0),
            content_size: Vec2::new(260.0, 250.0),
            ..ComputedNode::DEFAULT
        };
        assert_eq!(axis_range(&computed, Horizontal), 60.0);
        assert_eq!(axis_range(&computed, Vertical), 150.0);
        let range = axis_range(&computed, Vertical);
        assert_eq!(step_container(0.0, -CONTAINER_STEP, range), 0.0);
        assert_eq!(step_container(0.0, CONTAINER_STEP, range), CONTAINER_STEP);
        assert_eq!(step_container(145.0, CONTAINER_STEP, range), 150.0);
        // Content that fits has no range to step through at all.
        let fits = ComputedNode {
            size: Vec2::new(200.0, 100.0),
            content_size: Vec2::new(200.0, 80.0),
            ..ComputedNode::DEFAULT
        };
        assert_eq!(axis_range(&fits, Vertical), 0.0);
        assert_eq!(step_container(0.0, CONTAINER_STEP, 0.0), 0.0);
    }
}
