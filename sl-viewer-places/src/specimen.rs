//! Shared plumbing for this crate's gallery / `ui_test` floater specimens.
//!
//! A table specimen's rows come from the table widget's own
//! [`spawn_specimen_table_rows`], since neither specimen host runs the virtual
//! list pool or a window's populate / bind systems. What that leaves to the
//! consumer is dressing the rows the way its live systems would; the one piece
//! every window here shares is the selection a reply leaves behind.

use bevy::prelude::*;
use bevy_flair::style::components::ClassList;
use sl_viewer_ui_core::skin::{SELECTED_CLASS, set_state_class};

use crate::ui_table::{
    SpecimenTable, TableRowCells, TableSpec, TableState, spawn_specimen_table_rows,
};

/// Pool and bind `rows` (each a row's value per column, drawn in the spec's
/// cell colour) into the table `table`, and show the data indices in
/// `selected` as its selection — in the [`TableState`], as a click would leave
/// it, and on the rows, as the widget's selection pass would paint them.
/// Returns each row's entity and cells, for a caller that attaches its row
/// observer or fills custom columns.
pub(crate) fn spawn_sample_table_rows(
    commands: &mut Commands,
    table: SpecimenTable,
    spec: &'static TableSpec,
    rows: &[Vec<String>],
    selected: &[usize],
) -> Vec<(Entity, TableRowCells)> {
    let values: Vec<Vec<(String, Color)>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| (value.clone(), spec.cell_color))
                .collect()
        })
        .collect();
    let bound = spawn_specimen_table_rows(commands, table, spec, &values);
    let selection = selected.to_vec();
    let anchor = selected.last().copied();
    commands
        .entity(table.root)
        .entry::<TableState>()
        .and_modify(move |mut state| state.set_selection(selection, anchor));
    for (index, (row, _cells)) in bound.iter().enumerate() {
        let is_selected = selected.contains(&index);
        commands
            .entity(*row)
            .entry::<ClassList>()
            .and_modify(move |mut classes| {
                set_state_class(&mut classes, SELECTED_CLASS, is_selected);
            });
    }
    bound
}
