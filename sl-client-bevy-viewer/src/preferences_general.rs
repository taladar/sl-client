//! The Preferences **general** tab (`viewer-preferences-general-tab`).
//!
//! The first tab of the preferences floater ([`crate::preferences`]): interface
//! language, the maturity-rating preference, the login start-location default,
//! UI scale, the headline name-tag toggles, and the away timeout. Everything
//! binds through the two-way settings binding
//! ([`crate::settings_binding`]), so the shell's snapshot / Cancel-revert / OK
//! semantics cover every control for free; the systems here are the *appliers*
//! — the pieces that make a stored value do something.
//!
//! Per-control notes:
//!
//! - **Language** applies **live** (unlike the reference's restart notice): the
//!   [`crate::i18n`] runtime switch re-resolves every visible string when the
//!   `UiLanguage` setting changes.
//! - **Maturity** is the one server-backed control: the account-scoped
//!   `PreferredMaturity` setting drives
//!   [`Command::SetAgentPreferences`] (the `AgentPreferences` capability), the
//!   grid's echo is checked, a mismatch retries up to
//!   [`MATURITY_MAX_ATTEMPTS`], and a final failure (or a grid without the
//!   capability, detected by [`MATURITY_TIMEOUT_SECONDS`]) rolls the setting
//!   back and raises the reference `MaturityChangeError` notification. The
//!   combo refuses a value above the account's login ceiling
//!   (`agent_access_max`) by reverting, the reference `canSetMaturity` rule.
//! - **Start location** is read at the *next* login
//!   ([`resolve_start_location`]); the `--start` CLI flag overrides it for one
//!   run.
//! - **UI scale** drives Bevy's [`UiScale`] live; its row carries a live
//!   percentage readout and a reset-to-100% button.
//! - **Name tags** — only the master toggle and the own-tag toggle live here
//!   (the toggles today's renderer honours, applied in
//!   [`crate::avatars::position_name_tags`]); the full reference set is the
//!   separate `viewer-name-tags-preferences` task.
//! - The **away timeout** is registered and edited here but *consumed* by the
//!   away / do-not-disturb mode machinery (`viewer-do-not-disturb-away`),
//!   which is not built yet. The busy / autorespond reply texts moved to the
//!   chat tab ([`crate::preferences_chat`]), where the reference keeps them.
//! - **Skipped deliberately**: `ShowStartLocation` (no login screen exists),
//!   the 12/24-hour clock override (the [`crate::i18n`] ICU formatters follow
//!   the locale's own hour cycle), and the Firestorm-only name-tag extras.
//!
//! Reference (Firestorm, read-only): `panel_preferences_general.xml`,
//! `llfloaterpreference.cpp` (`onChangeMaturity`), `llagent.cpp`
//! (`sendMaturityPreferenceToServer`, `handlePreferredMaturityResult`).

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, SliderRange, SliderStep};
use sl_client_bevy::{
    AgentPreferences, Command, Maturity, SlCommand, SlEvent, SlSessionEvent, StartLocation,
};
use sl_settings::{Scope, SettingValue};

use crate::i18n::Translator;
use crate::notifications::ShowNotification;
use crate::preferences::{spawn_pref_combo, spawn_pref_section, spawn_pref_slider};
use crate::settings::ViewerSettings;
use crate::settings_binding::SettingBinding;

/// The settings section the general tab's own keys live in.
const GENERAL_SECTION: &[&str] = &["general"];

/// The settings section of the login-time keys.
const LOGIN_SECTION: &[&str] = &["login"];

/// The settings section of the UI-wide keys.
const UI_SECTION: &[&str] = &["ui"];

/// The account-scoped maturity preference: `"PG"`, `"M"` or `"A"` (the wire's
/// `access_prefs.max` short strings). Server-backed — see the module docs.
pub(crate) const SETTING_PREFERRED_MATURITY: &str = "PreferredMaturity";

/// The default login start location: `"last"`, `"home"` or a
/// `uri:Region&x&y&z` string ([`StartLocation`]'s wire form). Read at login by
/// [`resolve_start_location`]; the `--start` CLI flag overrides it.
pub(crate) const SETTING_LOGIN_START_LOCATION: &str = "LoginStartLocation";

/// The UI scale factor applied to every logical pixel (Bevy's [`UiScale`]).
pub(crate) const SETTING_UI_SCALE: &str = "UiScale";

/// Seconds of inactivity before the viewer marks the avatar away; `0` = never.
/// Registered here, consumed by the away-mode task
/// (`viewer-do-not-disturb-away`).
pub(crate) const SETTING_AFK_TIMEOUT: &str = "AfkTimeoutSeconds";

/// The UI-scale slider's bounds and step (the reference `UIScaleFactor` range).
const UI_SCALE_MIN: f32 = 0.75;
/// See [`UI_SCALE_MIN`].
const UI_SCALE_MAX: f32 = 2.0;
/// See [`UI_SCALE_MIN`].
const UI_SCALE_STEP: f32 = 0.025;

/// How many times a maturity change is re-sent after a mismatching echo or a
/// timeout before giving up (the reference's three retries).
const MATURITY_MAX_ATTEMPTS: u8 = 3;

/// Seconds after which an unanswered maturity send counts as failed — the path
/// a grid without the `AgentPreferences` capability (local OpenSim) takes.
const MATURITY_TIMEOUT_SECONDS: f64 = 10.0;

/// Register the general tab's settings (the language key lives in
/// [`crate::i18n`], the name-tag keys in [`crate::avatars`]).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        GENERAL_SECTION,
        SETTING_PREFERRED_MATURITY,
        SettingValue::String("PG".to_owned()),
        "The maturity rating ceiling to ask the grid for: PG, M or A",
    );
    settings.register_in(
        LOGIN_SECTION,
        SETTING_LOGIN_START_LOCATION,
        SettingValue::String("last".to_owned()),
        "The default login start location: last, home, or uri:Region&x&y&z",
    );
    settings.register_in(
        UI_SECTION,
        SETTING_UI_SCALE,
        SettingValue::F32(1.0),
        "The UI scale factor (1.0 = native size)",
    );
    settings.register_in(
        GENERAL_SECTION,
        SETTING_AFK_TIMEOUT,
        SettingValue::U32(300),
        "Seconds of inactivity before going away automatically (0 = never)",
    );
}

/// Build the general tab's content into its panel (the [`PREF_TABS`] `build`
/// hook).
///
/// [`PREF_TABS`]: crate::preferences::PREF_TABS
pub(crate) fn build_general_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-language");
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-language",
        SettingBinding::global(crate::i18n::SETTING_UI_LANGUAGE),
        &[
            ("preferences-locale-default", string_value("")),
            ("preferences-locale-english", string_value("en")),
            ("preferences-locale-japanese", string_value("ja")),
            ("preferences-locale-arabic", string_value("ar")),
            ("preferences-locale-polish", string_value("pl")),
            ("preferences-locale-pseudo", string_value("pseudo")),
        ],
    );

    spawn_pref_section(commands, panel, "preferences-section-content-rating");
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-maturity",
        SettingBinding::account(SETTING_PREFERRED_MATURITY),
        &[
            ("preferences-maturity-general", string_value("PG")),
            ("preferences-maturity-moderate", string_value("M")),
            ("preferences-maturity-adult", string_value("A")),
        ],
    );

    spawn_pref_section(commands, panel, "preferences-section-start-location");
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-start-location",
        SettingBinding::global(SETTING_LOGIN_START_LOCATION),
        &[
            ("preferences-start-last", string_value("last")),
            ("preferences-start-home", string_value("home")),
        ],
    );

    spawn_pref_section(commands, panel, "preferences-section-interface");
    let ui_scale_row = spawn_pref_slider(
        commands,
        panel,
        "preferences-row-ui-scale",
        SettingBinding::global(SETTING_UI_SCALE),
        SliderRange::new(UI_SCALE_MIN, UI_SCALE_MAX),
        SliderStep(UI_SCALE_STEP),
    );
    // The live percentage readout and the reset-to-100% button ride in the
    // same row (the row helper documents that callers may extend it).
    commands.spawn((
        Text::default(),
        crate::ui_font::UiFont::Sans.at(crate::preferences::FONT),
        TextColor(crate::preferences::LABEL_COLOR),
        UiScaleReadout,
        Pickable::IGNORE,
        Name::new("preferences:ui-scale-readout"),
        ChildOf(ui_scale_row),
    ));
    let reset = crate::preferences::spawn_footer_button(
        commands,
        ui_scale_row,
        "preferences-ui-scale-reset",
        0,
    );
    commands.entity(reset).observe(reset_ui_scale);

    spawn_pref_section(commands, panel, "preferences-section-name-tags");
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tags",
        SettingBinding::global(crate::avatars::SETTING_SHOW_NAME_TAGS),
    );
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-own-name-tag",
        SettingBinding::global(crate::avatars::SETTING_SHOW_OWN_NAME_TAG),
    );
    // The full name-tag toggle set (viewer-name-tags-preferences): line choices,
    // status lines, colouring and the fade distances the billboard renderer and
    // content composer already honour, bound here to persisted settings.
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tag-display-names",
        SettingBinding::global(crate::name_tag_content::SETTING_SHOW_DISPLAY_NAMES),
    );
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tag-usernames",
        SettingBinding::global(crate::name_tag_content::SETTING_SHOW_USERNAMES),
    );
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tag-group-titles",
        SettingBinding::global(crate::name_tag_content::SETTING_SHOW_GROUP_TITLES),
    );
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tag-typing",
        SettingBinding::global(crate::name_tag_content::SETTING_SHOW_TYPING),
    );
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tag-distance",
        SettingBinding::global(crate::name_tag_content::SETTING_SHOW_DISTANCE),
    );
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tag-friend-color",
        SettingBinding::global(crate::name_tag_content::SETTING_SHOW_FRIEND_COLOR),
    );
    crate::preferences::spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-name-tag-color-by-distance",
        SettingBinding::global(crate::name_tag_content::SETTING_COLOR_BY_DISTANCE),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-name-tag-fade-start",
        SettingBinding::global(crate::name_tag_billboard::SETTING_FADE_START),
        SliderRange::new(0.0, 128.0),
        SliderStep(1.0),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-name-tag-fade-range",
        SettingBinding::global(crate::name_tag_billboard::SETTING_FADE_RANGE),
        SliderRange::new(0.0, 64.0),
        SliderStep(1.0),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-name-tag-bubble-opacity",
        SettingBinding::global(crate::name_tag_billboard::SETTING_BUBBLE_OPACITY),
        SliderRange::new(0.0, 1.0),
        SliderStep(0.05),
    );

    spawn_pref_section(commands, panel, "preferences-section-away");
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-afk-timeout",
        SettingBinding::global(SETTING_AFK_TIMEOUT),
        &[
            ("preferences-afk-never", SettingValue::U32(0)),
            ("preferences-afk-2-min", SettingValue::U32(120)),
            ("preferences-afk-5-min", SettingValue::U32(300)),
            ("preferences-afk-10-min", SettingValue::U32(600)),
            ("preferences-afk-30-min", SettingValue::U32(1_800)),
            ("preferences-afk-60-min", SettingValue::U32(3_600)),
        ],
    );
}

/// A [`SettingValue::String`] from a literal, so the option tables read flat.
fn string_value(value: &str) -> SettingValue {
    SettingValue::String(value.to_owned())
}

// ---------------------------------------------------------------------------
// UI scale.
// ---------------------------------------------------------------------------

/// Marks the UI-scale row's live percentage readout text.
#[derive(Component, Debug, Clone, Copy)]
struct UiScaleReadout;

/// Observer: the UI-scale reset button — write the setting back to 100%; the
/// bound slider and the readout follow through the store.
fn reset_ui_scale(_activate: On<Activate>, settings: Option<ResMut<ViewerSettings>>) {
    if let Some(mut settings) = settings {
        settings.set(Scope::Global, SETTING_UI_SCALE, SettingValue::F32(1.0));
    }
}

/// Keep the UI-scale readout showing the stored factor as a percentage
/// (whole percents, one decimal for the half-percent steps the 0.025
/// increment produces).
fn update_ui_scale_readout(
    settings: Option<Res<ViewerSettings>>,
    mut readouts: Query<&mut Text, With<UiScaleReadout>>,
) {
    let Some(settings) = settings else {
        return;
    };
    let Ok(value) = settings.store().get_f32(SETTING_UI_SCALE) else {
        return;
    };
    let percent = value * 100.0;
    let wanted = if (percent - percent.round()).abs() < 0.05 {
        format!("{percent:.0}%")
    } else {
        format!("{percent:.1}%")
    };
    for mut text in &mut readouts {
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
    }
}

/// Drive Bevy's [`UiScale`] from the stored factor, live. Idempotent — only
/// writes when the resource disagrees with the store, so nothing relayouts
/// while the value is at rest.
fn apply_ui_scale(settings: Option<Res<ViewerSettings>>, mut ui_scale: ResMut<UiScale>) {
    let Some(settings) = settings else {
        return;
    };
    let Ok(want) = settings.store().get_f32(SETTING_UI_SCALE) else {
        return;
    };
    let clamped = want.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
    if (ui_scale.0 - clamped).abs() > f32::EPSILON {
        ui_scale.0 = clamped;
    }
}

// ---------------------------------------------------------------------------
// Maturity: the server-backed preference.
// ---------------------------------------------------------------------------

/// The maturity conversation's state: the account's ceiling from login, the
/// last value the grid confirmed, and the change in flight (if any).
#[derive(Resource, Debug, Default)]
struct MaturitySync {
    /// The account's maturity ceiling (`agent_access_max` from login); a
    /// preference above it is refused locally, the reference `canSetMaturity`.
    ceiling: Option<Maturity>,
    /// The short string (`"PG"` / `"M"` / `"A"`) the grid last confirmed.
    last_confirmed: Option<String>,
    /// A server-reported value waiting to be adopted into the settings store
    /// (deferred until the account scope has loaded, so the write persists).
    pending_server: Option<String>,
    /// The change currently awaiting the grid's echo.
    in_flight: Option<MaturityAttempt>,
}

/// One in-flight maturity change.
#[derive(Debug, Clone)]
struct MaturityAttempt {
    /// The short string sent to the grid.
    value: String,
    /// [`Time::elapsed_secs_f64`] when the (latest) send went out.
    sent_at: f64,
    /// How many sends this change has used (first send = 1).
    attempts: u8,
}

/// Ingest the login account (ceiling + the grid's current rating) and the
/// `AgentPreferences` echoes: a matching echo confirms the in-flight change, a
/// mismatching one retries up to [`MATURITY_MAX_ATTEMPTS`] then rolls back
/// (via [`drive_maturity_setting`] adopting the server value) with the
/// reference `MaturityChangeError` notification.
fn ingest_maturity_events(
    mut events: MessageReader<SlEvent>,
    mut sync: ResMut<MaturitySync>,
    mut sl: MessageWriter<SlCommand>,
    mut show: MessageWriter<ShowNotification>,
    translator: Translator,
    time: Res<Time>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::Account(account) => {
                sync.ceiling = Some(account.agent_access_max);
                sync.pending_server = Some(maturity_short(account.agent_access).to_owned());
                sync.in_flight = None;
                // Ask for the stored preference set too — its `access_prefs.max`
                // is the authoritative value (agent_access is the login echo).
                sl.write(SlCommand(Command::RequestAgentPreferences));
            }
            SlSessionEvent::AgentPreferences(prefs) => {
                let Some(server_max) = prefs.max_access_pref.clone() else {
                    continue;
                };
                let Some(attempt) = sync.in_flight.take() else {
                    sync.pending_server = Some(server_max);
                    continue;
                };
                if attempt.value == server_max {
                    sync.last_confirmed = Some(server_max);
                    continue;
                }
                if attempt.attempts < MATURITY_MAX_ATTEMPTS {
                    sl.write(set_maturity_command(&attempt.value));
                    sync.in_flight = Some(MaturityAttempt {
                        sent_at: time.elapsed_secs_f64(),
                        attempts: attempt.attempts.saturating_add(1),
                        ..attempt
                    });
                    continue;
                }
                show.write(maturity_change_error(
                    &translator,
                    &attempt.value,
                    &server_max,
                ));
                sync.pending_server = Some(server_max);
            }
            _ => {}
        }
    }
}

/// The store-facing half of the maturity flow, gated on the account scope
/// being loaded (so every write persists): adopt a server-reported value, time
/// out an unanswered send, and send a user change (refusing one above the
/// ceiling by reverting).
fn drive_maturity_setting(
    mut sync: ResMut<MaturitySync>,
    settings: Option<ResMut<ViewerSettings>>,
    mut sl: MessageWriter<SlCommand>,
    mut show: MessageWriter<ShowNotification>,
    translator: Translator,
    time: Res<Time>,
) {
    let Some(mut settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }

    // Adopt a server-reported value: the grid is the source of truth for what
    // it stores, so its value lands in the setting and becomes the baseline.
    if let Some(server) = sync.pending_server.take() {
        settings.set_account(
            SETTING_PREFERRED_MATURITY,
            SettingValue::String(server.clone()),
        );
        sync.last_confirmed = Some(server);
        sync.in_flight = None;
    }

    // Time out an unanswered send — the no-capability path (local OpenSim).
    if let Some(attempt) = sync.in_flight.take() {
        if time.elapsed_secs_f64() - attempt.sent_at <= MATURITY_TIMEOUT_SECONDS {
            sync.in_flight = Some(attempt);
        } else if attempt.attempts < MATURITY_MAX_ATTEMPTS {
            sl.write(set_maturity_command(&attempt.value));
            sync.in_flight = Some(MaturityAttempt {
                sent_at: time.elapsed_secs_f64(),
                attempts: attempt.attempts.saturating_add(1),
                ..attempt
            });
        } else if let Some(last) = sync.last_confirmed.clone() {
            show.write(maturity_change_error(&translator, &attempt.value, &last));
            settings.set_account(SETTING_PREFERRED_MATURITY, SettingValue::String(last));
        }
    }

    // Send a user change once nothing is in flight.
    if sync.in_flight.is_some() {
        return;
    }
    let Some(last) = sync.last_confirmed.clone() else {
        return;
    };
    let Ok(wanted) = settings
        .store()
        .get_str(SETTING_PREFERRED_MATURITY)
        .map(str::to_owned)
    else {
        return;
    };
    if wanted == last {
        return;
    }
    if !maturity_within_ceiling(&wanted, sync.ceiling) {
        // Refuse locally, the reference validate rule: revert the setting.
        settings.set_account(SETTING_PREFERRED_MATURITY, SettingValue::String(last));
        return;
    }
    sl.write(set_maturity_command(&wanted));
    sync.in_flight = Some(MaturityAttempt {
        value: wanted,
        sent_at: time.elapsed_secs_f64(),
        attempts: 1,
    });
}

/// The [`Command::SetAgentPreferences`] carrying only `access_prefs.max`.
fn set_maturity_command(value: &str) -> SlCommand {
    SlCommand(Command::SetAgentPreferences(Box::new(AgentPreferences {
        max_access_pref: Some(value.to_owned()),
        ..AgentPreferences::default()
    })))
}

/// The reference `MaturityChangeError` notification, with both ratings
/// localized.
fn maturity_change_error(
    translator: &Translator,
    preferred: &str,
    actual: &str,
) -> ShowNotification {
    ShowNotification::new("MaturityChangeError")
        .arg(
            "PREFERRED_MATURITY",
            maturity_display(translator, preferred),
        )
        .arg("ACTUAL_MATURITY", maturity_display(translator, actual))
}

/// A rating short string's localized display name (falls back to the short
/// string itself for an unknown one).
fn maturity_display(translator: &Translator, short: &str) -> String {
    match short {
        "PG" => translator.get("preferences-maturity-general"),
        "M" => translator.get("preferences-maturity-moderate"),
        "A" => translator.get("preferences-maturity-adult"),
        other => other.to_owned(),
    }
}

/// A [`Maturity`]'s wire short string ([`Maturity::Unknown`] — and any future
/// variant, the enum being non-exhaustive — maps to `"PG"`, the conservative
/// floor).
const fn maturity_short(maturity: Maturity) -> &'static str {
    match maturity {
        Maturity::Mature => "M",
        Maturity::Adult => "A",
        _ => "PG",
    }
}

/// A rating short string's ordering rank (unknown strings rank lowest, so they
/// never pass the ceiling check).
const fn maturity_rank(short: &str) -> u8 {
    match short.as_bytes() {
        b"PG" => 1,
        b"M" => 2,
        b"A" => 3,
        _ => 0,
    }
}

/// Whether `wanted` is within the account's ceiling. A missing or unknown
/// ceiling permits everything — a grid that never said otherwise (local
/// OpenSim) should not lock the combo to PG.
const fn maturity_within_ceiling(wanted: &str, ceiling: Option<Maturity>) -> bool {
    let limit = match ceiling {
        Some(Maturity::Pg) => 1,
        Some(Maturity::Mature) => 2,
        // Adult, Unknown, any future variant, or no ceiling at all: permissive.
        Some(_) | None => 3,
    };
    let rank = maturity_rank(wanted);
    rank != 0 && rank <= limit
}

// ---------------------------------------------------------------------------
// Start location.
// ---------------------------------------------------------------------------

/// The login start location: an explicit `--start` CLI flag wins, else the
/// stored [`SETTING_LOGIN_START_LOCATION`] (in [`StartLocation`]'s wire form),
/// else "last". An unparsable stored value falls back to "last" rather than
/// aborting a login.
pub(crate) fn resolve_start_location(
    cli: Option<StartLocation>,
    stored: Option<&str>,
) -> StartLocation {
    if let Some(cli) = cli {
        return cli;
    }
    stored
        .and_then(|value| value.parse::<StartLocation>().ok())
        .unwrap_or(StartLocation::Last)
}

/// The general tab's runtime systems: the UI-scale applier and the maturity
/// server conversation. (The tab *content* is built by the shell through
/// [`PREF_TABS`]; the language applier lives in [`crate::i18n`], the name-tag
/// gates in [`crate::avatars`].)
///
/// [`PREF_TABS`]: crate::preferences::PREF_TABS
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PreferencesGeneralPlugin;

impl Plugin for PreferencesGeneralPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaturitySync>().add_systems(
            Update,
            (
                apply_ui_scale,
                update_ui_scale_readout,
                ingest_maturity_events,
                drive_maturity_setting.after(ingest_maturity_events),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        Command, LoginAccount, Maturity, RegionCoordinates, SlCommand, SlEvent, SlSessionEvent,
        StartLocation,
    };
    use sl_settings::{SettingValue, SettingsStore};

    use super::{
        AgentPreferences, MaturitySync, SETTING_PREFERRED_MATURITY, drive_maturity_setting,
        ingest_maturity_events, maturity_rank, maturity_short, maturity_within_ceiling,
        resolve_start_location,
    };
    use crate::notifications::ShowNotification;
    use crate::settings::ViewerSettings;

    /// A headless app running the maturity conversation over a store with the
    /// account scope "loaded", plus the resources the [`crate::i18n::Translator`]
    /// param needs (empty bundles — lookups fall back to the key, which is all
    /// the assertions rely on).
    fn maturity_app() -> App {
        let mut store = SettingsStore::new();
        store
            .register(
                SETTING_PREFERRED_MATURITY,
                SettingValue::String("PG".to_owned()),
                "rating",
            )
            .ok();
        let mut settings = ViewerSettings::from_store_for_test(store);
        settings.mark_account_loaded_for_test();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<SlEvent>()
            .add_message::<SlCommand>()
            .add_message::<ShowNotification>()
            .insert_resource(settings)
            .init_resource::<MaturitySync>()
            .init_resource::<bevy_fluent::Localization>()
            .init_resource::<crate::i18n::LocaleFormatting>()
            .insert_resource(crate::i18n::UiLocale::new(
                crate::i18n::LocaleChoice::English,
            ))
            .add_systems(
                Update,
                (
                    ingest_maturity_events,
                    drive_maturity_setting.after(ingest_maturity_events),
                ),
            );
        app
    }

    /// The stored effective maturity string.
    fn stored_maturity(app: &App) -> String {
        app.world()
            .resource::<ViewerSettings>()
            .store()
            .get_str(SETTING_PREFERRED_MATURITY)
            .unwrap_or("<unregistered>")
            .to_owned()
    }

    /// Drain and return all pending [`SlCommand`]s.
    fn drain_commands(app: &mut App) -> Vec<Command> {
        app.world_mut()
            .resource_mut::<Messages<SlCommand>>()
            .drain()
            .map(|command| command.0)
            .collect()
    }

    /// Drain and return all pending [`ShowNotification`]s' template names.
    fn drain_notifications(app: &mut App) -> Vec<&'static str> {
        app.world_mut()
            .resource_mut::<Messages<ShowNotification>>()
            .drain()
            .map(|show| show.template)
            .collect()
    }

    /// The login [`SlSessionEvent::Account`] with the given current / maximum
    /// ratings.
    fn account_event(access: Maturity, ceiling: Maturity) -> SlEvent {
        SlEvent(SlSessionEvent::Account(Box::new(LoginAccount {
            home: None,
            look_at: None,
            agent_access: access,
            agent_access_max: ceiling,
            max_agent_groups: None,
            library_root: None,
            library_owner: None,
        })))
    }

    /// A grid `AgentPreferences` reply / echo carrying only `max_access_pref`.
    fn prefs_event(max: &str) -> SlEvent {
        SlEvent(SlSessionEvent::AgentPreferences(Box::new(
            AgentPreferences {
                max_access_pref: Some(max.to_owned()),
                ..AgentPreferences::default()
            },
        )))
    }

    /// The user's setting write (the combo binding's effect).
    fn set_stored_maturity(app: &mut App, value: &str) {
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .set_account(
                SETTING_PREFERRED_MATURITY,
                SettingValue::String(value.to_owned()),
            );
    }

    /// Login seeds the setting from the grid's current rating and asks for the
    /// stored preference set.
    #[test]
    fn login_seeds_rating_and_requests_preferences() {
        let mut app = maturity_app();
        app.world_mut()
            .write_message(account_event(Maturity::Mature, Maturity::Adult));
        app.update();
        assert_eq!(stored_maturity(&app), "M");
        let commands = drain_commands(&mut app);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, Command::RequestAgentPreferences)),
            "login should request the stored preference set"
        );
    }

    /// A user change within the ceiling is sent, and a matching echo confirms
    /// it (no rollback, no notification).
    #[test]
    fn user_change_sends_and_matching_echo_confirms() {
        let mut app = maturity_app();
        app.world_mut()
            .write_message(account_event(Maturity::Mature, Maturity::Adult));
        app.update();
        drain_commands(&mut app);

        set_stored_maturity(&mut app, "A");
        app.update();
        let commands = drain_commands(&mut app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                Command::SetAgentPreferences(prefs)
                    if prefs.max_access_pref.as_deref() == Some("A")
            )),
            "an in-ceiling change should be sent"
        );

        app.world_mut().write_message(prefs_event("A"));
        app.update();
        assert_eq!(stored_maturity(&app), "A");
        assert_eq!(drain_notifications(&mut app), Vec::<&str>::new());
        // Confirmed: nothing further is sent.
        assert_eq!(drain_commands(&mut app).len(), 0);
    }

    /// Persistently mismatching echoes retry up to the limit, then roll the
    /// setting back to the grid's value and raise `MaturityChangeError`.
    #[test]
    fn mismatching_echoes_retry_then_roll_back() {
        let mut app = maturity_app();
        app.world_mut()
            .write_message(account_event(Maturity::Mature, Maturity::Adult));
        app.update();
        drain_commands(&mut app);

        set_stored_maturity(&mut app, "A");
        app.update();
        assert_eq!(drain_commands(&mut app).len(), 1, "initial send");

        // Two mismatching echoes: each retries.
        for retry in 1..=2 {
            app.world_mut().write_message(prefs_event("M"));
            app.update();
            assert_eq!(drain_commands(&mut app).len(), 1, "retry {retry}");
            assert_eq!(drain_notifications(&mut app), Vec::<&str>::new());
        }

        // The third mismatch exhausts the attempts: rollback + notification.
        app.world_mut().write_message(prefs_event("M"));
        app.update();
        assert_eq!(drain_commands(&mut app).len(), 0, "no further retry");
        assert_eq!(drain_notifications(&mut app), vec!["MaturityChangeError"]);
        assert_eq!(stored_maturity(&app), "M");

        // The rollback settles: nothing new is sent afterwards.
        app.update();
        assert_eq!(drain_commands(&mut app).len(), 0);
    }

    /// A change above the account's ceiling is refused locally: the setting
    /// reverts and nothing is sent.
    #[test]
    fn over_ceiling_change_is_refused() {
        let mut app = maturity_app();
        app.world_mut()
            .write_message(account_event(Maturity::Mature, Maturity::Mature));
        app.update();
        drain_commands(&mut app);

        set_stored_maturity(&mut app, "A");
        app.update();
        assert_eq!(stored_maturity(&app), "M");
        assert_eq!(drain_commands(&mut app).len(), 0);
    }

    /// An unanswered send times out (the no-capability grid): with the
    /// attempts exhausted it rolls back and notifies.
    #[test]
    fn unanswered_send_times_out_and_rolls_back() {
        let mut app = maturity_app();
        app.world_mut()
            .write_message(account_event(Maturity::Mature, Maturity::Adult));
        app.update();
        drain_commands(&mut app);

        set_stored_maturity(&mut app, "A");
        app.update();
        assert_eq!(drain_commands(&mut app).len(), 1, "initial send");

        // Forge the in-flight attempt into the timed-out, attempts-exhausted
        // state (the test cannot wait 10 real seconds).
        {
            let mut sync = app.world_mut().resource_mut::<MaturitySync>();
            if let Some(attempt) = sync.in_flight.as_mut() {
                attempt.sent_at = -3600.0;
                attempt.attempts = 3;
            }
        }
        app.update();
        assert_eq!(drain_notifications(&mut app), vec!["MaturityChangeError"]);
        assert_eq!(stored_maturity(&app), "M");
    }

    /// The wire short strings round-trip through rank in ceiling order.
    #[test]
    fn maturity_ranks_are_ordered() {
        assert!(maturity_rank(maturity_short(Maturity::Pg)) < maturity_rank("M"));
        assert!(maturity_rank("M") < maturity_rank("A"));
        assert_eq!(maturity_rank("nonsense"), 0);
    }

    /// The ceiling check permits up to the ceiling, refuses above it, and
    /// treats a missing / unknown ceiling as permissive.
    #[test]
    fn ceiling_check_matches_reference_rule() {
        assert!(maturity_within_ceiling("PG", Some(Maturity::Pg)));
        assert!(!maturity_within_ceiling("M", Some(Maturity::Pg)));
        assert!(maturity_within_ceiling("M", Some(Maturity::Mature)));
        assert!(!maturity_within_ceiling("A", Some(Maturity::Mature)));
        assert!(maturity_within_ceiling("A", Some(Maturity::Adult)));
        assert!(maturity_within_ceiling("A", Some(Maturity::Unknown)));
        assert!(maturity_within_ceiling("A", None));
        assert!(!maturity_within_ceiling("nonsense", None));
    }

    /// CLI wins, then the stored value, then "last"; garbage falls back.
    #[test]
    fn start_location_precedence() {
        assert_eq!(
            resolve_start_location(Some(StartLocation::Home), Some("last")),
            StartLocation::Home
        );
        assert_eq!(
            resolve_start_location(None, Some("home")),
            StartLocation::Home
        );
        assert_eq!(
            resolve_start_location(None, Some("uri:Tester&1&2&3")),
            StartLocation::Region {
                region: "Tester".to_owned(),
                position: RegionCoordinates::new(1.0, 2.0, 3.0),
            }
        );
        assert_eq!(
            resolve_start_location(None, Some("garbage")),
            StartLocation::Last
        );
        assert_eq!(resolve_start_location(None, None), StartLocation::Last);
    }
}
