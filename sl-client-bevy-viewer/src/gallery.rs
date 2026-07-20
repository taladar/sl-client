//! The **UI gallery** (`viewer-ui-test-harness`): every registered UI element,
//! rendered on its own, with **no login, no grid and no world**.
//!
//! ```console
//! sl-client-bevy-viewer-gallery
//! ```
//!
//! # What it is for, now that the matrix is not its job
//!
//! The gallery answers the one question a machine cannot: **does this look
//! right**. Whether a layout is *correct* — content inside its box, nothing off
//! screen, columns straight, text unsliced — is machine-checkable, and
//! `crate::ui_test` checks it across every element in every script, direction,
//! scale and translation length. Walking that grid by eye is exactly the
//! combinatorial explosion the harness exists to end, so the gallery does not try.
//!
//! What is left for a human is real and cannot be automated: is the spacing ugly,
//! is the contrast wrong, does the accent land somewhere silly, does this read as
//! one design with the panel next to it. And the discovery loop — a person
//! notices something wrong here, and the fix is a **check** in
//! `crate::ui_test`, which from then on runs against every element forever. The
//! gallery is where bugs are *found*; the harness is where they stay found.
//!
//! # Why it can exist at all
//!
//! Because of the registry's one rule: an element is **constructible without its
//! wiring** (`crate::ui_element`). Every button here is clickable and every
//! click is inert — the element emits a `UiAction` and, in this binary, nothing
//! listens. No teleport, no object edit, no L$. That is not a special gallery
//! mode with the dangerous parts stubbed out; it is the same construction the
//! viewer uses, minus the handlers.
//!
//! # Driving it
//!
//! | Key | What it does |
//! | --- | --- |
//! | `Tab` / `Shift+Tab` | walk the focusable widgets |
//! | `Enter` / `Space` | activate the focused widget (logs its inert action) |
//! | `D` | flip the layout direction (LTR / RTL) — the whole tree mirrors |
//! | `L` | cycle the strings: native, pseudolocalised, each script in turn |
//! | `S` | cycle the UI font size |
//! | `Escape` | quit |
//!
//! `L` and `D` are the matrix, hand-drivable. They are here not to *check* the
//! cells — the harness does that — but so a person can look at the cell a failing
//! check just named.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::tab_navigation::{TabIndex, TabNavigationPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::PresentMode;
use bevy_flair::style::components::ClassList;
use tracing::info;

use crate::pie_menu::{FIXTURE_PIE, OpenPieMenu, PieMenuPlugin};
use crate::skin::SkinSelection;
use crate::ui::{
    UiDirection, UiScaffoldSystems, apply_panel_visibility, apply_ui_direction, column,
    invalidate_logical_boxes, resolve_logical_boxes, row, scroll_focus_into_view, spawn_ui_root,
};
use crate::ui_element::{ELEMENTS, ElementCx, SCRIPTS, SampleText, UiAction};
use crate::ui_font::{UiFont, register_ui_fonts};

/// The key that flips the layout direction.
const DIRECTION_KEY: KeyCode = KeyCode::KeyD;

/// The key that cycles which strings the elements are shown with.
const SAMPLE_KEY: KeyCode = KeyCode::KeyL;

/// The key that cycles the UI font size.
const SIZE_KEY: KeyCode = KeyCode::KeyS;

/// The gallery's own chrome font size, in logical pixels. Fixed, deliberately:
/// the chrome must stay legible while the *elements* are cycled to 30 px, or the
/// thing telling you which cell you are looking at becomes unreadable exactly
/// when you need it.
const CHROME_FONT_SIZE: f32 = 13.0;

/// The font sizes [`SIZE_KEY`] cycles the elements through.
const FONT_SIZES: [f32; 4] = [11.0, 15.0, 22.0, 30.0];

/// The gallery's background.
const BACKGROUND: Color = Color::srgb(0.09, 0.10, 0.13);

/// An element card's backdrop, so each element reads as one thing.
const CARD_BACKGROUND: Color = Color::srgba(1.0, 1.0, 1.0, 0.04);

/// The colour of an element's id and summary.
const CHROME_COLOR: Color = Color::srgb(0.62, 0.68, 0.78);

/// The colour of the header line.
const HEADER_COLOR: Color = Color::srgb(0.95, 0.85, 0.45);

/// The sticky header bar's background — a touch lighter than the page, so the
/// fixed legend reads as a bar above the scrolling list.
const HEADER_BAR_BACKGROUND: Color = Color::srgb(0.14, 0.16, 0.20);

/// Which cell of the matrix the gallery is currently showing.
///
/// The same axes [`crate::ui_test`] sweeps, exposed as one resource so a person
/// can steer to the cell a failing check named and look at it.
#[derive(Resource, Debug, Clone, Copy)]
struct GalleryCell {
    /// Which strings the elements are built with.
    text: SampleText,
    /// The size the elements' text is set at.
    font_size: f32,
}

impl Default for GalleryCell {
    /// The resting cell is the registry's own resting context, rather than a
    /// second opinion about it: the gallery opens on the same thing a test's
    /// baseline cell shows.
    fn default() -> Self {
        let resting = ElementCx::new();
        Self {
            text: resting.text,
            font_size: resting.font_size,
        }
    }
}

impl GalleryCell {
    /// This cell as the context an element is spawned with.
    const fn cx(self) -> ElementCx {
        ElementCx {
            text: self.text,
            font_size: self.font_size,
        }
    }

    /// The next sample in the cycle: native, pseudo, then each script in turn.
    ///
    /// Pseudolocalisation comes second rather than last because it is the one a
    /// person can still *read* while seeing it break — the most useful cell to
    /// land on by accident.
    fn next_sample(self) -> SampleText {
        match self.text {
            SampleText::Native => SampleText::Pseudo,
            SampleText::Pseudo => SCRIPTS
                .first()
                .map_or(SampleText::Native, SampleText::Script),
            SampleText::Script(current) => {
                let next = SCRIPTS
                    .iter()
                    .position(|sample| sample.name == current.name)
                    .and_then(|index| SCRIPTS.get(index.saturating_add(1)));
                next.map_or(SampleText::Native, SampleText::Script)
            }
        }
    }

    /// The next font size in the cycle.
    fn next_size(self) -> f32 {
        let index = FONT_SIZES
            .iter()
            .position(|size| size.to_bits() == self.font_size.to_bits())
            .map_or(0, |index| index.saturating_add(1));
        FONT_SIZES.get(index).copied().unwrap_or(FONT_SIZES[0])
    }
}

/// A marker on the scrolling page node, so [`scroll_gallery`] knows which
/// `ScrollPosition` the wheel drives.
#[derive(Component, Debug, Clone, Copy)]
struct GalleryPage;

/// Logical pixels scrolled per wheel notch reported in [`MouseScrollUnit::Line`],
/// matching [`crate::virtual_list`] so the two surfaces scroll at one speed.
const LINE_SCROLL_PIXELS: f32 = 48.0;

/// A marker on the node holding the element cards, so a cell change can clear
/// and respawn them without touching the chrome.
#[derive(Component, Debug, Clone, Copy)]
struct GalleryElements;

/// A marker on the header line, which reports the live cell.
#[derive(Component, Debug, Clone, Copy)]
struct GalleryHeader;

/// Run the gallery: a window, the viewer's real UI scaffold, and every
/// registered element rendered on its own.
///
/// Returns `()` rather than a `Result` because there is nothing here to fail at:
/// no credentials to reject, no grid to be unreachable, no world to fail to
/// load. That is the whole point of the gallery, and the signature says so.
pub fn run() {
    crate::init_tracing();
    info!(
        elements = ELEMENTS.len(),
        scripts = SCRIPTS.len(),
        "starting the UI gallery: no login, no world; D flips direction, L cycles \
         script/pseudoloc, S cycles font size"
    );
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "sl-client-bevy-viewer — UI gallery".to_owned(),
                        name: Some("sl-client-bevy-viewer-gallery".to_owned()),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                })
                // Watch the skin `.css` files: the gallery is the skin-authoring
                // surface, so an edit re-applies live here without a restart.
                .set(AssetPlugin {
                    watch_for_changes_override: Some(true),
                    ..default()
                })
                // The binary installs its own subscriber (`crate::init_tracing`),
                // as the viewer does; two would clash over the global slot.
                .disable::<LogPlugin>(),
        )
        // The keyboard half of focus. `DefaultPlugins` wires focus dispatch but
        // not navigation, so without this `Tab` is inert — and a gallery in which
        // nothing can be focused cannot show that anything is focusable.
        .add_plugins(TabNavigationPlugin)
        // The radial menu (`viewer-ui-radial-menu`), which brings its own ring
        // material and the systems that drive a live pie. The gallery wants the
        // whole widget rather than a picture of one: a pie is almost entirely
        // gesture — the centre-jump near an edge, the dead zone, the two
        // interaction modes, descending into a sub-pie — and none of that is
        // visible on a menu that is only drawn. Right-click the `radial-menu-target`
        // card. Safe here despite the pointer grab, because the widget saves and
        // restores the cursor itself rather than relying on the viewer's input
        // context, which this binary has not got.
        .add_plugins(PieMenuPlugin)
        // The line-menu widget: the menu-bar specimen's drop-downs, and the
        // context menu the right-click toggle below opens. Right-click anywhere
        // in the gallery opens a pie or a drop-down context menu depending on the
        // `PointerMenuStyle` toggle, so the two presentations of a menu are
        // switchable side by side rather than one being a pre-opened card.
        .add_plugins(crate::menu::MenuWidgetPlugin)
        .init_resource::<PointerMenuStyle>()
        // The tab widget's runtime half: a resizable strip's width reaching layout
        // (the divider demo) and each tab's corners tracking the direction.
        .add_plugins(crate::ui_tab::TabWidgetPlugin)
        // The skin / design-token system, so the gallery is dressed in a real
        // skin and the switcher below can flip skins and theme overlays live.
        .insert_resource(crate::skin::SkinSelection::resolve(None, None))
        .add_plugins(crate::skin::ViewerSkinPlugin)
        // Seeded from `SL_VIEWER_UI_DIRECTION`, as the viewer does, so the gallery
        // can be started straight into RTL rather than only reached by pressing `D`.
        .insert_resource(UiDirection::from_env())
        .init_resource::<GalleryCell>()
        // Declared so an element's button has somewhere to emit to. Nothing reads
        // it but `log_actions`, which is exactly the point: in this binary every
        // click is inert by construction.
        .add_message::<UiAction>()
        .insert_resource(ClearColor(BACKGROUND))
        .add_systems(
            Startup,
            (
                register_ui_fonts,
                spawn_gallery_camera,
                spawn_ui_root.in_set(UiScaffoldSystems::SpawnRoot),
                setup_gallery.after(UiScaffoldSystems::SpawnRoot),
            ),
        )
        .add_systems(
            Update,
            (
                drive_gallery_keys,
                respawn_elements_on_cell_change.after(drive_gallery_keys),
                update_gallery_header,
                update_skin_switcher_label,
                update_pointer_menu_label,
                scroll_gallery,
                log_actions,
                quit_on_escape,
            ),
        )
        .add_systems(
            PostUpdate,
            (
                apply_panel_visibility,
                invalidate_logical_boxes,
                resolve_logical_boxes,
                apply_ui_direction,
            )
                .chain()
                .before(bevy::ui::UiSystems::Layout),
        )
        // *After* layout, so both read this frame's computed boxes: the gallery
        // page is the scroll container the focus ring exposed as needing scroll-
        // into-view, and the tab order is re-numbered from on-screen position.
        .add_systems(
            PostUpdate,
            (order_gallery_tab_stops, scroll_focus_into_view).after(bevy::ui::UiSystems::Layout),
        )
        .run();
}

/// Re-number the gallery's focus stops into reading order — top-to-bottom, then
/// leading-to-trailing — so `Tab` walks the page the way the eye does.
///
/// Every specimen sets its own `TabIndex` (almost all `0`), and the gallery packs
/// them from sources whose spawn / hierarchy order does not track where they land
/// on screen — a `floater` specimen parents to the root, the header switcher sits
/// above the page — so `bevy_input_focus`'s hierarchy-order tie-break makes `Tab`
/// jump around (`viewer-ui-gallery-tab-order`). Sorting the live positions and
/// assigning a rank sidesteps the cause whatever it is. Gallery-only: a real UI
/// orders its own stops deliberately, at spawn.
///
/// Runs after layout (positions are valid) and re-derives every frame, so a
/// cell-change respawn or a font-size reflow re-sorts for free; the `!=` guard
/// keeps it a no-op once the ranks have settled.
fn order_gallery_tab_stops(
    positions: Query<(Entity, &UiGlobalTransform), With<TabIndex>>,
    mut indices: Query<&mut TabIndex>,
) {
    let mut ordered: Vec<(Entity, f32, f32)> = positions
        .iter()
        .map(|(entity, transform)| (entity, transform.translation.y, transform.translation.x))
        .collect();
    // `total_cmp` is a total order (no `Option`, deterministic on ties), so the
    // sort never oscillates frame to frame for a settled layout.
    ordered.sort_by(|(_, a_y, a_x), (_, b_y, b_x)| a_y.total_cmp(b_y).then(a_x.total_cmp(b_x)));
    for (rank, (entity, _, _)) in ordered.iter().enumerate() {
        let Ok(mut index) = indices.get_mut(*entity) else {
            continue;
        };
        // `TabIndex` is `i32`; a gallery never has 2^31 stops, but avoid the
        // banned `as` cast anyway.
        let want = TabIndex(i32::try_from(rank).unwrap_or(i32::MAX));
        if *index != want {
            *index = want;
        }
    }
}

/// A 2D camera: the gallery renders UI and nothing else. No 3D, no world, no
/// scene — that is the whole idea.
fn spawn_gallery_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Scroll the gallery page with the mouse wheel.
///
/// `bevy_ui` clips an `Overflow::scroll` node but does not itself move it — the
/// app owns the wheel. Mirrors [`crate::virtual_list::scroll_virtual_lists`]:
/// same per-notch step, same `Line` / `Pixel` unit handling. The offset floors at
/// zero; `bevy_ui` clamps the far end to the scrollable range at layout time.
fn scroll_gallery(
    wheel: Res<AccumulatedMouseScroll>,
    mut pages: Query<&mut ScrollPosition, With<GalleryPage>>,
) {
    if wheel.delta.y.abs() < f32::EPSILON {
        return;
    }
    let delta = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * LINE_SCROLL_PIXELS,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    for mut position in &mut pages {
        position.0.y = (position.0.y - delta).max(0.0);
    }
}

/// Spawn the chrome and the element list under the scaffold's root.
fn setup_gallery(mut commands: Commands, root: Res<crate::ui::UiRoot>, cell: Res<GalleryCell>) {
    // **The whole gallery is a right-click surface**, so a pie can be opened at any
    // screen position — including hard against an edge or in a corner, which is the
    // clamped-placement case worth being able to see by hand. This mirrors the real
    // viewer, where a right-click anywhere in the world opens the pie; there is no
    // persistent on-screen menu, so there is nothing to scroll to an edge. The
    // observer sits on the scaffold root, which receives the press wherever no
    // blocking widget is under the pointer — the margins and gaps, and every edge.
    commands.entity(root.0).observe(open_gallery_menu);
    // A **sticky header bar** outside the scroll area, so the key legend and the
    // live-cell readout stay on screen while the element list scrolls under it.
    // `flex_shrink: 0` keeps it at its content height; the page below takes the
    // rest.
    let header = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                padding: UiRect::all(Val::Px(12.0)),
                ..column(Val::Px(8.0))
            },
            BackgroundColor(HEADER_BAR_BACKGROUND),
            // Skinned, so the header bar recolours with the skin too.
            ClassList::new_with_classes(["sk-card"]),
            // Above the scrolling page in the UI stack, so `bevy_ui` picking
            // routes clicks on the switcher buttons to them even when the page is
            // scrolled — the page's scrolled-off content is not clipped for
            // picking and would otherwise sit over the sticky header and swallow
            // the clicks (the "controls only work at the very top" symptom).
            GlobalZIndex(1),
            ChildOf(root.0),
        ))
        .with_child((
            Text::default(),
            UiFont::Mono.at(CHROME_FONT_SIZE),
            TextColor(HEADER_COLOR),
            GalleryHeader,
        ))
        .id();
    spawn_skin_switcher(&mut commands, header);
    let page = commands
        .spawn((
            Node {
                // Takes the height the header leaves (`flex_grow` in the root's
                // column) and scrolls its content: the element list runs past the
                // window at the larger font sizes, and a gallery that cannot reach
                // its own last element is not one. `min_height: 0` is what lets a
                // flex child shrink below its content so the overflow actually
                // clips and scrolls rather than growing off-screen. Both axes, so a
                // wide element or translation stays reachable too. Driven by
                // [`scroll_gallery`].
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(16.0)),
                overflow: Overflow::scroll(),
                ..column(Val::Px(12.0))
            },
            // `Overflow::scroll` only sets the style; the offset lives in this
            // separate component, which both the layout (to place the children)
            // and [`scroll_gallery`] (to move it) need present from the start.
            ScrollPosition::default(),
            GalleryPage,
            ChildOf(root.0),
        ))
        .id();
    let elements = commands
        .spawn((
            Node {
                ..column(Val::Px(12.0))
            },
            GalleryElements,
            ChildOf(page),
        ))
        .id();
    spawn_element_cards(&mut commands, elements, *cell);
}

/// The label prefix in front of the live skin selection on the switcher.
const SKIN_SWITCHER_PREFIX: &str = "  ⇦ live: ";

/// A switcher control / chip's inline fallback background, so it is legible in
/// the instant before the skin `.css` loads (the `.sk-*` classes recolour it
/// once the skin is applied).
const CHIP_FALLBACK_BG: Color = Color::srgb(0.16, 0.19, 0.25);

/// A switcher control / chip's inline fallback border colour.
const CHIP_FALLBACK_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A marker on the "next skin" button.
#[derive(Component, Debug, Clone, Copy)]
struct CycleSkinButton;

/// A marker on the "next theme" button.
#[derive(Component, Debug, Clone, Copy)]
struct CycleThemeButton;

/// A marker on the switcher's live-selection label text.
#[derive(Component, Debug, Clone, Copy)]
struct SkinSwitcherLabel;

/// Spawn the skin / theme switcher control under the gallery header: two buttons
/// that cycle the skin and its theme overlay, a live label of the current
/// selection, and a strip of skinned sample chips (a button, a tab, an accent
/// bar, and gain / loss swatches) that visibly recolour the instant a skin or
/// theme is selected. The buttons and chips are themselves skinned (`.sk-*`),
/// so the switcher is its own proof surface.
fn spawn_skin_switcher(commands: &mut Commands, header: Entity) {
    let strip = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(6.0),
                ..row(Val::Px(10.0))
            },
            ChildOf(header),
        ))
        .id();

    commands
        .spawn((switcher_button(), CycleSkinButton, ChildOf(strip)))
        .with_child(chip_text("Skin ▸"))
        .observe(cycle_skin_clicked);
    commands
        .spawn((switcher_button(), CycleThemeButton, ChildOf(strip)))
        .with_child(chip_text("Theme ▸"))
        .observe(cycle_theme_clicked);
    commands.spawn((
        Text::default(),
        UiFont::Mono.at(CHROME_FONT_SIZE),
        TextColor(HEADER_COLOR),
        SkinSwitcherLabel,
        ChildOf(strip),
    ));

    // The right-click menu toggle: flips whether a right-click anywhere opens a
    // pie or a drop-down context menu, so both presentations are reachable from
    // the same surface.
    commands
        .spawn((switcher_button(), PointerMenuButton, ChildOf(strip)))
        .with_child(chip_text("Right-click ▸"))
        .observe(cycle_pointer_menu_clicked);
    commands.spawn((
        Text::default(),
        UiFont::Mono.at(CHROME_FONT_SIZE),
        TextColor(HEADER_COLOR),
        PointerMenuLabel,
        ChildOf(strip),
    ));

    // Skinned sample chips: switching the skin or theme recolours these live.
    commands
        .spawn((chip("sk-button"), ChildOf(strip)))
        .with_child(chip_text("Button"));
    commands
        .spawn((chip("sk-tab"), ChildOf(strip)))
        .with_child(chip_text("Tab"));
    // The logical-box demo: a leading accent bar + hanging indent that mirrors
    // to the trailing edge under RTL (press `D`).
    commands
        .spawn((chip("sk-accent"), ChildOf(strip)))
        .with_child(chip_text("Accent"));
    commands.spawn((
        Text::new("▲ gain"),
        UiFont::Mono.at(CHROME_FONT_SIZE),
        TextColor(HEADER_COLOR),
        ClassList::new_with_classes(["sk-gain"]),
        ChildOf(strip),
    ));
    commands.spawn((
        Text::new("▼ loss"),
        UiFont::Mono.at(CHROME_FONT_SIZE),
        TextColor(HEADER_COLOR),
        ClassList::new_with_classes(["sk-loss"]),
        ChildOf(strip),
    ));
}

/// The bundle shared by the two switcher buttons: a focusable `bevy_ui_widgets`
/// button, skinned by the `sk-button` class, with inline fallback paint.
fn switcher_button() -> impl Bundle {
    (
        Button,
        ClassList::new_with_classes(["sk-button"]),
        Node {
            padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(CHIP_FALLBACK_BG),
        BorderColor::all(CHIP_FALLBACK_BORDER),
    )
}

/// A non-interactive skinned chip carrying the given `sk-*` class, with inline
/// fallback paint.
fn chip(class: &'static str) -> impl Bundle {
    (
        ClassList::new_with_classes([class]),
        Node {
            padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(CHIP_FALLBACK_BG),
        BorderColor::all(CHIP_FALLBACK_BORDER),
    )
}

/// A chip's text child. Its colour is inherited from the skinned parent (CSS
/// `color` is an inherited property), with white as the pre-skin fallback.
fn chip_text(label: &str) -> impl Bundle {
    (
        Text::new(label),
        UiFont::Sans.at(CHROME_FONT_SIZE),
        TextColor(Color::WHITE),
    )
}

/// Marker on the right-click menu toggle button.
#[derive(Component)]
struct PointerMenuButton;

/// Marker on the right-click menu toggle's live-selection label.
#[derive(Component)]
struct PointerMenuLabel;

/// Observer: flip the right-click menu presentation when its button is
/// activated.
fn cycle_pointer_menu_clicked(_activate: On<Activate>, mut style: ResMut<PointerMenuStyle>) {
    style.cycle();
}

/// Keep the right-click toggle's live label in step with [`PointerMenuStyle`].
fn update_pointer_menu_label(
    style: Res<PointerMenuStyle>,
    mut labels: Query<&mut Text, With<PointerMenuLabel>>,
) {
    if !style.is_changed() {
        return;
    }
    let wanted = format!("{SKIN_SWITCHER_PREFIX}{}", style.label());
    for mut text in &mut labels {
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
    }
}

/// Observer: advance to the next skin when the skin button is activated.
fn cycle_skin_clicked(_activate: On<Activate>, mut selection: ResMut<SkinSelection>) {
    selection.cycle_skin();
}

/// Observer: advance the theme overlay when the theme button is activated.
fn cycle_theme_clicked(_activate: On<Activate>, mut selection: ResMut<SkinSelection>) {
    selection.cycle_theme();
}

/// Keep the switcher's live label in step with the current [`SkinSelection`].
fn update_skin_switcher_label(
    selection: Res<SkinSelection>,
    mut labels: Query<&mut Text, With<SkinSwitcherLabel>>,
) {
    if !selection.is_changed() {
        return;
    }
    let wanted = format!("{SKIN_SWITCHER_PREFIX}{}", selection.label());
    for mut text in &mut labels {
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
    }
}

/// The bundle shared by every gallery element card: a content-sized column with
/// the card backdrop, **skinned** (`sk-card`) so it recolours with the active
/// skin / theme — which is what makes a skin switch visibly reskin the whole
/// gallery, not just the switcher chips.
fn card_bundle(parent: Entity) -> impl Bundle {
    (
        Node {
            padding: UiRect::all(Val::Px(10.0)),
            max_width: Val::Px(760.0),
            ..column(Val::Px(6.0))
        },
        BackgroundColor(CARD_BACKGROUND),
        ClassList::new_with_classes(["sk-card"]),
        ChildOf(parent),
    )
}

/// Spawn one card per registered element into `parent`.
///
/// Every element in [`ELEMENTS`] and nothing hand-picked, so an element added to
/// the registry shows up here for free — the same property that gets it swept by
/// the harness.
fn spawn_element_cards(commands: &mut Commands, parent: Entity, cell: GalleryCell) {
    for element in ELEMENTS {
        let card = commands.spawn(card_bundle(parent)).id();
        commands.spawn((
            Text::new(format!("{} — {}", element.id, element.summary)),
            UiFont::Mono.at(CHROME_FONT_SIZE),
            TextColor(CHROME_COLOR),
            ChildOf(card),
        ));
        (element.spawn)(commands, card, cell.cx());
    }
    spawn_resizable_tabs_card(commands, parent, cell);
    spawn_scroll_tabs_cards(commands, parent, cell);
}

/// Spawn the tab **scroll demo** cards — a few-tabs and a many-tabs copy of each
/// orientation, so the scroll control (a vertical scrollbar, horizontal arrows)
/// can be seen appearing when the tabs outgrow the space and staying hidden when
/// they fit. Auto from available space — the pairs differ only in tab count.
///
/// Not driven by [`ELEMENTS`]: a scrolling strip clips its tabs, and the human
/// wants to drive the wheel / arrows here.
fn spawn_scroll_tabs_cards(commands: &mut Commands, parent: Entity, cell: GalleryCell) {
    use crate::ui_tab::{TabPlacement, spawn_tabs_scroll_demo};
    for (placement, few, many, control, id) in [
        (
            TabPlacement::BlockStart,
            "tabs-scroll-h-few",
            "tabs-scroll-h-many",
            "trailing-edge arrows",
            "horizontal",
        ),
        (
            TabPlacement::InlineStart,
            "tabs-scroll-v-few",
            "tabs-scroll-v-many",
            "a scrollbar (drag it or use the wheel)",
            "vertical",
        ),
    ] {
        let card = commands.spawn(card_bundle(parent)).id();
        commands.spawn((
            Text::new(format!(
                "{id} tab overflow — a few tabs (left, no control) beside many (right, {control}); \
                 the control auto-shows from available space, not a flag."
            )),
            UiFont::Mono.at(CHROME_FONT_SIZE),
            TextColor(CHROME_COLOR),
            ChildOf(card),
        ));
        // The two copies side by side so the presence / absence of the control is
        // a direct comparison.
        let row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Start,
                    ..row(Val::Px(16.0))
                },
                ChildOf(card),
            ))
            .id();
        spawn_tabs_scroll_demo(commands, row, cell.cx(), placement, 3, few);
        spawn_tabs_scroll_demo(commands, row, cell.cx(), placement, 12, many);
    }
}

/// Spawn the resizable-tabs demo card — the one surface where a human can grab
/// the divider and drag it.
///
/// Not driven by [`ELEMENTS`] because a clipped tab label is deliberate overflow
/// the harness would flag (see [`crate::ui_tab::spawn_tabs_resizable_demo`]); the
/// gallery hosts it by hand instead. Rebuilt with the rest on a cell change.
fn spawn_resizable_tabs_card(commands: &mut Commands, parent: Entity, cell: GalleryCell) {
    let card = commands.spawn(card_bundle(parent)).id();
    commands.spawn((
        Text::new(
            "tabs-resizable — vertical tabs with a draggable divider; grab the bright grip and \
             drag to move the split (not in the harness registry — clipped labels are deliberate \
             overflow)."
                .to_owned(),
        ),
        UiFont::Mono.at(CHROME_FONT_SIZE),
        TextColor(CHROME_COLOR),
        ChildOf(card),
    ));
    crate::ui_tab::spawn_tabs_resizable_demo(commands, card, cell.cx());
}

/// Read the gallery's keys into [`GalleryCell`] / [`UiDirection`].
fn drive_gallery_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut cell: ResMut<GalleryCell>,
    mut direction: ResMut<UiDirection>,
) {
    if keyboard.just_pressed(DIRECTION_KEY) {
        *direction = match *direction {
            UiDirection::Ltr => UiDirection::Rtl,
            UiDirection::Rtl => UiDirection::Ltr,
        };
    }
    if keyboard.just_pressed(SAMPLE_KEY) {
        cell.text = cell.next_sample();
    }
    if keyboard.just_pressed(SIZE_KEY) {
        cell.font_size = cell.next_size();
    }
}

/// Rebuild every card when the cell changes.
///
/// Despawned and respawned rather than patched, because an element's strings are
/// baked in at construction — exactly as a real panel's are, and exactly as
/// `viewer-i18n-fluent-scaffold` will rebuild them on a locale switch. Patching
/// the text in place would be the gallery testing a code path the viewer does not
/// have.
fn respawn_elements_on_cell_change(
    mut commands: Commands,
    cell: Res<GalleryCell>,
    lists: Query<Entity, With<GalleryElements>>,
) {
    if !cell.is_changed() || cell.is_added() {
        return;
    }
    for list in &lists {
        commands.entity(list).despawn_related::<Children>();
        spawn_element_cards(&mut commands, list, *cell);
    }
}

/// Keep the header reporting the live cell, so a person always knows which of the
/// matrix's cells is on screen.
fn update_gallery_header(
    cell: Res<GalleryCell>,
    direction: Res<UiDirection>,
    mut headers: Query<&mut Text, With<GalleryHeader>>,
) {
    if !cell.is_changed() && !direction.is_changed() {
        return;
    }
    let wanted = format!(
        "UI gallery — {} elements | strings: {} (L) | size: {} px (S) | direction: {} (D) | \
         Tab walks, Enter activates (inert), Escape quits",
        ELEMENTS.len(),
        cell.text.name(),
        cell.font_size,
        if direction.is_rtl() { "RTL" } else { "LTR" },
    );
    for mut text in &mut headers {
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
    }
}

/// Log every [`UiAction`] an element emits.
///
/// The gallery's entire action wiring, and the demonstration that the registry's
/// no-wiring rule holds: a click reaches a log line and stops there. In the
/// viewer this is where a real handler would sit.
fn log_actions(mut actions: MessageReader<UiAction>) {
    for action in actions.read() {
        info!(
            element = action.element,
            action = action.action,
            "element action (inert in the gallery)"
        );
    }
}

/// Which menu a right-click on the gallery surface opens — the two presentations
/// of a menu, switchable so they can be compared without one being a pre-opened
/// card. Cycled by the header's toggle button.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PointerMenuStyle {
    /// A radial (pie) menu at the pointer.
    #[default]
    Pie,
    /// A line-based drop-down context menu at the pointer.
    DropDown,
}

impl PointerMenuStyle {
    /// Advance to the other presentation.
    const fn cycle(&mut self) {
        *self = match *self {
            Self::Pie => Self::DropDown,
            Self::DropDown => Self::Pie,
        };
    }

    /// The live label of the current presentation.
    const fn label(self) -> &'static str {
        match self {
            Self::Pie => "pie",
            Self::DropDown => "drop-down",
        }
    }
}

/// Open a menu where the gallery was right-clicked, in whichever presentation
/// the [`PointerMenuStyle`] toggle selects.
///
/// The **secondary** button, as a context menu is everywhere. It opens at the
/// pointer, so a right-click near a viewport edge exercises the inward clamp /
/// edge flip, and one in a corner exercises both edges at once — the placement
/// cases the unit tests cover and this lets a person watch, in either widget.
fn open_gallery_menu(
    press: On<Pointer<Press>>,
    style: Res<PointerMenuStyle>,
    mut pies: MessageWriter<OpenPieMenu>,
    mut context_menus: MessageWriter<crate::menu::OpenContextMenu>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    match *style {
        PointerMenuStyle::Pie => {
            pies.write(OpenPieMenu {
                menu: &FIXTURE_PIE,
                at: press.pointer_location.position,
                element: "radial-menu",
                conditions: &[],
            });
        }
        PointerMenuStyle::DropDown => {
            context_menus.write(crate::menu::OpenContextMenu {
                menu: &crate::menu::FIXTURE_CONTEXT_MENU,
                at: press.pointer_location.position,
                element: "context-menu",
            });
        }
    }
}

/// Quit on `Escape`.
fn quit_on_escape(keyboard: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::{FONT_SIZES, GalleryCell};
    use crate::ui_element::{SCRIPTS, SampleText};
    use pretty_assertions::assert_eq;

    /// The sample cycle reaches every script and closes back to native, so a
    /// person pressing `L` can get to any cell the harness might name — and can
    /// get back out again.
    #[test]
    fn the_sample_cycle_visits_every_script_and_closes() {
        let mut cell = GalleryCell::default();
        let mut seen = Vec::new();
        // Native + pseudo + one step per script, then back to native.
        for _step in 0..SCRIPTS.len().saturating_add(2) {
            seen.push(cell.text.name().to_owned());
            cell.text = cell.next_sample();
        }
        assert_eq!(
            cell.text,
            SampleText::Native,
            "the cycle must close back to native: {seen:?}"
        );
        for sample in SCRIPTS {
            assert!(
                seen.iter().any(|name| name == sample.name),
                "the cycle never reaches {}: {seen:?}",
                sample.name
            );
        }
    }

    /// The size cycle closes, and visits every size.
    #[test]
    fn the_size_cycle_visits_every_size_and_closes() {
        let mut cell = GalleryCell {
            font_size: FONT_SIZES[0],
            ..GalleryCell::default()
        };
        let mut seen = Vec::new();
        for _step in 0..FONT_SIZES.len() {
            seen.push(cell.font_size);
            cell.font_size = cell.next_size();
        }
        assert_eq!(
            cell.font_size.to_bits(),
            FONT_SIZES[0].to_bits(),
            "the size cycle must close"
        );
        let mut sizes: Vec<u32> = seen.iter().map(|size| size.to_bits()).collect();
        sizes.sort_unstable();
        sizes.dedup();
        assert_eq!(sizes.len(), FONT_SIZES.len(), "every size must be visited");
    }
}
