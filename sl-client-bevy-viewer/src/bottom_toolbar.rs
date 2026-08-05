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
//! bottom-anchored column whose bottom-most row is the button bar and, just above
//! it, a single full-width **upper row** split into two fixed halves — a
//! **leading** slot ([`BottomArea::upper_leading`]) that holds the nearby-chat
//! bar and a **trailing** slot ([`BottomArea::upper_trailing`]) that holds the
//! parcel music / nearby-media cluster (and the volume, voice and quick-prefs
//! controls as they land). Both halves sit on the same row directly on top of the
//! button bar and do not overlap horizontally, so a control appearing or
//! disappearing in one half (e.g. the music cluster when a parcel has a stream)
//! never moves the other — the bug `viewer-music-controls-push-chat-bar`, where a
//! shared vertical stack let the music row shove the chat bar upward. The two
//! slots are spawned in a fixed order, so which half a control lands in never
//! depends on the order the neighbour plugins spawn in. (The button bar's
//! "Conversations" toggle opens the chat *window*; it is distinct from the
//! always-visible nearby-chat *input* bar in the leading slot.)
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

use crate::conversations::{BLINK_HZ, CONVERSATIONS_FLOATER_ID, ConversationModel};
use crate::edit_tool::BUILD_TOOLS_FLOATER_ID;
use crate::floater::{Floater, floater_panel};
use crate::i18n::Translated;
use crate::inventory::INVENTORY_FLOATER_ID;
use crate::minimap::MINIMAP_FLOATER_ID;
use crate::nearby_chat_bar::NearbyChatBar;
use crate::search::SEARCH_FLOATER_ID;
use crate::snapshot_floater::SNAPSHOT_FLOATER_ID;
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::world_map::WORLD_MAP_FLOATER_ID;

/// The `element` the bottom toolbar attributes its actions to — the tag
/// [`handle_toolbar_actions`] filters on, so it routes *its* buttons' presses and
/// not some other widget's.
pub(crate) const BOTTOM_TOOLBAR_ELEMENT: &str = "bottom-toolbar";

/// The z-index the bottom area renders at — above the floaters (so a window never
/// hides the persistent toolbar), matching the top menu bar's
/// [`crate::menu_bar::TOP_MENU_ELEMENT`] strip. `pub(crate)` so the Conversations
/// floater's bottom-left dock host can lift a docked window just above the bar (a
/// window docked *against* the bar must still take clicks, else its input reads as
/// dead — the bar would win the pick at `9000` over a docked floater at `0`).
pub(crate) const BOTTOM_BAR_Z: i32 = 9_000;

/// The toolbar button / label font size, in logical pixels.
const TOOLBAR_FONT_SIZE: f32 = 13.0;

/// The gap between adjacent toolbar buttons, in logical pixels.
const BUTTON_GAP: f32 = 4.0;

/// The fixed width of the reserved **state slot** at the bar's leading edge (and
/// the matching trailing spacer that balances it), in logical pixels. Wide enough
/// for the "Stand Up" / "Stop flycam" state button ([`crate::stand_stop_button`])
/// it hosts on **one line** (the longer "Stop flycam" plus the button's padding).
/// A fixed width — occupied or empty — so the button appearing or disappearing
/// never reflows the centred toolbar buttons, and the trailing spacer keeps that
/// centre truly centred rather than nudged by the leading slot.
const STATE_SLOT_WIDTH: f32 = 140.0;

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
    /// The Conversations floater ([`crate::conversations`]) — nearby chat, IMs,
    /// group chats and conferences.
    Conversations,
    /// The minimap floater ([`crate::minimap`]).
    Minimap,
    /// The world-map floater ([`crate::world_map`]).
    WorldMap,
    /// The Search floater ([`crate::search`]) — the directory search.
    Search,
    /// The Build Tools floater ([`crate::edit_tool`]) — opening it enters edit
    /// mode with nothing selected yet.
    BuildTools,
    /// The snapshot floater ([`crate::snapshot_floater`]) — the photo tool, opened
    /// from the Snapshot button (the reference's toolbar `snapshot` command).
    Snapshot,
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

    /// The stable floater id this button toggles, or `None` for the
    /// non-floater targets (the nearby-chat bar, an unlanded placeholder).
    ///
    /// The id — not the module's `XUi` resource — is what the toolbar resolves
    /// a floater through: a lazily-built floater
    /// ([`crate::floater::DeferredFloaterContent`]) has chrome (and so a
    /// [`Floater`] with this id) from startup, while its resource only appears
    /// on first open, and the toolbar is exactly the opener that performs that
    /// first open.
    const fn floater_id(self) -> Option<&'static str> {
        match self {
            Self::Inventory => Some(INVENTORY_FLOATER_ID),
            Self::Conversations => Some(CONVERSATIONS_FLOATER_ID),
            Self::Minimap => Some(MINIMAP_FLOATER_ID),
            Self::WorldMap => Some(WORLD_MAP_FLOATER_ID),
            Self::Search => Some(SEARCH_FLOATER_ID),
            Self::BuildTools => Some(BUILD_TOOLS_FLOATER_ID),
            Self::Snapshot => Some(SNAPSHOT_FLOATER_ID),
            Self::NearbyChat | Self::Unlanded => None,
        }
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

/// The toolbar's buttons, left-to-right. The two chat toggles lead the bar — the
/// always-visible nearby-chat *input* bar, then the Conversations *window*
/// (semantically the pair, so they sit together) — followed by the reference
/// viewer's remaining buttons. Only nearby chat, Conversations and Inventory have
/// a live target today; the rest are shown disabled until their own tasks land,
/// exactly as the top menu bar ships its menu *names* ahead of their entries.
static TOOLBAR_BUTTONS: &[ToolbarButtonDef] = &[
    // The chat toggle leads the bar (leftmost under LTR, rightmost under RTL — the
    // row mirrors for free), as the reference viewer places its chat button.
    ToolbarButtonDef {
        action: "toggle-nearby-chat",
        label_key: "bottom-toolbar-chat",
        target: ToolbarTarget::NearbyChat,
    },
    // Conversations sits right beside chat: the nearby-chat bar and the
    // conversation window are the same "talk to people" pair.
    ToolbarButtonDef {
        action: "toggle-conversations",
        label_key: "bottom-toolbar-conversations",
        target: ToolbarTarget::Conversations,
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
        target: ToolbarTarget::WorldMap,
    },
    ToolbarButtonDef {
        action: "toggle-minimap",
        label_key: "bottom-toolbar-minimap",
        target: ToolbarTarget::Minimap,
    },
    ToolbarButtonDef {
        action: "toggle-search",
        label_key: "bottom-toolbar-search",
        target: ToolbarTarget::Search,
    },
    ToolbarButtonDef {
        action: "toggle-build-tools",
        label_key: "bottom-toolbar-build",
        target: ToolbarTarget::BuildTools,
    },
    ToolbarButtonDef {
        action: "toggle-people",
        label_key: "bottom-toolbar-people",
        target: ToolbarTarget::Unlanded,
    },
    ToolbarButtonDef {
        action: "toggle-snapshot",
        label_key: "bottom-toolbar-snapshot",
        target: ToolbarTarget::Snapshot,
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
/// themselves into the row just above the button bar. That row is split into two
/// non-overlapping halves — [`upper_leading`](Self::upper_leading) and
/// [`upper_trailing`](Self::upper_trailing) — so a control appearing in one half
/// never reflows the other.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct BottomArea {
    /// The bottom-anchored column that holds the whole area — the chat overlay
    /// ([`crate::chat`]) reads its measured height to sit just above it.
    pub(crate) area: Entity,
    /// The **leading** half of the row directly above the button bar — the
    /// nearby-chat bar ([`crate::nearby_chat_bar`]) spawns into it, so it always
    /// rides in the same place at the bar's leading edge.
    pub(crate) upper_leading: Entity,
    /// The **trailing** half of that same row — the parcel music / nearby-media
    /// cluster ([`crate::parcel_audio`]) spawns into it (as will the volume, voice
    /// and quick-prefs controls). It sits beside the chat bar, not above it, so
    /// toggling the music cluster's visibility never moves the chat bar.
    pub(crate) upper_trailing: Entity,
    /// The button-bar row itself. Still awaiting a consumer (a future control that
    /// needs the bar strip directly rather than the upper row).
    #[expect(
        dead_code,
        reason = "the bar-strip handle is published for a future bottom-edge control that targets \
                  the button row directly; `area` and the upper slots are consumed"
    )]
    pub(crate) bar: Entity,
    /// The reserved **state slot** at the bar's leading edge — a fixed-width host
    /// the Stand Up / Stop flycam state button ([`crate::stand_stop_button`])
    /// parents into. Its fixed width (matched by a trailing spacer) keeps the state
    /// button from ever reflowing the centred toolbar buttons.
    pub(crate) state_slot: Entity,
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
/// RTL): an *upper row* for the neighbour controls above, then the button-bar row
/// below it. The upper row is split into a leading and a trailing half so the
/// nearby-chat bar and the parcel-audio cluster sit **side by side** on one line
/// directly above the buttons — appearing or disappearing without shoving each
/// other (`viewer-music-controls-push-chat-bar`). The bar wraps upward when it is
/// too narrow for every button.
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
            // clicks off the world, so the (empty) upper row does not swallow
            // pointer hits aimed at the scene.
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("bottom-area"),
            ChildOf(root.0),
        ))
        .id();

    // The upper row the neighbour controls parent into — above the button bar,
    // full width, its children bottom-aligned so each rides directly on the bar
    // regardless of its height. Empty (zero-height) until a control lands.
    let upper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::FlexEnd,
                ..row(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("bottom-area-upper"),
            ChildOf(area),
        ))
        .id();

    // Two fixed non-overlapping halves of the upper row, spawned leading-then-
    // trailing so their sides never depend on which neighbour plugin spawns first
    // (both are one-shot `Update` systems). Each is half the width and bottom-
    // aligns its own content onto the button bar; each half's occupant toggles
    // independently without moving the other.
    let upper_leading = spawn_upper_slot(
        &mut commands,
        upper,
        JustifyContent::FlexStart,
        "bottom-area-upper-leading",
    );
    let upper_trailing = spawn_upper_slot(
        &mut commands,
        upper,
        JustifyContent::FlexEnd,
        "bottom-area-upper-trailing",
    );

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

    // The reserved leading state slot, spawned first so it sits at the button
    // group's leading edge, then the toolbar buttons, then a trailing spacer of the
    // same fixed width. The two fixed-width bookends keep the centred buttons truly
    // centred and stop the state button (which appears / disappears with the seated
    // / flycam state) from ever reflowing the row.
    let state_slot = spawn_state_slot(&mut commands, bar);

    for (index, def) in TOOLBAR_BUTTONS.iter().enumerate() {
        spawn_live_button(&mut commands, bar, index, def);
    }

    spawn_state_spacer(&mut commands, bar);

    commands.insert_resource(BottomArea {
        area,
        upper_leading,
        upper_trailing,
        bar,
        state_slot,
    });
}

/// Spawn the reserved leading [state slot](STATE_SLOT_WIDTH) under the button bar —
/// a fixed-width, non-blocking box the Stand Up / Stop flycam state button parents
/// into. Fixed width so its (dis)appearing occupant never reflows the centred
/// buttons; centres its own child so the button reads centred within the slot.
fn spawn_state_slot(commands: &mut Commands, bar: Entity) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Px(STATE_SLOT_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // Transparent and non-blocking when empty; the state button itself takes
            // clicks once it is spawned in.
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("bottom-toolbar-state-slot"),
            ChildOf(bar),
        ))
        .id()
}

/// Spawn the trailing spacer that balances the leading [state slot](spawn_state_slot):
/// an empty fixed-width box of the same width, so the toolbar buttons stay centred
/// on the window rather than nudged rightward by the leading slot.
fn spawn_state_spacer(commands: &mut Commands, bar: Entity) {
    commands.spawn((
        Node {
            width: Val::Px(STATE_SLOT_WIDTH),
            flex_shrink: 0.0,
            ..default()
        },
        Pickable {
            should_block_lower: false,
            is_hoverable: true,
        },
        Name::new("bottom-toolbar-state-spacer"),
        ChildOf(bar),
    ));
}

/// Spawn one half of the [upper row](spawn_bottom_toolbar): a fixed 50%-wide,
/// non-blocking [`row`] whose content is bottom-aligned onto the button bar and
/// pushed to the `justify` edge (leading or trailing). Fixed width — not
/// flex-grown — so an empty or hidden half keeps its size and never lets the
/// other half spread across it (the reflow the split exists to prevent).
fn spawn_upper_slot(
    commands: &mut Commands,
    upper: Entity,
    justify: JustifyContent,
    name: &'static str,
) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(50.0),
                justify_content: justify,
                align_items: AlignItems::FlexEnd,
                ..row(Val::ZERO)
            },
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new(name),
            ChildOf(upper),
        ))
        .id()
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
/// same harmless dispatch the top menu bar relies on. A floater is resolved by
/// its stable id ([`ToolbarTarget::floater_id`]), never through its module's
/// `XUi` resource: for a lazily-built floater the resource does not exist until
/// this very toggle performs the first open.
fn handle_toolbar_actions(
    mut actions: MessageReader<UiAction>,
    floaters: Query<(Entity, &Floater)>,
    mut nearby_chat: Option<ResMut<NearbyChatBar>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    for action in actions.read() {
        if action.element != BOTTOM_TOOLBAR_ELEMENT {
            continue;
        }
        let target = TOOLBAR_BUTTONS
            .iter()
            .find(|def| def.action == action.action)
            .map(|def| def.target);
        if let Some(id) = target.and_then(ToolbarTarget::floater_id)
            && let Some(panel) = floater_panel(&floaters, id)
            && let Ok(mut shown) = panels.get_mut(panel)
        {
            shown.0 = !shown.0;
        }
        if target == Some(ToolbarTarget::NearbyChat)
            && let Some(bar) = nearby_chat.as_deref_mut()
        {
            bar.toggle();
        }
    }
}

/// Resolve whether a button's target floater is currently open, or `None` when the
/// target is unlanded (so it stays disabled). Floaters resolve by stable id
/// (see [`ToolbarTarget::floater_id`]).
fn resolve_target_open(
    target: ToolbarTarget,
    floaters: &Query<(Entity, &Floater)>,
    nearby_chat: Option<&NearbyChatBar>,
    panels: &Query<&UiPanelShown>,
) -> Option<bool> {
    match target {
        ToolbarTarget::NearbyChat => nearby_chat.map(NearbyChatBar::is_shown),
        ToolbarTarget::Unlanded => None,
        wired => wired
            .floater_id()
            .and_then(|id| floater_panel(floaters, id))
            .and_then(|panel| panels.get(panel).ok())
            .map(|shown| shown.0),
    }
}

/// Keep each toolbar button's look current: lit while its floater is open, resting
/// while closed, greyed while unlanded — writing through change detection only on
/// a real change so an idle bar does not re-trigger layout.
fn update_toolbar_button_states(
    floaters: Query<(Entity, &Floater)>,
    conversation_model: Option<Res<ConversationModel>>,
    nearby_chat: Option<Res<NearbyChatBar>>,
    time: Res<Time>,
    mut buttons: Query<(&ToolbarButton, &mut BackgroundColor)>,
    panels: Query<&UiPanelShown>,
    mut labels: Query<&mut TextColor>,
) {
    let nearby_chat = nearby_chat.as_deref();
    // The Conversations button flashes while the window is closed and an IM /
    // group / conference has unread lines — the reference's toolbar attention cue
    // (the window is never popped open over what the user is doing).
    let conversations_flash = conversation_model
        .as_deref()
        .is_some_and(ConversationModel::has_im_attention)
        && (time.elapsed_secs() * BLINK_HZ).fract() < 0.5;
    for (button, mut background) in &mut buttons {
        let visual = match resolve_target_open(button.target, &floaters, nearby_chat, &panels) {
            Some(true) => ToolbarButtonVisual::Active,
            Some(false) => ToolbarButtonVisual::Enabled,
            None => ToolbarButtonVisual::Disabled,
        };
        // A closed Conversations button with pending attention pulses to its lit
        // colour on the blink's "on" phase.
        let bg = if button.target == ToolbarTarget::Conversations
            && visual == ToolbarButtonVisual::Enabled
            && conversations_flash
        {
            ToolbarButtonVisual::Active.background()
        } else {
            visual.background()
        };
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
    use super::{
        BottomArea, TOOLBAR_BUTTONS, ToolbarButtonVisual, ToolbarTarget, spawn_bottom_toolbar,
    };
    use crate::ui::UiRoot;
    use bevy::prelude::*;
    use pretty_assertions::{assert_eq, assert_ne};

    /// The wired toolbar buttons today are the leading nearby-chat toggle,
    /// Conversations (its semantic pair, right beside it), Inventory, the
    /// world map and the minimap, in that order; the rest are unlanded
    /// placeholders. A regression that silently disabled a live toggle,
    /// reordered the pair, or wired a target that does not exist would trip
    /// here. The chat toggle leads the bar, as the reference places it.
    #[test]
    fn nearby_chat_and_inventory_are_wired() {
        let wired: Vec<&str> = TOOLBAR_BUTTONS
            .iter()
            .filter(|def| def.target.is_wired())
            .map(|def| def.action)
            .collect();
        assert_eq!(
            wired,
            [
                "toggle-nearby-chat",
                "toggle-conversations",
                "toggle-inventory",
                "toggle-map",
                "toggle-minimap",
                "toggle-search",
                "toggle-build-tools",
                "toggle-snapshot"
            ]
        );
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

    /// The boxed-error alias the app-driven tests bubble failures through, so
    /// they read `?` rather than the `expect`/`unwrap` the restriction lints ban.
    type TestError = Box<dyn core::error::Error>;

    /// Run [`spawn_bottom_toolbar`] once in a headless app and return the
    /// published [`BottomArea`], for the layout-structure assertions below.
    fn spawn_area() -> Result<(App, BottomArea), TestError> {
        let mut app = App::new();
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        app.add_systems(Startup, spawn_bottom_toolbar);
        app.update();
        let area = *app
            .world()
            .get_resource::<BottomArea>()
            .ok_or("spawn_bottom_toolbar did not publish BottomArea")?;
        Ok((app, area))
    }

    /// The ordered children of an entity, as a `Vec`.
    fn children_of(app: &App, entity: Entity) -> Vec<Entity> {
        app.world()
            .entity(entity)
            .get::<Children>()
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    }

    /// The nearby-chat bar (leading) and the parcel-audio cluster (trailing) must
    /// share **one row** directly above the button bar, split into two fixed halves
    /// in a deterministic leading-then-trailing order — so the music cluster
    /// appearing never pushes the chat bar (the bug this task fixes). A regression
    /// that stacked them in a column again, dropped the fixed 50% width (letting one
    /// half spread over the other), or reversed the slot order would trip here.
    #[test]
    fn upper_row_splits_into_fixed_leading_and_trailing_halves() -> Result<(), TestError> {
        let (app, area) = spawn_area()?;

        // The upper region is the shared parent of both slots — one row, not two
        // stacked children.
        let upper = app
            .world()
            .entity(area.upper_leading)
            .get::<ChildOf>()
            .map(ChildOf::parent)
            .ok_or("the leading slot has no parent")?;
        assert_eq!(
            app.world()
                .entity(area.upper_trailing)
                .get::<ChildOf>()
                .map(ChildOf::parent),
            Some(upper),
            "the trailing slot shares the upper row with the leading slot",
        );

        // That shared parent flows along the inline axis (side by side), not the
        // block axis (stacked).
        let upper_node = app
            .world()
            .entity(upper)
            .get::<Node>()
            .ok_or("the upper row has no Node")?;
        assert_eq!(
            upper_node.flex_direction,
            FlexDirection::Row,
            "the upper region is a row so its halves sit side by side",
        );

        // Leading before trailing, so the chat bar lands on the leading edge under
        // LTR (and mirrors for free under RTL) regardless of spawn timing.
        let upper_kids = children_of(&app, upper);
        assert_eq!(
            upper_kids,
            vec![area.upper_leading, area.upper_trailing],
            "the slots are ordered leading then trailing",
        );

        // Each half is a fixed 50% wide and bottom-aligned onto the button bar; a
        // fixed (not flex-grown) width is what keeps a hidden half from letting the
        // other spread across it.
        for slot in [area.upper_leading, area.upper_trailing] {
            let node = app
                .world()
                .entity(slot)
                .get::<Node>()
                .ok_or("a slot has no Node")?;
            assert_eq!(
                node.width,
                Val::Percent(50.0),
                "each half is fixed 50% wide"
            );
            assert_eq!(
                node.align_items,
                AlignItems::FlexEnd,
                "each half bottom-aligns its content onto the button bar",
            );
        }

        // The whole upper region sits above the button bar in the area column: the
        // area holds the upper row then the button bar, in that order. (The bar is
        // found by position, not the `dead_code`-expected `BottomArea::bar` field,
        // so reading it here would not defeat that expectation.)
        let area_kids = children_of(&app, area.area);
        assert_eq!(
            area_kids.first().copied(),
            Some(upper),
            "the upper row is the first (top) child of the area column",
        );
        assert_eq!(
            area_kids.len(),
            2,
            "the area column holds exactly the upper row and the button bar below it",
        );
        Ok(())
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
