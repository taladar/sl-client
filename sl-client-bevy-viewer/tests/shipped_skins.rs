//! The shipped skins are well-formed: every declared skin and theme exists,
//! every palette token is defined, and no stylesheet uses a banned physical box
//! property.
//!
//! These read `assets/skins/`, which ships with the binary, so they live here
//! rather than beside the skin code: reading them from `sl-viewer-ui-core`
//! would mean that crate reaching outside its own directory, which widens its
//! commit-hook relevance to the whole repository (see
//! `book/src/tools/build-performance.md`).
//!
//! They check the *content* of the stylesheets, not the skin engine — a missing
//! theme file, or a `left:` that should have been `inset-inline-start:`, is a
//! defect in the CSS and only visible against the real files.

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use pretty_assertions::assert_ne;
    use sl_viewer_ui_core::skin::{SKINS, THEMES, scan_banned_properties};
    use sl_viewer_ui_core::skin_colors::COLOR_TOKENS;
    use sl_viewer_ui_core::skin_palette::PALETTE_CSS_PROPERTIES;

    /// A boxed error so tests can use `?` instead of `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The absolute path of the shipped skins directory.
    fn skins_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("skins")
    }

    /// The structural rules, which live with the crate that embeds them
    /// (`sl-viewer-ui-core/src/skins/`) rather than in this crate's assets: a
    /// shipped skin reaches them through the embedded fallback sheet, so there
    /// is exactly one copy and it is baked into the binary.
    ///
    /// That crate asserts the wiring *within* `common.css` itself
    /// (`skin_palette.rs`'s `every_palette_role_is_wired_and_has_a_fallback_value`);
    /// what is left here is the half only a shipped skin can answer.
    fn common_css() -> Result<String, TestError> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("sl-viewer-ui-core")
            .join("src")
            .join("skins")
            .join("common.css");
        Ok(fs_err::read_to_string(path)?)
    }

    /// Every shipped skin `.css` — the base of each skin plus every theme
    /// overlay — must be free of the banned physical box properties. This is the
    /// build-time enforcement of "no physical left/right in a skin".
    #[test]
    fn no_shipped_skin_uses_a_banned_property() -> Result<(), TestError> {
        let mut checked = 0_usize;
        for entry in walk_css(&skins_dir())? {
            let css = fs_err::read_to_string(&entry)?;
            let findings = scan_banned_properties(&css);
            assert!(
                findings.is_empty(),
                "{}: uses banned physical properties {findings:?}; write the logical name instead",
                entry.display()
            );
            checked = checked.saturating_add(1);
        }
        assert!(checked > 0, "no skin css files were found to check");
        Ok(())
    }

    /// Collect every `.css` file under a directory tree.
    fn walk_css(dir: &std::path::Path) -> Result<Vec<PathBuf>, TestError> {
        let mut out = Vec::new();
        for entry in fs_err::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                out.extend(walk_css(&path)?);
            } else if path.extension().is_some_and(|ext| ext == "css") {
                out.push(path);
            }
        }
        Ok(out)
    }

    /// The scanner flags a banned declaration but not a `var()` reference or a
    /// legitimate logical property with a similar name.
    /// Every shipped skin id has a base stylesheet on disk, and every declared
    /// theme overlay exists under its skin — so the switcher can never select a
    /// missing file.
    #[test]
    fn shipped_skins_and_themes_exist() -> Result<(), TestError> {
        for skin in SKINS {
            let base = skins_dir().join(skin).join("skin.css");
            assert!(base.is_file(), "missing skin base {}", base.display());
        }
        for (skin, theme) in THEMES {
            let overlay = skins_dir()
                .join(skin)
                .join("themes")
                .join(format!("{theme}.css"));
            assert!(overlay.is_file(), "missing theme {}", overlay.display());
        }
        Ok(())
    }

    /// Every shipped skin defines every palette token, so no skin silently
    /// falls back to another skin's colours.
    #[test]
    fn shipped_skins_define_every_palette_token() -> Result<(), TestError> {
        for skin in SKINS {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join("skins")
                .join(skin)
                .join("skin.css");
            let css = fs_err::read_to_string(&path)?;
            for def in COLOR_TOKENS {
                assert!(
                    css.contains(&format!("--{}:", def.css_var())),
                    "{} does not define --{}",
                    path.display(),
                    def.css_var()
                );
            }
        }
        Ok(())
    }

    /// Every chrome role the widget set paints from is wired in `common.css`
    /// and defined by every shipped skin.
    ///
    /// The role palette (`viewer-audit-skin-token-coverage`) is the half of the
    /// skin that reaches the Rust-painted widget states, and its failure mode
    /// is **silent**: a `-sk-color-*` declaration `common.css` never makes, or
    /// a `var(--role)` no skin defines, simply leaves that field at its
    /// built-in fallback, so the widget keeps the previous skin's colour and
    /// nothing is logged. Three links have to hold — the registered property,
    /// the `common.css` rule, and the token — and this checks the two the
    /// compiler cannot.
    #[test]
    fn every_palette_role_is_wired_and_defined() -> Result<(), TestError> {
        let common = common_css()?;
        let skins: Vec<(PathBuf, String)> = SKINS
            .iter()
            .map(|skin| {
                let path = skins_dir().join(skin).join("skin.css");
                let css = fs_err::read_to_string(&path)?;
                Ok::<_, TestError>((path, css))
            })
            .collect::<Result<_, _>>()?;
        for (property, _field) in PALETTE_CSS_PROPERTIES {
            let declaration = format!("{property}:");
            let line = common
                .lines()
                .map(str::trim)
                .find(|line| line.starts_with(&declaration))
                .ok_or_else(|| {
                    format!("common.css does not wire {property}, so no skin can reach that role")
                })?;
            let token = line
                .split_once("var(--")
                .and_then(|(_before, rest)| rest.split_once(')'))
                .map(|(token, _after)| token)
                .ok_or_else(|| {
                    format!("{property} must read a --role token, not the literal `{line}`")
                })?;
            for (path, css) in &skins {
                assert!(
                    css.contains(&format!("--{token}:")),
                    "{} does not define --{token}, which common.css reads for {property}",
                    path.display()
                );
            }
        }
        Ok(())
    }

    /// No skin may paint the map's tracking beacon in a colour it also gives an
    /// avatar dot.
    ///
    /// This is the bug the minimap palette was written to close
    /// (`viewer-minimap-avatar-dot-color`): the beacon a double-click teleport
    /// leaves at its destination is indistinguishable from somebody standing
    /// there once the two share a colour. Shape distinguishes them too — the
    /// beacon is a ring where a dot is a disc — but a skin is free to retune
    /// colours and not free to reshape the marks, so the constraint has to be
    /// checked here, against the real stylesheets.
    #[test]
    fn no_skin_gives_the_map_beacon_an_avatar_dot_colour() -> Result<(), TestError> {
        for skin in SKINS {
            let path = skins_dir().join(skin).join("skin.css");
            let css = fs_err::read_to_string(&path)?;
            let track = declared_value(&css, "minimap-track");
            assert!(
                track.is_some(),
                "{} does not define --minimap-track",
                path.display()
            );
            for dot in [
                "minimap-avatar",
                "minimap-avatar-friend",
                "minimap-avatar-muted",
                "minimap-avatar-self",
                "minimap-avatar-linden",
            ] {
                assert_ne!(
                    declared_value(&css, dot),
                    track,
                    "{}: --{dot} is the tracking beacon's colour",
                    path.display()
                );
            }
        }
        Ok(())
    }

    /// The value a stylesheet declares for one custom property, lowercased and
    /// trimmed. `None` when the property is not declared at all.
    ///
    /// Deliberately exact-matches `--<name>:` so `--minimap-avatar` does not
    /// also pick up `--minimap-avatar-friend`.
    fn declared_value(css: &str, name: &str) -> Option<String> {
        let needle = format!("--{name}:");
        css.lines().find_map(|line| {
            let rest = line.trim().strip_prefix(&needle)?;
            Some(rest.trim().trim_end_matches(';').trim().to_lowercase())
        })
    }
}
