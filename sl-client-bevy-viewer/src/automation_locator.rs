//! The **locator engine against the viewer's own widgets**: the reveals that
//! need a real menu bar, combo, pie menu or floater — walking a menu path
//! through a submenu, picking a combo's option, choosing a pie slice by its
//! key, and opening a window by id — each driven through the real pointer at
//! the point the engine aims at. The engine's own teeth (each actionability
//! check, scroll areas, virtual lists) live in `sl-viewer-automation`.

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{
        ActionabilityCheck, AutomationError, Deadline, Locator, NodeValue, NodeVisibility, Role,
    };
    use sl_viewer_automation::{
        Gesture, Intent, Progress, Pursuit, PursuitError, Route, RouteProgress, Target,
        open_floater,
    };

    use crate::floater::Floater;
    use crate::floater_chrome::floater_app;
    use crate::floaters::FLOATERS;
    use crate::pie_menu::{FIXTURE_PIE, OpenPieMenu, PieMenuPlugin};
    use crate::ui::UiPanelShown;
    use crate::ui_contract::install_element_hosting;
    use crate::ui_element::ElementCx;
    use crate::ui_elements::ELEMENTS;
    use crate::ui_test::interact::{self, InteractionTest};
    use crate::ui_test::{drain_actions, settle, spawn_element_into};

    /// The registered element `id`, hosted as the contract sweep hosts it.
    fn registered(id: &str) -> Result<App, String> {
        let element = ELEMENTS
            .iter()
            .find(|element| element.id == id)
            .ok_or_else(|| format!("the registry has no `{id}` element"))?;
        let mut app = InteractionTest::new().build();
        install_element_hosting(&mut app);
        // Also turns on the `UiAction` recording `drain_actions` reads.
        spawn_element_into(&mut app, element, ElementCx::new());
        settle(&mut app);
        Ok(app)
    }

    /// Make one gesture with the real pointer.
    fn gesture(app: &mut App, gesture: Gesture, target: &Target) {
        match gesture {
            Gesture::Click => interact::click(app, target.aim, MouseButton::Left),
            Gesture::Hover => interact::hover(app, target.aim),
        }
    }

    /// A gesture a route asked for, and the name key of its node.
    type Made = (Gesture, Option<String>);

    /// Run `route` to its end, making each gesture it asks for; the gestures
    /// made, and the node of the last.
    fn walk(app: &mut App, mut route: Route) -> Result<(Vec<Made>, Option<String>), String> {
        let mut made = Vec::new();
        loop {
            match route
                .poll(app.world_mut())
                .map_err(|error| error.to_string())?
            {
                RouteProgress::Act {
                    gesture: what,
                    target,
                } => {
                    made.push((what, target.node.name_key.clone()));
                    gesture(app, what, &target);
                }
                RouteProgress::Waiting(_) => app.update(),
                RouteProgress::Done(last) => {
                    return Ok((made, last.and_then(|node| node.name_key)));
                }
            }
        }
    }

    /// The actions the widgets emitted since the last look, as their strings.
    fn actions(app: &mut App) -> Vec<String> {
        drain_actions(app)
            .into_iter()
            .map(|action| action.action.to_string())
            .collect()
    }

    /// **A menu path is walked through a submenu by key**: the bar menu is
    /// clicked open, the submenu line hovered open, and the entry clicked —
    /// which is what fires the entry's action.
    #[test]
    fn a_menu_path_reaches_an_entry_in_a_submenu() -> Result<(), String> {
        let mut app = registered("menu-bar")?;
        drain_actions(&mut app);
        let (made, last) = walk(
            &mut app,
            Route::menu_path(&[
                "menu-fixture-world",
                "menu-fixture-environment",
                "menu-fixture-midday",
            ]),
        )?;
        assert_eq!(
            made,
            vec![
                (Gesture::Click, Some("menu-fixture-world".to_owned())),
                (Gesture::Hover, Some("menu-fixture-environment".to_owned())),
                (Gesture::Click, Some("menu-fixture-midday".to_owned())),
            ],
            "click the bar menu open, hover the submenu open, click the entry"
        );
        assert_eq!(
            last.as_deref(),
            Some("menu-fixture-midday"),
            "the last node"
        );
        assert_eq!(
            actions(&mut app),
            vec!["env-midday".to_owned()],
            "its action fired"
        );
        Ok(())
    }

    /// **An open menu is not clicked shut**: a path whose bar menu is
    /// already open skips that click.
    #[test]
    fn a_menu_path_leaves_an_open_menu_open() -> Result<(), String> {
        let mut app = registered("menu-bar")?;
        interact::click_node(&mut app, "menu-button:menu-fixture-world")?;
        drain_actions(&mut app);
        let (made, _last) = walk(
            &mut app,
            Route::menu_path(&["menu-fixture-world", "menu-fixture-mini-map"]),
        )?;
        assert_eq!(
            made,
            vec![(Gesture::Click, Some("menu-fixture-mini-map".to_owned()))],
            "the bar menu was already open"
        );
        assert_eq!(
            actions(&mut app),
            vec!["mini-map".to_owned()],
            "its action fired"
        );
        Ok(())
    }

    /// **A disabled entry stops the path** where it would stop a person: the
    /// route waits on it being enabled, and times out saying so.
    #[test]
    fn a_menu_path_waits_on_a_disabled_entry() -> Result<(), String> {
        let mut app = registered("menu-bar")?;
        drain_actions(&mut app);
        let route = Route::menu_path(&["menu-fixture-avatar", "menu-fixture-sit-down"])
            .with_deadline(Deadline {
                frames: Some(30),
                millis: None,
            });
        let Err(error) = walk(&mut app, route) else {
            return Err("the disabled entry was clicked".to_owned());
        };
        assert!(
            error.contains("still failing the enabled check"),
            "the timeout names the check: {error}"
        );
        assert!(actions(&mut app).is_empty(), "and no action fired");
        Ok(())
    }

    /// **A combo's option is picked by name**: the combo is opened and the
    /// option inside it clicked, and the combo shows it.
    #[test]
    fn a_combo_option_is_picked_by_name() -> Result<(), String> {
        let mut app = registered("combo-box")?;
        app.add_plugins(crate::ui_combo::ComboWidgetPlugin);
        let combo = Locator {
            role: Some(Role::Combobox),
            ..Locator::test_id("combo-demo:combo")
        };
        let (made, _last) = walk(
            &mut app,
            Route::select_option(combo.clone(), Locator::role(Role::ListItem).named("High")),
        )?;
        assert_eq!(made.len(), 2, "open, then pick: {made:?}");
        let target = pursue(&mut app, Pursuit::new(combo, Intent::Hover))?;
        assert_eq!(
            target.node.value,
            Some(NodeValue::Text("High".to_owned())),
            "the combo shows the pick"
        );
        Ok(())
    }

    /// Poll `pursuit` a frame at a time until it is ready.
    fn pursue(app: &mut App, mut pursuit: Pursuit) -> Result<Target, String> {
        loop {
            match pursuit
                .poll(app.world_mut())
                .map_err(|error| error.to_string())?
            {
                Progress::Ready(target) => return Ok(target),
                Progress::Waiting(_) => app.update(),
            }
        }
    }

    /// **A pie slice is chosen by its key** once the pie is open, through
    /// the pointer at the slice's label.
    #[test]
    fn a_pie_slice_is_chosen_by_its_key() -> Result<(), String> {
        let mut app = InteractionTest::new().build();
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        crate::ui_test::enable_action_recording(&mut app);
        app.init_asset::<Shader>();
        app.add_plugins(PieMenuPlugin);
        settle(&mut app);
        app.world_mut().write_message(OpenPieMenu {
            menu: &FIXTURE_PIE,
            at: Vec2::new(700.0, 500.0),
            element: "radial-menu",
            conditions: Vec::new(),
        });
        drain_actions(&mut app);
        let (made, _last) = walk(
            &mut app,
            Route::pie_slice(Locator::role(Role::MenuItem).name_key("pie-fixture-touch")),
        )?;
        assert_eq!(
            made,
            vec![(Gesture::Click, Some("pie-fixture-touch".to_owned()))],
            "one click, on the slice"
        );
        assert_eq!(
            actions(&mut app),
            vec!["touch".to_owned()],
            "and it is the slice chosen"
        );
        Ok(())
    }

    /// **A window is opened by id** and named as the scope to look in: hidden
    /// before, visible after, and an unknown id is not found.
    #[test]
    fn a_floater_is_opened_by_id() -> Result<(), String> {
        let about = FLOATERS
            .iter()
            .find(|floater| floater.id == "about")
            .ok_or("no about floater")?;
        let mut app = floater_app(InteractionTest::new(), about);
        // The specimen host shows its floater; close it, as a viewer starts.
        let mut floaters = app.world_mut().query::<(Entity, &Floater)>();
        let root = floaters
            .iter(app.world())
            .find_map(|(entity, floater)| (floater.id == "about").then_some(entity))
            .ok_or("the about floater was not spawned")?;
        app.world_mut()
            .get_mut::<UiPanelShown>(root)
            .ok_or("the floater has no shown flag")?
            .0 = false;
        settle(&mut app);
        let window = Locator {
            role: Some(Role::Window),
            ..Locator::test_id("floater:about")
        };
        let mut before = Pursuit::new(window.clone(), Intent::Hover);
        match before
            .poll(app.world_mut())
            .map_err(|error| error.to_string())?
        {
            Progress::Waiting(ActionabilityCheck::Visible) => {}
            other => return Err(format!("a closed window is hidden, not {other:?}")),
        }
        let opened = open_floater(app.world_mut(), "about").map_err(|error| error.to_string())?;
        assert_eq!(opened, window, "the window's own locator comes back");
        let target = pursue(&mut app, Pursuit::new(opened, Intent::Hover))?;
        assert_eq!(
            target.node.visibility,
            NodeVisibility::Visible,
            "open and on top"
        );
        match open_floater(app.world_mut(), "no-such-floater") {
            Err(PursuitError::Automation(error))
                if matches!(*error, AutomationError::NotFound { .. }) => {}
            other => return Err(format!("an unknown id is not found, not {other:?}")),
        }
        Ok(())
    }
}
