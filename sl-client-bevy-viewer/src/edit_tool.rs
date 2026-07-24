//! The build-tool state and the **Build Tools floater**
//! (`viewer-object-edit-floater-shell` plus the tool-mode half of
//! `viewer-transform-gizmos`): the edit-mode switch, the manipulator mode
//! (move / rotate / stretch), the grid options the gizmos snap with, the
//! numeric position / rotation / size fields mirroring the primary selection,
//! and the tabbed shell the per-aspect editors (object / features / texture /
//! content tabs — their own roadmap tasks) will dock into.
//!
//! # Model
//!
//! - [`EditToolState`] is the resource everything keys off: `active` mirrors
//!   the floater's visibility (open floater = edit mode), `tool` picks the
//!   gizmo ([`crate::gizmos`]), and the toggles / grid values feed selection
//!   ([`crate::edit_selection`]) and gizmo snapping. Opened from the **Build**
//!   menu or `Ctrl+B`.
//! - The **numeric fields** show the primary selection's Second Life
//!   transform ([`crate::objects::ObjectSlMotion`]) live — including during a
//!   gizmo drag — and commit an edit on `Enter` or focus loss, sending the
//!   same `MultipleObjectUpdate` ([`Command::UpdateObject`]) the gizmos send.
//!   Rotations display as the reference viewer's XYZ Euler degrees
//!   ([`crate::edit_math::rotation_to_euler_deg`]).
//! - The **tab strip** hosts the per-aspect editors: the Object and Features
//!   pages carry the parameter editors ([`crate::edit_params`],
//!   `viewer-prim-parameter-editing`); the Texture and Content pages are
//!   placeholders whose contents ship in `viewer-prim-texture-editing` and
//!   `viewer-prim-inventory-editing`.
//!
//! Reference (Firestorm, read-only): `llfloatertools`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{Command, ObjectTransform, Permissions, SlCommand, Vector};

use crate::edit_math::{clamp_scale, euler_deg_to_rotation, rotation_to_euler_deg};
use crate::edit_selection::SelectionSet;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::input_context::InputContext;
use crate::objects::{ObjectSlMotion, ObjectState, SceneObject};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, fill_tab_container, spawn_tab_container,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::web_floater::set_editor_text;

/// The floater's font size, in logical pixels.
pub(crate) const TOOL_FONT_SIZE: f32 = 13.0;

/// The width of a numeric transform field, in `"0"`-glyph advances.
const FIELD_WIDTH_GLYPHS: f32 = 8.0;

/// A tool button's background while its tool is active.
const TOOL_ACTIVE_COLOR: Color = Color::srgba(0.25, 0.45, 0.7, 1.0);

/// A tool button's background while inactive.
const TOOL_IDLE_COLOR: Color = Color::srgba(0.18, 0.18, 0.2, 1.0);

/// The toggle-row check glyph while on.
pub(crate) const CHECKED_GLYPH: &str = "☑";

/// The toggle-row check glyph while off.
pub(crate) const UNCHECKED_GLYPH: &str = "☐";

/// The default grid unit, in metres — the reference's `GridResolution`.
const DEFAULT_GRID_UNIT: f32 = 0.5;

/// The skin class for the floater's label / summary text
/// (`--text-muted`-driven; see `assets/skins/common.css`).
pub(crate) const LABEL_CLASS: &str = "sk-build-label";

/// The skin class for the floater's value / button text
/// (`--text-primary`-driven).
pub(crate) const VALUE_CLASS: &str = "sk-build-value";

/// The skin class for the placeholder tab text (`--text-disabled`-driven).
const PLACEHOLDER_CLASS: &str = "sk-build-placeholder";

/// Which manipulator the build tool drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EditTool {
    /// The translate gizmo (axis arrows + planar handles).
    #[default]
    Move,
    /// The rotate gizmo (axis rings).
    Rotate,
    /// The scale gizmo (face + corner handles).
    Stretch,
}

/// The grid frame the gizmos align to and snap in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum GridFrame {
    /// The world axes (the reference's `GRID_MODE_WORLD`).
    #[default]
    World,
    /// The primary selection's own axes (`GRID_MODE_LOCAL`).
    Local,
    /// A reference object's axes (`GRID_MODE_REF_OBJECT`). Modelled now so the
    /// snapping code handles it, but only settable once the grid-options task
    /// (`viewer-build-grid-options`) ships its *Use Selection for Grid*
    /// command.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the reference-object grid frame is set by the grid-options task \
                      (viewer-build-grid-options); the frame model carries it from the start"
        )
    )]
    Reference,
}

/// The build tool's shared state. See the [module documentation](self).
#[expect(
    clippy::struct_excessive_bools,
    reason = "the flags mirror the reference viewer's independent build-tool toggles \
              (EditLinkedParts, ScaleUniform, SnapEnabled) plus the tool's own active bit; \
              none is a state machine in disguise"
)]
#[derive(Resource, Debug)]
pub(crate) struct EditToolState {
    /// Whether the build tool is active (the floater is open): selection
    /// clicks, gizmos, and the touch-suppression all key off this.
    pub(crate) active: bool,
    /// The manipulator picked in the floater (the resting tool).
    pub(crate) tool: EditTool,
    /// A manipulator temporarily forced by a held modifier — the reference's
    /// `Ctrl` = rotate / `Ctrl+Shift` = stretch while held
    /// (`LLToolCompTranslate::handleHover`'s mask dispatch). Cleared on
    /// release; [`effective_tool`](Self::effective_tool) folds it in.
    pub(crate) held_override: Option<EditTool>,
    /// Edit linked parts: select and edit individual linkset prims instead of
    /// whole linksets (the reference's `EditLinkedParts`).
    pub(crate) edit_linked: bool,
    /// Stretch both sides: scale about the selection centre instead of
    /// holding the opposite face in place (the reference's `ScaleUniform`).
    pub(crate) stretch_both: bool,
    /// Whether grid snapping is on (the reference's `SnapEnabled`).
    pub(crate) snap: bool,
    /// The grid unit, in metres (the reference's `GridResolution`).
    pub(crate) grid_unit: f32,
    /// The grid frame the gizmos align to.
    pub(crate) frame: GridFrame,
}

impl Default for EditToolState {
    /// Reference-faithful defaults: move tool, whole-linkset selection, snap
    /// on at a half-metre grid, world frame.
    fn default() -> Self {
        Self {
            active: false,
            tool: EditTool::Move,
            held_override: None,
            edit_linked: false,
            stretch_both: false,
            snap: true,
            grid_unit: DEFAULT_GRID_UNIT,
            frame: GridFrame::World,
        }
    }
}

impl EditToolState {
    /// The manipulator actually in effect: a held modifier override
    /// (`Ctrl` = rotate, `Ctrl+Shift` = stretch), or the floater's resting
    /// tool.
    pub(crate) fn effective_tool(&self) -> EditTool {
        self.held_override.unwrap_or(self.tool)
    }
}

/// Fold the held modifiers into [`EditToolState::held_override`] — the
/// reference's in-edit-mode tool chords: `Ctrl` shows the rotate rig,
/// `Ctrl+Shift` the stretch rig, releasing returns to the floater's tool.
/// Suppressed while a text field has focus, so `Ctrl` chords typed into a
/// field never flip the rig.
fn apply_tool_modifier_override(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    mut state: ResMut<EditToolState>,
) {
    let want = if !state.active || *context == InputContext::TextEntry {
        None
    } else {
        let ctrl =
            keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
        let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
        match (ctrl, shift) {
            (true, true) => Some(EditTool::Stretch),
            (true, false) => Some(EditTool::Rotate),
            _released => None,
        }
    };
    if state.held_override != want {
        state.held_override = want;
    }
}

/// Which boolean of [`EditToolState`] a toggle row flips.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum BuildToggle {
    /// Grid snapping on / off.
    Snap,
    /// Edit linked parts.
    EditLinked,
    /// Stretch both sides.
    StretchBoth,
    /// Local (vs world) grid frame.
    LocalFrame,
}

impl BuildToggle {
    /// Read the toggle's current value off the state.
    fn get(self, state: &EditToolState) -> bool {
        match self {
            Self::Snap => state.snap,
            Self::EditLinked => state.edit_linked,
            Self::StretchBoth => state.stretch_both,
            Self::LocalFrame => state.frame == GridFrame::Local,
        }
    }

    /// Flip the toggle on the state.
    fn flip(self, state: &mut EditToolState) {
        match self {
            Self::Snap => state.snap = !state.snap,
            Self::EditLinked => state.edit_linked = !state.edit_linked,
            Self::StretchBoth => state.stretch_both = !state.stretch_both,
            Self::LocalFrame => {
                state.frame = if state.frame == GridFrame::Local {
                    GridFrame::World
                } else {
                    GridFrame::Local
                };
            }
        }
    }
}

/// The transform row a numeric field belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldGroup {
    /// Position, metres (region-local for a root, parent-relative for a
    /// linked part).
    Position,
    /// Rotation, Euler degrees.
    Rotation,
    /// Size, metres.
    Size,
}

/// Marks one of the nine transform fields with its row and axis.
#[derive(Component, Debug, Clone, Copy)]
struct BuildNumericField {
    /// Which row.
    group: FieldGroup,
    /// Which axis (0 = X, 1 = Y, 2 = Z).
    axis: usize,
}

/// Marks the grid-unit field.
#[derive(Component, Debug, Clone, Copy)]
struct BuildGridUnitField;

/// Marks a tool button with the tool it selects.
#[derive(Component, Debug, Clone, Copy)]
struct BuildToolButton(EditTool);

/// Marks a toggle row's check glyph.
#[derive(Component, Debug, Clone, Copy)]
struct BuildToggleGlyph(BuildToggle);

/// The build floater's entities.
#[derive(Resource, Debug)]
pub(crate) struct BuildToolsUi {
    /// The floater root (carries `UiPanelShown`).
    panel: Entity,
    /// The nine transform fields, position / rotation / size × X / Y / Z.
    fields: [Entity; 9],
    /// The grid-unit field.
    grid_field: Entity,
    /// The selection-summary text.
    summary_text: Entity,
    /// The tab strip.
    tab_strip: Entity,
    /// The five tab pages, in tab order (General / Object / Features /
    /// Texture / Content).
    tab_pages: [Entity; 5],
}

impl BuildToolsUi {
    /// The floater root, for the menu-bar open state and toggle.
    pub(crate) const fn panel(&self) -> Entity {
        self.panel
    }
}

/// The tab-page container entities the per-aspect editors dock into, published
/// by [`spawn_build_floater`] for the parameter-tab module
/// ([`crate::edit_params`]) to populate.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct BuildTabPages {
    /// The **General** tab page (name / description; the reference's
    /// `llpanelpermissions` — its permission / sale surfaces are their own
    /// tasks).
    pub(crate) general: Entity,
    /// The **Object** tab page (also hosts the transform rows).
    pub(crate) object: Entity,
    /// The **Features** tab page.
    pub(crate) features: Entity,
}

/// The plugin wiring the build tool into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditToolPlugin;

impl Plugin for EditToolPlugin {
    /// Register the tool state, spawn the floater, and run the sync systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<EditToolState>()
            .init_resource::<BuildFieldFocus>()
            .add_systems(
                Startup,
                (spawn_build_floater, crate::edit_params::spawn_param_tabs)
                    .chain()
                    .after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    toggle_build_floater_on_ctrl_b,
                    mirror_floater_into_state,
                    apply_tool_modifier_override,
                    update_tool_button_visuals,
                    update_toggle_glyphs,
                    sync_tab_pages,
                    update_selection_summary,
                    sync_numeric_fields,
                    commit_numeric_fields,
                )
                    .chain(),
            );
    }
}

/// Spawn the Build Tools floater: tool buttons, toggles, grid unit, the
/// selection summary, the nine transform fields, and the placeholder tab
/// shell.
fn spawn_build_floater(mut commands: Commands, root: Option<Res<UiRoot>>) {
    let Some(root) = root.map(|root| root.0) else {
        return;
    };
    let handle = spawn_floater(
        &mut commands,
        root,
        FloaterSpec {
            id: "build-tools",
            title: String::from("Build Tools"),
            position: Vec2::new(60.0, 80.0),
            // A definite, resizable content area (like the profile floater):
            // the tab bar and pages track the window and the pages scroll
            // their overflow, so the parameter editors stay reachable at any
            // size.
            default_size: Some(Vec2::new(420.0, 640.0)),
            min_size: Some(Vec2::new(340.0, 400.0)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("build-tools-floater-title"));
    let content = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                // Fill the floater's definite content slot: the fixed rows
                // above the tabs keep their content height and the tab
                // container grows into the rest.
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..column(Val::Px(6.0))
            },
            ChildOf(handle.content),
        ))
        .id();

    // Tool row: Move / Rotate / Stretch.
    let tool_row = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(6.0),
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    for (tool, key, tab_index) in [
        (EditTool::Move, "build-tool-move", 1),
        (EditTool::Rotate, "build-tool-rotate", 2),
        (EditTool::Stretch, "build-tool-stretch", 3),
    ] {
        spawn_tool_button(&mut commands, tool_row, tool, key, tab_index);
    }

    // Toggle rows.
    for (toggle, key, tab_index) in [
        (BuildToggle::Snap, "build-toggle-snap", 4),
        (BuildToggle::LocalFrame, "build-toggle-local-frame", 5),
        (BuildToggle::EditLinked, "build-toggle-edit-linked", 6),
        (BuildToggle::StretchBoth, "build-toggle-stretch-both", 7),
    ] {
        spawn_toggle_row(&mut commands, content, toggle, key, tab_index);
    }

    // Grid unit row.
    let grid_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    spawn_row_label(&mut commands, grid_row, "build-grid-unit-label");
    let grid_field = spawn_text_input(
        &mut commands,
        grid_row,
        &TextInputSpec {
            initial: format_metres(DEFAULT_GRID_UNIT),
            font_size: TOOL_FONT_SIZE,
            width_glyphs: 6.0,
            tab_index: 8,
            ..TextInputSpec::new("build-grid-unit", TextInputKind::Float)
        },
    );
    commands.entity(grid_field).insert(BuildGridUnitField);

    // Selection summary.
    let summary_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(TOOL_FONT_SIZE),
            // A skinless fallback; the skin recolours via the class token.
            TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
            ClassList::new_with_classes([LABEL_CLASS]),
            Name::new("build-tools:summary"),
            ChildOf(content),
        ))
        .id();

    // The tab shell: the reference's per-aspect editor tabs, in its order —
    // General (name / description; permissions are their own tasks), Object
    // (the transform fields, as the reference's `llpanelobject` places them,
    // plus the shape editors), Features, and the placeholder Texture /
    // Content pages whose contents are their own roadmap tasks.
    let tab_labels: [String; 5] = [
        "build-tab-general".to_owned(),
        "build-tab-object".to_owned(),
        "build-tab-features".to_owned(),
        "build-tab-texture".to_owned(),
        "build-tab-content".to_owned(),
    ];
    let tabs = spawn_tab_container(
        &mut commands,
        content,
        &TabSpec {
            element: "build-tabs",
            placement: TabPlacement::BlockStart,
            labels: &tab_labels,
            active: 0,
            tab_index: 20,
            font_size: TOOL_FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    // The floater is resizable (a definite content area), so the widget must
    // track it rather than content-size — the bar widens with the window and
    // the panels grow and scroll (the profile floater's arrangement).
    fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);
    // Inside each container panel, a page wrapper carrying `UiPanelShown`:
    // the container toggles panels with `Visibility` (they stay laid out),
    // which alone would leave a hidden page's fields Tab-reachable — the
    // wrapper's `UiPanelShown` parks their `TabIndex` stops and drops focus
    // ([`sync_tab_pages`] mirrors the strip's active tab into it).
    let mut tab_pages = [Entity::PLACEHOLDER; 5];
    for (index, panel) in tabs.panels.iter().enumerate() {
        let page = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    ..column(Val::Px(4.0))
                },
                UiPanelShown(index == 0),
                Name::new(format!("build-tab-page:{index}")),
                ChildOf(*panel),
            ))
            .id();
        // The not-yet-implemented tabs carry a placeholder line; the General
        // / Object / Features pages get their editors from the shell below
        // and the parameter-tab module ([`crate::edit_params`]).
        if index >= 3 {
            commands.spawn((
                Text::default(),
                Translated::new("build-tab-placeholder"),
                UiFont::Sans.at(TOOL_FONT_SIZE),
                // A skinless fallback; the skin recolours via the class token.
                TextColor(Color::srgba(0.6, 0.6, 0.6, 1.0)),
                ClassList::new_with_classes([PLACEHOLDER_CLASS]),
                ChildOf(page),
            ));
        }
        if let Some(slot) = tab_pages.get_mut(index) {
            *slot = page;
        }
    }
    let tab_strip = tabs.strip;

    // The three transform rows, inside the Object tab (the reference's
    // `llpanelobject` position / rotation / size spinners).
    let object_page = tab_pages.get(1).copied().unwrap_or(content);
    let mut fields = [Entity::PLACEHOLDER; 9];
    for (group_index, (group, key)) in [
        (FieldGroup::Position, "build-position-label"),
        (FieldGroup::Rotation, "build-rotation-label"),
        (FieldGroup::Size, "build-size-label"),
    ]
    .into_iter()
    .enumerate()
    {
        let transform_row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(4.0),
                    ..row(Val::Px(4.0))
                },
                ChildOf(object_page),
            ))
            .id();
        spawn_row_label(&mut commands, transform_row, key);
        for axis in 0_usize..3_usize {
            let element = match group {
                FieldGroup::Position => "build-pos",
                FieldGroup::Rotation => "build-rot",
                FieldGroup::Size => "build-size",
            };
            let slot_index = group_index.saturating_mul(3).saturating_add(axis);
            let field = spawn_text_input(
                &mut commands,
                transform_row,
                &TextInputSpec {
                    font_size: TOOL_FONT_SIZE,
                    width_glyphs: FIELD_WIDTH_GLYPHS,
                    tab_index: i32::try_from(slot_index.saturating_add(21)).unwrap_or(21),
                    ..TextInputSpec::new(element, TextInputKind::Float)
                },
            );
            commands
                .entity(field)
                .insert(BuildNumericField { group, axis });
            if let Some(slot) = fields.get_mut(slot_index) {
                *slot = field;
            }
        }
    }

    commands.insert_resource(BuildTabPages {
        general: tab_pages.first().copied().unwrap_or(content),
        object: tab_pages.get(1).copied().unwrap_or(content),
        features: tab_pages.get(2).copied().unwrap_or(content),
    });
    commands.insert_resource(BuildToolsUi {
        panel: handle.root,
        fields,
        grid_field,
        summary_text,
        tab_strip,
        tab_pages,
    });
}

/// Spawn one tool button (Move / Rotate / Stretch).
fn spawn_tool_button(
    commands: &mut Commands,
    parent: Entity,
    tool: EditTool,
    label_key: &'static str,
    tab_index: i32,
) {
    let button = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..row(Val::ZERO)
            },
            BorderColor::all(Color::srgba(0.4, 0.4, 0.45, 1.0)),
            BackgroundColor(TOOL_IDLE_COLOR),
            BuildToolButton(tool),
            Pickable::default(),
            Name::new(format!("build-tools:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.entity(button).observe(
        move |press: On<Pointer<Press>>, mut state: ResMut<EditToolState>| {
            if press.button == PointerButton::Primary {
                state.tool = tool;
            }
        },
    );
}

/// Spawn one toggle row (check glyph + label) flipping a [`BuildToggle`].
fn spawn_toggle_row(
    commands: &mut Commands,
    parent: Entity,
    toggle: BuildToggle,
    label_key: &'static str,
    tab_index: i32,
) {
    let toggle_row = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(tab_index),
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Pickable::default(),
            toggle,
            Name::new(format!("build-tools:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(UNCHECKED_GLYPH),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        BuildToggleGlyph(toggle),
        Pickable::IGNORE,
        ChildOf(toggle_row),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        Pickable::IGNORE,
        ChildOf(toggle_row),
    ));
    commands.entity(toggle_row).observe(
        move |press: On<Pointer<Press>>, mut state: ResMut<EditToolState>| {
            if press.button == PointerButton::Primary {
                toggle.flip(&mut state);
            }
        },
    );
}

/// Spawn a translated row label.
pub(crate) fn spawn_row_label(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        // A skinless fallback; the skin recolours via the class token.
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        Node {
            min_width: Val::Px(64.0),
            ..Default::default()
        },
        ChildOf(parent),
    ));
}

/// `Ctrl+B` toggles the Build Tools floater (the reference's Build shortcut),
/// while the world or a plain widget owns the keyboard.
fn toggle_build_floater_on_ctrl_b(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    ui: Option<Res<BuildToolsUi>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    if *context == InputContext::TextEntry {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyB)) {
        return;
    }
    if let Some(ui) = ui
        && let Ok(mut shown) = panels.get_mut(ui.panel)
    {
        shown.0 = !shown.0;
    }
}

/// Mirror the floater's visibility into [`EditToolState::active`] — an open
/// Build Tools window *is* edit mode — and clear the selection when it closes
/// (which also deselects on the wire).
fn mirror_floater_into_state(
    ui: Option<Res<BuildToolsUi>>,
    panels: Query<&UiPanelShown>,
    mut state: ResMut<EditToolState>,
    mut selection: ResMut<SelectionSet>,
) {
    let shown = ui
        .and_then(|ui| panels.get(ui.panel).ok().map(|shown| shown.0))
        .unwrap_or(false);
    if state.active != shown {
        state.active = shown;
        if !shown && !selection.is_empty() {
            selection.clear();
        }
    }
}

/// Tint the active tool's button.
fn update_tool_button_visuals(
    state: Res<EditToolState>,
    mut buttons: Query<(&BuildToolButton, &mut BackgroundColor)>,
) {
    if !state.is_changed() {
        return;
    }
    // The lit button follows the *effective* tool, so a held `Ctrl` /
    // `Ctrl+Shift` chord is visible in the floater too.
    for (button, mut background) in &mut buttons {
        background.0 = if button.0 == state.effective_tool() {
            TOOL_ACTIVE_COLOR
        } else {
            TOOL_IDLE_COLOR
        };
    }
}

/// Keep the toggle rows' check glyphs in step with the state.
fn update_toggle_glyphs(
    state: Res<EditToolState>,
    mut glyphs: Query<(&BuildToggleGlyph, &mut Text)>,
) {
    if !state.is_changed() {
        return;
    }
    for (glyph, mut text) in &mut glyphs {
        let want = if glyph.0.get(&state) {
            CHECKED_GLYPH
        } else {
            UNCHECKED_GLYPH
        };
        if text.0 != want {
            want.clone_into(&mut text.0);
        }
    }
}

/// Show the active tab's page (and hide — focus-park — the rest).
fn sync_tab_pages(
    ui: Option<Res<BuildToolsUi>>,
    strips: Query<&TabStrip, Changed<TabStrip>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(strip) = strips.get(ui.tab_strip) else {
        return;
    };
    for (index, page) in ui.tab_pages.iter().enumerate() {
        if let Ok(mut shown) = panels.get_mut(*page) {
            let want = index == strip.active;
            if shown.0 != want {
                shown.0 = want;
            }
        }
    }
}

/// Rewrite the selection-summary line: how many objects are selected, the
/// primary's name (from its `ObjectProperties` reply), and a no-modify warning
/// when the primary is not modifiable by the agent.
fn update_selection_summary(
    ui: Option<Res<BuildToolsUi>>,
    selection: Res<SelectionSet>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(mut text) = texts.get_mut(ui.summary_text) else {
        return;
    };
    let want = if selection.is_empty() {
        translator.get("build-selection-none")
    } else {
        let count = i64::try_from(selection.len()).unwrap_or(i64::MAX);
        let mut line = translator.format(
            "build-selection-count",
            &TransArgs::new().int("count", count),
        );
        if let Some(properties) = selection
            .primary()
            .and_then(|primary| primary.properties.as_ref())
        {
            if !properties.name.is_empty() {
                line.push_str(" — ");
                line.push_str(&properties.name);
            }
            if !properties.permissions.owner.contains(Permissions::MODIFY) {
                line.push_str(" — ");
                line.push_str(&translator.get("build-selection-no-modify"));
            }
        }
        line
    };
    if text.0 != want {
        text.0 = want;
    }
}

/// Format a metre / degree value the way the fields display it.
fn format_metres(value: f32) -> String {
    format!("{value:.3}")
}

/// Normalize an angle in degrees into `[0, 360)` for display, the reference
/// build floater's convention.
fn display_degrees(value: f32) -> f32 {
    let wrapped = value.rem_euclid(360.0);
    if wrapped.is_finite() { wrapped } else { 0.0 }
}

/// The primary selection's current row values, in display order X / Y / Z.
fn group_values(motion: &ObjectSlMotion, group: FieldGroup) -> [f32; 3] {
    match group {
        FieldGroup::Position => [motion.position.x, motion.position.y, motion.position.z],
        FieldGroup::Rotation => {
            let euler = rotation_to_euler_deg(&motion.rotation);
            [
                display_degrees(euler[0]),
                display_degrees(euler[1]),
                display_degrees(euler[2]),
            ]
        }
        FieldGroup::Size => [motion.scale.x, motion.scale.y, motion.scale.z],
    }
}

/// Keep the nine transform fields (and the grid-unit field) displaying the
/// live values — skipping whichever field the user is editing, so typing is
/// never clobbered mid-edit.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the UI handles, \
              the tool / selection / focus state, the motion mirror, the field queries, and the \
              text-layout contexts a programmatic field rewrite needs"
)]
fn sync_numeric_fields(
    ui: Option<Res<BuildToolsUi>>,
    state: Res<EditToolState>,
    selection: Res<SelectionSet>,
    focus: Res<InputFocus>,
    motions: Query<&ObjectSlMotion>,
    markers: Query<&BuildNumericField>,
    mut editors: Query<&mut EditableText>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !state.active {
        return;
    }
    let primary_motion = selection
        .primary()
        .and_then(|node| motions.get(node.entity).ok());
    for field in ui.fields {
        if focus.get() == Some(field) {
            continue;
        }
        let Ok(marker) = markers.get(field) else {
            continue;
        };
        let want = primary_motion.map_or_else(String::new, |motion| {
            let values = group_values(motion, marker.group);
            format_metres(*values.get(marker.axis).unwrap_or(&0.0))
        });
        if let Ok(mut editor) = editors.get_mut(field)
            && editor.value().to_string() != want
        {
            set_editor_text(&mut editor, &want, &mut font_cx, &mut layout_cx);
        }
    }
    // The grid-unit field mirrors the state (it can be changed by a future
    // grid-options floater too).
    if focus.get() != Some(ui.grid_field) {
        let want = format_metres(state.grid_unit);
        if let Ok(mut editor) = editors.get_mut(ui.grid_field)
            && editor.value().to_string() != want
        {
            set_editor_text(&mut editor, &want, &mut font_cx, &mut layout_cx);
        }
    }
}

/// Which field held keyboard focus last frame, to commit a numeric edit on
/// focus loss (blur) as well as on `Enter`.
#[derive(Resource, Debug, Default)]
struct BuildFieldFocus {
    /// The field entity focused last frame, if any.
    last: Option<Entity>,
}

/// Commit numeric edits: on `Enter` in a focused transform field, or when
/// focus leaves one, parse its row and send the corresponding
/// `MultipleObjectUpdate` for the primary selection — the exact command the
/// gizmos send. The grid-unit field commits into [`EditToolState`] instead.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the UI handles, \
              the tool / selection / focus state, the field queries, and the outgoing command \
              writer plus the local write-back queries"
)]
fn commit_numeric_fields(
    ui: Option<Res<BuildToolsUi>>,
    mut state: ResMut<EditToolState>,
    selection: Res<SelectionSet>,
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus_track: ResMut<BuildFieldFocus>,
    markers: Query<&BuildNumericField>,
    editors: Query<&EditableText>,
    objects: Res<ObjectState>,
    mut motions: Query<(&mut ObjectSlMotion, &SceneObject)>,
    mut transforms: crate::gizmos::EditTransformQuery,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    let focused_field = focus
        .get()
        .filter(|entity| ui.fields.contains(entity) || *entity == ui.grid_field);
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    // The field to commit: the focused one on Enter, or the one focus just
    // left.
    let commit = if enter {
        focused_field
    } else if focus_track.last != focused_field {
        focus_track
            .last
            .filter(|entity| ui.fields.contains(entity) || *entity == ui.grid_field)
    } else {
        None
    };
    focus_track.last = focused_field;
    let Some(field) = commit else {
        return;
    };

    // Grid unit.
    if field == ui.grid_field {
        if let Ok(editor) = editors.get(field)
            && let Some(value) = parse_field(&editor.value().to_string())
        {
            state.grid_unit = value.clamp(0.01, 10.0);
        }
        return;
    }

    let Ok(marker) = markers.get(field) else {
        return;
    };
    let Some(primary) = selection.primary() else {
        return;
    };
    // Parse the whole row (all three axes) so a single-axis edit keeps its
    // siblings' displayed values.
    let mut values = [0.0_f32; 3];
    for axis in 0_usize..3_usize {
        let Some(entity) = ui.fields.iter().copied().find(|entity| {
            markers
                .get(*entity)
                .is_ok_and(|m| m.group == marker.group && m.axis == axis)
        }) else {
            return;
        };
        let Ok(editor) = editors.get(entity) else {
            return;
        };
        let Some(value) = parse_field(&editor.value().to_string()) else {
            return;
        };
        if let Some(slot) = values.get_mut(axis) {
            *slot = value;
        }
    }

    let [x, y, z] = values;
    let transform = match marker.group {
        FieldGroup::Position => ObjectTransform {
            position: Some(Vector { x, y, z }),
            group: !state.edit_linked,
            ..Default::default()
        },
        FieldGroup::Rotation => ObjectTransform {
            rotation: Some(euler_deg_to_rotation(values)),
            group: !state.edit_linked,
            ..Default::default()
        },
        FieldGroup::Size => ObjectTransform {
            scale: Some(Vector {
                x: clamp_scale(x),
                y: clamp_scale(y),
                z: clamp_scale(z),
            }),
            group: !state.edit_linked,
            ..Default::default()
        },
    };
    // Local echo, so the scene (and the gizmo) follows immediately; the
    // simulator's own update confirms it.
    if let Ok((mut motion, scene)) = motions.get_mut(primary.entity) {
        match marker.group {
            FieldGroup::Position => {
                motion.position = Vector { x, y, z };
            }
            FieldGroup::Rotation => motion.rotation = euler_deg_to_rotation(values),
            FieldGroup::Size => {
                motion.scale = Vector {
                    x: clamp_scale(x),
                    y: clamp_scale(y),
                    z: clamp_scale(z),
                };
            }
        }
        crate::gizmos::write_back_motion(
            &motion,
            scene,
            primary.entity,
            objects.geometry_of(&primary.scoped),
            &mut transforms,
        );
    }
    debug!(
        "build-tools: numeric {:?} commit on {:?}",
        marker.group, primary.scoped
    );
    commands.write(SlCommand(Command::UpdateObject {
        local_id: primary.scoped,
        transform,
    }));
}

/// Spawn the gallery specimen of the Build Tools panel: the static shape —
/// tool buttons, a toggle row, and one numeric transform row — with no live
/// behaviour, for the no-login gallery and the `ui_test` matrix.
pub(crate) fn spawn_build_tools_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(6.0))
            },
            Name::new("build-tools-specimen"),
            ChildOf(parent),
        ))
        .id();
    let tools = commands
        .spawn((
            Node {
                // Wraps rather than overflowing in a narrow window — the
                // row-level half of the content-driven convention.
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(6.0),
                ..row(Val::Px(6.0))
            },
            ChildOf(root),
        ))
        .id();
    for label in ["Move", "Rotate", "Stretch"] {
        commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..Default::default()
                },
                BorderColor::all(Color::srgba(0.4, 0.4, 0.45, 1.0)),
                BackgroundColor(if label == "Move" {
                    TOOL_ACTIVE_COLOR
                } else {
                    TOOL_IDLE_COLOR
                }),
                ChildOf(tools),
            ))
            .with_child((
                Text::new(cx.text(label)),
                cx.font(UiFont::Sans),
                TextColor(Color::WHITE),
            ));
    }
    let toggle = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(6.0),
                ..row(Val::Px(6.0))
            },
            ChildOf(root),
        ))
        .id();
    commands
        .spawn((Node::default(), ChildOf(toggle)))
        .with_child((
            Text::new(CHECKED_GLYPH),
            cx.font(UiFont::Sans),
            TextColor(Color::WHITE),
        ));
    commands
        .spawn((
            Node {
                max_width: Val::Px(220.0),
                ..Default::default()
            },
            ChildOf(toggle),
        ))
        .with_child((
            Text::new(cx.text("Snap to grid")),
            cx.font(UiFont::Sans),
            TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ));
    let transform_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            ChildOf(root),
        ))
        .id();
    commands
        .spawn((
            Node {
                min_width: Val::Px(64.0),
                max_width: Val::Px(220.0),
                ..Default::default()
            },
            ChildOf(transform_row),
        ))
        .with_child((
            Text::new(cx.text("Position")),
            cx.font(UiFont::Sans),
            TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ));
    for (element, value) in [
        ("build-specimen-x", "128.000"),
        ("build-specimen-y", "64.500"),
        ("build-specimen-z", "23.125"),
    ] {
        spawn_text_input(
            commands,
            transform_row,
            &TextInputSpec {
                initial: value.to_owned(),
                font_size: cx.font_size,
                width_glyphs: FIELD_WIDTH_GLYPHS,
                ..TextInputSpec::new(element, TextInputKind::Float)
            },
        );
    }
    root
}

/// Run condition: true while the build tool is **inactive** — the gate that
/// hands the left click back to the touch pick
/// ([`crate::hud_pick::pick_and_touch`]) outside edit mode.
pub(crate) fn edit_tool_inactive(state: Res<EditToolState>) -> bool {
    !state.active
}

/// Parse one numeric field's committed value.
fn parse_field(text: &str) -> Option<f32> {
    match TextInputKind::Float.parse(text.trim()) {
        Some(crate::ui_text_input::TextInputValue::Float(value)) => {
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "f64 → f32 narrowing at the field boundary; std has no lossless \
                          From/TryFrom for it, and the value is a bounded metre / degree entry"
            )]
            let value = value as f32;
            value.is_finite().then_some(value)
        }
        _other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildToggle, EditTool, EditToolState, GridFrame, display_degrees, group_values};
    use crate::objects::ObjectSlMotion;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Vector;

    /// The defaults are the reference's: move tool, snap on, half-metre grid,
    /// world frame, whole-linkset selection.
    #[test]
    fn reference_defaults() {
        let state = EditToolState::default();
        assert!(!state.active);
        assert_eq!(state.tool, EditTool::Move);
        assert!(state.snap);
        assert!(!state.edit_linked);
        assert!(!state.stretch_both);
        assert_eq!(state.frame, GridFrame::World);
        assert!((state.grid_unit - 0.5).abs() < 1.0e-6);
    }

    /// Each toggle flips its own flag, and the frame toggle round-trips
    /// world ↔ local.
    #[test]
    fn toggles_flip_their_flags() {
        let mut state = EditToolState::default();
        BuildToggle::Snap.flip(&mut state);
        assert!(!state.snap);
        BuildToggle::EditLinked.flip(&mut state);
        assert!(state.edit_linked);
        BuildToggle::StretchBoth.flip(&mut state);
        assert!(state.stretch_both);
        BuildToggle::LocalFrame.flip(&mut state);
        assert_eq!(state.frame, GridFrame::Local);
        assert!(BuildToggle::LocalFrame.get(&state));
        BuildToggle::LocalFrame.flip(&mut state);
        assert_eq!(state.frame, GridFrame::World);
    }

    /// The held-modifier chords override the resting tool while held —
    /// `Ctrl+Shift` (stretch) beats `Ctrl` (rotate) — and release restores
    /// the floater's choice.
    #[test]
    fn modifier_override_precedence() {
        let mut state = EditToolState::default();
        assert_eq!(state.effective_tool(), EditTool::Move);
        state.held_override = Some(EditTool::Rotate);
        assert_eq!(state.effective_tool(), EditTool::Rotate);
        state.held_override = Some(EditTool::Stretch);
        assert_eq!(state.effective_tool(), EditTool::Stretch);
        state.held_override = None;
        assert_eq!(state.effective_tool(), EditTool::Move);
        // The resting tool shows through once released, whatever it was.
        state.tool = EditTool::Rotate;
        assert_eq!(state.effective_tool(), EditTool::Rotate);
    }

    /// The reference-object grid frame is part of the model from the start
    /// (the grid-options task makes it settable); it is its own distinct
    /// frame.
    #[test]
    fn reference_frame_is_modelled() {
        assert!(GridFrame::Reference != GridFrame::World);
        assert!(GridFrame::Reference != GridFrame::Local);
    }

    /// Angles display normalized to `[0, 360)`.
    #[test]
    fn degrees_display_normalized() {
        assert!((display_degrees(-90.0) - 270.0).abs() < 1.0e-4);
        assert!((display_degrees(370.0) - 10.0).abs() < 1.0e-4);
        assert!(display_degrees(0.0).abs() < 1.0e-6);
    }

    /// The row values come off the mirrored Second Life motion, rotations as
    /// display-normalized Euler degrees.
    #[test]
    fn group_values_read_the_motion() {
        let motion = ObjectSlMotion {
            position: Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            rotation: crate::edit_math::euler_deg_to_rotation([0.0, 0.0, -90.0]),
            scale: Vector {
                x: 0.5,
                y: 1.5,
                z: 2.5,
            },
            is_root: true,
            attachment: false,
        };
        let close = |got: [f32; 3], want: [f32; 3]| {
            got.into_iter()
                .zip(want)
                .all(|(a, b)| (a - b).abs() < 1.0e-6)
        };
        assert!(close(
            group_values(&motion, super::FieldGroup::Position),
            [1.0, 2.0, 3.0]
        ));
        assert!(close(
            group_values(&motion, super::FieldGroup::Size),
            [0.5, 1.5, 2.5]
        ));
        let [roll, pitch, yaw] = group_values(&motion, super::FieldGroup::Rotation);
        assert!(roll.abs() < 1.0e-3);
        assert!(pitch.abs() < 1.0e-3);
        assert!((yaw - 270.0).abs() < 1.0e-2);
    }
}
