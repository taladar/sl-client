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
    /// The Fluent key for the pre-filled text. Resolved through
    /// [`crate::i18n`], then `[KEY]`-substituted with the raised
    /// notification's [`NotificationArgs`] (the reference defaults are
    /// substitution templates like `[DESC] (new)`).
    pub(crate) default_key: &'static str,
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
///
/// See `viewer-notification-catalogue`.
pub(crate) const NOTIFICATIONS: &[NotificationTemplate] = &[
    // A generic transient tip — the fallback for an unkeyed server hint.
    NotificationTemplate {
        name: "SystemTip",
        kind: NotificationKind::Tip,
        message_key: "notification-system-tip",
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
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: "notification-save-wearable-as-default",
        }),
    },
    NotificationTemplate {
        name: "SaveOutfitAs",
        kind: NotificationKind::AlertModal,
        message_key: "notification-save-outfit-as",
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: true,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: "notification-save-outfit-as-default",
        }),
    },
    NotificationTemplate {
        name: "RenameOutfit",
        kind: NotificationKind::AlertModal,
        message_key: "notification-rename-outfit",
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "new_name",
            default_key: "notification-rename-outfit-default",
        }),
    },
    NotificationTemplate {
        name: "ConfirmOverwriteOutfit",
        kind: NotificationKind::AlertModal,
        message_key: "notification-confirm-overwrite-outfit",
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
        priority: NotificationPriority::Normal,
        persist: false,
        log_to_chat: false,
        unique: false,
        ignorable: false,
        form: OK_CANCEL_FORM,
        input: Some(NotificationInput {
            name: "message",
            default_key: "notification-new-ao-set-default",
        }),
    },
    NotificationTemplate {
        name: "NewAOCantContainNonASCII",
        kind: NotificationKind::AlertModal,
        message_key: "notification-new-ao-cant-contain-non-ascii",
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
        priority: NotificationPriority::Normal,
        persist: true,
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
        DISCARD_KEEP_EDITING_FORM, LEAVE_CANCEL_FORM, NOTIFICATIONS, NotificationArgs,
        NotificationKind, NotificationManager, REMOVE_CANCEL_FORM, REPLACE_ATTACHMENT_FORM,
        SAVE_ALL_DISCARD_CANCEL_FORM, SAVE_CANCEL_FORM, SAVE_DISCARD_CANCEL_FORM, SEND_CANCEL_FORM,
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
            if let Some(input) = entry.input {
                assert!(
                    keys.contains(input.default_key),
                    "{}: input default_key {} has no en/main.ftl entry",
                    entry.name,
                    input.default_key
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
            (SAVE_DISCARD_CANCEL_FORM, &["Yes", "No", "Cancel"][..]),
            (SAVE_ALL_DISCARD_CANCEL_FORM, &["Yes", "No", "Cancel"][..]),
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
                assert!(
                    !input.default_key.is_empty(),
                    "{}: empty input default_key",
                    entry.name
                );
            }
        }
        // The save-outfit / save-wearable / rename-outfit / new-AO-set prompts.
        assert_eq!(input_count, 4, "unexpected number of input templates");
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
