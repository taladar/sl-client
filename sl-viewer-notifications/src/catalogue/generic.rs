//! The generic fallbacks and demo exemplars — one notification of each
//! [kind](crate::NotificationKind), used when a server string arrives with no
//! structured key of its own, plus the two viewer-original failure notices.
//!
//! One family of [the catalogue](crate::NOTIFICATIONS), joined into it
//! in source order by `catalogue`.

use crate::{
    NO_FORM, NotificationIgnore, NotificationKind, NotificationPriority, NotificationTemplate,
    OK_CANCEL_FORM, OK_FORM,
};

/// The generic fallbacks family's catalogue entries.
pub(crate) const ENTRIES: &[NotificationTemplate] = &[
    // A generic transient tip — the fallback for an unkeyed server hint.
    NotificationTemplate {
        name: "SystemTip",
        kind: NotificationKind::Tip,
        message_key: "notification-system-tip",
        title_key: None,
        priority: NotificationPriority::Unspecified,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    // A generic informational toast — the fallback for a plain `AlertMessage`
    // string the simulator sends with no structured key.
    NotificationTemplate {
        name: "SystemMessage",
        kind: NotificationKind::Notify,
        message_key: "notification-system-message",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: true,
        log_to_chat: true,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    // A generic sticky alert that must be acknowledged — the fallback for a
    // non-modal `AlertMessage` / `AgentAlertMessage` that carries no form of its
    // own.
    NotificationTemplate {
        name: "GenericAlert",
        kind: NotificationKind::Alert,
        message_key: "notification-generic-alert",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: true,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    // Viewer-original (no reference counterpart): a queued protocol command whose
    // request never reached the simulator, so the user's action did nothing.
    // `unique` with the command name as context, so a command failing every frame
    // (camera, controls) replaces its own toast instead of stacking.
    NotificationTemplate {
        name: "ViewerCommandSendFailed",
        kind: NotificationKind::Notify,
        message_key: "notification-viewer-command-send-failed",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: true,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    // Viewer-original (no reference counterpart): a request the session made was
    // never answered, so whatever it was for quietly did not happen. `unique`
    // with the request label as context, mirroring ViewerCommandSendFailed.
    NotificationTemplate {
        name: "ViewerRequestNoReply",
        kind: NotificationKind::Notify,
        message_key: "notification-viewer-request-no-reply",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: true,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    // A concrete keyed alert exercising `[KEY]` substitution and `unique` dedup —
    // the region-restart countdown the simulator sends as an `AlertInfo` key.
    NotificationTemplate {
        name: "RegionRestartMinutes",
        kind: NotificationKind::Alert,
        message_key: "notification-region-restart-minutes",
        title_key: None,
        priority: NotificationPriority::High,
        persist: true,
        log_to_chat: true,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    // A modal confirm exercising the scrim path and the ignore checkbox — the
    // reference `ConfirmQuit`.
    NotificationTemplate {
        name: "ConfirmQuit",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-quit",
        title_key: None,
        priority: NotificationPriority::Critical,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignore: NotificationIgnore::DefaultResponse,
        ignore_key: Some("notification-ignoretext-confirm-quit"),
        form: OK_CANCEL_FORM,
        input: None,
    },
];
