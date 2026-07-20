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
//! # Built on the reusable search-field widget
//!
//! The box itself — the bordered single-line field, the `×` clear button, the
//! placeholder, and clear-on-`Escape` — is the reusable [`crate::ui_search`]
//! widget, so the menu filter and the inventory filter are one widget in two
//! places rather than two hand-rolled boxes. This module keeps only what is
//! *menu-specific*: mirroring the term into [`MenuFilter`], and consuming the
//! field's own pointer press so clicking it to type does not reach the menu
//! widget's outside-press dismiss (which would close the menu the term just
//! opened).
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

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;

use crate::menu::MenuFilter;
use crate::menu_bar::TOP_MENU_ELEMENT;
use crate::ui_search::{SearchFieldSpec, spawn_search_field};

/// The field's font size, in logical pixels — matched to the menu bar's entries.
const SEARCH_FONT: f32 = 15.0;

/// The box's least width, in logical pixels, so an empty field is a real target
/// rather than collapsing to its (empty) content.
const FIELD_MIN_WIDTH: f32 = 140.0;

/// The placeholder shown in the empty menu-search box.
const PLACEHOLDER: &str = "Search menus";

/// Marks the menu search's [`EditableText`], so [`sync_menu_filter`] can find this
/// widget's field among all the viewer's search fields.
#[derive(Component)]
struct MenuSearchField;

/// The menu-search widget's runtime — mirror the field into [`MenuFilter`].
///
/// Registers the filter resource (also inited by [`crate::menu`]'s
/// `MenuWidgetPlugin`; `init_resource` is idempotent, so declaring it here too
/// lets the widget and its tests stand alone) and the one menu-specific system.
/// The box's own chrome — the clear button, placeholder and clear-on-`Escape` —
/// belongs to [`crate::ui_search::SearchFieldPlugin`]. The field itself is spawned
/// into the bar by [`crate::menu_bar`], via [`spawn_menu_search_field`].
pub(crate) struct MenuSearchPlugin;

impl Plugin for MenuSearchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuFilter>()
            .add_systems(Update, sync_menu_filter);
    }
}

/// Spawn the menu search box as a child of `parent` (the menu-bar row), and return
/// the box entity — the reusable [`crate::ui_search`] widget, marked as *this*
/// widget's field and taught to swallow its own press so the menu it filters stays
/// open.
pub(crate) fn spawn_menu_search_field(commands: &mut Commands, parent: Entity) -> Entity {
    let handle = spawn_search_field(
        commands,
        parent,
        &SearchFieldSpec {
            font_size: SEARCH_FONT,
            min_width: FIELD_MIN_WIDTH,
            placeholder: PLACEHOLDER.to_owned(),
            ..SearchFieldSpec::new("menu")
        },
    );
    commands
        .entity(handle.field)
        .insert(MenuSearchField)
        .observe(
            |mut press: On<Pointer<Press>>, mut focus: ResMut<InputFocus>| {
                // Consume the press so it does not reach the menu widget's root
                // dismiss observer — clicking the field to type must not close the
                // menu the term just opened (`crate::menu`).
                press.propagate(false);
                focus.set(press.entity, FocusCause::Navigated);
            },
        );
    handle.container
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

#[cfg(test)]
mod tests {
    use super::{MenuSearchField, MenuSearchPlugin, spawn_menu_search_field};
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use pretty_assertions::assert_eq;

    use crate::menu::MenuFilter;
    use crate::menu_bar::TOP_MENU_ELEMENT;
    use crate::ui::{UiRoot, UiScaffoldSystems};
    use crate::ui_search::SearchFieldPlugin;
    use crate::ui_test::{LayoutTest, TestError, find_by_name, settle};

    /// Build a layout-test app with the menu-search system, the shared search-field
    /// widget's systems, the field spawned under the root, and the keyboard
    /// resource the layout harness omits.
    fn build_app() -> App {
        let mut app = LayoutTest::new().build();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_plugins((MenuSearchPlugin, SearchFieldPlugin))
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

    /// Set the menu search field's text directly, as typing into it would.
    fn set_field_text(app: &mut App, text: &str) {
        let mut fields = app
            .world_mut()
            .query_filtered::<&mut EditableText, With<MenuSearchField>>();
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

    /// The clear (`×`) button — the widget's — is hidden on an empty field and
    /// shown once a term is typed.
    #[test]
    fn the_clear_button_appears_with_a_term() -> Result<(), TestError> {
        let mut app = build_app();
        let clear =
            find_by_name(&mut app, "menu:search-clear").ok_or("the clear button did not spawn")?;
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

    /// `Escape` clears an active search term when the field is focused, and the
    /// cleared term propagates to the [`MenuFilter`].
    #[test]
    fn escape_clears_the_search() -> Result<(), TestError> {
        let mut app = build_app();
        set_field_text(&mut app, "fly");
        settle(&mut app);
        let field = find_by_name(&mut app, "menu:field").ok_or("the field did not spawn")?;
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        settle(&mut app);
        let filter = app.world().resource::<MenuFilter>();
        assert!(filter.query.is_empty(), "the filter clears with the field");
        Ok(())
    }
}
