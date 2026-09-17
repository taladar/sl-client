//! Standard action-confirmation modals: shared `alertmodal` / `alert`
//! confirms raised by their owning feature (inventory / people / groups /
//! login flow). Data-only entries here; the owning feature supplies the
//! `[COUNT]` / `[NAME]` / `[GROUP]` arguments and reads the response.
//!
//! One family of [the catalogue](crate::NOTIFICATIONS), joined into it
//! in source order by `catalogue`.

use crate::{
    LEAVE_CANCEL_FORM, NotificationIgnore, NotificationKind, NotificationPriority,
    NotificationTemplate, OK_CANCEL_FORM, OK_FORM, VIEW_IM_QUIT_FORM,
};

/// The standard action-confirmation modals family's catalogue entries.
pub(crate) const ENTRIES: &[NotificationTemplate] = &[
    NotificationTemplate {
        name: "ConfirmEmptyTrash",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-empty-trash",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RemoveFromFriends",
        kind: NotificationKind::AlertModal,
        message_key: "notification-remove-from-friends",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GroupLeaveConfirmMember",
        kind: NotificationKind::Alert,
        message_key: "notification-group-leave-confirm-member",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: LEAVE_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "YouHaveBeenLoggedOut",
        kind: NotificationKind::AlertModal,
        message_key: "notification-you-have-been-logged-out",
        title_key: None,
        priority: NotificationPriority::High,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: VIEW_IM_QUIT_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MustAgreeToLogIn",
        kind: NotificationKind::AlertModal,
        message_key: "notification-must-agree-to-login",
        title_key: None,
        priority: NotificationPriority::High,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignore: NotificationIgnore::None,
        ignore_key: None,
        form: OK_FORM,
        input: None,
    },
];
