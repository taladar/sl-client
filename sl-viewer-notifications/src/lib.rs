//! The **declarative notification catalogue** and its runtime state
//! (`viewer-ui-notification-host`): the data model the toast / notification host
//! (`notification_host`) is driven by.
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
//!   (`ui_element`).
//! - [`NotificationManager`] — the host's runtime state: the id source, the
//!   `unique` dedup index, and the bounded history ring the future notification
//!   list / history panel ([[viewer-notification-history]]) renders.
//!
//! Everything here is pure data and logic (no Bevy world access beyond the
//! message / resource derives), so the catalogue lookup, the substitution and
//! the dedup are unit-tested directly. The rendering — stacking, timing out,
//! fading, dismissing — lives in `notification_host`.
//!
//! # Where the data lives
//!
//! This file is the types, the lookup and the runtime state. The data itself
//! is next door, because ~1,300 entries and ~100 button rows are more than one
//! file can usefully hold:
//!
//! - `catalogue` — one module per notification family (appearance, groups,
//!   teleport, …), each holding its family's entries, flattened at compile
//!   time into the single [`NOTIFICATIONS`] slice re-exported here. A new
//!   entry is added to the family it belongs to, not to a 20k-line table.
//! - `forms` — the button rows a template's form offers, one `const` per
//!   distinct row (the reference's `<usetemplate>`), shared across families
//!   and re-exported here.

use std::{
    collections::{HashMap, VecDeque},
    sync::LazyLock,
};

use bevy_ecs::prelude::{Message, Resource};

mod catalogue;
mod forms;

pub use crate::{catalogue::NOTIFICATIONS, forms::*};

/// The rendering channel / behaviour class of a notification — the reference
/// `LLNotificationTemplate` `type`, narrowed to the four the host substrate
/// needs. The specific dialog tasks add their forms *on top* of these kinds
/// rather than new kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
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
    #[must_use]
    pub const fn fades(self) -> bool {
        matches!(self, Self::Tip | Self::Notify)
    }

    /// Whether this kind blocks the world behind a scrim: only
    /// [`AlertModal`](Self::AlertModal).
    #[must_use]
    pub const fn is_modal(self) -> bool {
        matches!(self, Self::AlertModal)
    }

    /// The on-screen lifetime before the fade begins, in seconds, or `0.0` for a
    /// kind that never auto-expires (alerts and modals wait for a click). The
    /// values mirror the reference `NotificationTipToastLifeTime` (10 s) and
    /// `NotificationToastLifeTime` (30 s).
    #[must_use]
    pub const fn lifetime_secs(self) -> f32 {
        match self {
            Self::Tip => 10.0,
            Self::Notify => 30.0,
            Self::Alert | Self::AlertModal => 0.0,
        }
    }
}

/// How long a toast takes to fade out after its lifetime elapses, in seconds —
/// the reference `ToastFadingTime`.
pub const TOAST_FADE_SECS: f32 = 2.0;

/// The gap between two stacked toasts, in logical pixels — the reference
/// `ToastGap`.
pub const TOAST_GAP: f32 = 8.0;

/// A notification's priority — the reference `LLNotificationPriority`. Ordered so
/// a higher-priority toast sorts to the more visible bottom of the stack (see
/// `notification_host`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationPriority {
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
pub struct NotificationButton {
    /// The stable button name sent back as the [`NotificationResponse::button`] —
    /// the reference "functor button" name (`"OK"`, `"Cancel"`, `"Yes"`, …). Not
    /// translated: it is an identifier, not a label.
    pub name: &'static str,
    /// The Fluent key for the button's visible label, resolved through
    /// `i18n` so the label localizes while the [`name`](Self::name)
    /// stays stable.
    pub label_key: &'static str,
    /// Whether this is the default button — the one chosen on `Enter` and on a
    /// toast's auto-expiry (the reference `expire_option`).
    pub is_default: bool,
}

/// A single-line text-input field on a notification's form — the reference
/// `<input>` element (the save-outfit / save-wearable / rename-outfit name
/// prompts). The host pre-fills the field with the resolved
/// [`default_key`](Self::default_key) text and returns the edited value on
/// [`NotificationResponse::input`] when a button is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationInput {
    /// The stable field name a consumer routes on — the reference `<input
    /// name=…>` (`"message"`, `"new_name"`). An identifier, not a label.
    pub name: &'static str,
    /// The Fluent key for the pre-filled text, or `None` for a field that
    /// starts empty (the announcement prompts). Resolved through
    /// `i18n`, then `[KEY]`-substituted with the raised
    /// notification's [`NotificationArgs`] (the reference defaults are
    /// substitution templates like `[DESC] (new)`).
    pub default_key: Option<&'static str>,
}

/// How a template's "don't show me this again" checkbox behaves — the
/// reference `EIgnoreType` on `LLNotificationForm`. The kind decides three
/// things: whether the toast offers the checkbox at all
/// ([`offers_checkbox`](Self::offers_checkbox)), whether the Preferences
/// alerts tab lists the notification and a per-name show/suppress `Bool`
/// setting exists ([`is_suppressible`](Self::is_suppressible)), and what a
/// suppressed raise auto-responds with
/// (`notification_host`'s `auto_response_button`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationIgnore {
    /// No ignore behaviour: no checkbox, never suppressible (the reference
    /// `IGNORE_NO` — every template without an `<ignore>` / `ignoretext`).
    None,
    /// The form shows a checkbox whose ticked state merely rides the
    /// [`NotificationResponse::ignored`] flag for the template's owner to
    /// interpret — no suppression setting is registered and the alerts tab
    /// does not list it (the reference `IGNORE_CHECKBOX_ONLY`, e.g. "Always
    /// choose this option" style prompts owned by their feature).
    CheckboxOnly,
    /// Suppressible; a suppressed raise auto-responds with the form's
    /// default button so the confirmed action still proceeds (the reference
    /// `IGNORE_WITH_DEFAULT_RESPONSE` — the overwhelmingly common kind).
    DefaultResponse,
    /// [`DefaultResponse`](Self::DefaultResponse), but the suppression is
    /// runtime-only and resets every session (the reference
    /// `IGNORE_WITH_DEFAULT_RESPONSE_SESSION_ONLY`; currently populated by
    /// no reference template — modelled so a future port has the variant).
    DefaultResponseSessionOnly,
    /// Suppressible; the checkbox reads "always choose this option" and a
    /// suppressed raise replays the button the user last pressed (the
    /// reference `IGNORE_WITH_LAST_RESPONSE`, persisted per avatar under
    /// [`last_response_setting_name`]).
    LastResponse,
    /// Suppressible; a suppressed raise is simply not shown and answers
    /// nothing (the reference `IGNORE_SHOW_AGAIN`; currently mapped by no
    /// reference template — modelled so a future port has the variant, and the
    /// registration / auto-respond / label match arms already handle it).
    ShowAgain,
}

impl NotificationIgnore {
    /// Whether the toast form carries the "don't show again" / "always
    /// choose" checkbox (every kind but [`None`](Self::None)).
    #[must_use]
    pub const fn offers_checkbox(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Whether a per-name show/suppress setting exists, the host honours it
    /// on raise, and the Preferences alerts tab lists the notification —
    /// the reference `buildPopupList` criterion `ignore > IGNORE_NO`.
    #[must_use]
    pub const fn is_suppressible(self) -> bool {
        matches!(
            self,
            Self::DefaultResponse
                | Self::DefaultResponseSessionOnly
                | Self::LastResponse
                | Self::ShowAgain
        )
    }
}

/// A declarative notification template — one catalogue entry, mirroring the
/// reference `LLNotificationTemplate`. See the [module documentation](self).
#[derive(Debug, Clone, Copy)]
pub struct NotificationTemplate {
    /// The unique catalogue key (the reference `name`), matched by
    /// [`template`] and echoed on every [`NotificationResponse`].
    pub name: &'static str,
    /// The rendering channel / behaviour class.
    pub kind: NotificationKind,
    /// The Fluent key for the message body. Resolved through `i18n`,
    /// then `[KEY]`-substituted with the raised notification's
    /// [`NotificationArgs`]. A caller may override the resolved text entirely
    /// with [`ShowNotification::body`] (for an already-localized server string).
    pub message_key: &'static str,
    /// The Fluent key for an optional dialog title (the reference `label`) —
    /// rendered as a header line on an alert / modal card, and the
    /// human-readable name the history panel / preferences alerts tab can
    /// show out of context. `None` for the majority of entries, whose body
    /// is self-describing; a tip never carries one.
    pub title_key: Option<&'static str>,
    /// The priority (the reference `priority`) — drives the stack ordering.
    pub priority: NotificationPriority,
    /// Whether this notification persists in the notification well / across
    /// sessions (the reference `persist`). Carried for the history panel; the
    /// host does not itself persist toasts yet.
    pub persist: bool,
    /// Whether the body is also echoed into nearby chat (the reference
    /// `log_to_chat`).
    pub log_to_chat: bool,
    /// Whether at most one live instance may exist (the reference `<unique>`):
    /// raising a second, scoped by [`ShowNotification::context`], replaces the
    /// first rather than stacking a duplicate.
    pub unique: bool,
    /// The "don't show me this again" behaviour (the reference `<ignore>` /
    /// `ignoretext`): whether the form offers the checkbox, whether ticking
    /// it records a suppression the host honours on the next raise (managed
    /// by the Preferences alerts tab), and what a suppressed raise
    /// auto-responds with. See [`NotificationIgnore`].
    pub ignore: NotificationIgnore,
    /// The Fluent key of the reference `ignoretext` — the human-readable
    /// one-line description of what suppressing this notification means
    /// ("Confirm before I pay an object"). The alerts tab's row label, and
    /// for [`NotificationIgnore::CheckboxOnly`] the checkbox label itself.
    /// `Some` exactly when [`ignore`](Self::ignore) is not
    /// [`NotificationIgnore::None`].
    pub ignore_key: Option<&'static str>,
    /// The buttons the toast offers (the reference `<form>` / `<usetemplate>`).
    pub form: &'static [NotificationButton],
    /// An optional single-line text-input field (the reference `<input>`),
    /// shown between the body and the button row. Its edited text comes back
    /// on [`NotificationResponse::input`].
    pub input: Option<NotificationInput>,
}

impl NotificationTemplate {
    /// The default button's [`name`](NotificationButton::name) — chosen on
    /// `Enter` and on auto-expiry — or `None` when the form is empty.
    #[must_use]
    pub fn default_button(&self) -> Option<&'static str> {
        self.form
            .iter()
            .find(|button| button.is_default)
            .map(|button| button.name)
    }
}

/// [`NOTIFICATIONS`] indexed by [`name`](NotificationTemplate::name), built once
/// on the first lookup.
///
/// A linear scan of ~1,300 entries ran on every raise *and* on every
/// [`NotificationResponse`] route, which is where a notification's name is
/// resolved most: the host looks the template up again to decide the channel,
/// the suppression and the auto-response. One map build pays for all of them.
static TEMPLATES_BY_NAME: LazyLock<HashMap<&'static str, &'static NotificationTemplate>> =
    LazyLock::new(|| {
        NOTIFICATIONS
            .iter()
            .map(|entry| (entry.name, entry))
            .collect()
    });

/// Look up a catalogue [`NotificationTemplate`] by its [`name`](NotificationTemplate::name).
#[must_use]
pub fn template(name: &str) -> Option<&'static NotificationTemplate> {
    TEMPLATES_BY_NAME.get(name).copied()
}

/// The settings section under which each ignorable notification's "show again"
/// flag lives (`[notifications]` in the persisted file). A `Bool(false)`
/// override suppresses the named notification; the default is `Bool(true)`
/// (show). The Preferences alerts tab ([[viewer-preferences-alerts-tab]]) is the
/// UI over these flags.
pub const NOTIFICATIONS_SECTION: &str = "notifications";

/// The settings key holding the saved auto-response button for a
/// [`NotificationIgnore::LastResponse`] template — the reference's
/// `"Default" + name` entry in the ignores group. Lives in the same
/// [`NOTIFICATIONS_SECTION`] as the show/suppress flags, as a `String`
/// holding the [`NotificationButton::name`] the user last pressed (empty =
/// none saved yet, fall back to the form's default button).
#[must_use]
pub fn last_response_setting_name(name: &str) -> String {
    format!("Default{name}")
}

/// The `[KEY]` substitution arguments for a notification's message — the
/// reference `LLNotification` substitutions, fed from a keyed `AlertInfo`'s
/// `ExtraParams` on the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationArgs {
    /// The key / value bindings, in insertion order (so a rebuild is stable).
    pairs: Vec<(String, String)>,
}

impl NotificationArgs {
    /// An empty argument set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind `key` to `value`, replacing any existing binding for that key.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
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
    /// store (`notification_persist`) serializes these to re-raise a
    /// persisted notification with its original substitutions.
    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// Rebuild an argument set from serialized [`pairs`](Self::pairs) — the inverse
    /// used when a persisted notification is reloaded from disk.
    #[must_use]
    pub const fn from_pairs(pairs: Vec<(String, String)>) -> Self {
        Self { pairs }
    }

    /// Parse an `AlertInfo` `ExtraParams` blob into arguments: `key=value` pairs
    /// separated by `|` or newlines, each side trimmed. A fragment without an
    /// `=` is ignored. The reference parses this per-alert; this handles the
    /// common `key=value` form.
    #[must_use]
    pub fn parse_extra_params(blob: &str) -> Self {
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
#[must_use]
pub fn substitute(template: &str, args: &NotificationArgs) -> String {
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
pub struct NotificationId(u64);

/// A request to raise a notification from the catalogue — the message a caller
/// writes; the host (`notification_host`) reads it, resolves the
/// template's text, and stacks a toast.
#[derive(Message, Debug, Clone)]
pub struct ShowNotification {
    /// The catalogue template [`name`](NotificationTemplate::name). A raise for a
    /// name not in [`NOTIFICATIONS`] is dropped (logged), so a typo fails loudly
    /// rather than silently.
    pub template: &'static str,
    /// The `[KEY]` substitution arguments for the template's message.
    pub args: NotificationArgs,
    /// An already-localized body to show verbatim instead of resolving the
    /// template's [`message_key`](NotificationTemplate::message_key) — for a
    /// plain server `AlertMessage` string that arrives pre-translated.
    pub body: Option<String>,
    /// A context string that scopes the `unique` dedup: two raises of a unique
    /// template with **different** contexts coexist, with the **same** context
    /// the second replaces the first (the reference `<unique><context>`).
    pub context: Option<String>,
}

impl ShowNotification {
    /// Raise the catalogue template `name` with no arguments, body override or
    /// context — the common case.
    #[must_use]
    pub fn new(name: &'static str) -> Self {
        Self {
            template: name,
            args: NotificationArgs::new(),
            body: None,
            context: None,
        }
    }

    /// Builder: set a `[KEY]` substitution argument.
    #[must_use]
    pub fn arg(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.args.set(key, value);
        self
    }

    /// Builder: override the resolved body with an already-localized string.
    #[must_use]
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Builder: scope the `unique` dedup with a context string.
    #[must_use]
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// A user (or automatic) response to a raised notification — the message the
/// host writes when a button is clicked, a fading toast expires, or a
/// notification is dismissed. A consumer (a specific dialog task) reads it to
/// send the corresponding protocol reply.
#[derive(Message, Debug, Clone)]
pub struct NotificationResponse {
    /// The notification this responds to.
    pub id: NotificationId,
    /// The catalogue template name, so a consumer can route without tracking the
    /// [`id`](Self::id).
    pub template: &'static str,
    /// The chosen button's [`name`](NotificationButton::name), or `None` when the
    /// toast expired or was dismissed without a choice (a fading tip, or an
    /// external [`DismissNotification`]).
    pub button: Option<&'static str>,
    /// Whether the "don't show me this again" checkbox was ticked — the host has
    /// already recorded the suppression; a consumer may act on it too.
    pub ignored: bool,
    /// The text-input field's edited value, for a template with a
    /// [`NotificationTemplate::input`] field — `None` for an inputless
    /// template (or when the toast was dismissed without resolving).
    pub input: Option<String>,
}

/// A request to dismiss a live notification programmatically (its underlying
/// condition passed, e.g. an offer was rescinded). Tears the toast down and
/// emits a [`NotificationResponse`] with no [`button`](NotificationResponse::button).
#[derive(Message, Debug, Clone, Copy)]
pub struct DismissNotification {
    /// The notification to dismiss.
    pub id: NotificationId,
}

/// One entry in the notification history — the data the future notification list
/// / history panel ([[viewer-notification-history]]) renders. Recorded when a
/// notification is raised; its [`response`](Self::response) is filled in when the
/// user answers.
#[derive(Debug, Clone)]
pub struct NotificationRecord {
    /// The raised notification's id.
    pub id: NotificationId,
    /// The catalogue template name.
    pub template: &'static str,
    /// The kind (channel) it was shown on.
    pub kind: NotificationKind,
    /// The resolved, display-ready body text.
    pub body: String,
    /// The chosen button once answered, or `None` while still live or if it
    /// expired / was dismissed without a choice.
    pub response: Option<&'static str>,
}

/// The most history entries kept: a bounded ring, so a long session's toasts do
/// not grow without bound. Older entries drop off the front.
///
/// Public because it is a contract of [`NotificationManager::push_history`] and
/// [`NotificationManager::history`], not an implementation detail: a caller
/// reading the history back needs to know it is capped.
pub const HISTORY_CAP: usize = 256;

/// The host's runtime state: the id source, the `unique` dedup index, and the
/// bounded history ring. Rendering state lives on the toast entities themselves
/// (see `notification_host`); this resource holds only what is not
/// per-entity.
#[derive(Resource, Debug, Default)]
pub struct NotificationManager {
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
    pub const fn allocate_id(&mut self) -> NotificationId {
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
    #[must_use]
    pub fn live_unique(&self, name: &str, context: Option<&str>) -> Option<NotificationId> {
        self.unique_live
            .get(&Self::unique_key(name, context))
            .copied()
    }

    /// Register `id` as the live instance of a `unique` template + context.
    pub fn register_unique(&mut self, name: &str, context: Option<&str>, id: NotificationId) {
        self.unique_live.insert(Self::unique_key(name, context), id);
    }

    /// Drop `id` from the `unique` index (it is no longer live).
    pub fn clear_unique(&mut self, id: NotificationId) {
        self.unique_live.retain(|_key, value| *value != id);
    }

    /// Record a newly raised notification in the history ring, dropping the
    /// oldest entries past [`HISTORY_CAP`].
    pub fn push_history(&mut self, record: NotificationRecord) {
        self.history.push_back(record);
        while self.history.len() > HISTORY_CAP {
            self.history.pop_front();
        }
    }

    /// Record the response on the history entry for `id`, if it is still in the
    /// ring.
    pub fn record_response(&mut self, id: NotificationId, button: Option<&'static str>) {
        if let Some(record) = self.history.iter_mut().rev().find(|record| record.id == id) {
            record.response = button;
        }
    }

    /// The history entries, oldest first — the data the history panel renders.
    pub fn history(&self) -> impl Iterator<Item = &NotificationRecord> {
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
        LEAVE_CANCEL_FORM, NOTIFICATIONS, NotificationArgs, NotificationIgnore, NotificationKind,
        NotificationManager, OK_CANCEL_DONT_ASK_FORM, REBAKE_CLOSE_FORM, REBAKE_REGION_FORM,
        REMOVE_CANCEL_FORM, REPLACE_ATTACHMENT_FORM, SAVE_ALL_DISCARD_CANCEL_FORM,
        SAVE_CANCEL_FORM, SAVE_DISCARD_CANCEL_FORM, SEND_CANCEL_FORM, THIS_ESTATE_ALL_ESTATES_FORM,
        VIEW_IM_QUIT_FORM, YES_NO_BUTTONS_FORM, YES_NO_FORM, substitute, template,
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

    /// A template carries an ignore label exactly when it has ignore behaviour
    /// — the label is the alerts-tab row (or, checkbox-only, the checkbox
    /// text), so a suppressible entry without one would render blank and a
    /// label on a `None` entry would be dead data.
    #[test]
    fn ignore_key_matches_ignore_kind() {
        for entry in NOTIFICATIONS {
            assert_eq!(
                entry.ignore_key.is_some(),
                entry.ignore.offers_checkbox(),
                "{}: ignore {:?} vs ignore_key {:?}",
                entry.name,
                entry.ignore,
                entry.ignore_key
            );
        }
    }

    /// Pin the ported ignore-kind sets to the reference `notifications.xml`
    /// (2026-08 Firestorm): exactly three `save_option` templates
    /// (LastResponse), exactly two `checkbox_only` ones (excluded from the
    /// alerts tab), no session-only or show-again entries, and 140
    /// suppressible templates overall. A port that drifts from these numbers
    /// should be a deliberate edit here.
    #[test]
    fn ignore_kind_port_matches_reference() {
        let mut last_response: Vec<&str> = Vec::new();
        let mut checkbox_only: Vec<&str> = Vec::new();
        let mut unexpected_kind: Vec<&str> = Vec::new();
        let mut suppressible = 0_usize;
        for entry in NOTIFICATIONS {
            match entry.ignore {
                NotificationIgnore::LastResponse => last_response.push(entry.name),
                NotificationIgnore::CheckboxOnly => checkbox_only.push(entry.name),
                NotificationIgnore::DefaultResponseSessionOnly | NotificationIgnore::ShowAgain => {
                    unexpected_kind.push(entry.name);
                }
                NotificationIgnore::None | NotificationIgnore::DefaultResponse => {}
            }
            if entry.ignore.is_suppressible() {
                suppressible = suppressible.saturating_add(1);
            }
        }
        assert_eq!(
            unexpected_kind,
            Vec::<&str>::new(),
            "no reference template maps to session-only / show-again today"
        );
        last_response.sort_unstable();
        checkbox_only.sort_unstable();
        assert_eq!(
            last_response,
            [
                "DoNotDisturbModePay",
                "FirstJoinSupportGroup2",
                "ReplaceAttachment"
            ]
        );
        assert_eq!(
            checkbox_only,
            ["ParcelPlayingMedia", "PromptMFATokenWithSave"]
        );
        assert_eq!(suppressible, 140);
    }

    /// The keyed server alerts `notification_host::ingest_alert_messages`
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
            // The consolidated port's server-keyed families.
            ("GodlikeRequestFailed", NotificationKind::Notify),
            ("GenericRequestFailed", NotificationKind::Notify),
            ("SpecialPowersRequestFailedLogged", NotificationKind::Notify),
            ("ExpireExplanation", NotificationKind::Notify),
            ("DieExplanation", NotificationKind::Notify),
            ("CantUploadPostcard", NotificationKind::Notify),
            ("PrimfeedLoginRequestFailed", NotificationKind::Notify),
            ("NoTransNoSaveToContents", NotificationKind::Notify),
            ("NowOwnObject", NotificationKind::Notify),
            ("NowOwnObjectInv", NotificationKind::Notify),
            ("CantRezOnLand", NotificationKind::Notify),
            ("RezFailTooManyRequests", NotificationKind::Notify),
            ("NoNewObjectRegionFull", NotificationKind::Notify),
            ("NoOwnNoGardening", NotificationKind::Notify),
            ("NoCopyPermsNoObject", NotificationKind::Notify),
            ("NoTransPermsNoObject", NotificationKind::Notify),
            ("AddToNavMeshNoCopy", NotificationKind::Notify),
            ("DupeWithNoRootsSelected", NotificationKind::Notify),
            ("CantDupeCuzRegionIsFull", NotificationKind::Notify),
            ("CantDupeCuzParcelNotFound", NotificationKind::Notify),
            ("CantCreateCuzParcelFull", NotificationKind::Notify),
            ("RezAttemptFailed", NotificationKind::Notify),
            ("ToxicInvRezAttemptFailed", NotificationKind::Notify),
            ("InvItemIsBlacklisted", NotificationKind::Notify),
            ("NoCanRezObjects", NotificationKind::Notify),
            ("SaveBackToInvDisabled", NotificationKind::Notify),
            ("NoExistNoSaveToContents", NotificationKind::Notify),
            ("NoModNoSaveToContents", NotificationKind::Notify),
            ("NoSaveBackToInvDisabled", NotificationKind::Notify),
            ("NoCopyNoSelCopy", NotificationKind::Notify),
            ("NoTransNoSelCopy", NotificationKind::Notify),
            ("NoTransNoCopy", NotificationKind::Notify),
            ("NoPermsNoRemoval", NotificationKind::Notify),
            ("NoModNoSaveSelection", NotificationKind::Notify),
            ("NoCopyNoSaveSelection", NotificationKind::Notify),
            ("NoModNoTaking", NotificationKind::Notify),
            ("RezDestInternalError", NotificationKind::Notify),
            ("DeleteFailObjNotFound", NotificationKind::Notify),
            ("CMOParcelFull", NotificationKind::Notify),
            ("CMOParcelPerms", NotificationKind::Notify),
            ("CMOParcelResources", NotificationKind::Notify),
            ("NoParcelPermsNoObject", NotificationKind::Notify),
            ("CMORegionVersion", NotificationKind::Notify),
            ("CMONavMesh", NotificationKind::Notify),
            ("CMOWTF", NotificationKind::Notify),
            ("NoPermModifyObject", NotificationKind::Notify),
            (
                "CantEnablePhysObjContributesToNav",
                NotificationKind::Notify,
            ),
            ("CantEnablePhysKeyframedObj", NotificationKind::Notify),
            (
                "CantEnablePhysNotEnoughLandResources",
                NotificationKind::Notify,
            ),
            ("CantEnablePhysCostTooGreat", NotificationKind::Notify),
            ("PhantomWithConcavePiece", NotificationKind::Notify),
            ("UnableAddItem", NotificationKind::Notify),
            ("UnableEditItem", NotificationKind::Notify),
            ("NoPermToEdit", NotificationKind::Notify),
            ("CantSaveItemDoesntExist", NotificationKind::Notify),
            ("CantSaveItemAlreadyExists", NotificationKind::Notify),
            ("CantSaveModifyAttachment", NotificationKind::Notify),
            ("AssetServerTimeoutObjReturn", NotificationKind::Notify),
            ("RegionDisablePhysicsShapes", NotificationKind::Notify),
            ("NoModNavmeshAcrossRegions", NotificationKind::Notify),
            (
                "NoSetPhysicsPropertiesOnObjectType",
                NotificationKind::Notify,
            ),
            ("NoSetRootPrimWithNoShape", NotificationKind::Notify),
            ("NoRegionSupportPhysMats", NotificationKind::Notify),
            ("OnlyRootPrimPhysMats", NotificationKind::Notify),
            ("NoSupportCharacterPhysMats", NotificationKind::Notify),
            ("InvalidPhysMatProperty", NotificationKind::Notify),
            ("NoPermsAlterStitchingMeshObj", NotificationKind::Notify),
            ("NoPermsAlterShapeMeshObj", NotificationKind::Notify),
            ("LinkFailedOwnersDiffer", NotificationKind::Notify),
            (
                "LinkFailedNoModNavmeshAcrossRegions",
                NotificationKind::Notify,
            ),
            ("LinkFailedNoPermToEdit", NotificationKind::Notify),
            ("LinkFailedTooManyPrims", NotificationKind::Notify),
            ("LinkFailedCantLinkNoCopyNoTrans", NotificationKind::Notify),
            ("LinkFailedNothingLinkable", NotificationKind::Notify),
            (
                "LinkFailedTooManyPathfindingChars",
                NotificationKind::Notify,
            ),
            ("LinkFailedInsufficientLand", NotificationKind::Notify),
            ("LinkFailedTooMuchPhysics", NotificationKind::Notify),
            ("CantCreateObjectRegionFull", NotificationKind::Notify),
            ("CantCreateAnimatedObjectTooLarge", NotificationKind::Notify),
            ("CantCreateMultipleObjAtLoc", NotificationKind::Notify),
            ("UnableToCreateObjTimeOut", NotificationKind::Notify),
            ("UnableToCreateObjUnknown", NotificationKind::Notify),
            ("UnableToCreateObjMissingFromDB", NotificationKind::Notify),
            ("RezFailureTookTooLong", NotificationKind::Notify),
            ("FailedToPlaceObjAtLoc", NotificationKind::Notify),
            ("CantCreatePlantsOnLand", NotificationKind::Notify),
            ("CantRestoreObjectNoWorldPos", NotificationKind::Notify),
            ("CantRezObjectInvalidMeshData", NotificationKind::Notify),
            ("CantRezObjectTooManyScripts", NotificationKind::Notify),
            ("CantCreateObjectNoAccess", NotificationKind::Notify),
            ("CantCreateObject", NotificationKind::Notify),
            ("InvalidObjectParams", NotificationKind::Notify),
            ("CantDuplicateObjectNoAcess", NotificationKind::Notify),
            ("CantChangeShape", NotificationKind::Notify),
            (
                "NoPermsLinkAnimatedObjectTooLarge",
                NotificationKind::Notify,
            ),
            (
                "NoPermsSetFlagAnimatedObjectTooLarge",
                NotificationKind::Notify,
            ),
            (
                "CantChangeAnimatedObjectStateInsufficientLand",
                NotificationKind::Notify,
            ),
            ("ErrorNoMeshData", NotificationKind::Notify),
            ("NoAccessToClaimObjects", NotificationKind::Notify),
            ("DeedFailedNoPermToDeedForGroup", NotificationKind::Notify),
            ("CantTouchObjectBannedFromParcel", NotificationKind::Notify),
            ("PlzNarrowDeleteParams", NotificationKind::Notify),
            ("TenObjectsDisabledPlzRefresh", NotificationKind::Notify),
            ("CantBuildOverflowParcel", NotificationKind::Notify),
            ("ClaimObjectFailedNoPermission", NotificationKind::Notify),
            ("CantCreateObjectParcelFull", NotificationKind::Notify),
            ("FailedPlacingObject", NotificationKind::Notify),
            ("CantDerezInventoryError", NotificationKind::Notify),
            ("CantFindObject", NotificationKind::Notify),
            (
                "InventoryCreationInWorldObjectFailed",
                NotificationKind::Notify,
            ),
            ("LargePrimAgentIntersect", NotificationKind::Notify),
            ("ExportFinished", NotificationKind::Notify),
            ("ExportFailed", NotificationKind::Notify),
            ("ExportColladaSuccess", NotificationKind::Notify),
            ("ExportColladaFailure", NotificationKind::Notify),
            ("ImportSuccess", NotificationKind::Notify),
            ("MuteLimitReached", NotificationKind::Notify),
            ("RevokedModifyRights", NotificationKind::Notify),
            ("AddToContactSetSingleSuccess", NotificationKind::Notify),
            ("AddToContactSetMultipleSuccess", NotificationKind::Notify),
            ("EjectAvatarFromGroup", NotificationKind::Notify),
            ("CantFetchInventoryForGroupNotice", NotificationKind::Notify),
            ("CantSendGroupNoticeNotPermitted", NotificationKind::Notify),
            (
                "CantSendGroupNoticeCantConstructInventory",
                NotificationKind::Notify,
            ),
            ("CantParceInventoryInNotice", NotificationKind::Notify),
            ("ParcelNoTerraforming", NotificationKind::Notify),
            ("UpdateViewerBuyParcel", NotificationKind::Notify),
            ("CantBuyParcelNotForSale", NotificationKind::Notify),
            (
                "CantBuySalePriceOrLandAreaChanged",
                NotificationKind::Notify,
            ),
            ("CantBuyParcelNotAuthorized", NotificationKind::Notify),
            (
                "CantBuyParcelAwaitingPurchaseAuth",
                NotificationKind::Notify,
            ),
            ("SelectedMultipleOwnedLand", NotificationKind::Notify),
            ("CantJoinTooFewLeasedParcels", NotificationKind::Notify),
            (
                "CantDivideLandMultipleParcelsSelected",
                NotificationKind::Notify,
            ),
            ("CantDivideLandCantFindParcel", NotificationKind::Notify),
            (
                "CantDivideLandWholeParcelSelected",
                NotificationKind::Notify,
            ),
            ("LandHasBeenDivided", NotificationKind::Notify),
            ("PassPurchased", NotificationKind::Notify),
            ("LandPassExpireSoon", NotificationKind::Notify),
            ("CantDeedGroupLand", NotificationKind::Notify),
            ("CantBuyPassTryAgain", NotificationKind::Notify),
            ("NoPrivsToBuyObject", NotificationKind::Notify),
            ("ClaimObjectFailedNoMoney", NotificationKind::Notify),
            ("BuyObjectFailedNoMoney", NotificationKind::Notify),
            ("BuyInventoryFailedNoMoney", NotificationKind::Notify),
            ("BuyPassFailedNoMoney", NotificationKind::Notify),
            ("AddPrimitiveFailure", NotificationKind::Notify),
            ("RezObjectFailure", NotificationKind::Notify),
            ("CantTransfterMoneyRegionDisabled", NotificationKind::Notify),
            ("DroppedMoneyTransferRequest", NotificationKind::Notify),
            ("CantPayNoAgent", NotificationKind::Notify),
            ("CantDonateToPublicObjects", NotificationKind::Notify),
            ("UserBalanceOrLandUsageError", NotificationKind::Notify),
            ("LandSearchBlocked", NotificationKind::Notify),
            ("RegionDisallowsClassifieds", NotificationKind::Notify),
            ("CantCreateLandmarkForEvent", NotificationKind::Notify),
            ("CantCreateLandmark", NotificationKind::Notify),
            ("NoPermToCopyInventory", NotificationKind::Notify),
            ("UnableToUploadAsset", NotificationKind::Notify),
            ("CantCreateRequestedInv", NotificationKind::Notify),
            ("CantCreateRequestedInvFolder", NotificationKind::Notify),
            ("CantCreateInventory", NotificationKind::Notify),
            ("InventoryNotForSale", NotificationKind::Notify),
            ("CantFindInvItem", NotificationKind::Notify),
            ("CantCreateInventoryName", NotificationKind::Notify),
            ("UnableAddScript", NotificationKind::Notify),
            ("RegionSezNotAHome", NotificationKind::Notify),
            ("HomeLocationLimits", NotificationKind::Notify),
            ("TeleportedHomeByObjectOnParcel", NotificationKind::Notify),
            ("TeleportedHomeByObject", NotificationKind::Notify),
            ("TeleportedByAttachment", NotificationKind::Notify),
            ("TeleportedByObjectOnParcel", NotificationKind::Notify),
            ("TeleportedByObjectOwnedBy", NotificationKind::Notify),
            ("TeleportedByObjectUnknownUser", NotificationKind::Notify),
            ("ResetHomePositionNotLegal", NotificationKind::Notify),
            ("CantInviteRegionFull", NotificationKind::Notify),
            ("CantSetHomeAtRegion", NotificationKind::Notify),
            ("ListValidHomeLocations", NotificationKind::Notify),
            ("SetHomePosition", NotificationKind::Notify),
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
            (super::YES_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::CREATE_A_NEW_ACCOUNT_TRY_AGAIN_FORM,
                &["OK", "Cancel"][..],
            ),
            (super::CREATE_ACCOUNT_CONTINUE_FORM, &["OK", "Cancel"][..]),
            (
                super::CONFIRM_AND_LOG_OUT_CANCEL_FORM,
                &["OK", "Cancel"][..],
            ),
            (
                super::OK_HELP_TELEPORT_FORM,
                &["OK", "Help", "Teleport"][..],
            ),
            (super::OK_HELP_FORM, &["OK", "Help"][..]),
            (super::CONFIRM_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::MALE_FEMALE_FORM, &["OK", "Cancel"][..]),
            (
                super::INSTALL_SKIP_NOT_NOW_FORM,
                &["Yes", "No", "Cancel"][..],
            ),
            (super::QUIT_FORM, &["OK"][..]),
            (super::CONTINUE_CANCEL_FORM, &["continue", "cancel"][..]),
            (super::RESET_REMIND_ME_NEXT_TIME_FORM, &["OK", "Cancel"][..]),
            (
                super::MOVE_ITEMS_DONT_MOVE_ITEMS_FORM,
                &["OK", "Cancel"][..],
            ),
            (
                super::MOVE_ITEMS_DONT_MOVE_ITEMS_CANCEL_FORM,
                &["Yes", "No", "Cancel"][..],
            ),
            (
                super::SAVE_OR_DISCARD_CANCEL_FORM,
                &["Yes", "No", "Cancel"][..],
            ),
            (super::DEED_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::UNLINK_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::DISCARD_CHANGES_KEEP_EDITING_2_FORM,
                &["discard", "keep"][..],
            ),
            (super::CONTINUE_LABEL_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::STRIP_ALPHA_USE_AS_IS_FORM,
                &["strip", "use_as_is"][..],
            ),
            (super::SET_NAME_CANCEL_FORM, &["SetName", "Cancel"][..]),
            (
                super::REPLACE_CURRENT_LIST_USE_NEW_NAME_FORM,
                &["ReplaceList", "SetName"][..],
            ),
            (
                super::DELETE_LIST_CANCEL_FORM,
                &["DeleteList", "Cancel"][..],
            ),
            (
                super::ACCEPT_DECLINE_MUTE_FORM,
                &["Accept", "Decline", "Mute"][..],
            ),
            (super::RESPOND_FORM, &["respondbutton"][..]),
            (super::SHUTDOWN_NOW_LATER_FORM, &["OK", "Cancel"][..]),
            (
                super::GO_TO_KNOWLEDGE_BASE_CLOSE_FORM,
                &["OK", "Cancel"][..],
            ),
            (super::CHANGE_PREFERENCES_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::QUIT_DONT_QUIT_FORM, &["OK", "Cancel"][..]),
            (super::FIX_IT_KEEP_IT_FORM, &["OK", "Cancel"][..]),
            (
                super::ALL_MODES_CURRENT_MODE_CANCEL_FORM,
                &["Yes", "No", "Cancel"][..],
            ),
            (super::SAVE_BACKUP_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::RESTORE_AND_QUIT_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::OFFER_CANCEL_FORM, &["Offer", "Cancel"][..]),
            (super::ACCEPT_DECLINE_FORM, &["Accept", "Decline"][..]),
            (super::CREATE_CANCEL_FORM, &["Create", "Cancel"][..]),
            (
                super::APPLY_CHANGES_IGNORE_CHANGES_CANCEL_FORM,
                &["Yes", "No", "Cancel"][..],
            ),
            (super::EJECT_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::BAN_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::JOIN_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::CREATE_GROUP_FOR_L_COST_CANCEL_FORM,
                &["OK", "Cancel"][..],
            ),
            (super::JOIN_DECLINE_FORM, &["OK", "Cancel"][..]),
            (super::CLOSE_FORM, &["OK"][..]),
            (super::YES_NO_CANCEL_FORM, &["Yes", "No", "Cancel"][..]),
            (
                super::JOIN_DECLINE_INFO_FORM,
                &["Join", "Decline", "Info"][..],
            ),
            (
                super::SUFFIXED_YES_NO_FORM,
                &["OK_okcancelignore", "Cancel_okcancelignore"][..],
            ),
            (
                super::FREEZE_UNFREEZE_CANCEL_FORM,
                &["Yes", "No", "Cancel"][..],
            ),
            (
                super::EJECT_EJECT_AND_BAN_CANCEL_FORM,
                &["Yes", "No", "Cancel"][..],
            ),
            (super::DONE_FORM, &["Done"][..]),
            (
                super::PLAY_MEDIA_NOW_ALWAYS_PLAY_MEDIA_DO_NOT_PLAY_MEDIA_FORM,
                &["Play Media Now", "Always Play Media", "Do Not Pley Media"][..],
            ),
            (super::PLAY_DONT_PLAY_FORM, &["Yes", "No"][..]),
            (super::ENABLE_DISABLE_FORM, &["Enable", "Disable"][..]),
            (super::ALLOW_DENY_FORM, &["Allow", "Deny"][..]),
            (
                super::ACTION_NOW_CONDITION_ALLOW_THIS_DOMAIN_CONDITION_ALLOW_THIS_URL_FORM,
                &["Do Now", "RememberDomain", "RememberURL"][..],
            ),
            (
                super::ALLOW_DENY_BLACKLIST_WHITELIST_FORM,
                &["Allow", "Deny", "BlacklistDomain", "WhitelistDomain"][..],
            ),
            (super::ADD_CANCEL_FORM, &["Add", "Cancel"][..]),
            (super::OK_NO_FORM, &["OK", "Cancel"][..]),
            (
                super::CONFIRM_PURCHASE_CANCEL_FORM,
                &["ConfirmPurchase", "Cancel"][..],
            ),
            (super::PAY_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::UPLOAD_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::CONTINUE_NAMED_CANCEL_FORM,
                &["Continue", "Cancel"][..],
            ),
            (super::DETAILS_CANCEL_FORM, &["Details", "Cancel"][..]),
            (super::COPY_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::REMOVE_ITEMS_AND_DELETE_CANCEL_FORM,
                &["OK", "Cancel"][..],
            ),
            (super::DELETE_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::CHECK_TRASH_FOLDER_I_WILL_EMPTY_TRASH_LATER_FORM,
                &["OK", "Cancel"][..],
            ),
            (super::ACCEPT_DISCARD_FORM, &["Keep", "Discard"][..]),
            (
                super::SHOW_ACCEPT_DISCARD_PLUS4_FORM,
                &[
                    "Show",
                    "Accept",
                    "Discard",
                    "ShowSilent",
                    "AcceptSilent",
                    "DiscardSilent",
                    "Mute",
                ][..],
            ),
            (super::OKAY_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::GO_TO_PAGE_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::LATER_GO_NOW_FORM, &["Later", "GoNow..."][..]),
            (super::TRUST_CANCEL_FORM, &["OK", "Cancel"][..]),
            (super::TELEPORT_CANCEL_FORM, &["OK", "Cancel"][..]),
            (
                super::CHANGE_AND_CONTINUE_CANCEL_FORM,
                &["OK", "Cancel"][..],
            ),
            (
                super::TELEPORT_NAMED_CANCEL_FORM,
                &["Teleport", "Cancel"][..],
            ),
            (
                super::TELEPORT_CHANGE_AND_CONTINUE_FORM,
                &["Teleport", "Cancel"][..],
            ),
            (
                super::ALLOW_ALWAYS_ALLOW_DENY_FORM,
                &["Allow", "Always Allow", "Deny"][..],
            ),
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
        // The outfit / AO / kick-freeze-announcement prompts plus the
        // consolidated port's name / message / purchase dialogs.
        assert_eq!(input_count, 34, "unexpected number of input templates");
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
