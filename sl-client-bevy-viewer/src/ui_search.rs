//! The **reusable search-field widget** (`viewer-ui-search-field`): a single-line
//! text field wrapped in the affordances every search box shares — a clear (`×`)
//! button shown only while the field holds a term, `Escape`-to-clear, an optional
//! placeholder shown while empty, and an optional leading search glyph.
//!
//! # One widget, many search boxes
//!
//! The reference viewer grows this same box independently everywhere it searches
//! — the menu filter, the inventory filter, people / groups / places finders —
//! (`llsearcheditor`, `llfiltereditor`). We write it once, on the single-line
//! [`crate::ui_text_input`] field: [`spawn_search_field`] composes that field
//! (bare, so the *box* carries the border and background) with the clear button,
//! placeholder and glyph, and returns a [`SearchFieldHandle`] naming the box and
//! the inner field.
//!
//! The two migrated consumers are [`crate::menu_search`] (the menu-bar filter) and
//! [`crate::inventory`] (the inventory filter); each spawns the widget, marks or
//! stores the returned field, and reacts to its text — the widget owns the input
//! chrome, the consumer owns what the term *means*.
//!
//! # Constructible without wiring
//!
//! Per the registry rule ([`crate::ui_element`]): the field holds and edits its
//! own text and reaches no session. A consumer reads the term via
//! [`EditableText::value`] / `Changed<EditableText>` and does its own filtering;
//! nothing here emits a [`crate::ui_element::UiAction`]. The clear button, the
//! placeholder and `Escape`-to-clear are the widget's, driven by
//! [`SearchFieldPlugin`]'s systems off the field's own value.
//!
//! Direction-neutral by construction: the box is a [`crate::ui::row`], so the
//! leading glyph and the trailing clear button swap ends under RTL with no code
//! here saying so (convention 1).
//!
//! Reference (Firestorm, read-only): `llsearcheditor`, `llfiltereditor` (the
//! search / filter line editors with their clear button and search icon).

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_flair::style::components::ClassList;

use crate::ui::row;
use crate::ui_element::TextMayClip;
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The skin class on the search box (the bordered container), so a skin can give
/// it the same control surface as the other editable fields.
const SEARCH_FIELD_CLASS: &str = "sk-search-field";

/// The skin class on the circular clear (`×`) button.
const SEARCH_CLEAR_CLASS: &str = "sk-search-clear";

/// The search box's border colour (a skin overrides it via [`SEARCH_FIELD_CLASS`]).
const BOX_BORDER: Color = Color::srgb(0.30, 0.36, 0.46);

/// The search box's background colour (a skin overrides it).
const BOX_BACKGROUND: Color = Color::srgb(0.10, 0.12, 0.16);

/// The default least width of the box, in logical pixels, so an empty field is a
/// real click target rather than collapsing to its (empty) content.
const DEFAULT_MIN_WIDTH: f32 = 140.0;

/// The default font size of the field's text and the glyphs, in logical pixels.
const DEFAULT_FONT_SIZE: f32 = 15.0;

/// The gap between the glyph, the field and the clear button, in logical pixels.
const INNER_GAP: f32 = 4.0;

/// The box's inner padding, in logical pixels: a little space inside the border.
const BOX_PADDING_X: f32 = 6.0;

/// The box's inner vertical padding, in logical pixels.
const BOX_PADDING_Y: f32 = 3.0;

/// The clear button's diameter, in logical pixels.
const CLEAR_SIZE: f32 = 16.0;

/// The clear button's circle colour (a skin overrides it via
/// [`SEARCH_CLEAR_CLASS`]).
const CLEAR_BACKGROUND: Color = Color::srgb(0.34, 0.40, 0.52);

/// The `×` glyph the clear button draws (U+00D7).
const CLEAR_GLYPH: &str = "\u{00d7}";

/// The clear button's glyph size, in logical pixels.
const CLEAR_FONT: f32 = 12.0;

/// The leading search glyph (🔍, U+1F50D), shown when
/// [`SearchFieldSpec::search_glyph`] is set.
const SEARCH_GLYPH: &str = "\u{1f50d}";

/// The inset of the field's text from its box, in logical pixels — matches
/// [`crate::ui_text_input`]'s field padding, so the placeholder overlay lands
/// exactly where the field's own text starts.
const FIELD_TEXT_INSET: f32 = 6.0;

/// The typed-text colour.
const TEXT_COLOR: Color = Color::srgb(0.92, 0.94, 0.98);

/// The placeholder and glyph colour — muted against the typed text, so the
/// placeholder reads as a prompt rather than as content.
const MUTED_COLOR: Color = Color::srgb(0.55, 0.60, 0.68);

/// A marker on the widget's inner [`EditableText`], so [`SearchFieldPlugin`]'s
/// generic systems (the clear-on-`Escape`) can find a *search* field among all
/// editable fields, and a consumer can tell the widget's field from any other.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SearchInputField;

/// The clear (`×`) button, naming the field it clears. Shown by
/// [`toggle_search_clear`] only while that field holds a term.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SearchClearButton {
    /// The field this button clears.
    field: Entity,
}

/// The placeholder text, naming the field it prompts for. Shown by
/// [`toggle_search_placeholder`] only while that field is empty.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SearchPlaceholder {
    /// The field this placeholder belongs to.
    field: Entity,
}

/// What [`spawn_search_field`] hands back: the box and the inner field, so a
/// consumer can parent siblings to the box, and mark / store / read the field.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SearchFieldHandle {
    /// The bordered container — the search *box*.
    pub(crate) container: Entity,
    /// The inner [`EditableText`], whose value is the search term.
    pub(crate) field: Entity,
}

/// Everything a search field is built from — a struct rather than a positional
/// call, matching the neighbouring widgets ([`crate::ui_tab::TabSpec`],
/// [`TextInputSpec`]). Build one with [`SearchFieldSpec::new`] and override with
/// struct-update syntax.
#[derive(Debug, Clone)]
pub(crate) struct SearchFieldSpec {
    /// The prefix of the widget's node [`Name`]s, for the gallery and lookups.
    pub(crate) element: &'static str,
    /// The field's focus stop, for slotting it into the surrounding tab order.
    pub(crate) tab_index: i32,
    /// The field text's font size, in logical pixels.
    pub(crate) font_size: f32,
    /// The box's least width, in logical pixels.
    pub(crate) min_width: f32,
    /// The prompt shown while the field is empty, or empty for none.
    pub(crate) placeholder: String,
    /// Whether to draw the leading 🔍 glyph.
    pub(crate) search_glyph: bool,
}

impl SearchFieldSpec {
    /// A spec for `element` with the module defaults: no placeholder, no glyph,
    /// the default size and least width. Override the rest with struct-update
    /// syntax.
    pub(crate) const fn new(element: &'static str) -> Self {
        Self {
            element,
            tab_index: 0,
            font_size: DEFAULT_FONT_SIZE,
            min_width: DEFAULT_MIN_WIDTH,
            placeholder: String::new(),
            search_glyph: false,
        }
    }
}

/// Spawn a search field under `parent`, returning the box and inner field
/// ([`SearchFieldHandle`]).
///
/// The box is a bordered [`crate::ui::row`] holding an optional leading glyph, the
/// bare single-line field (which fills the middle and scrolls), and a trailing
/// clear button — so the glyph and the button mirror ends under RTL for free. The
/// field carries [`SearchInputField`]; the clear button and placeholder are driven
/// by [`SearchFieldPlugin`] off the field's value.
pub(crate) fn spawn_search_field(
    commands: &mut Commands,
    parent: Entity,
    spec: &SearchFieldSpec,
) -> SearchFieldHandle {
    let container = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                min_width: Val::Px(spec.min_width),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::axes(Val::Px(BOX_PADDING_X), Val::Px(BOX_PADDING_Y)),
                ..row(Val::Px(INNER_GAP))
            },
            BorderColor::all(BOX_BORDER),
            BackgroundColor(BOX_BACKGROUND),
            ClassList::new_with_classes([SEARCH_FIELD_CLASS]),
            Name::new(format!("{}:search", spec.element)),
            ChildOf(parent),
        ))
        .id();

    if spec.search_glyph {
        commands.spawn((
            Text::new(SEARCH_GLYPH),
            UiFont::Sans.at(spec.font_size),
            TextColor(MUTED_COLOR),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            Name::new(format!("{}:search-glyph", spec.element)),
            ChildOf(container),
        ));
    }

    // The field slot: it fills the middle between glyph and clear button, and
    // positions the placeholder overlay. It clips **both** axes — the inline so the
    // field's scrolling text and an over-long placeholder are cut here rather than
    // escaping the box, and the block so the absolute placeholder (which taffy
    // would otherwise add to the slot's content height) does not report a height
    // taller than the field. A clipped node reports its own box to its parent, so
    // the box around it stays sized to the field.
    let slot = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                align_items: AlignItems::Center,
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                ..default()
            },
            Name::new(format!("{}:search-slot", spec.element)),
            ChildOf(container),
        ))
        .id();

    let field = spawn_text_input(
        commands,
        slot,
        &TextInputSpec {
            tab_index: spec.tab_index,
            font_size: spec.font_size,
            // Bare (the box carries the chrome) and filling (the box's width, not
            // the text, decides the field's).
            decorated: false,
            fill: true,
            ..TextInputSpec::new(spec.element, TextInputKind::Line)
        },
    );
    commands
        .entity(field)
        .insert((SearchInputField, TextColor(TEXT_COLOR)));

    if !spec.placeholder.is_empty() {
        commands.spawn((
            Text::new(spec.placeholder.clone()),
            // One line, clipped by the slot when longer than the box — never
            // wrapped tall, which (as an absolute node with no width bound) it
            // otherwise would.
            TextLayout::no_wrap(),
            UiFont::Sans.at(spec.font_size),
            TextColor(MUTED_COLOR),
            Node {
                position_type: PositionType::Absolute,
                // Aligned with the field's text origin: both sit one field-text
                // inset in from their box's top-left.
                left: Val::Px(FIELD_TEXT_INSET),
                top: Val::Px(FIELD_TEXT_INSET),
                ..default()
            },
            // The placeholder is decorative and may be clipped by the slot when it
            // is longer than the box — a declaration, so the clipping check knows.
            TextMayClip {
                reason: "a search field's placeholder is a prompt clipped to the box width, like \
                         the field's own scrolling text",
            },
            Pickable::IGNORE,
            SearchPlaceholder { field },
            Name::new(format!("{}:search-placeholder", spec.element)),
            ChildOf(slot),
        ));
    }

    spawn_clear_button(commands, container, field, spec.element);

    SearchFieldHandle { container, field }
}

/// Spawn the trailing clear (`×`) button, hidden until the field holds a term, and
/// wire its click to clear the field and keep the caret in it.
fn spawn_clear_button(
    commands: &mut Commands,
    container: Entity,
    field: Entity,
    element: &'static str,
) {
    let clear = commands
        .spawn((
            Node {
                width: Val::Px(CLEAR_SIZE),
                height: Val::Px(CLEAR_SIZE),
                // Hidden while the field is empty; `toggle_search_clear` reveals it.
                display: Display::None,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BackgroundColor(CLEAR_BACKGROUND),
            ClassList::new_with_classes([SEARCH_CLEAR_CLASS]),
            SearchClearButton { field },
            Name::new(format!("{element}:search-clear")),
            ChildOf(container),
        ))
        .id();
    commands.spawn((
        Text::new(CLEAR_GLYPH),
        UiFont::Sans.at(CLEAR_FONT),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        Name::new(format!("{element}:search-clear-glyph")),
        ChildOf(clear),
    ));
    commands.entity(clear).observe(
        move |mut press: On<Pointer<Press>>,
              mut fields: Query<&mut EditableText>,
              mut focus: ResMut<InputFocus>| {
            // Consume the press so it does not reach an ancestor's dismiss observer
            // (the menu bar closes its drop-down on an outside press — clearing the
            // field must not close the menu the term just opened).
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            if let Ok(mut field_text) = fields.get_mut(field) {
                field_text.clear();
                // Keep the caret in the field so the user can type a new term.
                focus.set(field, FocusCause::Navigated);
            }
        },
    );
}

/// The plugin for the search-field widget's runtime: the clear-button and
/// placeholder visibility, and clear-on-`Escape`.
///
/// Each system is a no-op where there are no search fields, so adding it is always
/// safe — the viewer and the gallery both add it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SearchFieldPlugin;

impl Plugin for SearchFieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                toggle_search_clear,
                toggle_search_placeholder,
                clear_focused_search_on_escape,
            ),
        );
    }
}

/// Show a search field's clear (`×`) button only while its field holds a
/// non-blank term, the way the reference viewer reveals its clear affordance.
fn toggle_search_clear(
    fields: Query<&EditableText>,
    mut buttons: Query<(&SearchClearButton, &mut Node)>,
) {
    for (button, mut node) in &mut buttons {
        let has_term = fields
            .get(button.field)
            .is_ok_and(|field| !field.value().to_string().trim().is_empty());
        let wanted = if has_term {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Show a search field's placeholder only while its field is empty.
fn toggle_search_placeholder(
    fields: Query<&EditableText>,
    mut placeholders: Query<(&SearchPlaceholder, &mut Node)>,
) {
    for (placeholder, mut node) in &mut placeholders {
        let empty = fields
            .get(placeholder.field)
            .map_or(true, |field| field.value().to_string().is_empty());
        let wanted = if empty { Display::Flex } else { Display::None };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Clear the **focused** search field on `Escape` when it holds a term — so
/// `Escape` cancels a search in one press (and lets a consumer, e.g. the menu,
/// react to the cleared term).
///
/// Scoped to the focused field, so with more than one search box on screen only
/// the one being typed into is cleared; a stray `Escape` elsewhere is a no-op.
fn clear_focused_search_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut fields: Query<&mut EditableText, With<SearchInputField>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    let Ok(mut field) = fields.get_mut(focused) else {
        return;
    };
    if !field.value().to_string().is_empty() {
        field.clear();
    }
}

/// Spawn the gallery specimen: a search field with the leading glyph, so the box,
/// the glyph and the (initially hidden) clear button are swept by
/// [`crate::ui_test`] across every script, direction, scale and font size.
///
/// The placeholder is left off the specimen deliberately: it is an *absolute*
/// overlay, which taffy folds into the slot's `content_size` in a way the
/// content-overflow harness reads as an overflow even though the box is sized
/// correctly and the overlay is clipped. Its behaviour is covered by this module's
/// unit tests and by the two live consumers (menu / inventory) instead.
pub(crate) fn spawn_search_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    spawn_search_field(
        commands,
        parent,
        &SearchFieldSpec {
            font_size: cx.font_size,
            search_glyph: true,
            ..SearchFieldSpec::new("search-field")
        },
    )
    .container
}

#[cfg(test)]
mod tests {
    use super::{SearchFieldPlugin, SearchFieldSpec, SearchInputField, spawn_search_field};
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use pretty_assertions::assert_eq;

    use crate::ui::{UiRoot, UiScaffoldSystems};
    use crate::ui_test::{LayoutTest, TestError, find_by_name, settle};

    /// Build a layout-test app with the widget's systems and one search field
    /// (with a placeholder) spawned under the root, plus the keyboard resource the
    /// layout harness omits.
    fn build_app() -> App {
        let mut app = LayoutTest::new().build();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(SearchFieldPlugin)
            .add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_search_field(
                        &mut commands,
                        root.0,
                        &SearchFieldSpec {
                            placeholder: "Search".to_owned(),
                            search_glyph: true,
                            ..SearchFieldSpec::new("test-search")
                        },
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
        settle(&mut app);
        app
    }

    /// Set the one search field's text directly, as typing into it would.
    fn set_field_text(app: &mut App, text: &str) {
        let mut fields = app
            .world_mut()
            .query_filtered::<&mut EditableText, With<SearchInputField>>();
        for mut field in fields.iter_mut(app.world_mut()) {
            field.editor.set_text(text);
        }
    }

    /// The clear (`×`) button is hidden on an empty field and shown once a term is
    /// typed; the placeholder does the opposite.
    #[test]
    fn clear_and_placeholder_track_the_term() -> Result<(), TestError> {
        let mut app = build_app();
        let clear = find_by_name(&mut app, "test-search:search-clear")
            .ok_or("the clear button did not spawn")?;
        let placeholder = find_by_name(&mut app, "test-search:search-placeholder")
            .ok_or("the placeholder did not spawn")?;
        let display =
            |app: &App, entity: Entity| app.world().entity(entity).get::<Node>().map(|n| n.display);

        assert_eq!(
            display(&app, clear).ok_or("clear lost its Node")?,
            Display::None,
            "clear is hidden while empty",
        );
        assert_eq!(
            display(&app, placeholder).ok_or("placeholder lost its Node")?,
            Display::Flex,
            "placeholder shows while empty",
        );

        set_field_text(&mut app, "boots");
        settle(&mut app);

        assert_eq!(
            display(&app, clear).ok_or("clear lost its Node")?,
            Display::Flex,
            "clear appears with a term",
        );
        assert_eq!(
            display(&app, placeholder).ok_or("placeholder lost its Node")?,
            Display::None,
            "placeholder hides with a term",
        );
        Ok(())
    }

    /// `Escape` clears the focused search field's term.
    #[test]
    fn escape_clears_the_focused_field() -> Result<(), TestError> {
        use bevy::input_focus::InputFocus;
        let mut app = build_app();
        set_field_text(&mut app, "boots");
        settle(&mut app);
        // Focus the field, then press Escape.
        let field = find_by_name(&mut app, "test-search:field").ok_or("the field did not spawn")?;
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, bevy::input_focus::FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        settle(&mut app);

        let value = app
            .world()
            .entity(field)
            .get::<EditableText>()
            .ok_or("the field lost its EditableText")?
            .value()
            .to_string();
        assert!(value.is_empty(), "Escape clears the focused field's term");
        Ok(())
    }
}
