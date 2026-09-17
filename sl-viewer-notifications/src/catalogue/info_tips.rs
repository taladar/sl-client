//! Info tips / notifies not routed to nearby chat.
//!
//! One family of [the catalogue](crate::NOTIFICATIONS), joined into it
//! in source order by `catalogue`.

use crate::{
    NO_FORM, NotificationIgnore, NotificationKind, NotificationPriority, NotificationTemplate,
};

/// The info tips family's catalogue entries.
pub(crate) const ENTRIES: &[NotificationTemplate] = &[
    NotificationTemplate {
        name: "LandmarkCreated",
        kind: NotificationKind::Tip,
        message_key: "notification-landmark-created",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GrantedModifyRights",
        kind: NotificationKind::Notify,
        message_key: "notification-granted-modify-rights",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TeleportToPerson",
        kind: NotificationKind::Tip,
        message_key: "notification-teleport-to-person",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: NO_FORM,
        input: None,
    },
];
