//! The Preferences **colors & skins** tab
//! (`viewer-preferences-colors-skins-tab`).
//!
//! Two concerns share the tab, as in the reference's Colors / Skins panels:
//!
//! - **Skin & theme choice.** The UI skin (`graphite` / `azure`) and its theme
//!   overlay become persisted global settings ([`SETTING_UI_SKIN`] /
//!   [`SETTING_UI_SKIN_THEME`]) instead of a CLI-only flag;
//!   [`apply_skin_setting`] drives the live [`SkinSelection`] re-dress the
//!   moment the setting changes — no restart, unlike the reference. The theme
//!   combo repopulates per skin ([`repopulate_theme_combo`]), since overlays
//!   are skin-specific (azure ships base-only today). The CLI / env override
//!   still wins at startup for that run (`SkinSelection::resolve`); the memo
//!   seeding below keeps it standing until the user actually edits the combo.
//! - **The user-tunable colour palette.** Every [`crate::skin_colors`] token —
//!   chat colours by source, the keyword-alert highlight, the name-tag palette
//!   and its distance bands — gets a colour-swatch row bound at **account**
//!   scope, with a per-row reset. The skin supplies the defaults (via the
//!   [`crate::skin_colors`] bridge), so reset-to-default *is*
//!   reset-to-skin-value, and un-overridden swatches follow a skin switch
//!   live.
//!
//! Reference (Firestorm, read-only): `panel_preferences_colors.xml`,
//! `panel_preferences_skins.xml` (`LLPanelPreferenceSkins`),
//! `llfloaterpreference.cpp` (`Pref.getUIColor` / `Pref.applyUIColor`).

use bevy::prelude::*;
use sl_settings::{Scope, SettingValue};
use tracing::warn;

use crate::preferences::{spawn_pref_color, spawn_pref_combo_with_anchor, spawn_pref_section};
use crate::preferences_camera_move::spawn_reset_button;
use crate::settings::ViewerSettings;
use crate::settings_binding::{ComboBindingValues, SettingBinding};
use crate::skin::{DEFAULT_SKIN, SKINS, SkinSelection, THEMES};
use crate::ui_combo::SetComboOptions;

/// The stable id of this tab in [`crate::preferences::PREF_TABS`].
pub(crate) const TAB_ID: &str = "colors-skins";

/// The settings section the skin choice lives in.
const UI_SECTION: &[&str] = &["ui"];

/// The chosen UI skin id (a directory under `assets/skins/`). Global scope —
/// the skin dresses the pre-login UI, before any account scope exists.
pub(crate) const SETTING_UI_SKIN: &str = "UiSkin";

/// The chosen theme overlay id under the skin, `""` for the skin's own base.
pub(crate) const SETTING_UI_SKIN_THEME: &str = "UiSkinTheme";

/// Register the skin-choice settings (the colour palette registers with
/// [`crate::skin_colors::register_settings`]).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        UI_SECTION,
        SETTING_UI_SKIN,
        SettingValue::String(DEFAULT_SKIN.to_owned()),
        "The UI skin (a directory under assets/skins)",
    );
    settings.register_in(
        UI_SECTION,
        SETTING_UI_SKIN_THEME,
        SettingValue::String(String::new()),
        "The skin's theme overlay; empty for the skin's own base",
    );
}

/// The persisted skin / theme pair from a (pre-app, throwaway) settings load,
/// as `SkinSelection::resolve` inputs: the theme is `None` when unset or
/// empty. `resolve` validates both against the shipped skins.
pub(crate) fn stored_skin_choice(settings: &ViewerSettings) -> (Option<String>, Option<String>) {
    let skin = settings
        .store()
        .get_str(SETTING_UI_SKIN)
        .ok()
        .map(str::to_owned);
    let theme = settings
        .store()
        .get_str(SETTING_UI_SKIN_THEME)
        .ok()
        .filter(|theme| !theme.is_empty())
        .map(str::to_owned);
    (skin, theme)
}

/// Marks the theme combo's anchor entity, so [`repopulate_theme_combo`] can
/// rebuild its options when the skin changes.
#[derive(Component, Debug, Clone, Copy)]
struct ThemeComboAnchor;

/// The Fluent key of a skin's combo option label.
fn skin_label_key(skin: &str) -> String {
    format!("preferences-skin-{skin}")
}

/// The Fluent key of a theme's combo option label.
fn theme_label_key(theme: &str) -> String {
    format!("preferences-theme-{theme}")
}

/// The Fluent key of the theme combo's "no overlay" option.
const THEME_BASE_KEY: &str = "preferences-theme-base";

/// Build the tab's content: the skin section (two combos), then one
/// colour-swatch row per [`COLOR_TOKENS`] entry, grouped into chat / name-tag /
/// distance sections, each with a reset-to-skin-default.
pub(crate) fn build_colors_skins_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-skin");
    let skin_options: Vec<(String, SettingValue)> = SKINS
        .iter()
        .map(|skin| {
            (
                skin_label_key(skin),
                SettingValue::String((*skin).to_owned()),
            )
        })
        .collect();
    let skin_option_refs: Vec<(&str, SettingValue)> = skin_options
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let (_skin_row, _skin_anchor) = spawn_pref_combo_with_anchor(
        commands,
        panel,
        "preferences-row-skin",
        SettingBinding::global(SETTING_UI_SKIN),
        &skin_option_refs,
    );
    // Built with the base option only; `repopulate_theme_combo` fills in the
    // active skin's overlays (and keeps them in step with later skin flips).
    let (_theme_row, theme_anchor) = spawn_pref_combo_with_anchor(
        commands,
        panel,
        "preferences-row-theme",
        SettingBinding::global(SETTING_UI_SKIN_THEME),
        &[(THEME_BASE_KEY, SettingValue::String(String::new()))],
    );
    commands.entity(theme_anchor).insert(ThemeComboAnchor);

    spawn_pref_section(commands, panel, "preferences-section-chat-colors");
    for setting in [
        crate::skin_colors::SETTING_CHAT_SELF,
        crate::skin_colors::SETTING_CHAT_OTHERS,
        crate::skin_colors::SETTING_CHAT_OBJECTS,
        crate::skin_colors::SETTING_CHAT_IM,
        crate::skin_colors::SETTING_CHAT_SYSTEM,
        crate::skin_colors::SETTING_KEYWORD_ALERT,
    ] {
        spawn_color_row(commands, panel, setting);
    }

    spawn_pref_section(commands, panel, "preferences-section-name-tag-colors");
    for setting in [
        crate::skin_colors::SETTING_NAME_TAG_DEFAULT,
        crate::skin_colors::SETTING_NAME_TAG_SELF,
        crate::skin_colors::SETTING_NAME_TAG_FRIEND,
        crate::skin_colors::SETTING_NAME_TAG_MUTED,
        crate::skin_colors::SETTING_NAME_TAG_LINDEN,
        crate::skin_colors::SETTING_NAME_TAG_MISMATCH,
    ] {
        spawn_color_row(commands, panel, setting);
    }

    spawn_pref_section(
        commands,
        panel,
        "preferences-section-name-tag-distance-colors",
    );
    for setting in [
        crate::skin_colors::SETTING_NAME_TAG_DISTANCE_WHISPER,
        crate::skin_colors::SETTING_NAME_TAG_DISTANCE_CHAT,
        crate::skin_colors::SETTING_NAME_TAG_DISTANCE_SHOUT,
        crate::skin_colors::SETTING_NAME_TAG_DISTANCE_BEYOND,
    ] {
        spawn_color_row(commands, panel, setting);
    }
}

/// One palette row: an account-bound colour swatch plus its
/// reset-to-skin-default button, labelled by [`row_label_key`].
fn spawn_color_row(commands: &mut Commands, panel: Entity, setting: &'static str) {
    let row = spawn_pref_color(
        commands,
        panel,
        row_label_key(setting),
        SettingBinding::account(setting),
    );
    spawn_reset_button(commands, row, Scope::Account, setting);
}

/// The Fluent key of a palette row's label, one per
/// [`COLOR_TOKENS`](crate::skin_colors::COLOR_TOKENS) setting — a static
/// mapping because the label helpers want `&'static str` keys. The tests pin
/// that every table entry is covered (nothing falls through to the unknown
/// key).
fn row_label_key(setting: &str) -> &'static str {
    match setting {
        "ChatColorSelf" => "preferences-row-chat-color-self",
        "ChatColorOthers" => "preferences-row-chat-color-others",
        "ChatColorObjects" => "preferences-row-chat-color-objects",
        "ChatColorIm" => "preferences-row-chat-color-im",
        "ChatColorSystem" => "preferences-row-chat-color-system",
        "KeywordAlertColor" => "preferences-row-keyword-alert-color",
        "NameTagColorDefault" => "preferences-row-name-tag-color-default",
        "NameTagColorSelf" => "preferences-row-name-tag-color-self",
        "NameTagColorFriend" => "preferences-row-name-tag-color-friend",
        "NameTagColorMuted" => "preferences-row-name-tag-color-muted",
        "NameTagColorLinden" => "preferences-row-name-tag-color-linden",
        "NameTagColorMismatch" => "preferences-row-name-tag-color-mismatch",
        "NameTagDistanceColorWhisper" => "preferences-row-name-tag-distance-whisper",
        "NameTagDistanceColorChat" => "preferences-row-name-tag-distance-chat",
        "NameTagDistanceColorShout" => "preferences-row-name-tag-distance-shout",
        "NameTagDistanceColorBeyond" => "preferences-row-name-tag-distance-beyond",
        _other => "preferences-row-unknown-color",
    }
}

/// Drive the live [`SkinSelection`] from the persisted skin/theme settings.
///
/// The [`Local`] memo is seeded from the store on first run **without
/// applying**, so a CLI / env override (which put a different value into
/// [`SkinSelection`] at startup) stands until the user actually edits a combo.
/// After that, any divergence between the stored pair and the memo validates
/// and applies: an unknown skin is ignored with a warning, a theme the new
/// skin does not ship is reset to base (its override cleared), and
/// [`SkinSelection`] is only written when it really changes — the existing
/// `apply_skin_selection` re-dress does the rest.
fn apply_skin_setting(
    mut settings: Option<ResMut<ViewerSettings>>,
    selection: Option<ResMut<SkinSelection>>,
    mut memo: Local<Option<(String, String)>>,
) {
    let Some(settings) = settings.as_mut() else {
        return;
    };
    let Some(mut selection) = selection else {
        return;
    };
    let stored_skin = settings
        .store()
        .get_str(SETTING_UI_SKIN)
        .unwrap_or(DEFAULT_SKIN)
        .to_owned();
    let stored_theme = settings
        .store()
        .get_str(SETTING_UI_SKIN_THEME)
        .unwrap_or("")
        .to_owned();
    let Some(applied) = memo.as_mut() else {
        *memo = Some((stored_skin, stored_theme));
        return;
    };
    if *applied == (stored_skin.clone(), stored_theme.clone()) {
        return;
    }
    if !SKINS.contains(&stored_skin.as_str()) {
        warn!(
            "preferences: unknown skin {stored_skin:?}; keeping {:?}",
            applied.0
        );
        *applied = (stored_skin, stored_theme);
        return;
    }
    let theme_shipped = THEMES
        .iter()
        .any(|(skin, theme)| *skin == stored_skin && *theme == stored_theme);
    let effective_theme = if stored_theme.is_empty() || theme_shipped {
        stored_theme.clone()
    } else {
        // A theme the new skin does not ship (e.g. graphite/dark surviving a
        // flip to azure): fall back to the base and clear the stale override.
        settings.reset(Scope::Global, SETTING_UI_SKIN_THEME);
        String::new()
    };
    *applied = (stored_skin.clone(), effective_theme.clone());
    let wanted = SkinSelection {
        skin: stored_skin,
        theme: (!effective_theme.is_empty()).then_some(effective_theme),
    };
    if *selection != wanted {
        *selection = wanted;
    }
}

/// Keep the theme combo's options matching the effective skin: the base entry
/// plus one entry per [`THEMES`] overlay that skin ships. In-place via
/// [`SetComboOptions`] (the build-once rule); the bound values are updated in
/// step so a pick writes the right theme id.
fn repopulate_theme_combo(
    settings: Option<Res<ViewerSettings>>,
    mut anchors: Query<(Entity, &mut ComboBindingValues), With<ThemeComboAnchor>>,
    mut set_options: MessageWriter<SetComboOptions>,
    mut memo: Local<Option<String>>,
) {
    let Some(settings) = settings else {
        return;
    };
    let Ok((anchor, mut values)) = anchors.single_mut() else {
        // The tab (and its combo) builds deferred; try again once it exists.
        return;
    };
    let skin = settings
        .store()
        .get_str(SETTING_UI_SKIN)
        .unwrap_or(DEFAULT_SKIN)
        .to_owned();
    if memo.as_ref() == Some(&skin) {
        return;
    }
    let mut labels = vec![THEME_BASE_KEY.to_owned()];
    let mut mapped = vec![SettingValue::String(String::new())];
    for (theme_skin, theme) in THEMES {
        if *theme_skin == skin {
            labels.push(theme_label_key(theme));
            mapped.push(SettingValue::String((*theme).to_owned()));
        }
    }
    set_options.write(SetComboOptions {
        combo: anchor,
        labels,
    });
    values.0 = mapped;
    *memo = Some(skin);
}

/// This tab's runtime systems; the tab content itself is built by the
/// preferences shell through [`crate::preferences::PREF_TABS`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PreferencesColorsSkinsPlugin;

impl Plugin for PreferencesColorsSkinsPlugin {
    fn build(&self, app: &mut App) {
        // `ComboWidgetPlugin` also registers this message; doing it here too
        // (idempotent) keeps the plugin safe to add standalone (tests).
        app.add_message::<SetComboOptions>()
            .add_systems(Update, (apply_skin_setting, repopulate_theme_combo));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_settings::SettingsStore;

    use super::*;
    use crate::skin_colors::COLOR_TOKENS;
    use crate::ui_combo::ComboSelection;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A minimal app with the registered settings (skin + palette), the
    /// default [`SkinSelection`] and this tab's systems.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        crate::skin_colors::register_settings(&mut settings);
        app.insert_resource(settings)
            .init_resource::<SkinSelection>()
            .add_plugins(PreferencesColorsSkinsPlugin);
        app
    }

    /// A freshly registered store pins the intended defaults: the default
    /// skin, no theme overlay.
    #[test]
    fn registered_defaults_pin_the_intended_values() -> Result<(), TestError> {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        let store = settings.store();
        assert_eq!(store.get_str(SETTING_UI_SKIN)?, DEFAULT_SKIN);
        assert_eq!(store.get_str(SETTING_UI_SKIN_THEME)?, "");
        Ok(())
    }

    /// Building the tab into an empty panel spawns every searchable row: the
    /// two skin combos plus one row per palette token.
    #[test]
    fn build_spawns_every_row() {
        let mut app = App::new();
        let panel = app.world_mut().spawn_empty().id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        build_colors_skins_tab(&mut commands, panel);
        queue.apply(app.world_mut());
        let mut rows = app
            .world_mut()
            .query::<&crate::preferences::PrefSearchRow>();
        let expected = COLOR_TOKENS.len().saturating_add(2);
        assert_eq!(
            rows.iter(app.world()).count(),
            expected,
            "2 combo rows + one per palette token"
        );
    }

    /// Every palette setting maps to its own row-label Fluent key (no
    /// collisions, none falling through to the unknown key), and the tab's
    /// section / combo keys are distinct.
    #[test]
    fn tab_label_keys_are_distinct() {
        let mut keys = vec![
            "preferences-tab-colors-skins",
            "preferences-section-skin",
            "preferences-row-skin",
            "preferences-row-theme",
            "preferences-section-chat-colors",
            "preferences-section-name-tag-colors",
            "preferences-section-name-tag-distance-colors",
            "preferences-reset-default",
            THEME_BASE_KEY,
        ];
        for def in COLOR_TOKENS {
            let key = row_label_key(def.setting);
            assert_ne!(
                key, "preferences-row-unknown-color",
                "{} has no row label key",
                def.setting
            );
            keys.push(key);
        }
        let distinct: std::collections::BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(distinct.len(), keys.len(), "duplicate Fluent key");
    }

    /// A stored skin change drives [`SkinSelection`] (the live re-dress), and
    /// a theme valid for that skin rides along.
    #[test]
    fn skin_setting_drives_selection() -> Result<(), TestError> {
        let mut app = test_app();
        // First update seeds the memo from the (default) store.
        app.update();
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_UI_SKIN_THEME,
            SettingValue::String("dark".to_owned()),
        );
        app.update();
        assert_eq!(
            *app.world().resource::<SkinSelection>(),
            SkinSelection {
                skin: "graphite".to_owned(),
                theme: Some("dark".to_owned()),
            }
        );
        Ok(())
    }

    /// Flipping to a skin that does not ship the stored theme falls back to
    /// the skin's base and clears the stale theme override.
    #[test]
    fn invalid_theme_resets_on_skin_change() -> Result<(), TestError> {
        let mut app = test_app();
        app.update();
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_UI_SKIN_THEME,
            SettingValue::String("dark".to_owned()),
        );
        app.update();
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_UI_SKIN,
            SettingValue::String("azure".to_owned()),
        );
        app.update();
        assert_eq!(
            *app.world().resource::<SkinSelection>(),
            SkinSelection {
                skin: "azure".to_owned(),
                theme: None,
            }
        );
        let settings = app.world().resource::<ViewerSettings>();
        assert!(
            !settings.store().is_overridden(SETTING_UI_SKIN_THEME),
            "the stale theme override must be cleared"
        );
        Ok(())
    }

    /// An unknown stored skin (a hand-edited config) is ignored with the
    /// current selection kept.
    #[test]
    fn unknown_skin_is_ignored() -> Result<(), TestError> {
        let mut app = test_app();
        app.update();
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_UI_SKIN,
            SettingValue::String("no-such-skin".to_owned()),
        );
        app.update();
        assert_eq!(
            app.world().resource::<SkinSelection>().skin,
            DEFAULT_SKIN,
            "an unknown skin must not be applied"
        );
        Ok(())
    }

    /// The memo seeds from the store without applying, so a CLI / env override
    /// that made [`SkinSelection`] differ from the store at startup stands
    /// until the user edits.
    #[test]
    fn startup_cli_override_is_not_clobbered() -> Result<(), TestError> {
        let mut app = test_app();
        // As if `--skin azure` overrode the stored default at startup.
        app.insert_resource(SkinSelection {
            skin: "azure".to_owned(),
            theme: None,
        });
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<SkinSelection>().skin,
            "azure",
            "the CLI-selected skin must survive the settings sync"
        );
        Ok(())
    }

    /// The theme combo repopulates per skin: graphite offers base + dark,
    /// azure only base, and the bound values track the labels.
    #[test]
    fn theme_combo_repopulates_per_skin() -> Result<(), TestError> {
        let mut app = test_app();
        let anchor = app
            .world_mut()
            .spawn((
                ThemeComboAnchor,
                ComboSelection {
                    element: "preferences-row-theme",
                    active: 0,
                },
                ComboBindingValues(vec![SettingValue::String(String::new())]),
            ))
            .id();
        app.update();
        // Default skin (graphite): base + dark.
        {
            let values = app
                .world()
                .entity(anchor)
                .get::<ComboBindingValues>()
                .map(|values| values.0.clone())
                .unwrap_or_default();
            assert_eq!(
                values,
                vec![
                    SettingValue::String(String::new()),
                    SettingValue::String("dark".to_owned()),
                ]
            );
        }
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_UI_SKIN,
            SettingValue::String("azure".to_owned()),
        );
        app.update();
        let values = app
            .world()
            .entity(anchor)
            .get::<ComboBindingValues>()
            .map(|values| values.0.clone())
            .unwrap_or_default();
        assert_eq!(
            values,
            vec![SettingValue::String(String::new())],
            "azure ships no overlays: base only"
        );
        Ok(())
    }
}
