//! The **automation model sweep**: every registered element read through the
//! semantic UI model (`sl_viewer_automation::UiModel`), and what a snapshot
//! costs on every registered floater.
//!
//! The model infers roles from the stock `bevy_ui_widgets` components, so the
//! claim checked here is the one a test driver stands on: every stock widget
//! any element spawns appears in the tree **exactly once, with its role** — not
//! swallowed by a leaf widget around it, not duplicated, not missing because
//! it sits under a container the walk skipped. The custom widgets (floaters,
//! tab rows, lists, menus) get their roles in
//! `viewer-automation-semantic-custom-widgets`, which extends this sweep's
//! question to them.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use bevy::prelude::*;
    use bevy::text::EditableText;
    use bevy::ui_widgets::{Button, Checkbox, RadioButton, Slider};
    use sl_automation_proto::{NodeId, Role, UiNode};
    use sl_viewer_automation::{node_id, snapshot};

    use crate::floater_chrome::floater_app;
    use crate::floaters::FLOATERS;
    use crate::ui_contract::install_element_hosting;
    use crate::ui_element::{ElementCx, UiElement};
    use crate::ui_elements::ELEMENTS;
    use crate::ui_test::interact::InteractionTest;
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

    /// Every stock widget in the app and the role the model must give it.
    fn stock_widgets(app: &mut App) -> Vec<(Entity, Option<String>, Role)> {
        let mut query = app.world_mut().query::<(
            Entity,
            Option<&Name>,
            Has<EditableText>,
            Has<Slider>,
            Has<Checkbox>,
            Has<RadioButton>,
            Has<Button>,
        )>();
        let mut widgets = Vec::new();
        for (entity, name, field, slider, checkbox, radio, button) in query.iter(app.world()) {
            // The model's own precedence, spelled out independently: a field
            // that is also a button is a field.
            let role = if field {
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
