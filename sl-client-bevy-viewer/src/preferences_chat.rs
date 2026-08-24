//! The Preferences **chat / IM + privacy** tab
//! (`viewer-preferences-chat-privacy-tab`).
//!
//! The chat-and-privacy tab of the preferences floater
//! ([`crate::preferences`]): the chat **display** options (overlay font size,
//! nearby-toast lifetime, overlay line cap), the chat / IM **disk-logging**
//! options (which chat kinds are written, the transcript timestamp and
//! filename format, the `conversation.log` index), the **automatic replies**
//! (moved here from the general tab — the reference keeps them under Privacy ▸
//! Autoresponse), and the **privacy** controls (online-status visibility and
//! IM-to-email forwarding, the server-stored `UserInfo` pair).
//!
//! Per-control notes:
//!
//! - The **display** options are applied live by [`crate::chat`] (the overlay)
//!   and [`crate::conversations`] (the transcript font).
//! - The **logging** options feed [`chat_log_config_from_settings`]; the
//!   rebuilt [`ChatLogConfig`] is pushed to the runtime's chat logger via
//!   [`Command::SetChatLogConfig`] — once when the account scope loads at
//!   login ([`push_chat_log_config_at_login`]) and again on every OK press
//!   that changed it ([`apply_chat_privacy`]). Until the login push lands the
//!   runtime logs under the all-types-on default it was built with (a benign
//!   few-frame window). The log *path* stays with the network & cache tab; the
//!   12/24-hour clock style has no reference setting and stays 24-hour.
//! - The **automatic replies** are account-scoped texts *consumed* by the
//!   do-not-disturb / away mode machinery ([`crate::presence`]); the keys keep
//!   their original names, so nothing persisted changed by the move here.
//! - The **automatic rejection** section is the settings half of the standing
//!   auto-reject modes ([`crate::auto_reject`]): the three mode toggles (also on
//!   Comm ▸ Online Status, where the reference keeps them), their canned
//!   replies, the two friends exemptions, and the two narrower suppressions
//!   (already-joined group invitations, ad-hoc conferences). The autoresponse
//!   item sits with the replies it is sent alongside, as its item id — an item's
//!   own context menu is what sets it.
//! - The **privacy** pair is server state, not a local setting: the values
//!   live on the grid (`UserInfo` capability, legacy `UserInfoRequest` /
//!   `UpdateUserInfo` UDP on OpenSim), so both are **transient** settings —
//!   seeded from the grid's reply ([`seed_user_info`], requested each time
//!   the floater opens) and written back on OK only when they differ from the
//!   last grid-confirmed state. The IM-to-email toggle is meaningful on
//!   OpenSim; Second Life manages the forwarding preference on the account
//!   website and ignores the field.
//! - Script-permission prompt suppression lives in the **alerts** tab's
//!   notification table (`ScriptPerm`), not here. Chat *colours* are the
//!   colors & skins tab's task; keyword alerts, chat bubbles, per-line
//!   display timestamps and look-at privacy are their own roadmap tasks.
//!
//! Reference (Firestorm, read-only): `panel_preferences_chat.xml`,
//! `panel_preferences_privacy.xml`, `llfloaterpreference.cpp`
//! (`setPersonalInfo`), `llagent.cpp` (`sendAgentUpdateUserInfo`).

use std::collections::BTreeSet;

use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderStep};
use sl_client_bevy::{
    ChatLogConfig, Command, DirectoryVisibility, LoggedChatType, SlCommand, SlEvent,
    SlSessionEvent, TimestampFormat,
};
use sl_settings::{Scope, SettingValue};

use crate::preferences::{
    PreferencesApplied, PreferencesUi, spawn_pref_checkbox, spawn_pref_combo, spawn_pref_section,
    spawn_pref_slider, spawn_pref_text,
};
use crate::settings::ViewerSettings;
use crate::settings_binding::SettingBinding;
use crate::ui::UiPanelShown;
use crate::ui_text_input::TextInputKind;
use crate::world_api::{
    SETTING_AUTORESPOND_NON_FRIENDS_RESPONSE, SETTING_AUTORESPOND_RESPONSE, SETTING_BUSY_RESPONSE,
    SETTING_CHAT_FONT_SIZE, SETTING_CHAT_MAX_LINES, SETTING_NEARBY_TOAST_LIFETIME,
};

/// The stable id of this tab in [`crate::preferences::PREF_TABS`].
pub(crate) const TAB_ID: &str = "chat";

/// The settings section the chat tab's keys live in.
const CHAT_SECTION: &[&str] = &["chat"];

/// Whether nearby chat is written to the per-avatar transcript directory (the
/// reference `LogNearbyChat`). Account-scoped, like every logging key here.
pub(crate) const SETTING_LOG_NEARBY_CHAT: &str = "LogNearbyChat";

/// Whether IMs, group chat and conference chat are written to per-conversation
/// transcripts (the reference `KeepConversationLogTranscripts` level, folded
/// to a single toggle over the session kinds).
pub(crate) const SETTING_LOG_INSTANT_MESSAGES: &str = "LogInstantMessages";

/// Whether transcript filenames use the legacy resident-name scheme (the
/// reference `UseLegacyIMLogNames`).
pub(crate) const SETTING_LOG_LEGACY_NAMES: &str = "UseLegacyIMLogNames";

/// Whether transcript filenames carry a date suffix (the reference
/// `LogFileNamewithDate`).
pub(crate) const SETTING_LOG_FILENAME_DATE: &str = "LogFileNameWithDate";

/// Whether logged lines carry a timestamp prefix at all (the reference
/// `LogTimestamp`).
pub(crate) const SETTING_LOG_TIMESTAMP: &str = "LogTimestamp";

/// Whether a logged timestamp includes the date (the reference
/// `LogTimestampDate`).
pub(crate) const SETTING_LOG_TIMESTAMP_DATE: &str = "LogTimestampDate";

/// Whether a logged timestamp includes seconds (the reference
/// `FSSecondsinChatTimestamps`; this project defaults it **on**).
pub(crate) const SETTING_LOG_TIMESTAMP_SECONDS: &str = "LogTimestampSeconds";

/// Whether the `conversation.log` index of conversations is kept (the
/// reference `KeepConversationLogTranscripts` ≥ "log" level).
pub(crate) const SETTING_CONVERSATION_LOG: &str = "KeepConversationLog";

/// Days a `conversation.log` entry survives without activity before the
/// load-time purge drops it (the Firestorm `FSConversationLogLifetime`).
pub(crate) const SETTING_CONVERSATION_LOG_RETENTION: &str = "ConversationLogRetentionDays";

/// Transient: whether the account's online status is hidden from the people
/// directory ("only friends and groups know I'm online"). Server state — the
/// grid's `UserInfo` `directory_visibility` — never persisted locally.
pub(crate) const SETTING_ONLINE_STATUS_HIDDEN: &str = "OnlineStatusHidden";

/// Transient: whether offline IMs are forwarded to the account email. Server
/// state (`UserInfo` `im_via_email`, meaningful on OpenSim), never persisted
/// locally.
pub(crate) const SETTING_IM_VIA_EMAIL: &str = "ImViaEmail";

/// The default Do Not Disturb auto-reply (the reference
/// `DoNotDisturbModeResponseDefault`).
const BUSY_RESPONSE_DEFAULT: &str = "This resident has turned on 'Do Not Disturb' mode and will \
                                     see your message later.";

/// The default autorespond auto-reply (the reference `AutoResponseModeDefault`,
/// without its `[APP_NAME]` interpolation).
const AUTORESPOND_RESPONSE_DEFAULT: &str = "The Resident you messaged has 'autorespond mode' enabled, which means they have requested \
     not to be disturbed. Your message will still be shown in their IM panel for later viewing.";

/// The multiline reply fields' visible height, in text lines.
const REPLY_FIELD_LINES: f32 = 3.0;

/// Register the chat tab's settings. The display and logging keys are
/// persisted (`[chat]` section; the logging keys bind at account scope); the
/// two `UserInfo` mirrors are transient — server state must never be written
/// to disk as if it were ours.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        CHAT_SECTION,
        SETTING_CHAT_FONT_SIZE,
        SettingValue::U32(1),
        "Chat text size: 0 = small, 1 = medium, 2 = large",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_NEARBY_TOAST_LIFETIME,
        SettingValue::U32(23),
        "Seconds a nearby-chat overlay line stays on screen (including the fade)",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_CHAT_MAX_LINES,
        SettingValue::U32(12),
        "The most nearby-chat overlay lines shown at once",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_LOG_NEARBY_CHAT,
        SettingValue::Bool(true),
        "Save nearby chat to the per-avatar transcript directory",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_LOG_INSTANT_MESSAGES,
        SettingValue::Bool(true),
        "Save IMs, group chat and conference chat to per-conversation transcripts",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_LOG_LEGACY_NAMES,
        SettingValue::Bool(false),
        "Name transcript files with the legacy resident-name scheme",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_LOG_FILENAME_DATE,
        SettingValue::Bool(false),
        "Add a date suffix to transcript filenames",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_LOG_TIMESTAMP,
        SettingValue::Bool(true),
        "Prefix each logged line with a timestamp",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_LOG_TIMESTAMP_DATE,
        SettingValue::Bool(true),
        "Include the date in logged timestamps",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_LOG_TIMESTAMP_SECONDS,
        SettingValue::Bool(true),
        "Include seconds in logged timestamps",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_CONVERSATION_LOG,
        SettingValue::Bool(false),
        "Keep the conversation.log index of past conversations",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_CONVERSATION_LOG_RETENTION,
        SettingValue::U32(30),
        "Days an inactive conversation stays in conversation.log",
    );
    settings.register_transient(
        SETTING_ONLINE_STATUS_HIDDEN,
        SettingValue::Bool(false),
        "Server state: hide my online status from the people directory",
    );
    settings.register_transient(
        SETTING_IM_VIA_EMAIL,
        SettingValue::Bool(false),
        "Server state: email me IMs that arrive while I'm offline (OpenSim)",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_BUSY_RESPONSE,
        SettingValue::String(BUSY_RESPONSE_DEFAULT.to_owned()),
        "The automatic reply sent to IMs while in Do Not Disturb mode",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_AUTORESPOND_RESPONSE,
        SettingValue::String(AUTORESPOND_RESPONSE_DEFAULT.to_owned()),
        "The automatic reply sent to IMs while in autorespond mode",
    );
    settings.register_in(
        CHAT_SECTION,
        SETTING_AUTORESPOND_NON_FRIENDS_RESPONSE,
        SettingValue::String(AUTORESPOND_RESPONSE_DEFAULT.to_owned()),
        "The automatic reply sent to non-friends' IMs while in autorespond-to-non-friends mode",
    );
}

/// Build the chat tab's content into its panel (the
/// [`crate::preferences::PREF_TABS`] `build` hook).
pub(crate) fn build_chat_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-chat-display");
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-chat-font-size",
        SettingBinding::global(SETTING_CHAT_FONT_SIZE),
        &[
            ("preferences-chat-font-small", SettingValue::U32(0)),
            ("preferences-chat-font-medium", SettingValue::U32(1)),
            ("preferences-chat-font-large", SettingValue::U32(2)),
        ],
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-nearby-toast-lifetime",
        SettingBinding::global(SETTING_NEARBY_TOAST_LIFETIME),
        SliderRange::new(3.0, 60.0),
        SliderStep(1.0),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-chat-max-lines",
        SettingBinding::global(SETTING_CHAT_MAX_LINES),
        SliderRange::new(1.0, 50.0),
        SliderStep(1.0),
    );

    spawn_pref_section(commands, panel, "preferences-section-chat-logging");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-log-nearby-chat",
        SettingBinding::account(SETTING_LOG_NEARBY_CHAT),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-log-instant-messages",
        SettingBinding::account(SETTING_LOG_INSTANT_MESSAGES),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-log-timestamp",
        SettingBinding::account(SETTING_LOG_TIMESTAMP),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-log-timestamp-date",
        SettingBinding::account(SETTING_LOG_TIMESTAMP_DATE),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-log-timestamp-seconds",
        SettingBinding::account(SETTING_LOG_TIMESTAMP_SECONDS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-log-filename-date",
        SettingBinding::account(SETTING_LOG_FILENAME_DATE),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-log-legacy-names",
        SettingBinding::account(SETTING_LOG_LEGACY_NAMES),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-conversation-log",
        SettingBinding::account(SETTING_CONVERSATION_LOG),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-conversation-log-retention",
        SettingBinding::account(SETTING_CONVERSATION_LOG_RETENTION),
        SliderRange::new(1.0, 365.0),
        SliderStep(1.0),
    );

    spawn_pref_section(commands, panel, "preferences-section-busy-response");
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-busy-response",
        SettingBinding::account(SETTING_BUSY_RESPONSE),
        TextInputKind::Multiline,
        REPLY_FIELD_LINES,
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-autorespond-response",
        SettingBinding::account(SETTING_AUTORESPOND_RESPONSE),
        TextInputKind::Multiline,
        REPLY_FIELD_LINES,
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-autorespond-non-friends-response",
        SettingBinding::account(SETTING_AUTORESPOND_NON_FRIENDS_RESPONSE),
        TextInputKind::Multiline,
        REPLY_FIELD_LINES,
    );
    // The two opt-in replies (`viewer-do-not-disturb-away`): each is a toggle
    // plus its text, since neither state — merely away, or blocking the sender
    // — implies wanting to answer at all.
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-send-away-response",
        SettingBinding::account(crate::presence::SETTING_SEND_AWAY_RESPONSE),
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-away-response",
        SettingBinding::account(crate::presence::SETTING_AWAY_RESPONSE),
        TextInputKind::Multiline,
        REPLY_FIELD_LINES,
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-send-muted-response",
        SettingBinding::account(crate::presence::SETTING_SEND_MUTED_RESPONSE),
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-muted-response",
        SettingBinding::account(crate::presence::SETTING_MUTED_RESPONSE),
        TextInputKind::Multiline,
        REPLY_FIELD_LINES,
    );
    // The item every mode reply carries along, as its inventory id — set from an
    // item's own context menu ("Send with Autoresponses"), shown here so it can
    // be read back and cleared.
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-autoresponse-item",
        SettingBinding::account(crate::world_api::SETTING_AUTORESPONSE_ITEM),
        TextInputKind::Line,
        1.0,
    );

    // The standing auto-reject modes (`crate::auto_reject`). The three mode
    // toggles are also on Comm ▸ Online Status, where the reference keeps them;
    // their replies and exemptions live only here.
    spawn_pref_section(commands, panel, "preferences-section-auto-reject");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-reject-teleport-offers",
        SettingBinding::account(crate::auto_reject::SETTING_REJECT_TELEPORT_OFFERS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-dont-reject-teleport-from-friends",
        SettingBinding::account(crate::auto_reject::SETTING_DONT_REJECT_TELEPORT_FROM_FRIENDS),
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-reject-teleport-response",
        SettingBinding::account(crate::auto_reject::SETTING_REJECT_TELEPORT_RESPONSE),
        TextInputKind::Multiline,
        REPLY_FIELD_LINES,
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-reject-friendship-requests",
        SettingBinding::account(crate::auto_reject::SETTING_REJECT_FRIENDSHIP_REQUESTS),
    );
    spawn_pref_text(
        commands,
        panel,
        "preferences-row-reject-friendship-response",
        SettingBinding::account(crate::auto_reject::SETTING_REJECT_FRIENDSHIP_RESPONSE),
        TextInputKind::Multiline,
        REPLY_FIELD_LINES,
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-reject-group-invites",
        SettingBinding::account(crate::auto_reject::SETTING_REJECT_ALL_GROUP_INVITES),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-show-joined-group-invitations",
        SettingBinding::account(crate::auto_reject::SETTING_SHOW_JOINED_GROUP_INVITATIONS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-ignore-ad-hoc-sessions",
        SettingBinding::account(crate::auto_reject::SETTING_IGNORE_AD_HOC_SESSIONS),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-dont-ignore-ad-hoc-from-friends",
        SettingBinding::account(crate::auto_reject::SETTING_DONT_IGNORE_AD_HOC_FROM_FRIENDS),
    );

    spawn_pref_section(commands, panel, "preferences-section-privacy");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-online-status-hidden",
        SettingBinding::account(SETTING_ONLINE_STATUS_HIDDEN),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-im-via-email",
        SettingBinding::account(SETTING_IM_VIA_EMAIL),
    );
}

// ---------------------------------------------------------------------------
// The chat-log configuration push.
// ---------------------------------------------------------------------------

/// The [`ChatLogConfig`] last pushed to the runtime, so an OK press that
/// changed nothing sends nothing. `None` until the first push (the runtime
/// then still runs its construction-time configuration).
#[derive(Resource, Debug, Default)]
struct PushedChatLogConfig(Option<ChatLogConfig>);

/// The chat-log configuration the current settings describe. Defaults match
/// the registered setting defaults, so a bare store yields the same
/// all-types-on configuration the runtime is constructed with.
pub(crate) fn chat_log_config_from_settings(settings: &ViewerSettings) -> ChatLogConfig {
    let store = settings.store();
    let flag = |name: &str, default: bool| store.get_bool(name).unwrap_or(default);
    let mut enabled = BTreeSet::new();
    if flag(SETTING_LOG_NEARBY_CHAT, true) {
        enabled.insert(LoggedChatType::Nearby);
    }
    if flag(SETTING_LOG_INSTANT_MESSAGES, true) {
        enabled.extend([
            LoggedChatType::InstantMessage,
            LoggedChatType::Group,
            LoggedChatType::Conference,
        ]);
    }
    let timestamp = flag(SETTING_LOG_TIMESTAMP, true).then(|| TimestampFormat {
        date: flag(SETTING_LOG_TIMESTAMP_DATE, true),
        seconds: flag(SETTING_LOG_TIMESTAMP_SECONDS, true),
        ..TimestampFormat::default()
    });
    ChatLogConfig {
        enabled,
        legacy_im_names: flag(SETTING_LOG_LEGACY_NAMES, false),
        date_suffix: flag(SETTING_LOG_FILENAME_DATE, false),
        timestamp,
        conversation_log: flag(SETTING_CONVERSATION_LOG, false),
        conversation_log_retention_days: store
            .get_u32(SETTING_CONVERSATION_LOG_RETENTION)
            .unwrap_or(30),
        ..ChatLogConfig::default()
    }
}

/// Push the settings-derived chat-log configuration once the per-avatar
/// account scope has loaded at login, so the avatar's stored logging
/// preferences replace the runtime's all-types-on construction default.
/// One-shot; ordered after [`crate::settings::load_account_settings`].
fn push_chat_log_config_at_login(
    settings: Option<Res<ViewerSettings>>,
    mut pushed: ResMut<PushedChatLogConfig>,
    mut sl: MessageWriter<SlCommand>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    *done = true;
    let config = chat_log_config_from_settings(&settings);
    sl.write(SlCommand(Command::SetChatLogConfig(Box::new(
        config.clone(),
    ))));
    pushed.0 = Some(config);
}

// ---------------------------------------------------------------------------
// The server-stored privacy pair (`UserInfo`).
// ---------------------------------------------------------------------------

/// The grid's last-confirmed `UserInfo` state, mirrored by the two transient
/// settings. `None` until the first reply — before that no update is ever
/// sent, so an OK press cannot clobber server state we never saw (the
/// reference keeps the controls disabled until its `UserInfoReply` too; here
/// the account guard plus this gate serve that role).
#[derive(Resource, Debug, Default)]
struct UserInfoSync {
    /// `(online status hidden, IM via email)` as last confirmed by the grid.
    echo: Option<(bool, bool)>,
}

/// Request the grid's stored `UserInfo` pair each time the preferences
/// floater opens, so the privacy checkboxes show (and diff against) current
/// server state. Cap-preferred with a UDP fallback in the runtime; the reply
/// lands in [`seed_user_info`].
fn request_user_info_on_open(
    ui: Option<Res<PreferencesUi>>,
    panels: Query<&UiPanelShown>,
    mut was_open: Local<bool>,
    mut sl: MessageWriter<SlCommand>,
) {
    let open = ui.is_some_and(|ui| panels.get(ui.root).is_ok_and(|shown| shown.0));
    if open && !*was_open {
        sl.write(SlCommand(Command::RequestUserInfo));
    }
    *was_open = open;
}

/// Seed the two transient privacy settings (and the echo they are diffed
/// against) from the grid's `UserInfo` reply. Written at account scope — the
/// scope the rows bind — but transient, so nothing lands on disk. A reply
/// arriving while the floater is open updates the visible checkboxes in
/// place; a subsequent Cancel reverts them to the values snapshotted at open,
/// which is display-only drift the next open's re-request corrects.
fn seed_user_info(
    mut events: MessageReader<SlEvent>,
    mut sync: ResMut<UserInfoSync>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let Some(mut settings) = settings else {
        return;
    };
    for event in events.read() {
        if let SlSessionEvent::UserInfo(info) = &event.0 {
            let hidden = info.directory_visibility == DirectoryVisibility::Hidden;
            settings.set(
                Scope::Account,
                SETTING_ONLINE_STATUS_HIDDEN,
                SettingValue::Bool(hidden),
            );
            settings.set(
                Scope::Account,
                SETTING_IM_VIA_EMAIL,
                SettingValue::Bool(info.im_via_email),
            );
            sync.echo = Some((hidden, info.im_via_email));
        }
    }
}

/// The per-OK apply hook: push a changed chat-log configuration to the
/// runtime's logger, and send a changed privacy pair to the grid
/// ([`Command::UpdateUserInfo`]). Both are diffed — an OK press that changed
/// neither sends nothing — and the privacy send is gated on a grid echo
/// having been seen at all.
fn apply_chat_privacy(
    mut applied: MessageReader<PreferencesApplied>,
    settings: Option<Res<ViewerSettings>>,
    mut sync: ResMut<UserInfoSync>,
    mut pushed: ResMut<PushedChatLogConfig>,
    mut sl: MessageWriter<SlCommand>,
) {
    if applied.read().next().is_none() {
        return;
    }
    let Some(settings) = settings else {
        return;
    };

    let config = chat_log_config_from_settings(&settings);
    if pushed.0.as_ref() != Some(&config) {
        sl.write(SlCommand(Command::SetChatLogConfig(Box::new(
            config.clone(),
        ))));
        pushed.0 = Some(config);
    }

    if let Some(echo) = sync.echo {
        let store = settings.store();
        let hidden = store
            .get_bool(SETTING_ONLINE_STATUS_HIDDEN)
            .unwrap_or(false);
        let im_via_email = store.get_bool(SETTING_IM_VIA_EMAIL).unwrap_or(false);
        if (hidden, im_via_email) != echo {
            let directory_visibility = if hidden {
                DirectoryVisibility::Hidden
            } else {
                DirectoryVisibility::Default
            };
            sl.write(SlCommand(Command::UpdateUserInfo {
                im_via_email,
                directory_visibility,
            }));
            // Optimistic: the POST acknowledges without echoing the fields;
            // the next floater open re-requests the authoritative state.
            sync.echo = Some((hidden, im_via_email));
        }
    }
}

/// The chat tab's runtime side (the tab *content* is built by the shell
/// through [`crate::preferences::PREF_TABS`]): the login-time chat-log
/// configuration push, the `UserInfo` request / seed pair, and the per-OK
/// apply hook.
pub(crate) struct PreferencesChatPlugin;

impl Plugin for PreferencesChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PushedChatLogConfig>()
            .init_resource::<UserInfoSync>()
            .add_systems(
                Update,
                (
                    push_chat_log_config_at_login.after(crate::settings::load_account_settings),
                    request_user_info_on_open,
                    seed_user_info,
                    apply_chat_privacy.after(seed_user_info),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        ChatLogConfig, Command, DirectoryVisibility, LoggedChatType, SlCommand, SlEvent,
        SlSessionEvent, UserInfo,
    };
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        PushedChatLogConfig, SETTING_CONVERSATION_LOG, SETTING_CONVERSATION_LOG_RETENTION,
        SETTING_IM_VIA_EMAIL, SETTING_LOG_FILENAME_DATE, SETTING_LOG_INSTANT_MESSAGES,
        SETTING_LOG_LEGACY_NAMES, SETTING_LOG_NEARBY_CHAT, SETTING_LOG_TIMESTAMP,
        SETTING_LOG_TIMESTAMP_SECONDS, SETTING_ONLINE_STATUS_HIDDEN, UserInfoSync,
        apply_chat_privacy, chat_log_config_from_settings, seed_user_info,
    };
    use crate::preferences::PreferencesApplied;
    use crate::settings::ViewerSettings;

    /// A [`ViewerSettings`] with the chat tab's settings registered and the
    /// account scope marked loaded.
    fn test_settings() -> ViewerSettings {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        super::register_settings(&mut settings);
        settings.mark_account_loaded_for_test();
        settings
    }

    /// A headless app running the seed + apply systems.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<SlEvent>()
            .add_message::<SlCommand>()
            .add_message::<PreferencesApplied>()
            .insert_resource(test_settings())
            .init_resource::<PushedChatLogConfig>()
            .init_resource::<UserInfoSync>()
            .add_systems(
                Update,
                (seed_user_info, apply_chat_privacy.after(seed_user_info)),
            );
        app
    }

    /// Drain and return all pending [`SlCommand`]s.
    fn drain_commands(app: &mut App) -> Vec<Command> {
        app.world_mut()
            .resource_mut::<Messages<SlCommand>>()
            .drain()
            .map(|command| command.0)
            .collect()
    }

    /// The grid's `UserInfo` reply as an [`SlEvent`].
    fn user_info_event(hidden: bool, im_via_email: bool) -> SlEvent {
        SlEvent(SlSessionEvent::UserInfo(UserInfo {
            im_via_email,
            directory_visibility: if hidden {
                DirectoryVisibility::Hidden
            } else {
                DirectoryVisibility::Default
            },
            email: String::new(),
        }))
    }

    /// Write `value` to an account-scoped bool setting (the checkbox
    /// binding's effect).
    fn set_bool(app: &mut App, name: &str, value: bool) {
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Account,
            name,
            SettingValue::Bool(value),
        );
    }

    /// The default settings describe the same all-types-on configuration the
    /// runtime is constructed with, and every knob maps through.
    #[test]
    fn config_from_settings_maps_every_knob() {
        let mut settings = test_settings();
        let config = chat_log_config_from_settings(&settings);
        assert_eq!(
            config.enabled,
            [
                LoggedChatType::Nearby,
                LoggedChatType::InstantMessage,
                LoggedChatType::Group,
                LoggedChatType::Conference,
            ]
            .into_iter()
            .collect()
        );
        assert!(config.timestamp.is_some());
        assert!(!config.legacy_im_names);
        assert!(!config.date_suffix);
        assert!(!config.conversation_log);
        assert_eq!(config.conversation_log_retention_days, 30);
        assert_eq!(config.recall_window, ChatLogConfig::default().recall_window);

        settings.set(
            Scope::Account,
            SETTING_LOG_NEARBY_CHAT,
            SettingValue::Bool(false),
        );
        settings.set(
            Scope::Account,
            SETTING_LOG_INSTANT_MESSAGES,
            SettingValue::Bool(false),
        );
        settings.set(
            Scope::Account,
            SETTING_LOG_TIMESTAMP_SECONDS,
            SettingValue::Bool(false),
        );
        settings.set(
            Scope::Account,
            SETTING_LOG_LEGACY_NAMES,
            SettingValue::Bool(true),
        );
        settings.set(
            Scope::Account,
            SETTING_LOG_FILENAME_DATE,
            SettingValue::Bool(true),
        );
        settings.set(
            Scope::Account,
            SETTING_CONVERSATION_LOG,
            SettingValue::Bool(true),
        );
        settings.set(
            Scope::Account,
            SETTING_CONVERSATION_LOG_RETENTION,
            SettingValue::U32(7),
        );
        let config = chat_log_config_from_settings(&settings);
        assert!(config.enabled.is_empty());
        assert_eq!(config.timestamp.map(|format| format.seconds), Some(false));
        assert!(config.legacy_im_names);
        assert!(config.date_suffix);
        assert!(config.conversation_log);
        assert_eq!(config.conversation_log_retention_days, 7);

        settings.set(
            Scope::Account,
            SETTING_LOG_TIMESTAMP,
            SettingValue::Bool(false),
        );
        assert_eq!(chat_log_config_from_settings(&settings).timestamp, None);
    }

    /// OK pushes the chat-log configuration only when it changed since the
    /// last push.
    #[test]
    fn apply_pushes_chat_log_config_only_on_change() {
        let mut app = test_app();
        app.world_mut().write_message(PreferencesApplied);
        app.update();
        let commands = drain_commands(&mut app);
        assert_eq!(commands.len(), 1, "first OK pushes the initial config");
        assert!(matches!(
            commands.first(),
            Some(Command::SetChatLogConfig(_))
        ));

        app.world_mut().write_message(PreferencesApplied);
        app.update();
        assert_eq!(
            drain_commands(&mut app).len(),
            0,
            "an unchanged OK sends nothing"
        );

        set_bool(&mut app, SETTING_LOG_NEARBY_CHAT, false);
        app.world_mut().write_message(PreferencesApplied);
        app.update();
        let commands = drain_commands(&mut app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                Command::SetChatLogConfig(config) if !config.logs_nearby()
            )),
            "a changed OK pushes the new config"
        );
    }

    /// The grid's reply seeds the transient settings and the echo; OK sends
    /// `UpdateUserInfo` only for a user change, and never before a reply was
    /// seen at all.
    #[test]
    fn user_info_seeds_and_applies_on_change_only() {
        let mut app = test_app();

        // OK before any grid reply: no update may be sent.
        set_bool(&mut app, SETTING_ONLINE_STATUS_HIDDEN, true);
        app.world_mut().write_message(PreferencesApplied);
        app.update();
        assert!(
            !drain_commands(&mut app)
                .iter()
                .any(|command| matches!(command, Command::UpdateUserInfo { .. })),
            "no update before the grid's state was seen"
        );

        // The reply seeds the settings (overwriting the premature edit).
        app.world_mut().write_message(user_info_event(false, false));
        app.update();
        drain_commands(&mut app);
        let settings = app.world().resource::<ViewerSettings>();
        assert!(
            !settings
                .store()
                .get_bool(super::SETTING_ONLINE_STATUS_HIDDEN)
                .unwrap_or(true),
            "the reply overwrites the premature edit"
        );
        assert!(
            !settings
                .store()
                .get_bool(SETTING_IM_VIA_EMAIL)
                .unwrap_or(true)
        );

        // An unchanged OK sends nothing.
        app.world_mut().write_message(PreferencesApplied);
        app.update();
        assert!(
            !drain_commands(&mut app)
                .iter()
                .any(|command| matches!(command, Command::UpdateUserInfo { .. })),
        );

        // A changed pair is sent, and a repeat OK stays quiet.
        set_bool(&mut app, SETTING_ONLINE_STATUS_HIDDEN, true);
        app.world_mut().write_message(PreferencesApplied);
        app.update();
        let commands = drain_commands(&mut app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                Command::UpdateUserInfo {
                    im_via_email: false,
                    directory_visibility: DirectoryVisibility::Hidden,
                }
            )),
            "the changed pair is sent"
        );
        app.world_mut().write_message(PreferencesApplied);
        app.update();
        assert!(
            !drain_commands(&mut app)
                .iter()
                .any(|command| matches!(command, Command::UpdateUserInfo { .. })),
            "the optimistic echo suppresses a repeat send"
        );
    }
}
