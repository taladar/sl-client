//! Internationalisation (`viewer-i18n-fluent-scaffold`): the string foundation
//! every UI-bearing panel is built on, so panels are authored translatable from
//! the first one rather than retrofitted at the hundredth.
//!
//! # Why this is sequenced ahead of the panels
//!
//! A panel that ships an English literal (`Text::new("Save changes")`) is a
//! panel that has to be found and rewritten before it can be translated, and the
//! reference viewer's twenty-year retrofit is the evidence for how expensive that
//! is. So the scaffold lands first and the rule is: a panel looks a string up by
//! **key**, and the bundle — not the call site — decides the words, the plural
//! branch, the gender, and even the punctuation the UI inserts around them.
//!
//! # What it wires
//!
//! - **[`ViewerI18nPlugin`]** integrates [`bevy_fluent`]: Project Fluent `.ftl`
//!   bundles loaded as Bevy assets from `assets/locales/<lang>/`, with runtime
//!   locale switching. The bundles are negotiated into a [`Localization`] lookup
//!   resource that falls back to English for any key a locale has not translated.
//! - **[`Translator`]** is the lookup API every panel uses. It takes **typed
//!   named arguments** ([`TransArgs`]) — a count as a number, a name as a string,
//!   a gender as a selector key — and formats them *inside* Fluent, so the
//!   `.ftl` `{ $count -> [one] … *[other] … }` selector resolves the plural for
//!   the active locale from CLDR rules. It never takes, and never returns to a
//!   caller to re-interpolate, a pre-formatted string.
//! - **[`UiLocale`]** is the active locale as a resource. It carries the locale's
//!   layout **[`direction`](UiLocale::direction)** (LTR / RTL), which drives the
//!   layout via [`crate::ui::UiDirection`], and the locale's **typographic
//!   conventions** resolved from the bundle — today the truncation
//!   [`ellipsis`](UiLocale::ellipsis) the tab widget appends to a clipped label,
//!   which Chinese and Japanese conventionally write as a centred `……` rather
//!   than the Latin `…`.
//!
//! # Why Fluent, not `LLTrans::getCountString`
//!
//! The reference viewer pluralises with a hardcoded `if`-ladder over three
//! languages, which is wrong for Polish — a language it ships. Fluent's plural
//! rules are per-locale and correct: the same `items-selected` authoring picks
//! `one` in English, `one`/`few`/`many`/`other` in Polish, and all six CLDR
//! categories in Arabic, with nothing in this module or the call site knowing the
//! difference. [`plural_selection_matches_cldr_rules`](tests) proves it.
//!
//! # Pseudolocalisation
//!
//! [`crate::ui_pseudoloc`] is folded in here as a pseudo-*locale*: with
//! [`UiLocale::pseudo`] set, every [`Translator`] lookup is post-processed by
//! [`pseudolocalise`], so the whole UI turns pseudo from one switch rather than
//! each call site opting in. The transform runs on the formatted result, after
//! the arguments are interpolated, so an expanded, accented, fenced string is
//! exactly what a real panel would have to survive.
//!
//! Locale detection / override is `viewer-i18n-locale-selection`; sending the
//! language to the grid is `viewer-i18n-agent-language`; chat machine-translation
//! is `viewer-i18n-chat-translation`. This task is the foundation all three build
//! on.

use bevy::asset::{LoadState, LoadedFolder};
use bevy::ecs::system::{IntoObserverSystem, SystemParam};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_fluent::{FluentPlugin, Locale, Localization, LocalizationBuilder};
use fluent::{FluentArgs, FluentValue};
use fluent_content::{Content as _, Request};
use sl_l10n::{CivilDateTime, DateTimeLength, DateTimeStyle, LocaleFormatters};
use unic_langid::{LanguageIdentifier, langid};

use crate::ui::{
    LogicalMargin, LogicalRect, UiDirection, UiPanelShown, UiRoot, UiScaffoldSystems, column, row,
};
use crate::ui_font::UiFont;
use crate::ui_pseudoloc::pseudolocalise;
use crate::ui_tab::TabEllipsisMarker;

/// The key the truncation ellipsis is stored under in every bundle. Read by
/// [`refresh_locale_ellipsis`] and applied to the tab widget by
/// [`apply_locale_ellipsis`].
const ELLIPSIS_KEY: &str = "ui-ellipsis";

/// The truncation ellipsis used before any bundle has loaded, or for a locale
/// that does not translate [`ELLIPSIS_KEY`] — the Latin single ellipsis, matching
/// the tab widget's own [`crate::ui_tab::DEFAULT_ELLIPSIS`].
const FALLBACK_ELLIPSIS: &str = "…";

/// The folder under the Bevy asset root the locale bundles are loaded from.
const LOCALES_FOLDER: &str = "locales";

/// The environment variable that seeds the initial [`UiLocale`], so a locale can
/// be selected before the locale selector (`viewer-i18n-locale-selection`)
/// exists. The value is a language tag (`en`, `ja`, `ar`, `pl`) or `pseudo`.
const UI_LOCALE_ENV: &str = "SL_VIEWER_UI_LOCALE";

/// The [`UI_LOCALE_ENV`] value that selects the pseudolocale.
const PSEUDO_LOCALE_VALUE: &str = "pseudo";

/// The i18n plugin: integrates [`bevy_fluent`], loads the locale bundles, and
/// exposes the [`Translator`] lookup and the [`UiLocale`] resource. See the
/// [module documentation](self).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ViewerI18nPlugin;

impl Plugin for ViewerI18nPlugin {
    /// Wire the Fluent asset pipeline, the locale resources, and the `F6` demo.
    fn build(&self, app: &mut App) {
        let choice = LocaleChoice::from_env();
        app.add_plugins(FluentPlugin)
            // The negotiation input `bevy_fluent` reads: the requested locale,
            // with English as the fallback for any untranslated key.
            .insert_resource(Locale::new(choice.language()).with_default(langid!("en")))
            .insert_resource(UiLocale::new(choice))
            // The lookup resource is empty until the bundle folder finishes
            // loading; initialised so [`Translator`]'s `Res<Localization>` never
            // reads a missing resource in the frames before then.
            .init_resource::<Localization>()
            .init_resource::<LocaleFormatting>()
            .insert_resource(DirectionOverride::from_env())
            .init_resource::<I18nDemoVisible>()
            .init_resource::<I18nDemoCount>()
            .init_resource::<I18nDemoGender>()
            .add_systems(Startup, load_locale_folder)
            .add_systems(Startup, setup_i18n_demo.after(UiScaffoldSystems::SpawnRoot))
            .add_systems(
                Update,
                (
                    // Rebuild the lookup when the folder loads or the locale
                    // changes, then propagate the locale's conventions.
                    maintain_localization,
                    refresh_locale_ellipsis.after(maintain_localization),
                    maintain_value_formatters,
                    sync_ui_direction,
                    apply_locale_ellipsis,
                    apply_translations,
                    toggle_i18n_demo,
                    apply_i18n_demo_visibility.after(toggle_i18n_demo),
                    update_i18n_demo_text,
                ),
            );
    }
}

/// A locale the viewer ships a bundle for, plus the pseudolocale — the fixed set
/// the switcher (and the `SL_VIEWER_UI_LOCALE` seed) chooses from until a full
/// locale selector lands.
///
/// An enum rather than a free [`LanguageIdentifier`] so the cycle is total and
/// the pseudolocale — which is not a language tag — has a first-class slot beside
/// the real ones.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LocaleChoice {
    /// English — the base locale and the fallback for every other bundle.
    #[default]
    English,
    /// Japanese — a CJK locale, for the centred-ellipsis convention and a
    /// plural-less language.
    Japanese,
    /// Arabic — a right-to-left locale, for the direction flip and six-way
    /// plural rules.
    Arabic,
    /// Polish — for the `one`/`few`/`many` plural rules the reference viewer
    /// gets wrong.
    Polish,
    /// The pseudolocale: English words run through [`pseudolocalise`] at lookup
    /// (see [`crate::ui_pseudoloc`]).
    Pseudo,
}

impl LocaleChoice {
    /// The Fluent language this choice looks strings up in. [`Self::Pseudo`]
    /// resolves to English and is transformed after the lookup, so it shares
    /// English's bundle.
    const fn language(self) -> LanguageIdentifier {
        match self {
            Self::English | Self::Pseudo => langid!("en"),
            Self::Japanese => langid!("ja"),
            Self::Arabic => langid!("ar"),
            Self::Polish => langid!("pl"),
        }
    }

    /// Whether lookups in this choice are pseudolocalised.
    const fn is_pseudo(self) -> bool {
        matches!(self, Self::Pseudo)
    }

    /// The next choice in the switcher's cycle.
    const fn next(self) -> Self {
        match self {
            Self::English => Self::Japanese,
            Self::Japanese => Self::Arabic,
            Self::Arabic => Self::Polish,
            Self::Polish => Self::Pseudo,
            Self::Pseudo => Self::English,
        }
    }

    /// The initial choice, seeded from [`UI_LOCALE_ENV`]: a language tag, the
    /// literal `pseudo`, or (unset / unrecognised) English.
    fn from_env() -> Self {
        match std::env::var(UI_LOCALE_ENV) {
            Ok(value) => Self::parse(&value),
            Err(_) => Self::default(),
        }
    }

    /// [`Self::from_env`]'s decision, split out so it is testable without reading
    /// the process environment.
    fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        if trimmed.eq_ignore_ascii_case(PSEUDO_LOCALE_VALUE) {
            return Self::Pseudo;
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "ja" => Self::Japanese,
            "ar" => Self::Arabic,
            "pl" => Self::Polish,
            _ => Self::English,
        }
    }
}

/// The active UI locale as a resource, plus the conventions derived from it.
///
/// This is the single place the rest of the UI reads "what language are we in,
/// and which way does it lay out". [`ViewerI18nPlugin`] keeps it in step with
/// the Fluent [`Locale`]: [`maintain_localization`] rebuilds the lookup when it
/// changes, and [`sync_ui_direction`] flips the layout.
#[derive(Resource, Debug, Clone)]
pub(crate) struct UiLocale {
    /// Which shipped locale is active.
    pub(crate) choice: LocaleChoice,
    /// The active language tag.
    pub(crate) lang: LanguageIdentifier,
    /// The layout direction of [`lang`](Self::lang) — LTR for Latin / CJK /
    /// Cyrillic, RTL for Arabic / Hebrew.
    pub(crate) direction: UiDirection,
    /// The truncation ellipsis for this locale (the [`ELLIPSIS_KEY`] string),
    /// resolved from the bundle once it loads; [`FALLBACK_ELLIPSIS`] until then.
    pub(crate) ellipsis: String,
    /// Whether every lookup is pseudolocalised (the [`LocaleChoice::Pseudo`]
    /// case).
    pub(crate) pseudo: bool,
}

impl UiLocale {
    /// A locale resource for `choice`, with the ellipsis at its fallback until a
    /// bundle loads.
    fn new(choice: LocaleChoice) -> Self {
        let lang = choice.language();
        Self {
            direction: direction_of(&lang),
            lang,
            choice,
            ellipsis: FALLBACK_ELLIPSIS.to_owned(),
            pseudo: choice.is_pseudo(),
        }
    }
}

/// The layout direction of a language.
///
/// Right-to-left when the tag's script is a right-to-left script, or (no explicit
/// script) when the language's default script is right-to-left. The tables are
/// the handful the viewer can plausibly ship and mean to render bidi-correct;
/// anything else is left-to-right. Kept a pure function so it is unit-tested
/// against real tags without a running app.
fn direction_of(lang: &LanguageIdentifier) -> UiDirection {
    /// Script subtags written right-to-left.
    const RTL_SCRIPTS: &[&str] = &["Arab", "Hebr", "Syrc", "Thaa", "Nkoo", "Rohg", "Yezi"];
    /// Languages whose default script is right-to-left, for tags that omit the
    /// script subtag (`ar` rather than `ar-Arab`).
    const RTL_LANGS: &[&str] = &[
        "ar", "he", "fa", "ur", "ps", "sd", "ug", "yi", "dv", "ku", "ckb", "syr",
    ];
    if let Some(script) = lang.script {
        if RTL_SCRIPTS.contains(&script.as_str()) {
            return UiDirection::Rtl;
        }
        // An explicit non-RTL script settles it, even for an RTL-default
        // language (e.g. a hypothetical `ku-Latn`).
        return UiDirection::Ltr;
    }
    if RTL_LANGS.contains(&lang.language.as_str()) {
        UiDirection::Rtl
    } else {
        UiDirection::Ltr
    }
}

/// A manual layout-direction override, seeded from `SL_VIEWER_UI_DIRECTION`.
///
/// The knob predates this module (it forced RTL to exercise the mirroring before
/// a locale existed) and stays honoured: when set, it wins over the locale's own
/// direction, so an operator can still force RTL on an LTR locale. When unset,
/// the locale drives the layout, which is the point of the scaffold.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct DirectionOverride(Option<UiDirection>);

impl DirectionOverride {
    /// Read the override from the environment once, at plugin build.
    fn from_env() -> Self {
        // Reuse the same parse the scaffold's own knob uses, so the two never
        // disagree on what `rtl` means.
        Self(UiDirection::rtl_override_from_env())
    }
}

/// The handle to the loaded locale folder, kept alive so
/// [`maintain_localization`] can rebuild the lookup when the locale switches at
/// runtime (a fresh negotiation over the same loaded bundles).
#[derive(Resource, Debug)]
struct LocaleFolder(Handle<LoadedFolder>);

/// Startup: begin loading every bundle under [`LOCALES_FOLDER`].
fn load_locale_folder(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load_folder(LOCALES_FOLDER);
    commands.insert_resource(LocaleFolder(handle));
}

/// Rebuild the [`Localization`] lookup once the bundle folder has loaded, and
/// again whenever the [`Locale`] changes (a runtime locale switch re-negotiates
/// the fallback chain over the already-loaded bundles).
fn maintain_localization(
    mut commands: Commands,
    builder: LocalizationBuilder,
    asset_server: Res<AssetServer>,
    folder: Option<Res<LocaleFolder>>,
    locale: Res<Locale>,
    mut built: Local<bool>,
) {
    let Some(folder) = folder else {
        return;
    };
    if !matches!(
        asset_server.get_load_state(&folder.0),
        Some(LoadState::Loaded)
    ) {
        return;
    }
    // Build on the first load, then only when the requested locale changes.
    if *built && !locale.is_changed() {
        return;
    }
    commands.insert_resource(builder.build(&folder.0));
    *built = true;
}

/// Copy the active locale's truncation ellipsis out of the freshly-built bundle
/// onto [`UiLocale`], so [`apply_locale_ellipsis`] can drive the tab widget from
/// it. Runs after [`maintain_localization`] so it reads the current bundle.
fn refresh_locale_ellipsis(localization: Res<Localization>, mut locale: ResMut<UiLocale>) {
    if !localization.is_changed() {
        return;
    }
    let resolved = localization
        .content(ELLIPSIS_KEY)
        .unwrap_or_else(|| FALLBACK_ELLIPSIS.to_owned());
    if locale.ellipsis != resolved {
        locale.ellipsis = resolved;
    }
}

/// The active locale's value formatters (`viewer-i18n-number-datetime-formats`).
///
/// The CLDR/ICU-backed number, currency and date/time formatters the
/// [`Translator`] exposes so a panel writes a grouped balance / coordinate /
/// timestamp for the active locale rather than a bare `to_string()`. `None`
/// until [`maintain_value_formatters`] first builds it (and never expected to
/// stay `None`, since the locale tag always parses); the [`Translator`] helpers
/// fall back to an un-grouped render while it is, mirroring the string lookup's
/// key-fallback.
#[derive(Resource, Debug, Default)]
struct LocaleFormatting {
    /// The formatters for [`UiLocale::lang`], rebuilt on a locale change.
    formatters: Option<LocaleFormatters>,
}

/// (Re)build the [`LocaleFormatting`] formatters whenever the active locale
/// changes, so the number / date conventions follow the language switch.
fn maintain_value_formatters(locale: Res<UiLocale>, mut formatting: ResMut<LocaleFormatting>) {
    if !locale.is_changed() {
        return;
    }
    // `unic_langid` and `icu_locale_core` agree on BCP-47, so the tag string is
    // the bridge between the Fluent locale and the ICU formatters.
    let tag = locale.lang.to_string();
    match LocaleFormatters::from_tag(&tag) {
        Ok(formatters) => formatting.formatters = Some(formatters),
        Err(error) => warn!(%tag, %error, "could not build locale value formatters"),
    }
}

/// Drive [`UiDirection`] from the active locale, so a right-to-left locale
/// mirrors the whole layout with no per-panel special-casing.
///
/// A [`DirectionOverride`] (the `SL_VIEWER_UI_DIRECTION` knob) still wins, so the
/// pre-existing manual RTL affordance keeps working; otherwise the locale owns
/// the layout direction, which is what this scaffold introduces.
fn sync_ui_direction(
    locale: Res<UiLocale>,
    over: Res<DirectionOverride>,
    mut direction: ResMut<UiDirection>,
) {
    if !locale.is_changed() {
        return;
    }
    let target = over.0.unwrap_or(locale.direction);
    if *direction != target {
        *direction = target;
    }
}

/// Rewrite every tab widget's truncation-ellipsis marker to the active locale's
/// [`ellipsis`](UiLocale::ellipsis), so a CJK locale gets its centred `……` and a
/// locale switch updates markers already on screen.
///
/// Runs on a locale change (updating every marker) and for markers spawned since
/// (a strip created after the switch), so both a running switch and a fresh strip
/// land on the right glyph.
fn apply_locale_ellipsis(
    locale: Res<UiLocale>,
    fresh: Query<Entity, Added<TabEllipsisMarker>>,
    mut markers: Query<&mut Text, With<TabEllipsisMarker>>,
) {
    if locale.is_changed() {
        for mut text in &mut markers {
            if text.0 != locale.ellipsis {
                text.0.clone_from(&locale.ellipsis);
            }
        }
        return;
    }
    for entity in &fresh {
        if let Ok(mut text) = markers.get_mut(entity)
            && text.0 != locale.ellipsis
        {
            text.0.clone_from(&locale.ellipsis);
        }
    }
}

/// A typed named-argument set for a [`Translator`] lookup.
///
/// The point of the whole API: an argument goes into Fluent *typed* — a count as
/// a number (so the plural selector and `NUMBER()` see a number), a name or a
/// gender key as a string — never flattened to text first. Building it is a
/// chain of typed setters; the caller never touches [`FluentValue`].
///
/// Values are stored owned (`'static`) so the built args outlive the borrow the
/// lookup takes and a caller can build them from a temporary.
#[derive(Debug, Default)]
pub(crate) struct TransArgs(FluentArgs<'static>);

impl TransArgs {
    /// An empty argument set.
    pub(crate) fn new() -> Self {
        Self(FluentArgs::new())
    }

    /// Set an integer argument — the plural / `NUMBER()` case.
    #[must_use]
    pub(crate) fn int(mut self, key: &'static str, value: i64) -> Self {
        self.0.set(key, FluentValue::from(value));
        self
    }

    /// Set a string argument — a name inserted verbatim, or a selector key such
    /// as a gender. Stored owned so it outlives the lookup borrow.
    ///
    /// A floating-point setter is deliberately *not* here: a bare `f64` would
    /// render with no locale grouping or decimal mark, which is the deferred
    /// `viewer-i18n-number-datetime-formats` task's job. Integer counts (which
    /// drive plural selection, and must stay integers so CLDR does not treat
    /// them as having a visible fraction) are the only numeric case the scaffold
    /// carries.
    #[must_use]
    pub(crate) fn text(mut self, key: &'static str, value: &str) -> Self {
        self.0.set(key, FluentValue::from(value.to_owned()));
        self
    }

    /// The underlying Fluent arguments, for the lookup to format with.
    const fn as_fluent(&self) -> &FluentArgs<'static> {
        &self.0
    }
}

/// The string-lookup API every panel uses.
///
/// A [`SystemParam`] bundling the [`Localization`] lookup and the active
/// [`UiLocale`]. A panel resolves a key with [`get`](Self::get), or a key with
/// typed arguments with [`format`](Self::format); both fall back to English for
/// an untranslated key and to the key itself for one no bundle defines (the
/// Fluent convention, so a missing key is visible rather than blank), and both
/// apply the pseudolocale transform when it is active.
#[derive(SystemParam)]
pub(crate) struct Translator<'w> {
    /// The negotiated bundle chain the strings are looked up in.
    localization: Res<'w, Localization>,
    /// The active locale — read for the pseudolocale flag.
    locale: Res<'w, UiLocale>,
    /// The active locale's value formatters (numbers / currency / date-time).
    formatting: Res<'w, LocaleFormatting>,
}

impl Translator<'_> {
    /// Resolve a key with no arguments.
    pub(crate) fn get(&self, key: &str) -> String {
        self.finish(key, self.localization.content(key))
    }

    /// Resolve a key, interpolating typed named arguments inside Fluent — so the
    /// `.ftl` plural / gender selectors resolve against the real values.
    pub(crate) fn format(&self, key: &str, args: &TransArgs) -> String {
        let content = self
            .localization
            .content(Request::new(key).args(args.as_fluent()));
        self.finish(key, content)
    }

    /// Fall back to the key for a miss, then pseudolocalise if active.
    fn finish(&self, key: &str, content: Option<String>) -> String {
        let raw = content.unwrap_or_else(|| key.to_owned());
        if self.locale.pseudo {
            pseudolocalise(&raw)
        } else {
            raw
        }
    }

    /// The active formatters, or `None` before they have built (the first
    /// frame). The value helpers fall back to a plain render while `None`.
    ///
    /// Value formatting is *not* pseudolocalised: a number or a date is not
    /// prose, so mangling its digits would only obscure whether the grouping is
    /// right — the axis the pseudolocale tests is string length, which the
    /// surrounding label already carries.
    fn formatters(&self) -> Option<&LocaleFormatters> {
        self.formatting.formatters.as_ref()
    }

    /// Format a signed integer — an object / item count, an L$-less amount — with
    /// the locale's grouping separator (`1,234,567`).
    pub(crate) fn integer(&self, value: i64) -> String {
        self.formatters()
            .map_or_else(|| value.to_string(), |formatters| formatters.integer(value))
    }

    /// Format a float — a coordinate, a scale, a distance — to `fraction_digits`
    /// places with the locale's grouping separator and decimal mark.
    pub(crate) fn decimal(&self, value: f64, fraction_digits: u8) -> String {
        self.formatters()
            .and_then(|formatters| formatters.decimal(value, fraction_digits).ok())
            .unwrap_or_else(|| {
                let digits = usize::from(fraction_digits);
                format!("{value:.digits$}")
            })
    }

    /// Format a Linden-dollar balance (`L$1,234`), the amount grouped for the
    /// locale.
    pub(crate) fn currency_l(&self, value: i64) -> String {
        self.formatters().map_or_else(
            || format!("L${value}"),
            |formatters| formatters.currency_l(value),
        )
    }

    /// Format a civil date / time for the locale at the given style and length.
    /// Falls back to an ISO-ish rendering before the formatters build or if a
    /// component is out of range.
    pub(crate) fn datetime(
        &self,
        when: CivilDateTime,
        style: DateTimeStyle,
        length: DateTimeLength,
    ) -> String {
        self.formatters()
            .and_then(|formatters| formatters.datetime(when, style, length).ok())
            .unwrap_or_else(|| {
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}",
                    when.year, when.month, when.day, when.hour, when.minute
                )
            })
    }

    /// Parse a number a user typed in the active locale's conventions back into
    /// an `f64`, for a localized float input field. `None` if it does not parse
    /// (or before the formatters build).
    pub(crate) fn parse_number(&self, input: &str) -> Option<f64> {
        self.formatters()
            .and_then(|formatters| formatters.parse_number(input).ok())
    }
}

/// A static (argument-free) UI label bound to a Fluent key.
///
/// Put it on any entity that also has a [`Text`], and [`apply_translations`]
/// keeps that text resolved from the key: it fills in once the bundle loads,
/// re-resolves on a locale switch, and localizes a freshly-spawned label the
/// frame it appears. So a panel spawns `Text::default()` + `Translated::new(key)`
/// and never touches an English literal — the mechanism the widgets a panel does
/// not own use too ([`crate::ui_tab`]'s tab labels, the floater title).
///
/// The key is owned (`String`) rather than `&'static str` so a widget can bind a
/// label it was handed at runtime (a tab strip's `&str` labels). For a label
/// that needs **arguments** (a plural, a name), format eagerly with
/// [`Translator::format`] and write the result — this component is only for the
/// argument-free case, which is the common one and the only one that can be
/// re-resolved from the key alone.
#[derive(Component, Debug, Clone)]
pub(crate) struct Translated {
    /// The Fluent key this label resolves to.
    key: String,
}

impl Translated {
    /// Bind a label to `key`.
    pub(crate) fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

/// Keep every [`Translated`] label's [`Text`] resolved from its key.
///
/// Re-resolves **all** labels when the bundle changes (it just loaded, or a
/// locale switch rebuilt it) or the locale changes (the pseudolocale flip, which
/// does not rebuild the bundle); otherwise only labels added since last frame, so
/// a panel spawned after the bundle loaded still localizes without a full sweep.
fn apply_translations(
    translator: Translator,
    localization: Res<Localization>,
    locale: Res<UiLocale>,
    fresh: Query<Entity, Added<Translated>>,
    mut labels: Query<(&Translated, &mut Text)>,
) {
    if localization.is_changed() || locale.is_changed() {
        for (label, mut text) in &mut labels {
            let next = translator.get(&label.key);
            if text.0 != next {
                text.0 = next;
            }
        }
        return;
    }
    for entity in &fresh {
        if let Ok((label, mut text)) = labels.get_mut(entity) {
            let next = translator.get(&label.key);
            if text.0 != next {
                text.0 = next;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The `F6` i18n demo panel.
//
// A live consumer that makes the whole scaffold load-bearing rather than dead
// code: it switches locale at runtime (proving the asset rebuild, the direction
// flip and the ellipsis refresh), and it drives `Translator::format` with a
// live count and gender (proving typed arguments and per-locale plural / gender
// selection). Modelled on the `F5` scaffold demo (`crate::ui`).
// ---------------------------------------------------------------------------

/// The key that toggles the demo panel.
const I18N_DEMO_TOGGLE_KEY: KeyCode = KeyCode::F6;

/// The counts the demo cycles through, chosen to land in a different CLDR plural
/// category: `1` is `one`, `2` is `few` in Polish / `two` in Arabic, `5` is
/// `many` in Polish, `0` is `many` in Polish / `zero` in Arabic.
const DEMO_COUNTS: [i64; 5] = [1, 2, 5, 22, 0];

/// The name inserted into the `greeting` string — never translated, only placed.
const DEMO_NAME: &str = "Ada";

/// The panel's inset from the top-leading corner, clear of the `F3`/`F4`/`F5`
/// overlays.
const DEMO_PANEL_MARGIN: f32 = 130.0;

/// The demo panel's translucent backdrop, matching the scaffold demos'.
const DEMO_PANEL_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);

/// The demo's heading / label colour.
const DEMO_TEXT_COLOR: Color = Color::srgb(0.82, 0.87, 0.94);

/// A demo button's background.
const DEMO_BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A demo button's border.
const DEMO_BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The demo's text size, in logical pixels.
const DEMO_FONT_SIZE: f32 = 15.0;

/// Whether the demo panel is currently shown.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct I18nDemoVisible(bool);

/// Which of [`DEMO_COUNTS`] the demo's plural line is currently showing.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct I18nDemoCount(usize);

impl I18nDemoCount {
    /// The current count value. The index is kept in range by [`next`](Self::next),
    /// so the lookup is always in bounds; the fallback is unreachable defensive
    /// code that keeps this off the indexing / panic path.
    fn value(self) -> i64 {
        DEMO_COUNTS.get(self.0).copied().unwrap_or(0)
    }

    /// Advance to the next count, wrapping without arithmetic that can overflow
    /// or index out of range.
    const fn next(&mut self) {
        let advanced = self.0.saturating_add(1);
        self.0 = if advanced < DEMO_COUNTS.len() {
            advanced
        } else {
            0
        };
    }
}

/// Which gender the demo's `friend-status` selector is currently showing.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
enum I18nDemoGender {
    /// Selector key `female`.
    #[default]
    Female,
    /// Selector key `male`.
    Male,
    /// Selector key `other` (also the `*` default branch).
    Other,
}

impl I18nDemoGender {
    /// The Fluent selector key this gender passes as the `$gender` argument.
    const fn key(self) -> &'static str {
        match self {
            Self::Female => "female",
            Self::Male => "male",
            Self::Other => "other",
        }
    }

    /// The next gender in the cycle.
    const fn next(self) -> Self {
        match self {
            Self::Female => Self::Male,
            Self::Male => Self::Other,
            Self::Other => Self::Female,
        }
    }
}

/// A marker on the demo panel's root node.
#[derive(Component, Debug, Clone, Copy)]
struct I18nDemoRoot;

/// Which live line of the demo a `Text` node is, so one system rewrites them all.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum I18nDemoLine {
    /// The panel title (`i18n-demo-title`).
    Title,
    /// The locale-switch button, naming the active locale in its own script.
    LocaleButton,
    /// The layout direction and the resolved truncation ellipsis.
    Conventions,
    /// The `greeting` string with the inserted name.
    Greeting,
    /// The `items-selected` plural line at the live count.
    Plural,
    /// The count-cycle button.
    CountButton,
    /// The `friend-status` gender line.
    Gender,
    /// The gender-cycle button.
    GenderButton,
    /// A grouped integer (an object / item count) — the number-formatting demo.
    Number,
    /// A grouped L$ balance — the currency-formatting demo.
    Currency,
    /// A fractional value (a coordinate) — the decimal-mark demo.
    Coordinate,
    /// A formatted date + time — the date/time-formatting demo.
    DateTime,
    /// The locale-formatted coordinate parsed back to its canonical value — the
    /// input-parsing demo (a localized float field reads `128,75` as `128.75`).
    ParseRoundTrip,
}

/// The count the demo's grouping line formats, big enough to show the locale's
/// grouping separator (and, in Polish, the minimum-grouping rule).
const DEMO_NUMBER: i64 = 1_234_567;

/// The L$ balance the demo's currency line formats.
const DEMO_BALANCE: i64 = 2_048_576;

/// A coordinate-style fractional value the demo formats to two places, to show
/// the locale's decimal mark (a dot in English, a comma in German / Polish).
const DEMO_COORDINATE: f64 = 128.75;

/// The date-time the demo formats, showing the locale's field order and digits.
const DEMO_WHEN: CivilDateTime = CivilDateTime {
    year: 2026,
    month: 7,
    day: 19,
    hour: 14,
    minute: 30,
    second: 5,
};

/// Startup: spawn the demo panel under [`UiRoot`], hidden until `F6`.
fn setup_i18n_demo(mut commands: Commands, root: Res<UiRoot>) {
    let panel = commands
        .spawn((
            Node {
                display: Display::None,
                padding: UiRect::all(Val::Px(12.0)),
                max_width: Val::Px(460.0),
                ..column(Val::Px(6.0))
            },
            // Asymmetric and logical, so switching to an RTL locale visibly walks
            // the panel to the other side of the window.
            LogicalMargin(LogicalRect {
                inline_start: Val::Px(DEMO_PANEL_MARGIN),
                block_start: Val::Px(DEMO_PANEL_MARGIN),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(DEMO_PANEL_BACKGROUND),
            UiPanelShown(false),
            I18nDemoRoot,
            ChildOf(root.0),
        ))
        .id();
    demo_line(&mut commands, panel, I18nDemoLine::Title);
    demo_line(&mut commands, panel, I18nDemoLine::Conventions);
    demo_line(&mut commands, panel, I18nDemoLine::Greeting);
    demo_line(&mut commands, panel, I18nDemoLine::Plural);
    demo_line(&mut commands, panel, I18nDemoLine::Gender);
    demo_line(&mut commands, panel, I18nDemoLine::Number);
    demo_line(&mut commands, panel, I18nDemoLine::Currency);
    demo_line(&mut commands, panel, I18nDemoLine::Coordinate);
    demo_line(&mut commands, panel, I18nDemoLine::DateTime);
    demo_line(&mut commands, panel, I18nDemoLine::ParseRoundTrip);
    // The buttons flow in text order, so they swap ends under RTL with no code
    // here saying so; they wrap rather than overflow when a label runs long.
    let buttons = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(6.0),
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    demo_button(
        &mut commands,
        buttons,
        1,
        I18nDemoLine::LocaleButton,
        cycle_locale,
    );
    demo_button(
        &mut commands,
        buttons,
        2,
        I18nDemoLine::CountButton,
        cycle_count,
    );
    demo_button(
        &mut commands,
        buttons,
        3,
        I18nDemoLine::GenderButton,
        cycle_gender,
    );
}

/// Spawn one of the demo's live text lines under `parent`.
fn demo_line(commands: &mut Commands, parent: Entity, line: I18nDemoLine) {
    commands.spawn((
        Text::default(),
        UiFont::Sans.at(DEMO_FONT_SIZE),
        TextColor(DEMO_TEXT_COLOR),
        line,
        ChildOf(parent),
    ));
}

/// Spawn one focusable demo button, its live label carried on an [`I18nDemoLine`]
/// child, wired to `on_activate`.
fn demo_button<M>(
    commands: &mut Commands,
    parent: Entity,
    tab_index: i32,
    line: I18nDemoLine,
    on_activate: impl IntoObserverSystem<Activate, (), M>,
) {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(DEMO_BUTTON_BORDER),
            BackgroundColor(DEMO_BUTTON_BACKGROUND),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            UiFont::Sans.at(DEMO_FONT_SIZE),
            TextColor(Color::WHITE),
            line,
        ))
        .observe(on_activate);
}

/// Observer: advance the active locale one step around [`LocaleChoice::next`].
fn cycle_locale(_activate: On<Activate>, mut locale: ResMut<UiLocale>, mut fluent: ResMut<Locale>) {
    let choice = locale.choice.next();
    let lang = choice.language();
    locale.choice = choice;
    locale.direction = direction_of(&lang);
    locale.pseudo = choice.is_pseudo();
    locale.lang = lang.clone();
    // Drive the Fluent negotiation input too, so [`maintain_localization`]
    // rebuilds the lookup and [`refresh_locale_ellipsis`] re-reads the ellipsis.
    *fluent = Locale::new(lang).with_default(langid!("en"));
}

/// Observer: advance the demo's plural count.
fn cycle_count(_activate: On<Activate>, mut count: ResMut<I18nDemoCount>) {
    count.next();
}

/// Observer: advance the demo's gender selector.
fn cycle_gender(_activate: On<Activate>, mut gender: ResMut<I18nDemoGender>) {
    *gender = gender.next();
}

/// Toggle the demo panel on `F6`.
fn toggle_i18n_demo(keyboard: Res<ButtonInput<KeyCode>>, mut visible: ResMut<I18nDemoVisible>) {
    if keyboard.just_pressed(I18N_DEMO_TOGGLE_KEY) {
        visible.0 = !visible.0;
    }
}

/// Drive the demo panel's [`UiPanelShown`] from [`I18nDemoVisible`].
fn apply_i18n_demo_visibility(
    visible: Res<I18nDemoVisible>,
    mut panels: Query<&mut UiPanelShown, With<I18nDemoRoot>>,
) {
    if !visible.is_changed() {
        return;
    }
    for mut shown in &mut panels {
        if shown.0 != visible.0 {
            shown.0 = visible.0;
        }
    }
}

/// Rewrite the demo's live lines each frame from the [`Translator`] and the
/// current demo state — the one place the scaffold's whole API is exercised
/// against a running app.
fn update_i18n_demo_text(
    translator: Translator,
    locale: Res<UiLocale>,
    count: Res<I18nDemoCount>,
    gender: Res<I18nDemoGender>,
    mut lines: Query<(&I18nDemoLine, &mut Text)>,
) {
    for (line, mut text) in &mut lines {
        let next = match line {
            I18nDemoLine::Title => translator.get("i18n-demo-title"),
            I18nDemoLine::LocaleButton => {
                format!("Locale (F6): {}", translator.get("language-name"))
            }
            I18nDemoLine::Conventions => {
                let dir = if locale.direction.is_rtl() {
                    "RTL"
                } else {
                    "LTR"
                };
                format!("Direction: {dir}   Ellipsis: {}", locale.ellipsis)
            }
            I18nDemoLine::Greeting => {
                translator.format("greeting", &TransArgs::new().text("name", DEMO_NAME))
            }
            I18nDemoLine::Plural => translator.format(
                "items-selected",
                &TransArgs::new().int("count", count.value()),
            ),
            I18nDemoLine::CountButton => format!("Count: {}", count.value()),
            I18nDemoLine::Gender => translator.format(
                "friend-status",
                &TransArgs::new().text("gender", gender.key()),
            ),
            I18nDemoLine::GenderButton => format!("Gender: {}", gender.key()),
            I18nDemoLine::Number => format!("Number: {}", translator.integer(DEMO_NUMBER)),
            I18nDemoLine::Currency => {
                format!("Balance: {}", translator.currency_l(DEMO_BALANCE))
            }
            I18nDemoLine::Coordinate => {
                format!("Position X: {}", translator.decimal(DEMO_COORDINATE, 2))
            }
            I18nDemoLine::DateTime => format!(
                "When: {}",
                translator.datetime(DEMO_WHEN, DateTimeStyle::DateTime, DateTimeLength::Medium)
            ),
            I18nDemoLine::ParseRoundTrip => {
                // Format the coordinate for the locale, then parse that localized
                // text back — the input-field path. The parsed value is the same
                // canonical number regardless of locale.
                let localized = translator.decimal(DEMO_COORDINATE, 2);
                let parsed = translator
                    .parse_number(&localized)
                    .map_or_else(|| "?".to_owned(), |value| format!("{value}"));
                format!("Parse: {localized} → {parsed}")
            }
        };
        if text.0 != next {
            text.0 = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LocaleChoice, TransArgs, direction_of};
    use fluent::{FluentArgs, FluentBundle, FluentResource, FluentValue};
    use fluent_content::{Content as _, Request};
    use pretty_assertions::assert_eq;
    use unic_langid::{LanguageIdentifier, langid};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The English bundle source, embedded so the plural / argument behaviour is
    /// testable without the async asset load. The runtime loads the same file as
    /// a Bevy asset.
    const EN_FTL: &str = include_str!("../assets/locales/en/main.ftl");

    /// The Polish bundle source — the plural case the reference viewer gets
    /// wrong, embedded for the CLDR-rules test.
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

    /// Right-to-left tags resolve RTL; everything the viewer ships or is likely
    /// to, LTR. An explicit non-RTL script overrides an RTL-default language.
    #[test]
    fn direction_follows_script_and_language() {
        assert_eq!(direction_of(&langid!("en")), super::UiDirection::Ltr);
        assert_eq!(direction_of(&langid!("ja")), super::UiDirection::Ltr);
        assert_eq!(direction_of(&langid!("pl")), super::UiDirection::Ltr);
        assert_eq!(direction_of(&langid!("ar")), super::UiDirection::Rtl);
        assert_eq!(direction_of(&langid!("he")), super::UiDirection::Rtl);
        assert_eq!(direction_of(&langid!("fa-IR")), super::UiDirection::Rtl);
        // An explicit Latin script on an RTL-default language is LTR.
        assert_eq!(direction_of(&langid!("ku-Latn")), super::UiDirection::Ltr);
        // An explicit Arabic script on any language is RTL.
        assert_eq!(direction_of(&langid!("az-Arab")), super::UiDirection::Rtl);
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

    /// The typed argument builder stores what it is given without flattening it
    /// to text — an integer stays a number, so the plural selector sees a number.
    #[test]
    fn trans_args_keeps_a_number_a_number() -> Result<(), TestError> {
        // An integer and a string, so both typed setters are covered and a count
        // stays a number (the plural selector needs it typed, not flattened to
        // text — and an integer, so CLDR does not see a visible fraction).
        let args = TransArgs::new().int("count", 3).text("name", "Ada");
        let count = args
            .as_fluent()
            .get("count")
            .ok_or_else(|| -> TestError { "no count".into() })?;
        assert!(
            matches!(count, FluentValue::Number(number) if (number.value - 3.0).abs() < f64::EPSILON),
            "count must stay a number: {count:?}"
        );
        let name = args
            .as_fluent()
            .get("name")
            .ok_or_else(|| -> TestError { "no name".into() })?;
        assert!(
            matches!(name, FluentValue::String(value) if value.as_ref() == "Ada"),
            "name must be a string: {name:?}"
        );
        Ok(())
    }

    /// The env seed maps tags (and `pseudo`) to choices, case-insensitively, and
    /// falls back to English for anything unrecognised.
    #[test]
    fn locale_choice_parses_the_env_seed() {
        assert_eq!(LocaleChoice::parse("ja"), LocaleChoice::Japanese);
        assert_eq!(LocaleChoice::parse("AR"), LocaleChoice::Arabic);
        assert_eq!(LocaleChoice::parse(" pl "), LocaleChoice::Polish);
        assert_eq!(LocaleChoice::parse("Pseudo"), LocaleChoice::Pseudo);
        assert_eq!(LocaleChoice::parse("klingon"), LocaleChoice::English);
    }

    /// The locale cycle is total and returns to its start, so the switcher can
    /// loop without a bounds check.
    #[test]
    fn locale_choice_cycles() {
        let mut choice = LocaleChoice::English;
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push(choice);
            choice = choice.next();
        }
        assert_eq!(choice, LocaleChoice::English, "cycle returns to start");
        assert_eq!(seen.len(), 5, "five distinct choices before wrapping");
    }
}
