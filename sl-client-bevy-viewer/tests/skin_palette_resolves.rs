//! The chrome role palette survives the whole round trip: a shipped skin's
//! `--<role>` tokens, through `common.css`'s `:root` rule, into the
//! [`SkinPalette`] component the Rust-painted widget states read.
//!
//! Every other check of the palette is static — the property table covers each
//! field, the shipped skins define each token. Those cannot see the one link
//! that actually carries the colours: whether `bevy_flair` resolves a
//! `-sk-color-*` declaration on `:root` onto the styled entity at all. That
//! link fails **silently** (a rule that parses to nothing leaves the field at
//! its fallback, and nothing is logged), and it is the link a `bevy_flair`
//! upgrade is most likely to move, so it is worth driving the real stylesheets
//! through the real engine once.
//!
//! Lives here rather than in `sl-viewer-ui-core` for the same reason the other
//! skin tests do: it reads `assets/skins/`, which ships with this binary.

#[cfg(test)]
mod test {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use bevy::asset::LoadState;
    use bevy::prelude::*;
    use bevy_flair::prelude::*;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_viewer_ui_core::skin::SkinTextCaret;
    use sl_viewer_ui_core::skin_palette::{SkinPalette, register_palette_properties};

    /// A boxed error so tests can use `?` instead of `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// How long to pump frames before giving up on the stylesheet load.
    ///
    /// A **wall-clock** bound, not a frame count: the load is asynchronous and
    /// this app has nothing in it, so its frames cost almost nothing — 600 of
    /// them went by in 0.46 s under a loaded machine and the sheet had not
    /// arrived, which is a test that measures the host's spare CPU rather than
    /// the skin. Generous, because it is only ever reached on a real failure.
    const LOAD_TIMEOUT: Duration = Duration::from_secs(60);

    /// How long to wait between frames while the load is in flight, so the
    /// pump does not spin a core next to the rest of the suite.
    const POLL_INTERVAL: Duration = Duration::from_millis(5);

    /// An app with just the CSS engine, reading the shipped `assets/`.
    fn app() -> App {
        app_with_assets(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"))
    }

    /// The directory holding the **test-only** stylesheets — a skin that exists
    /// to be asserted against and must not ship, so it lives beside this file
    /// rather than under `assets/skins/` (where `shipped_skins.rs` would walk
    /// it and the `--skin` switcher would have to know about it).
    fn test_assets_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("assets")
    }

    /// [`app`], with the asset root given — so a test can load a stylesheet
    /// that is not one of the shipped skins. The embedded `common.css` /
    /// `fallback.css` are reached through `embedded://`, which is the same
    /// wherever the file root points.
    fn app_with_assets(assets: &std::path::Path) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: assets.to_string_lossy().into_owned(),
                ..AssetPlugin::default()
            },
            // Headless, but present: `bevy_flair`'s style pass reads
            // `WindowEvent` and writes `RequestRedraw`, both of which this
            // plugin registers. Without it the first frame fails parameter
            // validation rather than styling anything.
            WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..WindowPlugin::default()
            },
            FlairPlugin,
        ));
        // The structural rules and the fallback token values are embedded in
        // the binary, and a shipped skin reaches them by `@import`ing the
        // embedded path — so without this registration the import resolves to
        // nothing and every assertion below reads the built-in fallback.
        sl_viewer_ui_core::skin::embed_fallback_stylesheet(&mut app);
        register_palette_properties(&mut app);
        // The caret / selection shim, for the same reason: `caret-color` is not
        // a property `bevy_flair` knows on its own, so without this the
        // `.sk-text-field` rule's caret silently parses to nothing.
        sl_viewer_ui_core::skin::register_caret_properties(&mut app);
        // `bevy_flair` snapshots the property registries into its CSS asset
        // loader in `Plugin::finish`, and registers the loader there — which
        // `App::update` never runs, only `App::run`. Without this pair the
        // stylesheet sits in `Loading` for ever, which is exactly what it did.
        app.finish();
        app.cleanup();
        app
    }

    /// Pump frames until `handle`'s stylesheet has loaded (or failed), so the
    /// style pass has something to resolve.
    fn load(app: &mut App, handle: &Handle<StyleSheet>) -> Result<(), TestError> {
        let started = Instant::now();
        loop {
            app.update();
            let state = app.world().resource::<AssetServer>().load_state(handle);
            match state {
                LoadState::Loaded => return Ok(()),
                LoadState::Failed(error) => {
                    return Err(format!("the skin stylesheet failed to load: {error}").into());
                }
                LoadState::NotLoaded | LoadState::Loading => {}
            }
            if started.elapsed() >= LOAD_TIMEOUT {
                return Err(format!(
                    "the skin stylesheet was still {state:?} after {LOAD_TIMEOUT:?}"
                )
                .into());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Graphite's `--surface-bg`, `--text-muted` and `--pie-selected` reach the
    /// styled root as [`SkinPalette`] fields.
    ///
    /// Three roles rather than one: a surface (the role a class would also
    /// reach), a text role (the one 20 copied constants collapsed onto), and a
    /// pie-menu role (defined by this task, so a skin that predates it would
    /// leave the fallback standing and the assertion would catch that).
    ///
    /// **This is also the guard on import order**, which is not what CSS
    /// authors expect: `bevy_flair` places a plainly-`@import`ed sheet's rules
    /// so they *beat* the importing file's own, so a skin that imports the
    /// embedded fallback without `layer(fallback)` silently renders in the
    /// fallback's colours instead of its own. That is precisely what this test
    /// caught when the fallback sheet landed — `--text-muted` came back
    /// `#9ea8bd` rather than Graphite's `#cbd5e2` — and the same trap
    /// `themes/dark.css` documents for the overlay direction. Keep the
    /// assertions on roles the fallback also defines *differently*, or this
    /// stops guarding anything.
    #[test]
    fn a_shipped_skin_reaches_the_palette() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        load(&mut app, &handle)?;
        // One more frame after the load, for the style pass that dresses the
        // entity the sheet just arrived for.
        app.update();

        let palette = app.world().get::<SkinPalette>(root).copied().ok_or(
            "the styled root never received a SkinPalette — the `:root` rule in \
                    common.css did not resolve, so every Rust-painted widget is stuck on \
                    its built-in fallback",
        )?;
        let fallback = SkinPalette::default();

        // Graphite's own values, as `graphite/skin.css` declares them.
        assert_eq!(palette.surface_bg, Color::srgba_u8(0x1c, 0x1f, 0x26, 0xf2));
        assert_eq!(palette.text_muted, Color::srgb_u8(0xcb, 0xd5, 0xe2));
        assert_eq!(
            palette.pie_selected,
            Color::srgba_u8(0xf2, 0x69, 0x2c, 0x59)
        );
        assert_ne!(
            palette.surface_bg, fallback.surface_bg,
            "the skin must actually override the fallback, or this test proves nothing"
        );
        Ok(())
    }

    /// The embedded fallback sheet stands on its own: loaded with no skin in
    /// sight, it still resolves the whole role palette.
    ///
    /// This is the sheet that makes "there is always a stylesheet" true, which
    /// is what lets widget state live in the cascade
    /// (`viewer-skin-widget-state-classes`) instead of in a per-frame Rust
    /// paint. If it ever stopped resolving, every state would quietly flatten
    /// to one look rather than failing loudly.
    ///
    /// It also proves the link the shipped-skin test above only uses in
    /// passing: a `@import` naming the `embedded://` source resolves at all.
    #[test]
    fn the_embedded_fallback_resolves_on_its_own() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load(sl_viewer_ui_core::skin::FALLBACK_STYLESHEET);
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        load(&mut app, &handle)?;
        app.update();

        let palette = app.world().get::<SkinPalette>(root).copied().ok_or(
            "the embedded fallback never produced a SkinPalette — either the \
             sheet did not load or its `@import` of the embedded common.css \
             resolved to nothing",
        )?;
        // The sheet's own hex, as `fallback.css` declares it — not
        // `SkinPalette::FALLBACK` directly: the constants are `f32` literals
        // (0.65) and the CSS round-trips through 8 bits (0xa6 → 0.6509804), so
        // the two agree to within a step and never exactly.
        // `fallback_tokens_match_rust` is what holds them together; this
        // asserts the parse and cascade delivered those same bytes.
        assert_eq!(palette.text_muted, Color::srgb_u8(0x9e, 0xa8, 0xbd));
        assert_eq!(palette.surface_bg, Color::srgba_u8(0x1c, 0x1f, 0x26, 0xf2));
        assert_eq!(palette.pie_label_sub_pie, Color::srgb_u8(0xa6, 0xdb, 0xff));
        Ok(())
    }

    /// The three-deep cascade resolves in the right order: a theme overlay
    /// beats the skin it sits on, which beats the embedded fallback.
    ///
    /// Each link is a separate `@import` with its own layering, and they were
    /// written years apart — `themes/dark.css` layers the skin beneath itself,
    /// and the skin now layers the fallback beneath *that*, so a token can be
    /// claimed at any of three depths. Nothing exercised the stack as a whole
    /// until the fallback made it three deep, and every way it can go wrong
    /// looks the same from outside: the wrong colour, quietly.
    #[test]
    fn a_theme_overlay_beats_its_skin_which_beats_the_fallback() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/themes/dark.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        load(&mut app, &handle)?;
        app.update();

        let palette = app
            .world()
            .get::<SkinPalette>(root)
            .copied()
            .ok_or("the theme overlay never produced a SkinPalette")?;

        // Claimed by the overlay: the deepest writer wins.
        assert_eq!(palette.surface_bg, Color::srgba_u8(0x0a, 0x0c, 0x10, 0xfa));
        assert_eq!(palette.text_primary, Color::srgb_u8(0xf4, 0xf7, 0xfb));
        // Claimed by Graphite but not by the overlay: falls through one level,
        // and lands on the skin's value rather than the fallback's `#9ea8bd`.
        assert_eq!(palette.text_muted, Color::srgb_u8(0xcb, 0xd5, 0xe2));
        // There is deliberately no "only the fallback defines it" assertion:
        // both shipped skins define all 41 tokens, so no such role exists
        // today. What proves the fallback was reached through *both* imports is
        // that a `SkinPalette` arrived at all — the `:root` rule that writes it
        // lives in `common.css`, which only the fallback pulls in.
        Ok(())
    }

    /// The three selector mechanisms widget state now rests on really resolve:
    /// `:checked`, `:disabled`, and a descendant combinator reaching a child
    /// node from its ancestor's state.
    ///
    /// `viewer-skin-widget-state-classes` moves state out of per-frame Rust
    /// paint, and prefers a pseudo-class wherever the state *is* one the engine
    /// already knows — `bevy_flair` syncs `bevy_ui::Checked` to `:checked` and
    /// `InteractionDisabled` to `:disabled`, so a tab's selection needs no
    /// marker class and nothing to keep in step. The descendant combinator is
    /// what lets a caption grey from its button's or its strip's state, since
    /// `bevy_ui` has no style inheritance.
    ///
    /// All three fail the same silent way if unsupported: the rule simply does
    /// not match, the node keeps whatever it was spawned with, and nothing is
    /// logged. Asserting them here is what makes it safe for the widgets to
    /// stop painting.
    #[test]
    fn checked_disabled_and_descendant_selectors_resolve() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();

        // A selected tab, a resting one, and a disabled one carrying a caption.
        let selected = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-tab"),
                bevy::ui::Checked,
                ChildOf(root),
            ))
            .id();
        let resting = app
            .world_mut()
            .spawn((Node::default(), ClassList::new("sk-tab"), ChildOf(root)))
            .id();
        let refused = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-tab"),
                bevy::ui::InteractionDisabled,
                ChildOf(root),
            ))
            .id();
        let refused_caption = app
            .world_mut()
            .spawn((
                Text::new("caption"),
                ClassList::new("sk-tab-label"),
                ChildOf(refused),
            ))
            .id();
        let live_caption = app
            .world_mut()
            .spawn((
                Text::new("caption"),
                ClassList::new("sk-tab-label"),
                ChildOf(resting),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let background = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0)
        };
        // `:checked` — Graphite's --card-bg, not the resting --surface-bg.
        assert_eq!(
            background(selected),
            Some(Color::srgb_u8(0x26, 0x2b, 0x34)),
            ":checked did not match, so a selected tab is indistinguishable"
        );
        assert_eq!(
            background(resting),
            Some(Color::srgba_u8(0x1c, 0x1f, 0x26, 0xf2))
        );

        let text = |entity| app.world().get::<TextColor>(entity).map(|color| color.0);
        // The descendant combinator, driven by the ancestor's `:disabled`.
        assert_eq!(
            text(refused_caption),
            Some(Color::srgb_u8(0x73, 0x7d, 0x8f)),
            "`.sk-tab:disabled .sk-tab-label` did not match, so a refused tab's \
             caption reads as one that would answer a click"
        );
        assert_eq!(text(live_caption), Some(Color::srgb_u8(0xe6, 0xeb, 0xf2)));
        Ok(())
    }

    /// A selected row really is painted, and a resting one really is not.
    ///
    /// `.sk-table-row` / `.sk-list-row` declare the resting `transparent`, and
    /// `.sk-active` the selection — two rules of equal specificity, so which
    /// one wins is decided by the order they appear in `common.css` and by
    /// nothing else. That is a property of the file, not of the code that adds
    /// the class, and no Rust test can see it.
    #[test]
    fn a_selected_row_beats_its_resting_rule() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let selected = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-table-row sk-active"),
                ChildOf(root),
            ))
            .id();
        let resting = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-table-row"),
                ChildOf(root),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let background = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0)
        };
        assert_eq!(
            background(selected),
            Some(Color::srgba_u8(0x3d, 0x57, 0x85, 0x8c)),
            "a selected row resolved to its resting rule, so selection is \
             invisible in every list the cascade paints"
        );
        // Through `to_srgba`, because a resolved colour arrives as `Srgba` and
        // `Color::NONE` is a `LinearRgba` — the same colour, a different
        // variant, and `assert_eq` on `Color` would compare the variants.
        assert_eq!(
            background(resting).map(|color| color.to_srgba()),
            Some(Srgba::NONE),
            "a resting row must be transparent"
        );
        Ok(())
    }

    /// **A row's four states resolve in the order the file puts them in.**
    ///
    /// Striped, hovered and selected are one class or pseudo-class each,
    /// compounded with the row's own — (0,2,0) every time — so specificity
    /// separates none of them and a row that is all three is painted by
    /// whichever rule `common.css` states last. That is the whole mechanism
    /// (`viewer-skin-list-row-striping`), and it is invisible to Rust: the
    /// classes go on, and what comes out the other end is a colour.
    ///
    /// Graphite bands nothing, so the stripe here resolves to the transparent
    /// its token holds — `a_light_list_bands_hovers_and_selects` is where the
    /// stripe has a colour to show. What this pins is the precedence, and the
    /// selected row's text, which Graphite *does* move off the field family.
    #[test]
    fn a_rows_states_resolve_stripe_then_hover_then_selection() -> Result<(), TestError> {
        use bevy::picking::hover::Hovered;

        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let mut row = |classes: &str, hovered: bool| {
            let entity = app
                .world_mut()
                .spawn((Node::default(), ClassList::new(classes), ChildOf(root)))
                .id();
            if hovered {
                app.world_mut().entity_mut(entity).insert(Hovered(true));
            }
            entity
        };
        let striped = row("sk-table-row sk-stripe", false);
        let hovered = row("sk-table-row", true);
        let striped_hovered = row("sk-table-row sk-stripe", true);
        let selected_hovered = row("sk-table-row sk-active", true);
        let hovered_hand_rolled = row("sk-list-row", true);

        // The text half: a cell in a selected row against one in an ordinary
        // row, both inside a list's face, so the re-rooting applies to both and
        // only the selection separates them.
        let list = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-list-surface"),
                ChildOf(root),
            ))
            .id();
        let selected_row = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-table-row sk-active"),
                ChildOf(list),
            ))
            .id();
        let selected_cell = app
            .world_mut()
            .spawn((
                Text::new("Kelly"),
                ClassList::new("sk-text"),
                ChildOf(selected_row),
            ))
            .id();
        let resting_row = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-table-row"),
                ChildOf(list),
            ))
            .id();
        let resting_cell = app
            .world_mut()
            .spawn((
                Text::new("Robin"),
                ClassList::new("sk-text"),
                ChildOf(resting_row),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let fill = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0)
        };
        let text = |entity| app.world().get::<TextColor>(entity).map(|color| color.0);
        let hover = Some(Color::srgb_u8(0x35, 0x3c, 0x4a));

        assert_eq!(
            fill(striped).map(|color| color.to_srgba()),
            Some(Srgba::NONE),
            "Graphite bands nothing, so a striped row is its list's face"
        );
        assert_eq!(
            fill(hovered),
            hover,
            "a row under the pointer did not take `--list-row-hover`, so no \
             list in the viewer answers the pointer"
        );
        assert_eq!(
            fill(hovered_hand_rolled),
            hover,
            "a hand-rolled list's row must hover like a table's — that shared \
             rule is what retired the texture picker's `.sk-picker-row:hover`"
        );
        assert_eq!(
            fill(striped_hovered),
            hover,
            "the hover must come after the stripe, or the pointer does \
             nothing on every other row"
        );
        assert_eq!(
            fill(selected_hovered),
            Some(Color::srgba_u8(0x3d, 0x57, 0x85, 0x8c)),
            "the selection must come after the hover, or moving the pointer \
             over the selected row un-selects it to the eye"
        );
        assert_eq!(
            text(selected_cell),
            Some(Color::srgb_u8(0xe6, 0xeb, 0xf2)),
            "a selected row's text did not take `--list-row-selected-text`"
        );
        assert_eq!(
            text(resting_cell),
            Some(Color::srgb_u8(0xff, 0xff, 0xff)),
            "an unselected cell must stay on the field family, or the \
             selected-row rule is reaching the whole list"
        );
        Ok(())
    }

    /// **Toggling `Checked` at runtime moves the mark, both ways.**
    ///
    /// Every other checkbox test here spawns one entity per state, so all of
    /// them pass on an engine that computes a style once and never revisits
    /// it. This one clicks: the mark has to appear when `Checked` arrives and
    /// **go away again** when it leaves, on the same entity, which is the only
    /// thing a user ever actually does to a checkbox.
    #[test]
    fn toggling_checked_moves_the_mark_both_ways() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let row = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-checkbox"),
                ChildOf(root),
            ))
            .id();
        let box_node = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-checkbox-box"),
                ChildOf(row),
            ))
            .id();
        let tick = app
            .world_mut()
            .spawn((
                Text::default(),
                PseudoElementsSupport,
                ClassList::new("sk-checkbox-tick"),
                ChildOf(box_node),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let mark = |app: &App, entity: Entity| {
            app.world()
                .get::<Children>(entity)
                .and_then(|kids| kids.iter().next())
                .and_then(|before| app.world().get::<TextSpan>(before))
                .map(|span| span.0.clone())
        };

        assert_eq!(
            mark(&app, tick).as_deref(),
            Some(""),
            "a freshly spawned unchecked box already carries a mark"
        );

        app.world_mut().entity_mut(row).insert(bevy::ui::Checked);
        app.update();
        assert_eq!(
            mark(&app, tick),
            Some("\u{2713}".to_owned()),
            "checking the box did not bring the mark"
        );

        app.world_mut()
            .entity_mut(row)
            .remove::<bevy::ui::Checked>();
        app.update();
        assert_eq!(
            mark(&app, tick).as_deref(),
            Some(""),
            "UNCHECKING the box left the mark behind — `content` is applied \
             when the rule starts matching but never reverted when it stops"
        );
        Ok(())
    }

    /// **Moving a radio's selection takes the pip off the old option.**
    ///
    /// The checkbox's failure is worse on a radio, where it is not one stale
    /// mark but a group that shows every option you have ever chosen at once.
    #[test]
    fn moving_a_radio_selection_takes_the_old_pip_away() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((
                Node::default(),
                Styled::new(handle.clone()),
                ClassList::new("sk-radio-group"),
            ))
            .id();
        let spawn_option = |app: &mut App, checked: bool| {
            let mut option =
                app.world_mut()
                    .spawn((Node::default(), ClassList::new("sk-radio"), ChildOf(root)));
            if checked {
                option.insert(bevy::ui::Checked);
            }
            let option = option.id();
            let indicator = app
                .world_mut()
                .spawn((
                    Node::default(),
                    ClassList::new("sk-radio-indicator"),
                    ChildOf(option),
                ))
                .id();
            let pip = app
                .world_mut()
                .spawn((
                    Text::default(),
                    PseudoElementsSupport,
                    ClassList::new("sk-radio-pip"),
                    ChildOf(indicator),
                ))
                .id();
            (option, pip)
        };
        let (first, first_pip) = spawn_option(&mut app, true);
        let (second, second_pip) = spawn_option(&mut app, false);

        load(&mut app, &handle)?;
        app.update();

        let mark = |app: &App, entity: Entity| {
            app.world()
                .get::<Children>(entity)
                .and_then(|kids| kids.iter().next())
                .and_then(|before| app.world().get::<TextSpan>(before))
                .map(|span| span.0.clone())
        };
        assert_eq!(
            mark(&app, first_pip),
            Some("\u{25cf}".to_owned()),
            "the lit option has no pip"
        );

        // Move the selection, exactly as the group's own observer does.
        app.world_mut()
            .entity_mut(first)
            .remove::<bevy::ui::Checked>();
        app.world_mut().entity_mut(second).insert(bevy::ui::Checked);
        app.update();

        assert_eq!(
            mark(&app, second_pip),
            Some("\u{25cf}".to_owned()),
            "the newly lit option did not get the pip"
        );
        assert_eq!(
            mark(&app, first_pip).as_deref(),
            Some(""),
            "the OLD option kept its pip, so the group shows two selected \
             options at once"
        );
        Ok(())
    }

    /// **All four checkbox looks come out of the stylesheet.**
    ///
    /// The widget paints nothing at all: `:checked` and `:disabled` reach the
    /// box and the tick from the row, and unchecked is `transparent` rather
    /// than a rewritten glyph. So *every* state of it is this file's, and a
    /// Rust test can only see that the classes are present — which
    /// `ui_checkbox`'s own tests do. This is the half that says they resolve.
    ///
    /// The checked-and-disabled case is the one worth pinning: the tick greys
    /// only where it is shown, so that rule has to be compound. A lone
    /// `:disabled` on the tick would paint a grey tick on an *unchecked* box.
    #[test]
    fn a_checkbox_takes_all_four_of_its_looks_from_the_skin() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();

        // (checked, disabled) -> its box and tick.
        let mut spawn_box = |checked: bool, disabled: bool| {
            let mut row = app.world_mut().spawn((
                Node::default(),
                ClassList::new("sk-checkbox"),
                ChildOf(root),
            ));
            if checked {
                row.insert(bevy::ui::Checked);
            }
            if disabled {
                row.insert(bevy::ui::InteractionDisabled);
            }
            let row = row.id();
            let box_node = app
                .world_mut()
                .spawn((
                    Node::default(),
                    ClassList::new("sk-checkbox-box"),
                    ChildOf(row),
                ))
                .id();
            let tick = app
                .world_mut()
                .spawn((
                    // Empty, exactly as the widget spawns it: the glyph is the
                    // skin's `content`, reached through `::before`.
                    Text::default(),
                    PseudoElementsSupport,
                    ClassList::new("sk-checkbox-tick"),
                    ChildOf(box_node),
                ))
                .id();
            let caption = app
                .world_mut()
                .spawn((Text::default(), ClassList::new("sk-text"), ChildOf(row)))
                .id();
            (box_node, tick, caption)
        };
        let (off_box, off_tick, off_caption) = spawn_box(false, false);
        let (on_box, on_tick, _on_caption) = spawn_box(true, false);
        let (refused_box, refused_tick, refused_caption) = spawn_box(false, true);
        let (refused_on_box, refused_on_tick, _refused_on_caption) = spawn_box(true, true);

        load(&mut app, &handle)?;
        app.update();

        let fill = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0.to_srgba())
        };
        // The `::before` pseudo-element bevy_flair spawned under the tick —
        // where `content` and its colour actually land. Found by position
        // because `PseudoElement` is private to bevy_flair: its
        // `PseudoElementsSupport` hook spawns `::before` and then `::after`,
        // so the first child is the one. A future version spawning them the
        // other way round fails this loudly rather than silently, which is
        // what to want from a positional assumption.
        let before = |entity| {
            app.world()
                .get::<Children>(entity)
                .and_then(|kids| kids.iter().next())
        };
        let mark = |entity| {
            before(entity)
                .and_then(|e| app.world().get::<TextSpan>(e))
                .map(|span| span.0.clone())
        };
        let tick = |entity| {
            before(entity)
                .and_then(|e| app.world().get::<TextColor>(e))
                .map(|color| color.0.to_srgba())
        };
        assert_eq!(
            fill(off_box),
            Some(Srgba::hex("1a1f29").map_err(|error| error.to_string())?),
            "an unchecked box takes `--check-bg`"
        );
        assert_eq!(
            fill(on_box),
            Some(Srgba::hex("3d5785").map_err(|error| error.to_string())?),
            "`:checked` did not reach the box"
        );
        assert_eq!(
            fill(refused_box),
            Some(Srgba::hex("232730").map_err(|error| error.to_string())?),
            "`:disabled` did not reach the box"
        );
        // The glyph is the skin's, which is the point: a skin that wants ✔, ✗
        // or × writes it in `content` and the widget never learns.
        assert_eq!(
            mark(on_tick),
            Some("\u{2713}".to_owned()),
            "`content` did not reach the tick's `::before`, so no skin can \
             choose the mark"
        );
        assert_eq!(
            mark(off_tick).as_deref(),
            Some(""),
            "an unchecked box must carry no mark at all"
        );
        assert_eq!(
            tick(on_tick),
            Some(Srgba::hex("e6ebf2").map_err(|error| error.to_string())?),
            "a checked tick takes `--check-tick`"
        );
        assert_eq!(
            mark(refused_tick).as_deref(),
            Some(""),
            "a refused *unchecked* box must still show no mark"
        );
        assert_eq!(
            tick(refused_on_tick),
            Some(Srgba::hex("737d8f").map_err(|error| error.to_string())?),
            "a refused checked tick greys"
        );
        assert_eq!(
            fill(refused_on_box),
            Some(Srgba::hex("232730").map_err(|error| error.to_string())?),
            "disabled must beat checked on the box"
        );
        // And the caption greys with the box. A greyed box beside a caption in
        // full contrast reads as an empty box rather than as a setting this
        // window will not let you change — the same pairing `.sk-button` makes.
        let caption_color = |entity| {
            app.world()
                .get::<TextColor>(entity)
                .map(|color| color.0.to_srgba())
        };
        assert_eq!(
            caption_color(off_caption),
            Some(Srgba::hex("e6ebf2").map_err(|error| error.to_string())?),
            "an enabled caption is ordinary body text"
        );
        assert_eq!(
            caption_color(refused_caption),
            Some(Srgba::hex("737d8f").map_err(|error| error.to_string())?),
            "a refused checkbox must grey its caption, not only its box"
        );
        Ok(())
    }

    /// **A radio's disc, ring and pip are all the skin's.**
    ///
    /// The same move as the checkbox, and finished the same way. First
    /// `apply_radio_selection`'s glyph-swapping loop went, leaving the mark to
    /// `content`; then the indicator stopped being a character at all. A glyph
    /// carries exactly one colour, so a recolour could never be the reference's
    /// **pale disc inside a dark ring** — that needs a filled, outlined box, and
    /// this is what says the box is reachable: two fills, a greyed one, and the
    /// pip that shows against them.
    #[test]
    fn a_radio_takes_its_disc_ring_and_pip_from_the_skin() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        // A group around the options, because the refused look is selected from
        // it — a whole group is what a consumer refuses, not one option.
        let mut spawn_group = |disabled: bool| {
            let mut group = app.world_mut().spawn((
                Node::default(),
                ClassList::new("sk-radio-group"),
                ChildOf(root),
            ));
            if disabled {
                group.insert(bevy::ui::InteractionDisabled);
            }
            group.id()
        };
        let live = spawn_group(false);
        let refused = spawn_group(true);
        let mut spawn_option = |group: Entity, checked: bool| {
            let mut item = app.world_mut().spawn((
                Node::default(),
                ClassList::new("sk-radio"),
                ChildOf(group),
            ));
            if checked {
                item.insert(bevy::ui::Checked);
            }
            let item = item.id();
            let disc = app
                .world_mut()
                .spawn((
                    Node::default(),
                    ClassList::new("sk-radio-indicator"),
                    ChildOf(item),
                ))
                .id();
            let pip = app
                .world_mut()
                .spawn((
                    // Empty, exactly as the widget spawns it: the mark is the
                    // skin's `content`, reached through `::before`.
                    Text::default(),
                    PseudoElementsSupport,
                    ClassList::new("sk-radio-pip"),
                    ChildOf(disc),
                ))
                .id();
            (disc, pip)
        };
        let (resting_disc, resting_pip) = spawn_option(live, false);
        let (lit_disc, lit_pip) = spawn_option(live, true);
        let (refused_disc, _refused_pip) = spawn_option(refused, false);
        let (_refused_lit_disc, refused_lit_pip) = spawn_option(refused, true);

        load(&mut app, &handle)?;
        app.update();

        let fill = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0.to_srgba())
        };
        let before = |entity| {
            app.world()
                .get::<Children>(entity)
                .and_then(|kids| kids.iter().next())
        };
        let mark = |entity| {
            before(entity)
                .and_then(|e| app.world().get::<TextSpan>(e))
                .map(|span| span.0.clone())
        };
        let pip_color = |entity| {
            before(entity)
                .and_then(|e| app.world().get::<TextColor>(e))
                .map(|color| color.0.to_srgba())
        };

        // The disc is a *box*, which is the whole point: a glyph carries one
        // colour, so only a filled, outlined node can be the reference's pale
        // disc inside a dark ring.
        assert_eq!(
            fill(resting_disc),
            Some(Srgba::hex("1a1f29").map_err(|error| error.to_string())?),
            "an unselected disc takes `--radio-bg`"
        );
        assert_eq!(
            fill(lit_disc),
            Some(Srgba::hex("3d5785").map_err(|error| error.to_string())?),
            "`:checked` did not reach the disc"
        );
        assert_eq!(
            fill(refused_disc),
            Some(Srgba::hex("232730").map_err(|error| error.to_string())?),
            "a refused group did not grey its options' discs"
        );
        // Round, and from the stylesheet: the widget spawns a plain square
        // node, so a skin that wanted a square indicator would only have to
        // drop this one declaration. Asserted because a `50%` bevy_flair failed
        // to parse would leave the disc square with nothing else to show for
        // it.
        assert_eq!(
            app.world()
                .get::<Node>(resting_disc)
                .map(|node| node.border_radius.top_left),
            Some(Val::Percent(50.0)),
            "the disc did not take its `border-radius` from the skin"
        );
        assert_eq!(
            mark(lit_pip),
            Some("\u{25cf}".to_owned()),
            "`content` did not reach the pip's `::before`, so selection is \
             colour-only and a recolour-free skin cannot show it"
        );
        assert_eq!(
            mark(resting_pip).as_deref(),
            Some(""),
            "an unselected option must carry no mark at all"
        );
        assert_eq!(
            pip_color(lit_pip),
            Some(Srgba::hex("e6ebf2").map_err(|error| error.to_string())?),
            "a lit pip takes `--radio-pip`"
        );
        assert_eq!(
            pip_color(refused_lit_pip),
            Some(Srgba::hex("737d8f").map_err(|error| error.to_string())?),
            "a refused group's lit pip greys"
        );
        Ok(())
    }

    /// A field that cannot be edited greys, and an ordinary one does not — for
    /// **both** stances, which are no longer one look.
    ///
    /// `.sk-text-field` is stamped on every editor by the scaffold and carries
    /// the typed text's colour. A *disabled* field is reached by `:disabled`
    /// over the `InteractionDisabled` the consumer already sets, with no code
    /// at all; a *read-only* one needs `reflect_read_only_field` to put
    /// `.sk-read-only` on, because no pseudo-class describes a field that takes
    /// focus and still refuses an edit. Both are (0,2,0), so which wins where
    /// they overlap is the file's order — the same thing
    /// `a_selected_row_beats_its_resting_rule` pins for rows.
    #[test]
    fn a_field_that_refuses_edits_greys_its_text() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let editable = app
            .world_mut()
            .spawn((
                Text::new("typed"),
                ClassList::new("sk-text-field"),
                ChildOf(root),
            ))
            .id();
        let read_only = app
            .world_mut()
            .spawn((
                Text::new("typed"),
                ClassList::new("sk-text-field sk-read-only"),
                ChildOf(root),
            ))
            .id();
        let refused = app
            .world_mut()
            .spawn((
                Text::new("typed"),
                ClassList::new("sk-text-field"),
                bevy::ui::InteractionDisabled,
                ChildOf(root),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let text = |entity| app.world().get::<TextColor>(entity).map(|color| color.0);
        assert_eq!(
            text(editable),
            Some(Color::srgb_u8(0xff, 0xff, 0xff)),
            "an editable field's text must take `--field-text`"
        );
        assert_eq!(
            text(read_only),
            Some(Color::srgb_u8(0x73, 0x7d, 0x8f)),
            "`.sk-read-only` must beat `.sk-text-field`, or a read-only field \
             reads as editable"
        );
        assert_eq!(
            text(refused),
            Some(Color::srgb_u8(0x73, 0x7d, 0x8f)),
            "`:disabled` must reach the field's text with no class of its own, \
             or every consumer is back to painting the grey itself"
        );
        Ok(())
    }

    /// **A skin can make a data surface light while the chrome stays dark**, and
    /// everything on it stays legible — the whole point of the field family
    /// (`viewer-skin-light-surface-roles`).
    ///
    /// The test skin beside this file sets the field / list tokens to the
    /// reference's Vintage values over the embedded fallback's dark chrome, and
    /// touches **nothing else**; `no Rust change` is the claim, so the nodes
    /// below are exactly the classes the widget set spawns. What it pins is the
    /// set of places a light face has to reach for the skin to be wearable at
    /// all:
    ///
    /// - the field's own face and its text, and the caret drawn on it — a caret
    ///   taken from the chrome text role is white on light sage, which is the
    ///   bug the caret rule was added to fix in the first place;
    /// - a scroll list's face, and the text roles **inside** it, which resolve
    ///   to the field family rather than to the near-white chrome ones;
    /// - a button dropped into a row, which brings its chrome back with it.
    ///
    /// All of it through the real engine, because every one of these is a
    /// cascade question (specificity and file order) that no static check on
    /// the CSS text can answer.
    #[test]
    fn a_light_field_family_stays_legible_over_dark_chrome() -> Result<(), TestError> {
        let mut app = app_with_assets(&test_assets_dir());
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("light-field.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let field = app
            .world_mut()
            .spawn((
                Text::new("typed"),
                ClassList::new("sk-field sk-text-field"),
                ChildOf(root),
            ))
            .id();
        let list = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-list-surface"),
                ChildOf(root),
            ))
            .id();
        let row = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-list-row"),
                ChildOf(list),
            ))
            .id();
        let cell = app
            .world_mut()
            .spawn((Text::new("Kelly"), ClassList::new("sk-text"), ChildOf(row)))
            .id();
        let button = app
            .world_mut()
            .spawn((Node::default(), ClassList::new("sk-button"), ChildOf(row)))
            .id();
        let caption = app
            .world_mut()
            .spawn((
                Text::new("Remove"),
                ClassList::new("sk-text"),
                ChildOf(button),
            ))
            .id();
        let panel_label = app
            .world_mut()
            .spawn((
                Text::new("Nearby"),
                ClassList::new("sk-text"),
                ChildOf(root),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let fill = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0)
        };
        let text = |entity| app.world().get::<TextColor>(entity).map(|color| color.0);

        assert_eq!(
            fill(field),
            Some(Color::srgb_u8(0xba, 0xc3, 0xbe)),
            "`.sk-field` did not take the light `--field-bg`"
        );
        assert_eq!(
            text(field),
            Some(Color::srgb_u8(0x00, 0x00, 0x00)),
            "typed text must be the field family's, or it is near-white on a \
             light face"
        );
        assert_eq!(
            app.world()
                .get::<SkinTextCaret>(field)
                .map(|caret| caret.caret),
            Some(Color::srgb_u8(0x00, 0x00, 0x00)),
            "the caret must come from the field family too — a caret derived \
             from the chrome text is invisible on a light field"
        );
        assert_eq!(
            fill(list),
            Some(Color::srgb_u8(0xc8, 0xcf, 0xcc)),
            "`.sk-list-surface` did not take `--list-bg`"
        );
        assert_eq!(
            text(cell),
            Some(Color::srgb_u8(0x00, 0x00, 0x00)),
            "a row's text must re-root onto the field family inside a list \
             surface, or every cell is near-white on light sage"
        );
        // The fallback sheet's `--text-primary`, which this fixture leaves
        // alone: the point is that the chrome role is untouched.
        let chrome_text = Some(Color::srgb_u8(0xe6, 0xeb, 0xf5));
        assert_eq!(
            text(caption),
            chrome_text,
            "a button inside a row brings its own chrome, so its caption must \
             NOT take the list's text colour"
        );
        assert_eq!(
            text(panel_label),
            chrome_text,
            "a label outside any list must keep the chrome text role — the \
             re-rooting has to be scoped to the data surface"
        );
        Ok(())
    }

    /// **The search box you are typing in brightens, and is ringed once.**
    ///
    /// The other half of `viewer-skin-search-box-focused-fill`: the widget puts
    /// `.sk-focus-within` on the box (its own test), and this is what the class
    /// then buys — `--field-bg-focused` on the container, which is the only way
    /// a search box can show the focused face at all, since the editor that
    /// takes focus is a different entity from the box that paints.
    ///
    /// The ring is the part that cannot be read off the CSS text. Two rules
    /// already ring every focused editor — `.sk-text-field:focus` on any focus
    /// and the scaffold's `.sk-focusable:focus-visible` on Tab — and a bare
    /// field fills only the middle of the box, so left alone a focused search
    /// box draws a second ring inside the first, around part of it. The
    /// suppression is a descendant rule at (0,3,0) against their (0,2,0), and
    /// specificity is exactly the kind of claim a static check cannot make: it
    /// is asserted here, through the engine, with **both** of those rules live.
    #[test]
    fn a_focused_search_box_rings_the_box_and_not_the_editor() -> Result<(), TestError> {
        use bevy::input_focus::{InputFocus, InputFocusVisible};

        let mut app = app_with_assets(&test_assets_dir());
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("light-field.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        // Two boxes, each holding a bare editor with the classes the scaffold
        // stamps on every one (`stamp_text_field_class` / `stamp_focus_ring_class`).
        let resting = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-search-field"),
                ChildOf(root),
            ))
            .id();
        app.world_mut().spawn((
            Text::new("typed"),
            ClassList::new("sk-text-field sk-focusable"),
            ChildOf(resting),
        ));
        let lit = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-search-field sk-focus-within"),
                ChildOf(root),
            ))
            .id();
        let editor = app
            .world_mut()
            .spawn((
                Text::new("typed"),
                ClassList::new("sk-text-field sk-focusable"),
                ChildOf(lit),
            ))
            .id();
        // Focus, and *visibly* so: the suppression has to beat the any-focus
        // rule and the Tab-only one at once.
        app.world_mut()
            .insert_resource(InputFocus::from_entity(editor));
        app.world_mut().insert_resource(InputFocusVisible(true));

        load(&mut app, &handle)?;
        app.update();

        let fill = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0)
        };
        let ring = |entity| {
            app.world()
                .get::<Outline>(entity)
                .map(|outline| outline.width)
        };

        assert_eq!(
            fill(resting),
            Some(Color::srgb_u8(0xba, 0xc3, 0xbe)),
            "a box nobody is typing in must keep `--field-bg`"
        );
        assert_eq!(
            fill(lit),
            Some(Color::srgb_u8(0xc8, 0xcf, 0xcc)),
            "`.sk-focus-within` must bring `--field-bg-focused` to the BOX — \
             `.sk-field:focus` cannot, because the box is not what has focus"
        );
        assert_eq!(
            ring(lit),
            Some(Val::Px(2.0)),
            "the focus ring belongs on the box, the way the reference lights \
             the search editor's own border"
        );
        assert_eq!(
            ring(editor),
            Some(Val::Px(0.0)),
            "and must be taken off the editor inside it, or a focused search \
             box is ringed twice — once around part of itself"
        );
        assert_eq!(
            ring(resting),
            Some(Val::Px(0.0)),
            "a resting box needs a baseline of its own, or the ring it is given \
             once is never taken away"
        );
        Ok(())
    }

    /// **A skin can reshape the focus ring, not only recolour it.**
    ///
    /// The ring's colour was always `--focus-ring`; its *geometry* was a pair
    /// of literals repeated by the three rules that ring — a focusable widget,
    /// a focused editor, a focused search box — so a skin whose whole look is a
    /// hairline tight against the control could only get one by restating all
    /// three. `--focus-ring-width` / `--focus-ring-offset` make it a value.
    ///
    /// Worth a test of its own because both shipped skins give the tokens the
    /// same numbers the literals had, so neither can tell a live token from a
    /// literal that agrees with it — and a `var()` `bevy_flair` failed to parse
    /// in a *length* property would leave the ring at nothing, with no log.
    /// The fixture beside this file sets a 1 px ring at zero offset and touches
    /// nothing else.
    #[test]
    fn a_skin_can_reshape_the_focus_ring_by_value() -> Result<(), TestError> {
        use bevy::input_focus::{InputFocus, InputFocusVisible};

        let mut app = app_with_assets(&test_assets_dir());
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("hairline-ring.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        // One of each ringed thing: a plain focusable (Tab only), an editor
        // (any focus), and a search box (which is ringed through its class).
        let focusable = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-focusable sk-button"),
                ChildOf(root),
            ))
            .id();
        let search = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-search-field sk-focus-within"),
                ChildOf(root),
            ))
            .id();
        app.world_mut()
            .insert_resource(InputFocus::from_entity(focusable));
        app.world_mut().insert_resource(InputFocusVisible(true));

        load(&mut app, &handle)?;
        app.update();

        let outline = |entity| {
            app.world()
                .get::<Outline>(entity)
                .map(|outline| (outline.width, outline.offset))
        };
        assert_eq!(
            outline(focusable).ok_or("the focusable resolved no outline at all")?,
            (Val::Px(1.0), Val::Px(0.0)),
            "a skin that sets `--focus-ring-width` / `--focus-ring-offset` must \
             get that ring — if these are still the fallback's 2px/1px the \
             tokens are not wired, and if they are zero the `var()` did not \
             parse in a length property"
        );
        assert_eq!(
            outline(search).ok_or("the search box resolved no outline at all")?,
            (Val::Px(1.0), Val::Px(0.0)),
            "every rule that rings must read the same pair, or a skin reshapes \
             the ring in some places and not others"
        );
        Ok(())
    }

    /// **A skin draws a bevel by value, and it stays lit from the same corner
    /// under RTL** (`viewer-skin-bevel-border-policy`).
    ///
    /// A bevel is the one paint with a handedness that must *not* follow the
    /// writing direction — mirrored, it looks lit from the wrong side — so its
    /// four side colours are physical, written by `common.css` from the
    /// `--button-bevel-*` / `--field-bevel-*` tokens and forbidden to a skin.
    /// The fixture beside this file sets those tokens to Vintage's inverted
    /// bevel (dark top-left, light bottom-right, on the button and the field
    /// alike) and nothing else.
    ///
    /// Four things pinned: each widget's two edge groups land on the right
    /// sides; the button and the field read their own pairs rather than one;
    /// flipping the root to `dir="rtl"` — the attribute every locale-aware
    /// selector reads — moves no edge; and a refused field drops back to a
    /// flat frame, because `:disabled`'s one-colour `border-color` has to
    /// beat the bevel's longhands, which is a cascade question the CSS text
    /// cannot answer on its own.
    #[test]
    fn a_bevel_stays_lit_from_the_same_corner_under_rtl() -> Result<(), TestError> {
        let mut app = app_with_assets(&test_assets_dir());
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("vintage-bevel.css");
        let mut direction = AttributeList::new();
        direction.set_attribute("dir", "ltr");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone()), direction))
            .id();
        let mut spawn = |classes: &str| {
            app.world_mut()
                .spawn((Node::default(), ClassList::new(classes), ChildOf(root)))
                .id()
        };
        let button = spawn("sk-button");
        let field = spawn("sk-field");
        let search = spawn("sk-search-field");
        let refused = spawn("sk-field");
        app.world_mut()
            .entity_mut(refused)
            .insert(bevy::ui::InteractionDisabled);

        load(&mut app, &handle)?;
        app.update();

        let black = Color::srgb_u8(0x00, 0x00, 0x00);
        let bevel = |top_left: Color, bottom_right: Color| BorderColor {
            top: top_left,
            left: top_left,
            bottom: bottom_right,
            right: bottom_right,
        };
        let raised_button = bevel(black, Color::srgb_u8(0x73, 0x84, 0x9b));
        let sunken_field = bevel(black, Color::srgb_u8(0xd8, 0xd8, 0xd8));
        let border = |app: &App, entity| app.world().get::<BorderColor>(entity).copied();

        for pass in ["ltr", "rtl"] {
            app.world_mut()
                .get_mut::<AttributeList>(root)
                .ok_or("the root lost its attribute list")?
                .set_attribute("dir", pass);
            app.update();
            assert_eq!(
                border(&app, button),
                Some(raised_button),
                "{pass}: the button's top/left must take `--button-bevel-top-left` \
                 and its bottom/right the other token — a mirrored or collapsed \
                 bevel is lit from a corner no skin chose"
            );
            assert_eq!(
                border(&app, field),
                Some(sunken_field),
                "{pass}: a field reads its OWN pair — if this is the button's, \
                 one token pair is doing two widgets' jobs"
            );
            assert_eq!(
                border(&app, search),
                Some(sunken_field),
                "{pass}: a search box is a field well and must bevel like one"
            );
        }
        let flat = Color::srgb_u8(0x47, 0x47, 0x52);
        assert_eq!(
            border(&app, refused),
            Some(BorderColor::all(flat)),
            "a refused field is flat `--control-border-disabled` on all four \
             sides — `border-color` in `.sk-field:disabled` must beat the \
             bevel's per-side longhands"
        );
        Ok(())
    }

    /// **A combo's drop-down has a surface of its own, and an opaque one.**
    ///
    /// It is a *list*, so it takes the field family's text — and it **floats**,
    /// which is the half that was got wrong: it was given `--list-bg`, a 25%
    /// black scrim meant for a list sitting in a panel, and a drop-down has no
    /// panel behind it. The chrome and the world read straight through the
    /// options. The reference makes `ComboListBgColor` opaque in every skin it
    /// ships, and gives it a name of its own for exactly this reason.
    ///
    /// Asserted as "not the scrim, and not see-through" rather than against one
    /// hex value, because what matters is the property, not the colour a skin
    /// happens to choose.
    #[test]
    fn a_floating_drop_down_is_not_a_scrim() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let popover = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-combo-list"),
                ChildOf(root),
            ))
            .id();
        let embedded = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-list-surface"),
                ChildOf(root),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let fill = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0.to_srgba())
        };
        let drop_down = fill(popover).ok_or("the drop-down resolved no background at all")?;
        assert!(
            drop_down.alpha >= 1.0,
            "a drop-down floats, so ANY transparency puts whatever is behind it \
             among the option labels — 5% was enough to read as a missing \
             background over text: {drop_down:?}"
        );
        assert_ne!(
            Some(drop_down),
            fill(embedded),
            "a floating drop-down and a list embedded in a panel must not share \
             one token — that is how the scrim got onto the drop-down"
        );
        Ok(())
    }

    /// **A skin that bands its lists gets banded lists, with no Rust change.**
    ///
    /// The other half of `a_rows_states_resolve_stripe_then_hover_then_selection`:
    /// that one pins the precedence in a skin where the stripe is transparent,
    /// this one gives the four row roles the reference's measured Vintage
    /// values and reads them back. Both flat skins leave a list's rows to the
    /// list's own face, so without a fixture nothing here would be visible in
    /// any sheet the binary ships — which is exactly the case that hides a
    /// token that never reached its rule.
    ///
    /// The caption of a **button** inside a selected row is the assertion that
    /// pays for itself: `--list-row-selected-text` is black in this fixture
    /// and the chrome text is near-white, so a selected-row rule that reached
    /// past the row's own cells would put black text on a dark button.
    #[test]
    fn a_light_list_bands_hovers_and_selects() -> Result<(), TestError> {
        use bevy::picking::hover::Hovered;

        let mut app = app_with_assets(&test_assets_dir());
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("light-field.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let list = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-list-surface"),
                ChildOf(root),
            ))
            .id();
        let mut row = |classes: &str, hovered: bool| {
            let entity = app
                .world_mut()
                .spawn((Node::default(), ClassList::new(classes), ChildOf(list)))
                .id();
            if hovered {
                app.world_mut().entity_mut(entity).insert(Hovered(true));
            }
            entity
        };
        let resting = row("sk-list-row", false);
        let striped = row("sk-list-row sk-stripe", false);
        let hovered = row("sk-list-row", true);
        let selected = row("sk-list-row sk-active", false);

        let selected_cell = app
            .world_mut()
            .spawn((
                Text::new("Kelly"),
                ClassList::new("sk-text"),
                ChildOf(selected),
            ))
            .id();
        let button = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-button"),
                ChildOf(selected),
            ))
            .id();
        let caption = app
            .world_mut()
            .spawn((
                Text::new("Remove"),
                ClassList::new("sk-text"),
                ChildOf(button),
            ))
            .id();

        load(&mut app, &handle)?;
        app.update();

        let fill = |entity| {
            app.world()
                .get::<BackgroundColor>(entity)
                .map(|background| background.0)
        };
        let text = |entity| app.world().get::<TextColor>(entity).map(|color| color.0);

        assert_eq!(
            fill(resting).map(|color| color.to_srgba()),
            Some(Srgba::NONE),
            "an ordinary row is the list's own face showing through — the \
             fixture leaves `--list-row-bg` at the fallback's transparent"
        );
        assert_eq!(
            fill(striped),
            Some(Color::srgb_u8(0xb8, 0xbf, 0xbb)),
            "`ScrollBGStripeColor` never reached the row, so a banded skin \
             gets no bands"
        );
        assert_eq!(
            fill(hovered),
            Some(Color::srgb_u8(0xbe, 0xc3, 0xc3)),
            "`ScrollHoveredColor` never reached the row"
        );
        assert_eq!(
            fill(selected),
            Some(Color::srgb_u8(0x8d, 0x90, 0xc2)),
            "`ScrollSelectedBGColor` never reached the row — a light list \
             cannot take the chrome's blue selection"
        );
        assert_eq!(
            text(selected_cell),
            Some(Color::srgb_u8(0x00, 0x00, 0x00)),
            "a selected row's cell must take the skin's selected text"
        );
        assert_eq!(
            text(caption),
            Some(Color::srgb_u8(0xe6, 0xeb, 0xf5)),
            "a button inside a SELECTED row must keep the chrome text, the \
             same exception a button in an ordinary row already has"
        );
        Ok(())
    }

    /// A refused action button greys — **both** halves of it.
    ///
    /// This is the pair that broke once already: `set_action_button_enabled`
    /// stopped painting, on the strength of `.sk-action-button:disabled` and
    /// its descendant rule, and the spawn helper was never given the classes
    /// those select on — so a refused button looked exactly like one that
    /// would answer a click, in the live viewer only. `rows.rs` asserts the
    /// spawn end; this asserts the rule end, against the real shipped skin.
    #[test]
    fn a_refused_action_button_greys_box_and_caption() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();

        let refused = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-action-button"),
                bevy::ui::InteractionDisabled,
                ChildOf(root),
            ))
            .id();
        let refused_caption = app
            .world_mut()
            .spawn((
                Text::new("Do it"),
                ClassList::new("sk-text"),
                ChildOf(refused),
            ))
            .id();
        let live = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-action-button"),
                ChildOf(root),
            ))
            .id();
        let live_caption = app
            .world_mut()
            .spawn((Text::new("Do it"), ClassList::new("sk-text"), ChildOf(live)))
            .id();

        load(&mut app, &handle)?;
        app.update();

        assert_eq!(
            app.world()
                .get::<BackgroundColor>(refused)
                .map(|background| background.0),
            Some(Color::srgb_u8(0x23, 0x27, 0x30)),
            "`.sk-action-button:disabled` did not match, so a refused button \
             keeps its live fill"
        );
        let text = |entity| app.world().get::<TextColor>(entity).map(|color| color.0);
        assert_eq!(
            text(refused_caption),
            Some(Color::srgb_u8(0x73, 0x7d, 0x8f)),
            "the descendant half did not match, so a refused button's caption \
             reads as live"
        );
        assert_eq!(
            text(live_caption),
            Some(Color::srgb_u8(0xe6, 0xeb, 0xf2)),
            "an enabled button's caption must fall back to `.sk-text`"
        );
        // The live button's fill is its panel's, not the skin's: no rule
        // matches it at all, which is the point of not using `.sk-button` here.
        assert_ne!(
            app.world()
                .get::<BackgroundColor>(live)
                .map(|background| background.0),
            Some(Color::srgb_u8(0x23, 0x27, 0x30)),
            "an enabled action button must take no fill from the skin"
        );
        Ok(())
    }
}
