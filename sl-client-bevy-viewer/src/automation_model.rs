//! The **automation model sweep**: every registered element read through the
//! semantic UI model (`sl_viewer_automation::UiModel`), and what a snapshot
//! costs on every registered floater.
//!
//! The model infers roles from the stock `bevy_ui_widgets` components, so the
//! claim checked here is the one a test driver stands on: every stock widget
//! any element spawns appears in the tree **exactly once, with its role** — not
//! swallowed by a leaf widget around it, not duplicated, not missing because
//! it sits under a container the walk skipped. A stock widget that a custom
//! widget is built on (a tab's radio option, a menu button, a combo's anchor)
//! is expected under the role its `Semantic` names. That every focus stop has a
//! role and a name at all is the contract guard's question
//! (`ui_contract::every_focus_stop_has_a_contract_row`).

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use bevy::prelude::*;
    use bevy::text::EditableText;
    use bevy::ui_widgets::{Button, Checkbox, RadioButton, Slider};
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{NodeId, NodeState, NodeValue, Role, UiNode};
    use sl_viewer_automation::{node_id, snapshot};
    use sl_viewer_ui_core::i18n::Translator;
    use sl_viewer_ui_core::semantic::Semantic;
    use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};

    use crate::floater_chrome::floater_app;
    use crate::floaters::FLOATERS;
    use crate::ui_contract::install_element_hosting;
    use crate::ui_element::{ElementCx, UiElement};
    use crate::ui_elements::ELEMENTS;
    use crate::ui_test::interact::{self, InteractionTest};
    use crate::ui_test::{settle, spawn_element_into};

    /// An interactive app with one registered element spawned and settled —
    /// the contract sweep's host, so the model reads what a pointer would.
    fn element_app(element: &UiElement) -> App {
        let mut app = InteractionTest::new().build();
        install_element_hosting(&mut app);
        spawn_element_into(&mut app, element, ElementCx::new());
        settle(&mut app);
        app
    }

    /// Every stock widget in the app and the role the model must give it —
    /// its stock role, or the role a `Semantic` on it says it really plays.
    fn stock_widgets(app: &mut App) -> Vec<(Entity, Option<String>, Role)> {
        let mut query = app.world_mut().query::<(
            Entity,
            Option<&Name>,
            Option<&Semantic>,
            Has<EditableText>,
            Has<Slider>,
            Has<Checkbox>,
            Has<RadioButton>,
            Has<Button>,
        )>();
        let mut widgets = Vec::new();
        for (entity, name, semantic, field, slider, checkbox, radio, button) in
            query.iter(app.world())
        {
            // The model's own precedence, spelled out independently: a field
            // that is also a button is a field.
            let role = if !(field || slider || checkbox || radio || button) {
                continue;
            } else if let Some(semantic) = semantic {
                semantic.role()
            } else if field {
                Role::Textbox
            } else if slider {
                Role::Slider
            } else if checkbox {
                Role::Checkbox
            } else if radio {
                Role::Radio
            } else if button {
                Role::Button
            } else {
                continue;
            };
            widgets.push((entity, name.map(|name| name.as_str().to_owned()), role));
        }
        widgets
    }

    /// How many times each node id occurs in the tree, and the node itself.
    fn tally<'a>(nodes: &'a [UiNode], into: &mut HashMap<NodeId, (usize, &'a UiNode)>) {
        for node in nodes {
            let entry = into.entry(node.id).or_insert((0, node));
            entry.0 = entry.0.saturating_add(1);
            tally(&node.children, into);
        }
    }

    /// **Every stock widget of every registered element is in the tree once,
    /// with its role.** A widget nested inside a leaf widget, or reached
    /// twice, or given the wrong role, fails naming the element and the
    /// widget.
    #[test]
    fn every_registered_element_yields_a_complete_tree() -> Result<(), String> {
        let mut failures = Vec::new();
        let mut checked = 0_usize;
        for element in ELEMENTS {
            let mut app = element_app(element);
            let widgets = stock_widgets(&mut app);
            let nodes = snapshot(app.world_mut()).map_err(|error| error.to_string())?;
            let mut seen = HashMap::new();
            tally(&nodes, &mut seen);
            for (entity, name, role) in widgets {
                checked = checked.saturating_add(1);
                let what = format!("element `{}` widget {entity} ({name:?})", element.id);
                match seen.get(&node_id(entity)) {
                    None => failures.push(format!("{what}: missing from the tree")),
                    Some(&(count, _node)) if count != 1 => {
                        failures.push(format!("{what}: in the tree {count} times"));
                    }
                    Some(&(_count, node)) if node.role != role => failures.push(format!(
                        "{what}: role {} where {role} was expected",
                        node.role
                    )),
                    Some(_) => {}
                }
            }
        }
        assert!(checked > 0, "the sweep found no stock widgets to check");
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// The node whose test id is `test_id`, anywhere in `nodes`.
    fn find<'a>(nodes: &'a [UiNode], test_id: &str) -> Option<&'a UiNode> {
        nodes.iter().find_map(|node| {
            if node.test_id.as_deref() == Some(test_id) {
                Some(node)
            } else {
                find(&node.children, test_id)
            }
        })
    }

    /// The node `test_id` in a fresh snapshot of `app`.
    fn node(app: &mut App, test_id: &str) -> Result<UiNode, String> {
        settle(app);
        let nodes = snapshot(app.world_mut()).map_err(|error| error.to_string())?;
        find(&nodes, test_id)
            .cloned()
            .ok_or_else(|| format!("no node `{test_id}` in {nodes:#?}"))
    }

    /// The registered element `id`, hosted as the contract sweep hosts it.
    fn registered(id: &str) -> Result<App, String> {
        let element = ELEMENTS
            .iter()
            .find(|element| element.id == id)
            .ok_or_else(|| format!("the registry has no `{id}` element"))?;
        Ok(element_app(element))
    }

    /// **A combo is expanded exactly while its list is open, and its value is
    /// the chosen entry.** Opened and picked through the real pointer.
    #[test]
    fn a_combo_is_expanded_while_open_and_shows_its_choice() -> Result<(), String> {
        let mut app = registered("combo-box")?;
        app.add_plugins(crate::ui_combo::ComboWidgetPlugin);
        let combo = node(&mut app, "combo-demo:combo")?;
        assert_eq!(
            (combo.role, combo.value.clone()),
            (Role::Combobox, Some(NodeValue::Text("Medium".to_owned()))),
            "a closed combo shows its choice"
        );
        assert!(
            !combo.has_state(NodeState::Expanded),
            "closed to begin with"
        );

        interact::click_node(&mut app, "combo-demo:combo")?;
        let open = node(&mut app, "combo-demo:combo")?;
        assert!(open.has_state(NodeState::Expanded), "a click opens it");
        let list = find(&open.children, "combo-popover").ok_or("the open list is not its child")?;
        let rows: Vec<(Role, Option<&str>)> = list
            .children
            .iter()
            .map(|row| (row.role, row.name.as_deref()))
            .collect();
        assert_eq!(
            rows,
            vec![
                (Role::ListItem, Some("Low")),
                (Role::ListItem, Some("Medium")),
                (Role::ListItem, Some("High")),
            ],
            "the list's options"
        );

        interact::click_node(&mut app, "combo-option:2")?;
        let picked = node(&mut app, "combo-demo:combo")?;
        assert_eq!(
            picked.value,
            Some(NodeValue::Text("High".to_owned())),
            "the pick is the new value"
        );
        assert!(
            !picked.has_state(NodeState::Expanded),
            "and picking closes the list"
        );
        Ok(())
    }

    /// **A spinner is a spin button valued by its number, and a click on its
    /// arrow changes the value the model reports.** Its arrows are buttons
    /// beside it in the spinner's group, called by their own keys — no glyph
    /// names them.
    #[test]
    fn a_spin_buttons_arrow_changes_its_reported_value() -> Result<(), String> {
        let mut app = registered("spinner")?;
        let field = node(&mut app, "spinner:field")?;
        assert_eq!(
            (field.role, field.value.clone(), field.name_key.as_deref()),
            (
                Role::SpinButton,
                Some(NodeValue::Number(1.25)),
                Some("spinner-specimen-name")
            ),
            "the field is the spin button, valued by its number"
        );
        let group = node(&mut app, "spinner:spinner")?;
        let arrows: Vec<(Role, Option<&str>)> = ["spinner:up", "spinner:down"]
            .iter()
            .filter_map(|id| find(&group.children, id))
            .map(|arrow| (arrow.role, arrow.name_key.as_deref()))
            .collect();
        assert_eq!(
            arrows,
            vec![
                (Role::Button, Some("spinner-increase")),
                (Role::Button, Some("spinner-decrease")),
            ],
            "both arrows, named, in the spinner's group"
        );

        interact::click_node(&mut app, "spinner:up")?;
        let stepped = node(&mut app, "spinner:field")?;
        assert_eq!(
            stepped.value,
            Some(NodeValue::Number(1.26)),
            "one step up is a hundredth"
        );
        interact::click_node(&mut app, "spinner:down")?;
        interact::click_node(&mut app, "spinner:down")?;
        let back = node(&mut app, "spinner:field")?;
        assert_eq!(back.value, Some(NodeValue::Number(1.24)), "and two down");
        Ok(())
    }

    /// **A tab strip is a tab list, and the selection moves with a click.**
    #[test]
    fn the_selected_tab_follows_the_click() -> Result<(), String> {
        let mut app = registered("tabs-top")?;
        let strip = node(&mut app, "tabs-top:tab-strip")?;
        assert_eq!(strip.role, Role::TabList, "the strip");
        let selected = |strip: &UiNode| -> Vec<(Option<String>, bool)> {
            let mut tabs = Vec::new();
            collect_tabs(strip, &mut tabs);
            tabs
        };
        let before = selected(&strip);
        assert_eq!(before.len(), 3, "three tabs: {before:?}");
        assert!(
            before.iter().all(|(name, _)| name.is_some()),
            "every tab is named: {before:?}"
        );
        assert_eq!(
            before.iter().map(|(_, on)| *on).collect::<Vec<_>>(),
            vec![true, false, false],
            "the first tab starts selected"
        );
        interact::click_node(&mut app, "tabs-top:tab:1")?;
        let after = selected(&node(&mut app, "tabs-top:tab-strip")?);
        assert_eq!(
            after.iter().map(|(_, on)| *on).collect::<Vec<_>>(),
            vec![false, true, false],
            "the clicked tab is the selected one, and only it"
        );
        Ok(())
    }

    /// Every tab under `node`, depth first: its name and whether it is
    /// selected.
    fn collect_tabs(node: &UiNode, out: &mut Vec<(Option<String>, bool)>) {
        if node.role == Role::Tab {
            out.push((node.name.clone(), node.has_state(NodeState::Selected)));
        }
        for child in &node.children {
            collect_tabs(child, out);
        }
    }

    /// **A pie slice is a menu item named by its key**, resolved in English
    /// — not by its compass position, and not left as an unnamed picture.
    #[test]
    fn a_pie_slice_is_named_by_its_key() -> Result<(), String> {
        let mut app = InteractionTest::new().build();
        install_element_hosting(&mut app);
        app.add_systems(
            Startup,
            (|mut commands: Commands, translator: Translator, root: Res<UiRoot>| {
                crate::pie_menu::spawn_pie_menu(
                    &mut commands,
                    root.0,
                    ElementCx::new(),
                    &translator,
                    &crate::pie_menu::FIXTURE_PIE,
                    "radial-menu",
                    crate::pie_menu::PieConditions::default(),
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        let pie = node(&mut app, "pie-menu")?;
        assert_eq!(pie.role, Role::Menu, "the pie is a menu");
        let north = find(&pie.children, "pie-label:north").ok_or("no north slice")?;
        assert_eq!(
            (north.role, north.name_key.as_deref()),
            (Role::MenuItem, Some("pie-fixture-touch")),
            "the north slice"
        );
        assert!(
            north
                .name
                .as_deref()
                .is_some_and(|name| name != "pie-fixture-touch"),
            "the key resolves to English, not to itself: {:?}",
            north.name
        );
        assert!(
            pie.children
                .iter()
                .filter(|slice| slice.role == Role::MenuItem)
                .all(|slice| slice.name.is_some()),
            "every slice is named: {pie:#?}"
        );
        Ok(())
    }

    /// **A menu button is expanded while its menu is open, and each entry is
    /// named by its key and shows its shortcut.**
    #[test]
    fn a_menu_opens_expanded_with_named_entries() -> Result<(), String> {
        let mut app = registered("menu-bar")?;
        let bar = node(&mut app, "menu-bar")?;
        assert_eq!(bar.role, Role::MenuBar, "the bar");
        let button = "menu-button:menu-fixture-world";
        let closed = node(&mut app, button)?;
        assert!(
            !closed.has_state(NodeState::Expanded),
            "closed to begin with"
        );
        interact::click_node(&mut app, button)?;
        let open = node(&mut app, button)?;
        assert_eq!(open.role, Role::MenuItem, "a menu button is a menu item");
        assert_eq!(
            open.name, closed.name,
            "the open menu's entries are not part of the button's name"
        );
        assert!(open.has_state(NodeState::Expanded), "a click opens it");
        // The drop-down hangs under the button's host, beside the button.
        let menu = node(&mut app, "menu-popup:menu-fixture-world")?;
        assert_eq!(menu.role, Role::Menu, "the drop-down");
        let entry = find(&menu.children, "menu-item:mini-map").ok_or("no mini-map entry")?;
        assert_eq!(
            (
                entry.role,
                entry.name_key.as_deref(),
                entry.accelerator.as_deref()
            ),
            (
                Role::MenuItem,
                Some("menu-fixture-mini-map"),
                Some("Ctrl+Shift+M")
            ),
            "an entry is named by its key and carries its shortcut"
        );
        assert!(
            entry
                .name
                .as_deref()
                .is_some_and(|name| !name.contains("Ctrl")),
            "the shortcut is not part of the name: {:?}",
            entry.name
        );
        interact::click_node(&mut app, button)?;
        assert!(
            !node(&mut app, button)?.has_state(NodeState::Expanded),
            "a second click closes it"
        );
        Ok(())
    }

    /// The fastest of this many snapshots is the cost reported, so a one-off
    /// scheduler hiccup does not read as the model's.
    const TIMING_RUNS: usize = 5;

    /// What one snapshot of any registered floater may cost. Generous — it is
    /// a guard against the model going quadratic, not a benchmark — and far
    /// above what was measured (see the roadmap entry that landed the model).
    const SNAPSHOT_BUDGET: Duration = Duration::from_millis(250);

    /// **A snapshot of the largest floater stays cheap.** Every registered
    /// window is opened with its content and snapshotted; the slowest is
    /// reported, with its node count, and held to [`SNAPSHOT_BUDGET`].
    #[test]
    fn snapshot_cost_on_every_floater() -> Result<(), String> {
        let mut slowest: Option<(&str, Duration, usize)> = None;
        for floater in FLOATERS {
            let mut app = floater_app(InteractionTest::new(), floater);
            let mut best = Duration::MAX;
            let mut count = 0;
            for _run in 0..TIMING_RUNS {
                let started = Instant::now();
                let nodes = snapshot(app.world_mut()).map_err(|error| error.to_string())?;
                best = best.min(started.elapsed());
                let mut seen = HashMap::new();
                tally(&nodes, &mut seen);
                count = seen.len();
            }
            if slowest.is_none_or(|(_id, time, _count)| best > time) {
                slowest = Some((floater.id, best, count));
            }
        }
        let (id, time, count) = slowest.ok_or("no floaters to measure")?;
        #[expect(
            clippy::print_stderr,
            reason = "the measurement is the point: `--nocapture` shows it for the record"
        )]
        {
            eprintln!("slowest floater snapshot: `{id}`, {count} nodes, {time:?}");
        }
        assert!(
            time <= SNAPSHOT_BUDGET,
            "snapshotting `{id}` ({count} nodes) took {time:?}, over {SNAPSHOT_BUDGET:?}"
        );
        Ok(())
    }
}
