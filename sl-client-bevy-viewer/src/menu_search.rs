//! The viewer's **menu search** (`viewer-ui-menu-search`): a small text field in
//! the top menu bar, just to the trailing side of the last menu, that finds a
//! command *within the menus* — type a few letters and opening a menu shows only
//! the entries whose label matches, the way the reference viewer's status-bar
//! filter does.
//!
//! # A filter over the menu tree, drawn by the menus themselves
//!
//! The field owns only its text. Each frame [`sync_menu_filter`] mirrors that
//! text into [`crate::menu`]'s [`MenuFilter`] resource, and the menu widget does
//! the rest: while the filter is active, a menu's drop-down shows only the
//! matching entries (drawn highlighted), hides the rest, keeps a submenu whose
//! subtree carries a match, and shows a whole menu whose own label matched — the
//! reference's `LLStatusBar` / `hightlightAndHide`. Typing also **auto-opens**
//! the first bar menu that has a match (`crate::menu`'s `open_filtered_menu`), so
//! there is no separate results surface to keep in step: the menus *are* the
//! results, and hovering another top menu opens its filtered drop-down too.
//!
//! # In the bar, not a corner
//!
//! The field is spawned as the last child of the menu-bar row
//! ([`crate::menu_bar`] calls [`spawn_menu_search_field`]), so it sits
//! immediately after the final menu button and reflows with the bar under a
//! larger font or a longer translation (convention 2), rather than floating at
//! the window's trailing edge.
//!
//! Reference (Firestorm, read-only): `indra/newview/llstatusbar.{h,cpp}`
//! (`mFilterEdit`, `onUpdateFilterTerm`, `hightlightAndHide`),
//! `panel_status_bar.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};
use bevy_flair::style::components::ClassList;

use crate::menu::MenuFilter;
use crate::menu_bar::TOP_MENU_ELEMENT;
use crate::ui_font::UiFont;

/// The search field's background.
const FIELD_BACKGROUND: Color = Color::srgb(0.10, 0.12, 0.16);

/// The search field's border.
const FIELD_BORDER: Color = Color::srgb(0.30, 0.36, 0.46);

/// The typed-text colour.
const TEXT_COLOR: Color = Color::srgb(0.92, 0.94, 0.98);

/// The field's font size, in logical pixels — matched to the menu bar's entries.
const SEARCH_FONT: f32 = 15.0;

/// The field's least width, in logical pixels, so an empty field is a real
/// target rather than collapsing to its (empty) content.
const FIELD_MIN_WIDTH: f32 = 140.0;

/// The clear button's diameter, in logical pixels.
const CLEAR_SIZE: f32 = 16.0;

/// The clear button's circle colour.
const CLEAR_BACKGROUND: Color = Color::srgb(0.34, 0.40, 0.52);

/// The `×` glyph the clear button draws (U+00D7), against its circle.
const CLEAR_GLYPH: &str = "\u{00d7}";

/// The clear button's glyph size, in logical pixels.
const CLEAR_FONT: f32 = 12.0;

/// The search field, marking the [`EditableText`] whose text drives the filter.
#[derive(Component)]
struct MenuSearchField;

/// The clear (`×`) button, shown only while the field holds a term.
#[derive(Component)]
struct MenuSearchClear;

/// The menu-search widget's runtime — the field that drives [`MenuFilter`].
///
/// Registers the filter resource (also inited by [`crate::menu`]'s
/// `MenuWidgetPlugin`; `init_resource` is idempotent, so declaring it here too
/// lets the widget and its tests stand alone) and the two per-frame systems. The
/// field itself is spawned into the bar by [`crate::menu_bar`], via
/// [`spawn_menu_search_field`].
pub(crate) struct MenuSearchPlugin;

impl Plugin for MenuSearchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuFilter>().add_systems(
            Update,
            (
                sync_menu_filter,
                clear_menu_search_on_escape,
                toggle_clear_button,
            ),
        );
    }
}

/// Spawn the search box as a child of `parent` (the menu-bar row), and return the
/// box entity. It is a bordered container holding a single-line [`EditableText`]
/// (focused on click, `Tab`-reachable) and a circular `×` clear button that shows
/// only while the field holds a term — the reference viewer's search editor, in
/// the bar after the last menu.
pub(crate) fn spawn_menu_search_field(commands: &mut Commands, parent: Entity) -> Entity {
    let box_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                min_width: Val::Px(FIELD_MIN_WIDTH),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                ..default()
            },
            BorderColor::all(FIELD_BORDER),
            BackgroundColor(FIELD_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-search-field"]),
            Name::new("menu-search"),
            ChildOf(parent),
        ))
        .id();

    let mut field = EditableText::new("");
    field.allow_newlines = false;
    field.visible_lines = Some(1.0);
    commands
        .spawn((
            field,
            UiFont::Sans.at(SEARCH_FONT),
            TextColor(TEXT_COLOR),
            TextCursorStyle::default(),
            TabIndex(0),
            Node {
                // Grow to fill the box, so the clear button hugs the trailing edge.
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            },
            MenuSearchField,
            Name::new("menu-search-field"),
            ChildOf(box_entity),
        ))
        .observe(
            |mut press: On<Pointer<Press>>, mut focus: ResMut<InputFocus>| {
                // Consume the press so it does not reach the menu widget's root
                // dismiss observer — clicking the field to type must not close the
                // menu the term just opened (`crate::menu`).
                press.propagate(false);
                focus.set(press.entity, FocusCause::Navigated);
            },
        );

    let clear = commands
        .spawn((
            Node {
                width: Val::Px(CLEAR_SIZE),
                height: Val::Px(CLEAR_SIZE),
                display: Display::None,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BackgroundColor(CLEAR_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-search-clear"]),
            MenuSearchClear,
            Name::new("menu-search-clear"),
            ChildOf(box_entity),
        ))
        .id();
    commands.entity(clear).observe(
        move |mut press: On<Pointer<Press>>,
              mut fields: Query<(Entity, &mut EditableText), With<MenuSearchField>>,
              mut focus: ResMut<InputFocus>| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            if let Ok((entity, mut field)) = fields.single_mut() {
                field.clear();
                // Keep the caret in the field so the user can type a new term.
                focus.set(entity, FocusCause::Navigated);
            }
        },
    );
    commands.spawn((
        Text::new(CLEAR_GLYPH),
        UiFont::Sans.at(CLEAR_FONT),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        Name::new("menu-search-clear-glyph"),
        ChildOf(clear),
    ));

    box_entity
}

/// Mirror the field's live text into [`MenuFilter`], so opening a top menu while
/// a term is active filters its drop-down ([`crate::menu`]).
///
/// The field owns the text and the caret; this turns a change into the shared,
/// lower-cased filter the menu widget reads when it builds a popup. A change is
/// written through the resource's change detection only on a real difference.
fn sync_menu_filter(
    fields: Query<&EditableText, With<MenuSearchField>>,
    mut filter: ResMut<MenuFilter>,
) {
    let Ok(field) = fields.single() else {
        return;
    };
    let query = field.value().to_string().trim().to_lowercase();
    if filter.element != TOP_MENU_ELEMENT || filter.query != query {
        filter.element = TOP_MENU_ELEMENT;
        filter.query = query;
    }
}

/// Show the clear (`×`) button only while the field holds a term, the way the
/// reference viewer's search editor reveals its clear affordance.
fn toggle_clear_button(
    fields: Query<&EditableText, With<MenuSearchField>>,
    mut clears: Query<&mut Node, With<MenuSearchClear>>,
) {
    let has_text = fields
        .single()
        .is_ok_and(|field| !field.value().to_string().trim().is_empty());
    let Ok(mut node) = clears.single_mut() else {
        return;
    };
    let wanted = if has_text {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != wanted {
        node.display = wanted;
    }
}

/// Clear the search on `Escape` when it holds a term — so `Escape` cancels a
/// search (and, via [`crate::menu`], closes the menu it filtered) in one press.
///
/// Reads the raw keyboard rather than a focus-routed key event, and acts only
/// when there is something to clear, so a stray `Escape` elsewhere is a no-op.
fn clear_menu_search_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut fields: Query<&mut EditableText, With<MenuSearchField>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Ok(mut field) = fields.single_mut() else {
        return;
    };
    if !field.value().to_string().is_empty() {
        field.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{MenuSearchPlugin, spawn_menu_search_field};
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use pretty_assertions::assert_eq;

    use crate::menu::MenuFilter;
    use crate::menu_bar::TOP_MENU_ELEMENT;
    use crate::ui::{UiRoot, UiScaffoldSystems};
    use crate::ui_test::{LayoutTest, TestError, find_by_name, settle};

    /// Build a layout-test app with the widget's systems, the field spawned under
    /// the root, and the keyboard resource the layout harness omits.
    fn build_app() -> App {
        let mut app = LayoutTest::new().build();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(MenuSearchPlugin)
            .add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_menu_search_field(&mut commands, root.0);
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
        settle(&mut app);
        app
    }

    /// Set the search field's text directly, as typing into it would.
    fn set_field_text(app: &mut App, text: &str) {
        let mut fields = app.world_mut().query::<&mut EditableText>();
        for mut field in fields.iter_mut(app.world_mut()) {
            field.editor.set_text(text);
        }
    }

    /// The field's text lands in the shared [`MenuFilter`], lower-cased and under
    /// the top bar's element — what the menu widget reads to filter a drop-down.
    #[test]
    fn the_field_drives_the_menu_filter() -> Result<(), TestError> {
        let mut app = build_app();
        set_field_text(&mut app, "Fly");
        settle(&mut app);
        let filter = app.world().resource::<MenuFilter>();
        assert_eq!(
            filter.query, "fly",
            "the term is lower-cased into the filter"
        );
        assert_eq!(
            filter.element, TOP_MENU_ELEMENT,
            "the filter targets the top menu bar",
        );
        Ok(())
    }

    /// The clear (`×`) button is hidden on an empty field and shown once a term
    /// is typed, the way the reference viewer reveals its clear affordance.
    #[test]
    fn the_clear_button_appears_with_a_term() -> Result<(), TestError> {
        let mut app = build_app();
        let clear =
            find_by_name(&mut app, "menu-search-clear").ok_or("the clear button did not spawn")?;
        let display = |app: &App| {
            app.world()
                .entity(clear)
                .get::<Node>()
                .map(|node| node.display)
        };
        assert_eq!(
            display(&app).ok_or("the clear button lost its Node")?,
            Display::None,
            "the clear button is hidden while the field is empty",
        );
        set_field_text(&mut app, "fly");
        settle(&mut app);
        assert_eq!(
            display(&app).ok_or("the clear button lost its Node")?,
            Display::Flex,
            "the clear button appears once there is a term",
        );
        Ok(())
    }

    /// `Escape` clears an active search term.
    #[test]
    fn escape_clears_the_search() -> Result<(), TestError> {
        let mut app = build_app();
        set_field_text(&mut app, "fly");
        settle(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        settle(&mut app);
        let filter = app.world().resource::<MenuFilter>();
        assert!(filter.query.is_empty(), "the filter clears with the field");
        Ok(())
    }
}
