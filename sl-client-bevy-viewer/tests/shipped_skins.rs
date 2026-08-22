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

    use sl_viewer_ui_core::skin::{SKINS, THEMES, scan_banned_properties};
    use sl_viewer_ui_core::skin_colors::COLOR_TOKENS;

    /// A boxed error so tests can use `?` instead of `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The absolute path of the shipped skins directory.
    fn skins_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("skins")
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
}
