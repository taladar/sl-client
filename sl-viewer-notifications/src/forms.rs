//! **The form tables** — the button rows a notification's `<form>` offers.
//!
//! One `const` per distinct row, named for the buttons it carries, shared by
//! every catalogue entry that offers that row (the reference's
//! `<usetemplate>`). They live apart from the catalogue because they are
//! referenced from every family and edited on their own.

use crate::NotificationButton;

/// The empty form — a notification with no buttons (a tip, or a bare
/// informational notify).
pub const NO_FORM: &[NotificationButton] = &[];

/// A one-button acknowledgement form (`OK`) — the reference `okbutton` template.
pub const OK_FORM: &[NotificationButton] = &[NotificationButton {
    name: "OK",
    label_key: "notification-button-ok",
    is_default: true,
}];

/// An OK / Cancel form — the reference `okcancelbuttons` template.
pub const OK_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative button reads "Leave" — the reference
/// `okcancelbuttons` with `yestext="Leave"`, used by the leave-group confirm. The
/// affirmative keeps the stable `OK` [`name`](NotificationButton::name) so a
/// consumer routes on it; only the label differs.
pub const LEAVE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-leave",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The logged-out modal's form — the reference `okcancelbuttons` with
/// `yestext="View IM & Chat"` / `notext="Quit"`: the affirmative opens the IM /
/// chat window, the negative quits. As with [`LEAVE_CANCEL_FORM`] the button
/// [`name`](NotificationButton::name)s stay the stable `OK` / `Cancel` so a
/// consumer routes on them; only the labels differ.
pub const VIEW_IM_QUIT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-view-im-chat",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-quit",
        is_default: false,
    },
];

/// A Yes / No confirm — the reference `okcancelignore` with `yestext="Yes"` /
/// `notext="No"` (the drop-attachment / auto-wear confirms). As with
/// [`LEAVE_CANCEL_FORM`] the stable `OK` / `Cancel`
/// [`name`](NotificationButton::name)s (the underlying reference template's
/// button names) are what a consumer routes on; only the labels differ.
pub const YES_NO_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-no",
        is_default: false,
    },
];

/// The save-wearable-changes confirm's three buttons — the reference
/// `yesnocancelbuttons` with `yestext="Save"` / `notext="Don't Save"`. The
/// reference functor names `Yes` / `No` / `Cancel` stay stable under the
/// localized labels.
pub const SAVE_DISCARD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-save",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-dont-save",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// [`SAVE_DISCARD_CANCEL_FORM`] with the affirmative reading "Save All" — the
/// reference `yesnocancelbuttons` with `yestext="Save All"` (the
/// save-all-clothing-changes confirm).
pub const SAVE_ALL_DISCARD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-save-all",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-dont-save",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The discard-unsaved-changes confirm — the reference `okcancelignore` with
/// `yestext="Discard"` / `notext="Keep Editing"`. Stable `OK` / `Cancel`
/// names under the localized labels, as with [`LEAVE_CANCEL_FORM`].
pub const DISCARD_KEEP_EDITING_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-discard",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-keep-editing",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Save" — the reference
/// `okcancelignore` with `yestext="Save"` (the overwrite-outfit confirm).
pub const SAVE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-save",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Remove" — the reference
/// `okcancelbuttons` with `yestext="Remove"` (the remove-AO-set confirm).
pub const REMOVE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-remove",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Send" — the reference
/// `okcancelbuttons` with `yestext="Send"` (the send-sysinfo-to-IM confirm).
pub const SEND_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-send",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// A raw Yes / No two-button form declared explicitly in the reference (the
/// sysinfo-request prompt): the functor names **and** labels are Yes / No.
/// The reference marks no default; the affirmative takes it, per the shared
/// one-default invariant.
pub const YES_NO_BUTTONS_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-no",
        is_default: false,
    },
];

/// The estate-scope chooser — the reference `yesnocancelbuttons` with
/// `yestext="This Estate"` / `notext="All Estates"`, shared by every
/// estate access-list / manager / experience add & remove prompt. The
/// reference functor names `Yes` / `No` / `Cancel` stay stable under the
/// localized labels.
pub const THIS_ESTATE_ALL_ESTATES_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-this-estate",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-all-estates",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The kick-everyone confirm — the reference `okcancelbuttons` with
/// `yestext="Kick All Residents"`.
pub const KICK_ALL_RESIDENTS_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-kick-all-residents",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The elevation-ranges confirm — the reference `yesnocancelbuttons` with
/// `yestext="Ok"` / `notext="Cancel"` / `canceltext="Don't ask"`. Stable
/// `Yes` / `No` / `Cancel` names under OK / Cancel / Don't-ask labels.
pub const OK_CANCEL_DONT_ASK_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-cancel",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-dont-ask",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Bake" — the reference
/// `okcancelbuttons` with `yestext="Bake"` (the max-allowed-groups notice).
pub const BAKE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-bake",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The pathfinding-dirty modal's form — the reference `okcancelbuttons`
/// with `yestext="Rebake"` / `notext="Close"`.
pub const REBAKE_CLOSE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-rebake",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-close",
        is_default: false,
    },
];

/// The pathfinding-dirty notify's one-button form — the reference
/// `okbutton` with `yestext="Rebake region"`.
pub const REBAKE_REGION_FORM: &[NotificationButton] = &[NotificationButton {
    name: "OK",
    label_key: "notification-button-rebake-region",
    is_default: true,
}];

/// The replace-attachment prompt's buttons: the reference declares this form
/// explicitly with functor names `Yes` / `No` under `OK` / `Cancel` labels,
/// so those are the stable names a consumer routes on.
pub const REPLACE_ATTACHMENT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const YES_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_A_NEW_ACCOUNT_TRY_AGAIN_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-create-a-new-account",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-try-again",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_ACCOUNT_CONTINUE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-create-account",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-continue",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONFIRM_AND_LOG_OUT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-confirm-and-log-out",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OK_HELP_TELEPORT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "Help",
        label_key: "notification-button-help",
        is_default: false,
    },
    NotificationButton {
        name: "Teleport",
        label_key: "notification-button-teleport",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OK_HELP_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "Help",
        label_key: "notification-button-help",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONFIRM_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-confirm",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const MALE_FEMALE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-male",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-female",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const INSTALL_SKIP_NOT_NOW_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-install",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-skip",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-not-now",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const QUIT_FORM: &[NotificationButton] = &[NotificationButton {
    name: "OK",
    label_key: "notification-button-quit",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONTINUE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "continue",
        label_key: "notification-button-continue",
        is_default: true,
    },
    NotificationButton {
        name: "cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const RESET_REMIND_ME_NEXT_TIME_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-reset",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-remind-me-next-time",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const MOVE_ITEMS_DONT_MOVE_ITEMS_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-move-items",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-dont-move-items",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const MOVE_ITEMS_DONT_MOVE_ITEMS_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-move-items",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-dont-move-items",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SAVE_OR_DISCARD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-save",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-discard",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DEED_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-deed",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const UNLINK_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-unlink",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DISCARD_CHANGES_KEEP_EDITING_2_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "discard",
        label_key: "notification-button-discard-changes",
        is_default: true,
    },
    NotificationButton {
        name: "keep",
        label_key: "notification-button-keep-editing-2",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONTINUE_LABEL_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-continue",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const STRIP_ALPHA_USE_AS_IS_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "strip",
        label_key: "notification-button-strip-alpha",
        is_default: true,
    },
    NotificationButton {
        name: "use_as_is",
        label_key: "notification-button-use-as-is",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SET_NAME_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "SetName",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const REPLACE_CURRENT_LIST_USE_NEW_NAME_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "ReplaceList",
        label_key: "notification-button-replace-current-list",
        is_default: false,
    },
    NotificationButton {
        name: "SetName",
        label_key: "notification-button-use-new-name",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DELETE_LIST_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "DeleteList",
        label_key: "notification-button-delete",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACCEPT_DECLINE_MUTE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Accept",
        label_key: "notification-button-accept",
        is_default: true,
    },
    NotificationButton {
        name: "Decline",
        label_key: "notification-button-decline",
        is_default: false,
    },
    NotificationButton {
        name: "Mute",
        label_key: "notification-button-mute",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const RESPOND_FORM: &[NotificationButton] = &[NotificationButton {
    name: "respondbutton",
    label_key: "notification-button-respond",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SHUTDOWN_NOW_LATER_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-shutdown-now",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-later",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const GO_TO_KNOWLEDGE_BASE_CLOSE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-go-to-knowledge-base",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-close",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CHANGE_PREFERENCES_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-change-preferences",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const QUIT_DONT_QUIT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-quit",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-dont-quit",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const FIX_IT_KEEP_IT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-fix-it",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-keep-it",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALL_MODES_CURRENT_MODE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-all-modes",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-current-mode",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SAVE_BACKUP_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-save-backup",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const RESTORE_AND_QUIT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-restore-and-quit",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OFFER_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Offer",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACCEPT_DECLINE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Accept",
        label_key: "notification-button-accept",
        is_default: true,
    },
    NotificationButton {
        name: "Decline",
        label_key: "notification-button-decline",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Create",
        label_key: "notification-button-create",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const APPLY_CHANGES_IGNORE_CHANGES_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-apply-changes",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-ignore-changes",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const EJECT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-eject",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const BAN_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-ban",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const JOIN_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-join",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_GROUP_FOR_L_COST_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-create-group-for-l-cost",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const JOIN_DECLINE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-join",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-decline",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CLOSE_FORM: &[NotificationButton] = &[NotificationButton {
    name: "OK",
    label_key: "notification-button-close",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const YES_NO_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-no",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const JOIN_DECLINE_INFO_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Join",
        label_key: "notification-button-join",
        is_default: true,
    },
    NotificationButton {
        name: "Decline",
        label_key: "notification-button-decline",
        is_default: false,
    },
    NotificationButton {
        name: "Info",
        label_key: "notification-button-info",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SUFFIXED_YES_NO_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK_okcancelignore",
        label_key: "notification-button-yes",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel_okcancelignore",
        label_key: "notification-button-no",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const FREEZE_UNFREEZE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-freeze",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-unfreeze",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const EJECT_EJECT_AND_BAN_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-eject",
        is_default: true,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-eject-and-ban",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DONE_FORM: &[NotificationButton] = &[NotificationButton {
    name: "Done",
    label_key: "notification-button-done",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const PLAY_MEDIA_NOW_ALWAYS_PLAY_MEDIA_DO_NOT_PLAY_MEDIA_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Play Media Now",
        label_key: "notification-button-play-media-now",
        is_default: true,
    },
    NotificationButton {
        name: "Always Play Media",
        label_key: "notification-button-always-play-media",
        is_default: false,
    },
    NotificationButton {
        name: "Do Not Pley Media",
        label_key: "notification-button-do-not-play-media",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const PLAY_DONT_PLAY_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Yes",
        label_key: "notification-button-play",
        is_default: false,
    },
    NotificationButton {
        name: "No",
        label_key: "notification-button-dont-play",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ENABLE_DISABLE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Enable",
        label_key: "notification-button-enable",
        is_default: true,
    },
    NotificationButton {
        name: "Disable",
        label_key: "notification-button-disable",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALLOW_DENY_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Allow",
        label_key: "notification-button-allow",
        is_default: true,
    },
    NotificationButton {
        name: "Deny",
        label_key: "notification-button-deny",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACTION_NOW_CONDITION_ALLOW_THIS_DOMAIN_CONDITION_ALLOW_THIS_URL_FORM:
    &[NotificationButton] = &[
    NotificationButton {
        name: "Do Now",
        label_key: "notification-button-action-now",
        is_default: true,
    },
    NotificationButton {
        name: "RememberDomain",
        label_key: "notification-button-condition-allow-this-domain",
        is_default: false,
    },
    NotificationButton {
        name: "RememberURL",
        label_key: "notification-button-condition-allow-this-url",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALLOW_DENY_BLACKLIST_WHITELIST_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Allow",
        label_key: "notification-button-allow",
        is_default: true,
    },
    NotificationButton {
        name: "Deny",
        label_key: "notification-button-deny",
        is_default: false,
    },
    NotificationButton {
        name: "BlacklistDomain",
        label_key: "notification-button-blacklist",
        is_default: false,
    },
    NotificationButton {
        name: "WhitelistDomain",
        label_key: "notification-button-whitelist",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ADD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Add",
        label_key: "notification-button-add",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OK_NO_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-no",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONFIRM_PURCHASE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "ConfirmPurchase",
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const PAY_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-pay",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const UPLOAD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-upload",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONTINUE_NAMED_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Continue",
        label_key: "notification-button-continue",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DETAILS_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Details",
        label_key: "notification-button-details",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const COPY_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-copy",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const REMOVE_ITEMS_AND_DELETE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-remove-items-and-delete",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DELETE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-delete",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CHECK_TRASH_FOLDER_I_WILL_EMPTY_TRASH_LATER_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-check-trash-folder",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-i-will-empty-trash-later",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACCEPT_DISCARD_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Keep",
        label_key: "notification-button-accept",
        is_default: true,
    },
    NotificationButton {
        name: "Discard",
        label_key: "notification-button-discard",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SHOW_ACCEPT_DISCARD_PLUS4_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Show",
        label_key: "notification-button-show",
        is_default: true,
    },
    NotificationButton {
        name: "Accept",
        label_key: "notification-button-accept",
        is_default: false,
    },
    NotificationButton {
        name: "Discard",
        label_key: "notification-button-discard",
        is_default: false,
    },
    NotificationButton {
        name: "ShowSilent",
        label_key: "notification-button-show-2",
        is_default: false,
    },
    NotificationButton {
        name: "AcceptSilent",
        label_key: "notification-button-accept-2",
        is_default: false,
    },
    NotificationButton {
        name: "DiscardSilent",
        label_key: "notification-button-discard-2",
        is_default: false,
    },
    NotificationButton {
        name: "Mute",
        label_key: "notification-button-mute-sender",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OKAY_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-okay",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const GO_TO_PAGE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-go-to-page",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const LATER_GO_NOW_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Later",
        label_key: "notification-button-later",
        is_default: true,
    },
    NotificationButton {
        name: "GoNow...",
        label_key: "notification-button-go-now",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TRUST_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-trust",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TELEPORT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-teleport",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CHANGE_AND_CONTINUE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "OK",
        label_key: "notification-button-change-and-continue",
        is_default: false,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TELEPORT_NAMED_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Teleport",
        label_key: "notification-button-teleport",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TELEPORT_CHANGE_AND_CONTINUE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Teleport",
        label_key: "notification-button-change-and-continue-2",
        is_default: true,
    },
    NotificationButton {
        name: "Cancel",
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALLOW_ALWAYS_ALLOW_DENY_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: "Allow",
        label_key: "notification-button-allow",
        is_default: true,
    },
    NotificationButton {
        name: "Always Allow",
        label_key: "notification-button-always-allow",
        is_default: false,
    },
    NotificationButton {
        name: "Deny",
        label_key: "notification-button-deny",
        is_default: false,
    },
];
