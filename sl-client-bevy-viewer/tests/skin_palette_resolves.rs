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
}
