//! **The form tables** — the button rows a notification's `<form>` offers.
//!
//! One `const` per distinct row, named for the buttons it carries, shared by
//! every catalogue entry that offers that row (the reference's
//! `<usetemplate>`). They live apart from the catalogue because they are
//! referenced from every family and edited on their own.
//!
//! Every button name comes from a **button-set enum** declared here
//! ([`OkCancel`], [`YesNoCancel`], …) — one per distinct set of names, shared
//! by every form offering that set whatever its labels read. A consumer
//! answers through the same enum (`NotificationResponse::answer` with a
//! `TemplateRef`), so the string a form sends and the one a handler expects
//! are produced in one place.

use crate::{FormAnswer, NotificationButton};

/// Declare the button sets the forms below are built from: for each, an enum
/// with one variant per button, its inherent `const fn name` (the stable wire
/// name a [`NotificationButton`] carries, usable in a `const` form table), and
/// its [`FormAnswer`] impl. Under `cfg(test)` it also lists every set's names
/// in `ANSWER_SETS`, so a test can check each form against exactly one set.
macro_rules! form_answers {
    ($(
        $(#[$meta:meta])*
        $answer:ident { $($variant:ident = $name:literal),+ $(,)? }
    )+) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum $answer {
                $(
                    #[doc = concat!("The `", $name, "` button.")]
                    $variant,
                )+
            }

            impl $answer {
                /// The button's stable name — what the form table carries,
                /// the host answers with and the open-notification file
                /// stores. Not translated: it is an identifier, not a label.
                #[must_use]
                pub const fn name(self) -> &'static str {
                    match self {
                        $(Self::$variant => $name,)+
                    }
                }
            }

            impl FormAnswer for $answer {
                const NAMES: &'static [&'static str] = &[$($name),+];

                fn from_name(name: &str) -> Option<Self> {
                    match name {
                        $($name => Some(Self::$variant),)+
                        _ => None,
                    }
                }

                fn name(self) -> &'static str {
                    Self::name(self)
                }
            }
        )+

        /// Every button set's [`FormAnswer::NAMES`], in declaration order.
        #[cfg(test)]
        pub(crate) const ANSWER_SETS: &[&[&str]] = &[$(<$answer as FormAnswer>::NAMES),+];
    };
}

form_answers! {
    /// A one-button acknowledgement form's button — the reference `okbutton`
    /// functor name, whatever the label reads (OK, Close, Quit, …).
    OkOnly {
        Ok = "OK",
    }
    /// An OK / Cancel form's buttons — the reference `okcancelbuttons` /
    /// `okcancelignore` functor names, whatever the labels read (Yes / No,
    /// Leave / Cancel, …). [`YES_NO_FORM`] is one of these: its Yes is `OK`.
    OkCancel {
        Ok = "OK",
        Cancel = "Cancel",
    }
    /// A Yes / No / Cancel form's buttons — the reference `yesnocancelbuttons`
    /// functor names, whatever the labels read (Save / Don't Save / Cancel, …).
    YesNoCancel {
        Yes = "Yes",
        No = "No",
        Cancel = "Cancel",
    }
    /// A two-button form named `Yes` / `No`, whatever the labels read (Play /
    /// Don't Play, …).
    YesNo {
        Yes = "Yes",
        No = "No",
    }
    /// The buttons of [`OK_HELP_TELEPORT_FORM`].
    OkHelpTeleport {
        Ok = "OK",
        Help = "Help",
        Teleport = "Teleport",
    }
    /// The buttons of [`OK_HELP_FORM`].
    OkHelp {
        Ok = "OK",
        Help = "Help",
    }
    /// The buttons of [`CONTINUE_CANCEL_FORM`].
    ContinueCancel {
        Continue = "continue",
        Cancel = "cancel",
    }
    /// The buttons of [`DISCARD_CHANGES_KEEP_EDITING_2_FORM`].
    DiscardChangesKeepEditing {
        Discard = "discard",
        Keep = "keep",
    }
    /// The buttons of [`STRIP_ALPHA_USE_AS_IS_FORM`].
    StripAlphaUseAsIs {
        Strip = "strip",
        UseAsIs = "use_as_is",
    }
    /// The buttons of [`SET_NAME_CANCEL_FORM`].
    SetNameCancel {
        SetName = "SetName",
        Cancel = "Cancel",
    }
    /// The buttons of [`REPLACE_CURRENT_LIST_USE_NEW_NAME_FORM`].
    ReplaceListSetName {
        ReplaceList = "ReplaceList",
        SetName = "SetName",
    }
    /// The buttons of [`DELETE_LIST_CANCEL_FORM`].
    DeleteListCancel {
        DeleteList = "DeleteList",
        Cancel = "Cancel",
    }
    /// The buttons of [`ACCEPT_DECLINE_MUTE_FORM`].
    AcceptDeclineMute {
        Accept = "Accept",
        Decline = "Decline",
        Mute = "Mute",
    }
    /// The buttons of [`RESPOND_FORM`].
    Respond {
        Respond = "respondbutton",
    }
    /// The buttons of [`OFFER_CANCEL_FORM`].
    OfferCancel {
        Offer = "Offer",
        Cancel = "Cancel",
    }
    /// The buttons of [`ACCEPT_DECLINE_FORM`].
    AcceptDecline {
        Accept = "Accept",
        Decline = "Decline",
    }
    /// The buttons of [`CREATE_CANCEL_FORM`].
    CreateCancel {
        Create = "Create",
        Cancel = "Cancel",
    }
    /// The buttons of [`JOIN_DECLINE_INFO_FORM`].
    JoinDeclineInfo {
        Join = "Join",
        Decline = "Decline",
        Info = "Info",
    }
    /// The buttons of [`SUFFIXED_YES_NO_FORM`].
    SuffixedOkCancel {
        Ok = "OK_okcancelignore",
        Cancel = "Cancel_okcancelignore",
    }
    /// The buttons of [`DONE_FORM`].
    Done {
        Done = "Done",
    }
    /// The buttons of
    /// [`PLAY_MEDIA_NOW_ALWAYS_PLAY_MEDIA_DO_NOT_PLAY_MEDIA_FORM`] (the
    /// reference's misspelt `Do Not Pley Media` name is kept: it is the id).
    PlayMediaChoice {
        PlayNow = "Play Media Now",
        AlwaysPlay = "Always Play Media",
        DoNotPlay = "Do Not Pley Media",
    }
    /// The buttons of [`ENABLE_DISABLE_FORM`].
    EnableDisable {
        Enable = "Enable",
        Disable = "Disable",
    }
    /// The buttons of [`ALLOW_DENY_FORM`].
    AllowDeny {
        Allow = "Allow",
        Deny = "Deny",
    }
    /// The buttons of
    /// [`ACTION_NOW_CONDITION_ALLOW_THIS_DOMAIN_CONDITION_ALLOW_THIS_URL_FORM`].
    DoNowRemember {
        DoNow = "Do Now",
        RememberDomain = "RememberDomain",
        RememberUrl = "RememberURL",
    }
    /// The buttons of [`ALLOW_DENY_BLACKLIST_WHITELIST_FORM`].
    AllowDenyBlacklistWhitelist {
        Allow = "Allow",
        Deny = "Deny",
        BlacklistDomain = "BlacklistDomain",
        WhitelistDomain = "WhitelistDomain",
    }
    /// The buttons of [`ADD_CANCEL_FORM`].
    AddCancel {
        Add = "Add",
        Cancel = "Cancel",
    }
    /// The buttons of [`CONFIRM_PURCHASE_CANCEL_FORM`].
    ConfirmPurchaseCancel {
        ConfirmPurchase = "ConfirmPurchase",
        Cancel = "Cancel",
    }
    /// The buttons of [`CONTINUE_NAMED_CANCEL_FORM`].
    ContinueNamedCancel {
        Continue = "Continue",
        Cancel = "Cancel",
    }
    /// The buttons of [`DETAILS_CANCEL_FORM`].
    DetailsCancel {
        Details = "Details",
        Cancel = "Cancel",
    }
    /// The buttons of [`ACCEPT_DISCARD_FORM`].
    KeepDiscard {
        Keep = "Keep",
        Discard = "Discard",
    }
    /// The buttons of [`SHOW_ACCEPT_DISCARD_PLUS4_FORM`].
    ShowAcceptDiscard {
        Show = "Show",
        Accept = "Accept",
        Discard = "Discard",
        ShowSilent = "ShowSilent",
        AcceptSilent = "AcceptSilent",
        DiscardSilent = "DiscardSilent",
        Mute = "Mute",
    }
    /// The buttons of [`LATER_GO_NOW_FORM`].
    LaterGoNow {
        Later = "Later",
        GoNow = "GoNow...",
    }
    /// A Teleport / Cancel form's buttons.
    TeleportCancel {
        Teleport = "Teleport",
        Cancel = "Cancel",
    }
    /// The buttons of [`ALLOW_ALWAYS_ALLOW_DENY_FORM`].
    AllowAlwaysAllowDeny {
        Allow = "Allow",
        AlwaysAllow = "Always Allow",
        Deny = "Deny",
    }
}

/// The empty form — a notification with no buttons (a tip, or a bare
/// informational notify).
pub const NO_FORM: &[NotificationButton] = &[];

/// A one-button acknowledgement form (`OK`) — the reference `okbutton` template.
pub const OK_FORM: &[NotificationButton] = &[NotificationButton {
    name: OkOnly::Ok.name(),
    label_key: "notification-button-ok",
    is_default: true,
}];

/// An OK / Cancel form — the reference `okcancelbuttons` template.
pub const OK_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
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
        name: OkCancel::Ok.name(),
        label_key: "notification-button-leave",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
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
        name: OkCancel::Ok.name(),
        label_key: "notification-button-view-im-chat",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
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
        name: OkCancel::Ok.name(),
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
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
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-save",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-dont-save",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// [`SAVE_DISCARD_CANCEL_FORM`] with the affirmative reading "Save All" — the
/// reference `yesnocancelbuttons` with `yestext="Save All"` (the
/// save-all-clothing-changes confirm).
pub const SAVE_ALL_DISCARD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-save-all",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-dont-save",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The discard-unsaved-changes confirm — the reference `okcancelignore` with
/// `yestext="Discard"` / `notext="Keep Editing"`. Stable `OK` / `Cancel`
/// names under the localized labels, as with [`LEAVE_CANCEL_FORM`].
pub const DISCARD_KEEP_EDITING_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-discard",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-keep-editing",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Save" — the reference
/// `okcancelignore` with `yestext="Save"` (the overwrite-outfit confirm).
pub const SAVE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-save",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Remove" — the reference
/// `okcancelbuttons` with `yestext="Remove"` (the remove-AO-set confirm).
pub const REMOVE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-remove",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Send" — the reference
/// `okcancelbuttons` with `yestext="Send"` (the send-sysinfo-to-IM confirm).
pub const SEND_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-send",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
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
        name: YesNo::Yes.name(),
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: YesNo::No.name(),
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
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-this-estate",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-all-estates",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The kick-everyone confirm — the reference `okcancelbuttons` with
/// `yestext="Kick All Residents"`.
pub const KICK_ALL_RESIDENTS_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-kick-all-residents",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The elevation-ranges confirm — the reference `yesnocancelbuttons` with
/// `yestext="Ok"` / `notext="Cancel"` / `canceltext="Don't ask"`. Stable
/// `Yes` / `No` / `Cancel` names under OK / Cancel / Don't-ask labels.
pub const OK_CANCEL_DONT_ASK_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-dont-ask",
        is_default: false,
    },
];

/// An OK / Cancel form whose affirmative reads "Bake" — the reference
/// `okcancelbuttons` with `yestext="Bake"` (the max-allowed-groups notice).
pub const BAKE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-bake",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// The pathfinding-dirty modal's form — the reference `okcancelbuttons`
/// with `yestext="Rebake"` / `notext="Close"`.
pub const REBAKE_CLOSE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-rebake",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-close",
        is_default: false,
    },
];

/// The pathfinding-dirty notify's one-button form — the reference
/// `okbutton` with `yestext="Rebake region"`.
pub const REBAKE_REGION_FORM: &[NotificationButton] = &[NotificationButton {
    name: OkOnly::Ok.name(),
    label_key: "notification-button-rebake-region",
    is_default: true,
}];

/// The replace-attachment prompt's buttons: the reference declares this form
/// explicitly with functor names `Yes` / `No` under `OK` / `Cancel` labels,
/// so those are the stable names a consumer routes on.
pub const REPLACE_ATTACHMENT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNo::Yes.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: YesNo::No.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const YES_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_A_NEW_ACCOUNT_TRY_AGAIN_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-create-a-new-account",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-try-again",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_ACCOUNT_CONTINUE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-create-account",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-continue",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONFIRM_AND_LOG_OUT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-confirm-and-log-out",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OK_HELP_TELEPORT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkHelpTeleport::Ok.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: OkHelpTeleport::Help.name(),
        label_key: "notification-button-help",
        is_default: false,
    },
    NotificationButton {
        name: OkHelpTeleport::Teleport.name(),
        label_key: "notification-button-teleport",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OK_HELP_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkHelp::Ok.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: OkHelp::Help.name(),
        label_key: "notification-button-help",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONFIRM_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-confirm",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const MALE_FEMALE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-male",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-female",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const INSTALL_SKIP_NOT_NOW_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-install",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-skip",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-not-now",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const QUIT_FORM: &[NotificationButton] = &[NotificationButton {
    name: OkOnly::Ok.name(),
    label_key: "notification-button-quit",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONTINUE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: ContinueCancel::Continue.name(),
        label_key: "notification-button-continue",
        is_default: true,
    },
    NotificationButton {
        name: ContinueCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const RESET_REMIND_ME_NEXT_TIME_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-reset",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-remind-me-next-time",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const MOVE_ITEMS_DONT_MOVE_ITEMS_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-move-items",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-dont-move-items",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const MOVE_ITEMS_DONT_MOVE_ITEMS_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-move-items",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-dont-move-items",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SAVE_OR_DISCARD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-save",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-discard",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DEED_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-deed",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const UNLINK_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-unlink",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DISCARD_CHANGES_KEEP_EDITING_2_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: DiscardChangesKeepEditing::Discard.name(),
        label_key: "notification-button-discard-changes",
        is_default: true,
    },
    NotificationButton {
        name: DiscardChangesKeepEditing::Keep.name(),
        label_key: "notification-button-keep-editing-2",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONTINUE_LABEL_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-continue",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const STRIP_ALPHA_USE_AS_IS_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: StripAlphaUseAsIs::Strip.name(),
        label_key: "notification-button-strip-alpha",
        is_default: true,
    },
    NotificationButton {
        name: StripAlphaUseAsIs::UseAsIs.name(),
        label_key: "notification-button-use-as-is",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SET_NAME_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: SetNameCancel::SetName.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: SetNameCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const REPLACE_CURRENT_LIST_USE_NEW_NAME_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: ReplaceListSetName::ReplaceList.name(),
        label_key: "notification-button-replace-current-list",
        is_default: false,
    },
    NotificationButton {
        name: ReplaceListSetName::SetName.name(),
        label_key: "notification-button-use-new-name",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DELETE_LIST_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: DeleteListCancel::DeleteList.name(),
        label_key: "notification-button-delete",
        is_default: true,
    },
    NotificationButton {
        name: DeleteListCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACCEPT_DECLINE_MUTE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: AcceptDeclineMute::Accept.name(),
        label_key: "notification-button-accept",
        is_default: true,
    },
    NotificationButton {
        name: AcceptDeclineMute::Decline.name(),
        label_key: "notification-button-decline",
        is_default: false,
    },
    NotificationButton {
        name: AcceptDeclineMute::Mute.name(),
        label_key: "notification-button-mute",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const RESPOND_FORM: &[NotificationButton] = &[NotificationButton {
    name: Respond::Respond.name(),
    label_key: "notification-button-respond",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SHUTDOWN_NOW_LATER_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-shutdown-now",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-later",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const GO_TO_KNOWLEDGE_BASE_CLOSE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-go-to-knowledge-base",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-close",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CHANGE_PREFERENCES_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-change-preferences",
        is_default: false,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const QUIT_DONT_QUIT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-quit",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-dont-quit",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const FIX_IT_KEEP_IT_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-fix-it",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-keep-it",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALL_MODES_CURRENT_MODE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-all-modes",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-current-mode",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SAVE_BACKUP_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-save-backup",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const RESTORE_AND_QUIT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-restore-and-quit",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OFFER_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OfferCancel::Offer.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: OfferCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACCEPT_DECLINE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: AcceptDecline::Accept.name(),
        label_key: "notification-button-accept",
        is_default: true,
    },
    NotificationButton {
        name: AcceptDecline::Decline.name(),
        label_key: "notification-button-decline",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: CreateCancel::Create.name(),
        label_key: "notification-button-create",
        is_default: true,
    },
    NotificationButton {
        name: CreateCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const APPLY_CHANGES_IGNORE_CHANGES_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-apply-changes",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-ignore-changes",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const EJECT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-eject",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const BAN_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-ban",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const JOIN_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-join",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CREATE_GROUP_FOR_L_COST_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-create-group-for-l-cost",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const JOIN_DECLINE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-join",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-decline",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CLOSE_FORM: &[NotificationButton] = &[NotificationButton {
    name: OkOnly::Ok.name(),
    label_key: "notification-button-close",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const YES_NO_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-yes",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-no",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const JOIN_DECLINE_INFO_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: JoinDeclineInfo::Join.name(),
        label_key: "notification-button-join",
        is_default: true,
    },
    NotificationButton {
        name: JoinDeclineInfo::Decline.name(),
        label_key: "notification-button-decline",
        is_default: false,
    },
    NotificationButton {
        name: JoinDeclineInfo::Info.name(),
        label_key: "notification-button-info",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SUFFIXED_YES_NO_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: SuffixedOkCancel::Ok.name(),
        label_key: "notification-button-yes",
        is_default: false,
    },
    NotificationButton {
        name: SuffixedOkCancel::Cancel.name(),
        label_key: "notification-button-no",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const FREEZE_UNFREEZE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-freeze",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-unfreeze",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const EJECT_EJECT_AND_BAN_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNoCancel::Yes.name(),
        label_key: "notification-button-eject",
        is_default: true,
    },
    NotificationButton {
        name: YesNoCancel::No.name(),
        label_key: "notification-button-eject-and-ban",
        is_default: false,
    },
    NotificationButton {
        name: YesNoCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DONE_FORM: &[NotificationButton] = &[NotificationButton {
    name: Done::Done.name(),
    label_key: "notification-button-done",
    is_default: true,
}];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const PLAY_MEDIA_NOW_ALWAYS_PLAY_MEDIA_DO_NOT_PLAY_MEDIA_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: PlayMediaChoice::PlayNow.name(),
        label_key: "notification-button-play-media-now",
        is_default: true,
    },
    NotificationButton {
        name: PlayMediaChoice::AlwaysPlay.name(),
        label_key: "notification-button-always-play-media",
        is_default: false,
    },
    NotificationButton {
        name: PlayMediaChoice::DoNotPlay.name(),
        label_key: "notification-button-do-not-play-media",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const PLAY_DONT_PLAY_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: YesNo::Yes.name(),
        label_key: "notification-button-play",
        is_default: false,
    },
    NotificationButton {
        name: YesNo::No.name(),
        label_key: "notification-button-dont-play",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ENABLE_DISABLE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: EnableDisable::Enable.name(),
        label_key: "notification-button-enable",
        is_default: true,
    },
    NotificationButton {
        name: EnableDisable::Disable.name(),
        label_key: "notification-button-disable",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALLOW_DENY_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: AllowDeny::Allow.name(),
        label_key: "notification-button-allow",
        is_default: true,
    },
    NotificationButton {
        name: AllowDeny::Deny.name(),
        label_key: "notification-button-deny",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACTION_NOW_CONDITION_ALLOW_THIS_DOMAIN_CONDITION_ALLOW_THIS_URL_FORM:
    &[NotificationButton] = &[
    NotificationButton {
        name: DoNowRemember::DoNow.name(),
        label_key: "notification-button-action-now",
        is_default: true,
    },
    NotificationButton {
        name: DoNowRemember::RememberDomain.name(),
        label_key: "notification-button-condition-allow-this-domain",
        is_default: false,
    },
    NotificationButton {
        name: DoNowRemember::RememberUrl.name(),
        label_key: "notification-button-condition-allow-this-url",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALLOW_DENY_BLACKLIST_WHITELIST_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: AllowDenyBlacklistWhitelist::Allow.name(),
        label_key: "notification-button-allow",
        is_default: true,
    },
    NotificationButton {
        name: AllowDenyBlacklistWhitelist::Deny.name(),
        label_key: "notification-button-deny",
        is_default: false,
    },
    NotificationButton {
        name: AllowDenyBlacklistWhitelist::BlacklistDomain.name(),
        label_key: "notification-button-blacklist",
        is_default: false,
    },
    NotificationButton {
        name: AllowDenyBlacklistWhitelist::WhitelistDomain.name(),
        label_key: "notification-button-whitelist",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ADD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: AddCancel::Add.name(),
        label_key: "notification-button-add",
        is_default: true,
    },
    NotificationButton {
        name: AddCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OK_NO_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-no",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONFIRM_PURCHASE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: ConfirmPurchaseCancel::ConfirmPurchase.name(),
        label_key: "notification-button-ok",
        is_default: true,
    },
    NotificationButton {
        name: ConfirmPurchaseCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const PAY_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-pay",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const UPLOAD_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-upload",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CONTINUE_NAMED_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: ContinueNamedCancel::Continue.name(),
        label_key: "notification-button-continue",
        is_default: true,
    },
    NotificationButton {
        name: ContinueNamedCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DETAILS_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: DetailsCancel::Details.name(),
        label_key: "notification-button-details",
        is_default: true,
    },
    NotificationButton {
        name: DetailsCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const COPY_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-copy",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const REMOVE_ITEMS_AND_DELETE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-remove-items-and-delete",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const DELETE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-delete",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CHECK_TRASH_FOLDER_I_WILL_EMPTY_TRASH_LATER_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-check-trash-folder",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-i-will-empty-trash-later",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ACCEPT_DISCARD_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: KeepDiscard::Keep.name(),
        label_key: "notification-button-accept",
        is_default: true,
    },
    NotificationButton {
        name: KeepDiscard::Discard.name(),
        label_key: "notification-button-discard",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const SHOW_ACCEPT_DISCARD_PLUS4_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: ShowAcceptDiscard::Show.name(),
        label_key: "notification-button-show",
        is_default: true,
    },
    NotificationButton {
        name: ShowAcceptDiscard::Accept.name(),
        label_key: "notification-button-accept",
        is_default: false,
    },
    NotificationButton {
        name: ShowAcceptDiscard::Discard.name(),
        label_key: "notification-button-discard",
        is_default: false,
    },
    NotificationButton {
        name: ShowAcceptDiscard::ShowSilent.name(),
        label_key: "notification-button-show-2",
        is_default: false,
    },
    NotificationButton {
        name: ShowAcceptDiscard::AcceptSilent.name(),
        label_key: "notification-button-accept-2",
        is_default: false,
    },
    NotificationButton {
        name: ShowAcceptDiscard::DiscardSilent.name(),
        label_key: "notification-button-discard-2",
        is_default: false,
    },
    NotificationButton {
        name: ShowAcceptDiscard::Mute.name(),
        label_key: "notification-button-mute-sender",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const OKAY_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-okay",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const GO_TO_PAGE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-go-to-page",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const LATER_GO_NOW_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: LaterGoNow::Later.name(),
        label_key: "notification-button-later",
        is_default: true,
    },
    NotificationButton {
        name: LaterGoNow::GoNow.name(),
        label_key: "notification-button-go-now",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TRUST_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-trust",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TELEPORT_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-teleport",
        is_default: true,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const CHANGE_AND_CONTINUE_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: OkCancel::Ok.name(),
        label_key: "notification-button-change-and-continue",
        is_default: false,
    },
    NotificationButton {
        name: OkCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: true,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TELEPORT_NAMED_CANCEL_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: TeleportCancel::Teleport.name(),
        label_key: "notification-button-teleport",
        is_default: true,
    },
    NotificationButton {
        name: TeleportCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const TELEPORT_CHANGE_AND_CONTINUE_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: TeleportCancel::Teleport.name(),
        label_key: "notification-button-change-and-continue-2",
        is_default: true,
    },
    NotificationButton {
        name: TeleportCancel::Cancel.name(),
        label_key: "notification-button-cancel",
        is_default: false,
    },
];

/// Generated from the reference form it mirrors (see the entries
/// using it); stable functor names under localized labels.
pub const ALLOW_ALWAYS_ALLOW_DENY_FORM: &[NotificationButton] = &[
    NotificationButton {
        name: AllowAlwaysAllowDeny::Allow.name(),
        label_key: "notification-button-allow",
        is_default: true,
    },
    NotificationButton {
        name: AllowAlwaysAllowDeny::AlwaysAllow.name(),
        label_key: "notification-button-always-allow",
        is_default: false,
    },
    NotificationButton {
        name: AllowAlwaysAllowDeny::Deny.name(),
        label_key: "notification-button-deny",
        is_default: false,
    },
];
