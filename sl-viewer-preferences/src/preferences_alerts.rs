//! The Preferences **alerts / popups** tab (`viewer-preferences-alerts-tab`).
//!
//! Three headline notification toggles, then the per-notification list the
//! reference calls the popup list: one row per *suppressible* catalogue
//! template (`NotificationIgnore::is_suppressible`) with a "Show" checkbox
//! bound to the template's `[notifications]` show/suppress `Bool` setting and
//! a label showing the ported reference `ignoretext`
//! (`NotificationTemplate::ignore_key`). This is Firestorm's current
//! single-filtered-list design (`buildPopupList` over `all_popups`), not the
//! older two-list one; the floater's shared search box doubles as the list
//! filter.
//!
//! Mechanics worth naming:
//!
//! - **The rows are the settings binding.** Each pooled row's checkbox is a
//!   plain [`Checkbox`] the bind pass points at a different setting by
//!   replacing its [`SettingBinding`] — the binding layer's idempotent sync
//!   then repaints it, its `ValueChange` observer writes the store, and the
//!   shell's account guard disables it until the account scope loads. No
//!   bespoke toggle plumbing.
//! - **Hidden snapshot markers.** The shell's Cancel/OK snapshot walks every
//!   [`SettingBinding`] under the floater on the open edge — but the
//!   virtualized list only materialises the rows on screen. A display-none
//!   container of one widgetless marker binding per suppressible template
//!   keeps every list edit inside the snapshot, so Cancel reverts the whole
//!   list, not just the visible slice.
//! - **Filter coupling.** The list matches the shared lowercased term against
//!   its resolved labels and reports "this tab still has hits" through
//!   `PreferencesExtraHits`, so the shell dims / jumps tabs exactly as it
//!   does for `spawn_pref_checkbox` rows.
//!
//! Reference (Firestorm, read-only): `panel_preferences_alerts.xml`,
//! `llfloaterpreference.cpp` (`buildPopupList` / `onSelectPopup`).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::Checkbox;

use crate::i18n::{Translated, Translator};
use crate::notifications::NOTIFICATIONS;
use crate::preferences::{
    CHECK_OFF, CHECK_SIZE, CONTROL_BORDER, PREF_TABS, PrefCheckboxBox, PreferencesExtraHits,
    PreferencesState, apply_preferences_filter, mirror_preferences_filter, spawn_pref_checkbox,
    spawn_pref_combo, spawn_pref_section, spawn_pref_text,
};
use crate::settings_binding::SettingBinding;
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableSelectionMode, TableSpec,
    set_table_cell, spawn_table, spawn_table_row,
};
use crate::ui_text_input::TextInputKind;
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use sl_settings::SettingValue;

/// The stable id of this tab in `PREF_TABS`.
const TAB_ID: &str = "alerts";

/// The list's uniform row height, in logical pixels.
const ROW_HEIGHT: f32 = 24.0;

/// The list's header / cell font size, in logical pixels.
const FONT: f32 = 13.0;

/// The fixed width of the "Show" checkbox column, in logical pixels.
const SHOW_COL_WIDTH: f32 = 48.0;

/// The header label colour (the preferences section palette).
const HEADER_COLOR: Color = Color::srgb(0.75, 0.80, 0.88);

/// The cell label colour (the preferences row-label palette).
const CELL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The popup list's table: the custom "Show" checkbox column and the flexible
/// ignoretext label column. Display-only as far as the widget is concerned —
/// no selection, no sort, no persisted geometry.
const ALERTS_TABLE: TableSpec = TableSpec {
    element: "preferences-alerts",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "",
            token: "show",
            kind: TableColumnKind::Custom,
            width: TableColumnWidth::Fixed {
                default: SHOW_COL_WIDTH,
            },
            align: TableAlign::Center,
            sortable: false,
        },
        TableColumn {
            header_key: "preferences-alerts-col-label",
            token: "label",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
    ],
    default_sort: &[],
    builtin_sort: false,
    row_height: ROW_HEIGHT,
    font_size: FONT,
    header_color: HEADER_COLOR,
    cell_color: CELL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: None,
    widths_setting: None,
};

/// The alerts tab's retained table entities, inserted by `build_alerts_tab`.
#[derive(Resource, Debug, Clone, Copy)]
struct AlertsTabUi {
    /// The table root ([`spawn_table_row`] needs it).
    table: Entity,
    /// The virtualized viewport (carries the [`VirtualList`] whose
    /// `item_count` follows the view).
    viewport: Entity,
}

/// One suppressible catalogue template, projected for the list.
#[derive(Debug, Clone)]
struct AlertEntry {
    /// The template name — the show/suppress setting's key.
    template: &'static str,
    /// The label as last resolved through the active locale.
    label: String,
    /// [`label`](Self::label) lowercased, the filter's match target.
    label_lower: String,
}

/// The list model: every suppressible template, and the filtered, sorted view
/// the pooled rows present. Rebuilt by [`refresh_alerts_view`]; its change
/// tick tells [`bind_alerts_rows`] to re-project every visible row.
#[derive(Resource, Debug, Default)]
struct AlertsModel {
    /// All suppressible templates, labels resolved (unsorted, unfiltered).
    entries: Vec<AlertEntry>,
    /// Indices into [`entries`](Self::entries): the rows currently presented,
    /// label-sorted and filter-matched.
    view: Vec<usize>,
}

/// The cell entities of one pooled row, for the bind pass.
#[derive(Component, Debug, Clone, Copy)]
struct AlertRowParts {
    /// The row's checkbox widget (its [`SettingBinding`] is replaced on
    /// rebind).
    checkbox: Entity,
    /// The label column's value text node.
    label_cell: Entity,
}

/// Build the alerts tab into its panel: the headline toggles, the popup-list
/// table, and the hidden snapshot markers (one widgetless [`SettingBinding`]
/// per suppressible template — see the module doc).
pub(crate) fn build_alerts_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-alert-headlines");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-friend-online-toasts",
        SettingBinding::account(crate::people::SETTING_FRIEND_NOTIFY),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-contact-set-online-toasts",
        SettingBinding::account(crate::people::SETTING_CONTACT_SET_NOTIFY),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-group-notice-toasts",
        SettingBinding::account(crate::group_notice::SETTING_GROUP_NOTICE_TOASTS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-auto-accept-inventory",
        SettingBinding::account(crate::offers_invites::SETTING_AUTO_ACCEPT_INVENTORY),
    );

    // The radar's enter / leave reports (viewer-avatar-radar): each an opt-in
    // per-avatar toggle, plus the output channel and the young-account alert.
    spawn_pref_section(commands, panel, "preferences-section-radar-alerts");
    for (label_key, setting) in [
        (
            "preferences-row-radar-chat-enter",
            crate::radar::SETTING_CHAT_ENTER,
        ),
        (
            "preferences-row-radar-chat-leave",
            crate::radar::SETTING_CHAT_LEAVE,
        ),
        (
            "preferences-row-radar-draw-enter",
            crate::radar::SETTING_DRAW_ENTER,
        ),
        (
            "preferences-row-radar-draw-leave",
            crate::radar::SETTING_DRAW_LEAVE,
        ),
        (
            "preferences-row-radar-sim-enter",
            crate::radar::SETTING_SIM_ENTER,
        ),
        (
            "preferences-row-radar-sim-leave",
            crate::radar::SETTING_SIM_LEAVE,
        ),
        (
            "preferences-row-radar-age-alert",
            crate::radar::SETTING_AGE_ALERT,
        ),
    ] {
        spawn_pref_checkbox(commands, panel, label_key, SettingBinding::account(setting));
    }
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-radar-output",
        SettingBinding::account(crate::radar::SETTING_ALERT_OUTPUT),
        &[
            (
                "preferences-radar-output-chat",
                SettingValue::String("chat".to_owned()),
            ),
            (
                "preferences-radar-output-toast",
                SettingValue::String("toast".to_owned()),
            ),
        ],
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-radar-age-days",
        SettingBinding::account(crate::radar::SETTING_AGE_DAYS),
        TextInputKind::Integer,
        1.0,
    );

    spawn_pref_section(commands, panel, "preferences-section-alert-popups");
    let table = spawn_table(commands, panel, &ALERTS_TABLE);
    // The custom checkbox column renders no built-in header — give it the
    // translated "Show" label the reference column carries.
    if let Some(header_cell) = table.header_cell(0) {
        commands.spawn((
            Text::default(),
            Translated::new("preferences-alerts-col-show"),
            crate::ui_font::UiFont::Sans.at(FONT),
            TextColor(HEADER_COLOR),
            ChildOf(header_cell),
        ));
    }
    commands.insert_resource(AlertsTabUi {
        table: table.root,
        viewport: table.viewport,
    });
    commands.insert_resource(AlertsModel::default());

    // The hidden snapshot markers (see the module doc): a display-none
    // container so the markers never lay out, one binding per suppressible
    // template so the shell's open-edge snapshot covers the whole list.
    let markers = commands
        .spawn((
            Node {
                display: Display::None,
                ..default()
            },
            Name::new("preferences:alerts:snapshot-markers"),
            ChildOf(panel),
        ))
        .id();
    for entry in NOTIFICATIONS {
        if entry.ignore.is_suppressible() {
            commands.spawn((
                Node::default(),
                SettingBinding::account(entry.name),
                ChildOf(markers),
            ));
        }
    }
}

/// Rebuild [`AlertsModel`] when its inputs move: resolve the labels on the
/// first run and on a locale switch, and re-derive the view (label-sorted,
/// filter-matched) whenever that happens or the shared filter term changes.
/// Keeps the viewport's [`VirtualList::item_count`] current and reports the
/// tab's filter hits through `PreferencesExtraHits`.
fn refresh_alerts_view(
    model: Option<ResMut<AlertsModel>>,
    ui: Option<Res<AlertsTabUi>>,
    state: Res<PreferencesState>,
    translator: Translator,
    mut extra_hits: ResMut<PreferencesExtraHits>,
    mut lists: Query<&mut VirtualList>,
) {
    let (Some(mut model), Some(ui)) = (model, ui) else {
        return;
    };
    let relabel = model.entries.is_empty() || translator.changed();
    if !relabel && !state.is_changed() {
        return;
    }
    if relabel {
        model.entries = NOTIFICATIONS
            .iter()
            .filter(|entry| entry.ignore.is_suppressible())
            .filter_map(|entry| entry.ignore_key.map(|ignore_key| (entry, ignore_key)))
            .map(|(entry, ignore_key)| {
                let label = translator.get(ignore_key);
                let label_lower = label.to_lowercase();
                AlertEntry {
                    template: entry.name,
                    label,
                    label_lower,
                }
            })
            .collect();
    }
    let term = state.filter();
    let filtering = !term.is_empty();
    let view = build_view(&model.entries, term);
    let count = view.len();
    if let Ok(mut list) = lists.get_mut(ui.viewport)
        && list.item_count != count
    {
        list.item_count = count;
        list.scroll_to_top();
    }
    model.view = view;

    // Report the tab's hits for the shell's dim / first-hit-jump pass —
    // change-guarded, so a quiet frame does not retrigger the filter sweep.
    let hits = filtering && count > 0;
    if let Some(tab_index) = PREF_TABS.iter().position(|tab| tab.id == TAB_ID)
        && extra_hits.0.get(&tab_index).copied() != Some(hits)
    {
        extra_hits.0.insert(tab_index, hits);
    }
}

/// The view over `entries` for a lowercased filter `term`: the indices of the
/// matching entries (all of them for an empty term), ordered by their
/// case-folded label. Pure, so the filter/sort behaviour is unit-testable.
fn build_view(entries: &[AlertEntry], term: &str) -> Vec<usize> {
    let filtering = !term.is_empty();
    let mut view: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| !filtering || entry.label_lower.contains(term))
        .map(|(index, _)| index)
        .collect();
    view.sort_by(|left, right| {
        let left_key = entries.get(*left).map(|entry| &entry.label_lower);
        let right_key = entries.get(*right).map(|entry| &entry.label_lower);
        left_key.cmp(&right_key)
    });
    view
}

/// Give each freshly-pooled viewport row its cells: the table's text cells,
/// the checkbox in the custom column, and the [`AlertRowParts`] wiring.
fn populate_alerts_rows(
    mut commands: Commands,
    ui: Option<Res<AlertsTabUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        let cells = spawn_table_row(&mut commands, row_entity, ui.table, &ALERTS_TABLE);
        let (Some(show_cell), Some(label_cell)) = (cells.cell(0), cells.cell(1)) else {
            continue;
        };
        // A bare Checkbox: the bind pass points it at its row's setting by
        // inserting the SettingBinding, and the binding layer does the rest
        // (sync, write, the shell's account guard, the disabled tint).
        let checkbox = commands
            .spawn((
                Checkbox,
                Node {
                    width: Val::Px(CHECK_SIZE),
                    height: Val::Px(CHECK_SIZE),
                    border: UiRect::all(Val::Px(2.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(CONTROL_BORDER),
                BackgroundColor(CHECK_OFF),
                TabIndex(0),
                PrefCheckboxBox,
                ChildOf(show_cell),
            ))
            .id();
        commands.entity(row_entity).insert(AlertRowParts {
            checkbox,
            label_cell,
        });
    }
}

/// Project the view into the pooled rows: on a model rebuild or a row-window
/// move, set each visible row's label and point its checkbox at its
/// template's setting by replacing the [`SettingBinding`].
fn bind_alerts_rows(
    model: Option<Res<AlertsModel>>,
    ui: Option<Res<AlertsTabUi>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &AlertRowParts)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut commands: Commands,
) {
    let (Some(model), Some(ui)) = (model, ui) else {
        return;
    };
    let refresh_all = model.is_changed();
    for (row, child_of, parts) in &rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let Some(index) = row.index else {
            // Parked rows are hidden by the pool; nothing to project.
            continue;
        };
        let Some(entry) = model
            .view
            .get(index)
            .and_then(|entry_index| model.entries.get(*entry_index))
        else {
            continue;
        };
        set_table_cell(&mut texts, parts.label_cell, &entry.label, CELL_COLOR);
        commands
            .entity(parts.checkbox)
            .insert(SettingBinding::account(entry.template));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{AlertEntry, build_view};

    /// An entry whose lowercased label is derived, as the refresh builds them.
    fn entry(template: &'static str, label: &str) -> AlertEntry {
        AlertEntry {
            template,
            label: label.to_owned(),
            label_lower: label.to_lowercase(),
        }
    }

    /// An empty term presents everything, ordered by case-folded label; a term
    /// keeps only substring matches (case-insensitively) in the same order.
    #[test]
    fn view_sorts_by_label_and_filters_by_substring() {
        let entries = vec![
            entry("PayObject", "Confirm before I pay an object"),
            entry("AboutLand", "About Land: unsaved changes"),
            entry("ScriptPerm", "Warn about script permissions"),
        ];
        assert_eq!(build_view(&entries, ""), vec![1, 0, 2]);
        assert_eq!(build_view(&entries, "about"), vec![1, 2]);
        assert_eq!(build_view(&entries, "pay an"), vec![0]);
        assert_eq!(build_view(&entries, "no such term"), Vec::<usize>::new());
    }
}

/// Owns the alerts tab's list: the model refresh, the row pool population and
/// the row binding. The tab's *content build* is `build_alerts_tab`, invoked
/// by the shell through `PREF_TABS`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreferencesAlertsPlugin;

impl Plugin for PreferencesAlertsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // Before the filter sweep so a term change dims / jumps tabs
                // with this frame's hit report; after the mirror so the term
                // is this frame's.
                refresh_alerts_view
                    .after(mirror_preferences_filter)
                    .before(apply_preferences_filter)
                    .before(layout_virtual_lists),
                // After the pool follows the item count, as the People list
                // orders its populate / bind.
                (populate_alerts_rows, bind_alerts_rows)
                    .chain()
                    .after(layout_virtual_lists),
            ),
        );
    }
}
