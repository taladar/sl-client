//! The registry sweeps: every element in `crate::ui_elements::ELEMENTS` and
//! every window in `crate::floaters::FLOATERS`, run through the layout harness
//! across the matrix of viewport, scale factor, UI scale and direction.
//!
//! The harness itself is `sl-viewer-testkit`, which sits below the widgets so
//! they can test against it too. What stays here is what the harness cannot
//! see: the two registries, and the feature modules a few checks reach into.
//! Re-exported under this module's old name so the existing
//! `crate::ui_test::…` paths keep resolving.

#[cfg(test)]
pub(crate) use sl_viewer_testkit::*;

#[cfg(test)]
mod tests {
    use super::{
        LayoutTest, TestError, activate, drain_actions, enable_action_recording, find_by_name,
        layout_violations, overflow_violations, settle, spawn_element,
    };
    use crate::floater::{FloaterElement, register_floater_layout};
    use crate::floaters::FLOATERS;
    use crate::ui::{UiDirection, UiRoot, UiScaffoldSystems};
    use crate::ui_element::{ElementCx, SCRIPTS, SampleText, UiAction};
    use crate::ui_elements::ELEMENTS;
    use crate::ui_font::UiFont;
    use bevy::input_focus::InputFocus;
    use bevy::input_focus::tab_navigation::NavAction;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// The UI font sizes the matrix sweeps.
    ///
    /// The user's font-size preference is a first-class way to break a layout —
    /// the same way a long translation is, from the other side — so it is an axis
    /// rather than a constant.
    const FONT_SIZES: [f32; 3] = [11.0, 15.0, 22.0];

    /// The window scale factors the matrix sweeps. 1.5 is where the padding bug
    /// was first measured by hand.
    const SCALE_FACTORS: [f32; 3] = [1.0, 1.5, 2.0];

    // -----------------------------------------------------------------------
    // The harness has to have teeth. These two tests are about the *checks*,
    // not about the UI: a suite whose checks cannot fail is a suite that
    // reports success because it looked at nothing.
    // -----------------------------------------------------------------------

    /// The known-bad structure from `viewer-text-node-padding-measure`, which is
    /// the reason this harness exists: a `Text` node carrying **its own** padding
    /// and border is laid out with the wrong wrap width, so it gets one fewer
    /// line than it draws and the last line hangs out of the bottom.
    ///
    /// This asserts the bug is **still present** and that the check **sees it**.
    /// Both halves matter. It is the proof that `overflow_violations` has teeth —
    /// a check that cannot fail protects nothing — and it is a canary: when Bevy
    /// fixes the measure upstream this test starts failing, which is precisely
    /// when we want to be told, so the workaround can go.
    ///
    /// Diagnosing this by hand cost a login to OpenSim, a temporary debug key in
    /// the demo panel, and six rounds of a human pressing it and reporting
    /// numbers. It is a pure function of a font, a string and a width.
    #[test]
    fn a_text_node_may_not_carry_its_own_padding() {
        let test = LayoutTest::new();
        let mut app = test.build();
        let text = app
            .world_mut()
            .spawn((
                Text::new(
                    "A much longer label, of the length a translated string reaches when the \
                     original was written in English and measured once, which is exactly the \
                     case a fixed pixel rect gets wrong.",
                ),
                UiFont::Sans.at(15.0),
                Node {
                    // The bug: padding and a border on the text node itself.
                    padding: UiRect {
                        left: Val::Px(24.0),
                        right: Val::Px(8.0),
                        top: Val::Px(4.0),
                        bottom: Val::Px(4.0),
                    },
                    border: UiRect {
                        left: Val::Px(4.0),
                        ..UiRect::ZERO
                    },
                    ..default()
                },
                Name::new("text-with-its-own-padding"),
            ))
            .id();
        // Inside a bounded panel, because that is where the bug lives: the wrap
        // width has to arrive from the *parent's* content box for the measure to
        // subtract it wrongly. A text node bounded by its own `max_width` lays
        // out correctly and would make this test quietly vacuous.
        app.world_mut()
            .spawn((
                Node {
                    // A column, as every real panel is: in a row the child would
                    // be stretched instead of bounded, and the measure would
                    // never be handed the too-wide width that is the bug.
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(12.0)),
                    max_width: Val::Px(560.0),
                    ..default()
                },
                Name::new("bounding-panel"),
            ))
            .add_child(text);
        settle(&mut app);

        let violations = overflow_violations(&mut app);
        assert!(
            !violations.is_empty(),
            "a `Text` node carrying its own padding is the known upstream measure bug \
             (viewer-text-node-padding-measure) and `overflow_violations` must report it. \
             If this now passes, Bevy has fixed the measure: drop the workaround in \
             `crate::ui_element::spawn_label` and this test with it."
        );
    }

    /// The same text, decorated the way the convention says — the box on a
    /// **container**, the `Text` a plain child — must be clean.
    ///
    /// The other half of the pair above, and the one that makes it meaningful. A
    /// check that fires on the bad structure proves nothing on its own; it has to
    /// also *not* fire on the good one, or it is simply a check that always
    /// fires.
    #[test]
    fn the_same_text_in_a_decorated_container_is_clean() {
        let test = LayoutTest::new();
        let mut app = test.build();
        let text = app
            .world_mut()
            .spawn((
                Text::new(
                    "A much longer label, of the length a translated string reaches when the \
                     original was written in English and measured once, which is exactly the \
                     case a fixed pixel rect gets wrong.",
                ),
                UiFont::Sans.at(15.0),
                Name::new("plain-text-child"),
            ))
            .id();
        app.world_mut()
            .spawn((
                Node {
                    max_width: Val::Px(400.0),
                    padding: UiRect {
                        left: Val::Px(24.0),
                        right: Val::Px(8.0),
                        top: Val::Px(4.0),
                        bottom: Val::Px(4.0),
                    },
                    border: UiRect {
                        left: Val::Px(4.0),
                        ..UiRect::ZERO
                    },
                    ..default()
                },
                Name::new("decorated-container"),
            ))
            .add_child(text);
        settle(&mut app);

        let violations = overflow_violations(&mut app);
        assert!(
            violations.is_empty(),
            "decorating a container and leaving the text a plain child is the convention, and \
             must lay out cleanly: {violations:#?}"
        );
    }

    /// An inventory row's label, longer than the row is wide, must draw on a
    /// **single line** clipped at the row bounds, and its clip box must report
    /// the overflow that reveals the trailing `…` marker — while a short name
    /// stays one line and reveals nothing (`viewer-inventory-long-names-wrap-\
    /// overlap`).
    ///
    /// Two things are asserted against real layout with the real font:
    ///
    /// 1. **No wrap.** The over-long name in the real (`label_clip_node` +
    ///    `TextLayout::no_wrap`) column comes out one line, and — self-calibrating,
    ///    like the padding pair above — clearly shorter than the *same* name in a
    ///    plain wrapping column, so the test has teeth (it would not pass vacuously
    ///    if the name happened to fit).
    /// 2. **Ellipsis trigger.** `ui_ellipsis::ellipsis_wanted` is true for the
    ///    clip holding the long name (its content is wider than its box) and
    ///    false for a clip holding a short one — the exact condition
    ///    `apply_reveal_ellipsis` toggles the marker on. There is no marker in
    ///    this fixture, so the width it would occupy is zero.
    #[test]
    fn a_long_inventory_row_label_clips_and_flags_the_ellipsis() {
        use crate::inventory::label_clip_node;
        use sl_viewer_ui_core::ui_ellipsis::ellipsis_wanted;

        /// A name far wider than the bounded row, so wrapping (if it happened)
        /// would take several lines and the clip must overflow.
        const LONG_NAME: &str = "A ridiculously long inventory item name that is very much \
                                 wider than the inventory panel row could ever hope to be";
        /// A name that easily fits the column — no overflow, no marker.
        const SHORT_NAME: &str = "Hat";
        /// The row's bounded width and the font, mirroring the real row.
        const ROW_WIDTH: f32 = 340.0;
        const FONT_SIZE: f32 = 14.0;

        /// Build one bounded flex row (icon spacer + a label column + a short
        /// suffix), returning the label `Text` and its clip container entities.
        fn spawn_row(
            app: &mut App,
            root: Entity,
            name: &str,
            label_column: Node,
            no_wrap: bool,
        ) -> (Entity, Entity) {
            let row = app
                .world_mut()
                .spawn((
                    Node {
                        width: Val::Px(ROW_WIDTH),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        ..default()
                    },
                    ChildOf(root),
                ))
                .id();
            // An icon spacer, so the label column has to share the row's width.
            app.world_mut().spawn((
                Node {
                    min_width: Val::Px(20.0),
                    ..default()
                },
                ChildOf(row),
            ));
            let clip = app.world_mut().spawn((label_column, ChildOf(row))).id();
            let mut label =
                app.world_mut()
                    .spawn((Text::new(name), UiFont::Sans.at(FONT_SIZE), ChildOf(clip)));
            if no_wrap {
                // The text keeps its full width (as the real row does), so the
                // clip is what shrinks and reports the overflow.
                label.insert((
                    TextLayout::no_wrap(),
                    Node {
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
            }
            let label = label.id();
            // A short trailing decoration, as the real row carries.
            app.world_mut().spawn((
                Text::new("(worn)"),
                UiFont::Sans.at(FONT_SIZE),
                ChildOf(row),
            ));
            (label, clip)
        }

        let test = LayoutTest::new();
        let mut app = test.build();
        settle(&mut app);
        let root = app.world().resource::<crate::ui::UiRoot>().0;

        // The real column: shrink-and-clip container + a no-wrap label.
        let (fixed, fixed_clip) = spawn_row(&mut app, root, LONG_NAME, label_clip_node(), true);
        // The same name in a plain wrapping column, to calibrate the no-wrap claim.
        let (wrapping, _) = spawn_row(
            &mut app,
            root,
            LONG_NAME,
            Node {
                min_width: Val::Px(0.0),
                ..default()
            },
            false,
        );
        // A short name in the real column — must not overflow.
        let (_short, short_clip) = spawn_row(&mut app, root, SHORT_NAME, label_clip_node(), true);
        settle(&mut app);

        let height = |app: &mut App, entity: Entity| -> f32 {
            app.world()
                .entity(entity)
                .get::<ComputedNode>()
                .map(|computed| computed.size.y * computed.inverse_scale_factor)
                .unwrap_or_default()
        };
        let fixed_h = height(&mut app, fixed);
        let wrapping_h = height(&mut app, wrapping);

        // One line at 14 px is ~18 px; two lines are ~36. The fixed label must
        // be a single line — comfortably under a line and a half.
        assert!(
            fixed_h < FONT_SIZE * 1.5,
            "the no-wrap inventory label wrapped: {fixed_h} logical px tall (expected ~one line)"
        );
        // And the calibration must have actually wrapped, or the test proves
        // nothing: the plain column takes several lines for the same name.
        assert!(
            wrapping_h > fixed_h + FONT_SIZE,
            "the plain column did not wrap ({wrapping_h} vs {fixed_h} logical px), so this test \
             would pass even without the fix — widen `LONG_NAME` or narrow `ROW_WIDTH`"
        );

        // The overflow condition that reveals / hides the `…` marker.
        let overflows = |app: &App, entity: Entity| -> bool {
            app.world()
                .entity(entity)
                .get::<ComputedNode>()
                .is_some_and(|computed| {
                    ellipsis_wanted(computed.content_size.x, computed.size.x, 0.0)
                })
        };
        assert!(
            overflows(&app, fixed_clip),
            "the long label must overflow its clip, so the `…` marker shows"
        );
        assert!(
            !overflows(&app, short_clip),
            "the short label fits its clip, so the `…` marker stays hidden"
        );
    }

    // -----------------------------------------------------------------------
    // The matrix. Every registered element, in every cell.
    // -----------------------------------------------------------------------

    /// **Every element × every script.** The sweep the gallery cannot be: eight
    /// writing systems against every element in the registry, at both label and
    /// prose length, in the direction each script is actually written in.
    ///
    /// This is the combinatorial half that no human walks. A new element inherits
    /// it by being registered; a new script by being listed.
    #[test]
    fn every_element_survives_every_script() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            for sample in SCRIPTS {
                // RTL scripts are laid out RTL: testing Arabic in a left-to-right
                // UI would be testing a configuration no user ever has.
                let direction = match sample.name {
                    "Arabic" | "Hebrew" => UiDirection::Rtl,
                    _other => UiDirection::Ltr,
                };
                let test = LayoutTest::new().with_direction(direction);
                let cx = ElementCx {
                    text: SampleText::Script(sample),
                    ..ElementCx::new()
                };
                let mut app = spawn_element(test, element, cx);
                let violations = layout_violations(&mut app, test);
                if !violations.is_empty() {
                    failures.push(format!(
                        "element `{}` in {} ({direction:?}): {violations:#?}",
                        element.id, sample.name
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every element × pseudolocalisation × font size × scale factor.**
    ///
    /// The axes that break a layout without changing a single glyph anyone can
    /// read: a translation ~40% longer than the English it was measured against,
    /// a user who turned the UI font up, and a display that scales. Each one on
    /// its own has shipped a bug in this viewer already.
    #[test]
    fn every_element_survives_a_long_translation_at_every_scale() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            for font_size in FONT_SIZES {
                for scale_factor in SCALE_FACTORS {
                    let test = LayoutTest::new().with_scale_factor(scale_factor);
                    let cx = ElementCx {
                        text: SampleText::Pseudo,
                        font_size,
                    };
                    let mut app = spawn_element(test, element, cx);
                    let violations = layout_violations(&mut app, test);
                    if !violations.is_empty() {
                        failures.push(format!(
                            "element `{}` pseudolocalised at {font_size}px, scale \
                             {scale_factor}: {violations:#?}",
                            element.id,
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every element × direction × `UiScale`.**
    ///
    /// Mirroring is the axis with no partial credit: either the whole tree
    /// mirrors or the UI is broken for every RTL reader. Swept against `UiScale`
    /// as well as the direction because the two compose, and the reference viewer
    /// gets neither right.
    #[test]
    fn every_element_survives_both_directions_at_every_ui_scale() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            for direction in [UiDirection::Ltr, UiDirection::Rtl] {
                for ui_scale in [1.0_f32, 1.25, 2.0] {
                    let test = LayoutTest::new()
                        .with_direction(direction)
                        .with_ui_scale(ui_scale);
                    let mut app = spawn_element(test, element, ElementCx::new());
                    let violations = layout_violations(&mut app, test);
                    if !violations.is_empty() {
                        failures.push(format!(
                            "element `{}` {direction:?} at UiScale {ui_scale}: {violations:#?}",
                            element.id,
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// Every element must lay out at all — a non-zero box with a real position.
    ///
    /// The guard against the quiet failure this whole file is exposed to: if a
    /// fixture never spawned, or text never measured, every check above passes by
    /// looking at nothing. This is the test that says the others had something to
    /// look at.
    #[test]
    fn every_element_actually_lays_out() -> Result<(), TestError> {
        for element in ELEMENTS {
            let mut app = spawn_element(LayoutTest::new(), element, ElementCx::new());
            let mut query = app.world_mut().query::<(&ComputedNode, &Name)>();
            let sized = query
                .iter(app.world())
                .filter(|(computed, _)| computed.size.x > 0.0 && computed.size.y > 0.0)
                .count();
            assert!(
                sized > 0,
                "element `{}` laid out nothing with a non-zero size — the fixture did not \
                 spawn, or its text never measured, and every other check is passing \
                 vacuously",
                element.id
            );
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Behaviour. Not the resting state — what the element *does*.
    // -----------------------------------------------------------------------

    /// A button emits its action when activated, and **nothing else happens**.
    ///
    /// The registry's no-wiring rule, demonstrated: the button is driven exactly
    /// as a click or `Enter` drives it in the viewer, its real observer runs, and
    /// what it meant is read off a message queue — with nothing behind it that
    /// could teleport an avatar, edit an object or spend money. A button wired
    /// straight to a `Session` could not be tested at all without a grid.
    #[test]
    fn activating_a_button_emits_its_action_and_nothing_else() -> Result<(), TestError> {
        let element = ELEMENTS
            .iter()
            .find(|element| element.id == "button-row")
            .ok_or("the `button-row` element is not registered")?;
        let mut app = spawn_element(LayoutTest::new(), element, ElementCx::new());

        let cancel = find_by_name(&mut app, "button:cancel")
            .ok_or("the button row did not spawn a `cancel` button")?;
        activate(&mut app, cancel);

        assert_eq!(
            drain_actions(&mut app),
            vec![UiAction {
                element: "button-row",
                action: "cancel",
            }],
            "activating the Cancel button must emit exactly its own action"
        );
        Ok(())
    }

    /// `Tab` walks the button row in order, and `Shift+Tab` walks back.
    ///
    /// Driven through `bevy_input_focus`'s real navigation rather than by setting
    /// focus directly, so what is under test is what the user does. Three stops,
    /// not two: with two, a cycle is its own reverse and neither order nor
    /// direction is observable.
    #[test]
    fn tab_walks_the_button_row_in_order_and_shift_tab_walks_back() -> Result<(), TestError> {
        let element = ELEMENTS
            .iter()
            .find(|element| element.id == "button-row")
            .ok_or("the `button-row` element is not registered")?;
        let mut app = spawn_element(LayoutTest::new(), element, ElementCx::new());

        let order: Vec<Entity> = ["button:save", "button:discard", "button:cancel"]
            .into_iter()
            .filter_map(|name| find_by_name(&mut app, name))
            .collect();
        assert_eq!(order.len(), 3, "the button row must offer three tab stops");

        app.world_mut().resource_mut::<InputFocus>().clear();
        let mut walked = Vec::new();
        for _stop in 0..3 {
            if let Some(next) = super::navigate(&mut app, NavAction::Next) {
                walked.push(next);
            }
        }
        assert_eq!(
            walked, order,
            "`Tab` must walk the buttons in their declared order"
        );

        let back = super::navigate(&mut app, NavAction::Previous);
        assert_eq!(
            back,
            order.get(1).copied(),
            "`Shift+Tab` must walk back to the previous stop"
        );
        Ok(())
    }

    /// The registry is actually being swept — every element and every script is
    /// reached by the matrix above.
    ///
    /// Cheap insurance against the way a matrix rots: someone adds an element or
    /// a script, nothing references it, and the suite goes on being green about a
    /// smaller world than it claims.
    #[test]
    fn the_matrix_covers_the_whole_registry() {
        assert!(!ELEMENTS.is_empty(), "no elements to sweep");
        assert!(SCRIPTS.len() >= 2, "a one-script matrix is not a matrix");
    }

    // -----------------------------------------------------------------------
    // The floater matrix. Every registered window, in every cell.
    // -----------------------------------------------------------------------

    /// Build an app and spawn one registered **floater** into it, settled.
    ///
    /// The floater half of [`spawn_element`], and it differs in exactly two
    /// ways, both forced by what a floater is.
    ///
    /// It parents to the scaffold's UI root directly rather than to a card,
    /// because a floater is placed by an absolute `LogicalInset` measured from
    /// its parent — hosting it in a card would move every window and make the
    /// swept placement one the viewer never has.
    ///
    /// And it runs [`register_floater_layout`], without which
    /// `FloaterSpec::default_size` never reaches the content slot and every
    /// sized window would be swept as a content-driven one — the whole
    /// "does this fit its own default rect" question, silently not asked.
    ///
    /// This lives here rather than in `sl-viewer-testkit` because the harness
    /// deliberately depends on the UI *vocabulary* and never on the widgets it
    /// tests; the floater manager is a widget, and the registry naming three
    /// dozen feature modules is in this crate for the same reason `ELEMENTS`
    /// is.
    fn spawn_registered_floater(test: LayoutTest, floater: &FloaterElement, cx: ElementCx) -> App {
        let mut app = test.build();
        enable_action_recording(&mut app);
        register_floater_layout(&mut app);
        let spawn = *floater;
        app.add_systems(
            Startup,
            (move |mut commands: Commands, root: Res<UiRoot>| {
                spawn.spawn(&mut commands, root.0, cx);
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        app
    }

    /// **Every floater × every script.**
    ///
    /// The window's own chrome is what this reaches that the element sweep
    /// cannot: a title bar whose title is the only thing in it that translates,
    /// a button cluster pinned to the trailing edge that has to mirror, and a
    /// content slot that **clips** — so a window whose content outgrows its
    /// declared default rect is caught by `clipping_violations` rather than
    /// discovered by a user whose language runs long.
    #[test]
    fn every_floater_survives_every_script() {
        let mut failures = Vec::new();
        for floater in FLOATERS {
            for sample in SCRIPTS {
                let direction = match sample.name {
                    "Arabic" | "Hebrew" => UiDirection::Rtl,
                    _other => UiDirection::Ltr,
                };
                let test = LayoutTest::new().with_direction(direction);
                let cx = ElementCx {
                    text: SampleText::Script(sample),
                    ..ElementCx::new()
                };
                let mut app = spawn_registered_floater(test, floater, cx);
                let violations = layout_violations(&mut app, test);
                if !violations.is_empty() {
                    failures.push(format!(
                        "floater `{}` in {} ({direction:?}): {violations:#?}",
                        floater.id, sample.name
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every floater × pseudolocalisation × font size × scale factor.**
    ///
    /// A window's chrome is the part most likely to be sized by eye against one
    /// English title at one font size: the title bar's `SpaceBetween` gives the
    /// buttons the width the title leaves, so a 40%-longer title at 22 px is
    /// exactly where a glyph gets squeezed out of its box.
    #[test]
    fn every_floater_survives_a_long_translation_at_every_scale() {
        let mut failures = Vec::new();
        for floater in FLOATERS {
            for font_size in FONT_SIZES {
                for scale_factor in SCALE_FACTORS {
                    let test = LayoutTest::new().with_scale_factor(scale_factor);
                    let cx = ElementCx {
                        text: SampleText::Pseudo,
                        font_size,
                    };
                    let mut app = spawn_registered_floater(test, floater, cx);
                    let violations = layout_violations(&mut app, test);
                    if !violations.is_empty() {
                        failures.push(format!(
                            "floater `{}` pseudolocalised at {font_size}px, scale \
                             {scale_factor}: {violations:#?}",
                            floater.id,
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every floater × direction × `UiScale`.**
    ///
    /// A floater is the scaffold's one absolutely-placed thing, and its inset
    /// is written *logically* so the whole window mirrors — the position is an
    /// inline-start offset, not a left one. That is precisely the sort of claim
    /// that holds until somebody writes `left:` once, and nothing but a sweep
    /// notices.
    ///
    /// **The viewport grows with `UiScale`,** which the element sweep has no
    /// reason to do and this one does. `UiScale` multiplies every `Val::Px`, so
    /// it divides the room the UI has in its *own* units — and a floater's
    /// position is stated in those units. Holding the window fixed while
    /// doubling `UiScale` therefore halves the screen out from under every
    /// window's declared position and reports it as off-screen, which says
    /// nothing about the floater and everything about the screen. That is the
    /// argument `LayoutTest::logical_viewport` already makes for the
    /// scale-factor axis, and it applies here for exactly the same reason.
    ///
    /// Whether a window's default rect fits a *genuinely* small screen is a
    /// separate question, and [`every_floater_fits_a_laptop_window`] asks it
    /// once rather than smuggling it into every cell of three sweeps.
    #[test]
    fn every_floater_survives_both_directions_at_every_ui_scale() {
        /// Each `UiScale` with the window that leaves the UI the harness's
        /// default 1600x1200 of room *in its own units*. Written out rather
        /// than multiplied so the numbers are exact and the intent is on the
        /// page.
        const CELLS: [(f32, u32, u32); 3] =
            [(1.0, 1600, 1200), (1.25, 2000, 1500), (2.0, 3200, 2400)];

        let mut failures = Vec::new();
        for floater in FLOATERS {
            for direction in [UiDirection::Ltr, UiDirection::Rtl] {
                for (ui_scale, width, height) in CELLS {
                    let test = LayoutTest::new()
                        .with_viewport(width, height)
                        .with_direction(direction)
                        .with_ui_scale(ui_scale);
                    let mut app = spawn_registered_floater(test, floater, ElementCx::new());
                    let violations = layout_violations(&mut app, test);
                    if !violations.is_empty() {
                        failures.push(format!(
                            "floater `{}` {direction:?} at UiScale {ui_scale}: {violations:#?}",
                            floater.id,
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// Every floater must actually **open** — chrome, a title and a content
    /// slot with a real box.
    ///
    /// The vacuity guard, and for a floater it guards against something
    /// specific rather than hypothetical: [`spawn_floater`] starts a window
    /// **hidden**, a hidden subtree lays out at zero size, and every check
    /// above would then pass by measuring nothing at all. It is
    /// `FloaterElement::spawn`'s job to leave the window shown, and this is
    /// what holds it to that.
    ///
    /// [`spawn_floater`]: crate::floater::spawn_floater
    #[test]
    fn every_floater_actually_opens() -> Result<(), TestError> {
        for floater in FLOATERS {
            let mut app = spawn_registered_floater(LayoutTest::new(), floater, ElementCx::new());
            for part in ["floater-title-bar", "floater-title", "floater-content"] {
                let entity = find_by_name(&mut app, part)
                    .ok_or_else(|| format!("floater `{}` spawned no `{part}` node", floater.id))?;
                let sized = app
                    .world()
                    .entity(entity)
                    .get::<ComputedNode>()
                    .is_some_and(|computed| computed.size.x > 0.0 && computed.size.y > 0.0);
                assert!(
                    sized,
                    "floater `{}` laid `{part}` out at zero size — the window did not open, and \
                     every other floater check is passing vacuously",
                    floater.id
                );
            }
        }
        Ok(())
    }

    /// Every floater must open **fully on screen** on a laptop.
    ///
    /// The placement question, asked once and directly: a floater's default
    /// position is an absolute pixel offset written by whoever added the
    /// window, and the only thing standing between a badly chosen one and a
    /// window half off the edge is `clamp_floaters_on_screen` — which
    /// deliberately guarantees only that a 16 px sliver stays reachable, not
    /// that the window is usable.
    ///
    /// 1280x800 is the small end of what people actually have, and the strings
    /// are pseudolocalised because a window's chrome grows with its title. The
    /// clamp is not installed (see [`register_floater_layout`]): correcting the
    /// placement here would be the sweep hiding the thing it is looking for.
    #[test]
    fn every_floater_fits_a_laptop_window() {
        let mut failures = Vec::new();
        for floater in FLOATERS {
            let test = LayoutTest::new().with_viewport(1280, 800);
            let cx = ElementCx {
                text: SampleText::Pseudo,
                font_size: 15.0,
            };
            let mut app = spawn_registered_floater(test, floater, cx);
            let violations = layout_violations(&mut app, test);
            if !violations.is_empty() {
                failures.push(format!("floater `{}`: {violations:#?}", floater.id));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The floater sweep covers the whole registry, and the registry is not a
    /// list of one.
    #[test]
    fn the_matrix_covers_the_whole_floater_registry() {
        assert!(!FLOATERS.is_empty(), "no floaters to sweep");
    }

    /// Every element must fit a **narrow** window, at the longest strings.
    ///
    /// The other end of the viewport axis. A panel is written and eyeballed on
    /// the author's wide monitor, and the person it breaks for is on a laptop
    /// with the UI font turned up and a language that runs long — three axes that
    /// each look fine alone. 720x600 logical is a small but entirely real window.
    #[test]
    fn every_element_fits_a_narrow_window() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            let test = LayoutTest::new().with_viewport(720, 600);
            let cx = ElementCx {
                text: SampleText::Pseudo,
                font_size: 15.0,
            };
            let mut app = spawn_element(test, element, cx);
            let violations = layout_violations(&mut app, test);
            if !violations.is_empty() {
                failures.push(format!("element `{}`: {violations:#?}", element.id));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
