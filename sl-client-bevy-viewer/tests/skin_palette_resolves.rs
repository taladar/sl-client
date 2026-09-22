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
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
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
            (box_node, tick)
        };
        let (off_box, off_tick) = spawn_box(false, false);
        let (on_box, on_tick) = spawn_box(true, false);
        let (refused_box, refused_tick) = spawn_box(false, true);
        let (refused_on_box, refused_on_tick) = spawn_box(true, true);

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
        Ok(())
    }

    /// **A radio's ring is the skin's too**, both of them.
    ///
    /// The same move as the checkbox: `apply_radio_selection` used to rewrite
    /// the glyph between `○` and `◉`, and that loop is gone — the option's
    /// `:checked` picks the filled ring through `content`. A skin wanting a
    /// filled dot or a square says so in one rule.
    #[test]
    fn a_radio_takes_both_of_its_rings_from_the_skin() -> Result<(), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app
            .world()
            .resource::<AssetServer>()
            .load("skins/graphite/skin.css");
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let mut spawn_option = |checked: bool| {
            let mut item =
                app.world_mut()
                    .spawn((Node::default(), ClassList::new("sk-radio"), ChildOf(root)));
            if checked {
                item.insert(bevy::ui::Checked);
            }
            let item = item.id();
            app.world_mut()
                .spawn((
                    Text::default(),
                    PseudoElementsSupport,
                    ClassList::new("sk-radio-indicator"),
                    ChildOf(item),
                ))
                .id()
        };
        let resting = spawn_option(false);
        let lit = spawn_option(true);

        load(&mut app, &handle)?;
        app.update();

        let ring = |entity| {
            app.world()
                .get::<Children>(entity)
                .and_then(|kids| kids.iter().next())
                .and_then(|before| app.world().get::<TextSpan>(before))
                .map(|span| span.0.clone())
        };
        assert_eq!(
            ring(resting),
            Some("\u{25cb}".to_owned()),
            "an unselected option must carry the empty ring"
        );
        assert_eq!(
            ring(lit),
            Some("\u{25c9}".to_owned()),
            "`:checked` must reach the ring, or selection is colour-only and a \
             recolour-free skin cannot show it"
        );
        Ok(())
    }

    /// A field that cannot be edited greys, and an ordinary one does not.
    ///
    /// `.sk-text-field` is stamped on every editor by the scaffold and now
    /// carries the typed text's colour; `reflect_uneditable_text_color` adds
    /// `.sk-disabled-text` for a disabled *or* read-only field. Both rules are
    /// a single class, so which wins is the file's order — the same thing
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
        let refused = app
            .world_mut()
            .spawn((
                Text::new("typed"),
                ClassList::new("sk-text-field sk-disabled-text"),
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
            text(refused),
            Some(Color::srgb_u8(0x73, 0x7d, 0x8f)),
            "`.sk-disabled-text` must beat `.sk-text-field`, or a read-only \
             field reads as editable"
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
