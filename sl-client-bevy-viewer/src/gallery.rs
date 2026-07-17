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

use bevy::input_focus::tab_navigation::{TabIndex, TabNavigationPlugin};
use bevy::input_focus::{InputFocus, InputFocusVisible};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::window::PresentMode;
use tracing::info;

use crate::pie_menu::{FIXTURE_PIE, OpenPieMenu, PieMenuPlugin};
use crate::ui::{
    LogicalMargin, LogicalRect, UiDirection, UiScaffoldSystems, apply_panel_visibility,
    apply_ui_direction, column, invalidate_logical_boxes, resolve_logical_boxes, spawn_ui_root,
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
                drive_focus_ring,
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
        .run();
}

/// A 2D camera: the gallery renders UI and nothing else. No 3D, no world, no
/// scene — that is the whole idea.
fn spawn_gallery_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
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
    commands.entity(root.0).observe(open_gallery_pie);
    let page = commands
        .spawn((
            Node {
                // The window, minus a margin, scrolling on **both** axes: the
                // element list runs past the window at the larger font sizes, and a
                // gallery that cannot reach its own last element is not one. Both
                // axes so a wide element (or a wide translation) stays reachable too.
                width: Val::Percent(100.0),
                overflow: Overflow::scroll(),
                ..column(Val::Px(12.0))
            },
            LogicalMargin(LogicalRect::all(Val::Px(16.0))),
            ChildOf(root.0),
        ))
        .id();
    commands.spawn((
        Text::default(),
        UiFont::Mono.at(CHROME_FONT_SIZE),
        TextColor(HEADER_COLOR),
        GalleryHeader,
        ChildOf(page),
    ));
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

/// Spawn one card per registered element into `parent`.
///
/// Every element in [`ELEMENTS`] and nothing hand-picked, so an element added to
/// the registry shows up here for free — the same property that gets it swept by
/// the harness.
fn spawn_element_cards(commands: &mut Commands, parent: Entity, cell: GalleryCell) {
    for element in ELEMENTS {
        let card = commands
            .spawn((
                Node {
                    padding: UiRect::all(Val::Px(10.0)),
                    max_width: Val::Px(760.0),
                    ..column(Val::Px(6.0))
                },
                BackgroundColor(CARD_BACKGROUND),
                ChildOf(parent),
            ))
            .id();
        commands.spawn((
            Text::new(format!("{} — {}", element.id, element.summary)),
            UiFont::Mono.at(CHROME_FONT_SIZE),
            TextColor(CHROME_COLOR),
            ChildOf(card),
        ));
        (element.spawn)(commands, card, cell.cx());
    }
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

/// The focus ring, so `Tab` is visible.
///
/// Reads `InputFocusVisible` as well as [`InputFocus`], because `bevy_input_focus`
/// distinguishes *having* focus from *showing* it: a click hides the ring, `Tab`
/// brings it back.
fn drive_focus_ring(
    focus: Res<InputFocus>,
    focus_visible: Res<InputFocusVisible>,
    mut widgets: Query<(Entity, &mut BorderColor), With<TabIndex>>,
) {
    if !focus.is_changed() && !focus_visible.is_changed() {
        return;
    }
    for (widget, mut border) in &mut widgets {
        let focused = focus_visible.0 && focus.get() == Some(widget);
        let target = BorderColor::all(if focused {
            Color::srgb(1.0, 0.78, 0.25)
        } else {
            Color::srgb(0.40, 0.50, 0.62)
        });
        if *border != target {
            *border = target;
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

/// Open the fixture pie where the gallery was right-clicked.
///
/// The **secondary** button, as a context menu is everywhere. It opens the pie at
/// the pointer, so a right-click near a viewport edge exercises the inward clamp,
/// and one in a corner exercises both edges at once — the placement cases the unit
/// tests cover and this lets a person watch.
fn open_gallery_pie(press: On<Pointer<Press>>, mut requests: MessageWriter<OpenPieMenu>) {
    if press.button != PointerButton::Secondary {
        return;
    }
    requests.write(OpenPieMenu {
        menu: &FIXTURE_PIE,
        at: press.pointer_location.position,
        element: "radial-menu",
        conditions: &[],
    });
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
