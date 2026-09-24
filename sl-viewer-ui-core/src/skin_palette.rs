//! The skin's **chrome role palette** (`viewer-audit-skin-token-coverage`): the
//! resolved role colours, on the styled root, for the Rust code that paints
//! widget chrome per *state* and so cannot be driven by a CSS class.
//!
//! # Why a second mechanism at all
//!
//! A widget becomes skinnable by carrying a `.sk-*` [`ClassList`], and that is
//! still the preferred route: `bevy_flair` paints the node, and a skin or theme
//! switch (or a `--watch-skins` hot reload) repaints it with no code involved.
//! But a class can only paint what a *selector* can reach, and a large part of
//! the widget set is painted from Rust against state the CSS engine cannot see:
//!
//! - a floater title bar that is one colour while the floater has focus and
//!   another while it does not, recomputed as focus moves;
//! - a tab strip whose active tab, inactive tabs and disabled tabs are
//!   repainted whenever the selection changes;
//! - a table row that lights up while it is selected, in a virtualised list
//!   whose rows are recycled;
//! - a pie-menu wedge mesh, which is not a UI node at all.
//!
//! Those used to be `const … : Color = Color::srgb(…)` at the top of each
//! widget module — 643 such literals against 35 CSS classes when this was
//! audited, which is why switching skin restyled the menu bar and nothing else.
//! They now read their colours from here, so one palette drives both halves:
//! the classes in `common.css` and the Rust-painted states.
//!
//! # How it works
//!
//! [`SkinPalette`] is the same **shim-component** pattern as
//! [`crate::skin::SkinTextCaret`] and [`crate::skin::SkinChatBands`]: a flat,
//! reflectable component whose fields are registered as CSS properties
//! (`-sk-color-<role>`), written by a rule in `common.css` that assigns each
//! field the matching `var(--<role>)` token. That rule targets `:root`, so the
//! component lands on the [`UiRoot`] entity — one palette for the whole UI,
//! resolved once by the cascade, and re-resolved on every skin, theme or
//! locale change.
//!
//! A system reads it through the [`SkinColors`] system parameter:
//!
//! ```ignore
//! fn repaint_tabs(palette: SkinColors, mut tabs: Query<(&Tab, &mut BackgroundColor)>) {
//!     let palette = palette.get();
//!     for (tab, mut background) in &mut tabs {
//!         background.0 = if tab.active { palette.card_bg } else { palette.surface_bg };
//!     }
//! }
//! ```
//!
//! [`SkinColors::get`] hands back a plain `Copy` value, so a spawn helper takes
//! `palette: SkinPalette` by value rather than borrowing the world.
//!
//! # The unskinned fallback
//!
//! [`SkinPalette::default`] holds exactly the colours the widget modules used to
//! declare, so a world with no skin — a unit test, the gallery before its first
//! dress, a stylesheet that omits a role — looks the way it did before. That is
//! also why a role is only worth adding when some widget genuinely paints it:
//! an unused role is a value with nothing to fall back *to*.
//!
//! # Roles, not colours
//!
//! The field names are **roles** (`surface_bg`, `text_muted`, `selection_bg`),
//! never colour names, and several widgets deliberately share one: the tab
//! strip's active tab and a tab page are both `card_bg`, a disabled menu entry
//! and a disabled table cell are both `text_disabled`. Where the old constants
//! had drifted apart by a few hundredths — three copies of the primary label
//! colour, two of the menu surface — collapsing them onto one role is the point
//! of the exercise, not a regression.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
// `CssPropertyRegistry`, `RegisterComponentPropertiesExt` and
// `ReflectStructPropertyRefExt` all arrive through the prelude.
use bevy_flair::prelude::*;

use crate::ui::UiRoot;

/// Every chrome role the widget set paints from, resolved from the active skin.
///
/// Written by the `-sk-color-<role>` CSS properties (the `:root` rule in
/// `common.css`, whose values are the `--<role>` tokens each skin defines) onto
/// the [`UiRoot`], and read through [`SkinColors`].
///
/// See the module documentation for why this exists beside the `.sk-*` classes,
/// and [`SkinPalette::default`] for the unskinned fallback.
#[derive(Component, ComponentProperties, Reflect, Debug, Clone, Copy, PartialEq)]
#[properties(auto_insert_remove)]
#[reflect(Default)]
pub struct SkinPalette {
    // -- Surfaces ---------------------------------------------------------
    /// A framed surface: a floater body, a drop-down menu, a combo popover.
    pub surface_bg: Color,
    /// The frame around a [`Self::surface_bg`] surface, and the rule between
    /// two groups of entries in a menu.
    pub surface_border: Color,
    /// A background-only surface *inside* a framed one: a tab page, the active
    /// tab in a strip, a card.
    pub card_bg: Color,
    /// The scrim a floating layer sits on — the dock host behind docked
    /// floaters, a demo panel's backdrop.
    pub overlay_bg: Color,

    // -- Text -------------------------------------------------------------
    /// Primary body text: a label, a table cell, a menu entry, a tab caption.
    pub text_primary: Color,
    /// Secondary text: a caption above a control, a unit suffix, a hint, a
    /// column header, a resize grip.
    pub text_muted: Color,
    /// Text of a control whose action does not apply right now.
    pub text_disabled: Color,
    /// A heading inside a page — the one line that names the section.
    pub text_heading: Color,

    // -- Controls ---------------------------------------------------------
    /// A button's or combo's resting background.
    pub control_bg: Color,
    /// The background under the pointer: a hovered button, the highlighted
    /// menu entry, the hovered row of a popover.
    pub control_bg_hover: Color,
    /// A disabled control's background.
    pub control_bg_disabled: Color,
    /// A control's resting border.
    pub control_border: Color,
    /// A disabled control's border.
    pub control_border_disabled: Color,

    // -- Editable fields --------------------------------------------------
    /// The background of an editable text field, which is darker than a
    /// control so it reads as a well rather than a button.
    pub field_bg: Color,
    /// The colour of text being edited. Brighter than [`Self::text_primary`]:
    /// the field is where the eye goes.
    pub field_text: Color,

    // -- Emphasis ---------------------------------------------------------
    /// The accent: an active tab's frame, a radio's lit indicator, a drag
    /// grip, anything saying "this one".
    pub accent: Color,
    /// The background of a selected row, translucent so the row's own banding
    /// still reads through it.
    pub selection_bg: Color,
    /// The glyphs of a filter match inside an otherwise ordinary label.
    pub match_highlight: Color,

    // -- Structure --------------------------------------------------------
    /// A splitter or rule between two panes.
    pub divider: Color,
    /// The trough a scrollbar thumb or a slider handle runs in.
    pub track_bg: Color,
    /// A scrollbar thumb.
    pub scrollbar_thumb: Color,
    /// A slider handle or a trackball marker — brighter than a scrollbar
    /// thumb, because it is dragged deliberately rather than incidentally.
    pub slider_thumb: Color,

    // -- Floater chrome ---------------------------------------------------
    /// The title bar of the focused floater, laid *over* its body, so this is
    /// a translucent lightening rather than an opaque colour.
    pub title_bar_active: Color,
    /// The title text of an unfocused floater. The focused one is
    /// [`Self::text_primary`].
    pub title_text_inactive: Color,
    /// The background of a title-bar glyph button (close, minimise, tear-off)
    /// — the same translucent lightening as the title bar, fainter.
    pub glyph_button_bg: Color,

    // -- Pie menu ---------------------------------------------------------
    /// The pie menu's disc. Its own role rather than a surface: the pie is
    /// drawn as a mesh over the world, and is deliberately more translucent
    /// than a floater so what it acts on stays visible under it.
    pub pie_bg: Color,
    /// The spokes dividing the pie's wedges.
    pub pie_line: Color,
    /// The wedge under the pointer.
    pub pie_selected: Color,
    /// The caption of a wedge that opens a sub-pie rather than acting.
    pub pie_label_sub_pie: Color,
    /// The caption of a wedge that is present but unavailable. Its own role
    /// rather than [`Self::text_disabled`] because the pie fades a disabled
    /// entry *toward the disc* (the reference drops it to 0.3 alpha) instead
    /// of greying it: a flat grey on a translucent disc over the world reads
    /// as a different word, not as an unavailable one.
    pub pie_label_disabled: Color,
}

impl SkinPalette {
    /// The colours the widget modules declared before they read the skin —
    /// the visible fallback for an unskinned world (a unit test, the gallery
    /// before its first dress) and for a role a stylesheet omits.
    ///
    /// A `const`, not only a [`Default`], because a panel's *static* table
    /// spec is built in a `const` context and still has to name a role rather
    /// than a literal: `const SPEC: TableSpec = TableSpec { header_color:
    /// SkinPalette::FALLBACK.text_muted, … }`. That is what collapsed the
    /// twenty copies of the primary label colour (three of which had drifted)
    /// onto one value.
    pub const FALLBACK: Self = Self {
        surface_bg: Color::srgba(0.11, 0.12, 0.15, 0.95),
        surface_border: Color::srgb(0.30, 0.34, 0.42),
        card_bg: Color::srgb(0.19, 0.23, 0.31),
        overlay_bg: Color::srgba(0.06, 0.07, 0.10, 0.85),

        text_primary: Color::srgb(0.90, 0.92, 0.96),
        text_muted: Color::srgb(0.62, 0.66, 0.74),
        text_disabled: Color::srgb(0.45, 0.47, 0.52),
        text_heading: Color::srgb(0.70, 0.82, 1.0),

        control_bg: Color::srgb(0.16, 0.19, 0.25),
        control_bg_hover: Color::srgb(0.28, 0.34, 0.46),
        control_bg_disabled: Color::srgb(0.12, 0.12, 0.14),
        control_border: Color::srgb(0.40, 0.50, 0.62),
        control_border_disabled: Color::srgb(0.28, 0.28, 0.32),

        field_bg: Color::srgb(0.10, 0.12, 0.16),
        field_text: Color::WHITE,

        accent: Color::srgb(0.36, 0.72, 0.98),
        selection_bg: Color::srgba(0.24, 0.34, 0.52, 0.55),
        match_highlight: Color::srgb(0.98, 0.82, 0.40),

        divider: Color::srgb(0.34, 0.41, 0.53),
        track_bg: Color::srgb(0.12, 0.14, 0.18),
        scrollbar_thumb: Color::srgb(0.40, 0.48, 0.60),
        slider_thumb: Color::srgb(0.75, 0.78, 0.85),

        title_bar_active: Color::srgba(1.0, 1.0, 1.0, 0.10),
        title_text_inactive: Color::srgb(0.55, 0.58, 0.64),
        glyph_button_bg: Color::srgba(1.0, 1.0, 1.0, 0.06),

        pie_bg: Color::srgba(0.24, 0.24, 0.24, 0.8),
        pie_line: Color::srgba(0.0, 0.0, 0.0, 0.5),
        pie_selected: Color::srgba(0.95, 0.412, 0.173, 0.35),
        pie_label_sub_pie: Color::srgb(0.65, 0.86, 1.0),
        pie_label_disabled: Color::srgba(0.93, 0.95, 0.98, 0.22),
    };
}

impl Default for SkinPalette {
    /// [`SkinPalette::FALLBACK`] — the unskinned colours.
    fn default() -> Self {
        Self::FALLBACK
    }
}

/// Every palette CSS property and the [`SkinPalette`] field it writes.
///
/// One table so the registration, the `common.css` rule and the test that
/// checks the two agree all read from the same list. The property names carry
/// the `-sk-` vendor prefix the viewer's other custom properties use, so none
/// of them can ever collide with a real CSS property `bevy_flair` grows later.
pub const PALETTE_CSS_PROPERTIES: &[(&str, &str)] = &[
    ("-sk-color-surface-bg", "surface_bg"),
    ("-sk-color-surface-border", "surface_border"),
    ("-sk-color-card-bg", "card_bg"),
    ("-sk-color-overlay-bg", "overlay_bg"),
    ("-sk-color-text-primary", "text_primary"),
    ("-sk-color-text-muted", "text_muted"),
    ("-sk-color-text-disabled", "text_disabled"),
    ("-sk-color-text-heading", "text_heading"),
    ("-sk-color-control-bg", "control_bg"),
    ("-sk-color-control-bg-hover", "control_bg_hover"),
    ("-sk-color-control-bg-disabled", "control_bg_disabled"),
    ("-sk-color-control-border", "control_border"),
    (
        "-sk-color-control-border-disabled",
        "control_border_disabled",
    ),
    ("-sk-color-field-bg", "field_bg"),
    ("-sk-color-field-text", "field_text"),
    ("-sk-color-accent", "accent"),
    ("-sk-color-selection-bg", "selection_bg"),
    ("-sk-color-match-highlight", "match_highlight"),
    ("-sk-color-divider", "divider"),
    ("-sk-color-track-bg", "track_bg"),
    ("-sk-color-scrollbar-thumb", "scrollbar_thumb"),
    ("-sk-color-slider-thumb", "slider_thumb"),
    ("-sk-color-title-bar-active", "title_bar_active"),
    ("-sk-color-title-text-inactive", "title_text_inactive"),
    ("-sk-color-glyph-button-bg", "glyph_button_bg"),
    ("-sk-color-pie-bg", "pie_bg"),
    ("-sk-color-pie-line", "pie_line"),
    ("-sk-color-pie-selected", "pie_selected"),
    ("-sk-color-pie-label-sub-pie", "pie_label_sub_pie"),
    ("-sk-color-pie-label-disabled", "pie_label_disabled"),
];

/// Register the `-sk-color-<role>` CSS properties on the `bevy_flair`
/// registry, mapping each onto its [`SkinPalette`] field.
///
/// Called from the skin plugin's `build`, after `FlairPlugin` has stood the
/// registries up and before the CSS asset loader snapshots them at plugin
/// `finish`. A property that is never registered fails **silently** — the rule
/// simply parses to nothing — which is why the test below asserts the wiring.
pub fn register_palette_properties(app: &mut App) {
    app.register_component_properties::<SkinPalette>();
    let css = app.world().resource::<CssPropertyRegistry>();
    for (property, field) in PALETTE_CSS_PROPERTIES {
        css.register_property(*property, SkinPalette::property_field_ref(field));
    }
}

/// Read access to the active skin's [`SkinPalette`].
///
/// A system takes this instead of a `Res`, because the palette is a *component*
/// on the [`UiRoot`] (that is what lets the CSS cascade write it) rather than a
/// resource. [`Self::get`] hides the root lookup and the unskinned fallback, so
/// a caller never handles the "no root yet" case itself.
#[derive(SystemParam)]
#[expect(
    missing_debug_implementations,
    reason = "a `SystemParam` of a `Res` and a `Query`, which are not `Debug`"
)]
pub struct SkinColors<'w, 's> {
    /// The styled root the cascade resolves the palette onto.
    root: Option<Res<'w, UiRoot>>,
    /// Its palette, absent until the first skin is applied. Held as a [`Ref`]
    /// so [`SkinColors::is_changed`] can answer without a second query.
    palettes: Query<'w, 's, Ref<'static, SkinPalette>>,
}

impl SkinColors<'_, '_> {
    /// The active palette, or [`SkinPalette::default`] when there is no styled
    /// root yet or the active skin defines none of the roles.
    ///
    /// Cheap (one resource read and one component lookup) and `Copy`, so a
    /// system calls it once and passes the value into its spawn helpers.
    #[must_use]
    pub fn get(&self) -> SkinPalette {
        self.root
            .as_ref()
            .and_then(|root| self.palettes.get(root.0).ok())
            .map_or_else(SkinPalette::default, |palette| *palette)
    }

    /// Whether the palette was rewritten since this system last ran — a skin
    /// or theme switch, a `--watch-skins` hot reload, or the first dress.
    ///
    /// A repaint system that otherwise runs only on its own trigger (the
    /// active floater changing, a tab being picked) has to widen its guard
    /// with this, or its widgets keep the colours of the previous skin until
    /// something unrelated happens to move them.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.root
            .as_ref()
            .and_then(|root| self.palettes.get(root.0).ok())
            .is_some_and(|palette| palette.is_changed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Every `-sk-color-<role>` property is on the CSS registry after the
    /// properties are registered. An unregistered property is not a compile
    /// error but a skin rule that parses to nothing, so the wiring is asserted
    /// rather than assumed.
    #[test]
    fn every_palette_property_is_registered() {
        let mut app = App::new();
        // `FlairPlugin` stands up the registries this extends — in the viewer
        // it arrives with `ViewerSkinPlugin`, whose `build` calls the
        // registration below right after adding it.
        app.add_plugins((AssetPlugin::default(), FlairPlugin));
        register_palette_properties(&mut app);
        let css = app.world().resource::<CssPropertyRegistry>();
        for (property, _field) in PALETTE_CSS_PROPERTIES {
            assert!(
                css.get_property(property).is_some(),
                "the CSS property {property} is not registered, so a skin rule \
                 using it would silently parse to nothing"
            );
        }
    }

    /// The property table names a real field for every entry, and covers every
    /// field exactly once — the table is what `common.css` and the skin
    /// documentation are written against, so a field added without a property
    /// (or a property naming a field that was renamed) is a role no skin can
    /// reach.
    #[test]
    fn the_property_table_covers_every_role_once() {
        let palette = SkinPalette::default();
        let reflected: &dyn Struct = &palette;
        let fields: Vec<&str> = (0..reflected.field_len())
            .filter_map(|index| reflected.name_at(index))
            .collect();
        let mut mapped: Vec<&str> = PALETTE_CSS_PROPERTIES
            .iter()
            .map(|(_property, field)| *field)
            .collect();
        mapped.sort_unstable();
        let mut expected = fields.clone();
        expected.sort_unstable();
        assert_eq!(mapped, expected);
    }

    /// A boxed error so a test can use `?` instead of `unwrap` / `panic`,
    /// both of which the workspace lints forbid — in test code as much as in
    /// the library, since `--all-targets` is what the commit hook runs.
    type TestError = Box<dyn core::error::Error>;

    /// The structural rules, as the binary embeds them.
    ///
    /// Read through `include_str!` rather than the asset server: these checks
    /// are about what the *source* says, and embedding them here is what lets
    /// this crate assert its own skin wiring instead of a test in the viewer
    /// binary doing it across a directory boundary.
    const COMMON_CSS: &str = include_str!("skins/common.css");

    /// The always-present fallback sheet (`skin::FALLBACK_STYLESHEET`).
    const FALLBACK_CSS: &str = include_str!("skins/fallback.css");

    /// The `--role` token a `-sk-color-*` declaration in `common.css` reads,
    /// or `None` when the property is not wired there at all.
    fn token_for(property: &str) -> Option<&'static str> {
        let declaration = format!("{property}:");
        COMMON_CSS
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&declaration))?
            .split_once("var(--")
            .and_then(|(_before, rest)| rest.split_once(')'))
            .map(|(token, _after)| token)
    }

    /// The hex value `fallback.css` gives a token, as `Color`.
    fn fallback_token(token: &str) -> Option<Color> {
        let declaration = format!("--{token}:");
        let value = FALLBACK_CSS
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&declaration))?
            .split_once('#')?
            .1
            .split(';')
            .next()?;
        let byte = |index: usize| -> Option<f32> {
            let pair = value.get(index..index.checked_add(2)?)?;
            Some(f32::from(u8::from_str_radix(pair, 16).ok()?) / 255.0)
        };
        let alpha = if value.len() >= 8 { byte(6)? } else { 1.0 };
        Some(Color::srgba(byte(0)?, byte(2)?, byte(4)?, alpha))
    }

    /// Every role the widget set paints from is wired in `common.css` and given
    /// a value by the fallback sheet.
    ///
    /// Both halves fail **silently** in the running viewer: a `-sk-color-*`
    /// declaration `common.css` never makes leaves that field at its built-in
    /// constant, and a `var(--role)` nothing defines resolves to nothing at
    /// all. Since `fallback.css` is now imported by every shipped skin, a token
    /// it defines is the floor under every skin — so this is also what makes
    /// "a skin may omit a role" safe.
    #[test]
    fn every_palette_role_is_wired_and_has_a_fallback_value() -> Result<(), TestError> {
        for (property, _field) in PALETTE_CSS_PROPERTIES {
            let token = token_for(property)
                .ok_or_else(|| format!("common.css does not wire {property} to a --role token"))?;
            assert!(
                fallback_token(token).is_some(),
                "fallback.css does not define --{token}, which common.css reads \
                 for {property}, so an unskinned world resolves it to nothing"
            );
        }
        Ok(())
    }

    /// Nothing `common.css` reads is left undefined by the fallback sheet.
    ///
    /// Wider than the palette check above: the class rules consume tokens that
    /// no `SkinPalette` field backs (radii, the focus ring, the chat bands), and
    /// those have no Rust constant to fall back to — an undefined one is simply
    /// an unpainted widget.
    #[test]
    fn the_fallback_defines_every_token_common_css_reads() {
        let mut missing: Vec<&str> = Vec::new();
        let mut rest = COMMON_CSS;
        while let Some((_before, after)) = rest.split_once("var(--") {
            let Some((token, tail)) = after.split_once(')') else {
                break;
            };
            if !FALLBACK_CSS.contains(&format!("--{token}:")) && !missing.contains(&token) {
                missing.push(token);
            }
            rest = tail;
        }
        assert!(
            missing.is_empty(),
            "fallback.css leaves these tokens undefined: {missing:?}"
        );
    }

    /// The fallback sheet's values *are* [`SkinPalette::FALLBACK`].
    ///
    /// The sheet has to repeat them as hex — CSS cannot read a Rust constant —
    /// and a duplicated colour that nothing checks is exactly the drift this
    /// whole role vocabulary exists to end. The tolerance is one 8-bit step,
    /// which is all the hex round-trip can lose.
    #[test]
    fn fallback_tokens_match_rust() -> Result<(), TestError> {
        let palette = SkinPalette::FALLBACK;
        let reflected: &dyn Struct = &palette;
        for (property, field) in PALETTE_CSS_PROPERTIES {
            let token =
                token_for(property).ok_or_else(|| format!("{property} is not wired at all"))?;
            let css = fallback_token(token)
                .ok_or_else(|| format!("fallback.css does not define --{token}"))?;
            let rust = reflected
                .field(field)
                .and_then(|value| value.try_downcast_ref::<Color>())
                .copied()
                .ok_or_else(|| format!("SkinPalette has no Color field {field}"))?;
            let css = css.to_srgba();
            let rust = rust.to_srgba();
            for (channel, (from_css, from_rust)) in [
                ("red", (css.red, rust.red)),
                ("green", (css.green, rust.green)),
                ("blue", (css.blue, rust.blue)),
                ("alpha", (css.alpha, rust.alpha)),
            ] {
                assert!(
                    (from_css - from_rust).abs() <= 1.0 / 255.0,
                    "--{token} {channel} is {from_css} in fallback.css but \
                     {from_rust} in SkinPalette::FALLBACK.{field}"
                );
            }
        }
        Ok(())
    }

    /// The embedded token sheet obeys the physical-property ban too.
    ///
    /// The viewer binary's `shipped_skins` test scans the skins in its own
    /// `assets/`; this is the one that moved here. `common.css` is held to
    /// the same ban by `skin.rs`'s
    /// `common_css_writes_side_colours_only_from_bevel_tokens`, with the one
    /// exception the bevel policy grants it: a physical `left` / `right` in
    /// the *structural* rules would mirror wrongly in every skin at once
    /// rather than in one, but a bevel's side colours must not mirror at all.
    #[test]
    fn the_embedded_token_sheet_uses_no_banned_property() {
        let findings = crate::skin::scan_banned_properties(FALLBACK_CSS);
        assert!(
            findings.is_empty(),
            "fallback.css uses banned physical properties {findings:?}; \
             write the logical name instead"
        );
    }

    /// With no styled root in the world, the palette reads as the unskinned
    /// fallback rather than panicking or handing back black.
    #[test]
    fn an_unskinned_world_reads_the_fallback() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, |palette: SkinColors| {
            assert_eq!(palette.get(), SkinPalette::default());
        });
        app.update();
    }
}
