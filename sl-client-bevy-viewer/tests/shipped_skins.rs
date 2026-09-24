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

    /// **Every token `common.css` reads, every shipped skin defines.**
    ///
    /// The other half of the palette guard above, and the one that covers the
    /// tokens a *class* rule names rather than the `-sk-color-*` roles: the
    /// checkbox's `--check-*`, the radio's `--radio-*`, every surface and text
    /// colour. Its failure mode is the same silent one — a rule falling back to
    /// bevy_flair's default for an undefined `var()`, which is a colour no skin
    /// chose — and the omission is easiest to make in exactly the case that
    /// prompted this: a widget growing new tokens, with three sheets to add
    /// them to.
    ///
    /// The **fallback** sheet is checked too, because a skin is allowed to
    /// define only what it changes and that one is what everything else rests
    /// on.
    #[test]
    fn every_token_common_css_reads_is_defined_by_every_skin() -> Result<(), TestError> {
        let common = common_css()?;
        let mut used: Vec<&str> = common
            .split("var(--")
            .skip(1)
            .filter_map(|tail| tail.split_once(')').map(|(name, _rest)| name))
            .collect();
        used.sort_unstable();
        used.dedup();
        assert!(
            used.len() > 20,
            "only {} tokens found — the parse, not the skins, is what broke",
            used.len()
        );

        let fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("sl-viewer-ui-core")
            .join("src")
            .join("skins")
            .join("fallback.css");
        let mut sheets: Vec<PathBuf> = vec![fallback];
        sheets.extend(
            SKINS
                .iter()
                .map(|skin| skins_dir().join(skin).join("skin.css")),
        );

        let mut missing = Vec::new();
        for sheet in sheets {
            let css = fs_err::read_to_string(&sheet)?;
            for token in &used {
                if !css.contains(&format!("--{token}:")) {
                    missing.push(format!("{}: --{token}", sheet.display()));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "a rule reads a token the sheet never defines, so it paints \
             bevy_flair's default instead of a chosen colour: {missing:#?}"
        );
        Ok(())
    }

    /// **The skin chapter names every token and every class `common.css`
    /// uses, and no token it no longer reads.**
    ///
    /// `book/src/authoring/skins.md` is what a third-party skin author works
    /// from, and its tables say they are the whole vocabulary. A token they
    /// omit is one such a skin never defines, which fails the silent way
    /// [`every_token_common_css_reads_is_defined_by_every_skin`] guards against
    /// for the shipped skins only: the rule paints bevy_flair's default. The
    /// chapter fell two thirds behind before anything noticed
    /// (`viewer-skin-book-token-table-behind`), so a one-off catch-up is not
    /// the fix; this is.
    ///
    /// Only the **leading cell of a `Token` or `Class` table** counts as
    /// documenting a name — one mentioned in prose, or in another row's
    /// description, has not been given a row. The reverse direction (a row for
    /// a name the sheet stopped using) skips the per-glyph `--` modifier
    /// classes, which Rust stamps and no rule in the sheet names
    /// (`.sk-parcel-icon--voice`).
    ///
    /// The chapter is read at run time, never embedded, so this crate's
    /// commit-hook relevance stays its own directory (see
    /// `book/src/tools/build-performance.md`). The price is that an edit to the
    /// chapter alone does not re-run this test; the next change to the viewer
    /// or to `common.css` does.
    #[test]
    fn the_skin_chapter_names_every_token_and_class_common_css_uses() -> Result<(), TestError> {
        let common = without_comments(&common_css()?);
        let chapter = fs_err::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("book")
                .join("src")
                .join("authoring")
                .join("skins.md"),
        )?;
        let rows = vocabulary_cells(&chapter);

        let tokens = css_names(&common, "var(--", "--");
        let classes = css_names(&common, ".sk-", ".sk-");
        assert!(
            tokens.len() > 20 && classes.len() > 20,
            "only {} tokens and {} classes found — the parse, not the chapter, is what broke",
            tokens.len(),
            classes.len()
        );

        let undocumented: Vec<&String> = tokens
            .iter()
            .chain(&classes)
            .filter(|name| !rows.iter().any(|row| mentions(row, name)))
            .collect();
        assert!(
            undocumented.is_empty(),
            "common.css uses these and no table in book/src/authoring/skins.md names \
             them, so a skin written from the chapter never defines them: {undocumented:#?}"
        );

        let documented_tokens = rows
            .iter()
            .flat_map(|row| css_names(row, "`--", "--"))
            .collect::<std::collections::BTreeSet<_>>();
        let documented_classes = rows
            .iter()
            .flat_map(|row| css_names(row, "`.sk-", ".sk-"))
            .filter(|class| !class.contains("--"))
            .collect::<std::collections::BTreeSet<_>>();
        let stale: Vec<&String> = documented_tokens
            .difference(&tokens)
            .chain(documented_classes.difference(&classes))
            .collect();
        assert!(
            stale.is_empty(),
            "book/src/authoring/skins.md documents these, but common.css no longer \
             uses them: {stale:#?}"
        );
        Ok(())
    }

    /// Every name in `text` that follows `marker`, spelled with `prefix`
    /// (which is the tail of `marker`) and running to the first character
    /// that cannot continue a CSS identifier.
    ///
    /// On the chapter's side the marker includes the opening backtick, so only
    /// a name written as code counts — a table's `| --- |` rule is not a token.
    fn css_names(text: &str, marker: &str, prefix: &str) -> std::collections::BTreeSet<String> {
        text.match_indices(marker)
            .filter_map(|(at, _marker)| {
                let start = at.saturating_add(marker.len()).checked_sub(prefix.len())?;
                let tail = text.get(start..)?;
                let end = tail
                    .char_indices()
                    .skip(prefix.len())
                    .find(|&(_index, ch)| !is_ident_char(ch))
                    .map_or(tail.len(), |(index, _ch)| index);
                let name = tail.get(..end)?;
                (name.len() > prefix.len()).then(|| name.to_owned())
            })
            .collect()
    }

    /// The leading cell of every body row of every table in `markdown` whose
    /// first column is headed `Token` or `Class`.
    fn vocabulary_cells(markdown: &str) -> Vec<&str> {
        let mut cells = Vec::new();
        let mut in_vocabulary_table = false;
        let mut previous_was_row = false;
        for line in markdown.lines() {
            let Some(row) = line.trim().strip_prefix('|') else {
                previous_was_row = false;
                continue;
            };
            let first = row.split('|').next().unwrap_or_default().trim();
            if !previous_was_row {
                in_vocabulary_table = first == "Token" || first == "Class";
            } else if in_vocabulary_table && !first.starts_with("---") {
                cells.push(first);
            }
            previous_was_row = true;
        }
        cells
    }

    /// Whether `row` names `name` whole — `--list-row` is not named by a row
    /// that only mentions `--list-row-bg`.
    fn mentions(row: &str, name: &str) -> bool {
        row.match_indices(name).any(|(at, _name)| {
            row.get(at.saturating_add(name.len())..)
                .and_then(|rest| rest.chars().next())
                .is_none_or(|next| !is_ident_char(next))
        })
    }

    /// Whether `ch` can continue a CSS custom property or class name.
    const fn is_ident_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
    }

    /// `css` with its `/* … */` comments removed, so a class a comment only
    /// mentions — a retired one, or a skin author's example — is not taken
    /// for one the sheet uses.
    fn without_comments(css: &str) -> String {
        let mut out = String::with_capacity(css.len());
        let mut rest = css;
        while let Some((code, after)) = rest.split_once("/*") {
            out.push_str(code);
            rest = after.split_once("*/").map_or("", |(_comment, tail)| tail);
        }
        out.push_str(rest);
        out
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
