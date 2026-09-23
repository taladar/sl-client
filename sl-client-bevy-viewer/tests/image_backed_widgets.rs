//! A skin can give a widget an **image surface**, per state, from CSS alone
//! (`viewer-skin-image-backed-widgets`).
//!
//! The token model can recolour a widget but not reshape it: every widget is a
//! flat rounded rectangle, and the reference's classic skins are bevels,
//! grooves and shouldered trapezoids — nine-sliced images, one per state. The
//! engine can already express that (`bevy_flair` maps `-bevy-image` onto
//! `ImageNode` and registers it with `auto_insert_remove`), so what is worth
//! proving is the end-to-end claim: **the shipped `relief` theme dresses a
//! `.sk-button` in art, a flat skin leaves it alone, and each state brings its
//! own file.**
//!
//! Failure here is silent in the worst way — a rule whose `url()` or
//! `sliced()` does not parse leaves the node flat, which looks exactly like a
//! theme that was never selected.
//!
//! Lives here rather than in `sl-viewer-ui-core` for the same reason the other
//! skin tests do: it reads `assets/skins/`, which ships with this binary.

#[cfg(test)]
mod test {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use bevy::asset::LoadState;
    use bevy::image::{CompressedImageFormats, ImageFilterMode, ImageLoader, ImageSampler};
    use bevy::picking::hover::Hovered;
    use bevy::prelude::*;
    use bevy::ui::widget::ImageNodeSize;
    use bevy::ui::{InteractionDisabled, Pressed};
    use bevy_flair::prelude::*;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// How long to pump frames before giving up on the stylesheet load — a
    /// wall-clock bound for the same reason `skin_palette_resolves` uses one.
    const LOAD_TIMEOUT: Duration = Duration::from_secs(60);

    /// How long to wait between frames while the load is in flight.
    const POLL_INTERVAL: Duration = Duration::from_millis(5);

    /// The slice insets the art is drawn to: 24x24 files with 8 px corners.
    const INSET: f32 = 8.0;

    /// An app with the CSS engine over the shipped `assets/`, ready for more
    /// plugins — [`app`] is this plus the finish, and a caller that wants the
    /// layout stack on top needs to add it *before* that.
    fn app_unfinished() -> App {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: assets.to_string_lossy().into_owned(),
                ..AssetPlugin::default()
            },
            WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..WindowPlugin::default()
            },
            FlairPlugin,
        ));
        // The image asset type, which `MinimalPlugins` does not bring: a rule
        // carrying `-bevy-image` asks the asset server for a `Handle<Image>`
        // while the sheet is still parsing, and an unregistered asset type is a
        // panic *inside the loader* — which surfaces as "the stylesheet failed
        // to load" and looks like a CSS error. No `ImagePlugin` (that wants a
        // render device); the PNG files' own geometry is checked on disk below.
        app.init_asset::<Image>();
        sl_viewer_ui_core::skin::embed_fallback_stylesheet(&mut app);
        sl_viewer_ui_core::skin_palette::register_palette_properties(&mut app);
        app
    }

    /// An app with the CSS engine over the shipped `assets/`, finished.
    fn app() -> App {
        let mut app = app_unfinished();
        app.finish();
        app.cleanup();
        app
    }

    /// Pump frames until `handle`'s stylesheet has loaded (or failed).
    fn load(app: &mut App, handle: &Handle<StyleSheet>) -> Result<(), TestError> {
        let started = Instant::now();
        loop {
            app.update();
            let state = app.world().resource::<AssetServer>().load_state(handle);
            match state {
                LoadState::Loaded => return Ok(()),
                LoadState::Failed(error) => {
                    return Err(format!("the stylesheet failed to load: {error}").into());
                }
                LoadState::NotLoaded | LoadState::Loading => {}
            }
            if started.elapsed() >= LOAD_TIMEOUT {
                return Err(format!("the stylesheet was still {state:?}").into());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Dress a root in `sheet`, hang one `.sk-button` under it, and settle.
    fn button_under(sheet: &'static str, states: impl Bundle) -> Result<(App, Entity), TestError> {
        let mut app = app();
        let handle: Handle<StyleSheet> = app.world().resource::<AssetServer>().load(sheet);
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        let button = app
            .world_mut()
            .spawn((
                Node::default(),
                ClassList::new("sk-button"),
                ChildOf(root),
                states,
            ))
            .id();
        load(&mut app, &handle)?;
        app.update();
        Ok((app, button))
    }

    /// The asset path an entity's `ImageNode` points at, if it has one.
    fn image_path(app: &App, entity: Entity) -> Option<String> {
        let node = app.world().get::<ImageNode>(entity)?;
        Some(
            app.world()
                .resource::<AssetServer>()
                .get_path(&node.image)?
                .path()
                .display()
                .to_string(),
        )
    }

    /// **The theme dresses the button in art, sliced to the file's geometry.**
    ///
    /// Both halves matter. The `ImageNode` is inserted by the rule alone — the
    /// node is spawned without one and no Rust in the viewer adds it — and the
    /// slicing is what makes the art a *surface* rather than a stretched
    /// picture: without `sliced()` a 2 px bevel drawn on a 24 px file becomes a
    /// smeared gradient on a 120 px button.
    #[test]
    fn the_relief_theme_gives_a_button_a_sliced_image_surface() -> Result<(), TestError> {
        let (app, button) = button_under("skins/graphite/themes/relief.css", ())?;

        let node = app
            .world()
            .get::<ImageNode>(button)
            .ok_or("no ImageNode was inserted, so the `-bevy-image` rule did not resolve")?;
        match node.image_mode {
            NodeImageMode::Sliced(ref slicer) => {
                assert_eq!(
                    (
                        slicer.border.min_inset.x,
                        slicer.border.min_inset.y,
                        slicer.border.max_inset.x,
                        slicer.border.max_inset.y
                    ),
                    (INSET, INSET, INSET, INSET),
                    "the slice insets are not the ones the art is drawn to"
                );
            }
            ref other => {
                return Err(format!("the image is {other:?}, not sliced — it will stretch").into());
            }
        }
        assert_eq!(
            image_path(&app, button).as_deref(),
            Some("skins/graphite/widgets/push-button.png"),
            "the resting button wears the wrong file"
        );
        // And the surface must be RENDERABLE, not merely present. `ImageNode`
        // declares `#[require(Node, ImageNodeSize)]`, and the renderer's
        // `extract_uinode_images` query asks for `&ImageNode` *and*
        // `&ImageNodeSize` together — so a node that got the first without the
        // second is skipped in total silence. Every other assertion here passes
        // on such a node: the handle is right, the slicing is right, the file
        // decodes, and the button is invisible on screen.
        assert!(
            app.world().get::<ImageNodeSize>(button).is_some(),
            "the button got an `ImageNode` with no `ImageNodeSize` beside it, so \
             `extract_uinode_images` will never match it and the art is never \
             drawn — the rule's required components did not come with it"
        );
        Ok(())
    }

    /// **Each state brings its own file**, which is the reference's whole state
    /// model: three textures per button, swapped by state rather than tinted.
    ///
    /// Driven through the markers `bevy_flair` syncs its pseudo-classes from,
    /// so this asserts the selector wiring and not just the parse.
    #[test]
    fn every_button_state_swaps_the_file() -> Result<(), TestError> {
        for (state, file) in [
            ("hover", "push-button-hover.png"),
            ("active", "push-button-pressed.png"),
            ("disabled", "push-button-disabled.png"),
        ] {
            let (app, button) = match state {
                "hover" => button_under("skins/graphite/themes/relief.css", Hovered(true))?,
                "active" => button_under("skins/graphite/themes/relief.css", Pressed)?,
                _refused => button_under("skins/graphite/themes/relief.css", InteractionDisabled)?,
            };
            assert_eq!(
                image_path(&app, button),
                Some(format!("skins/graphite/widgets/{file}")),
                "`:{state}` did not swap the button's surface"
            );
        }
        Ok(())
    }

    /// **Every file the theme names decodes, at a size its insets fit, and is
    /// sampled without being smeared.**
    ///
    /// Three things no styled entity can answer, together because each needs
    /// the art actually read off the disk:
    ///
    /// - the asset server hands back a handle for a path that does not exist
    ///   and says nothing, so a renamed PNG would leave every other test here
    ///   green and the button flat on screen;
    /// - `sliced(8px …)` on a file narrower than 16 px would have the corners
    ///   overlap;
    /// - `ImagePlugin`'s default sampler is **linear**, which is right for the
    ///   world's textures and wrong for a 2 px bevel — at any UI scale but 1.0
    ///   it blurs the line the whole look rests on. Each PNG ships a `.meta`
    ///   asking for nearest, and get one letter of that RON wrong and it is
    ///   ignored in silence.
    #[test]
    fn every_file_the_theme_names_decodes_and_is_sampled_nearest() -> Result<(), TestError> {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let theme = fs_err::read_to_string(
            assets
                .join("skins")
                .join("graphite")
                .join("themes")
                .join("relief.css"),
        )?;
        // Only what a `url()` names: the prose above the rules mentions the
        // generator script, which is not art and has no `.png` to find.
        let named: Vec<String> = theme
            .split("widgets/")
            .skip(1)
            .filter_map(|tail| tail.split_once(".png").map(|(name, _rest)| name))
            .filter(|name| !name.contains(char::is_whitespace))
            .map(|name| format!("skins/graphite/widgets/{name}.png"))
            .collect();
        assert_eq!(
            named.len(),
            4,
            "the theme names {} files, not the four states",
            named.len()
        );

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: assets.to_string_lossy().into_owned(),
                ..AssetPlugin::default()
            },
        ));
        // `ImagePlugin` only *preregisters* the loader's extensions — the real
        // `ImageLoader` is registered by `bevy_render`'s wrapper, which wants a
        // GPU. Registering it directly is what makes a PNG decode headlessly;
        // preregistration alone leaves the load pending for ever, which is what
        // it did. No default sampler is installed either, so an ignored `.meta`
        // leaves `ImageSampler::Default` and fails below, rather than passing
        // by agreeing with a default this test picked.
        app.init_asset::<Image>();
        app.register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE));
        app.finish();
        app.cleanup();

        let handles: Vec<(String, Handle<Image>)> = named
            .into_iter()
            .map(|path| {
                let handle = app.world().resource::<AssetServer>().load(path.clone());
                (path, handle)
            })
            .collect();
        for (path, handle) in &handles {
            let started = Instant::now();
            loop {
                app.update();
                match app.world().resource::<AssetServer>().load_state(handle) {
                    LoadState::Loaded => break,
                    LoadState::Failed(error) => {
                        return Err(format!("`{path}` failed to load: {error}").into());
                    }
                    LoadState::NotLoaded | LoadState::Loading => {}
                }
                if started.elapsed() >= LOAD_TIMEOUT {
                    return Err(format!("`{path}` never loaded").into());
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            let image = app
                .world()
                .resource::<Assets<Image>>()
                .get(handle)
                .ok_or_else(|| format!("`{path}` loaded but is not in the collection"))?;
            assert!(
                f64::from(image.width()) >= f64::from(INSET) * 2.0
                    && f64::from(image.height()) >= f64::from(INSET) * 2.0,
                "`{path}` is {}x{}, smaller than its own slice insets",
                image.width(),
                image.height()
            );
            match image.sampler {
                ImageSampler::Descriptor(ref descriptor) => {
                    assert_eq!(
                        (descriptor.mag_filter, descriptor.min_filter),
                        (ImageFilterMode::Nearest, ImageFilterMode::Nearest),
                        "`{path}`'s meta loaded but asks for the wrong filter"
                    );
                }
                ImageSampler::Default => {
                    return Err(
                        format!("`{path}`'s `.meta` was ignored, so it samples linear").into(),
                    );
                }
            }
        }
        Ok(())
    }

    /// **Dressing a button in art does not move its caption.**
    ///
    /// bevy_ui lays a node's content out inside padding *and* border, so the
    /// two together are the caption's inset — and an image-backed theme must
    /// leave that inset alone or every button in the viewer tightens by the
    /// border it dropped. It is a tempting thing to drop, too: the art carries
    /// its own frame, so a painted border on top of it reads as a double edge.
    /// The fix is to make the border *transparent* rather than zero-width, and
    /// this is what says so — `border-width: 0` in the theme fails here with
    /// the caption 2 px nearer the edge on every side, which on screen is a
    /// label sitting on the bevel.
    #[test]
    fn the_art_does_not_shrink_the_box_the_caption_sits_in() -> Result<(), TestError> {
        let inset = |sheet| -> Result<(f32, f32), TestError> {
            let (app, button) = button_under(sheet, ())?;
            let node = app
                .world()
                .get::<Node>(button)
                .ok_or("the button lost its Node")?;
            // Only the `Px` cases are meaningful here, and both sheets use
            // them; anything else means the sheet changed shape under us.
            let px = |value| match value {
                Val::Px(pixels) => Ok(pixels),
                other => Err(format!("`{sheet}` sets a non-pixel box value: {other:?}")),
            };
            Ok((
                px(node.padding.top)? + px(node.border.top)?,
                px(node.padding.left)? + px(node.border.left)?,
            ))
        };
        assert_eq!(
            inset("skins/graphite/themes/relief.css")?,
            inset("skins/graphite/skin.css")?,
            "the relief theme changed how far a button's caption sits from its \
             edge — the art must replace the border's PAINT, not its WIDTH"
        );
        Ok(())
    }

    /// **Art does not decide how big a button is — its caption does.**
    ///
    /// A `bevy_ui` node carrying an `ImageNode` in `NodeImageMode::Auto` takes
    /// a **content-size measure** from the image, and a measure replaces the
    /// node's children as the thing that sizes it: the button stops being as
    /// big as its label and becomes as big as the picture — 24x24, the size of
    /// the art. `sliced()` is what clears that measure, so the whole of this
    /// theme rests on the mode reaching `ImageNode` and *staying* there.
    ///
    /// The tell is an asymmetry: a button with an explicit width keeps it (the
    /// style beats the measure) and collapses only in height, while a button
    /// sized by its content collapses in both. So this lays two buttons out
    /// for real — one flat, one dressed — and asserts the dressed one is no
    /// smaller.
    #[test]
    fn the_art_does_not_decide_how_big_the_button_is() -> Result<(), TestError> {
        let size_under = |sheet: &'static str| -> Result<Vec2, TestError> {
            let mut app = app_unfinished();
            sl_viewer_testkit::LayoutTest::new().install(&mut app, sl_viewer_testkit::UiHost::Bare);
            app.finish();
            app.cleanup();
            let handle: Handle<StyleSheet> = app.world().resource::<AssetServer>().load(sheet);
            let root = app
                .world_mut()
                .spawn((Node::default(), Styled::new(handle.clone())))
                .id();
            let button = app
                .world_mut()
                .spawn((Node::default(), ClassList::new("sk-button"), ChildOf(root)))
                .id();
            // A caption long enough that "as big as the label" and "as big as
            // the 24x24 art" cannot be confused for one another.
            app.world_mut().spawn((
                Text::new("Submit this notecard"),
                TextFont::from_font_size(14.0),
                ChildOf(button),
            ));
            load(&mut app, &handle)?;
            // Layout settles a frame behind the style that drives it.
            app.update();
            app.update();
            let computed = app
                .world()
                .get::<ComputedNode>(button)
                .ok_or("the button never reached layout")?;
            Ok(computed.size())
        };

        let flat = size_under("skins/graphite/skin.css")?;
        let dressed = size_under("skins/graphite/themes/relief.css")?;
        assert!(
            dressed.x >= flat.x && dressed.y >= flat.y,
            "the relief theme shrank the button from {flat:?} to {dressed:?} — \
             the art is sizing the node instead of the caption, which is what \
             an `ImageNode` left in `NodeImageMode::Auto` does (the art is \
             24x24)"
        );
        Ok(())
    }

    /// **The art covers the whole button, not just its caption.**
    ///
    /// `ImageNode::default()` paints into the **content** box — inside the
    /// padding *and* the border — so a surface brought in by CSS covers the
    /// node minus its padding unless the rule says otherwise. On a real dialog
    /// button (`padding: 5px 10px`, `border: 2px`) that is a bevel drawn at
    /// 88x18 inside a 112x32 box: a frame hugging the label with bare panel
    /// all around it, which on screen reads as a button that lost its padding.
    ///
    /// Nothing else here catches it. The node keeps its size, the handle and
    /// the slicing are right, `ImageNodeSize` is present, the file decodes —
    /// every other assertion in this file passes while the button looks wrong.
    /// So this one asserts the *box* the art is painted into.
    #[test]
    fn the_art_covers_the_button_and_not_just_its_caption() -> Result<(), TestError> {
        let sheet = "skins/graphite/themes/relief.css";
        let mut app = app_unfinished();
        sl_viewer_testkit::LayoutTest::new().install(&mut app, sl_viewer_testkit::UiHost::Bare);
        app.finish();
        app.cleanup();
        let handle: Handle<StyleSheet> = app.world().resource::<AssetServer>().load(sheet);
        let root = app
            .world_mut()
            .spawn((Node::default(), Styled::new(handle.clone())))
            .id();
        // Exactly `script_dialog::spawn_grid_button`'s box.
        let button = app
            .world_mut()
            .spawn((
                Node {
                    width: Val::Px(112.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ClassList::new("sk-button"),
                ChildOf(root),
            ))
            .id();
        app.world_mut().spawn((
            Text::new("Gift"),
            TextFont::from_font_size(15.0),
            ChildOf(button),
        ));
        load(&mut app, &handle)?;
        app.update();
        app.update();
        let image = app
            .world()
            .get::<ImageNode>(button)
            .ok_or_else(|| format!("`{sheet}` put no image on the button"))?;
        assert_eq!(
            image.visual_box,
            VisualBox::BorderBox,
            "`{sheet}` paints the button's art into {:?} — the default is \
             `ContentBox`, which insets the surface by the button's padding \
             and border and leaves a bevel hugging the caption",
            image.visual_box
        );
        Ok(())
    }

    /// **A flat skin is left flat.**
    ///
    /// The other half of "the shipped skins are visually unchanged": Graphite
    /// sets no image, so no `ImageNode` is inserted at all — the node keeps the
    /// background colour and border it always had. A rule that leaked into the
    /// base sheet would show up here rather than in someone's screenshot.
    #[test]
    fn a_flat_skin_leaves_the_button_flat() -> Result<(), TestError> {
        for sheet in [
            "skins/graphite/skin.css",
            "skins/azure/skin.css",
            "skins/graphite/themes/dark.css",
        ] {
            let (app, button) = button_under(sheet, ())?;
            assert!(
                app.world().get::<ImageNode>(button).is_none(),
                "`{sheet}` put an image on a button — the flat skins must stay flat"
            );
            let background = app
                .world()
                .get::<BackgroundColor>(button)
                .map(|colour| colour.0.to_srgba().alpha);
            assert_eq!(
                background.map(|alpha| alpha > 0.0),
                Some(true),
                "`{sheet}` left the button's surface transparent with no image to replace it"
            );
        }
        Ok(())
    }
}
