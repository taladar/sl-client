//! The **declarative notification catalogue** and its runtime state
//! (`viewer-ui-notification-host`): the data model the toast / notification host
//! ([`crate::notification_host`]) is driven by.
//!
//! # Why a data catalogue, not code
//!
//! The reference viewer declares every notification as **data** — a
//! `notifications.xml` of ~1,300 `<notification>` elements, each mapping onto an
//! `LLNotificationTemplate` (name, type, icon, timeout, priority, persistence,
//! form buttons, a "don't show me again" checkbox). Nothing about *what a given
//! notification is* lives in C++; only *how to raise one* does. This module
//! mirrors that: [`NOTIFICATIONS`] is the catalogue, [`NotificationTemplate`] is
//! the per-entry shape, and the host reads the catalogue rather than hard-coding
//! a panel per alert. A new dialog ([[viewer-permission-request-dialog]],
//! [[viewer-dialog-offers-invites]], [[viewer-dialog-lldialog]]) adds a catalogue
//! entry and reuses the host, rather than growing a bespoke surface.
//!
//! # The pieces
//!
//! - [`NotificationKind`] — the rendering channel / behaviour class (reference
//!   `type`): a transient [`Tip`](NotificationKind::Tip), an informational
//!   [`Notify`](NotificationKind::Notify) toast, a sticky
//!   [`Alert`](NotificationKind::Alert), or a blocking
//!   [`AlertModal`](NotificationKind::AlertModal).
//! - [`NotificationTemplate`] + [`NOTIFICATIONS`] — the catalogue.
//! - [`NotificationArgs`] + [`substitute`] — the `[KEY]` substitution the
//!   reference does on a template's text (and the `AlertInfo` `ExtraParams`
//!   parser that feeds it from the wire).
//! - [`ShowNotification`] / [`NotificationResponse`] / [`DismissNotification`] —
//!   the messages a caller raises a notification with and reads a reply from,
//!   following the viewer's "emit a message, someone else acts" convention
//!   ([`crate::ui_element`]).
//! - [`NotificationManager`] — the host's runtime state: the id source, the
//!   `unique` dedup index, and the bounded history ring the future notification
//!   list / history panel ([[viewer-notification-history]]) renders.
//!
//! Everything here is pure data and logic (no Bevy world access beyond the
//! message / resource derives), so the catalogue lookup, the substitution and
//! the dedup are unit-tested directly. The rendering — stacking, timing out,
//! fading, dismissing — lives in [`crate::notification_host`].

use std::collections::{HashMap, VecDeque};

use bevy::prelude::{Message, Resource};

/// The rendering channel / behaviour class of a notification — the reference
/// `LLNotificationTemplate` `type`, narrowed to the four the host substrate
/// needs. The specific dialog tasks add their forms *on top* of these kinds
/// rather than new kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationKind {
    /// A transient information tip (`notifytip`): auto-fades on its timer, carries
    /// no buttons, never blocks. The reference `NotificationTipToastLifeTime`.
    Tip,
    /// An informational toast (`notify`): auto-fades on its (longer) timer, may
    /// carry buttons, non-blocking. The reference `NotificationToastLifeTime`.
    Notify,
    /// A non-modal alert that must be acknowledged (`alert`): a corner toast that
    /// **sticks** until a button is clicked rather than fading, but does not grey
    /// out the world behind it.
    Alert,
    /// A modal alert (`alertmodal`): a centred dialog over a scrim that blocks
    /// interaction with the world until a button is clicked. Never fades.
    AlertModal,
}

impl NotificationKind {
    /// Whether a toast of this kind auto-fades on its timer (tips and notifies)
    /// rather than sticking until it is clicked (alerts and modals).
    pub(crate) const fn fades(self) -> bool {
        matches!(self, Self::Tip | Self::Notify)
    }

    /// Whether this kind blocks the world behind a scrim: only
    /// [`AlertModal`](Self::AlertModal).
    pub(crate) const fn is_modal(self) -> bool {
        matches!(self, Self::AlertModal)
    }

    /// The on-screen lifetime before the fade begins, in seconds, or `0.0` for a
    /// kind that never auto-expires (alerts and modals wait for a click). The
    /// values mirror the reference `NotificationTipToastLifeTime` (10 s) and
    /// `NotificationToastLifeTime` (30 s).
    pub(crate) const fn lifetime_secs(self) -> f32 {
        match self {
            Self::Tip => 10.0,
            Self::Notify => 30.0,
            Self::Alert | Self::AlertModal => 0.0,
        }
    }
}

/// How long a toast takes to fade out after its lifetime elapses, in seconds —
/// the reference `ToastFadingTime`.
pub(crate) const TOAST_FADE_SECS: f32 = 2.0;

/// The gap between two stacked toasts, in logical pixels — the reference
/// `ToastGap`.
pub(crate) const TOAST_GAP: f32 = 8.0;

/// A notification's priority — the reference `LLNotificationPriority`. Ordered so
/// a higher-priority toast sorts to the more visible bottom of the stack (see
/// [`crate::notification_host`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NotificationPriority {
    /// No priority stated (`UNSPECIFIED`); the reference treats this as normal,
    /// and it is the least prominent in the stack.
    Unspecified,
    /// Low priority (`LOW`).
    Low,
    /// The default priority (`NORMAL`).
    Normal,
    /// High priority (`HIGH`) — the reference sets this on the handful of alerts
    /// that must not be missed.
    High,
    /// Critical priority (`CRITICAL`) — the single most urgent class.
    Critical,
}

/// One button on a notification's form — a `<button>` (or a `<usetemplate>`
/// slot) in the reference `notifications.xml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NotificationButton {
    /// The stable button name sent back as the [`NotificationResponse::button`] —
    /// the reference "functor button" name (`"OK"`, `"Cancel"`, `"Yes"`, …). Not
    /// translated: it is an identifier, not a label.
    pub(crate) name: &'static str,
    /// The Fluent key for the button's visible label, resolved through
    /// [`crate::i18n`] so the label localizes while the [`name`](Self::name)
    /// stays stable.
    pub(crate) label_key: &'static str,
    /// Whether this is the default button — the one chosen on `Enter` and on a
    /// toast's auto-expiry (the reference `expire_option`).
    pub(crate) is_default: bool,
}

/// The empty form — a notification with no buttons (a tip, or a bare
/// informational notify).
pub(crate) const NO_FORM: &[NotificationButton] = &[];

/// A one-button acknowledgement form (`OK`) — the reference `okbutton` template.
pub(crate) const OK_FORM: &[NotificationButton] = &[NotificationButton {
    name: "OK",
    label_key: "notification-button-ok",
    is_default: true,
}];

/// An OK / Cancel form — the reference `okcancelbuttons` template.
pub(crate) const OK_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const LEAVE_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const VIEW_IM_QUIT_FORM: &[NotificationButton] = &[
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
pub(crate) const YES_NO_FORM: &[NotificationButton] = &[
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
pub(crate) const SAVE_DISCARD_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const SAVE_ALL_DISCARD_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const DISCARD_KEEP_EDITING_FORM: &[NotificationButton] = &[
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
pub(crate) const SAVE_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const REMOVE_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const SEND_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const YES_NO_BUTTONS_FORM: &[NotificationButton] = &[
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
pub(crate) const THIS_ESTATE_ALL_ESTATES_FORM: &[NotificationButton] = &[
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
pub(crate) const KICK_ALL_RESIDENTS_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const OK_CANCEL_DONT_ASK_FORM: &[NotificationButton] = &[
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
pub(crate) const BAKE_CANCEL_FORM: &[NotificationButton] = &[
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
pub(crate) const REBAKE_CLOSE_FORM: &[NotificationButton] = &[
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
pub(crate) const REBAKE_REGION_FORM: &[NotificationButton] = &[NotificationButton {
    name: "OK",
    label_key: "notification-button-rebake-region",
    is_default: true,
}];

/// The replace-attachment prompt's buttons: the reference declares this form
/// explicitly with functor names `Yes` / `No` under `OK` / `Cancel` labels,
/// so those are the stable names a consumer routes on.
pub(crate) const REPLACE_ATTACHMENT_FORM: &[NotificationButton] = &[
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

/// A single-line text-input field on a notification's form — the reference
/// `<input>` element (the save-outfit / save-wearable / rename-outfit name
/// prompts). The host pre-fills the field with the resolved
/// [`default_key`](Self::default_key) text and returns the edited value on
/// [`NotificationResponse::input`] when a button is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NotificationInput {
    /// The stable field name a consumer routes on — the reference `<input
    /// name=…>` (`"message"`, `"new_name"`). An identifier, not a label.
    pub(crate) name: &'static str,
    /// The Fluent key for the pre-filled text, or `None` for a field that
    /// starts empty (the announcement prompts). Resolved through
    /// [`crate::i18n`], then `[KEY]`-substituted with the raised
    /// notification's [`NotificationArgs`] (the reference defaults are
    /// substitution templates like `[DESC] (new)`).
    pub(crate) default_key: Option<&'static str>,
}

/// A declarative notification template — one catalogue entry, mirroring the
/// reference `LLNotificationTemplate`. See the [module documentation](self).
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "these are independent catalogue flags mirroring the reference \
              LLNotificationTemplate (persist / log_to_chat / unique / ignore), not a state \
              machine that wants an enum"
)]
pub(crate) struct NotificationTemplate {
    /// The unique catalogue key (the reference `name`), matched by
    /// [`template`] and echoed on every [`NotificationResponse`].
    pub(crate) name: &'static str,
    /// The rendering channel / behaviour class.
    pub(crate) kind: NotificationKind,
    /// The Fluent key for the message body. Resolved through [`crate::i18n`],
    /// then `[KEY]`-substituted with the raised notification's
    /// [`NotificationArgs`]. A caller may override the resolved text entirely
    /// with [`ShowNotification::body`] (for an already-localized server string).
    pub(crate) message_key: &'static str,
    /// The Fluent key for an optional dialog title (the reference `label`) —
    /// rendered as a header line on an alert / modal card, and the
    /// human-readable name the history panel / preferences alerts tab can
    /// show out of context. `None` for the majority of entries, whose body
    /// is self-describing; a tip never carries one.
    pub(crate) title_key: Option<&'static str>,
    /// The priority (the reference `priority`) — drives the stack ordering.
    pub(crate) priority: NotificationPriority,
    /// Whether this notification persists in the notification well / across
    /// sessions (the reference `persist`). Carried for the history panel; the
    /// host does not itself persist toasts yet.
    pub(crate) persist: bool,
    /// Whether the body is also echoed into nearby chat (the reference
    /// `log_to_chat`).
    pub(crate) log_to_chat: bool,
    /// Whether at most one live instance may exist (the reference `<unique>`):
    /// raising a second, scoped by [`ShowNotification::context`], replaces the
    /// first rather than stacking a duplicate.
    pub(crate) unique: bool,
    /// Whether the form offers a "don't show me this again" checkbox (the
    /// reference `<ignore>` / `ignoretext`): ticking it records a suppression the
    /// host honours on the next raise, and which the Preferences alerts tab
    /// ([[viewer-preferences-alerts-tab]]) manages.
    pub(crate) ignorable: bool,
    /// The buttons the toast offers (the reference `<form>` / `<usetemplate>`).
    pub(crate) form: &'static [NotificationButton],
    /// An optional single-line text-input field (the reference `<input>`),
    /// shown between the body and the button row. Its edited text comes back
    /// on [`NotificationResponse::input`].
    pub(crate) input: Option<NotificationInput>,
}

impl NotificationTemplate {
    /// The default button's [`name`](NotificationButton::name) — chosen on
    /// `Enter` and on auto-expiry — or `None` when the form is empty.
    pub(crate) fn default_button(&self) -> Option<&'static str> {
        self.form
            .iter()
            .find(|button| button.is_default)
            .map(|button| button.name)
    }
}

/// **The catalogue.** Every notification the host can raise, declared as data.
///
/// A curated port of the reference's `notifications.xml` — not its full ~1,300
/// entries (each bespoke dialog owns its own form), but the notifications with
/// **no dialog of their own** that today fall back to the generic raw-string
/// `SystemMessage` / `GenericAlert`:
///
/// - The generic fallbacks and demo exemplars first (`SystemTip` …
///   `ConfirmQuit`) — one of each kind, exercising `[KEY]` substitution, the
///   `unique` dedup and the ignore checkbox.
/// - **Keyed server alerts** the simulator sends by `AlertInfo` key
///   ([`crate::notification_host::ingest_alert_messages`] raises these when the
///   key matches): the maturity / access-blocked family, the region-restart
///   countdowns, and standalone failure notices.
/// - **Standard action-confirmation modals** shared across features (empty
///   trash, remove friend, leave group, logged-out, agree-to-login) — raised by
///   their owning feature, but the *entry* (text, buttons) belongs here.
/// - **Info tips / notifies** not routed to nearby chat (landmark created,
///   granted-modify-rights, a help tip).
/// - **The appearance & wearables family**
///   (`viewer-notification-catalogue-appearance-wearables`): outfit / wearable
///   editing confirms, attachment prompts, the avatar-rez diagnostics tips and
///   the server-keyed attach / drop refusals.
/// - **The avatar movement family**
///   (`viewer-notification-catalogue-avatar-movement`): animation upload / AO
///   set management, movement-mode toggle tips and the server-keyed sit /
///   stand refusals.
/// - **The diagnostics family**
///   (`viewer-notification-catalogue-diagnostics`): installation / hardware
///   warnings, file-handling failures and the local-file watcher errors.
/// - **The estate & region management family**
///   (`viewer-notification-catalogue-estate-region`): region tools, terrain
///   validation, estate access lists / scope choosers, admin kick / freeze
///   prompts, pathfinding state and the server-keyed freeze / eject / entry
///   refusals.
///
/// See `viewer-notification-catalogue`.
pub(crate) const NOTIFICATIONS: &[NotificationTemplate] = &[
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
        ignorable: false,
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
        ignorable: false,
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
        ignorable: false,
        form: OK_FORM,
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
        ignorable: false,
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
        ignorable: true,
        form: OK_CANCEL_FORM,
        input: None,
    },
    // ---- Keyed server alerts (raised by `ingest_alert_messages` when the
    // simulator's `AlertInfo` key names one of these). ----
    //
    // The maturity / access-blocked family: the simulator blocks an entry, a
    // land claim or a land buy whose maturity rating exceeds the agent's
    // preference. The reference's `_Change` / `_AdultsOnlyContent` variants are
    // deferred (they carry a "change my preference and retry" callback that
    // needs the maturity-preference plumbing); this ports the plain refusals.
    NotificationTemplate {
        name: "RegionEntryAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-region-entry-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TeleportEntryAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-teleport-entry-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LandClaimAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-land-claim-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LandBuyAccessBlocked",
        kind: NotificationKind::AlertModal,
        message_key: "notification-land-buy-access-blocked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    // The non-modal maturity notice the reference shows on a soft block (a tip
    // that logs to chat), with the `[REGIONMATURITY]` substitution.
    NotificationTemplate {
        name: "RegionEntryAccessBlocked_Notify",
        kind: NotificationKind::Tip,
        message_key: "notification-region-entry-access-blocked-notify",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: true,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // The seconds-granularity restart countdown — the companion to the existing
    // `RegionRestartMinutes`, with `[NAME]` / `[SECONDS]`.
    NotificationTemplate {
        name: "RegionRestartSeconds",
        kind: NotificationKind::Alert,
        message_key: "notification-region-restart-seconds",
        title_key: None,
        priority: NotificationPriority::High,
        persist: true,
        log_to_chat: true,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    // Standalone failure notices the simulator sends by key.
    NotificationTemplate {
        name: "TooManyScripts",
        kind: NotificationKind::Notify,
        message_key: "notification-too-many-scripts",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FailedToPlaceObject",
        kind: NotificationKind::Notify,
        message_key: "notification-failed-to-place-object",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FailedToFindWearableUnnamed",
        kind: NotificationKind::Notify,
        message_key: "notification-failed-to-find-wearable",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "HomePositionSet",
        kind: NotificationKind::Notify,
        message_key: "notification-home-position-set",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: true,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // ---- Standard action-confirmation modals: shared `alertmodal` / `alert`
    // confirms raised by their owning feature (inventory / people / groups /
    // login flow). Data-only entries here; the owning feature supplies the
    // `[COUNT]` / `[NAME]` / `[GROUP]` arguments and reads the response. ----
    NotificationTemplate {
        name: "ConfirmEmptyTrash",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-empty-trash",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
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
        ignorable: false,
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
        ignorable: false,
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
        ignorable: false,
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
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    // ---- Info tips / notifies not routed to nearby chat. ----
    NotificationTemplate {
        name: "LandmarkCreated",
        kind: NotificationKind::Tip,
        message_key: "notification-landmark-created",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
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
        ignorable: false,
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
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // ---- Appearance & wearables (viewer-notification-catalogue-appearance-wearables). ----
    //
    // Outfit / wearable editing confirms and failures, raised by the
    // appearance / outfits features (data entries only; the owning
    // feature raises each and reads the response).
    NotificationTemplate {
        name: "WearableSave",
        kind: NotificationKind::AlertModal,
        message_key: "notification-wearable-save",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: SAVE_DISCARD_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "SaveClothingBodyChanges",
        kind: NotificationKind::AlertModal,
        message_key: "notification-save-clothing-body-changes",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: SAVE_ALL_DISCARD_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "UsavedWearableChanges",
        kind: NotificationKind::AlertModal,
        message_key: "notification-unsaved-wearable-changes",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: DISCARD_KEEP_EDITING_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AutoWearNewClothing",
        kind: NotificationKind::AlertModal,
        message_key: "notification-auto-wear-new-clothing",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: YES_NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "SaveWearableAs",
        kind: NotificationKind::AlertModal,
        message_key: "notification-save-wearable-as",
        title_key: Some("notification-title-save-wearable"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: Some("notification-save-wearable-as-default"),
        }),
    },
    NotificationTemplate {
        name: "SaveOutfitAs",
        kind: NotificationKind::AlertModal,
        message_key: "notification-save-outfit-as",
        title_key: Some("notification-title-save-outfit"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: Some("notification-save-outfit-as-default"),
        }),
    },
    NotificationTemplate {
        name: "RenameOutfit",
        kind: NotificationKind::AlertModal,
        message_key: "notification-rename-outfit",
        title_key: Some("notification-title-rename-outfit"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "new_name",
            default_key: Some("notification-rename-outfit-default"),
        }),
    },
    NotificationTemplate {
        name: "ConfirmOverwriteOutfit",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-overwrite-outfit",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: true,
        form: SAVE_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "DeleteOutfits",
        kind: NotificationKind::AlertModal,
        message_key: "notification-delete-outfits",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "DeleteOutfitsWithName",
        kind: NotificationKind::AlertModal,
        message_key: "notification-delete-outfits-with-name",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantDeleteRequiredClothing",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cant-delete-required-clothing",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MyOutfitsPasteFailed",
        kind: NotificationKind::AlertModal,
        message_key: "notification-my-outfits-paste-failed",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CouldNotPutOnOutfit",
        kind: NotificationKind::AlertModal,
        message_key: "notification-could-not-put-on-outfit",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotWearTrash",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-wear-trash",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotWearInfoNotComplete",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-wear-info-not-complete",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CanNotChangeAppearanceUntilLoaded",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-change-appearance-until-loaded",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ClothingLoading",
        kind: NotificationKind::AlertModal,
        message_key: "notification-clothing-loading",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TooManyWearables",
        kind: NotificationKind::AlertModal,
        message_key: "notification-too-many-wearables",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MaxAttachmentsOnOutfit",
        kind: NotificationKind::AlertModal,
        message_key: "notification-max-attachments-on-outfit",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotSaveWearableOutOfSpace",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-save-wearable-out-of-space",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotSaveToAssetStore",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-save-to-asset-store",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ThumbnailOutfitPhoto",
        kind: NotificationKind::AlertModal,
        message_key: "notification-thumbnail-outfit-photo",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "OutfitPhotoLoadError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-outfit-photo-load-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FSLargeOutfitsWarningInThisSession",
        kind: NotificationKind::AlertModal,
        message_key: "notification-large-outfits-warning",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: true,
        form: OK_FORM,
        input: None,
    },
    // Attachment prompts. `ReplaceAttachment`'s reference `save_option`
    // (remember my choice) is approximated by the plain suppress flag: a
    // suppressed raise shows nothing, rather than replaying the saved
    // answer (the `ConfirmQuit` precedent).
    NotificationTemplate {
        name: "AttachmentDrop",
        kind: NotificationKind::AlertModal,
        message_key: "notification-attachment-drop",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: YES_NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ReplaceAttachment",
        kind: NotificationKind::Alert,
        message_key: "notification-replace-attachment",
        title_key: Some("notification-title-replace-existing-attachment"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: REPLACE_ATTACHMENT_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RiggedMeshAttachedToHUD",
        kind: NotificationKind::AlertModal,
        message_key: "notification-rigged-mesh-attached-to-hud",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: OK_FORM,
        input: None,
    },
    // Appearance tips / notifies, including the avatar-rez diagnostic
    // family (the reference's cloud / bake progress reporting).
    NotificationTemplate {
        name: "CancelledAttach",
        kind: NotificationKind::Tip,
        message_key: "notification-cancelled-attach",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ReplacedMissingWearable",
        kind: NotificationKind::Tip,
        message_key: "notification-replaced-missing-wearable",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AttachmentSaved",
        kind: NotificationKind::Tip,
        message_key: "notification-attachment-saved",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FailedToFindWearable",
        kind: NotificationKind::Notify,
        message_key: "notification-failed-to-find-wearable-named",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidWearable",
        kind: NotificationKind::Notify,
        message_key: "notification-invalid-wearable",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AppearanceToXMLSaved",
        kind: NotificationKind::Notify,
        message_key: "notification-appearance-to-xml-saved",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AppearanceToXMLFailed",
        kind: NotificationKind::Tip,
        message_key: "notification-appearance-to-xml-failed",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ShapeImportGenericFail",
        kind: NotificationKind::Tip,
        message_key: "notification-shape-import-generic-fail",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ShapeImportVersionFail",
        kind: NotificationKind::Tip,
        message_key: "notification-shape-import-version-fail",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezSelfBakedDoneNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-self-baked-done",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezSelfBakedUpdateNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-self-baked-update",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezSelfBakeForceUpdateNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-self-bake-force-update",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezCloudNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-cloud",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezArrivedNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-arrived",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezLeftCloudNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-left-cloud",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezEnteredAppearanceNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-entered-appearance",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezLeftAppearanceNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-left-appearance",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezLeftNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-left",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezSelfBakedTextureUploadNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-self-baked-texture-upload",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarRezSelfBakedTextureUpdateNotification",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-rez-self-baked-texture-update",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // Server-keyed attach / drop refusals: the simulator sends these by
    // `AlertInfo` key, so `ingest_alert_messages` resolves them from the
    // catalogue automatically.
    NotificationTemplate {
        name: "NotEnoughResourcesToAttach",
        kind: NotificationKind::Notify,
        message_key: "notification-not-enough-resources-to-attach",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AttachmentHasTooMuchInventory",
        kind: NotificationKind::Notify,
        message_key: "notification-attachment-has-too-much-inventory",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "IllegalAttachment",
        kind: NotificationKind::Notify,
        message_key: "notification-illegal-attachment",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttackMultipleObjOneSpot",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-multiple-obj-one-spot",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NoPermsTooManyAttachedAnimatedObjects",
        kind: NotificationKind::Notify,
        message_key: "notification-no-perms-too-many-attached-animated-objects",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttachObjectAvatarSittingOnIt",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-object-avatar-sitting-on-it",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "WhyAreYouTryingToWearShrubbery",
        kind: NotificationKind::Notify,
        message_key: "notification-why-are-you-trying-to-wear-shrubbery",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttachGroupOwnedObjs",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-group-owned-objs",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttachObjectsNotOwned",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-objects-not-owned",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttachNavmeshObjects",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-navmesh-objects",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttachObjectNoMovePermissions",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-object-no-move-permissions",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttachNotEnoughScriptResources",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-not-enough-script-resources",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantAttachObjectBeingRemoved",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-attach-object-being-removed",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantDropItemTrialUser",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-drop-item-trial-user",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantDropMeshAttachment",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-drop-mesh-attachment",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantDropAttachmentNoPermission",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-drop-attachment-no-permission",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantDropAttachmentInsufficientLandResources",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-drop-attachment-insufficient-land-resources",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantDropAttachmentInsufficientResources",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-drop-attachment-insufficient-resources",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantDropObjectFullParcel",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-drop-object-full-parcel",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantCreateOutfit",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-create-outfit",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // ---- Avatar movement (viewer-notification-catalogue-avatar-movement). ----
    //
    // Animation-upload failures and the animation-overrider (AO) set
    // management prompts, raised by the animation / AO features.
    NotificationTemplate {
        name: "WriteAnimationFail",
        kind: NotificationKind::AlertModal,
        message_key: "notification-write-animation-fail",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "DoNotSupportBulkAnimationUpload",
        kind: NotificationKind::AlertModal,
        message_key: "notification-do-not-support-bulk-animation-upload",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NewAOSet",
        kind: NotificationKind::AlertModal,
        message_key: "notification-new-ao-set",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: Some("notification-new-ao-set-default"),
        }),
    },
    NotificationTemplate {
        name: "NewAOCantContainNonASCII",
        kind: NotificationKind::AlertModal,
        message_key: "notification-new-ao-cant-contain-non-ascii",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RenameAOMustBeASCII",
        kind: NotificationKind::AlertModal,
        message_key: "notification-rename-ao-must-be-ascii",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NewAONameCantExist",
        kind: NotificationKind::AlertModal,
        message_key: "notification-new-ao-name-cant-exist",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RemoveAOSet",
        kind: NotificationKind::AlertModal,
        message_key: "notification-remove-ao-set",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: REMOVE_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOForeignItemsFound",
        kind: NotificationKind::AlertModal,
        message_key: "notification-ao-foreign-items-found",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ConfirmPoserOverwrite",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-poser-overwrite",
        title_key: Some("notification-title-confirm-pose-overwrite"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    // Movement notifies: the scripted-control notice and the
    // server-keyed sit / stand refusals (`AlertInfo` keys, resolved by
    // `ingest_alert_messages` automatically).
    NotificationTemplate {
        name: "FirstOverrideKeys",
        kind: NotificationKind::Notify,
        message_key: "notification-first-override-keys",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "SitFailCantMove",
        kind: NotificationKind::Notify,
        message_key: "notification-sit-fail-cant-move",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "SitFailNotAllowedOnLand",
        kind: NotificationKind::Notify,
        message_key: "notification-sit-fail-not-allowed-on-land",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "SitFailNotSameRegion",
        kind: NotificationKind::Notify,
        message_key: "notification-sit-fail-not-same-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "StandDeniedByObject",
        kind: NotificationKind::Notify,
        message_key: "notification-stand-denied-by-object",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ResitDeniedByObject",
        kind: NotificationKind::Notify,
        message_key: "notification-resit-denied-by-object",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantSitNoSuitableSurface",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-sit-no-suitable-surface",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantSitNoRoom",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-sit-no-room",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // AO notecard-import progress tips and the movement-mode toggle
    // tips (phantom / movelock / flight assist).
    NotificationTemplate {
        name: "AOImportComplete",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-complete",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportSetAlreadyExists",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-set-already-exists",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportPermissionDenied",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-permission-denied",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportCreateSetFailed",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-create-set-failed",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportDownloadFailed",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-download-failed",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportNoText",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-no-text",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportNoFolder",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-no-folder",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportNoStatePrefix",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-no-state-prefix",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportNoValidDelimiter",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-no-valid-delimiter",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportStateNameNotFound",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-state-name-not-found",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportAnimationNotFound",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-animation-not-found",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportInvalid",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-invalid",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportRetryCreateSet",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-retry-create-set",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportAbortCreateSet",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-abort-create-set",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AOImportLinkFailed",
        kind: NotificationKind::Tip,
        message_key: "notification-ao-import-link-failed",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "PhantomOn",
        kind: NotificationKind::Tip,
        message_key: "notification-phantom-on",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "PhantomOff",
        kind: NotificationKind::Tip,
        message_key: "notification-phantom-off",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MovelockEnabled",
        kind: NotificationKind::Tip,
        message_key: "notification-movelock-enabled",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MovelockDisabled",
        kind: NotificationKind::Tip,
        message_key: "notification-movelock-disabled",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MovelockEnabling",
        kind: NotificationKind::Tip,
        message_key: "notification-movelock-enabling",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MovelockDisabling",
        kind: NotificationKind::Tip,
        message_key: "notification-movelock-disabling",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FlightAssistEnabled",
        kind: NotificationKind::Tip,
        message_key: "notification-flight-assist-enabled",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // ---- Diagnostics (viewer-notification-catalogue-diagnostics). ----
    //
    // Installation / environment / hardware warnings and viewer-internal
    // errors. The hardware confirms (`UnsupportedHardware`,
    // `OldGPUDriver`) carry a visit-the-URL affirmative; the URL-opening
    // action belongs to the raising feature.
    NotificationTemplate {
        name: "MissingAlert",
        kind: NotificationKind::Notify,
        message_key: "notification-missing-alert",
        title_key: Some("notification-title-unknown-notification-message"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FloaterNotFound",
        kind: NotificationKind::AlertModal,
        message_key: "notification-floater-not-found",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "BadInstallation",
        kind: NotificationKind::AlertModal,
        message_key: "notification-bad-installation",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FoundLegacyNsisInstallation",
        kind: NotificationKind::AlertModal,
        message_key: "notification-found-legacy-nsis-installation",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MessageTemplateNotFound",
        kind: NotificationKind::AlertModal,
        message_key: "notification-message-template-not-found",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AllowMultipleViewers",
        kind: NotificationKind::AlertModal,
        message_key: "notification-allow-multiple-viewers",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "UnsupportedHardware",
        kind: NotificationKind::AlertModal,
        message_key: "notification-unsupported-hardware",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: YES_NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "OldGPUDriver",
        kind: NotificationKind::AlertModal,
        message_key: "notification-old-gpu-driver",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: YES_NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "UnknownGPU",
        kind: NotificationKind::AlertModal,
        message_key: "notification-unknown-gpu",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "DisplaySettingsNoShaders",
        kind: NotificationKind::AlertModal,
        message_key: "notification-display-settings-no-shaders",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NoHavok",
        kind: NotificationKind::AlertModal,
        message_key: "notification-no-havok",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NoSupportGLTFShader",
        kind: NotificationKind::Notify,
        message_key: "notification-no-support-gltf-shader",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LowMemory",
        kind: NotificationKind::AlertModal,
        message_key: "notification-low-memory",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ForceQuitDueToLowMemory",
        kind: NotificationKind::AlertModal,
        message_key: "notification-force-quit-due-to-low-memory",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "OutOfDiskSpace",
        kind: NotificationKind::Tip,
        message_key: "notification-out-of-disk-space",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RegionCapabilityRequestError",
        kind: NotificationKind::Alert,
        message_key: "notification-region-capability-request-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MissingString",
        kind: NotificationKind::AlertModal,
        message_key: "notification-missing-string",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FailedRequirementsCheck",
        kind: NotificationKind::AlertModal,
        message_key: "notification-failed-requirements-check",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CompressionTestResults",
        kind: NotificationKind::AlertModal,
        message_key: "notification-compression-test-results",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "SendSysinfoToIM",
        kind: NotificationKind::AlertModal,
        message_key: "notification-send-sysinfo-to-im",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: SEND_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FireStormReqInfo",
        kind: NotificationKind::AlertModal,
        message_key: "notification-firestorm-req-info",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: YES_NO_BUTTONS_FORM,
        input: None,
    },
    // File-handling failures (upload / resource / generic file I/O).
    NotificationTemplate {
        name: "CannotWriteFile",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-write-file",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NoFileExtension",
        kind: NotificationKind::AlertModal,
        message_key: "notification-no-file-extension",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidFileExtension",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-file-extension",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ProblemWithFile",
        kind: NotificationKind::AlertModal,
        message_key: "notification-problem-with-file",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotEncodeFile",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-encode-file",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CorruptResourceFile",
        kind: NotificationKind::AlertModal,
        message_key: "notification-corrupt-resource-file",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "UnknownResourceFileVersion",
        kind: NotificationKind::AlertModal,
        message_key: "notification-unknown-resource-file-version",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "UnableToCreateOutputFile",
        kind: NotificationKind::AlertModal,
        message_key: "notification-unable-to-create-output-file",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotUploadReason",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-upload-reason",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotOpenFileTooBig",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-open-file-too-big",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CannotLoad",
        kind: NotificationKind::AlertModal,
        message_key: "notification-cannot-load",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NotRegularFileError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-not-regular-file-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NotFolderError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-not-folder-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GenericFileEmptyError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-generic-file-empty-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GenericFileOpenReadError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-generic-file-open-read-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GenericFileOpenWriteError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-generic-file-open-write-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GenericFileReadError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-generic-file-read-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GenericFileWriteError",
        kind: NotificationKind::AlertModal,
        message_key: "notification-generic-file-write-error",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    // Local-file live-update failures (the local bitmaps / GLTF
    // preview watchers), persistent notifies in the reference.
    NotificationTemplate {
        name: "LocalBitmapsUpdateFileNotFound",
        kind: NotificationKind::Notify,
        message_key: "notification-local-bitmaps-update-file-not-found",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LocalBitmapsUpdateFailedFinal",
        kind: NotificationKind::Notify,
        message_key: "notification-local-bitmaps-update-failed-final",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LocalBitmapsVerifyFail",
        kind: NotificationKind::Notify,
        message_key: "notification-local-bitmaps-verify-fail",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LocalGLTFVerifyFail",
        kind: NotificationKind::Notify,
        message_key: "notification-local-gltf-verify-fail",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // ---- Estate & region management (viewer-notification-catalogue-estate-region). ----
    //
    // Region tools: top-object return / disable, terraforming, map
    // cache, terrain raw upload / download / bake, restart and the
    // region-wide announcement prompt.
    NotificationTemplate {
        name: "ReturnAllTopObjects",
        kind: NotificationKind::AlertModal,
        message_key: "notification-return-all-top-objects",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "DisableAllTopObjects",
        kind: NotificationKind::AlertModal,
        message_key: "notification-disable-all-top-objects",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "UnableToDisableOutsideScripts",
        kind: NotificationKind::AlertModal,
        message_key: "notification-unable-to-disable-outside-scripts",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RegionNoTerraforming",
        kind: NotificationKind::AlertModal,
        message_key: "notification-region-no-terraforming",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FlushMapVisibilityCaches",
        kind: NotificationKind::AlertModal,
        message_key: "notification-flush-map-visibility-caches",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "KickUsersFromRegion",
        kind: NotificationKind::AlertModal,
        message_key: "notification-kick-users-from-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ChangeObjectBonusFactor",
        kind: NotificationKind::AlertModal,
        message_key: "notification-change-object-bonus-factor",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: true,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateObjectReturn",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-object-return",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RawUploadStarted",
        kind: NotificationKind::AlertModal,
        message_key: "notification-raw-upload-started",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ConfirmBakeTerrain",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-bake-terrain",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ConfirmTextureHeights",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-texture-heights",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_DONT_ASK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FinishedRawDownload",
        kind: NotificationKind::AlertModal,
        message_key: "notification-finished-raw-download",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RegionMaturityChange",
        kind: NotificationKind::AlertModal,
        message_key: "notification-region-maturity-change",
        title_key: Some("notification-title-changed-region-maturity"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ConfirmRestart",
        kind: NotificationKind::Alert,
        message_key: "notification-confirm-restart",
        title_key: Some("notification-title-confirm-restart"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MessageRegion",
        kind: NotificationKind::Alert,
        message_key: "notification-message-region",
        title_key: Some("notification-title-message-everyone-in-this-region"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: None,
        }),
    },
    // Terrain texture / material validation failures (`unique
    // combine="cancel_old"` in the reference; our unique-replace
    // dedup has the same effect).
    NotificationTemplate {
        name: "InvalidTerrainBitDepth",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-bit-depth",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidTerrainAlphaNotFullyLoaded",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-alpha-not-fully-loaded",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidTerrainAlpha",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-alpha",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidTerrainSize",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-size",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidTerrainMaterialNotLoaded",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-material-not-loaded",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidTerrainMaterialLoadFailed",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-material-load-failed",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidTerrainMaterialDoubleSided",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-material-double-sided",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "InvalidTerrainMaterialAlphaMode",
        kind: NotificationKind::AlertModal,
        message_key: "notification-invalid-terrain-material-alpha-mode",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    // Estate access-list management results and shared estate
    // confirms.
    NotificationTemplate {
        name: "MaxAllowedAgentOnRegion",
        kind: NotificationKind::AlertModal,
        message_key: "notification-max-allowed-agent-on-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MaxBannedAgentsOnRegion",
        kind: NotificationKind::AlertModal,
        message_key: "notification-max-banned-agents-on-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MaxAgentOnRegionBatch",
        kind: NotificationKind::AlertModal,
        message_key: "notification-max-agent-on-region-batch",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MaxAllowedGroupsOnRegion",
        kind: NotificationKind::AlertModal,
        message_key: "notification-max-allowed-groups-on-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: BAKE_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MaxManagersOnRegion",
        kind: NotificationKind::AlertModal,
        message_key: "notification-max-managers-on-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "OwnerCanNotBeDenied",
        kind: NotificationKind::AlertModal,
        message_key: "notification-owner-cannot-be-denied",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ProblemAddingEstateManagerBanned",
        kind: NotificationKind::AlertModal,
        message_key: "notification-problem-adding-estate-manager-banned",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ProblemBanningEstateManager",
        kind: NotificationKind::AlertModal,
        message_key: "notification-problem-banning-estate-manager",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GroupIsAlreadyInList",
        kind: NotificationKind::AlertModal,
        message_key: "notification-group-is-already-in-list",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AgentIsAlreadyInList",
        kind: NotificationKind::AlertModal,
        message_key: "notification-agent-is-already-in-list",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AgentsAreAlreadyInList",
        kind: NotificationKind::AlertModal,
        message_key: "notification-agents-are-already-in-list",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AgentWasAddedToList",
        kind: NotificationKind::AlertModal,
        message_key: "notification-agent-was-added-to-list",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AgentsWereAddedToList",
        kind: NotificationKind::AlertModal,
        message_key: "notification-agents-were-added-to-list",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AgentWasRemovedFromList",
        kind: NotificationKind::AlertModal,
        message_key: "notification-agent-was-removed-from-list",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AgentsWereRemovedFromList",
        kind: NotificationKind::AlertModal,
        message_key: "notification-agents-were-removed-from-list",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ProblemImportingEstateCovenant",
        kind: NotificationKind::AlertModal,
        message_key: "notification-problem-importing-estate-covenant",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ProblemAddingEstateManager",
        kind: NotificationKind::AlertModal,
        message_key: "notification-problem-adding-estate-manager",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ProblemAddingEstateBanManager",
        kind: NotificationKind::AlertModal,
        message_key: "notification-problem-adding-estate-ban-manager",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ProblemAddingEstateGeneric",
        kind: NotificationKind::AlertModal,
        message_key: "notification-problem-adding-estate-generic",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateParcelAccessOverride",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-parcel-access-override",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateParcelEnvironmentOverride",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-parcel-environment-override",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateChangeCovenant",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-change-covenant",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RegionEntryAccessBlocked_PreferencesOutOfSync",
        kind: NotificationKind::AlertModal,
        message_key: "notification-region-entry-access-blocked-preferences-out-of-sync",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    // Grid-admin / god tools: kick / freeze prompts with an editable
    // message, the estate announcement, and the Linden-estate
    // safeguards.
    NotificationTemplate {
        name: "ConfirmKick",
        kind: NotificationKind::Alert,
        message_key: "notification-confirm-kick",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: KICK_ALL_RESIDENTS_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "KickUser",
        kind: NotificationKind::Alert,
        message_key: "notification-kick-user",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: Some("notification-kick-user-default"),
        }),
    },
    NotificationTemplate {
        name: "KickAllUsers",
        kind: NotificationKind::Alert,
        message_key: "notification-kick-all-users",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: Some("notification-kick-user-default"),
        }),
    },
    NotificationTemplate {
        name: "FreezeUser",
        kind: NotificationKind::Alert,
        message_key: "notification-freeze-user",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: Some("notification-freeze-user-default"),
        }),
    },
    NotificationTemplate {
        name: "UnFreezeUser",
        kind: NotificationKind::Alert,
        message_key: "notification-unfreeze-user",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: Some("notification-unfreeze-user-default"),
        }),
    },
    NotificationTemplate {
        name: "MessageEstate",
        kind: NotificationKind::Alert,
        message_key: "notification-message-estate",
        title_key: Some("notification-title-message-everyone-in-your-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: None,
        }),
    },
    NotificationTemplate {
        name: "ChangeLindenEstate",
        kind: NotificationKind::Alert,
        message_key: "notification-change-linden-estate",
        title_key: Some("notification-title-change-linden-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ChangeLindenAccess",
        kind: NotificationKind::Alert,
        message_key: "notification-change-linden-access",
        title_key: Some("notification-title-change-linden-estate-access"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    // The estate-scope choosers: apply an access-list / manager /
    // experience change to this estate only or to all the owner's
    // estates. The Remove variants are unique modals in the reference.
    NotificationTemplate {
        name: "EstateAllowedAgentAdd",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-allowed-agent-add",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateAllowedAgentRemove",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-allowed-agent-remove",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateAllowedGroupAdd",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-allowed-group-add",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateAllowedGroupRemove",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-allowed-group-remove",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateBannedAgentAdd",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-banned-agent-add",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateBannedAgentRemove",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-banned-agent-remove",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateManagerAdd",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-manager-add",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateManagerRemove",
        kind: NotificationKind::AlertModal,
        message_key: "notification-estate-manager-remove",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateAllowedExperienceAdd",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-allowed-experience-add",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateAllowedExperienceRemove",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-allowed-experience-remove",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateBlockedExperienceAdd",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-blocked-experience-add",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateBlockedExperienceRemove",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-blocked-experience-remove",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateTrustedExperienceAdd",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-trusted-experience-add",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateTrustedExperienceRemove",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-trusted-experience-remove",
        title_key: Some("notification-title-select-estate"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateBanUser",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-ban-user",
        title_key: Some("notification-title-confirm-ban"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateBanUserMultiple",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-ban-user-multiple",
        title_key: Some("notification-title-confirm-ban"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: THIS_ESTATE_ALL_ESTATES_FORM,
        input: None,
    },
    // Estate kick / teleport-home confirms.
    NotificationTemplate {
        name: "EstateKickUser",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-kick-user",
        title_key: Some("notification-title-confirm-kick"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateKickMultiple",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-kick-multiple",
        title_key: Some("notification-title-confirm-kick"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateTeleportHomeUser",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-teleport-home-user",
        title_key: Some("notification-title-confirm-teleport-home"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateTeleportHomeMultiple",
        kind: NotificationKind::Alert,
        message_key: "notification-estate-teleport-home-multiple",
        title_key: Some("notification-title-confirm-teleport-home"),
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: None,
    },
    // Pathfinding region state.
    NotificationTemplate {
        name: "PathfindingDirty",
        kind: NotificationKind::AlertModal,
        message_key: "notification-pathfinding-dirty",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: REBAKE_CLOSE_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "PathfindingDirtyRebake",
        kind: NotificationKind::Notify,
        message_key: "notification-pathfinding-dirty-rebake",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: REBAKE_REGION_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "DynamicPathfindingDisabled",
        kind: NotificationKind::Notify,
        message_key: "notification-dynamic-pathfinding-disabled",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "PathfindingCannotRebakeNavmesh",
        kind: NotificationKind::AlertModal,
        message_key: "notification-pathfinding-cannot-rebake-navmesh",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_FORM,
        input: None,
    },
    // Server-keyed region-entry refusals, restart toasts and the
    // freeze / eject / terrain-tool feedback (`AlertInfo` keys,
    // resolved by `ingest_alert_messages` automatically).
    NotificationTemplate {
        name: "RegionAboutToShutdown",
        kind: NotificationKind::Notify,
        message_key: "notification-region-about-to-shutdown",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "URBannedFromRegion",
        kind: NotificationKind::Notify,
        message_key: "notification-ur-banned-from-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NoTeenGridAccess",
        kind: NotificationKind::Notify,
        message_key: "notification-no-teen-grid-access",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ImproperPaymentStatus",
        kind: NotificationKind::Notify,
        message_key: "notification-improper-payment-status",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "MustGetAgeRegion",
        kind: NotificationKind::Notify,
        message_key: "notification-must-get-age-region",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: true,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RegionRestartMinutesToast",
        kind: NotificationKind::Notify,
        message_key: "notification-region-restart-minutes-toast",
        title_key: None,
        priority: NotificationPriority::High,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "RegionRestartSecondsToast",
        kind: NotificationKind::Notify,
        message_key: "notification-region-restart-seconds-toast",
        title_key: None,
        priority: NotificationPriority::High,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarFrozen",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-frozen",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarFrozenDuration",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-frozen-duration",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "YouFrozeAvatar",
        kind: NotificationKind::Notify,
        message_key: "notification-you-froze-avatar",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarHasUnFrozenYou",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-has-unfrozen-you",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarUnFrozen",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-unfrozen",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarFreezeFailure",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-freeze-failure",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarFreezeThaw",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-freeze-thaw",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarCantFreeze",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-cant-freeze",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EjectComingSoon",
        kind: NotificationKind::Notify,
        message_key: "notification-eject-coming-soon",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "NoEnterRegionMaybeFull",
        kind: NotificationKind::Notify,
        message_key: "notification-no-enter-region-maybe-full",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "SorryCantEjectUser",
        kind: NotificationKind::Notify,
        message_key: "notification-sorry-cant-eject-user",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarEjectFailed",
        kind: NotificationKind::Notify,
        message_key: "notification-avatar-eject-failed",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "FullRegionCantEnter",
        kind: NotificationKind::Notify,
        message_key: "notification-full-region-cant-enter",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EstateManagerFailedllTeleportHome",
        kind: NotificationKind::Notify,
        message_key: "notification-estate-manager-failed-ll-teleport-home",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "CantTeleportCouldNotFindUser",
        kind: NotificationKind::Notify,
        message_key: "notification-cant-teleport-could-not-find-user",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TerrainUploadFailed",
        kind: NotificationKind::Notify,
        message_key: "notification-terrain-upload-failed",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TerrainFileWritten",
        kind: NotificationKind::Notify,
        message_key: "notification-terrain-file-written",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TerrainFileWrittenStartingDownload",
        kind: NotificationKind::Notify,
        message_key: "notification-terrain-file-written-starting-download",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TerrainBaked",
        kind: NotificationKind::Notify,
        message_key: "notification-terrain-baked",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "GodBeatsFreeze",
        kind: NotificationKind::Notify,
        message_key: "notification-god-beats-freeze",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    // Estate / region tips.
    NotificationTemplate {
        name: "RegionEntryAccessBlocked_NotifyAdultsOnly",
        kind: NotificationKind::Tip,
        message_key: "notification-region-entry-access-blocked-notify-adults-only",
        title_key: None,
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: true,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "TerrainDownloaded",
        kind: NotificationKind::Tip,
        message_key: "notification-terrain-downloaded",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "EnteringGodMode",
        kind: NotificationKind::Tip,
        message_key: "notification-entering-god-mode",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "LeavingGodMode",
        kind: NotificationKind::Tip,
        message_key: "notification-leaving-god-mode",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "AvatarEjected",
        kind: NotificationKind::Tip,
        message_key: "notification-avatar-ejected",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
    NotificationTemplate {
        name: "ServerVersionChanged",
        kind: NotificationKind::Tip,
        message_key: "notification-server-version-changed",
        title_key: None,
        priority: NotificationPriority::Low,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: NO_FORM,
        input: None,
    },
];

/// Look up a catalogue [`NotificationTemplate`] by its [`name`](NotificationTemplate::name).
pub(crate) fn template(name: &str) -> Option<&'static NotificationTemplate> {
    NOTIFICATIONS
        .iter()
        .find(|candidate| candidate.name == name)
}

/// The settings section under which each ignorable notification's "show again"
/// flag lives (`[notifications]` in the persisted file). A `Bool(false)`
/// override suppresses the named notification; the default is `Bool(true)`
/// (show). The Preferences alerts tab ([[viewer-preferences-alerts-tab]]) is the
/// UI over these flags.
pub(crate) const NOTIFICATIONS_SECTION: &str = "notifications";

/// The `[KEY]` substitution arguments for a notification's message — the
/// reference `LLNotification` substitutions, fed from a keyed `AlertInfo`'s
/// `ExtraParams` on the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NotificationArgs {
    /// The key / value bindings, in insertion order (so a rebuild is stable).
    pairs: Vec<(String, String)>,
}

impl NotificationArgs {
    /// An empty argument set.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Bind `key` to `value`, replacing any existing binding for that key.
    pub(crate) fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        if let Some(existing) = self.pairs.iter_mut().find(|(name, _value)| *name == key) {
            existing.1 = value.into();
        } else {
            self.pairs.push((key, value.into()));
        }
    }

    /// The value bound to `key`, if any.
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(name, _value)| name == key)
            .map(|(_name, value)| value.as_str())
    }

    /// The key/value bindings, in insertion order — the persistent-notification
    /// store ([`crate::notification_persist`]) serializes these to re-raise a
    /// persisted notification with its original substitutions.
    pub(crate) fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// Rebuild an argument set from serialized [`pairs`](Self::pairs) — the inverse
    /// used when a persisted notification is reloaded from disk.
    pub(crate) const fn from_pairs(pairs: Vec<(String, String)>) -> Self {
        Self { pairs }
    }

    /// Parse an `AlertInfo` `ExtraParams` blob into arguments: `key=value` pairs
    /// separated by `|` or newlines, each side trimmed. A fragment without an
    /// `=` is ignored. The reference parses this per-alert; this handles the
    /// common `key=value` form.
    pub(crate) fn parse_extra_params(blob: &str) -> Self {
        let mut args = Self::new();
        for fragment in blob.split(['|', '\n']) {
            let fragment = fragment.trim();
            if fragment.is_empty() {
                continue;
            }
            if let Some((key, value)) = fragment.split_once('=') {
                args.set(key.trim(), value.trim());
            }
        }
        args
    }
}

/// Replace every `[KEY]` placeholder in `template` with its bound value from
/// `args`. An unbound placeholder is left verbatim (`[KEY]`), matching the
/// reference behaviour where a missing substitution shows the bracketed token
/// rather than an empty string — a visible signal that a value was expected.
pub(crate) fn substitute(template: &str, args: &NotificationArgs) -> String {
    let mut out = String::with_capacity(template.len());
    let mut key = String::new();
    let mut in_token = false;
    for character in template.chars() {
        if in_token {
            if character == ']' {
                if let Some(value) = args.get(&key) {
                    out.push_str(value);
                } else {
                    out.push('[');
                    out.push_str(&key);
                    out.push(']');
                }
                key.clear();
                in_token = false;
            } else {
                key.push(character);
            }
        } else if character == '[' {
            in_token = true;
        } else {
            out.push(character);
        }
    }
    // An unterminated `[` — emit the buffered text verbatim rather than dropping
    // it, so no message content is ever silently lost.
    if in_token {
        out.push('[');
        out.push_str(&key);
    }
    out
}

/// A monotonic identifier for one raised notification instance, so a caller can
/// match a [`NotificationResponse`] (or issue a [`DismissNotification`]) to the
/// exact notification it raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NotificationId(u64);

/// A request to raise a notification from the catalogue — the message a caller
/// writes; the host ([`crate::notification_host`]) reads it, resolves the
/// template's text, and stacks a toast.
#[derive(Message, Debug, Clone)]
pub(crate) struct ShowNotification {
    /// The catalogue template [`name`](NotificationTemplate::name). A raise for a
    /// name not in [`NOTIFICATIONS`] is dropped (logged), so a typo fails loudly
    /// rather than silently.
    pub(crate) template: &'static str,
    /// The `[KEY]` substitution arguments for the template's message.
    pub(crate) args: NotificationArgs,
    /// An already-localized body to show verbatim instead of resolving the
    /// template's [`message_key`](NotificationTemplate::message_key) — for a
    /// plain server `AlertMessage` string that arrives pre-translated.
    pub(crate) body: Option<String>,
    /// A context string that scopes the `unique` dedup: two raises of a unique
    /// template with **different** contexts coexist, with the **same** context
    /// the second replaces the first (the reference `<unique><context>`).
    pub(crate) context: Option<String>,
}

impl ShowNotification {
    /// Raise the catalogue template `name` with no arguments, body override or
    /// context — the common case.
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            template: name,
            args: NotificationArgs::new(),
            body: None,
            context: None,
        }
    }

    /// Builder: set a `[KEY]` substitution argument.
    #[must_use]
    pub(crate) fn arg(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.args.set(key, value);
        self
    }

    /// Builder: override the resolved body with an already-localized string.
    #[must_use]
    pub(crate) fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Builder: scope the `unique` dedup with a context string.
    #[must_use]
    pub(crate) fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// A user (or automatic) response to a raised notification — the message the
/// host writes when a button is clicked, a fading toast expires, or a
/// notification is dismissed. A consumer (a specific dialog task) reads it to
/// send the corresponding protocol reply.
#[derive(Message, Debug, Clone)]
pub(crate) struct NotificationResponse {
    /// The notification this responds to.
    pub(crate) id: NotificationId,
    /// The catalogue template name, so a consumer can route without tracking the
    /// [`id`](Self::id).
    pub(crate) template: &'static str,
    /// The chosen button's [`name`](NotificationButton::name), or `None` when the
    /// toast expired or was dismissed without a choice (a fading tip, or an
    /// external [`DismissNotification`]).
    pub(crate) button: Option<&'static str>,
    /// Whether the "don't show me this again" checkbox was ticked — the host has
    /// already recorded the suppression; a consumer may act on it too.
    pub(crate) ignored: bool,
    /// The text-input field's edited value, for a template with a
    /// [`NotificationTemplate::input`] field — `None` for an inputless
    /// template (or when the toast was dismissed without resolving).
    pub(crate) input: Option<String>,
}

/// A request to dismiss a live notification programmatically (its underlying
/// condition passed, e.g. an offer was rescinded). Tears the toast down and
/// emits a [`NotificationResponse`] with no [`button`](NotificationResponse::button).
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct DismissNotification {
    /// The notification to dismiss.
    pub(crate) id: NotificationId,
}

/// One entry in the notification history — the data the future notification list
/// / history panel ([[viewer-notification-history]]) renders. Recorded when a
/// notification is raised; its [`response`](Self::response) is filled in when the
/// user answers.
#[derive(Debug, Clone)]
pub(crate) struct NotificationRecord {
    /// The raised notification's id.
    pub(crate) id: NotificationId,
    /// The catalogue template name.
    pub(crate) template: &'static str,
    /// The kind (channel) it was shown on.
    pub(crate) kind: NotificationKind,
    /// The resolved, display-ready body text.
    pub(crate) body: String,
    /// The chosen button once answered, or `None` while still live or if it
    /// expired / was dismissed without a choice.
    pub(crate) response: Option<&'static str>,
}

/// The most history entries kept: a bounded ring, so a long session's toasts do
/// not grow without bound. Older entries drop off the front.
const HISTORY_CAP: usize = 256;

/// The host's runtime state: the id source, the `unique` dedup index, and the
/// bounded history ring. Rendering state lives on the toast entities themselves
/// (see [`crate::notification_host`]); this resource holds only what is not
/// per-entity.
#[derive(Resource, Debug, Default)]
pub(crate) struct NotificationManager {
    /// The next id [`allocate_id`](Self::allocate_id) hands out.
    next_id: u64,
    /// Live `unique` notifications, keyed by template name + context, so a repeat
    /// can find and replace its predecessor.
    unique_live: HashMap<String, NotificationId>,
    /// The bounded history ring, oldest at the front.
    history: VecDeque<NotificationRecord>,
}

impl NotificationManager {
    /// Allocate the next unique [`NotificationId`].
    pub(crate) const fn allocate_id(&mut self) -> NotificationId {
        let id = NotificationId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The dedup key for a `unique` template raised with an optional context.
    /// The `\u{1f}` (unit separator) cannot occur in a template name, so a name
    /// and a context never collide across the boundary.
    fn unique_key(name: &str, context: Option<&str>) -> String {
        match context {
            Some(context) => format!("{name}\u{1f}{context}"),
            None => name.to_owned(),
        }
    }

    /// The live notification for a `unique` template + context, if one is
    /// already showing.
    pub(crate) fn live_unique(&self, name: &str, context: Option<&str>) -> Option<NotificationId> {
        self.unique_live
            .get(&Self::unique_key(name, context))
            .copied()
    }

    /// Register `id` as the live instance of a `unique` template + context.
    pub(crate) fn register_unique(
        &mut self,
        name: &str,
        context: Option<&str>,
        id: NotificationId,
    ) {
        self.unique_live.insert(Self::unique_key(name, context), id);
    }

    /// Drop `id` from the `unique` index (it is no longer live).
    pub(crate) fn clear_unique(&mut self, id: NotificationId) {
        self.unique_live.retain(|_key, value| *value != id);
    }

    /// Record a newly raised notification in the history ring, dropping the
    /// oldest entries past [`HISTORY_CAP`].
    pub(crate) fn push_history(&mut self, record: NotificationRecord) {
        self.history.push_back(record);
        while self.history.len() > HISTORY_CAP {
            self.history.pop_front();
        }
    }

    /// Record the response on the history entry for `id`, if it is still in the
    /// ring.
    pub(crate) fn record_response(&mut self, id: NotificationId, button: Option<&'static str>) {
        if let Some(record) = self.history.iter_mut().rev().find(|record| record.id == id) {
            record.response = button;
        }
    }

    /// The history entries, oldest first — the data the history panel renders.
    pub(crate) fn history(&self) -> impl Iterator<Item = &NotificationRecord> {
        self.history.iter()
    }
}

/// Test-only builder conveniences.
#[cfg(test)]
impl NotificationArgs {
    /// Builder form of [`set`](Self::set): bind `key` to `value` and return
    /// `self`, for concise test fixtures.
    #[must_use]
    fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.set(key, value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BAKE_CANCEL_FORM, DISCARD_KEEP_EDITING_FORM, KICK_ALL_RESIDENTS_CANCEL_FORM,
        LEAVE_CANCEL_FORM, NOTIFICATIONS, NotificationArgs, NotificationKind, NotificationManager,
        OK_CANCEL_DONT_ASK_FORM, REBAKE_CLOSE_FORM, REBAKE_REGION_FORM, REMOVE_CANCEL_FORM,
        REPLACE_ATTACHMENT_FORM, SAVE_ALL_DISCARD_CANCEL_FORM, SAVE_CANCEL_FORM,
        SAVE_DISCARD_CANCEL_FORM, SEND_CANCEL_FORM, THIS_ESTATE_ALL_ESTATES_FORM,
        VIEW_IM_QUIT_FORM, YES_NO_BUTTONS_FORM, YES_NO_FORM, substitute, template,
    };
    use pretty_assertions::{assert_eq, assert_ne};

    /// The English Fluent bundle source, embedded so the catalogue's keys can be
    /// checked against it without the async asset load.
    const EN_FTL: &str = include_str!("../assets/locales/en/main.ftl");

    /// The set of message identifiers declared in [`EN_FTL`] — a message entry
    /// begins at column 0 with `identifier =` (attributes and continuation lines
    /// are indented, comments begin with `#`).
    fn ftl_keys() -> std::collections::HashSet<String> {
        EN_FTL
            .lines()
            .filter_map(|line| {
                if line.starts_with([' ', '\t', '#']) {
                    return None;
                }
                let ident = line.split_once('=')?.0.trim();
                if ident.is_empty() || ident.contains(char::is_whitespace) {
                    return None;
                }
                Some(ident.to_owned())
            })
            .collect()
    }

    /// Names are the catalogue's primary key and what a response routes on, so a
    /// duplicate would make one template's raise ambiguous.
    #[test]
    fn template_names_are_unique() {
        let mut names: Vec<&str> = NOTIFICATIONS.iter().map(|entry| entry.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "two catalogue templates share a name");
    }

    /// Lookup by name finds the entry, and an unknown name returns nothing rather
    /// than panicking or falling back — the host logs and drops a bad raise.
    #[test]
    fn lookup_finds_known_and_misses_unknown() {
        assert!(template("SystemTip").is_some());
        assert!(template("NoSuchNotification").is_none());
    }

    /// Every template's `message_key` and every button `label_key` must be
    /// non-empty, or a resolve would look up the empty key.
    #[test]
    fn every_template_and_button_has_a_key() {
        for entry in NOTIFICATIONS {
            assert!(
                !entry.message_key.is_empty(),
                "{}: empty message_key",
                entry.name
            );
            for button in entry.form {
                assert!(!button.name.is_empty(), "{}: empty button name", entry.name);
                assert!(
                    !button.label_key.is_empty(),
                    "{}: empty button label_key",
                    entry.name
                );
            }
        }
    }

    /// A form with buttons must name exactly one default (the Enter / expiry
    /// choice); an empty form has none.
    #[test]
    fn each_non_empty_form_has_one_default_button() {
        for entry in NOTIFICATIONS {
            let defaults = entry.form.iter().filter(|button| button.is_default).count();
            if entry.form.is_empty() {
                assert_eq!(defaults, 0, "{}: empty form with a default", entry.name);
            } else {
                assert_eq!(
                    defaults, 1,
                    "{}: form must have exactly one default",
                    entry.name
                );
                let default = entry
                    .form
                    .iter()
                    .find(|button| button.is_default)
                    .map(|button| button.name);
                assert_eq!(entry.default_button(), default);
            }
        }
    }

    /// Every catalogue `message_key` and button `label_key` resolves to an
    /// English Fluent entry, so a raised toast never renders its raw key. Guards
    /// the catalogue against a typo drifting from `en/main.ftl`.
    #[test]
    fn every_key_has_an_english_fluent_entry() {
        let keys = ftl_keys();
        for entry in NOTIFICATIONS {
            assert!(
                keys.contains(entry.message_key),
                "{}: message_key {} has no en/main.ftl entry",
                entry.name,
                entry.message_key
            );
            for button in entry.form {
                assert!(
                    keys.contains(button.label_key),
                    "{}: button label_key {} has no en/main.ftl entry",
                    entry.name,
                    button.label_key
                );
            }
            if let Some(key) = entry.input.and_then(|input| input.default_key) {
                assert!(
                    keys.contains(key),
                    "{}: input default_key {} has no en/main.ftl entry",
                    entry.name,
                    key
                );
            }
            if let Some(key) = entry.title_key {
                assert!(
                    keys.contains(key),
                    "{}: title_key {} has no en/main.ftl entry",
                    entry.name,
                    key
                );
            }
        }
    }

    /// The keyed server alerts [`crate::notification_host::ingest_alert_messages`]
    /// matches an `AlertInfo` key against are in the catalogue with the reference
    /// kind, so a real keyed alert resolves to the right channel.
    #[test]
    fn keyed_server_alerts_are_catalogued() {
        for (name, kind) in [
            ("RegionEntryAccessBlocked", NotificationKind::AlertModal),
            ("TeleportEntryAccessBlocked", NotificationKind::AlertModal),
            ("LandClaimAccessBlocked", NotificationKind::AlertModal),
            ("LandBuyAccessBlocked", NotificationKind::AlertModal),
            ("RegionEntryAccessBlocked_Notify", NotificationKind::Tip),
            ("RegionRestartSeconds", NotificationKind::Alert),
            ("TooManyScripts", NotificationKind::Notify),
            ("FailedToPlaceObject", NotificationKind::Notify),
            // The appearance & wearables attach / drop refusal family.
            ("NotEnoughResourcesToAttach", NotificationKind::Notify),
            ("AttachmentHasTooMuchInventory", NotificationKind::Notify),
            ("IllegalAttachment", NotificationKind::Notify),
            ("CantAttackMultipleObjOneSpot", NotificationKind::Notify),
            (
                "NoPermsTooManyAttachedAnimatedObjects",
                NotificationKind::Notify,
            ),
            (
                "CantAttachObjectAvatarSittingOnIt",
                NotificationKind::Notify,
            ),
            ("WhyAreYouTryingToWearShrubbery", NotificationKind::Notify),
            ("CantAttachGroupOwnedObjs", NotificationKind::Notify),
            ("CantAttachObjectsNotOwned", NotificationKind::Notify),
            ("CantAttachNavmeshObjects", NotificationKind::Notify),
            (
                "CantAttachObjectNoMovePermissions",
                NotificationKind::Notify,
            ),
            (
                "CantAttachNotEnoughScriptResources",
                NotificationKind::Notify,
            ),
            ("CantAttachObjectBeingRemoved", NotificationKind::Notify),
            ("CantDropItemTrialUser", NotificationKind::Notify),
            ("CantDropMeshAttachment", NotificationKind::Notify),
            ("CantDropAttachmentNoPermission", NotificationKind::Notify),
            (
                "CantDropAttachmentInsufficientLandResources",
                NotificationKind::Notify,
            ),
            (
                "CantDropAttachmentInsufficientResources",
                NotificationKind::Notify,
            ),
            ("CantDropObjectFullParcel", NotificationKind::Notify),
            ("CantCreateOutfit", NotificationKind::Notify),
            // The avatar-movement sit / stand refusal family.
            ("SitFailCantMove", NotificationKind::Notify),
            ("SitFailNotAllowedOnLand", NotificationKind::Notify),
            ("SitFailNotSameRegion", NotificationKind::Notify),
            ("StandDeniedByObject", NotificationKind::Notify),
            ("ResitDeniedByObject", NotificationKind::Notify),
            ("CantSitNoSuitableSurface", NotificationKind::Notify),
            ("CantSitNoRoom", NotificationKind::Notify),
            // The estate-region entry refusals and freeze / eject / terrain
            // feedback family.
            ("RegionAboutToShutdown", NotificationKind::Notify),
            ("URBannedFromRegion", NotificationKind::Notify),
            ("NoTeenGridAccess", NotificationKind::Notify),
            ("ImproperPaymentStatus", NotificationKind::Notify),
            ("MustGetAgeRegion", NotificationKind::Notify),
            ("AvatarFrozen", NotificationKind::Notify),
            ("AvatarFrozenDuration", NotificationKind::Notify),
            ("YouFrozeAvatar", NotificationKind::Notify),
            ("AvatarHasUnFrozenYou", NotificationKind::Notify),
            ("AvatarUnFrozen", NotificationKind::Notify),
            ("AvatarFreezeFailure", NotificationKind::Notify),
            ("AvatarFreezeThaw", NotificationKind::Notify),
            ("AvatarCantFreeze", NotificationKind::Notify),
            ("EjectComingSoon", NotificationKind::Notify),
            ("NoEnterRegionMaybeFull", NotificationKind::Notify),
            ("SorryCantEjectUser", NotificationKind::Notify),
            ("AvatarEjected", NotificationKind::Tip),
            ("AvatarEjectFailed", NotificationKind::Notify),
            ("FullRegionCantEnter", NotificationKind::Notify),
            (
                "EstateManagerFailedllTeleportHome",
                NotificationKind::Notify,
            ),
            ("CantTeleportCouldNotFindUser", NotificationKind::Notify),
            ("TerrainUploadFailed", NotificationKind::Notify),
            ("TerrainFileWritten", NotificationKind::Notify),
            (
                "TerrainFileWrittenStartingDownload",
                NotificationKind::Notify,
            ),
            ("TerrainBaked", NotificationKind::Notify),
            ("GodBeatsFreeze", NotificationKind::Notify),
        ] {
            let entry = template(name);
            assert!(entry.is_some(), "{name} not in catalogue");
            if let Some(entry) = entry {
                assert_eq!(entry.kind, kind, "{name}: wrong kind");
            }
        }
    }

    /// The custom-labelled forms keep their stable button names (the reference
    /// functor names) so a consumer routes on the name, not the localized
    /// label, and each names one default.
    #[test]
    fn custom_forms_route_on_stable_names() {
        for (form, expected) in [
            (LEAVE_CANCEL_FORM, &["OK", "Cancel"][..]),
            (VIEW_IM_QUIT_FORM, &["OK", "Cancel"][..]),
            (YES_NO_FORM, &["OK", "Cancel"][..]),
            (DISCARD_KEEP_EDITING_FORM, &["OK", "Cancel"][..]),
            (SAVE_CANCEL_FORM, &["OK", "Cancel"][..]),
            (REMOVE_CANCEL_FORM, &["OK", "Cancel"][..]),
            (SEND_CANCEL_FORM, &["OK", "Cancel"][..]),
            (KICK_ALL_RESIDENTS_CANCEL_FORM, &["OK", "Cancel"][..]),
            (BAKE_CANCEL_FORM, &["OK", "Cancel"][..]),
            (REBAKE_CLOSE_FORM, &["OK", "Cancel"][..]),
            (REBAKE_REGION_FORM, &["OK"][..]),
            (SAVE_DISCARD_CANCEL_FORM, &["Yes", "No", "Cancel"][..]),
            (SAVE_ALL_DISCARD_CANCEL_FORM, &["Yes", "No", "Cancel"][..]),
            (OK_CANCEL_DONT_ASK_FORM, &["Yes", "No", "Cancel"][..]),
            (THIS_ESTATE_ALL_ESTATES_FORM, &["Yes", "No", "Cancel"][..]),
            (REPLACE_ATTACHMENT_FORM, &["Yes", "No"][..]),
            (YES_NO_BUTTONS_FORM, &["Yes", "No"][..]),
        ] {
            let names: Vec<&str> = form.iter().map(|button| button.name).collect();
            assert_eq!(names, expected);
            assert_eq!(
                form.iter().filter(|button| button.is_default).count(),
                1,
                "a form must name exactly one default"
            );
        }
    }

    /// A template with a text-input field must offer buttons to submit it with
    /// (a bare input could never resolve), and its field name is a non-empty
    /// stable identifier.
    #[test]
    fn input_templates_have_buttons_and_a_field_name() {
        let mut input_count = 0_usize;
        for entry in NOTIFICATIONS {
            if let Some(input) = entry.input {
                input_count += 1;
                assert!(
                    !entry.form.is_empty(),
                    "{}: an input field needs buttons to submit it",
                    entry.name
                );
                assert!(!input.name.is_empty(), "{}: empty input name", entry.name);
                if let Some(key) = input.default_key {
                    assert!(!key.is_empty(), "{}: empty input default_key", entry.name);
                }
            }
        }
        // The save-outfit / save-wearable / rename-outfit / new-AO-set
        // prompts plus the kick / freeze / unfreeze / announcement dialogs.
        assert_eq!(input_count, 10, "unexpected number of input templates");
    }

    /// A tip never carries buttons and always auto-fades; a modal never fades.
    /// These are the invariants the host relies on when it routes a kind.
    #[test]
    fn kind_invariants_hold() {
        for entry in NOTIFICATIONS {
            if entry.kind == NotificationKind::Tip {
                assert!(
                    entry.form.is_empty(),
                    "{}: a tip must have no buttons",
                    entry.name
                );
                assert!(
                    entry.title_key.is_none(),
                    "{}: a tip renders no title header",
                    entry.name
                );
            }
            assert_eq!(
                entry.kind.fades(),
                entry.kind.lifetime_secs() > 0.0,
                "{}: fades() must agree with a positive lifetime",
                entry.name
            );
        }
        assert!(!NotificationKind::AlertModal.fades());
        assert!(NotificationKind::AlertModal.is_modal());
        assert!(!NotificationKind::Notify.is_modal());
    }

    /// Substitution replaces a bound key, leaves an unbound one bracketed, and
    /// handles adjacent and repeated tokens — the reference `[KEY]` behaviour.
    #[test]
    fn substitution_replaces_bound_and_keeps_unbound() {
        let args = NotificationArgs::new()
            .with("MINUTES", "5")
            .with("NAME", "Region A");
        assert_eq!(
            substitute("[NAME] restarts in [MINUTES] minutes", &args),
            "Region A restarts in 5 minutes"
        );
        // An unbound token is left verbatim.
        assert_eq!(substitute("hello [WHO]", &args), "hello [WHO]");
        // Adjacent and repeated tokens.
        assert_eq!(substitute("[MINUTES][MINUTES]", &args), "55");
        // Text with no tokens is unchanged.
        assert_eq!(substitute("plain text", &args), "plain text");
    }

    /// An unterminated `[` is emitted verbatim rather than swallowing the tail —
    /// no message content is lost to a malformed template.
    #[test]
    fn substitution_keeps_an_unterminated_bracket() {
        let args = NotificationArgs::new();
        assert_eq!(substitute("cost is [USD 5", &args), "cost is [USD 5");
    }

    /// `ExtraParams` parses `key=value` pairs on `|` / newline boundaries,
    /// trims whitespace, and ignores a fragment with no `=`.
    #[test]
    fn extra_params_parse_into_args() {
        let args = NotificationArgs::parse_extra_params("MINUTES=5 | NAME = Region A\nBOGUS");
        assert_eq!(
            substitute("[NAME]: [MINUTES] [BOGUS]", &args),
            "Region A: 5 [BOGUS]"
        );
    }

    /// A later `set` for the same key replaces the earlier value rather than
    /// appending a second binding.
    #[test]
    fn setting_a_key_twice_replaces_it() {
        let args = NotificationArgs::new().with("K", "one").with("K", "two");
        assert_eq!(substitute("[K]", &args), "two");
    }

    /// Ids are handed out monotonically and never repeat.
    #[test]
    fn ids_are_monotonic_and_distinct() {
        let mut manager = NotificationManager::default();
        let first = manager.allocate_id();
        let second = manager.allocate_id();
        let third = manager.allocate_id();
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    /// The `unique` index scopes by context: the same context finds the live
    /// instance, a different context does not, and clearing removes it.
    #[test]
    fn unique_index_scopes_by_context() {
        let mut manager = NotificationManager::default();
        let id = manager.allocate_id();
        manager.register_unique("RegionRestartMinutes", Some("region-a"), id);
        assert_eq!(
            manager.live_unique("RegionRestartMinutes", Some("region-a")),
            Some(id)
        );
        assert_eq!(
            manager.live_unique("RegionRestartMinutes", Some("region-b")),
            None
        );
        assert_eq!(manager.live_unique("RegionRestartMinutes", None), None);
        manager.clear_unique(id);
        assert_eq!(
            manager.live_unique("RegionRestartMinutes", Some("region-a")),
            None
        );
    }

    /// The history ring keeps responses attached to the right entry and stays
    /// bounded — a raise beyond the cap drops the oldest, not the newest.
    #[test]
    fn history_records_responses_and_stays_bounded() {
        use super::{HISTORY_CAP, NotificationRecord};
        let mut manager = NotificationManager::default();
        let mut first = None;
        for index in 0..HISTORY_CAP + 10 {
            let id = manager.allocate_id();
            if index == 0 {
                first = Some(id);
            }
            manager.push_history(NotificationRecord {
                id,
                template: "SystemTip",
                kind: NotificationKind::Tip,
                body: format!("tip {index}"),
                response: None,
            });
        }
        assert_eq!(manager.history().count(), HISTORY_CAP);
        // The very first entry has fallen off the front.
        if let Some(first) = first {
            assert!(manager.history().all(|record| record.id != first));
        }
        // A response attaches to a still-present entry.
        let live = manager.allocate_id();
        manager.push_history(NotificationRecord {
            id: live,
            template: "GenericAlert",
            kind: NotificationKind::Alert,
            body: String::from("alert"),
            response: None,
        });
        manager.record_response(live, Some("OK"));
        assert_eq!(
            manager
                .history()
                .find(|record| record.id == live)
                .and_then(|record| record.response),
            Some("OK")
        );
    }
}
