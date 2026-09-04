//! The viewer's UI vocabulary: the pieces every panel is built from, and the
//! two systems that decide how they look and what they say.
//!
//! This is the layer below the widgets. Nothing here knows what a floater or a
//! tab is — it knows about the UI root, the font stack, a logical (not
//! physical) box model that mirrors under a right-to-left locale, the skin that
//! colours it, and the Fluent bundles that fill it with text.
//!
//! - [`ui`] — the scaffold: the root entity, the logical box model, panel
//!   visibility, and the direction resolution that makes `margin-inline-start`
//!   mean the right edge in Arabic.
//! - [`ui_font`] — the font stack, including the bundled faces. The colour
//!   emoji font is bundled because the system one is usually COLRv1, which
//!   swash cannot rasterise; the rest are bundled so text renders identically
//!   on a host with no fonts installed.
//! - [`ui_text`], [`virtual_list`] — text nodes and a windowed list.
//! - [`ui_ellipsis`] — the trailing `…` a clipped value is marked with, and the
//!   one rule that decides when it shows.
//! - [`ui_element`] — the vocabulary a gallery entry is written in
//!   ([`ui_element::UiElement`], [`ui_element::ElementCx`]) plus the generic
//!   spawners. The registry *of* elements lives in the binary crate, because it
//!   names three dozen feature modules.
//! - [`i18n`], [`ui_pseudoloc`] — Fluent lookup and the pseudo-localization
//!   that makes an untranslated string obvious.
//! - [`skin`], [`skin_colors`] — the CSS skin system and the palette bridge
//!   that feeds a skin's colours to the settings store as defaults.
//! - [`ui_sounds`] — the UI's own sound effects.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `ui_font::UiFont` and `ui_text::UiText`. That only became a lint \
              when these items turned `pub` for the crate split; renaming them \
              would churn every call site in the viewer to satisfy a style rule \
              this codebase does not follow"
)]

pub mod i18n;
pub mod skin;
pub mod skin_colors;
pub mod ui;
pub mod ui_element;
pub mod ui_ellipsis;
pub mod ui_font;
pub mod ui_pseudoloc;
pub mod ui_sounds;
pub mod ui_text;
pub mod virtual_list;
