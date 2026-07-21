//! The viewer's **bottom toolbar** (`viewer-ui-bottom-toolbar`): the persistent
//! strip of toggle buttons along the bottom edge that open the main floaters
//! (Inventory, Appearance, Map, People, …), plus the **bottom-area layout host**
//! the other bottom-edge controls hang off.
//!
//! # Names now, floaters as they land
//!
//! Like [`crate::menu_bar`], this is the *bar*, not the finished set of windows
//! it opens. It stands up the reference viewer's toolbar buttons in their usual
//! arrangement, so every future floater task has a home to hang its toggle in,
//! and wires only the ones that already have a live target: today just
//! **Inventory** (the window [`crate::inventory`] already ships). A button whose
//! floater has not landed yet is shown **disabled** — a greyed, non-focusable
//! placeholder, exactly like the top menu bar's placeholder entries — so the bar
//! reads as the reference's familiar persistent toolbar while being honest that
//! most toggles are not wired yet. A future task flips one from
//! [`ToolbarTarget::Unlanded`] to a real target in [`TOOLBAR_BUTTONS`] and adds
//! its branch to [`handle_toolbar_actions`]; nothing else here changes.
//!
//! Each wired button is a **toggle**: pressing it flips its floater's
//! [`UiPanelShown`], and the button lights (an active/pressed background) for as
//! long as that floater is open — the reference toolbar's down state, driven from
//! the same read-model the menu bar's Inventory check mark reads.
//!
//! # The bottom area is a host, not just this bar
//!
//! The reference viewer stacks several other controls **above** the button bar —
//! the nearby-chat input bar ([[viewer-chat-input-bar]]), the audio / volume
//! control ([[viewer-volume-panel]]), the voice talk button
//! ([[viewer-voice-audio]], signalling only), and quick preferences
//! ([[viewer-quick-preferences]]) — each of which is its own task. This task owns
//! the **layout host** they fill: [`spawn_bottom_toolbar`] builds a
//! bottom-anchored column whose bottom-most row is the button bar and whose
//! *upper* stack ([`BottomArea::upper`], published as a resource) is where those
//! neighbour controls parent themselves, so they always sit above the buttons
//! regardless of the order they land in. (The button bar's "Conversations" toggle
//! opens the chat *window*; it is distinct from the always-visible nearby-chat
//! *input* bar that will live in the upper stack.)
//!
//! # Content-sized, wrapping, mirrored
//!
//! Per the scaffold's conventions the bar sizes to its content and, if the window
//! is too narrow for every button, **wraps upward** (`FlexWrap::WrapReverse`, so a
//! wrapped line stacks *above* rather than off the bottom of the screen) rather
//! than overflowing. The whole strip mirrors under a right-to-left locale for free
//! (the row follows the writing mode; the anchor is a [`LogicalInset`]). Every
//! label is resolved from a Fluent key through [`Translated`], never a baked-in
//! literal.
//!
//! # Constructible without its wiring
//!
//! Like every element ([`crate::ui_element`]), the bar is registered as a static
//! specimen ([`spawn_bottom_toolbar_specimen`]) the gallery / harness sweep across
//! every script, size and direction, with the live toggling left to the plugin.
//!
//! Reference (Firestorm, read-only): `llbottomtray` (the bottom tray container)
//! and `lltoolbar` (the toolbar buttons).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use crate::i18n::Translated;
use crate::inventory::InventoryUi;
use crate::nearby_chat_bar::NearbyChatBar;
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;

/// The `element` the bottom toolbar attributes its actions to — the tag
/// [`handle_toolbar_actions`] filters on, so it routes *its* buttons' presses and
/// not some other widget's.
pub(crate) const BOTTOM_TOOLBAR_ELEMENT: &str = "bottom-toolbar";

/// The z-index the bottom area renders at — above the floaters (so a window never
/// hides the persistent toolbar), matching the top menu bar's
/// [`crate::menu_bar::TOP_MENU_ELEMENT`] strip.
const BOTTOM_BAR_Z: i32 = 9_000;

/// The toolbar button / label font size, in logical pixels.
const TOOLBAR_FONT_SIZE: f32 = 13.0;

/// The gap between adjacent toolbar buttons, in logical pixels.
const BUTTON_GAP: f32 = 4.0;

/// The bar strip's fallback background, used when no skin is loaded — the skin's
/// `.sk-toolbar-bar` (`var(--surface-bg)`) overrides it. A dark, mostly-opaque
/// neutral so the buttons read against the world behind them.
const BAR_BACKGROUND: Color = Color::srgba(0.08, 0.09, 0.12, 0.92);

/// A button's border colour (the skin carries the corner radius via
/// `.sk-toolbar-button`; the background and text are painted in Rust so the
/// active / disabled states are one place, like the floater highlight).
const BUTTON_BORDER: Color = Color::srgb(0.30, 0.34, 0.42);

/// The CSS class on the bar strip, so a skin recolours its surface.
const BAR_CLASS: &str = "sk-toolbar-bar";

/// The CSS class on every toolbar button, carrying the skin's corner radius.
const BUTTON_CLASS: &str = "sk-toolbar-button";

/// How a toolbar button currently reads — the three visual states its background
/// and label colour are painted from, shared by the live state system
/// ([`update_toolbar_button_states`]) and the static specimen so the two never
/// drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolbarButtonVisual {
    /// Wired and its floater closed — the resting look.
    Enabled,
    /// Wired and its floater open — lit / pressed.
    Active,
    /// Not wired yet (its floater has not landed) — greyed and inert.
    Disabled,
}

impl ToolbarButtonVisual {
    /// This state's `(background, label)` colours. `const` so it is a plain table
    /// with no per-frame allocation, and the single source of truth both the live
    /// paint and the specimen read.
    const fn colors(self) -> (Color, Color) {
        match self {
            Self::Enabled => (Color::srgb(0.16, 0.19, 0.25), Color::srgb(0.90, 0.92, 0.96)),
            Self::Active => (Color::srgb(0.22, 0.40, 0.60), Color::srgb(0.97, 0.98, 1.0)),
            Self::Disabled => (
                Color::srgba(0.13, 0.15, 0.19, 0.65),
                Color::srgb(0.48, 0.51, 0.58),
            ),
        }
    }

    /// The background colour for this state.
    const fn background(self) -> Color {
        self.colors().0
    }

    /// The label colour for this state.
    const fn label(self) -> Color {
        self.colors().1
    }
}

/// Which floater / panel a toolbar button toggles.
///
/// An enum rather than an [`Entity`] because the button table
/// ([`TOOLBAR_BUTTONS`]) is a `static` known at compile time, while a floater's
/// entity is a runtime value; [`resolve_target_open`] bridges the two against the
/// live read-models each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolbarTarget {
    /// The nearby-chat bar ([`crate::nearby_chat_bar`]) — the leading toggle that
    /// shows / hides the local-chat input above the button bar (the reference's
    /// chat button).
    NearbyChat,
    /// The inventory window ([`crate::inventory`]), toggled today.
    Inventory,
    /// A floater that has not landed yet — the button is a disabled placeholder
    /// until its own task wires a real target here.
    Unlanded,
}

impl ToolbarTarget {
    /// Whether this target is wired to a live floater (so its button is
    /// interactive), as opposed to an unlanded placeholder.
    const fn is_wired(self) -> bool {
        !matches!(self, Self::Unlanded)
    }
}

/// One button on the bottom toolbar — its action string, its Fluent label key and
/// what it toggles.
#[derive(Debug, Clone, Copy)]
struct ToolbarButtonDef {
    /// The action string emitted as [`UiAction::action`], and the button's stable
    /// id.
    action: &'static str,
    /// The Fluent key its label resolves from.
    label_key: &'static str,
    /// The floater it toggles, or [`ToolbarTarget::Unlanded`] while none exists.
    target: ToolbarTarget,
}

/// The toolbar's buttons, in the reference viewer's left-to-right order. Only
/// Inventory has a live floater today; the rest are shown disabled until their
/// own tasks land, exactly as the top menu bar ships its menu *names* ahead of
/// their entries.
static TOOLBAR_BUTTONS: &[ToolbarButtonDef] = &[
    // The chat toggle leads the bar (leftmost under LTR, rightmost under RTL — the
    // row mirrors for free), as the reference viewer places its chat button.
    ToolbarButtonDef {
        action: "toggle-nearby-chat",
        label_key: "bottom-toolbar-chat",
        target: ToolbarTarget::NearbyChat,
    },
    ToolbarButtonDef {
        action: "toggle-inventory",
        label_key: "bottom-toolbar-inventory",
        target: ToolbarTarget::Inventory,
    },
    ToolbarButtonDef {
        action: "toggle-appearance",
        label_key: "bottom-toolbar-appearance",
        target: ToolbarTarget::Unlanded,
    },
    ToolbarButtonDef {
        action: "toggle-map",
        label_key: "bottom-toolbar-map",
        target: ToolbarTarget::Unlanded,
    },
    ToolbarButtonDef {
        action: "toggle-minimap",
        label_key: "bottom-toolbar-minimap",
        target: ToolbarTarget::Unlanded,
    },
    ToolbarButtonDef {
        action: "toggle-people",
        label_key: "bottom-toolbar-people",
        target: ToolbarTarget::Unlanded,
    },
    ToolbarButtonDef {
        action: "toggle-conversations",
        label_key: "bottom-toolbar-conversations",
        target: ToolbarTarget::Unlanded,
    },
    ToolbarButtonDef {
        action: "toggle-camera",
        label_key: "bottom-toolbar-camera",
        target: ToolbarTarget::Unlanded,
    },
];

/// A live toolbar button, carried on its box so the state system paints it and
/// the routing system knows what it toggles without a marker query per button.
#[derive(Component, Debug, Clone, Copy)]
struct ToolbarButton {
    /// What this button toggles.
    target: ToolbarTarget,
    /// The label text node, so [`update_toolbar_button_states`] can dim it in the
    /// disabled state.
    label: Entity,
}

/// The bottom-area layout host, published so the neighbour bottom-edge controls
/// (nearby chat bar, volume, voice, quick preferences — each its own task) parent
/// themselves **above** the button bar by spawning into [`upper`](Self::upper).
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct BottomArea {
    /// The bottom-anchored column that holds the whole area — the chat overlay
    /// ([`crate::chat`]) reads its measured height to sit just above it.
    pub(crate) area: Entity,
    /// The stack above the button bar the neighbour controls fill — the nearby-chat
    /// bar ([`crate::nearby_chat_bar`]) spawns into it.
    pub(crate) upper: Entity,
    /// The button-bar row itself. Still awaiting a consumer (a future control that
    /// needs the bar strip directly rather than the upper stack).
    #[expect(
        dead_code,
        reason = "the bar-strip handle is published for a future bottom-edge control that targets \
                  the button row directly; `area` and `upper` are now consumed"
    )]
    pub(crate) bar: Entity,
}

/// The bottom toolbar's runtime: spawn the bar, route its presses, and keep each
/// button's lit / disabled look current.
pub(crate) struct BottomToolbarPlugin;

impl Plugin for BottomToolbarPlugin {
    /// Wire the toolbar: spawn it once the [`UiRoot`] exists, then route presses
    /// and repaint button states each frame.
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            spawn_bottom_toolbar.after(UiScaffoldSystems::SpawnRoot),
        )
        .add_systems(
            Update,
            (handle_toolbar_actions, update_toolbar_button_states),
        );
    }
}

/// Spawn the bottom area and its button bar under the UI root, and publish the
/// [`BottomArea`] host.
///
/// The area is an **absolute**, full-width column pinned to the bottom edge (a
/// [`LogicalInset`] at `block_end` / both inline edges zero, so it mirrors under
/// RTL): an *upper* stack for the neighbour controls above, then the button-bar
/// row below it. The bar wraps upward when it is too narrow for every button.
fn spawn_bottom_toolbar(mut commands: Commands, root: Res<UiRoot>) {
    let area = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                // Full width so a neighbour control (the chat bar) can span the
                // row; the button bar centres its own content within it.
                width: Val::Percent(100.0),
                ..column(Val::ZERO)
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(0.0),
                inline_end: Val::Px(0.0),
                block_end: Val::Px(0.0),
                ..LogicalRect::AUTO
            }),
            GlobalZIndex(BOTTOM_BAR_Z),
            // Transparent and non-blocking: only the visible bar strip below takes
            // clicks off the world, so the (empty) upper stack does not swallow
            // pointer hits aimed at the scene.
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("bottom-area"),
            ChildOf(root.0),
        ))
        .id();

    // The upper stack the neighbour controls parent into — above the button bar,
    // full width, empty (and so zero-height) until one lands.
    let upper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("bottom-area-upper"),
            ChildOf(area),
        ))
        .id();

    // The button bar itself — the bottom-most strip. A full-width surface (so it
    // reads as one bar the width of the window, the reference's arrangement) whose
    // buttons are centred and wrap upward if the window is too narrow.
    let bar = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::WrapReverse,
                // The gap between wrapped lines (the axis `row`'s `column_gap`
                // cannot mean).
                row_gap: Val::Px(BUTTON_GAP),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                ..row(Val::Px(BUTTON_GAP))
            },
            BackgroundColor(BAR_BACKGROUND),
            ClassList::new_with_classes([BAR_CLASS]),
            // The visible strip blocks the world behind it, as a real toolbar does.
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("bottom-toolbar"),
            ChildOf(area),
        ))
        .id();

    for (index, def) in TOOLBAR_BUTTONS.iter().enumerate() {
        spawn_live_button(&mut commands, bar, index, def);
    }

    commands.insert_resource(BottomArea { area, upper, bar });
}

/// Spawn one **live** toolbar button under the bar: the box, its Fluent-bound
/// label, and — for a wired target — the focusable [`Button`] and the observer
/// that emits its [`UiAction`]. An unlanded target is spawned inert (no
/// [`Button`], no [`TabIndex`], no observer) and painted disabled.
fn spawn_live_button(commands: &mut Commands, bar: Entity, index: usize, def: &ToolbarButtonDef) {
    let wired = def.target.is_wired();
    let visual = if wired {
        ToolbarButtonVisual::Enabled
    } else {
        ToolbarButtonVisual::Disabled
    };
    let (button, label) = build_button_box(commands, bar, def.action, visual);

    // The label resolves from its Fluent key each frame ([`Translated`]), so it
    // fills in once the bundle loads and re-resolves on a locale switch.
    commands
        .entity(label)
        .insert(Translated::new(def.label_key));

    commands.entity(button).insert(ToolbarButton {
        target: def.target,
        label,
    });

    if wired {
        // Focusable and keyboard-activatable, in bar order.
        let tab_index = i32::try_from(index).unwrap_or(0).saturating_add(1);
        let action = def.action;
        commands
            .entity(button)
            .insert((Button, TabIndex(tab_index)))
            .observe(
                move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                    actions.write(UiAction {
                        element: BOTTOM_TOOLBAR_ELEMENT,
                        action,
                    });
                },
            );
    }
}

/// Build a toolbar button's box and label text node, returning `(box, label)`.
///
/// The shared half of the live button and the specimen: a padded, bordered box
/// (the skin carries its corner via [`BUTTON_CLASS`]) with a centred label as a
/// plain child — the text carries no decoration of its own, per the
/// text-measure caveat. `label` is passed already resolved (the specimen's swept
/// sample); the live path leaves it empty and binds a [`Translated`] key over it.
fn build_button_box(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    visual: ToolbarButtonVisual,
) -> (Entity, Entity) {
    let button = commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(visual.background()),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Pickable::default(),
            Name::new(format!("bottom-toolbar-button:{name}")),
            ChildOf(parent),
        ))
        .id();
    let label = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(TOOLBAR_FONT_SIZE),
            TextColor(visual.label()),
            Name::new("bottom-toolbar-label"),
            ChildOf(button),
        ))
        .id();
    (button, label)
}

/// Route the toolbar's presses to their live floaters.
///
/// Only the wired actions do anything today; an unlanded button emits no
/// [`UiAction`] at all (it is not a [`Button`]), and any future action string
/// added to [`TOOLBAR_BUTTONS`] before its handler simply falls through here — the
/// same harmless dispatch the top menu bar relies on. A `match` will earn its keep
/// once a second target is wired; today it is one equality check.
fn handle_toolbar_actions(
    mut actions: MessageReader<UiAction>,
    inventory: Option<Res<InventoryUi>>,
    mut nearby_chat: Option<ResMut<NearbyChatBar>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    for action in actions.read() {
        if action.element != BOTTOM_TOOLBAR_ELEMENT {
            continue;
        }
        if action.action == "toggle-inventory"
            && let Some(ui) = &inventory
            && let Ok(mut shown) = panels.get_mut(ui.panel())
        {
            shown.0 = !shown.0;
        }
        if action.action == "toggle-nearby-chat"
            && let Some(bar) = nearby_chat.as_deref_mut()
        {
            bar.toggle();
        }
    }
}

/// Resolve whether a button's target floater is currently open, or `None` when the
/// target is unlanded (so it stays disabled).
fn resolve_target_open(
    target: ToolbarTarget,
    inventory: Option<&InventoryUi>,
    nearby_chat: Option<&NearbyChatBar>,
    panels: &Query<&UiPanelShown>,
) -> Option<bool> {
    match target {
        ToolbarTarget::NearbyChat => nearby_chat.map(NearbyChatBar::is_shown),
        ToolbarTarget::Inventory => inventory
            .and_then(|ui| panels.get(ui.panel()).ok())
            .map(|shown| shown.0),
        ToolbarTarget::Unlanded => None,
    }
}

/// Keep each toolbar button's look current: lit while its floater is open, resting
/// while closed, greyed while unlanded — writing through change detection only on
/// a real change so an idle bar does not re-trigger layout.
fn update_toolbar_button_states(
    inventory: Option<Res<InventoryUi>>,
    nearby_chat: Option<Res<NearbyChatBar>>,
    mut buttons: Query<(&ToolbarButton, &mut BackgroundColor)>,
    panels: Query<&UiPanelShown>,
    mut labels: Query<&mut TextColor>,
) {
    let inventory = inventory.as_deref();
    let nearby_chat = nearby_chat.as_deref();
    for (button, mut background) in &mut buttons {
        let visual = match resolve_target_open(button.target, inventory, nearby_chat, &panels) {
            Some(true) => ToolbarButtonVisual::Active,
            Some(false) => ToolbarButtonVisual::Enabled,
            None => ToolbarButtonVisual::Disabled,
        };
        let bg = visual.background();
        if background.0 != bg {
            background.0 = bg;
        }
        let label = TextColor(visual.label());
        if let Ok(mut color) = labels.get_mut(button.label)
            && *color != label
        {
            *color = label;
        }
    }
}

// ---------------------------------------------------------------------------
// Registry specimen
// ---------------------------------------------------------------------------

/// Spawn a **static** bottom-toolbar specimen for the gallery / harness: the bar
/// strip with an enabled, an active (lit) and a disabled button, so all three
/// button states' layouts are swept across every script, size and direction.
///
/// In flow (not the live bar's absolute placement) so the harness measures it like
/// any other card, and with its labels drawn from the swept sample rather than a
/// Fluent key. The buttons still carry the same [`UiAction`]-emitting observer, so
/// a click is real in the viewer and inert in the gallery — by construction, not
/// by stubbing.
pub(crate) fn spawn_bottom_toolbar_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let bar = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::WrapReverse,
                row_gap: Val::Px(BUTTON_GAP),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                ..row(Val::Px(BUTTON_GAP))
            },
            BackgroundColor(BAR_BACKGROUND),
            ClassList::new_with_classes([BAR_CLASS]),
            Name::new("bottom-toolbar"),
            ChildOf(parent),
        ))
        .id();
    for (index, (label, action, visual)) in [
        (
            "Inventory",
            "toggle-inventory",
            ToolbarButtonVisual::Enabled,
        ),
        (
            "Appearance",
            "toggle-appearance",
            ToolbarButtonVisual::Active,
        ),
        ("Camera", "toggle-camera", ToolbarButtonVisual::Disabled),
    ]
    .into_iter()
    .enumerate()
    {
        let (button, label_node) = build_button_box(commands, bar, action, visual);
        commands
            .entity(label_node)
            .insert(Text::new(cx.text(label)));
        // Only the interactive states carry the press wiring, mirroring the live
        // bar (a disabled placeholder is not a `Button`).
        if visual != ToolbarButtonVisual::Disabled {
            let tab_index = i32::try_from(index).unwrap_or(0).saturating_add(1);
            commands
                .entity(button)
                .insert((Button, TabIndex(tab_index)))
                .observe(
                    move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                        actions.write(UiAction {
                            element: BOTTOM_TOOLBAR_ELEMENT,
                            action,
                        });
                    },
                );
        }
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::{TOOLBAR_BUTTONS, ToolbarButtonVisual, ToolbarTarget};
    use pretty_assertions::{assert_eq, assert_ne};

    /// The wired toolbar buttons today are the leading nearby-chat toggle and
    /// Inventory (in that order); the rest are unlanded placeholders. A regression
    /// that silently disabled a live toggle, or wired a target that does not exist,
    /// would trip here. The chat toggle leads the bar, as the reference places it.
    #[test]
    fn nearby_chat_and_inventory_are_wired() {
        let wired: Vec<&str> = TOOLBAR_BUTTONS
            .iter()
            .filter(|def| def.target.is_wired())
            .map(|def| def.action)
            .collect();
        assert_eq!(wired, ["toggle-nearby-chat", "toggle-inventory"]);
        assert!(
            TOOLBAR_BUTTONS
                .iter()
                .any(|def| def.target == ToolbarTarget::Inventory),
        );
        // The chat toggle is the first (leading) button.
        assert_eq!(
            TOOLBAR_BUTTONS.first().map(|def| def.action),
            Some("toggle-nearby-chat"),
        );
    }

    /// Action strings are the buttons' stable ids and what a press routes on, so a
    /// duplicate would make two buttons indistinguishable to
    /// [`super::handle_toolbar_actions`].
    #[test]
    fn button_actions_are_unique() {
        let mut actions: Vec<&str> = TOOLBAR_BUTTONS.iter().map(|def| def.action).collect();
        let total = actions.len();
        actions.sort_unstable();
        actions.dedup();
        assert_eq!(actions.len(), total, "two toolbar buttons share an action");
    }

    /// Every button has a non-empty Fluent label key — an empty key would resolve
    /// to nothing and leave a blank button.
    #[test]
    fn every_button_has_a_label_key() {
        for def in TOOLBAR_BUTTONS {
            assert!(!def.label_key.is_empty(), "{}: empty label key", def.action);
        }
    }

    /// The three visual states are visually distinct — the active button must not
    /// read the same as a resting or a disabled one, or the "floater is open"
    /// feedback is invisible.
    #[test]
    fn the_visual_states_differ() {
        let enabled = ToolbarButtonVisual::Enabled.background();
        let active = ToolbarButtonVisual::Active.background();
        let disabled = ToolbarButtonVisual::Disabled.background();
        assert_ne!(enabled, active);
        assert_ne!(enabled, disabled);
        assert_ne!(active, disabled);
    }
}
