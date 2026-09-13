//! The **Build Tools floater**, driven end to end: reflect, edit, commit
//! ([[viewer-build-floater-interaction-tests]]).
//!
//! The build floater is the densest surface the viewer ships and the second
//! half of the edit tool, and it is the one place where three tiers meet. A
//! *selection* is made in the world; a *window* shows what was selected; a
//! *field* inside that window is typed into and the edit goes back out on the
//! wire. Each tier already has its own harness — `world_test` for the first,
//! `floater_chrome` for the second, `ui_contract` for the third — and none of
//! them can see the round trip, because the round trip is precisely what
//! crosses between them.
//!
//! So these tests run on the one fold that holds all three
//! ([`crate::world_test::world_app_with_build_tools`]): the world fold with a
//! CPU pick resolver under a real UI, with the floater and its five per-aspect
//! editor tabs on top. A prim is streamed in as a grid would send it, selected
//! by a **click in the world**, and everything after that is asserted through
//! the window a user would be looking at.
//!
//! # Two arrangements that are not incidental
//!
//! **The window is parked to fit.** The shipped floater is 420 × 640 logical
//! pixels; the fixture viewport is 800 × 600. At its own opening position the
//! whole tab shell hangs off the bottom, and a control laid out past the
//! viewport edge is one no pointer can click — so
//! [`open_build_floater`](crate::world_test::open_build_floater) parks it
//! through the manager's own geometry-restore path. What is under test is the
//! floater, not the viewport it is cropped by.
//!
//! **The prim is framed beside the window.** A 420-wide window in an 800-wide
//! viewport always covers the viewport centre, wherever it sits, so the usual
//! "aim the camera at the fixture and click the middle of the screen" of the
//! gizmo tests would click the window instead of the world.
//! [`frame_beside_the_window`] aims the camera so the fixture lands in the strip
//! the window leaves free, and returns the point it actually landed on — the
//! click then goes where the prim *is* rather than where a hard-coded constant
//! hoped it would be.
//!
//! # What the strings are
//!
//! The fold installs the string lookup with **no bundles behind it**
//! (`sl_viewer_ui_core::i18n::install_untranslated`), so every key resolves to
//! itself. That is deliberate: a test here asserts *which* strings a line is
//! built from — that the selection summary grows a link-number segment in
//! edit-linked-parts mode, say — and never a translation's wording, which is
//! the shipped bundles' business and is pinned in `tests/locale_bundles.rs`.
//! The arithmetic behind those segments is unit-tested where it is computed.

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::Key;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use sl_client_bevy::{Command, ScopedObjectId, Vector};
    use sl_viewer_testkit::{box_of, find_by_name, interact};

    use crate::world_api::{EditTool, EditToolState, SelectionSet};
    use crate::world_test::{
        drain_commands, entity_of, install_camera, open_build_floater, seed_child_prim,
        seed_prim_numbered, settle, world_app_with_build_tools, world_to_viewport,
    };

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Where the fixture prim stands, in Second Life region-local metres.
    const FIXTURE_AT: Vector = Vector {
        x: 128.0,
        y: 128.0,
        z: 30.0,
    };

    /// The Build Tools window's root node name — [`crate::floater::spawn_floater`]
    /// names it after the floater's stable id.
    const WINDOW: &str = "floater:build-tools";

    /// How far from the window's edge a click must land to count as "beside" it,
    /// in logical pixels. A pointer landing exactly on the boundary is a
    /// coin-toss between the window and the world, and a test that flips a coin
    /// is worse than no test.
    const CLEARANCE: f32 = 16.0;

    /// The fixture viewport, logical pixels — [`crate::world_test`]'s window.
    const VIEWPORT: Vec2 = Vec2::new(800.0, 600.0);

    // ------------------------------------------------------------------
    // The fixture
    // ------------------------------------------------------------------

    /// The fold with the Build Tools window **open and settled**: the opening
    /// move of every test here.
    fn build_tools_app() -> Result<App, TestError> {
        let mut app = world_app_with_build_tools()?;
        open_build_floater(&mut app);
        Ok(app)
    }

    /// Where the Build Tools window ended up, in logical pixels.
    fn window_box(app: &mut App) -> Result<Rect, TestError> {
        box_of(app, WINDOW).ok_or_else(|| TestError::from("the Build Tools window never laid out"))
    }

    /// Whether `at` is on screen and clear of the window by [`CLEARANCE`].
    fn beside_the_window(at: Vec2, window: Rect) -> bool {
        let on_screen = at.x > 0.0 && at.y > 0.0 && at.x < VIEWPORT.x && at.y < VIEWPORT.y;
        let clear = at.x > window.max.x + CLEARANCE
            || at.x < window.min.x - CLEARANCE
            || at.y > window.max.y + CLEARANCE
            || at.y < window.min.y - CLEARANCE;
        on_screen && clear
    }

    /// Aim the camera so every one of `targets` (Bevy world positions) lands in
    /// the strip of viewport the window does not cover, and return where each
    /// landed, in logical pixels.
    ///
    /// The search over a couple of poses is the point. Which *side* of the
    /// screen a world offset moves a fixture to depends on the camera basis and
    /// the Second Life → Bevy axis swap, and a hard-coded answer to that is a
    /// constant that silently starts clicking the window the day either
    /// convention changes. Asking the projection where the prim actually went —
    /// and refusing the pose if it went somewhere unclickable — cannot rot that
    /// way.
    fn frame_beside_the_window(app: &mut App, targets: &[Vec3]) -> Result<Vec<Vec2>, TestError> {
        /// How far the camera's aim point is offset from the fixtures, in
        /// metres: enough to push them well clear of the centred window.
        const OFFSETS: [f32; 2] = [-5.0, 5.0];
        /// How far back the camera sits, in metres. The farther pose is what
        /// fits *two* fixtures into the free strip.
        const DISTANCES: [f32; 2] = [12.0, 24.0];

        let window = window_box(app)?;
        let centre = centroid(targets);
        for distance in DISTANCES {
            for offset in OFFSETS {
                let look = Vec3::new(centre.x + offset, centre.y, centre.z);
                let eye = Vec3::new(look.x, look.y + 2.0, look.z + distance);
                install_camera(app, eye, look);
                settle(app, 2);
                let mut points = Vec::with_capacity(targets.len());
                for target in targets {
                    match world_to_viewport(app, *target) {
                        Some(at) if beside_the_window(at, window) => points.push(at),
                        _unusable => break,
                    }
                }
                if points.len() == targets.len() {
                    return Ok(points);
                }
            }
        }
        Err(TestError::from(
            "no camera pose put the fixtures in the strip of viewport the Build Tools window \
             leaves free — the window may have grown past what an 800 × 600 fixture can hold \
             beside it",
        ))
    }

    /// The mean of `points`, component-wise (the workspace's arithmetic lint
    /// fires on `glam`'s overloaded operators).
    fn centroid(points: &[Vec3]) -> Vec3 {
        let count = f32::from(u16::try_from(points.len().max(1)).unwrap_or(1));
        let mut sum = Vec3::ZERO;
        for point in points {
            sum = Vec3::new(sum.x + point.x, sum.y + point.y, sum.z + point.z);
        }
        Vec3::new(sum.x / count, sum.y / count, sum.z / count)
    }

    /// The scene position of `scoped`'s object, for the camera to aim at.
    fn scene_position(app: &mut App, scoped: ScopedObjectId) -> Result<Vec3, TestError> {
        crate::world_test::scene_position_of(app, scoped)
            .ok_or_else(|| TestError::from("the fixture prim never reached the scene"))
    }

    /// Seed one fat prim at [`FIXTURE_AT`], frame it beside the window, and
    /// select it with a **real click in the world**. Returns its scoped id and
    /// the viewport point the click landed on (the world point every later
    /// pointer gesture aims at).
    fn select_a_fixture_prim(app: &mut App) -> Result<(ScopedObjectId, Vec2), TestError> {
        let scoped = crate::world_test::seed_prim(app, FIXTURE_AT);
        settle(app, 5);
        let position = scene_position(app, scoped)?;
        let points = frame_beside_the_window(app, &[position])?;
        let at = points
            .first()
            .copied()
            .ok_or("the camera search returned no point")?;
        crate::world_test::select_by_click(app, at);
        settle(app, 3);
        assert!(
            app.world().resource::<SelectionSet>().is_selected(scoped),
            "a click beside the window at {at:?} must select the prim — everything the floater \
             shows hangs off the selection"
        );
        Ok((scoped, at))
    }

    // ------------------------------------------------------------------
    // Addressing the window's controls
    // ------------------------------------------------------------------

    /// The node name of one transform field: `group` is `pos` / `rot` / `size`,
    /// `axis` is `x` / `y` / `z`.
    fn transform_field(group: &str, axis: &str) -> String {
        format!("build-{group}-{axis}:field")
    }

    /// The nine transform fields' node names, in row-major display order.
    fn transform_fields() -> Vec<String> {
        let mut names = Vec::with_capacity(9);
        for group in ["pos", "rot", "size"] {
            for axis in ["x", "y", "z"] {
                names.push(transform_field(group, axis));
            }
        }
        names
    }

    /// A named field's text, or the empty string when it has none.
    fn field_text(app: &mut App, name: &str) -> String {
        interact::text_of(app, name).unwrap_or_default()
    }

    /// Click the tab at `index` and settle, so its page is the shown one —
    /// wheeling the strip along first when the tab is not currently in it.
    ///
    /// A hidden tab page keeps its layout box but loses its visibility, and
    /// `bevy_picking`'s UI backend will not deliver a pointer to an invisible
    /// node — so a field on an unshown page cannot be clicked, only read. That
    /// makes a silently-missed tab click the worst kind of failure here: every
    /// later assertion would be about a page nobody opened. So the click is
    /// *verified* against the strip's own state.
    ///
    /// Five build tabs do not fit a 420-pixel window — which is why the widget
    /// grows scroll arrows when its bar overflows — so a tab outside the
    /// viewport is first brought in by **pressing that arrow**, the affordance
    /// the widget puts there for exactly this. The fixture makes it routine
    /// rather than exotic: with no translation bundles the labels are their own
    /// Fluent keys, longer than any shipped English word, so the overflow this
    /// walks is a permanent feature of the harness rather than a corner it
    /// stumbles into.
    fn show_tab(app: &mut App, index: usize) -> Result<(), TestError> {
        /// How many arrow presses the strip is given to bring a tab into view.
        const SCROLL_TRIES: u32 = 20;

        let name = format!("build-tabs:tab:{index}");
        for _try in 0..SCROLL_TRIES {
            let viewport = box_of(app, "build-tabs:tab-viewport")
                .ok_or("the build floater has no laid-out tab viewport")?;
            let tab = box_of(app, &name).ok_or("the build floater has no such tab")?;
            if tab.min.x >= viewport.min.x && tab.max.x <= viewport.max.x {
                interact::click_node(app, &name)?;
                settle(app, 3);
                let active = active_tab(app)?;
                if active == index {
                    return Ok(());
                }
                return Err(TestError::from(format!(
                    "a click on tab {index} (at {tab:?}, in a viewport at {viewport:?}) left tab \
                     {active} active"
                )));
            }
            // Out of view: press the arrow that walks the bar towards it.
            let toward_end = usize::from(tab.min.x >= viewport.min.x);
            interact::click_node(app, &format!("build-tabs:tab-arrow:{toward_end}"))?;
            settle(app, 2);
        }
        Err(TestError::from(format!(
            "tab {index} never came into the bar after {SCROLL_TRIES} arrow presses"
        )))
    }

    /// Which tab the strip currently has active.
    fn active_tab(app: &mut App) -> Result<usize, TestError> {
        let strip = find_by_name(app, "build-tabs:tab-strip")
            .ok_or("the build floater has no tab strip")?;
        app.world()
            .get::<crate::ui_tab::TabStrip>(strip)
            .map(|strip| strip.active)
            .ok_or_else(|| TestError::from("the tab strip carries no state"))
    }

    /// Whether the tab page at `index` is the shown one.
    fn tab_page_shown(app: &mut App, index: usize) -> Result<bool, TestError> {
        let page = find_by_name(app, &format!("build-tab-page:{index}"))
            .ok_or("the build floater has no such tab page")?;
        Ok(app
            .world()
            .get::<sl_viewer_ui_core::ui::UiPanelShown>(page)
            .is_some_and(|shown| shown.0))
    }

    /// Click the named field and **confirm the caret landed in it**, scrolling
    /// its page when it did not.
    ///
    /// A tab page is a stack of rows taller than the window that shows it, so
    /// the field a test wants is often below the fold — and a click at a node's
    /// laid-out centre that happens to be outside its scroll viewport reaches
    /// nothing at all. Silently. Everything typed afterwards then goes to
    /// whatever *does* hold focus, and the test fails somewhere else entirely,
    /// which is how the first draft of this module spent a run blaming the
    /// Texture tab for a tab strip that had never switched.
    ///
    /// So the field is first **wheeled into its panel** — the same gesture a
    /// user makes — and the click is then confirmed by asking who holds focus.
    /// A field that is outside its panel is out of view whether it is above the
    /// fold or below it, and the wheel goes whichever way brings it back, so
    /// the loop converges instead of scrolling past.
    fn click_field(app: &mut App, name: &str) -> Result<(), TestError> {
        /// How many wheel steps the page is given to bring a field into view.
        const SCROLL_TRIES: u32 = 24;
        /// Wheel lines per step.
        const LINES: f32 = 2.0;
        /// How far inside the panel's leading edge the wheel is aimed: over the
        /// rows' labels rather than their fields, since a field is entitled to
        /// take the wheel for its own content.
        const WHEEL_INSET: f32 = 6.0;

        let entity = find_by_name(app, name)
            .ok_or_else(|| TestError::from(format!("no field named `{name}` in the window")))?;
        // A control in the window's fixed rows (the grid unit) is not on any
        // page and has nothing to scroll; only a tab page's rows do.
        let panel = active_panel_entity(app)?;
        if !descends_from(app, entity, panel) {
            interact::click_node(app, name)?;
            settle(app, 2);
            return caret_landed(app, entity, name);
        }
        for _try in 0..SCROLL_TRIES {
            let panel = active_panel_box(app)?;
            let field = box_of(app, name)
                .ok_or_else(|| TestError::from(format!("the field `{name}` never laid out")))?;
            if field.min.y >= panel.min.y && field.max.y <= panel.max.y {
                interact::click_node(app, name)?;
                settle(app, 2);
                return caret_landed(app, entity, name);
            }
            let lines = if field.min.y < panel.min.y {
                LINES
            } else {
                -LINES
            };
            let at = Vec2::new(
                panel.min.x + WHEEL_INSET,
                f32::midpoint(panel.min.y, panel.max.y),
            );
            interact::scroll(app, at, Vec2::new(0.0, lines));
            settle(app, 2);
        }
        Err(TestError::from(format!(
            "the field `{name}` never came into its panel after {SCROLL_TRIES} wheel steps"
        )))
    }

    /// The box of the tab panel currently on show — the scroll viewport a
    /// field has to be inside to be clickable.
    fn active_panel_box(app: &mut App) -> Result<Rect, TestError> {
        let active = active_tab(app)?;
        box_of(app, &format!("build-tabs:panel:{active}"))
            .ok_or_else(|| TestError::from("the active tab panel never laid out"))
    }

    /// The entity of the tab panel currently on show.
    fn active_panel_entity(app: &mut App) -> Result<Entity, TestError> {
        let active = active_tab(app)?;
        find_by_name(app, &format!("build-tabs:panel:{active}"))
            .ok_or_else(|| TestError::from("the build floater has no active tab panel"))
    }

    /// Whether `entity` sits anywhere under `ancestor`.
    fn descends_from(app: &App, entity: Entity, ancestor: Entity) -> bool {
        let mut current = entity;
        while let Some(parent) = app.world().get::<ChildOf>(current) {
            if parent.parent() == ancestor {
                return true;
            }
            current = parent.parent();
        }
        false
    }

    /// `Ok` when `entity` now holds the keyboard focus, else what went wrong.
    fn caret_landed(app: &mut App, entity: Entity, name: &str) -> Result<(), TestError> {
        if app
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get()
            == Some(entity)
        {
            return Ok(());
        }
        let at = box_of(app, name);
        Err(TestError::from(format!(
            "a click on the field `{name}` (at {at:?}) did not put the caret in it, so nothing \
             typed would reach it"
        )))
    }

    /// Replace a focused field's text with `value` and commit it with `Enter` —
    /// the whole of a numeric edit, as a user performs it.
    ///
    /// The field is cleared with `End` + `Backspace` rather than a select-all
    /// so the deletion goes through the same edit path a user's does, and the
    /// count is taken from the text that is actually there.
    fn retype_and_commit(app: &mut App, name: &str, value: &str) -> Result<(), TestError> {
        click_field(app, name)?;
        interact::tap(app, KeyCode::End, Key::End);
        for _character in 0..field_text(app, name).chars().count() {
            interact::tap(app, KeyCode::Backspace, Key::Backspace);
        }
        assert_eq!(
            field_text(app, name),
            "",
            "the field `{name}` did not clear, so what follows would be typed onto its old value"
        );
        interact::type_str(app, value);
        assert_eq!(
            field_text(app, name),
            value,
            "the field `{name}` did not take `{value}`"
        );
        interact::tap(app, KeyCode::Enter, Key::Enter);
        settle(app, 3);
        Ok(())
    }

    /// Every `UpdateObject` in `commands`, unwrapped to its transform.
    fn object_updates(commands: &[Command]) -> Vec<&sl_client_bevy::ObjectTransform> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::UpdateObject { transform, .. } => Some(transform),
                _other => None,
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Reflect
    // ------------------------------------------------------------------

    /// **A selection fills the nine transform fields.**
    ///
    /// The reflect half of the floater's contract: `sync_numeric_fields` copies
    /// the primary selection's Second Life transform into the position /
    /// rotation / size rows, and copies it *out* again when the selection goes.
    /// The fixture prim's scale (2 × 3 × 4) is asymmetric on purpose — three
    /// equal numbers would pass even if the row were wired to one axis three
    /// times.
    #[test]
    fn a_selection_fills_the_nine_transform_fields() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        for name in transform_fields() {
            assert_eq!(
                field_text(&mut app, &name),
                "",
                "field `{name}` shows a value with nothing selected"
            );
        }

        let (_scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 1)?;

        for (name, want) in [
            (transform_field("pos", "x"), "128.000"),
            (transform_field("pos", "y"), "128.000"),
            (transform_field("pos", "z"), "30.000"),
            // The fixture prim is unrotated, and the display normalises into
            // [0, 360).
            (transform_field("rot", "x"), "0.000"),
            (transform_field("rot", "y"), "0.000"),
            (transform_field("rot", "z"), "0.000"),
            (transform_field("size", "x"), "2.000"),
            (transform_field("size", "y"), "3.000"),
            (transform_field("size", "z"), "4.000"),
        ] {
            assert_eq!(
                field_text(&mut app, &name),
                want,
                "field `{name}` does not mirror the selected prim"
            );
        }

        // …and the fields empty again when the selection does, rather than
        // going on showing an object nobody is editing.
        app.world_mut().resource_mut::<SelectionSet>().clear();
        settle(&mut app, 3);
        for name in transform_fields() {
            assert_eq!(
                field_text(&mut app, &name),
                "",
                "field `{name}` still shows a value after the selection was cleared"
            );
        }
        Ok(())
    }

    /// **An update from the grid refreshes the fields.**
    ///
    /// The other direction of reflect, and the one a resting window has to get
    /// right: the prim is moved by *somebody else* — another avatar's edit, a
    /// physics push, a script — and the numbers in the window have to follow it
    /// rather than go on showing where it used to be. `sync_numeric_fields`
    /// re-reads the motion mirror every frame for exactly this, skipping only
    /// the field the user is typing into.
    #[test]
    fn an_update_from_the_grid_refreshes_the_transform_fields() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (_scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 1)?;
        assert_eq!(
            field_text(&mut app, &transform_field("pos", "z")),
            "30.000",
            "the fields must start on the seeded position"
        );

        let moved = crate::world_test::fixture_prim(
            1,
            Vector {
                x: FIXTURE_AT.x,
                y: FIXTURE_AT.y,
                z: 42.5,
            },
            0,
        );
        app.world_mut().write_message(sl_client_bevy::SlEvent(
            sl_client_bevy::SlSessionEvent::ObjectUpdated(Box::new(moved)),
        ));
        settle(&mut app, 4);

        assert_eq!(
            field_text(&mut app, &transform_field("pos", "z")),
            "42.500",
            "an object update must move the numbers in the window with the prim"
        );
        Ok(())
    }

    /// **The summary line says what is selected.**
    ///
    /// `update_selection_summary` builds one line out of up to five segments,
    /// and which segments appear is the behaviour: nothing selected is its own
    /// string, a selection carries a count and a prim count, and *only* in
    /// edit-linked-parts mode with exactly one part does it carry that part's
    /// link number. The bundles are not loaded in this fixture, so each segment
    /// is its own Fluent key — which is what makes the assertion about the line's
    /// composition rather than about anybody's translation.
    #[test]
    fn the_summary_line_says_what_is_selected() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        assert_eq!(
            field_summary(&mut app),
            "build-selection-none",
            "an empty selection must say so"
        );

        let (_scoped, _at) = select_a_fixture_prim(&mut app)?;
        let summary = field_summary(&mut app);
        assert!(
            summary.contains("build-selection-count"),
            "a selection must be counted: {summary}"
        );
        assert!(
            summary.contains("build-selection-prims"),
            "a selection must carry its linkset prim count: {summary}"
        );
        assert!(
            !summary.contains("build-selection-link"),
            "a whole-linkset selection has no link number to show: {summary}"
        );

        // Edit linked parts, one part selected: the link-number segment appears.
        app.world_mut().resource_mut::<EditToolState>().edit_linked = true;
        settle(&mut app, 3);
        let summary = field_summary(&mut app);
        assert!(
            summary.contains("build-selection-link"),
            "one part selected in edit-linked-parts mode must show its link number: {summary}"
        );
        Ok(())
    }

    /// The selection summary's current text.
    fn field_summary(app: &mut App) -> String {
        find_by_name(app, "build-tools:summary")
            .and_then(|entity| app.world().get::<Text>(entity).map(|text| text.0.clone()))
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Edit and commit
    // ------------------------------------------------------------------

    /// **Typing a position and pressing `Enter` sends exactly one update.**
    ///
    /// The commit half, and the one place the whole chain is visible at once: a
    /// click focuses the field, the keystrokes reach its buffer, `Enter` parses
    /// the *whole row* (so the two untouched axes keep their displayed values),
    /// and one `UpdateObject` goes out carrying a position and nothing else. A
    /// commit that also sent a rotation or a scale would move the prim in ways
    /// the user never asked for.
    #[test]
    fn typing_a_position_and_committing_sends_one_update() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 1)?;
        let _settling = drain_commands(&mut app);

        retype_and_commit(&mut app, &transform_field("pos", "x"), "130.5")?;

        let commands = drain_commands(&mut app);
        let updates = object_updates(&commands);
        assert_eq!(
            updates.len(),
            1,
            "one committed field must send exactly one object update, got {commands:#?}"
        );
        let Some(transform) = updates.first() else {
            return Err(TestError::from("no update to read"));
        };
        let Some(position) = transform.position.as_ref() else {
            return Err(TestError::from("the update carries no position"));
        };
        assert!(
            (position.x - 130.5).abs() < 1e-3,
            "the typed X must reach the wire, got {}",
            position.x
        );
        assert!(
            (position.y - FIXTURE_AT.y).abs() < 1e-3 && (position.z - FIXTURE_AT.z).abs() < 1e-3,
            "the untouched axes must keep their displayed values, got ({}, {})",
            position.y,
            position.z
        );
        assert!(
            transform.rotation.is_none() && transform.scale.is_none(),
            "a position commit must carry only a position: {transform:#?}"
        );

        // The local echo: the scene follows immediately rather than waiting for
        // the simulator's own update, which is what keeps the gizmo on the prim.
        let entity = entity_of(&mut app, scoped).ok_or("the fixture prim has no entity")?;
        let moved = app
            .world()
            .get::<crate::objects::ObjectSlMotion>(entity)
            .map(|motion| motion.position.x)
            .unwrap_or_default();
        assert!(
            (moved - 130.5).abs() < 1e-3,
            "the local echo must move the prim, got {moved}"
        );
        Ok(())
    }

    /// **A letter never reaches a transform field, and commits nothing.**
    ///
    /// The `TextInputKind::Float` filter rejects the keystroke before the
    /// buffer, so the field is unchanged and `Enter` re-commits nothing — the
    /// difference between a field that refuses bad input and one that accepts
    /// it and sends garbage to the simulator.
    #[test]
    fn a_letter_never_reaches_a_transform_field() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (_scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 1)?;
        let _settling = drain_commands(&mut app);

        let name = transform_field("pos", "x");
        interact::click_node(&mut app, &name)?;
        interact::tap(&mut app, KeyCode::End, Key::End);
        interact::type_str(&mut app, "abc");
        assert_eq!(
            field_text(&mut app, &name),
            "128.000",
            "a letter is not a float character, so it must never enter the field"
        );

        interact::tap(&mut app, KeyCode::Enter, Key::Enter);
        settle(&mut app, 3);
        let commands = drain_commands(&mut app);
        let updates = object_updates(&commands);
        assert_eq!(
            updates.len(),
            1,
            "an `Enter` on an unchanged field still commits the row it is in — what must not \
             happen is a *different* value reaching the wire, got {commands:#?}"
        );
        let Some(position) = updates.first().and_then(|update| update.position.as_ref()) else {
            return Err(TestError::from("the update carries no position"));
        };
        assert!(
            (position.x - FIXTURE_AT.x).abs() < 1e-3,
            "the rejected keystrokes must not have changed the committed value, got {}",
            position.x
        );
        Ok(())
    }

    /// **The grid-unit field commits into the tool state, not onto the wire.**
    ///
    /// It is the one field in the row group that is not an object edit: it is
    /// the gizmos' snap resolution, clamped to a sane range, and nothing about
    /// it belongs on the wire.
    #[test]
    fn the_grid_unit_field_commits_into_the_tool_state() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (_scoped, _at) = select_a_fixture_prim(&mut app)?;
        let _settling = drain_commands(&mut app);

        retype_and_commit(&mut app, "build-grid-unit:field", "2.5")?;
        let grid = app.world().resource::<EditToolState>().grid_unit;
        assert!(
            (grid - 2.5).abs() < 1e-3,
            "the committed grid unit must reach the tool state, got {grid}"
        );
        assert!(
            object_updates(&drain_commands(&mut app)).is_empty(),
            "a grid-unit edit is not an object edit and must send nothing"
        );

        // …and it is clamped: the reference's grid runs from a centimetre to
        // ten metres.
        retype_and_commit(&mut app, "build-grid-unit:field", "99")?;
        let grid = app.world().resource::<EditToolState>().grid_unit;
        assert!(
            (grid - 10.0).abs() < 1e-3,
            "an out-of-range grid unit must clamp rather than take, got {grid}"
        );
        Ok(())
    }

    // ------------------------------------------------------------------
    // The shell's own controls
    // ------------------------------------------------------------------

    /// **The tool radio and the tool state follow each other, both ways.**
    ///
    /// Clicking an option is the user's half; a tool changed from anywhere else
    /// (a shortcut, the pie, a test) must move the dot back, or the floater
    /// would be showing a manipulator that is not the one in the world.
    #[test]
    fn the_tool_radio_and_the_tool_state_follow_each_other() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        assert_eq!(
            app.world().resource::<EditToolState>().tool,
            EditTool::Move,
            "the floater opens on the move tool"
        );

        // Click each option in turn: the radio order is the `BUILD_TOOLS` order.
        for (index, tool) in crate::world_api::BUILD_TOOLS.iter().enumerate() {
            interact::click_node(&mut app, &format!("build-tool:radio:{index}"))?;
            settle(&mut app, 3);
            assert_eq!(
                app.world().resource::<EditToolState>().tool,
                *tool,
                "clicking radio option {index} must pick {tool:?}"
            );
        }

        // …and back the other way.
        app.world_mut().resource_mut::<EditToolState>().tool = EditTool::Rotate;
        settle(&mut app, 3);
        let radio = find_by_name(&mut app, "build-tool:radio-group")
            .ok_or("the tool radio group is not in the window")?;
        let active = app
            .world()
            .get::<crate::ui_radio::RadioSelection>(radio)
            .map(|selection| selection.active)
            .ok_or("the tool radio group carries no selection")?;
        assert_eq!(
            active,
            EditTool::Rotate.radio_index(),
            "a tool changed from elsewhere must move the dot"
        );
        Ok(())
    }

    /// **Each toggle row flips its flag, and says so.**
    ///
    /// The four toggles are the ones every other build system reads — snapping,
    /// the grid frame, edit-linked-parts and stretch-both — and each is a row
    /// whose *glyph* is the only thing the user sees. A flag that flipped
    /// without the glyph following would be a control that lies about its state.
    #[test]
    fn each_toggle_row_flips_its_flag() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        for (key, before) in [
            ("build-toggle-snap", true),
            ("build-toggle-local-frame", false),
            ("build-toggle-edit-linked", false),
            ("build-toggle-stretch-both", false),
        ] {
            let row = format!("build-tools:{key}");
            assert_eq!(
                toggle_state(&app, key)?,
                before,
                "`{key}` does not start where the reference defaults put it"
            );
            interact::click_node(&mut app, &row)?;
            settle(&mut app, 3);
            assert_eq!(
                toggle_state(&app, key)?,
                !before,
                "a click on `{key}` must flip it"
            );
            assert_eq!(
                toggle_glyph(&mut app, &row),
                if before {
                    crate::edit_tool::UNCHECKED_GLYPH
                } else {
                    crate::edit_tool::CHECKED_GLYPH
                },
                "`{key}`'s glyph must follow its flag"
            );
            // Put it back, so each toggle is checked from the shipped defaults
            // rather than from whatever the previous one left behind.
            interact::click_node(&mut app, &row)?;
            settle(&mut app, 3);
            assert_eq!(
                toggle_state(&app, key)?,
                before,
                "a second click on `{key}` must flip it back"
            );
        }
        Ok(())
    }

    /// The tool-state flag one toggle row governs.
    fn toggle_state(app: &App, key: &str) -> Result<bool, TestError> {
        let state = app.world().resource::<EditToolState>();
        Ok(match key {
            "build-toggle-snap" => state.snap,
            "build-toggle-local-frame" => state.frame == crate::world_api::GridFrame::Local,
            "build-toggle-edit-linked" => state.edit_linked,
            "build-toggle-stretch-both" => state.stretch_both,
            _unknown => return Err(TestError::from("no such build toggle")),
        })
    }

    /// The check glyph a toggle row currently draws.
    fn toggle_glyph(app: &mut App, row: &str) -> String {
        let Some(entity) = find_by_name(app, row) else {
            return String::new();
        };
        let children: Vec<Entity> = app
            .world()
            .get::<Children>(entity)
            .map(|children| children.iter().collect())
            .unwrap_or_default();
        children
            .into_iter()
            .find_map(|child| app.world().get::<Text>(child).map(|text| text.0.clone()))
            .unwrap_or_default()
    }

    /// **`Ctrl+B` opens and closes the build window — and closing it leaves
    /// build mode.**
    ///
    /// An open Build Tools window *is* edit mode: the shortcut is the only way
    /// in for a user with no toolbar, and the close edge has to clear the
    /// selection (which also deselects on the wire), or the world would stay
    /// selected under a window nobody can see.
    #[test]
    fn ctrl_b_opens_and_closes_the_build_window() -> Result<(), TestError> {
        // Deliberately *not* `build_tools_app`: this test is about the opening.
        let mut app = world_app_with_build_tools()?;
        assert!(
            !app.world().resource::<EditToolState>().active,
            "the viewer does not start in build mode"
        );

        press_ctrl_b(&mut app);
        assert!(
            app.world().resource::<EditToolState>().active,
            "`Ctrl+B` must open the build window and so enter build mode"
        );
        assert!(
            find_by_name(&mut app, "build-tools:summary").is_some(),
            "the window's deferred content must have been built"
        );

        // Select something, then close: the selection must not outlive the
        // window.
        let (scoped, _at) = select_a_fixture_prim(&mut app)?;
        press_ctrl_b(&mut app);
        assert!(
            !app.world().resource::<EditToolState>().active,
            "a second `Ctrl+B` must close the window and leave build mode"
        );
        assert!(
            !app.world().resource::<SelectionSet>().is_selected(scoped),
            "closing the build window must clear the selection"
        );
        Ok(())
    }

    /// Press `Ctrl+B` and let the toggle settle.
    fn press_ctrl_b(app: &mut App) {
        interact::with_modifier(app, KeyCode::ControlLeft, Key::Control, |app| {
            interact::tap(app, KeyCode::KeyB, Key::Character("b".into()));
        });
        settle(app, 4);
    }

    /// **Every tab shows its own page, and every page has its editors docked.**
    ///
    /// The tab shell is what the five per-aspect editors dock into, and each
    /// fills its page from a *different* module on a `BuildTabPages`-appeared
    /// run condition. A page whose editor never spawned looks exactly like a
    /// page that is simply not shown, so each tab is asserted twice: the page is
    /// the shown one, and a control that only that module spawns is in the
    /// window.
    #[test]
    fn every_tab_shows_its_own_page_and_its_editors() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        // One witness per page, each spawned by the module that owns that tab:
        // the General / Object / Features pages by `edit_params`, the Texture
        // page by `edit_texture`, the Content page by `edit_contents`.
        let witnesses = [
            (0_usize, "build-name:field"),
            (1, "build-pos-x:field"),
            (2, "build-params:build-feature-flexi"),
            (3, "build-tex-glow:field"),
            (4, "contents:count"),
        ];
        for (index, witness) in witnesses {
            assert!(
                find_by_name(&mut app, witness).is_some(),
                "tab page {index} has no `{witness}` — its editor never docked"
            );
        }

        for (index, _witness) in witnesses {
            show_tab(&mut app, index)?;
            for (other, _witness) in witnesses {
                assert_eq!(
                    tab_page_shown(&mut app, other)?,
                    other == index,
                    "with tab {index} picked, page {other} has the wrong shown state"
                );
            }
        }
        Ok(())
    }

    /// **The linked-part buttons walk the linkset, and only in the mode that
    /// has parts.**
    ///
    /// The prev / next row is hidden outside edit-linked-parts mode — there are
    /// no parts to cycle when the selection is whole linksets — and inside it
    /// each press replaces the selection with the next member in the linkset's
    /// stable order, wrapping.
    #[test]
    fn the_linked_part_buttons_walk_the_linkset() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let root = seed_prim_numbered(&mut app, 1, FIXTURE_AT);
        settle(&mut app, 5);
        let child = seed_child_prim(
            &mut app,
            1,
            2,
            Vector {
                x: 0.0,
                y: 0.0,
                z: 3.0,
            },
        );
        settle(&mut app, 5);

        // The row is hidden while whole linksets are selected.
        assert_eq!(
            link_nav_display(&mut app)?,
            Display::None,
            "the linked-part row has nothing to do outside edit-linked-parts mode"
        );
        app.world_mut().resource_mut::<EditToolState>().edit_linked = true;
        settle(&mut app, 3);
        assert_eq!(
            link_nav_display(&mut app)?,
            Display::Flex,
            "edit-linked-parts mode must reveal the linked-part row"
        );

        // Select the root part, then walk.
        select_directly(&mut app, root)?;
        interact::click_node(&mut app, "build-tools:link-part-next")?;
        settle(&mut app, 3);
        assert!(
            app.world().resource::<SelectionSet>().is_selected(child),
            "next must step from the root to its child"
        );
        interact::click_node(&mut app, "build-tools:link-part-next")?;
        settle(&mut app, 3);
        assert!(
            app.world().resource::<SelectionSet>().is_selected(root),
            "next must wrap from the last part back to the root"
        );
        interact::click_node(&mut app, "build-tools:link-part-prev")?;
        settle(&mut app, 3);
        assert!(
            app.world().resource::<SelectionSet>().is_selected(child),
            "prev must wrap the other way"
        );
        Ok(())
    }

    /// The `display` of the linked-part navigation row.
    fn link_nav_display(app: &mut App) -> Result<Display, TestError> {
        let row = find_by_name(app, "build-tools:link-part-nav")
            .ok_or("the linked-part row is not in the window")?;
        app.world()
            .get::<Node>(row)
            .map(|node| node.display)
            .ok_or_else(|| TestError::from("the linked-part row is not a node"))
    }

    /// Select `scoped` without a pointer — for the cases where *what* is
    /// selected is the fixture rather than the thing under test (a linkset's
    /// child part is behind its root on screen).
    fn select_directly(app: &mut App, scoped: ScopedObjectId) -> Result<(), TestError> {
        let entity = entity_of(app, scoped).ok_or("the fixture prim has no entity")?;
        let full = sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::from_u128(u128::from(
            scoped.id.0,
        )));
        app.world_mut()
            .resource_mut::<SelectionSet>()
            .select_only(scoped, full, entity);
        settle(app, 3);
        Ok(())
    }

    // ------------------------------------------------------------------
    // The per-aspect editors
    // ------------------------------------------------------------------

    /// **A General-tab name edit commits a `SetObjectName`.**
    ///
    /// The simplest of the parameter tabs' commits, and the one that proves the
    /// tab's fields are wired to the same focus-and-`Enter` path the transform
    /// rows use — a page that reflected values but committed nothing would look
    /// identical at rest.
    #[test]
    fn a_name_edit_commits_a_set_object_name() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 0)?;
        let _settling = drain_commands(&mut app);

        retype_and_commit(&mut app, "build-name:field", "a renamed prim")?;

        let commands = drain_commands(&mut app);
        let named: Vec<&Command> = commands
            .iter()
            .filter(|command| matches!(command, Command::SetObjectName { .. }))
            .collect();
        assert_eq!(
            named.len(),
            1,
            "one committed name must send exactly one rename, got {commands:#?}"
        );
        match named.first() {
            Some(Command::SetObjectName { local_id, name }) => {
                assert_eq!(*local_id, scoped, "the rename must address the selection");
                assert_eq!(name, "a renamed prim", "the typed name must reach the wire");
            }
            _other => return Err(TestError::from("no rename to read")),
        }
        Ok(())
    }

    /// **The General tab's group Set… opens the picker, and the picker's answer
    /// commits a `SetObjectGroup`** (`viewer-region-estate-group-picker`).
    ///
    /// Both halves, because the first shipped with only the first working: the
    /// button opened a picker, a group was chosen, and nothing left the viewer.
    /// The handler was waiting on the selection's `ObjectProperties` — which
    /// the simulator sends when it feels like it, and had not yet — so the
    /// *send* sat behind a local echo it had no business depending on.
    #[test]
    fn the_group_set_button_opens_a_picker_whose_answer_commits() -> Result<(), TestError> {
        use crate::world_api::{GroupPicked, OpenGroupPicker};
        use sl_client_bevy::{GroupKey, Uuid};

        let mut app = build_tools_app()?;
        // Recorded rather than drained off the queue: a message lives two
        // frames and `settle` runs two updates, so reading `Messages` directly
        // races the buffer swap (see `sl_viewer_testkit::drain_actions`).
        sl_viewer_testkit::record::<OpenGroupPicker>(&mut app);
        let (scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 0)?;
        let _settling = drain_commands(&mut app);

        interact::click_node(&mut app, "build-params:action:build-set-group")?;
        settle(&mut app, 2);
        let opens = sl_viewer_testkit::drain::<OpenGroupPicker>(&mut app);
        let open = opens.last().ok_or("Set… opened no group picker")?;
        assert!(
            open.allow_none,
            "an object's group may be cleared, so the picker must offer none"
        );

        // The picker's answer, naming the button that asked — the seam the
        // window test in `group_picker` drives from the other side.
        let chosen = GroupKey::from(Uuid::from_u128(0x00C0_FFEE));
        app.world_mut().write_message(GroupPicked {
            requester: open.requester,
            group: Some(chosen),
            name: "Cartographers".to_owned(),
        });
        settle(&mut app, 2);

        let commands = drain_commands(&mut app);
        let set: Vec<&Command> = commands
            .iter()
            .filter(|command| matches!(command, Command::SetObjectGroup { .. }))
            .collect();
        assert_eq!(
            set.len(),
            1,
            "one confirmed pick must send exactly one group set, got {commands:#?}"
        );
        match set.first() {
            Some(Command::SetObjectGroup {
                local_ids,
                group_id,
            }) => {
                assert_eq!(local_ids.as_slice(), &[scoped]);
                assert_eq!(*group_id, chosen);
            }
            _other => return Err(TestError::from("no group set to read")),
        }
        Ok(())
    }

    /// **Confirming a group keeps the object selected.**
    ///
    /// The whole gesture, through the real pointer: press Set…, press a row in
    /// the picker that opens, press OK. The picker is a keyed floater, so OK
    /// **despawns the window under the cursor** — and the window is over the
    /// world, because a picker opens beside the floater that asked rather than
    /// on top of it.
    ///
    /// That despawn used to throw the selection away. The world-pick gesture
    /// decides on the press frame whether the pointer is over UI by looking for
    /// the pressed node in the hover map; a node that despawned during its own
    /// press leaves an entry with no `ComputedNode` behind it, which reads as
    /// "not a UI surface", so the press was taken for a click on empty world —
    /// and a click on empty world *deselects*. `install_ui_pointer_claim`'s
    /// observer now claims the press while the node is still alive.
    ///
    /// Driven with the pointer rather than by writing `GroupPicked`, because
    /// writing the message is exactly the step that skips the press this is
    /// about — which is why the sibling test above did not catch it.
    #[test]
    fn confirming_a_group_keeps_the_selection() -> Result<(), TestError> {
        use crate::world_api::GroupsModel;
        use sl_client_bevy::{GroupKey, GroupMembership, LandArea, TextureKey, Uuid};

        let mut app = build_tools_app()?;
        // A group to pick. The picker lists the agent's memberships, and an
        // agent in none of them has no row to press.
        app.world_mut()
            .resource_mut::<GroupsModel>()
            .apply_memberships(&[GroupMembership {
                group_id: GroupKey::from(Uuid::from_u128(0x00C0_FFEE)),
                group_powers: 0,
                accept_notices: true,
                group_insignia_id: TextureKey::from(Uuid::nil()),
                contribution: LandArea(0),
                group_name: "Cartographers".to_owned(),
            }]);
        let (scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 0)?;
        let _settling = drain_commands(&mut app);

        interact::click_node(&mut app, "build-params:action:build-set-group")?;
        settle(&mut app, 3);
        // Row 0 is the "none" row; row 1 is the one group seeded above.
        interact::click_node(&mut app, "group-picker-row:1")?;
        settle(&mut app, 2);
        interact::click_node(&mut app, "group-picker:group-picker-ok")?;
        settle(&mut app, 3);

        assert!(
            app.world().resource::<SelectionSet>().is_selected(scoped),
            "confirming a group deselected the object it was chosen for"
        );
        let commands = drain_commands(&mut app);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, Command::SetObjectGroup { .. })),
            "the confirmed pick sent no group set: {commands:#?}"
        );
        Ok(())
    }

    /// **An Object-tab shape edit commits a `SetObjectShape`.**
    ///
    /// The shape fields are quantized on the way out (the wire carries bytes,
    /// not floats), so this asserts a command went and addressed the right
    /// prim rather than a round-tripped float — the quantizers have their own
    /// inverse tests in `edit_params`.
    #[test]
    fn a_shape_edit_commits_a_set_object_shape() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 1)?;
        let _settling = drain_commands(&mut app);

        retype_and_commit(&mut app, "build-hollow:field", "25")?;

        let commands = drain_commands(&mut app);
        let shaped: Vec<&Command> = commands
            .iter()
            .filter(|command| matches!(command, Command::SetObjectShape { .. }))
            .collect();
        assert_eq!(
            shaped.len(),
            1,
            "one committed shape field must send exactly one shape update, got {commands:#?}"
        );
        if let Some(Command::SetObjectShape { local_id, .. }) = shaped.first() {
            assert_eq!(
                *local_id, scoped,
                "the shape update must address the selection"
            );
        }
        Ok(())
    }

    /// **A Texture-tab edit commits an `ObjectImage` for the selection.**
    ///
    /// The texture tab does not send its field: it rebuilds the object's whole
    /// `TextureEntry` from the faces the renderer is actually drawing and sends
    /// that, so that editing one attribute of one face leaves every other face
    /// and every other attribute alone. What this pins is that the rebuild
    /// reaches the wire at all — the entry's per-face content is `edit_texture`'s
    /// own tests' subject.
    #[test]
    fn a_texture_edit_commits_an_object_image() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (scoped, _at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 3)?;
        let _settling = drain_commands(&mut app);

        retype_and_commit(&mut app, "build-tex-glow:field", "0.5")?;

        let commands = drain_commands(&mut app);
        let images: Vec<&Command> = commands
            .iter()
            .filter(|command| matches!(command, Command::SetObjectImage { .. }))
            .collect();
        assert_eq!(
            images.len(),
            1,
            "one committed texture field must send exactly one texture entry, got {commands:#?}"
        );
        if let Some(Command::SetObjectImage { local_id, .. }) = images.first() {
            assert_eq!(
                *local_id, scoped,
                "the texture entry must address the selection"
            );
        }
        Ok(())
    }

    /// **The Texture tab follows a face selection.**
    ///
    /// The Select Face tool turns a click into a *face* rather than an object,
    /// and the Texture tab is the surface that has to say so — its faces line
    /// is the user's only signal of how wide the next edit will be. Whole
    /// object and one face are two different strings, and confusing them is how
    /// a one-face tint ends up on all six.
    #[test]
    fn the_texture_tab_follows_a_face_selection() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (_scoped, at) = select_a_fixture_prim(&mut app)?;
        show_tab(&mut app, 3)?;
        assert_eq!(
            texture_faces_line(&mut app),
            "build-tex-faces-all",
            "an ordinary object selection edits every face, and must say so"
        );

        // Select Face is the last of `BUILD_TOOLS`.
        let select_face = EditTool::SelectFace.radio_index();
        interact::click_node(&mut app, &format!("build-tool:radio:{select_face}"))?;
        settle(&mut app, 3);
        crate::world_test::select_by_click(&mut app, at);
        settle(&mut app, 3);

        let faces = app
            .world()
            .resource::<SelectionSet>()
            .primary_faces()
            .map(|set| set.len())
            .unwrap_or_default();
        assert_eq!(faces, 1, "a face click selects exactly one face");
        assert_eq!(
            texture_faces_line(&mut app),
            "build-tex-faces-count",
            "the Texture tab must say the edit is now scoped to a face count"
        );
        Ok(())
    }

    /// The Texture tab's "which faces will this hit" line.
    fn texture_faces_line(app: &mut App) -> String {
        find_by_name(app, "build-tex-info:Faces")
            .and_then(|entity| app.world().get::<Text>(entity).map(|text| text.0.clone()))
            .unwrap_or_default()
    }

    /// **`Ctrl+L` links the two selected prims, primary first.**
    ///
    /// Linking is the one build action whose *order* is data: the last object
    /// picked becomes the linkset root, which is the workflow ("select the
    /// intended root last") the reference teaches. Two real clicks — one plain,
    /// one with `Shift` — are what put two objects in the set in a known order.
    #[test]
    fn ctrl_l_links_the_two_selected_prims() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let first = seed_prim_numbered(&mut app, 1, FIXTURE_AT);
        let second = seed_prim_numbered(
            &mut app,
            2,
            Vector {
                x: FIXTURE_AT.x,
                y: FIXTURE_AT.y,
                // Six metres up: the two boxes are four tall, so they separate
                // *vertically* on screen and neither has to compete for the
                // narrow strip of viewport beside the window.
                z: FIXTURE_AT.z + 6.0,
            },
        );
        settle(&mut app, 5);
        let positions = [
            scene_position(&mut app, first)?,
            scene_position(&mut app, second)?,
        ];
        let points = frame_beside_the_window(&mut app, &positions)?;
        let (Some(at_first), Some(at_second)) = (points.first().copied(), points.get(1).copied())
        else {
            return Err(TestError::from("the camera search returned too few points"));
        };

        crate::world_test::select_by_click(&mut app, at_first);
        interact::with_modifier(&mut app, KeyCode::ShiftLeft, Key::Shift, |app| {
            crate::world_test::select_by_click(app, at_second);
        });
        settle(&mut app, 3);
        {
            let selection = app.world().resource::<SelectionSet>();
            assert!(
                selection.is_selected(first) && selection.is_selected(second),
                "a click and a shift-click must leave both prims selected"
            );
        }
        let _settling = drain_commands(&mut app);

        interact::with_modifier(&mut app, KeyCode::ControlLeft, Key::Control, |app| {
            interact::tap(app, KeyCode::KeyL, Key::Character("l".into()));
        });
        settle(&mut app, 3);

        let commands = drain_commands(&mut app);
        let links: Vec<&Command> = commands
            .iter()
            .filter(|command| matches!(command, Command::LinkObjects { .. }))
            .collect();
        assert_eq!(
            links.len(),
            1,
            "`Ctrl+L` on two selected prims must send exactly one link, got {commands:#?}"
        );
        if let Some(Command::LinkObjects { local_ids }) = links.first() {
            assert_eq!(
                local_ids.first().copied(),
                Some(second),
                "the last-picked object leads and becomes the linkset root"
            );
            assert_eq!(
                local_ids.len(),
                2,
                "both selected prims must be in the link"
            );
        }
        Ok(())
    }

    /// **`Ctrl+Shift+L` unlinks the selected linkset.**
    ///
    /// Linking is a shortcut on the same selection the floater is showing, and
    /// it is gated on build mode being active and on the keyboard *not* being
    /// owned by a text field — which is exactly the arrangement this fold has.
    #[test]
    fn ctrl_shift_l_unlinks_the_selected_linkset() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let root = seed_prim_numbered(&mut app, 1, FIXTURE_AT);
        settle(&mut app, 5);
        let _child = seed_child_prim(
            &mut app,
            1,
            2,
            Vector {
                x: 0.0,
                y: 0.0,
                z: 3.0,
            },
        );
        settle(&mut app, 5);
        select_directly(&mut app, root)?;
        let _settling = drain_commands(&mut app);

        interact::with_modifier(&mut app, KeyCode::ControlLeft, Key::Control, |app| {
            interact::with_modifier(app, KeyCode::ShiftLeft, Key::Shift, |app| {
                interact::tap(app, KeyCode::KeyL, Key::Character("l".into()));
            });
        });
        settle(&mut app, 3);

        let commands = drain_commands(&mut app);
        let delinks: Vec<&Command> = commands
            .iter()
            .filter(|command| matches!(command, Command::DelinkObjects { .. }))
            .collect();
        assert_eq!(
            delinks.len(),
            1,
            "`Ctrl+Shift+L` on a selected linkset must send exactly one delink, got {commands:#?}"
        );
        Ok(())
    }

    /// **The Create tool rezzes where the click landed.**
    ///
    /// The create click is not a selection click: it ray-casts the world, takes
    /// the surface point, and sends a `RezObject` in the region's Second Life
    /// frame. Aiming it at the fixture prim's own surface is what makes the
    /// expected position knowable — the ray strikes a box whose position the
    /// test seeded.
    #[test]
    fn the_create_tool_rezzes_where_the_click_landed() -> Result<(), TestError> {
        let mut app = build_tools_app()?;
        let (_scoped, at) = select_a_fixture_prim(&mut app)?;

        // The Create tool is radio option 0 — the first of `BUILD_TOOLS`.
        interact::click_node(&mut app, "build-tool:radio:0")?;
        settle(&mut app, 3);
        assert_eq!(
            app.world().resource::<EditToolState>().tool,
            EditTool::Create,
            "the create panel's tests below assume the Create tool is picked"
        );
        let _settling = drain_commands(&mut app);

        interact::hover(&mut app, at);
        interact::press(&mut app, MouseButton::Left);
        interact::release(&mut app, MouseButton::Left);
        settle(&mut app, 3);

        let commands = drain_commands(&mut app);
        let rezzes: Vec<&Command> = commands
            .iter()
            .filter(|command| matches!(command, Command::RezObject { .. }))
            .collect();
        assert_eq!(
            rezzes.len(),
            1,
            "one build click must rez exactly one object, got {commands:#?}"
        );
        let Some(Command::RezObject { shape, .. }) = rezzes.first() else {
            return Err(TestError::from("no rez to read"));
        };
        // The ray struck the fixture prim, whose box spans a metre either side
        // of its seeded centre in X and Y and two in Z; anything outside that
        // is not the surface the pointer was over.
        assert!(
            (shape.position.x - FIXTURE_AT.x).abs() < 2.0
                && (shape.position.y - FIXTURE_AT.y).abs() < 2.5
                && (shape.position.z - FIXTURE_AT.z).abs() < 3.0,
            "the rez must land on the surface the click struck, got {:?}",
            shape.position
        );
        Ok(())
    }
}
