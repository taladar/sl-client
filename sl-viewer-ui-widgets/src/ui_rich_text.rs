//! The **rich-text field** (`viewer-notecard-inline-items`): a multi-line text
//! field whose flow can carry **objects** — a whole `bevy_ui` subtree laid out
//! *in* the text, not beside it — and whose byte ranges can be **styled**
//! independently of the rest of the buffer.
//!
//! This is the widget a notecard needs. A notecard is Linden text plus embedded
//! inventory items: a landmark, an object, another notecard sits *inside* a
//! sentence as a clickable icon-and-name box, and the prose around it linkifies.
//! Reading one as a column of per-line rows works ([`crate::ui_text_input`] and
//! the old notecard reader did exactly that) right up to the moment the resident
//! wants to type, because a caret belongs to one laid-out buffer and cannot walk
//! a row of sibling nodes.
//!
//! # How an object gets into the flow
//!
//! `bevy_text`'s [`EditableText`] is `parley::PlainEditor`, which lays its whole
//! buffer out in one style and knows nothing of boxes. Parley's layout *builder*
//! has modelled both for a long time — an [`InlineBox`] reserves width and height
//! at a byte offset, and a ranged style scopes a property to a byte range — the
//! plain editor simply owns its builder and exposes neither. The workspace's
//! parley fork adds `PlainEditor::set_inline_boxes` / `set_range_styles` (see the
//! root `Cargo.toml`), and this widget is what drives them:
//!
//! - the consumer hands over a [`RichTextContent`]: which entities sit at which
//!   byte offsets, and which ranges wear which style;
//! - `sync_rich_text_model` measures each object entity and gives parley one
//!   box per object, so the text **flows around** them and wraps between them;
//! - `place_rich_text_objects` reads the finished layout back and moves each
//!   object entity to where parley put its box.
//!
//! The object entities are **siblings** of the field, never children of it: a
//! `bevy_ui` node's intrinsic size comes from a `ContentSize` measure, taffy only
//! measures childless nodes, and a field with children would silently lose the
//! "eighteen lines tall" that sizes the window it lives in. So
//! [`spawn_rich_text`] returns three entities — the `root` the field sizes, the
//! `field` that holds the text, and the `overlay` the objects are drawn and
//! clipped in.
//!
//! # How a range gets a colour
//!
//! A glyph's colour comes from the entity its brush's *section index* names in
//! the field's [`ComputedTextBlock`], which the text pipeline fills in from a
//! `Text` block's spans. An editable field has no spans, so this widget spawns
//! one inert node per [`RichTextClass`] and names those itself (the workspace's
//! Bevy fork adds `ComputedTextBlock::set_entities` for precisely this). A class
//! is therefore a real `TextColor` — and, if it asks for one, a real
//! [`Underline`] — resolved by the renderer exactly like a text span's.
//!
//! [`RichTextStyle::Hidden`] is the other half of the same mechanism, and it
//! takes **two** properties over a range: a zero font size, so the characters
//! keep their place in the buffer (the cursor still steps over them, a backspace
//! still deletes them, a save still sees them) while taking no advance, and a
//! transparent section of the field's own, so they have no visible glyph
//! either. That is what makes an embedded item's marker code point
//! invisible *underneath* the box drawn for it.
//!
//! The second half is not belt-and-braces. A zero font size takes the advance
//! away but the glyph is still emitted, and what its quad samples at a
//! degenerate size is the rasteriser's business — with the size alone, the
//! notecard editor and reader each drew a few hundred pixels of raw glyph atlas
//! beside the body (`viewer-notecard-body-draws-a-giant-artifact`). *Hidden*
//! has to mean invisible on its own terms.
//!
//! # Read-only
//!
//! A field the resident may not modify still wants a caret, a selection and a
//! copy — the reference's no-modify notecard is readable and copyable, just not
//! writable. That stance is the text field's own
//! ([`crate::ui_text_input::ReadOnlyField`], which refuses the edits that would
//! *change* a buffer and keeps every motion and selection edit), so
//! [`RichTextSpec::read_only`] simply asks for it rather than growing a second
//! mechanism beside it.
//!
//! Reference (Firestorm, read-only): `llviewertexteditor` (the embedded-item
//! segments), `lltextbase` (segment styling and hit testing).

use core::ops::Range;

use bevy::prelude::*;
use bevy::text::{ComputedTextBlock, EditableText, EditableTextSystems, TextBrush, TextEntity};
use bevy::ui::{UiSystems, widget::TextScroll};
use parley::{Cursor, InlineBox, InlineBoxKind, PositionedLayoutItem, StyleProperty};

use sl_viewer_ui_core::ui_element::{ContentMayOverflow, TextMayClip};

use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The width an object's box reserves before the object has ever been laid out,
/// as a multiple of the field's font size. The real width arrives on the next
/// frame from the object's own `ComputedNode`; this only keeps the first frame
/// from collapsing the text onto a zero-width box.
const UNMEASURED_OBJECT_WIDTH: f32 = 4.0;

/// The height an object's box reserves before it has been laid out, as a
/// multiple of the field's font size.
const UNMEASURED_OBJECT_HEIGHT: f32 = 1.2;

/// The narrowest a rich-text field is laid out at, in logical pixels — and, in
/// a container that offers no width of its own, the width it opens at.
///
/// A multi-line field has an intrinsic *height* (`visible_lines`) and no
/// intrinsic width at all: its measure takes the width it is offered, and a
/// container that offers none — a content-sized panel measuring its children —
/// leaves it as wide as its own padding. Plain text survives that (illegibly);
/// an object cannot, because its box is a real node that then hangs out of a
/// field narrower than one item. So a rich-text field declares a floor, the way
/// a single-line field declares a width in glyph advances: an intrinsic control
/// size, the sanctioned exception to the content-driven convention.
const MIN_FIELD_WIDTH: f32 = 360.0;

// ---------------------------------------------------------------------------
// The model a consumer hands over.
// ---------------------------------------------------------------------------

/// One style class a [`RichTextRange`] can name: a colour, and whether the
/// renderer underlines the run.
///
/// A class is a *class*, not a span: a notecard has one "link" class however
/// many links its body holds, and a script editor one class per token kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RichTextClass {
    /// The colour glyphs in this class are drawn in.
    pub color: Color,
    /// Whether runs in this class are underlined.
    pub underline: bool,
    /// Whether a press inside a range of this class raises
    /// [`RichTextRangeActivated`] — what makes a link a link.
    pub clickable: bool,
}

/// How one byte range of the buffer is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RichTextStyle {
    /// Drawn in the [`RichTextClass`] at this index of
    /// [`RichTextField::classes`]. An index no class answers is drawn plainly.
    Class(usize),
    /// Not drawn at all, and taking no room: the characters stay in the buffer
    /// and in every edit, but have no glyph and no advance. This is how an
    /// object's marker code point hides under the object drawn for it.
    Hidden,
}

/// A styled byte range of the field's buffer.
///
/// Offsets are the consumer's to keep current: an edit moves the text under
/// them, so they are normally recomputed from the buffer whenever it changes. A
/// range that lands out of bounds or inside a character is dropped rather than
/// breaking the layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichTextRange {
    /// The byte range of the buffer this styles.
    pub range: Range<usize>,
    /// How that range is drawn.
    pub style: RichTextStyle,
}

/// One object laid out in the flow of the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RichTextObject {
    /// The entity drawn for it — a child of [`RichTextField::overlay`], whose
    /// size the widget measures and whose position it sets.
    pub entity: Entity,
    /// The byte offset of the buffer the object sits at. The text before it
    /// flows up to the box and the text after it resumes on the far side.
    pub index: usize,
}

/// What a rich-text field lays out beyond its plain buffer: the objects in the
/// flow and the styled ranges over it.
///
/// The consumer owns this and rewrites it whenever the buffer changes. Rewriting
/// it with the same content is free — the widget compares before it invalidates
/// the layout.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct RichTextContent {
    /// The objects in the flow, in any order (parley orders them by offset).
    pub objects: Vec<RichTextObject>,
    /// The styled ranges, in any order.
    pub ranges: Vec<RichTextRange>,
}

/// A rich-text field's configuration, on the field entity itself.
#[derive(Component, Debug, Clone)]
pub struct RichTextField {
    /// The field's parent, which the field sizes.
    pub root: Entity,
    /// The clipping overlay the objects are positioned within.
    pub overlay: Entity,
    /// The style classes [`RichTextStyle::Class`] indexes.
    pub classes: Vec<RichTextClass>,
}

/// The inert nodes a field's brush sections resolve against, one per class.
///
/// Section 0 is the field's own [`TextColor`] (the default brush `bevy_ui` gives
/// every editable field), so a class at index `i` is section `i + 1`.
#[derive(Component, Debug, Clone, Default)]
struct RichTextSections {
    /// The section entities, section 0 first.
    entities: Vec<Entity>,
}

/// A section node's back-pointer to the field it colours.
///
/// It exists because naming sections changes **where a press lands**. `bevy_ui`
/// resolves a press on a text node to the *section* the glyph under the pointer
/// belongs to — that is how a `Text` block's individual spans are pickable — so
/// a field with sections hands its presses to these inert nodes instead of to
/// itself. They are therefore spawned as children of the field, so the press
/// bubbles to it (which is what keeps the caret working, since `bevy_ui_widgets`
/// places it from an observer on the field), and they carry this so a handler
/// that sees the section first can resolve the field without walking anything.
#[derive(Component, Debug, Clone, Copy)]
struct RichTextSectionOf {
    /// The field whose text this section colours.
    field: Entity,
}

/// Raised when a press lands inside a range whose class is
/// [`clickable`](RichTextClass::clickable) — a link in the prose being clicked.
///
/// The range is reported verbatim, so a consumer matches it against the model it
/// handed over rather than hit-testing anything itself.
#[derive(Message, Debug, Clone)]
pub struct RichTextRangeActivated {
    /// The field the press landed in.
    pub field: Entity,
    /// The byte range that was hit.
    pub range: Range<usize>,
    /// The class that range wears.
    pub class: usize,
}

// ---------------------------------------------------------------------------
// Spawning.
// ---------------------------------------------------------------------------

/// Everything a rich-text field is built from.
#[derive(Debug, Clone)]
pub struct RichTextSpec {
    /// The prefix of the field's node [`Name`], for the gallery and a test's
    /// lookups.
    pub element: &'static str,
    /// The text the field starts with.
    pub initial: String,
    /// The field's focus stop.
    pub tab_index: i32,
    /// The text's font size, in logical pixels.
    pub font_size: f32,
    /// The field's height, in visible text lines.
    pub visible_lines: f32,
    /// The narrowest the field is laid out at, in logical pixels — its
    /// intrinsic control width (see `MIN_FIELD_WIDTH`).
    pub min_width: f32,
    /// Whether the field **grows to fill** the space its container has spare,
    /// instead of standing at its declared lines and reading measure. What makes
    /// a body grow with the window around it; see
    /// [`crate::ui_text_input::TextInputSpec::fill`].
    pub fill: bool,
    /// Whether the buffer is protected from modification.
    pub read_only: bool,
    /// Whether the field draws its own border and background.
    pub decorated: bool,
    /// The style classes the content's ranges name.
    pub classes: Vec<RichTextClass>,
}

/// The query a field's section sync reads, named so the widget's systems stay
/// legible under the workspace's type-complexity lint.
type SectionFields<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        &'static RichTextField,
        &'static mut RichTextSections,
        &'static mut ComputedTextBlock,
    ),
    Changed<RichTextField>,
>;

impl RichTextSpec {
    /// A spec for `element` with no classes, an empty buffer and the text
    /// field's own defaults. Override the rest with struct-update syntax.
    #[must_use]
    pub const fn new(element: &'static str) -> Self {
        Self {
            element,
            initial: String::new(),
            tab_index: 0,
            font_size: 14.0,
            visible_lines: 8.0,
            min_width: MIN_FIELD_WIDTH,
            fill: false,
            read_only: false,
            decorated: true,
            classes: Vec::new(),
        }
    }
}

/// The three entities a rich-text field is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RichTextHandle {
    /// The outer node: the field's parent, which the field sizes.
    pub root: Entity,
    /// The field itself — the [`EditableText`] node whose buffer is the text.
    pub field: Entity,
    /// The overlay the objects are drawn in — the parent every object entity
    /// must be spawned under ([`spawn_rich_text_object`] takes it).
    pub overlay: Entity,
}

/// Spawn a rich-text field under `parent`.
///
/// Three nodes, and the split is load-bearing. The **root** hugs the field and
/// does not clip: `overflow: clip` zeroes a box's automatic minimum size (CSS,
/// and taffy with it), so a clipping root stops being pushed out by the field
/// inside it and collapses to whatever its own parent allots — in a
/// content-sized panel, to its padding, with the field hanging out of it. The
/// **overlay** does the clipping instead, and can afford to: it is absolutely
/// positioned at inset zero, so it fills the root's padding box exactly while
/// contributing nothing to its size. An object on a line the field has scrolled
/// past is then cut at the field's edge, the way the glyphs on that line are.
pub fn spawn_rich_text(
    commands: &mut Commands,
    parent: Entity,
    spec: &RichTextSpec,
) -> RichTextHandle {
    let root = commands
        .spawn((
            Node {
                // A **column**, so the field stretches across the root the way
                // it stretched across the panel it used to sit in directly. In
                // a row it would be a main-axis item instead, sized by its own
                // content measure — and a multi-line field measured against no
                // definite width comes out as wide as its padding.
                flex_direction: FlexDirection::Column,
                // The field's intrinsic width floor — see `MIN_FIELD_WIDTH`.
                min_width: Val::Px(spec.min_width),
                // A filling field's root has to grow too, or the field has
                // nothing to grow into: the root is the child a panel hands its
                // spare height to.
                flex_grow: if spec.fill { 1.0 } else { 0.0 },
                ..default()
            },
            Name::new(format!("{}:rich-text", spec.element)),
            ChildOf(parent),
        ))
        .id();
    let field = spawn_text_input(
        commands,
        root,
        &TextInputSpec {
            initial: spec.initial.clone(),
            font_size: spec.font_size,
            visible_lines: spec.visible_lines,
            tab_index: spec.tab_index,
            decorated: spec.decorated,
            fill: spec.fill,
            read_only: spec.read_only,
            ..TextInputSpec::new(spec.element, TextInputKind::Multiline)
        },
    );

    let overlay = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                overflow: Overflow::clip(),
                ..default()
            },
            // Spawned **after** the field, which is what puts it on top of it:
            // `bevy_ui` stacks siblings in tree order, and the field paints a
            // background — an overlay spawned first is painted over, and its
            // objects are invisible and unclickable while looking, in every
            // layout assertion, perfectly placed.
            //
            // On top, it must never take the pointer itself: a press belongs to
            // the text underneath, or to an object, which opts back in.
            Pickable::IGNORE,
            TextMayClip {
                reason: "a rich-text field scrolls its text to follow the caret, so an object on \
                         a line it has scrolled past is cut at the field's edge by design",
            },
            ContentMayOverflow {
                reason: "the objects are absolutely positioned by the text engine, not by taffy: \
                         where they sit is decided by where parley put their boxes in the \
                         buffer, so this node's content size answers a question nobody asked",
            },
            Name::new(format!("{}:rich-text-overlay", spec.element)),
            ChildOf(root),
        ))
        .id();
    commands.entity(field).insert((
        RichTextField {
            root,
            overlay,
            classes: spec.classes.clone(),
        },
        RichTextContent::default(),
        RichTextSections::default(),
    ));
    RichTextHandle {
        root,
        field,
        overlay,
    }
}

/// Spawn one object node under a field's `overlay`, returning it.
///
/// The node is absolutely positioned and starts **hidden rather than
/// undisplayed**, which is the whole trick: an object's box has to be the size
/// of the object, `bevy_ui` only measures a node it lays out, and a
/// `Display::None` node is never laid out — so hiding one that way would freeze
/// its box at zero and it would never appear at all. Hidden-but-laid-out, it is
/// measured on the frame it is spawned and shown on the frame parley places it.
///
/// Fill it with whatever the object should look like, and name it in a
/// [`RichTextObject`].
pub fn spawn_rich_text_object(commands: &mut Commands, overlay: Entity) -> Entity {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            ChildOf(overlay),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Sections: one inert node per class, named in the field's text block.
// ---------------------------------------------------------------------------

/// Keep each field's section nodes in step with its classes, and tell the
/// field's [`ComputedTextBlock`] about them.
///
/// **Section 0 is the field itself.** `bevy_ui` gives an editable field a
/// default brush of section 0, so section 0 is every unstyled glyph in the
/// buffer — and naming a section decides more than a colour: the renderer
/// resolves a press on a text node to the *section* the glyph under the pointer
/// belongs to, so a separate node for section 0 would take every press on
/// ordinary prose away from the field and the caret would never be placed. The
/// field carries its own [`TextColor`] already, which is exactly what section 0
/// should be drawn in, so it names itself and a press on prose still lands on
/// it.
///
/// The class sections are spawned nodes, and a press on a *styled* range does
/// land on one — deliberately: that is the press
/// [`dispatch_rich_text_range_clicks`] turns into an activation, and it resolves
/// the field back through [`RichTextSectionOf`].
fn sync_rich_text_sections(mut fields: SectionFields, mut commands: Commands) {
    for (field, config, mut sections, mut block) in &mut fields {
        for entity in sections.entities.drain(..) {
            if entity != field {
                commands.entity(entity).despawn();
            }
        }
        sections.entities.push(field);
        for class in &config.classes {
            let mut section = commands.spawn((
                // An inert node rather than a bare entity: it hangs off the UI
                // tree (so it dies with the field) and a UI parent's children
                // are expected to be nodes. Under the **root**, never under the
                // field: a node with children loses the `ContentSize` measure
                // that gives a field its "N lines tall", and the window around
                // it is sized by that.
                Node {
                    display: Display::None,
                    ..default()
                },
                TextColor(class.color),
                RichTextSectionOf { field },
                ChildOf(config.root),
            ));
            if class.underline {
                section.insert(Underline);
            }
            sections.entities.push(section.id());
        }
        // **The hidden section, last and always present.** A hidden range is
        // given a zero font size, which is what takes its advance away — but a
        // zero-size glyph is still a glyph the renderer emits a quad for, and
        // what that quad samples at zero size is not defined by anything here.
        // A transparent section makes "hidden" mean invisible in its own right,
        // rather than resting on a rasteriser's behaviour at a degenerate size.
        // Always spawned, even for a field with no classes, so the index is a
        // function of the class count alone.
        let hidden = commands.spawn((
            Node {
                display: Display::None,
                ..default()
            },
            TextColor(Color::NONE),
            RichTextSectionOf { field },
            ChildOf(config.root),
        ));
        sections.entities.push(hidden.id());
        // The renderer reads the section list off the block; nothing else on an
        // editable field writes it, so this is the whole handshake.
        block.set_entities(sections.entities.iter().map(|entity| TextEntity {
            entity: *entity,
            depth: 1,
            font_smoothing: FontSmoothing::default(),
        }));
    }
}

// ---------------------------------------------------------------------------
// The model → parley.
// ---------------------------------------------------------------------------

/// Give parley this frame's inline boxes and range styles.
///
/// Runs after the frame's edits are applied (so the offsets the consumer
/// computed are against the buffer parley is about to lay out) and before the
/// editable-text layout, so a box added this frame is in this frame's text.
///
/// Both setters compare before they invalidate, so handing over an unchanged
/// model costs two comparisons and no relayout — which is what lets the
/// consumer rewrite the model on every keystroke without thinking about it.
fn sync_rich_text_model(
    mut fields: Query<(
        &RichTextField,
        &RichTextContent,
        &TextFont,
        &mut EditableText,
    )>,
    objects: Query<&ComputedNode>,
) {
    for (config, content, font, mut editable) in &mut fields {
        // The editor tracks its own layout invalidation, and nothing gates on
        // this component having changed, so the model is handed over without
        // marking the field changed every frame.
        let editor = &mut editable.bypass_change_detection().editor;
        let font_size = editor.get_font_size();
        let mut boxes = Vec::with_capacity(content.objects.len());
        for (index, object) in content.objects.iter().enumerate() {
            // A node spawned this frame has no box yet; a placeholder keeps the
            // text from closing over a zero-width hole for one frame.
            let measured = objects
                .get(object.entity)
                .ok()
                .map(ComputedNode::size)
                .filter(|size| size.x > 0.0 && size.y > 0.0);
            let size = measured.unwrap_or_else(|| {
                Vec2::new(
                    font_size * UNMEASURED_OBJECT_WIDTH,
                    font_size * UNMEASURED_OBJECT_HEIGHT,
                )
            });
            boxes.push(InlineBox {
                id: u64::try_from(index).unwrap_or(u64::MAX),
                kind: InlineBoxKind::InFlow,
                index: object.index,
                width: size.x,
                height: size.y,
            });
        }
        editor.set_inline_boxes(boxes);

        let smoothing = font.font_smoothing;
        let styles = content
            .ranges
            .iter()
            .flat_map(|styled| {
                let range = styled.range.clone();
                match styled.style {
                    // **Two properties, not one.** The zero font size is what
                    // takes the character's advance away; the transparent brush
                    // is what makes it invisible. Relying on the size alone left
                    // a zero-size glyph for the renderer to emit a quad for, and
                    // what that quad samples is a rasteriser's business at a
                    // degenerate size — not something a widget should bet a
                    // blank screen area on.
                    RichTextStyle::Hidden => vec![
                        (range.clone(), StyleProperty::FontSize(0.0)),
                        (
                            range,
                            StyleProperty::Brush(TextBrush::new(
                                hidden_section_index(config.classes.len()),
                                smoothing,
                            )),
                        ),
                    ],
                    RichTextStyle::Class(class) => vec![(
                        range,
                        StyleProperty::Brush(TextBrush::new(
                            section_index(class, config.classes.len()),
                            smoothing,
                        )),
                    )],
                }
            })
            .collect();
        editor.set_range_styles(styles);
    }
}

/// The brush section index a class maps to: section 0 is the field's own
/// colour, so class `i` is section `i + 1`. A class no field declares falls back
/// to section 0 (the field's colour) rather than to an index nothing answers.
fn section_index(class: usize, declared: usize) -> u32 {
    if class >= declared {
        return 0;
    }
    u32::try_from(class.saturating_add(1)).unwrap_or(0)
}

/// The brush section index of the **transparent** section every field carries
/// after its classes — what a [`RichTextStyle::Hidden`] range is drawn in.
///
/// One past the last class, because `sync_rich_text_sections` appends it after
/// the class loop and before the handshake. Falls back to section 0 (the field's
/// own colour) only if the count does not fit a `u32`, which no real field
/// reaches; a hidden range would then be visible rather than addressing a
/// section nothing answers.
fn hidden_section_index(declared: usize) -> u32 {
    u32::try_from(declared.saturating_add(1)).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The finished layout → the object nodes.
// ---------------------------------------------------------------------------

/// Where an object's node goes on its line, given the box parley placed
/// (`box_top`), the line's `baseline` and the ascent of the line's *text*.
///
/// Parley puts an inline box's **bottom on the baseline**, which is right for a
/// box of arbitrary content — and wrong for this one, because an object here is
/// a pill with *text in it*. Bottom-aligned, that text sits a descent above the
/// prose around it: the pill reads as floating, which is exactly what it looks
/// like. Aligning the pill's **top** to the line's text ascent instead puts its
/// own first baseline on the line's baseline, so the item's name sits on the
/// same line as the words either side of it — what the reference draws, an
/// embedded item being a text segment there.
///
/// A line with no text (an item on a line of its own) has no ascent to align
/// to, and keeps parley's placement.
fn object_top(box_top: f32, baseline: f32, text_ascent: f32) -> f32 {
    if text_ascent > 0.0 {
        baseline - text_ascent
    } else {
        box_top
    }
}

/// Move each object entity to the box parley laid out for it.
///
/// Runs after the editable-text layout and its scroll, so the positions are this
/// frame's; the `Node` writes land in the next frame's layout, which is one
/// frame of lag on a position that only moves when the text does.
///
/// An object whose box was not placed — its marker deleted, its offset stale for
/// a frame — is hidden rather than left at its last position, so a stale object
/// never sits over live text.
fn place_rich_text_objects(
    fields: Query<(
        &RichTextField,
        &RichTextContent,
        &EditableText,
        &ComputedNode,
        &UiGlobalTransform,
        &TextScroll,
    )>,
    roots: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut nodes: Query<(&mut Node, &mut Visibility)>,
) {
    for (config, content, editable, node, transform, scroll) in &fields {
        let Some(layout) = editable.editor.try_layout() else {
            continue;
        };
        let Ok((overlay_node, overlay_transform)) = roots.get(config.overlay) else {
            continue;
        };
        let Some(overlay_inverse) = overlay_transform.try_inverse() else {
            continue;
        };
        let inverse_scale = node.inverse_scale_factor();
        // Where the text's own origin sits, in the field's local space — the
        // same transform `bevy_ui` draws the glyphs through. Spelled out per
        // component because `glam`'s whole-`Vec2` operators trip the workspace's
        // `arithmetic_side_effects` lint.
        let text_box = node.content_box().min;
        let text_origin = Vec2::new(text_box.x - scroll.0.x, text_box.y - scroll.0.y);
        let overlay_origin = overlay_node.padding_box().min;

        let mut placed: Vec<Option<Vec2>> = vec![None; content.objects.len()];
        for line in layout.lines() {
            // Where the line's *text* sits, which is what an object is aligned
            // to — see [`object_top`]. Taken from a glyph run rather than the
            // line metrics, because a line's ascent is grown by a tall inline
            // box and would then describe the box instead of the text.
            let text_ascent = line
                .items()
                .filter_map(|item| match item {
                    PositionedLayoutItem::GlyphRun(run) => Some(run.run().metrics().ascent),
                    PositionedLayoutItem::InlineBox(_box) => None,
                })
                .fold(0.0_f32, f32::max);
            let baseline = line.metrics().baseline;
            for item in line.items() {
                let PositionedLayoutItem::InlineBox(inline_box) = item else {
                    continue;
                };
                let Ok(index) = usize::try_from(inline_box.id) else {
                    continue;
                };
                if let Some(slot) = placed.get_mut(index) {
                    *slot = Some(Vec2::new(
                        inline_box.x,
                        object_top(inline_box.y, baseline, text_ascent),
                    ));
                }
            }
        }

        for (object, position) in content.objects.iter().zip(placed) {
            let Ok((mut object_node, mut visibility)) = nodes.get_mut(object.entity) else {
                continue;
            };
            let Some(position) = position else {
                // Hidden rather than undisplayed: an object still has to be laid
                // out to be measured, and a box whose marker comes back needs
                // its size ready.
                visibility.set_if_neq(Visibility::Hidden);
                continue;
            };
            // Parley's coordinates are in the field's text space; the object is
            // a child of the root, so the point makes the trip through the
            // world and back down.
            let in_field = Vec2::new(text_origin.x + position.x, text_origin.y + position.y);
            let in_world = transform.affine().transform_point2(in_field);
            let in_overlay = overlay_inverse.transform_point2(in_world);
            let left = Val::Px((in_overlay.x - overlay_origin.x) * inverse_scale);
            let top = Val::Px((in_overlay.y - overlay_origin.y) * inverse_scale);
            visibility.set_if_neq(Visibility::Inherited);
            if object_node.left != left || object_node.top != top {
                object_node.left = left;
                object_node.top = top;
                object_node.position_type = PositionType::Absolute;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Read-only, and clicking a styled range.
// ---------------------------------------------------------------------------

/// Raise [`RichTextRangeActivated`] when a press lands inside a clickable range.
///
/// The press is mapped to a byte offset through the same transform `bevy_ui`
/// maps a caret click through, so "the character under the pointer" means the
/// same thing here as it does to the editor.
fn dispatch_rich_text_range_clicks(
    press: On<Pointer<Press>>,
    fields: Query<(
        &RichTextField,
        &RichTextContent,
        &EditableText,
        &ComputedNode,
        &UiGlobalTransform,
        &ComputedUiRenderTargetInfo,
        &TextScroll,
    )>,
    sections: Query<&RichTextSectionOf>,
    ui_scale: Res<UiScale>,
    mut activated: MessageWriter<RichTextRangeActivated>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    // A press on a styled range is resolved by `bevy_ui` to that range's section
    // node, so the field is found through it when the press lands there first.
    let pressed = sections
        .get(press.entity)
        .map_or(press.entity, |section| section.field);
    let Ok((config, content, editable, node, transform, target, scroll)) = fields.get(pressed)
    else {
        return;
    };
    let Some(layout) = editable.editor.try_layout() else {
        return;
    };
    if ui_scale.0 <= 0.0 {
        return;
    }
    let Some(inverse) = transform.try_inverse() else {
        return;
    };
    // The same trip `bevy_ui_widgets` makes to turn a press into a caret
    // position, spelled out per component for the workspace's arithmetic lint.
    let pointer = press.pointer_location.position;
    let ui_pointer = Vec2::new(
        pointer.x * target.scale_factor() / ui_scale.0,
        pointer.y * target.scale_factor() / ui_scale.0,
    );
    let in_field = inverse.transform_point2(ui_pointer);
    let text_box = node.content_box().min;
    let index = Cursor::from_point(
        layout,
        in_field.x - text_box.x + scroll.0.x,
        in_field.y - text_box.y + scroll.0.y,
    )
    .index();
    for styled in &content.ranges {
        let RichTextStyle::Class(class) = styled.style else {
            continue;
        };
        if !config
            .classes
            .get(class)
            .is_some_and(|declared| declared.clickable)
        {
            continue;
        }
        if styled.range.contains(&index) {
            activated.write(RichTextRangeActivated {
                field: pressed,
                range: styled.range.clone(),
                class,
            });
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin owning the rich-text field's model sync, placement and guards.
#[derive(Debug, Clone, Copy, Default)]
pub struct RichTextPlugin;

impl Plugin for RichTextPlugin {
    /// Register the activation message, the systems, and the observer that maps
    /// a press into it.
    fn build(&self, app: &mut App) {
        // The field this is built on is the text-input widget's, and so are the
        // stances it can be put in: the caret, the overwrite mode, and the
        // read-only filter that keeps a no-modify body from being typed into.
        // Brought in here rather than left to the consumer, because a consumer
        // that forgets it gets a rich-text field that looks right and quietly
        // accepts every keystroke.
        if !app.is_plugin_added::<crate::ui_text_input::TextInputPlugin>() {
            app.add_plugins(crate::ui_text_input::TextInputPlugin);
        }
        app.add_message::<RichTextRangeActivated>()
            .add_systems(
                PostUpdate,
                (
                    sync_rich_text_sections,
                    // After the frame's edits, so the consumer's offsets and the
                    // buffer parley lays out are the same text; before the
                    // layout, so this frame's boxes are in this frame's flow.
                    sync_rich_text_model
                        .after(EditableTextSystems)
                        .before(UiSystems::PostLayout),
                    // After the layout *and* its scroll, so the positions read
                    // back are final.
                    place_rich_text_objects.after(UiSystems::PostLayout),
                )
                    .chain(),
            )
            .add_observer(dispatch_rich_text_range_clicks);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RichTextClass, RichTextContent, RichTextHandle, RichTextObject, RichTextPlugin,
        RichTextRange, RichTextRangeActivated, RichTextSpec, RichTextStyle, spawn_rich_text,
        spawn_rich_text_object,
    };
    use crate::ui_test::interact::{InteractionTest, click, focus, type_str};
    use crate::ui_test::{drain, record, settle, spawn_under_root};
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use bevy::ui::ComputedStackIndex;
    use pretty_assertions::assert_eq;

    /// A boxed error so a test can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The private-use code point the fixtures use where an object sits — the
    /// same shape a notecard's embedded-item marker has.
    const MARKER: char = '\u{f800}';

    /// The size every fixture object is pinned to, in logical pixels, so the
    /// boxes parley reserves are known rather than measured from a font.
    const OBJECT_SIZE: Vec2 = Vec2::new(60.0, 16.0);

    /// The container a fixture field is spawned in: a column of a definite
    /// width, like the content slot of a floater.
    fn fixture_column() -> Node {
        Node {
            width: Val::Px(420.0),
            flex_direction: FlexDirection::Column,
            ..default()
        }
    }

    /// An app with the interaction stack, the rich-text plugin and a field
    /// holding `text`, with `classes` declared. Returns the app and the handle.
    fn field_app(text: &str, classes: Vec<RichTextClass>) -> (App, RichTextHandle) {
        let mut app = InteractionTest::new().build();
        app.add_plugins(RichTextPlugin);
        record::<RichTextRangeActivated>(&mut app);
        settle(&mut app);
        // A column with a definite width, which is what a floater's content
        // slot is: a multi-line field's measure takes the width it is given, so
        // a shrink-wrapping parent would collapse it and every box with it.
        let root = spawn_under_root(&mut app, fixture_column());
        let mut commands = app.world_mut().commands();
        let handle = spawn_rich_text(
            &mut commands,
            root,
            &RichTextSpec {
                initial: text.to_owned(),
                visible_lines: 6.0,
                classes,
                ..RichTextSpec::new("rich-text-test")
            },
        );
        app.world_mut().flush();
        settle(&mut app);
        (app, handle)
    }

    /// Spawn one fixed-size object node under the field's root and name it in
    /// the field's content at `index`, with the marker there hidden.
    fn add_object(app: &mut App, handle: RichTextHandle, index: usize) -> Entity {
        let object = {
            let mut commands = app.world_mut().commands();
            let object = spawn_rich_text_object(&mut commands, handle.root);
            commands.entity(object).insert(Node {
                position_type: PositionType::Absolute,
                width: Val::Px(OBJECT_SIZE.x),
                height: Val::Px(OBJECT_SIZE.y),
                ..default()
            });
            object
        };
        app.world_mut().flush();
        let marker_len = MARKER.len_utf8();
        let mut content = app
            .world_mut()
            .get_mut::<RichTextContent>(handle.field)
            .ok_or("the field lost its content")
            .map_or_else(
                |_missing| RichTextContent::default(),
                |content| content.clone(),
            );
        content.objects.push(RichTextObject {
            entity: object,
            index,
        });
        content.ranges.push(RichTextRange {
            range: index..index.saturating_add(marker_len),
            style: RichTextStyle::Hidden,
        });
        app.world_mut().entity_mut(handle.field).insert(content);
        settle(app);
        settle(app);
        object
    }

    /// The object's laid-out box in world coordinates, if it has one.
    fn object_box(app: &App, object: Entity) -> Option<Rect> {
        let world = app.world();
        let computed = world.get::<ComputedNode>(object)?;
        let transform = world.get::<UiGlobalTransform>(object)?;
        Some(crate::ui_test::border_box(computed, transform))
    }

    /// **A filling field grows with the window around it.**
    ///
    /// The complaint this answers is "making the floater bigger does not make
    /// the body bigger": an editor's body is the part of its window worth
    /// enlarging, and a floater hands its content slot a definite size once it
    /// has one. A field that fills takes the leftover; one that does not stands
    /// at its declared lines however much room it is given.
    #[test]
    fn a_filling_field_takes_the_room_it_is_given() -> Result<(), TestError> {
        /// The height the fixture panel is given, far more than six lines.
        const PANEL_HEIGHT: f32 = 600.0;

        let size_of = |fill: bool, panel: Node| -> Option<Vec2> {
            let mut app = InteractionTest::new().build();
            app.add_plugins(RichTextPlugin);
            settle(&mut app);
            let panel = spawn_under_root(&mut app, panel);
            let handle = {
                let mut commands = app.world_mut().commands();
                spawn_rich_text(
                    &mut commands,
                    panel,
                    &RichTextSpec {
                        initial: "a line".to_owned(),
                        visible_lines: 6.0,
                        fill,
                        ..RichTextSpec::new("rich-text-fill")
                    },
                )
            };
            app.world_mut().flush();
            settle(&mut app);
            settle(&mut app);
            app.world()
                .get::<ComputedNode>(handle.field)
                .map(ComputedNode::size)
        };

        let sized = || Node {
            width: Val::Px(420.0),
            height: Val::Px(PANEL_HEIGHT),
            flex_direction: FlexDirection::Column,
            ..default()
        };
        let filling = size_of(true, sized()).ok_or("the filling field has no box")?;
        let standing = size_of(false, sized()).ok_or("the standing field has no box")?;
        assert!(
            filling.y > standing.y,
            "a filling field ({filling:?}) did not grow past its declared lines \
             ({standing:?})"
        );
        assert!(
            filling.y > PANEL_HEIGHT / 2.0,
            "a filling field ({filling:?}) took only a fraction of its panel \
             ({PANEL_HEIGHT})"
        );
        assert!(
            standing.y < PANEL_HEIGHT / 2.0,
            "a field that does not fill ({standing:?}) grew anyway"
        );
        assert!(
            filling.x > standing.x,
            "a filling field ({filling:?}) did not take its panel's width \
             ({standing:?})"
        );

        // **And an unsized panel must not make it take the screen.** A filling
        // field has no wrapping bound, so without an intrinsic width its measure
        // answers "whatever is available" — which in a content-driven window is
        // the display, and the window opens across it.
        let unsized_panel = size_of(
            true,
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
        )
        .ok_or("the unsized field has no box")?;
        assert!(
            unsized_panel.x < 600.0,
            "a filling field in a panel with no width took {unsized_panel:?}"
        );
        Ok(())
    }

    /// The overlay covers the field: it is what clips an object to the field's
    /// box, so any gap between the two is a place an object is cut short (or
    /// draws where the text cannot).
    #[test]
    fn the_overlay_covers_the_field() -> Result<(), TestError> {
        let text = format!("hello {MARKER} world");
        let (mut app, handle) = field_app(&text, Vec::new());
        let index = text.find(MARKER).ok_or("no marker in the fixture")?;
        let _object = add_object(&mut app, handle, index);
        let boxes = |app: &App, entity: Entity| -> Option<Rect> {
            let world = app.world();
            Some(crate::ui_test::border_box(
                world.get::<ComputedNode>(entity)?,
                world.get::<UiGlobalTransform>(entity)?,
            ))
        };
        let field = boxes(&app, handle.field).ok_or("the field has no box")?;
        let overlay = boxes(&app, handle.overlay).ok_or("the overlay has no box")?;
        assert!(
            overlay.min.y <= field.min.y && overlay.max.y >= field.max.y,
            "the overlay {overlay:?} does not cover the field {field:?} vertically"
        );
        assert!(
            overlay.min.x <= field.min.x && overlay.max.x >= field.max.x,
            "the overlay {overlay:?} does not cover the field {field:?} horizontally"
        );
        Ok(())
    }

    /// An object on a line **with** text is aligned to that text's baseline,
    /// not bottom-aligned to it the way parley places a box: the pill has text
    /// in it, and bottom-aligned that text floats a descent above the prose.
    /// An object alone on a line keeps parley's placement, there being no text
    /// to align to.
    #[test]
    fn an_object_sits_on_the_line_it_shares() {
        /// How near a placement has to be to count, in physical pixels.
        const TOLERANCE: f32 = 0.001;
        // A 20-tall box on a line whose text ascends 11 above a baseline at 15:
        // parley bottom-aligns it (top at -5), we align its own baseline (4).
        assert!((super::object_top(-5.0, 15.0, 11.0) - 4.0).abs() < TOLERANCE);
        // No text on the line: parley's placement stands.
        assert!((super::object_top(-5.0, 15.0, 0.0) + 5.0).abs() < TOLERANCE);
    }

    /// **A hidden range has a transparent section to be drawn in.**
    ///
    /// Hiding a range used to be a zero font size and nothing else, on the
    /// reasoning that a zero-size glyph has no advance *and* no glyph. Only the
    /// first half is true: the quad is still emitted, and at a degenerate size
    /// what it samples is the rasteriser's business — in the viewer it came out
    /// as a few hundred pixels of raw glyph atlas beside a notecard's body,
    /// white in the editor and grey with letter shapes in the reader
    /// (`viewer-notecard-body-draws-a-giant-artifact`).
    ///
    /// Nothing about the *layout* is wrong in that state, which is why a
    /// geometric test cannot see it and a headless dump of the tree finds
    /// nothing oversized: the shape is not a node. What this pins instead is the
    /// mechanism — the field's section list ends with a transparent section, at
    /// the index [`super::hidden_section_index`] hands to a hidden range's
    /// brush — because that is the part a future edit could quietly drop.
    #[test]
    fn a_hidden_range_is_drawn_in_a_transparent_section() -> Result<(), TestError> {
        let classes = vec![
            RichTextClass {
                color: Color::srgb(0.1, 0.2, 0.3),
                underline: true,
                clickable: true,
            },
            RichTextClass {
                color: Color::srgb(0.4, 0.5, 0.6),
                underline: false,
                clickable: false,
            },
        ];
        let declared = classes.len();
        let (app, handle) = field_app("one two", classes);

        let sections = app
            .world()
            .get::<super::RichTextSections>(handle.field)
            .ok_or("the field has no section list")?
            .entities
            .clone();
        // The field itself, one node per class, then the hidden one.
        assert_eq!(
            sections.len(),
            declared.saturating_add(2),
            "the section list is not field + classes + hidden"
        );
        assert_eq!(
            sections.first().copied(),
            Some(handle.field),
            "section 0 must stay the field itself, or a press on prose stops \
             placing the caret"
        );
        let index = usize::try_from(super::hidden_section_index(declared))
            .map_err(|_error| "the hidden section index does not fit a usize")?;
        let hidden = sections
            .get(index)
            .copied()
            .ok_or("the hidden section index is past the end of the list")?;
        assert_eq!(
            app.world().get::<TextColor>(hidden).map(|color| color.0),
            Some(Color::NONE),
            "the section a hidden range is drawn in must be transparent — a \
             zero font size alone leaves a glyph for the renderer to draw"
        );
        Ok(())
    }

    /// **The overlay draws over the field, not under it.** `bevy_ui` stacks
    /// siblings in tree order, and a field paints a background: an overlay
    /// spawned before the field is painted over, so every object in it is
    /// invisible and unclickable — while every geometric assertion about it
    /// still passes, because it is placed exactly where it should be. Found in
    /// the viewer, by a notecard whose items left a hole the right size and
    /// nothing in it.
    #[test]
    fn the_overlay_draws_over_the_field() -> Result<(), TestError> {
        let (app, handle) = field_app("hello", Vec::new());
        let world = app.world();
        let field = world
            .get::<ComputedStackIndex>(handle.field)
            .ok_or("the field has no stack index")?;
        let overlay = world
            .get::<ComputedStackIndex>(handle.overlay)
            .ok_or("the overlay has no stack index")?;
        let (overlay, field) = (overlay.0, field.0);
        assert!(
            overlay > field,
            "the overlay (stack {overlay}) is painted under the field (stack {field})"
        );
        Ok(())
    }

    /// An object named in the content is placed inside the field, on the line
    /// its marker sits on — the whole point of the widget.
    #[test]
    fn an_object_is_placed_inside_the_field() -> Result<(), TestError> {
        let text = format!("hello {MARKER} world");
        let (mut app, handle) = field_app(&text, Vec::new());
        let index = text.find(MARKER).ok_or("no marker in the fixture")?;
        let object = add_object(&mut app, handle, index);

        let field = {
            let world = app.world();
            let computed = world
                .get::<ComputedNode>(handle.field)
                .ok_or("the field was never laid out")?;
            let transform = world
                .get::<UiGlobalTransform>(handle.field)
                .ok_or("the field has no transform")?;
            crate::ui_test::border_box(computed, transform)
        };
        let placed = object_box(&app, object).ok_or("the object was never laid out")?;
        assert_eq!(
            app.world().get::<Visibility>(object).copied(),
            Some(Visibility::Inherited),
            "an object whose box parley placed should be shown"
        );
        assert!(
            placed.min.x >= field.min.x && placed.max.x <= field.max.x,
            "the object {placed:?} should sit within the field {field:?}"
        );
        assert!(
            placed.min.y >= field.min.y && placed.max.y <= field.max.y,
            "the object {placed:?} should sit within the field {field:?}"
        );
        assert!(
            placed.min.x > field.min.x,
            "the object should follow the text before it, not sit at the margin"
        );
        Ok(())
    }

    /// Two objects on one line are placed in buffer order, each clear of the
    /// other — the box parley reserved for the first is really reserved.
    #[test]
    fn two_objects_are_placed_in_order() -> Result<(), TestError> {
        let text = format!("a{MARKER}b{MARKER}c");
        let (mut app, handle) = field_app(&text, Vec::new());
        let first_index = text.find(MARKER).ok_or("no marker in the fixture")?;
        let second_index = text
            .rfind(MARKER)
            .ok_or("the fixture should hold two markers")?;
        let first = add_object(&mut app, handle, first_index);
        let second = add_object(&mut app, handle, second_index);

        let first_box = object_box(&app, first).ok_or("the first object has no box")?;
        let second_box = object_box(&app, second).ok_or("the second object has no box")?;
        assert!(
            first_box.max.x <= second_box.min.x,
            "the objects overlap: {first_box:?} and {second_box:?}"
        );
        Ok(())
    }

    /// An object whose marker the resident deletes is hidden rather than left
    /// floating over the text it no longer belongs to.
    #[test]
    fn an_object_with_no_box_is_hidden() -> Result<(), TestError> {
        let text = format!("x{MARKER}");
        let (mut app, handle) = field_app(&text, Vec::new());
        let index = text.find(MARKER).ok_or("no marker in the fixture")?;
        let object = add_object(&mut app, handle, index);
        assert_eq!(
            app.world().get::<Visibility>(object).copied(),
            Some(Visibility::Inherited)
        );

        // The marker is gone from the buffer, and so is the model that named it
        // — what the consumer does when it recomputes from the edited text.
        if let Some(mut editable) = app.world_mut().get_mut::<EditableText>(handle.field) {
            editable.editor.set_text("x");
        }
        app.world_mut()
            .entity_mut(handle.field)
            .insert(RichTextContent {
                objects: vec![RichTextObject {
                    entity: object,
                    // Past the end of the shortened buffer: parley drops it.
                    index: 99,
                }],
                ranges: Vec::new(),
            });
        settle(&mut app);
        settle(&mut app);
        assert_eq!(
            app.world().get::<Visibility>(object).copied(),
            Some(Visibility::Hidden),
            "an unplaced object should be hidden"
        );
        Ok(())
    }

    /// A read-only field refuses the keystrokes that would change its buffer
    /// while staying a document: the caret still moves and the text is still
    /// there to select and copy.
    #[test]
    fn a_read_only_field_refuses_typing() -> Result<(), TestError> {
        let mut app = InteractionTest::new().build();
        app.add_plugins(RichTextPlugin);
        settle(&mut app);
        let root = spawn_under_root(&mut app, fixture_column());
        let handle = {
            let mut commands = app.world_mut().commands();
            spawn_rich_text(
                &mut commands,
                root,
                &RichTextSpec {
                    initial: "read me".to_owned(),
                    read_only: true,
                    ..RichTextSpec::new("rich-text-readonly")
                },
            )
        };
        app.world_mut().flush();
        settle(&mut app);

        focus(&mut app, handle.field);
        type_str(&mut app, "nope");
        settle(&mut app);
        let value = app
            .world()
            .get::<EditableText>(handle.field)
            .ok_or("the field lost its editor")?
            .value()
            .to_string();
        assert_eq!(value, "read me", "a read-only field took a keystroke");
        Ok(())
    }

    /// A press inside a clickable range raises the activation, naming the range
    /// the consumer handed over — how a link in the prose is clicked.
    #[test]
    fn a_press_in_a_clickable_range_activates_it() -> Result<(), TestError> {
        let link = "https://example.com";
        let text = format!("see {link} now");
        let (mut app, handle) = field_app(
            &text,
            vec![RichTextClass {
                color: Color::srgb(0.4, 0.6, 1.0),
                underline: true,
                clickable: true,
            }],
        );
        let start = text.find(link).ok_or("no link in the fixture")?;
        let range = start..start.saturating_add(link.len());
        app.world_mut()
            .entity_mut(handle.field)
            .insert(RichTextContent {
                objects: Vec::new(),
                ranges: vec![RichTextRange {
                    range: range.clone(),
                    style: RichTextStyle::Class(0),
                }],
            });
        settle(&mut app);

        // A point on the **first** line, past the four characters of "see " —
        // the field is six lines tall, so its centre is below the text and would
        // clamp to the end of the buffer.
        let field_box = {
            let world = app.world();
            let computed = world
                .get::<ComputedNode>(handle.field)
                .ok_or("the field was never laid out")?;
            let transform = world
                .get::<UiGlobalTransform>(handle.field)
                .ok_or("the field has no transform")?;
            crate::ui_test::border_box(computed, transform)
        };
        let inside_the_link = Vec2::new(field_box.min.x + 60.0, field_box.min.y + 10.0);
        click(&mut app, inside_the_link, MouseButton::Left);
        settle(&mut app);
        let activations: Vec<RichTextRangeActivated> = drain(&mut app);
        let hit = activations
            .first()
            .ok_or("a press inside the link raised nothing")?;
        assert_eq!(hit.field, handle.field);
        assert_eq!(hit.class, 0);
        assert_eq!(hit.range, range);
        Ok(())
    }

    /// **A press on ordinary prose still places the caret** — the guarantee
    /// naming brush sections could quietly take away.
    ///
    /// `bevy_ui` resolves a press on a text node to the section the glyph under
    /// the pointer belongs to, and `bevy_ui_widgets` places the caret from an
    /// observer on the *field*. A separate node for section 0 would therefore
    /// swallow every press on unstyled text and the field would never focus, so
    /// section 0 is the field itself.
    #[test]
    fn a_press_on_prose_still_focuses_the_field() -> Result<(), TestError> {
        let (mut app, handle) = field_app(
            "see https://example.com now",
            vec![RichTextClass {
                color: Color::srgb(0.4, 0.6, 1.0),
                underline: true,
                clickable: true,
            }],
        );
        let field_box = {
            let world = app.world();
            let computed = world
                .get::<ComputedNode>(handle.field)
                .ok_or("the field was never laid out")?;
            let transform = world
                .get::<UiGlobalTransform>(handle.field)
                .ok_or("the field has no transform")?;
            crate::ui_test::border_box(computed, transform)
        };
        // Inside "see ", before the link starts.
        let on_the_prose = Vec2::new(field_box.min.x + 10.0, field_box.min.y + 10.0);
        click(&mut app, on_the_prose, MouseButton::Left);
        settle(&mut app);
        assert_eq!(
            app.world()
                .resource::<bevy::input_focus::InputFocus>()
                .get(),
            Some(handle.field),
            "a press on the field's own text did not reach the field"
        );
        Ok(())
    }
}
