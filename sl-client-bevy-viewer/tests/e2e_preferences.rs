//! End-to-end tests for the viewer's own settings and the surfaces they
//! dress, on the fake grid, through the automation driver alone, on both
//! backends ([[test-e2e-sweep-single-viewer-ui]]):
//!
//! - Preferences' colours and skins: a skin flip re-dresses the UI and the
//!   colours nobody overrode follow it; a chat colour picked for oneself
//!   recolours the chat overlay and the Nearby transcript, Cancel takes it
//!   back and OK keeps it; the account's settings file gains only that colour,
//!   and the row's Reset hands it back to the skin; a skin given on the
//!   command line is worn without being stored; a sheet edited on disk
//!   re-dresses a viewer that watches its skins;
//! - the debug-settings editor: a row fills the detail pane, an edit takes
//!   effect, a reset clears its own scope, Copy Name reaches a paste, and an
//!   edit made in Preferences shows in the open editor;
//! - in a right-to-left language, a tab strip that overflows to the left
//!   still scrolls to its far end;
//! - with web media on, the web browser window and the search window's web
//!   tab load a page.

#[cfg(test)]
mod test {
    use core::time::Duration;
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use pretty_assertions::assert_eq;
    use sl_automation_proto::{Locator, NodeVisibility, Role};
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::RegionConfig;
    use sl_fake_grid::scenario::Scenario;
    use sl_viewer_driver::{UiLocator, Viewer};

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something may take to arrive: a page, a settings write.
    const WAIT: Duration = Duration::from_secs(60);

    /// How often a file the viewer writes in its own time is looked at.
    const FILE_POLL: Duration = Duration::from_millis(100);

    /// A stage for `name` with the one viewer `Alpha`.
    fn stage(name: &str) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
    }

    /// The Preferences window's floater id.
    const PREFERENCES: &str = "preferences";

    /// Open Preferences on the tab whose caption is `tab`.
    async fn preferences_tab(alpha: &Viewer, tab: &str) -> Result<UiLocator, BodyError> {
        let window = alpha.open_floater(PREFERENCES).await?;
        let _tab = window
            .get(Locator::role(Role::Tab).name_key(tab))
            .click()
            .await?;
        Ok(window)
    }

    /// Preferences' button `key` (OK, Cancel).
    fn preferences_button(window: &UiLocator, key: &str) -> UiLocator {
        window.test_id(&format!("preferences:button:{key}"))
    }

    /// Every file called `name` anywhere under `dir`.
    fn files_named(dir: &Path, name: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let Ok(entries) = fs_err::read_dir(dir) else {
            return found;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(files_named(&path, name));
            } else if path.file_name().is_some_and(|file| file == name) {
                found.push(path);
            }
        }
        found
    }

    /// The text of viewer `label`'s settings file `name` — the account's
    /// `settings.toml` (under an `accounts` tree) or the global
    /// `viewer-settings.toml` — once `holds` accepts it. The viewer writes its
    /// settings in its own time, so this is the one place a test waits on a
    /// file: it looks every [`FILE_POLL`] until [`WAIT`] runs out.
    async fn settings_file_until(
        stage: &Stage,
        label: &str,
        name: &str,
        holds: impl Fn(&str) -> bool,
    ) -> Result<String, BodyError> {
        let dir = stage.artifacts(label)?.to_path_buf();
        let started = tokio::time::Instant::now();
        let mut last = String::new();
        loop {
            let texts: Vec<String> = files_named(&dir, name)
                .iter()
                .filter(|path| {
                    name != "settings.toml"
                        || path.components().any(|part| part.as_os_str() == "accounts")
                })
                .filter_map(|path| fs_err::read_to_string(path).ok())
                .collect();
            if let Some(text) = texts.iter().find(|text| holds(text)) {
                return Ok(text.clone());
            }
            if let Some(text) = texts.into_iter().next() {
                last = text;
            }
            if started.elapsed() >= WAIT {
                return Err(format!(
                    "viewer {label}'s {name} never held what was expected; it reads:\n{last}"
                )
                .into());
            }
            tokio::time::sleep(FILE_POLL).await;
        }
    }

    // ---- Colours and skins ------------------------------------------------

    /// The colors & skins tab.
    const COLORS_TAB: &str = "preferences-tab-colors-skins";

    /// The chat-self colour's row key.
    const CHAT_SELF: &str = "preferences-row-chat-color-self";

    /// The chat-others colour's row key, which the test never overrides.
    const CHAT_OTHERS: &str = "preferences-row-chat-color-others";

    /// Each shipped skin's own chat-self colour (its `--chat-self` token).
    const GRAPHITE_SELF: &str = "#ffffff";

    /// Azure's.
    const AZURE_SELF: &str = "#eaf3ff";

    /// Vintage's.
    const VINTAGE_SELF: &str = "#ffff00";

    /// The colour the test picks for its own chat.
    const PICKED: &str = "#ff0000";

    /// The colour row `key`'s swatch in the Preferences window.
    fn swatch(window: &UiLocator, key: &str) -> UiLocator {
        window.test_id(&format!("{key}:color-swatch"))
    }

    /// The colour picker a swatch of row `key` in Preferences opens.
    fn picker(alpha: &Viewer, key: &str) -> UiLocator {
        alpha
            .ui()
            .window(&format!("color-picker#{PREFERENCES}/{key}"))
    }

    /// Open row `key`'s picker and type `hex` into it.
    async fn pick(
        alpha: &Viewer,
        window: &UiLocator,
        key: &str,
        hex: &str,
    ) -> Result<UiLocator, BodyError> {
        let _opened = swatch(window, key).click().await?;
        let picker = picker(alpha, key);
        let _shown = alpha.expect(&picker).timeout(WAIT).to_be_visible().await?;
        let field = picker.test_id("color-picker-hex:field");
        let _typed = field.fill(hex.trim_start_matches('#')).await?;
        field.press("Enter").await?;
        Ok(picker)
    }

    /// Whether the text node `node` is drawn in `hex`, whatever alpha a fade
    /// gave it.
    fn inked(node: &sl_automation_proto::UiNode, hex: &str) -> bool {
        node.color
            .as_deref()
            .is_some_and(|color| color.starts_with(hex))
    }

    /// Wait until the one text node `text` names is drawn in `hex`.
    async fn expect_ink(alpha: &Viewer, text: &UiLocator, hex: &str) -> Result<(), BodyError> {
        let mut last = None;
        for _read in 0..60 {
            let shown = alpha.expect(text).timeout(WAIT).to_be_attached().await?;
            if shown.iter().all(|node| inked(node, hex)) {
                return Ok(());
            }
            last = Some(shown);
        }
        Err(format!("never drawn in {hex}: {last:#?}").into())
    }

    /// The line the test says in nearby chat.
    const SAID: &str = "A line in my colour";

    /// **Skins and colours**: a skin flip in Preferences re-dresses the UI and
    /// the colour rows nobody overrode follow it; a colour picked for one's
    /// own chat recolours the chat overlay and the Nearby transcript at once,
    /// Cancel takes it back and OK keeps it; the account's settings file gains
    /// that colour and no other; the row's Reset hands it back to the skin.
    #[test]
    fn a_skin_flip_redresses_the_ui_and_a_picked_chat_colour_is_stored_alone()
    -> Result<(), TestError> {
        stage("colors_and_skins").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            // Something said in nearby chat, and the Nearby transcript open —
            // first, so the windows the test then works in open over it.
            let bar = alpha.ui().test_id("nearby-chat-bar").role(Role::Textbox);
            let _typed = bar.fill(SAID).await?;
            bar.press("Enter").await?;
            let _conversations = alpha.open_floater("conversations").await?;
            let window = preferences_tab(alpha, COLORS_TAB).await?;
            let _graphite = alpha
                .expect(&swatch(&window, CHAT_SELF))
                .to_have_text(GRAPHITE_SELF)
                .await?;
            let title = window.test_id("floater-title");
            let graphite_ink = title.node().await?.color;
            let _azure = window
                .test_id("preferences-row-skin:combo")
                .select_option(Locator::role(Role::ListItem).name_key("preferences-skin-azure"))
                .await?;
            let _follows = alpha
                .expect(&swatch(&window, CHAT_SELF))
                .timeout(WAIT)
                .to_have_text(AZURE_SELF)
                .await?;
            let azure_ink = title.node().await?.color;
            assert!(
                azure_ink.is_some() && azure_ink != graphite_ink,
                "the window's title is re-dressed: {graphite_ink:?} → {azure_ink:?}"
            );
            let others = swatch(&window, CHAT_OTHERS).text().await?;

            // What was said is drawn in one's own colour, the skin's.
            let overlay = alpha
                .ui()
                .test_id("chat-overlay")
                .get(Locator::role(Role::Text).name_containing(SAID));
            let transcript = alpha
                .ui()
                .test_id("conversations-transcript")
                .get(Locator::role(Role::Text).name_containing(SAID));
            expect_ink(alpha, &overlay, AZURE_SELF).await?;
            expect_ink(alpha, &transcript, AZURE_SELF).await?;

            let picker = pick(alpha, &window, CHAT_SELF, PICKED).await?;
            expect_ink(alpha, &overlay, PICKED).await?;
            expect_ink(alpha, &transcript, PICKED).await?;
            let _cancelled = picker
                .test_id("color-picker-button:color-picker-cancel")
                .click()
                .await?;
            expect_ink(alpha, &transcript, AZURE_SELF).await?;
            let _back = alpha
                .expect(&swatch(&window, CHAT_SELF))
                .to_have_text(AZURE_SELF)
                .await?;
            let picker = pick(alpha, &window, CHAT_SELF, PICKED).await?;
            let _kept = picker
                .test_id("color-picker-button:color-picker-ok")
                .click()
                .await?;
            expect_ink(alpha, &transcript, PICKED).await?;
            let _ok = preferences_button(&window, "preferences-ok")
                .click()
                .await?;

            let stored = settings_file_until(stage, "Alpha", "settings.toml", |text| {
                text.contains("ChatColorSelf")
            })
            .await?;
            for untouched in [
                "ChatColorOthers",
                "ChatColorObjects",
                "ChatColorIm",
                "NameTagColor",
            ] {
                assert!(
                    !stored.contains(untouched),
                    "only the overridden colour is stored, not {untouched}:\n{stored}"
                );
            }

            let window = preferences_tab(alpha, COLORS_TAB).await?;
            let _reset = window
                .test_id(&format!("preferences:row:{CHAT_SELF}"))
                .test_id("preferences:button:preferences-reset-default")
                .click()
                .await?;
            let _skin_again = alpha
                .expect(&swatch(&window, CHAT_SELF))
                .to_have_text(AZURE_SELF)
                .await?;
            assert_eq!(
                swatch(&window, CHAT_OTHERS).text().await?,
                others,
                "the reset touched only its own row"
            );
            let _ok = preferences_button(&window, "preferences-ok")
                .click()
                .await?;
            let _cleared = settings_file_until(stage, "Alpha", "settings.toml", |text| {
                !text.contains("ChatColorSelf")
            })
            .await?;
            Ok(())
        })?;
        Ok(())
    }

    /// **A skin from the command line**: a viewer started with `--skin
    /// vintage` wears it — the colours nobody overrode are vintage's — while
    /// Preferences still shows the stored skin, and nothing it writes on its
    /// way out stores vintage.
    #[test]
    fn a_skin_given_at_start_is_worn_but_not_stored() -> Result<(), TestError> {
        stage("skin_flag")
            .skin("vintage")
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let window = preferences_tab(alpha, COLORS_TAB).await?;
                let _worn = alpha
                    .expect(&swatch(&window, CHAT_SELF))
                    .timeout(WAIT)
                    .to_have_text(VINTAGE_SELF)
                    .await?;
                let _stored = alpha
                    .expect(&window.test_id("preferences-row-skin:combo"))
                    .to_have_text("Graphite")
                    .await?;
                let _closed = preferences_button(&window, "preferences-cancel")
                    .click()
                    .await?;
                let _relogged = stage.relog("Alpha").await?;
                let global =
                    settings_file_until(stage, "Alpha", "viewer-settings.toml", |_text| true)
                        .await?;
                assert!(
                    !global.contains("vintage"),
                    "the command line's skin was stored:\n{global}"
                );
                Ok(())
            })?;
        Ok(())
    }

    /// The colour the watched-sheet test edits graphite's `--chat-self` to.
    const EDITED_SELF: &str = "#00ff00";

    /// **A watched sheet**: a viewer started with `--watch-skins` on a copy
    /// of the asset tree re-dresses itself when its skin's `.css` changes on
    /// disk — the colour row nobody overrode follows the edited token with
    /// no restart.
    #[test]
    fn an_edited_skin_sheet_redresses_a_watching_viewer() -> Result<(), TestError> {
        stage("skin_watch")
            .watch_skins()
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let window = preferences_tab(alpha, COLORS_TAB).await?;
                let _worn = alpha
                    .expect(&swatch(&window, CHAT_SELF))
                    .timeout(WAIT)
                    .to_have_text(GRAPHITE_SELF)
                    .await?;
                let sheet = stage.assets("Alpha")?.join("skins/graphite/skin.css");
                let text = fs_err::read_to_string(&sheet)?;
                let token = format!("--chat-self: {GRAPHITE_SELF};");
                assert!(text.contains(&token), "graphite's sheet declares {token}");
                fs_err::write(
                    &sheet,
                    text.replace(&token, &format!("--chat-self: {EDITED_SELF};")),
                )?;
                let _followed = alpha
                    .expect(&swatch(&window, CHAT_SELF))
                    .timeout(WAIT)
                    .to_have_text(EDITED_SELF)
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- The debug-settings editor ----------------------------------------

    /// The debug-settings editor's floater id.
    const DEBUG_SETTINGS: &str = "debug_settings";

    /// The setting the editor test edits: the parcel property lines.
    const PROPERTY_LINES: &str = "ShowPropertyLines";

    /// The detail pane's read-out `key`.
    fn detail(window: &UiLocator, key: &str) -> UiLocator {
        window.test_id(&format!("debug-settings:value:debug-settings-{key}"))
    }

    /// Open the World menu and wait for its property-lines entry to be
    /// `ticked`, then close the menu: the setting reaching the world.
    async fn expect_property_lines(alpha: &Viewer, ticked: bool) -> Result<(), BodyError> {
        let _menu = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-bar-world"))
            .click()
            .await?;
        let entry = alpha.ui().test_id("menu-item:toggle-property-lines");
        let _state = if ticked {
            alpha.expect(&entry).to_be_checked().await?
        } else {
            alpha.expect(&entry).to_be_unchecked().await?
        };
        alpha.press("Escape").await?;
        let _closed = alpha.expect(&entry).to_be_detached().await?;
        Ok(())
    }

    /// **The debug-settings editor**: a row picked from the search fills the
    /// detail pane; unticking the setting turns the property lines off in the
    /// world and records a Global override; the scope's Reset clears only
    /// that scope; Copy Name reaches a paste; and the same setting changed in
    /// Preferences shows in the editor, still open.
    #[test]
    fn the_debug_settings_editor_edits_resets_copies_and_follows_preferences()
    -> Result<(), TestError> {
        stage("debug_settings_editor").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            alpha.press("Ctrl+Alt+Shift+S").await?;
            let window = alpha.ui().window(DEBUG_SETTINGS);
            let _open = alpha.expect(&window).to_be_visible().await?;
            let _searched = window
                .test_id("debug-settings:search")
                .role(Role::Textbox)
                .fill(PROPERTY_LINES)
                .await?;
            let _row = window
                .get(Locator::role(Role::ListItem).named(PROPERTY_LINES))
                .timeout(WAIT)
                .click()
                .await?;
            let _named = alpha
                .expect(&window.test_id("debug-settings:name"))
                .to_have_text(PROPERTY_LINES)
                .await?;
            let _typed = alpha
                .expect(&detail(&window, "type"))
                .to_contain_text("Bool")
                .await?;
            let _default = alpha
                .expect(&detail(&window, "default"))
                .to_have_text("true")
                .await?;
            expect_property_lines(alpha, true).await?;

            let edit = window.test_id("debug-settings:edit:bool:checkbox");
            edit.uncheck().await?;
            let _global = alpha
                .expect(&detail(&window, "global"))
                .to_have_text("false")
                .await?;
            let _effective = alpha
                .expect(&detail(&window, "effective"))
                .to_have_text("false")
                .await?;
            expect_property_lines(alpha, false).await?;

            let reset = window.test_id("preferences:button:debug-settings-reset");
            let _reset = reset.click().await?;
            let _cleared = alpha
                .expect(&detail(&window, "global"))
                .to_have_text("\u{2013}")
                .await?;
            let _default_again = alpha
                .expect(&detail(&window, "effective"))
                .to_have_text("true")
                .await?;
            expect_property_lines(alpha, true).await?;

            let _copied = window
                .test_id("preferences:button:debug-settings-copy-name")
                .click()
                .await?;
            let bar = alpha.ui().test_id("nearby-chat-bar").role(Role::Textbox);
            let _focused = bar.click().await?;
            bar.press("Ctrl+V").await?;
            let _pasted = alpha.expect(&bar).to_have_text(PROPERTY_LINES).await?;
            let _emptied = bar.fill("").await?;

            let preferences = preferences_tab(alpha, "preferences-tab-world-ui").await?;
            preferences
                .test_id("preferences:row:preferences-row-property-lines")
                .role(Role::Checkbox)
                .uncheck()
                .await?;
            let _followed = alpha
                .expect(&detail(&window, "effective"))
                .to_have_text("false")
                .await?;
            let _cancelled = preferences_button(&preferences, "preferences-cancel")
                .click()
                .await?;
            let _reverted = alpha
                .expect(&detail(&window, "effective"))
                .to_have_text("true")
                .await?;
            Ok(())
        })?;
        Ok(())
    }

    // ---- Right to left ----------------------------------------------------

    /// The search window's floater id.
    const SEARCH: &str = "search";

    /// **A right-to-left strip still scrolls**: the search window, narrowed
    /// until its tabs overflow, in Arabic — whose tab strip runs right to left,
    /// so the tabs that do not fit are past its *left* end. The strip's
    /// jump-to-last button brings the last tab into view, and jump-to-first
    /// takes it back out.
    #[test]
    fn a_right_to_left_tab_strip_that_overflows_to_the_left_scrolls() -> Result<(), TestError> {
        stage("rtl_tab_overflow").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            alpha.press("Ctrl+F").await?;
            let search = alpha.ui().window(SEARCH);
            let _shown = alpha.expect(&search).to_be_visible().await?;
            let _narrowed = search
                .test_id("floater-resize")
                .drag_by(-1200.0, 0.0)
                .await?;
            let arrows = search.test_id("search-tabs:tab-arrows");
            let _overflowing = alpha.expect(&arrows).timeout(WAIT).to_be_visible().await?;

            let general = preferences_tab(alpha, "preferences-tab-general").await?;
            let _arabic = general
                .test_id("preferences-row-language:combo")
                .select_option(Locator::role(Role::ListItem).name_key("preferences-locale-arabic"))
                .await?;
            let _ok = preferences_button(&general, "preferences-ok")
                .click()
                .await?;

            let tabs = search.get(Locator::role(Role::Tab)).all().await?;
            let last = tabs.last().ok_or("the search window has no tabs")?;
            let first = tabs.first().ok_or("the search window has no tabs")?;
            let _jumped = search.test_id("search-tabs:tab-arrow:last").click().await?;
            let mut seen = last.node().await?;
            for _read in 0..30 {
                if seen.visibility == NodeVisibility::Visible {
                    break;
                }
                seen = last.node().await?;
            }
            assert_eq!(
                seen.visibility,
                NodeVisibility::Visible,
                "the last tab scrolled into view: {seen:?}"
            );
            let _back = search
                .test_id("search-tabs:tab-arrow:first")
                .click()
                .await?;
            let _first = alpha.expect(first).to_be_visible().await?;
            assert_ne_visible(last).await
        })?;
        Ok(())
    }

    /// Fail unless the one node `locator` names is out of view again.
    async fn assert_ne_visible(locator: &UiLocator) -> Result<(), BodyError> {
        let mut seen = locator.node().await?;
        for _read in 0..30 {
            if seen.visibility != NodeVisibility::Visible {
                return Ok(());
            }
            seen = locator.node().await?;
        }
        Err(format!("the last tab stayed in view: {seen:?}").into())
    }

    // ---- Web pages --------------------------------------------------------

    /// The title of the page the test's server answers with.
    const PAGE_TITLE: &str = "Stage Page";

    /// Serve the one page [`PAGE_TITLE`] names on a loopback port, whatever
    /// is asked for, from a thread that lives as long as the test process;
    /// answers its base URL.
    fn serve_page() -> Result<String, TestError> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let base = format!("http://{}/", listener.local_addr()?);
        let _server = std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                // The request head, up to its blank line.
                while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                    line.clear();
                }
                let body = format!(
                    "<!doctype html><html><head><title>{PAGE_TITLE}</title></head>\
                     <body><p>Hello from the stage.</p></body></html>"
                );
                let mut writer = &stream;
                let _written = write!(
                    writer,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Ok(base)
    }

    /// The home region, its simulator naming `search` as the grid's web
    /// search, as an OpenSim region does.
    fn searching_region(search: &str) -> Result<RegionConfig, TestError> {
        let url = url::Url::parse(search)?;
        let mut scenario = Scenario::default();
        let stock = Arc::clone(&scenario.setup);
        scenario.setup = Arc::new(move |sim, now| {
            stock(sim, now);
            let mut features = sim.simulator_features().clone();
            features
                .open_sim_extras
                .get_or_insert_with(Default::default)
                .search_server_url = Some(url.clone());
            sim.set_simulator_features(features);
        });
        Ok(RegionConfig {
            scenario: Some(scenario),
            ..RegionConfig::default()
        })
    }

    /// The one page `window` shows.
    fn page(window: &UiLocator) -> UiLocator {
        window.get(Locator::role(Role::Document))
    }

    /// **Web pages load**: with web media on, an address typed into the web
    /// browser window loads that page, and the search window's web tab loads
    /// the grid's search page for a query.
    #[test]
    fn the_web_browser_and_the_search_web_tab_load_a_page() -> Result<(), TestError> {
        let base = serve_page()?;
        stage("web_pages")
            .web_media()
            .region(searching_region(&format!("{base}search"))?)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let browser = alpha.open_floater("web-browser").await?;
                let address = browser.test_id("web-address:field");
                let wanted = format!("{base}browser");
                let _typed = address.fill(&wanted).await?;
                address.press("Enter").await?;
                let _loaded = alpha
                    .expect(&page(&browser))
                    .timeout(WAIT)
                    .to_have_text(&wanted)
                    .await?;
                let _titled = alpha
                    .expect(&browser.get(Locator::role(Role::Document).named(PAGE_TITLE)))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;

                // The address bar has the focus, which would take a chord.
                let search = alpha.open_floater(SEARCH).await?;
                let _web = search
                    .get(Locator::role(Role::Tab).name_key("search-tab-web"))
                    .click()
                    .await?;
                let query = search.test_id("search-query:field");
                let _typed = query.fill("stage").await?;
                query.press("Enter").await?;
                let _searched = alpha
                    .expect(&page(&search))
                    .timeout(WAIT)
                    .to_contain_text(&format!("{base}search?q=stage"))
                    .await?;
                let _search_titled = alpha
                    .expect(&search.get(Locator::role(Role::Document).named(PAGE_TITLE)))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                Ok(())
            })?;
        Ok(())
    }
}
