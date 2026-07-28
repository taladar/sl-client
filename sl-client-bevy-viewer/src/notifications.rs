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
/// Deliberately a representative seed rather than the reference's full ~1,300
/// entries: the point of the host is the *mechanism*, and each specific dialog
/// task adds the concrete entries it needs. The seed covers all four kinds, the
/// `[KEY]` substitution, the `unique` dedup and the ignore checkbox, so every
/// path the host implements has a live example.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        NOTIFICATIONS, NotificationArgs, NotificationKind, NotificationManager, substitute,
        template,
    };
    use pretty_assertions::{assert_eq, assert_ne};

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
