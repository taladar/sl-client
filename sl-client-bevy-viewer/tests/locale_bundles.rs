//! The shipped Fluent bundles resolve the plural and argument behaviour the
//! viewer relies on.
//!
//! These read `assets/locales/*/main.ftl`, which ship with the binary, so the
//! test lives here rather than with the i18n code: embedding them from
//! `sl-viewer-ui-core` would mean that crate reaching outside its own directory
//! with `include_str!`, which widens its commit-hook relevance to the whole
//! repository (see `book/src/tools/build-performance.md`).
//!
//! What they pin is the *content* of the bundles — that Polish really has
//! `few`/`many` where English has only `one`/`other`, and that Arabic reaches
//! `zero` and `two`. That is a property of the translations, not of the lookup
//! code, and it is what the reference viewer's three-language if-ladder cannot
//! express.

#[cfg(test)]
mod test {
    use fluent::{FluentArgs, FluentBundle, FluentResource, FluentValue};
    use fluent_content::{Content as _, Request};
    use pretty_assertions::assert_eq;
    use unic_langid::{LanguageIdentifier, langid};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The English bundle source. The runtime loads the same file as a Bevy
    /// asset; embedding it here tests the shipped content directly.
    const EN_FTL: &str = include_str!("../assets/locales/en/main.ftl");

    /// The Polish bundle source — the plural case the reference viewer gets
    /// wrong.
    const PL_FTL: &str = include_str!("../assets/locales/pl/main.ftl");

    /// The Arabic bundle source — six plural categories.
    const AR_FTL: &str = include_str!("../assets/locales/ar/main.ftl");

    /// Build a non-isolating Fluent bundle from one `.ftl` source, so lookups
    /// return clean strings (isolation marks would fail exact comparison).
    fn bundle(
        lang: LanguageIdentifier,
        source: &str,
    ) -> Result<FluentBundle<FluentResource>, TestError> {
        let resource = FluentResource::try_new(source.to_owned())
            .map_err(|(_, errors)| format!("parse: {errors:?}"))?;
        let mut bundle = FluentBundle::new(vec![lang]);
        bundle.set_use_isolating(false);
        bundle
            .add_resource(resource)
            .map_err(|errors| format!("add_resource: {errors:?}"))?;
        Ok(bundle)
    }

    /// Format a key with an integer argument against a bundle.
    fn count_line(source: &str, lang: LanguageIdentifier, value: i64) -> Result<String, TestError> {
        let bundle = bundle(lang, source)?;
        let mut args = FluentArgs::new();
        args.set("count", FluentValue::from(value));
        bundle
            .content(Request::new("items-selected").args(&args))
            .ok_or_else(|| "no items-selected".into())
    }

    /// English pluralisation: `one` at 1, `other` everywhere else, with the count
    /// interpolated by its typed argument.
    #[test]
    fn english_plural_and_argument() -> Result<(), TestError> {
        assert_eq!(count_line(EN_FTL, langid!("en"), 1)?, "1 item selected");
        assert_eq!(count_line(EN_FTL, langid!("en"), 5)?, "5 items selected");
        Ok(())
    }

    /// The load-bearing claim: Polish plural categories, which the reference
    /// viewer's three-language if-ladder cannot express, resolve from CLDR rules.
    /// 1 is `one`, 2-4 is `few`, 5+ (and 0) is `many`.
    #[test]
    fn plural_selection_matches_cldr_rules() -> Result<(), TestError> {
        assert_eq!(
            count_line(PL_FTL, langid!("pl"), 1)?,
            "Zaznaczono 1 element"
        );
        assert_eq!(
            count_line(PL_FTL, langid!("pl"), 2)?,
            "Zaznaczono 2 elementy"
        );
        assert_eq!(
            count_line(PL_FTL, langid!("pl"), 5)?,
            "Zaznaczono 5 elementów"
        );
        // 22 is `few` in Polish (unlike a naive "> 4 is many"): the CLDR rule
        // keys on the last digit, which a hardcoded ladder gets wrong.
        assert_eq!(
            count_line(PL_FTL, langid!("pl"), 22)?,
            "Zaznaczono 22 elementy"
        );
        Ok(())
    }

    /// Arabic reaches plural categories no European language has (`zero`, `two`),
    /// proving the selector is genuinely per-locale.
    #[test]
    fn arabic_reaches_zero_and_two_categories() -> Result<(), TestError> {
        assert_eq!(
            count_line(AR_FTL, langid!("ar"), 0)?,
            "لم يتم تحديد أي عنصر"
        );
        assert_eq!(count_line(AR_FTL, langid!("ar"), 2)?, "تم تحديد عنصرين");
        Ok(())
    }

    /// A gender selector driven by a typed string argument picks the right branch
    /// and falls through to the default for an unknown key.
    #[test]
    fn gender_selector_picks_the_branch() -> Result<(), TestError> {
        let bundle = bundle(langid!("en"), EN_FTL)?;
        for (key, expected) in [
            ("female", "She is online"),
            ("male", "He is online"),
            ("other", "They are online"),
            ("nonbinary", "They are online"),
        ] {
            let mut args = FluentArgs::new();
            args.set("gender", FluentValue::from(key));
            let got = bundle
                .content(Request::new("friend-status").args(&args))
                .ok_or_else(|| -> TestError { "no friend-status".into() })?;
            assert_eq!(got, expected, "gender {key}");
        }
        Ok(())
    }
}
