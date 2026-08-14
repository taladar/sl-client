//! The skin-colour bridge (`viewer-preferences-colors-skins-tab`): the active
//! skin's user-tunable colour palette, fed from CSS into the settings store.
//!
//! Each tunable colour — chat text by source, the name-tag palette, the
//! keyword-alert highlight — is a CSS custom property in every skin's
//! `:root {}` (hex values only). [`bevy_flair`] resolves those onto the styled
//! [`UiRoot`] entity as [`StyleVars`]; [`apply_skin_color_defaults`] reads them
//! whenever they change and installs each as its setting's **declared default**
//! ([`ViewerSettings::set_default`]). Per-account user overrides then sit above
//! that in the store, so:
//!
//! - a consumer reads one effective value (`get_color3`) and never cares
//!   whether it came from the user or the skin;
//! - "reset to default" is literally dropping the account override — the skin
//!   value shows through;
//! - switching skin or theme (and `--watch-skins` hot reload) live-updates
//!   every colour the user has not overridden, while overrides stay put;
//! - only genuine overrides are persisted, the reference viewer's
//!   save-only-if-different `colors.xml` behaviour.
//!
//! This is the counterpart of the reference's `LLUIColorTable` (per-skin
//! `colors.xml` defaults under user `colors.xml` overrides), with the settings
//! store as the single resolution point.
//!
//! The bridge reads in `Update`, one frame after `bevy_flair` computes the
//! vars in `PostUpdate` — a deliberate, harmless lag (colours are cosmetic and
//! the skin rarely changes).

use bevy::prelude::*;
use bevy_flair::style::components::StyleVars;
use bevy_flair::style::{VarOrToken, VarToken, VarTokens};
use sl_settings::SettingValue;
use tracing::warn;

use crate::settings::ViewerSettings;
use crate::ui::UiRoot;

/// The settings section the colour palette is grouped under in the persisted
/// TOML.
const SECTION: &[&str] = &["colors"];

/// Chat-overlay / transcript colour of the user's own lines (reference
/// `UserChatColor`).
pub(crate) const SETTING_CHAT_SELF: &str = "ChatColorSelf";

/// Chat colour of other avatars' lines (reference `AgentChatColor`).
pub(crate) const SETTING_CHAT_OTHERS: &str = "ChatColorOthers";

/// Chat colour of object chat (reference `ObjectChatColor`).
pub(crate) const SETTING_CHAT_OBJECTS: &str = "ChatColorObjects";

/// Colour of instant-message / group-chat lines (reference `AgentIMColor`).
pub(crate) const SETTING_CHAT_IM: &str = "ChatColorIm";

/// Colour of system / viewer-notice chat lines (reference `SystemChatColor`).
pub(crate) const SETTING_CHAT_SYSTEM: &str = "ChatColorSystem";

/// The keyword-alert highlight colour. Registered with the palette now; the
/// chat keyword-alerts feature (`viewer-chat-keyword-alerts`) is its consumer.
pub(crate) const SETTING_KEYWORD_ALERT: &str = "KeywordAlertColor";

/// Base name-tag colour: legacy names and matching display names (reference
/// `NameTagLegacy` / `NameTagMatch`).
pub(crate) const SETTING_NAME_TAG_DEFAULT: &str = "NameTagColorDefault";

/// The own avatar's name-tag colour (reference `NameTagSelf`).
pub(crate) const SETTING_NAME_TAG_SELF: &str = "NameTagColorSelf";

/// A friend's name-tag colour — the friend highlight (reference
/// `NameTagFriend`).
pub(crate) const SETTING_NAME_TAG_FRIEND: &str = "NameTagColorFriend";

/// A muted avatar's name-tag colour (reference `NameTagMuted`).
pub(crate) const SETTING_NAME_TAG_MUTED: &str = "NameTagColorMuted";

/// A Linden / grid-staff name-tag colour (reference `NameTagLinden`).
pub(crate) const SETTING_NAME_TAG_LINDEN: &str = "NameTagColorLinden";

/// Name-tag colour of a custom (mismatching) display name (reference
/// `NameTagMismatch`).
pub(crate) const SETTING_NAME_TAG_MISMATCH: &str = "NameTagColorMismatch";

/// Distance-band tag colour inside whisper range (reference
/// `NameTagWhisperDistanceColor`).
pub(crate) const SETTING_NAME_TAG_DISTANCE_WHISPER: &str = "NameTagDistanceColorWhisper";

/// Distance-band tag colour inside normal chat range (reference
/// `NameTagChatDistanceColor`).
pub(crate) const SETTING_NAME_TAG_DISTANCE_CHAT: &str = "NameTagDistanceColorChat";

/// Distance-band tag colour inside shout range (reference
/// `NameTagShoutDistanceColor`).
pub(crate) const SETTING_NAME_TAG_DISTANCE_SHOUT: &str = "NameTagDistanceColorShout";

/// Distance-band tag colour beyond shout range (reference
/// `NameTagBeyondShoutDistanceColor`).
pub(crate) const SETTING_NAME_TAG_DISTANCE_BEYOND: &str = "NameTagDistanceColorBeyond";

/// One user-tunable colour: its setting name, the skin CSS custom property
/// that supplies its default, and the built-in fallback used until (or in
/// place of) a skin value.
pub(crate) struct ColorTokenDef {
    /// The settings-store name (see the `SETTING_*` consts).
    pub(crate) setting: &'static str,
    /// The CSS custom-property name, **without** the `--` prefix — the form
    /// [`StyleVars`] keys use.
    css_var: &'static str,
    /// The sRGB fallback, used when no skin supplies the token (and before the
    /// first styled frame). Matches the pre-tab hardcoded values.
    fallback: [f32; 3],
    /// The declaration comment, written above the persisted override.
    comment: &'static str,
}

/// Every user-tunable skin colour, in the order the preferences tab lists
/// them.
pub(crate) const COLOR_TOKENS: &[ColorTokenDef] = &[
    ColorTokenDef {
        setting: SETTING_CHAT_SELF,
        css_var: "chat-self",
        fallback: [1.0, 1.0, 1.0],
        comment: "Chat colour of my own lines (RGB override of the skin's --chat-self)",
    },
    ColorTokenDef {
        setting: SETTING_CHAT_OTHERS,
        css_var: "chat-others",
        fallback: [1.0, 1.0, 1.0],
        comment: "Chat colour of other avatars' lines (skin --chat-others)",
    },
    ColorTokenDef {
        setting: SETTING_CHAT_OBJECTS,
        css_var: "chat-objects",
        fallback: [0.75, 0.75, 0.75],
        comment: "Chat colour of object chat (skin --chat-objects)",
    },
    ColorTokenDef {
        setting: SETTING_CHAT_IM,
        css_var: "chat-im",
        fallback: [0.66, 0.78, 0.92],
        comment: "Colour of instant-message and group-chat lines (skin --chat-im)",
    },
    ColorTokenDef {
        setting: SETTING_CHAT_SYSTEM,
        css_var: "chat-system",
        fallback: [1.0, 1.0, 1.0],
        comment: "Colour of system and viewer-notice chat lines (skin --chat-system)",
    },
    ColorTokenDef {
        setting: SETTING_KEYWORD_ALERT,
        css_var: "keyword-alert",
        fallback: [1.0, 0.78, 0.25],
        comment: "Chat keyword-alert highlight colour (skin --keyword-alert)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_DEFAULT,
        css_var: "name-tag-default",
        fallback: [1.0, 1.0, 1.0],
        comment: "Base name-tag colour (skin --name-tag-default)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_SELF,
        css_var: "name-tag-self",
        fallback: [1.0, 1.0, 1.0],
        comment: "My own name-tag colour (skin --name-tag-self)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_FRIEND,
        css_var: "name-tag-friend",
        fallback: [0.75, 0.92, 0.49],
        comment: "Friends' name-tag colour, the friend highlight (skin --name-tag-friend)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_MUTED,
        css_var: "name-tag-muted",
        fallback: [0.4, 0.4, 0.4],
        comment: "Muted avatars' name-tag colour (skin --name-tag-muted)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_LINDEN,
        css_var: "name-tag-linden",
        fallback: [0.0, 0.5, 1.0],
        comment: "Grid staff (Linden) name-tag colour (skin --name-tag-linden)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_MISMATCH,
        css_var: "name-tag-mismatch",
        fallback: [0.9, 0.9, 0.9],
        comment: "Custom-display-name name-tag colour (skin --name-tag-mismatch)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_DISTANCE_WHISPER,
        css_var: "name-tag-distance-whisper",
        fallback: [0.0, 1.0, 0.0],
        comment: "Distance-band tag colour inside whisper range (skin --name-tag-distance-whisper)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_DISTANCE_CHAT,
        css_var: "name-tag-distance-chat",
        fallback: [0.0, 1.0, 0.0],
        comment: "Distance-band tag colour inside chat range (skin --name-tag-distance-chat)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_DISTANCE_SHOUT,
        css_var: "name-tag-distance-shout",
        fallback: [1.0, 1.0, 0.0],
        comment: "Distance-band tag colour inside shout range (skin --name-tag-distance-shout)",
    },
    ColorTokenDef {
        setting: SETTING_NAME_TAG_DISTANCE_BEYOND,
        css_var: "name-tag-distance-beyond",
        fallback: [1.0, 0.0, 0.0],
        comment: "Distance-band tag colour beyond shout range (skin --name-tag-distance-beyond)",
    },
];

/// Register every palette setting with its built-in fallback default. The skin
/// supplies the real default a frame after it is applied
/// ([`apply_skin_color_defaults`]).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    for def in COLOR_TOKENS {
        settings.register_in(
            SECTION,
            def.setting,
            SettingValue::Color3(def.fallback),
            def.comment,
        );
    }
}

/// The effective colour of a palette setting: the store's resolved value
/// (account override → skin default → fallback), or the table fallback when no
/// store is available (the gallery, early startup, tests).
pub(crate) fn setting_color(settings: Option<&ViewerSettings>, setting_name: &str) -> Color {
    if let Some(settings) = settings
        && let Ok(rgb) = settings.store().get_color3(setting_name)
    {
        return color_from_rgb(rgb);
    }
    let fallback = COLOR_TOKENS
        .iter()
        .find(|def| def.setting == setting_name)
        .map_or([1.0, 1.0, 1.0], |def| def.fallback);
    color_from_rgb(fallback)
}

/// An opaque [`Color`] from a stored sRGB triple.
const fn color_from_rgb(rgb: [f32; 3]) -> Color {
    let [red, green, blue] = rgb;
    Color::srgb(red, green, blue)
}

/// Feed the active skin's palette tokens into the settings store as declared
/// defaults, whenever the styled [`UiRoot`]'s resolved vars change (initial
/// dress, skin/theme switch, stylesheet hot reload).
///
/// A skin that omits a token simply leaves the previous default standing —
/// third-party skins may define a subset. A token that is not a single hex
/// value is reported and skipped, never guessed at.
fn apply_skin_color_defaults(
    root: Option<Res<UiRoot>>,
    vars: Query<Ref<StyleVars>>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let Some(root) = root else {
        return;
    };
    let Ok(vars) = vars.get(root.0) else {
        return;
    };
    if !vars.is_changed() {
        return;
    }
    let Some(mut settings) = settings else {
        return;
    };
    for def in COLOR_TOKENS {
        let Some(tokens) = vars.get(def.css_var) else {
            continue;
        };
        let Some(rgb) = parse_hex_rgb(tokens) else {
            warn!(
                "skin colours: --{} is not a single hex colour; keeping the previous default",
                def.css_var
            );
            continue;
        };
        settings.set_default(def.setting, SettingValue::Color3(rgb));
    }
}

/// Parse a custom property's tokens as one CSS hex colour (`#rgb`, `#rgba`,
/// `#rrggbb` or `#rrggbbaa`), returning the sRGB triple. Any alpha digits are
/// accepted and ignored — the palette settings are RGB. `None` for anything
/// that is not exactly one hash token of a valid length.
fn parse_hex_rgb(tokens: &VarTokens) -> Option<[f32; 3]> {
    let mut iter = tokens.iter();
    let first = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    let VarOrToken::Token(VarToken::Hash(hex)) = first else {
        return None;
    };
    let hex = hex.as_str();
    let (red, green, blue) = match hex.len() {
        3 | 4 => (
            hex_nibble(hex, 0)?,
            hex_nibble(hex, 1)?,
            hex_nibble(hex, 2)?,
        ),
        6 | 8 => (hex_pair(hex, 0)?, hex_pair(hex, 2)?, hex_pair(hex, 4)?),
        _other => return None,
    };
    Some([channel(red), channel(green), channel(blue)])
}

/// A `0..=255` channel as the `0.0..=1.0` float the store keeps.
fn channel(value: u8) -> f32 {
    f32::from(value) / 255.0
}

/// The two hex digits starting at `start`, as a byte.
fn hex_pair(hex: &str, start: usize) -> Option<u8> {
    let slice = hex.get(start..start.checked_add(2)?)?;
    u8::from_str_radix(slice, 16).ok()
}

/// The single hex digit at `index`, expanded shorthand-style (`f` → `0xff`).
fn hex_nibble(hex: &str, index: usize) -> Option<u8> {
    let slice = hex.get(index..index.checked_add(1)?)?;
    let nibble = u8::from_str_radix(slice, 16).ok()?;
    nibble.checked_mul(17)
}

/// Registers [`apply_skin_color_defaults`]; the palette settings themselves are
/// registered with the rest in [`ViewerSettings::load`].
pub(crate) struct SkinColorsPlugin;

impl Plugin for SkinColorsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_skin_color_defaults);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingsStore};

    use super::*;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Assert two `f32` slices are element-wise equal within tolerance.
    fn approx_slice(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "length mismatch");
        for (got, want) in actual.iter().zip(expected) {
            assert!(
                (got - want).abs() < 1e-6,
                "{got} != {want} (within 1e-6) in {actual:?} vs {expected:?}"
            );
        }
    }

    /// A [`VarTokens`] holding one hash token, as `bevy_flair` stores a hex
    /// custom-property value.
    fn hash_tokens(hex: &str) -> VarTokens {
        VarTokens::from_iter([VarToken::Hash(hex.into())])
    }

    /// The hex forms parse to the expected sRGB triples, and every invalid
    /// shape is rejected rather than guessed at.
    #[test]
    fn hex_parsing_accepts_css_forms_only() {
        let cases: &[(&str, [f32; 3])] = &[
            ("ffffff", [1.0, 1.0, 1.0]),
            ("000000", [0.0, 0.0, 0.0]),
            ("bfeb7d", [0.749_019_6, 0.921_568_63, 0.490_196_08]),
            // Shorthand digits duplicate.
            ("f00", [1.0, 0.0, 0.0]),
            // Alpha digits are accepted and ignored.
            ("f008", [1.0, 0.0, 0.0]),
            ("1c1f26f2", [0.109_803_92, 0.121_568_63, 0.149_019_6]),
        ];
        for (hex, expected) in cases {
            let parsed = parse_hex_rgb(&hash_tokens(hex));
            assert!(parsed.is_some(), "{hex} failed to parse");
            if let Some(rgb) = parsed {
                approx_slice(&rgb, expected);
            }
        }

        // Wrong length, non-hex digits, a non-hash token, several tokens.
        assert_eq!(parse_hex_rgb(&hash_tokens("fffff")), None);
        assert_eq!(parse_hex_rgb(&hash_tokens("ggg")), None);
        assert_eq!(
            parse_hex_rgb(&VarTokens::from_iter([VarToken::Ident("red".into())])),
            None
        );
        assert_eq!(
            parse_hex_rgb(&VarTokens::from_iter([
                VarToken::Hash("ffffff".into()),
                VarToken::Hash("000000".into()),
            ])),
            None
        );
        assert_eq!(parse_hex_rgb(&VarTokens::new()), None);
    }

    /// Every shipped skin defines every palette token, so no skin silently
    /// falls back to another skin's colours.
    #[test]
    fn shipped_skins_define_every_palette_token() -> Result<(), TestError> {
        for skin in crate::skin::SKINS {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join("skins")
                .join(skin)
                .join("skin.css");
            let css = fs_err::read_to_string(&path)?;
            for def in COLOR_TOKENS {
                assert!(
                    css.contains(&format!("--{}:", def.css_var)),
                    "{} does not define --{}",
                    path.display(),
                    def.css_var
                );
            }
        }
        Ok(())
    }

    /// Setting and css-var names are unique across the palette table.
    #[test]
    fn palette_table_names_are_distinct() {
        let mut settings: Vec<&str> = COLOR_TOKENS.iter().map(|def| def.setting).collect();
        settings.sort_unstable();
        settings.dedup();
        assert_eq!(settings.len(), COLOR_TOKENS.len(), "duplicate setting name");

        let mut vars: Vec<&str> = COLOR_TOKENS.iter().map(|def| def.css_var).collect();
        vars.sort_unstable();
        vars.dedup();
        assert_eq!(vars.len(), COLOR_TOKENS.len(), "duplicate css var name");
    }

    /// The bridge system installs a styled root's hex tokens as declared
    /// defaults: an un-overridden setting follows the skin, an account
    /// override stays put, and [`setting_color`] resolves accordingly.
    #[test]
    fn skin_vars_become_defaults_under_overrides() -> Result<(), TestError> {
        let mut app = App::new();
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        // The user has overridden the friend colour; the skin must not clobber it.
        settings.set(
            Scope::Account,
            SETTING_NAME_TAG_FRIEND,
            SettingValue::Color3([0.1, 0.2, 0.3]),
        );
        app.insert_resource(settings);

        let mut vars = StyleVars::default();
        vars.insert(Arc::from("chat-self"), hash_tokens("ff0000"));
        vars.insert(Arc::from("name-tag-friend"), hash_tokens("00ff00"));
        let root = app.world_mut().spawn(vars).id();
        app.insert_resource(UiRoot(root));
        app.add_systems(Update, apply_skin_color_defaults);
        app.update();

        let settings = app.world().resource::<ViewerSettings>();
        approx_slice(
            &settings.store().get_color3(SETTING_CHAT_SELF)?,
            &[1.0, 0.0, 0.0],
        );
        approx_slice(
            &settings.store().get_color3(SETTING_NAME_TAG_FRIEND)?,
            &[0.1, 0.2, 0.3],
        );
        // A token the styled root does not carry keeps its fallback default.
        approx_slice(
            &settings.store().get_color3(SETTING_NAME_TAG_MUTED)?,
            &[0.4, 0.4, 0.4],
        );

        // The accessor resolves through the same store, and falls back to the
        // table without one.
        let effective = setting_color(Some(settings), SETTING_CHAT_SELF).to_srgba();
        approx_slice(
            &[effective.red, effective.green, effective.blue],
            &[1.0, 0.0, 0.0],
        );
        let fallback = setting_color(None, SETTING_NAME_TAG_LINDEN).to_srgba();
        approx_slice(
            &[fallback.red, fallback.green, fallback.blue],
            &[0.0, 0.5, 1.0],
        );
        Ok(())
    }
}
