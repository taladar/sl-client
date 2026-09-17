//! RLV (viewer-notification-catalogue-rlv).
//!
//! One family of [the catalogue](crate::NOTIFICATIONS), joined into it
//! in source order by `catalogue`.

use crate::{
    ALLOW_ALWAYS_ALLOW_DENY_FORM, NotificationIgnore, NotificationKind, NotificationPriority,
    NotificationTemplate, OK_FORM,
};

/// The RLV family's catalogue entries.
pub(crate) const ENTRIES: &[NotificationTemplate] = &[
    NotificationTemplate {
        name: "RLVaChangeStrings",
        kind: NotificationKind::AlertModal,
        message_key: "notification-rl-va-change-strings",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    // reference type="offer", mapped to Notify.
    NotificationTemplate {
        name: "RLVaListRequested",
        kind: NotificationKind::Notify,
        message_key: "notification-rl-va-list-requested",
        title_key: Some("notification-title-restriction-request-from-name-label"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::DefaultResponse,
        ignore_key: Some("notification-ignoretext-rl-va-list-requested"),
        form: ALLOW_ALWAYS_ALLOW_DENY_FORM,
        input: None,
    },
    // The next two have **no reference counterpart**, and could not: there,
    // `RestrainedLove` needs a restart to take effect, so the toggle raises a
    // `GenericAlert` reading "RLVa will be enabled after you restart" and the
    // menu item wears a "(pending restart)" suffix until you do. This viewer
    // applies the change at once, which is friendlier and is what makes these
    // necessary — the change has consequences the user has to be told about,
    // and the reference never had to tell anyone because nothing happened yet.
    //
    // `Alert` rather than `AlertModal`: it must be acknowledged (it is not a
    // tip that can fade past unread) but it does not need to block the world,
    // since it reports something that has already happened. `unique` so that
    // flipping the switch twice leaves one card, not a stack.
    NotificationTemplate {
        name: "RLVaToggledOn",
        kind: NotificationKind::Alert,
        message_key: "notification-rl-va-toggled-on",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RLVaToggledOff",
        kind: NotificationKind::Alert,
        message_key: "notification-rl-va-toggled-off",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
];
