//! The **Conversations floater** (`viewer-social-im-conversations`): one window
//! that hosts every text conversation — nearby (local) chat, one-to-one instant
//! messages, group chats and ad-hoc conferences — as a set of **vertical tabs**
//! down the leading edge, each fronting a transcript pane and its own chat input.
//!
//! # Why this is bespoke and not [`crate::ui_tab`]
//!
//! The reusable tab widget takes a **fixed** label set at spawn time
//! ([`crate::ui_tab::spawn_tab_container`]); conversations are **dynamic** — a tab
//! appears the moment a new IM / group / conference message (or invite) arrives
//! and lives for the session (or until its close button ends it). So this module
//! manages its own strip of tab buttons and its own stack of panels, adding and
//! removing one of each as conversations come and go, in the same visual language
//! as the shared widget. It *does* reuse the widget's [`TabStripWidth`] /
//! [`TabDivider`] so the strip / pane split is a **draggable, persisted** divider
//! for free (`crate::floater_persist` saves it per host floater). The Nearby Chat
//! tab is always present, always first, and cannot be closed (the reference
//! viewer's arrangement).
//!
//! # The model is pure; the ECS is a mirror of it
//!
//! [`ConversationModel`] is a plain, unit-tested resource: the ordered list of
//! conversations, each with a bounded transcript, an unread count, who is typing,
//! whether it is a pending invite, and a revision stamp, plus the name caches that
//! resolve a peer / group / conference to a readable tab title. It is fed **only**
//! from the [`SlEvent`] stream (the viewer never reaches into the session),
//! mirroring [`crate::chat`]'s overlay but keyed per conversation.
//! [`ConversationsUi`] is the parallel ECS side: the floater entities and one
//! [`ConversationView`] per model entry, spawned lazily as entries appear and
//! despawned when they close.
//!
//! # Which input widget fronts which tab
//!
//! The Nearby tab plugs in the **local-chat-input** widget
//! ([`crate::local_chat_input`]) — it carries the whisper/say/shout box, `/N`
//! channel routing and the `/command` registry that only make sense for local
//! chat — while every IM / group / conference tab plugs in the plain
//! **chat-input** widget ([`crate::chat_input`]): an IM has no channel or shout,
//! so its `Enter` maps straight to the session's IM / group / conference send.
//!
//! # Attention, not intrusion
//!
//! A new message to a conversation you are not looking at raises an **unread**
//! badge on its tab and makes the tab (when the window is open) and the toolbar
//! Conversations button (when it is closed) **flash** — but the window is never
//! popped open over what you are doing. Selecting the tab clears its unread.
//!
//! Reference (Firestorm, read-only): `llfloaterimcontainer`, `llfloaterimsession`,
//! `llconversationview`, `fsfloaternearbychat`. The friends and group **lists**
//! the reference also hangs off this floater are separate, already-existing tasks
//! and are deliberately not built here.

use std::collections::{BTreeMap, VecDeque};

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, ChatType, Command, GroupKey, ImDialog, ImSessionId, SlCommand, SlEvent, SlIdentity,
    SlSessionEvent, Uuid,
};

use crate::bottom_toolbar::{BOTTOM_BAR_Z, BottomArea};
use crate::chat_input::{ChatInputSpec, ChatInputSubmit, spawn_chat_input};
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translator};
use crate::local_chat_input::{LocalChatSubmit, spawn_local_chat_input};
use crate::ui::{
    LogicalInset, LogicalPadding, LogicalRect, UiDirection, UiRoot, UiScaffoldSystems, column, row,
};
use crate::ui_font::UiFont;
use crate::ui_tab::{TabDivider, TabPlacement, TabStrip, TabStripWidth, resize_strip_width};

/// The hosting floater's [`crate::floater::FloaterSpec::id`] — it also keys the
/// window's remembered geometry in [`crate::floater_persist`].
pub(crate) const CONVERSATIONS_FLOATER_ID: &str = "conversations";

/// The tab strip's element id — the key [`crate::floater_persist`] remembers the
/// strip / pane split width under (via the reused [`TabStrip`] / [`TabStripWidth`]).
const STRIP_ELEMENT: &str = "conversations-strip";

/// The most transcript lines kept per conversation in memory. Older lines scroll
/// off the top; the reference's recall window is comparable. A conversation's
/// full on-disk history (the chat-log transcript) is a separate paging concern.
const HISTORY_CAP: usize = 200;

/// How many persisted nearby-chat lines to recall from the on-disk transcript
/// ([`crate::chat_log`]) when the panel first loads after login — the scrollback of
/// previous conversation shown above the live lines, comparable to the reference
/// viewer's log-recall window. A single page today; older paging on scroll-to-top
/// is a follow-up.
const RECALL_LIMIT: usize = 100;

/// The vertical tab strip's starting width, in logical pixels — the draggable
/// divider adjusts it from here, and a stored split overrides it at seed time.
const STRIP_WIDTH: f32 = 150.0;

/// The floater's opening content-area size, in logical pixels (strip + panes).
const DEFAULT_SIZE: Vec2 = Vec2::new(560.0, 340.0);

/// The smallest the floater's content area may be dragged to, in logical pixels —
/// enough to keep the strip, a sliver of transcript and the input usable.
const MIN_SIZE: Vec2 = Vec2::new(360.0, 210.0);

/// The chrome / label font size, in logical pixels.
const CHROME_FONT_SIZE: f32 = 13.0;

/// The transcript font size, in logical pixels.
const TRANSCRIPT_FONT_SIZE: f32 = 13.0;

/// One wheel notch's scroll distance, in logical pixels — matched to the gallery
/// and the virtual list so every scroll surface feels the same.
const LINE_SCROLL_PIXELS: f32 = 24.0;

/// A large scroll offset used to pin a transcript to its **bottom**; `bevy_ui`
/// clamps it to the real scrollable range at layout time, so any value past the
/// end lands on the last line.
const SCROLL_TO_BOTTOM: f32 = 1.0e6;

/// The attention flash's frequency, in blinks per second — the tab / button
/// alternates its highlight at this rate while a conversation has unread lines.
pub(crate) const BLINK_HZ: f32 = 1.5;

/// An inactive tab's background — recessed, matching [`crate::ui_tab`]'s look.
const TAB_INACTIVE_BACKGROUND: Color = Color::srgb(0.11, 0.13, 0.17);

/// The active tab's background — the panel shade, so the selected tab reads as
/// merging into its content.
const TAB_ACTIVE_BACKGROUND: Color = Color::srgb(0.19, 0.23, 0.31);

/// The flash colour for a tab with unread lines, alternated with its resting
/// background at [`BLINK_HZ`] — a warm amber that reads as "look here".
const TAB_ATTENTION_BACKGROUND: Color = Color::srgb(0.42, 0.33, 0.12);

/// An inactive tab's border.
const TAB_BORDER: Color = Color::srgb(0.28, 0.33, 0.42);

/// The active tab's border — a bright accent, the loudest "this one is selected"
/// signal.
const TAB_ACTIVE_BORDER: Color = Color::srgb(0.52, 0.68, 0.95);

/// A tab label's colour.
const TAB_LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A tab's close-button glyph colour.
const CLOSE_GLYPH_COLOR: Color = Color::srgb(0.72, 0.74, 0.80);

/// The close-button glyph (a small ✕), on every non-Nearby tab.
const CLOSE_GLYPH: &str = "\u{2715}";

/// The panel area's background — the content shade the active tab shares.
const PANEL_BACKGROUND: Color = Color::srgb(0.19, 0.23, 0.31);

/// The transcript scroll surface's background — a touch darker than the panel so
/// the scrollback reads as a sunken well.
const TRANSCRIPT_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// A transcript line's colour.
const TRANSCRIPT_COLOR: Color = Color::srgb(0.88, 0.90, 0.95);

/// The "X is typing…" line's colour — dim, so it reads as ephemeral status.
const TYPING_COLOR: Color = Color::srgb(0.62, 0.68, 0.78);

/// The pending-invite bar's background — a faint highlight so the prompt stands
/// out above the (empty) transcript.
const INVITE_BACKGROUND: Color = Color::srgba(0.22, 0.26, 0.34, 0.9);

/// The invite prompt / button text colour.
const INVITE_TEXT_COLOR: Color = Color::srgb(0.94, 0.96, 1.0);

/// The Accept button's background — a muted green.
const ACCEPT_BACKGROUND: Color = Color::srgb(0.20, 0.42, 0.26);

/// The Decline button's background — a muted red.
const DECLINE_BACKGROUND: Color = Color::srgb(0.45, 0.22, 0.24);

/// The tab strip / divider thickness, in logical pixels.
const DIVIDER_THICKNESS: f32 = 6.0;

/// The divider bar's colour.
const DIVIDER_COLOR: Color = Color::srgb(0.34, 0.41, 0.53);

/// The divider grip nub's length, in logical pixels.
const DIVIDER_GRIP_LENGTH: f32 = 28.0;

/// The divider grip nub's colour — brighter than the bar, so it reads as a handle.
const DIVIDER_GRIP_COLOR: Color = Color::srgb(0.60, 0.72, 0.92);

/// The dock host's leading (left, mirrored under RTL) inset from the window edge,
/// in logical pixels — flush to the corner like the nearby-chat bar.
const DOCK_INSET: f32 = 0.0;

/// The dock host's faint background — a hosted floater reads as docked (mirrors
/// the floater manager's shared `DefaultDockHost` fill). Invisible until a
/// floater docks into it, as the host content-sizes from empty.
const DOCK_HOST_BACKGROUND: Color = Color::srgba(0.06, 0.07, 0.10, 0.85);

/// The dock host's z-index — one above the bottom bar
/// ([`crate::bottom_toolbar::BOTTOM_BAR_Z`]). This host sits *against* that bar, so
/// a floater docked here (which shares its host's z-plane, see
/// [`crate::floater`]'s `dock`) must out-rank the bar or the bar swallows clicks on
/// the docked floater's bottom-most control — its chat input.
const DOCK_HOST_Z: i32 = BOTTOM_BAR_Z + 1;

/// The Fluent key for the Nearby Chat tab's title.
const NEARBY_TITLE_KEY: &str = "conversations-nearby";

/// The Fluent key for the "our own line" speaker label in a transcript.
const YOU_LABEL_KEY: &str = "conversations-you";

/// The Fluent key for the "one person is typing" status (arg `name`).
const TYPING_ONE_KEY: &str = "conversations-typing-one";

/// The Fluent key for the "several people are typing" status.
const TYPING_MANY_KEY: &str = "conversations-typing-many";

/// The Fluent key for the pending-invite prompt.
const INVITE_PROMPT_KEY: &str = "conversations-invite-prompt";

/// The Fluent key for the invite Accept button.
const INVITE_ACCEPT_KEY: &str = "conversations-invite-accept";

/// The Fluent key for the invite Decline button.
const INVITE_DECLINE_KEY: &str = "conversations-invite-decline";

// ---------------------------------------------------------------------------
// Pure model
// ---------------------------------------------------------------------------

/// A conversation's stable identity — the per-tab key. `Nearby` is the singleton
/// local-chat tab; the rest key on the peer, group or conference.
///
/// Derives [`Ord`] so it can key the [`ConversationsUi`] view map (sl-types gives
/// the newtypes their ordering).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum ConversationKey {
    /// The local (nearby) chat tab — always present, always first.
    Nearby,
    /// A one-to-one instant-message conversation with a peer.
    Direct(AgentKey),
    /// A group IM session.
    Group(GroupKey),
    /// An ad-hoc conference IM session.
    Conference(ImSessionId),
}

impl ConversationKey {
    /// Whether this is the un-closable Nearby tab.
    const fn is_nearby(self) -> bool {
        matches!(self, Self::Nearby)
    }
}

/// One transcript line: who said it and what, plus whether it was **our own**
/// line (rendered with the localized "You" label rather than a stored name).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptLine {
    /// Whether we sent this line (a local echo of an outbound message).
    own: bool,
    /// The remote speaker's display name (empty for our own lines).
    speaker: String,
    /// The message text.
    body: String,
}

/// One conversation: its key, bounded transcript, unread count, who is currently
/// typing, whether it is a pending invite, and a revision stamp bumped on every
/// appended line (so the view knows when to re-render the transcript).
#[derive(Debug, Clone)]
struct Conversation {
    /// The conversation's identity.
    key: ConversationKey,
    /// The transcript, oldest at the front.
    lines: VecDeque<TranscriptLine>,
    /// Unread lines received while this was not the active conversation.
    unread: u32,
    /// Who is currently typing, id → display name (for the "X is typing…" line).
    typing: BTreeMap<AgentKey, String>,
    /// Whether this is an invitation we have not yet accepted (shows the
    /// Accept / Decline bar). Cleared on accept or when the first message lands.
    pending_invite: bool,
    /// Persisted **recall** lines rendered *above* the live [`lines`](Self::lines),
    /// loaded once from the on-disk chat-log transcript so the panel opens on
    /// previous-session history (the reference viewer's "load previous
    /// conversation"). Only the Nearby tab populates this today (via
    /// [`ConversationModel::set_nearby_recall`]); every other tab leaves it empty.
    /// Not subject to the live [`HISTORY_CAP`] and never counted as unread.
    recall: Vec<TranscriptLine>,
    /// Bumped on each appended line; the view's last-rendered value is compared
    /// against it to avoid rebuilding an unchanged transcript node.
    revision: u64,
}

impl Conversation {
    /// A fresh, empty conversation for `key`.
    const fn new(key: ConversationKey) -> Self {
        Self {
            key,
            lines: VecDeque::new(),
            unread: 0,
            typing: BTreeMap::new(),
            pending_invite: false,
            recall: Vec::new(),
            revision: 0,
        }
    }
}

/// A conversation's resolved tab title: the Nearby tab is localized at render
/// time, everything else carries its already-readable text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConversationTitle {
    /// The Nearby Chat tab — render as the localized [`NEARBY_TITLE_KEY`].
    Nearby,
    /// A named conversation — a peer, group or conference name (or a short-id
    /// fallback until the real name is known).
    Named(String),
}

/// The pure conversation model: the ordered list, the active conversation, and
/// the name caches that resolve a key to a readable title. Fed only from the
/// event stream.
#[derive(Resource, Debug)]
pub(crate) struct ConversationModel {
    /// The conversations, in tab order — Nearby is always index 0.
    entries: Vec<Conversation>,
    /// The active conversation — keyed (not indexed) so removing a tab never
    /// aliases the selection onto a different conversation.
    active: ConversationKey,
    /// Last-seen display name per agent, for one-to-one tab titles.
    agent_names: BTreeMap<AgentKey, String>,
    /// Resolved group names, harvested from membership / name / profile / invite
    /// events, for group tab titles.
    group_names: BTreeMap<GroupKey, String>,
    /// Resolved conference names, harvested from conference invites.
    conference_names: BTreeMap<ImSessionId, String>,
}

impl Default for ConversationModel {
    fn default() -> Self {
        Self {
            // Seed the always-present Nearby tab as the first, active conversation.
            entries: vec![Conversation::new(ConversationKey::Nearby)],
            active: ConversationKey::Nearby,
            agent_names: BTreeMap::new(),
            group_names: BTreeMap::new(),
            conference_names: BTreeMap::new(),
        }
    }
}

impl ConversationModel {
    /// The active conversation's key.
    const fn active_key(&self) -> ConversationKey {
        self.active
    }

    /// The index of `key`, or `None` if no such conversation exists yet.
    fn index_of(&self, key: ConversationKey) -> Option<usize> {
        self.entries.iter().position(|entry| entry.key == key)
    }

    /// The index of `key`, creating the conversation (appended after the existing
    /// tabs) if it does not exist. Nearby is always present, so only IM / group /
    /// conference keys are ever created here.
    fn ensure(&mut self, key: ConversationKey) -> usize {
        if let Some(index) = self.index_of(key) {
            return index;
        }
        self.entries.push(Conversation::new(key));
        self.entries.len().saturating_sub(1)
    }

    /// Records a peer's display name (ignoring empty names).
    fn note_agent_name(&mut self, id: AgentKey, name: &str) {
        if !name.is_empty() {
            self.agent_names.insert(id, name.to_owned());
        }
    }

    /// Records a group's display name (ignoring empty names).
    fn note_group_name(&mut self, id: GroupKey, name: &str) {
        if !name.is_empty() {
            self.group_names.insert(id, name.to_owned());
        }
    }

    /// Records a conference's display name (ignoring empty names).
    fn note_conference_name(&mut self, id: ImSessionId, name: &str) {
        if !name.is_empty() {
            self.conference_names.insert(id, name.to_owned());
        }
    }

    /// Appends a line to `key`'s transcript (creating the conversation if new),
    /// bumping its revision and, for a **remote** line to a non-active tab, its
    /// unread count.
    fn push_line(&mut self, key: ConversationKey, line: TranscriptLine) {
        let active = self.active;
        let index = self.ensure(key);
        let is_own = line.own;
        if let Some(entry) = self.entries.get_mut(index) {
            entry.lines.push_back(line);
            while entry.lines.len() > HISTORY_CAP {
                entry.lines.pop_front();
            }
            entry.revision = entry.revision.wrapping_add(1);
            // A message means the session is live — an invite that was pending is
            // now joined.
            entry.pending_invite = false;
            if !is_own && key != active {
                entry.unread = entry.unread.saturating_add(1);
            }
        }
    }

    /// A remote speaker's line for `key` (also clears that speaker's typing flag).
    fn push_remote(
        &mut self,
        key: ConversationKey,
        speaker_id: AgentKey,
        speaker: &str,
        body: &str,
    ) {
        self.push_line(
            key,
            TranscriptLine {
                own: false,
                speaker: speaker.to_owned(),
                body: body.to_owned(),
            },
        );
        self.clear_typing(key, speaker_id);
    }

    /// A remote nearby-chat line, whose typed speaker id we do not always hold —
    /// so it clears no typing flag.
    fn push_nearby(&mut self, speaker: &str, body: &str) {
        self.push_line(
            ConversationKey::Nearby,
            TranscriptLine {
                own: false,
                speaker: speaker.to_owned(),
                body: body.to_owned(),
            },
        );
    }

    /// Our own outbound line for `key` (the local echo the grid does not send
    /// back for IM / group / conference).
    fn push_own(&mut self, key: ConversationKey, body: &str) {
        self.push_line(
            key,
            TranscriptLine {
                own: true,
                speaker: String::new(),
                body: body.to_owned(),
            },
        );
    }

    /// The number of **live** nearby-chat lines currently held — what the recall
    /// query must skip (`already_shown`) so it only surfaces persisted history
    /// *older* than the live tail already on screen.
    fn nearby_live_len(&self) -> usize {
        self.index_of(ConversationKey::Nearby)
            .and_then(|index| self.entries.get(index))
            .map_or(0, |entry| entry.lines.len())
    }

    /// Replace the Nearby tab's **recall** lines (the persisted history rendered
    /// above the live transcript) and bump its revision so the view re-renders.
    /// A no-op if the Nearby tab is somehow absent.
    fn set_nearby_recall(&mut self, lines: Vec<TranscriptLine>) {
        if let Some(index) = self.index_of(ConversationKey::Nearby)
            && let Some(entry) = self.entries.get_mut(index)
        {
            entry.recall = lines;
            entry.revision = entry.revision.wrapping_add(1);
        }
    }

    /// Sets or clears a typist for `key`. A typing notification for an existing
    /// conversation (or a new one-to-one IM) opens / updates it; a stop just
    /// clears the flag.
    fn set_typing(&mut self, key: ConversationKey, agent: AgentKey, name: &str, typing: bool) {
        if typing {
            // A one-to-one typing notification opens the IM (the reference does);
            // a group / conference typing only updates an already-open session.
            if key.is_nearby()
                || matches!(key, ConversationKey::Direct(_))
                || self.index_of(key).is_some()
            {
                let index = self.ensure(key);
                if let Some(entry) = self.entries.get_mut(index) {
                    entry.typing.insert(agent, name.to_owned());
                }
            }
        } else {
            self.clear_typing(key, agent);
        }
    }

    /// Clears `agent` from `key`'s typing set, if present.
    fn clear_typing(&mut self, key: ConversationKey, agent: AgentKey) {
        if let Some(index) = self.index_of(key)
            && let Some(entry) = self.entries.get_mut(index)
        {
            entry.typing.remove(&agent);
        }
    }

    /// Marks `key` as a pending invite (creating the conversation if new).
    fn mark_invite(&mut self, key: ConversationKey) {
        let index = self.ensure(key);
        if let Some(entry) = self.entries.get_mut(index) {
            entry.pending_invite = true;
        }
    }

    /// Clears `key`'s pending-invite flag (on accept).
    fn accept_invite(&mut self, key: ConversationKey) {
        if let Some(index) = self.index_of(key)
            && let Some(entry) = self.entries.get_mut(index)
        {
            entry.pending_invite = false;
        }
    }

    /// Makes `key`'s conversation the active one and clears its unread count.
    /// A no-op if there is no such conversation.
    fn select(&mut self, key: ConversationKey) {
        if let Some(index) = self.index_of(key) {
            self.active = key;
            if let Some(entry) = self.entries.get_mut(index) {
                entry.unread = 0;
            }
        }
    }

    /// Removes `key`'s conversation (never the Nearby tab). If it was active, the
    /// selection falls back to Nearby. Returns whether anything was removed.
    fn close(&mut self, key: ConversationKey) -> bool {
        if key.is_nearby() {
            return false;
        }
        let Some(index) = self.index_of(key) else {
            return false;
        };
        self.entries.remove(index);
        if self.active == key {
            self.active = ConversationKey::Nearby;
        }
        true
    }

    /// The resolved tab title for `key`, from the name caches, with a short-id
    /// fallback until the real name is known.
    fn title(&self, key: ConversationKey) -> ConversationTitle {
        match key {
            ConversationKey::Nearby => ConversationTitle::Nearby,
            ConversationKey::Direct(id) => ConversationTitle::Named(
                self.agent_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| short_id(id.uuid())),
            ),
            ConversationKey::Group(id) => ConversationTitle::Named(
                self.group_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| short_id(id.uuid())),
            ),
            ConversationKey::Conference(id) => ConversationTitle::Named(
                self.conference_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| short_id(id.get())),
            ),
        }
    }

    /// Whether any **IM / group / conference** conversation (not nearby chat) has
    /// unread lines — what makes the toolbar Conversations button flash while the
    /// window is closed.
    pub(crate) fn has_im_attention(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| !entry.key.is_nearby() && entry.unread > 0)
    }

    /// The conversation an [`SlSessionEvent::ImTyping`] belongs to: an existing
    /// group / conference session keyed by the wire session id, else the
    /// one-to-one IM with the typist.
    fn typing_key(&self, from_agent_id: AgentKey, session_id: Uuid) -> ConversationKey {
        let group = ConversationKey::Group(GroupKey::from(session_id));
        if self.index_of(group).is_some() {
            return group;
        }
        let conference = ConversationKey::Conference(ImSessionId::from(session_id));
        if self.index_of(conference).is_some() {
            return conference;
        }
        ConversationKey::Direct(from_agent_id)
    }
}

/// A short, readable stand-in for an unresolved id — the first eight hex digits.
fn short_id(id: Uuid) -> String {
    id.simple().to_string().chars().take(8).collect()
}

/// The [`Command`] that sends `message` into the conversation `key`, or `None`
/// for the Nearby tab (whose send goes through the local-chat path instead).
///
/// A pure mapping so the routing is unit-testable without a live session.
fn command_for(key: ConversationKey, message: String) -> Option<Command> {
    match key {
        ConversationKey::Nearby => None,
        ConversationKey::Direct(to_agent_id) => Some(Command::InstantMessage {
            to_agent_id,
            message,
        }),
        ConversationKey::Group(group_id) => Some(Command::SendGroupMessage { group_id, message }),
        ConversationKey::Conference(session_id) => Some(Command::SendConferenceMessage {
            session_id,
            message,
        }),
    }
}

/// The accept / decline [`Command`] for a pending invite `key`, or `None` for a
/// key that is never an invite (Nearby / Direct).
fn invite_command(key: ConversationKey, accept: bool) -> Option<Command> {
    let (session_id, from_group) = match key {
        ConversationKey::Group(id) => (ImSessionId::from(id.uuid()), true),
        ConversationKey::Conference(id) => (id, false),
        ConversationKey::Nearby | ConversationKey::Direct(_) => return None,
    };
    Some(if accept {
        Command::AcceptChatInvite {
            session_id,
            from_group,
        }
    } else {
        Command::DeclineChatInvite {
            session_id,
            from_group,
        }
    })
}

/// Renders a transcript to a single string, `you` labelling our own lines. Takes
/// an iterator so the caller can render the persisted **recall** lines and the
/// live lines as one block (`recall.iter().chain(lines.iter())`) without joining
/// two owned collections.
fn format_transcript<'line>(
    lines: impl IntoIterator<Item = &'line TranscriptLine>,
    you: &str,
) -> String {
    let mut out = String::new();
    for line in lines {
        if !out.is_empty() {
            out.push('\n');
        }
        let speaker = if line.own { you } else { &line.speaker };
        out.push_str(speaker);
        out.push_str(": ");
        out.push_str(&line.body);
    }
    out
}

/// The label a tab shows: its title, with a trailing unread badge when it has
/// unread lines and is not the active tab.
fn tab_label(title: &str, unread: u32, active: bool) -> String {
    if unread > 0 && !active {
        format!("{title} ({unread})")
    } else {
        title.to_owned()
    }
}

// ---------------------------------------------------------------------------
// ECS side
// ---------------------------------------------------------------------------

/// The floater entities and the per-conversation views — the ECS mirror of
/// [`ConversationModel`].
#[derive(Resource, Debug)]
pub(crate) struct ConversationsUi {
    /// The floater root (carries [`crate::ui::UiPanelShown`]); open / close by
    /// flipping it.
    panel: Entity,
    /// The vertical tab strip the tab buttons flow into.
    strip: Entity,
    /// The panel area the per-conversation panes stack in (only the active one
    /// is displayed).
    panel_area: Entity,
    /// This floater's own dock host, anchored beside the nearby-chat bar at the
    /// bottom leading corner ([`position_conversations_dock_host`]) — so docking
    /// sends the window there rather than to the shared top-trailing host.
    dock_host: Entity,
    /// The Nearby tab's local-chat input field, so its [`LocalChatSubmit`] is
    /// routed to `Command::Chat` (and not mistaken for an IM).
    nearby_field: Entity,
    /// One view per conversation, keyed by the model's key.
    views: BTreeMap<ConversationKey, ConversationView>,
}

impl ConversationsUi {
    /// The floater root, for the toolbar / menu toggles.
    pub(crate) const fn panel(&self) -> Entity {
        self.panel
    }

    /// The vertical tab strip, so an external pane ([`crate::people`]) can add its
    /// own pinned tab button into the same strip as the conversation tabs.
    pub(crate) const fn strip(&self) -> Entity {
        self.strip
    }

    /// The panel area, so an external pane can stack its own pane beside the
    /// conversation panes (only one of all of them is ever displayed).
    pub(crate) const fn panel_area(&self) -> Entity {
        self.panel_area
    }
}

/// The ECS nodes of one conversation's tab and pane.
#[derive(Debug, Clone, Copy)]
struct ConversationView {
    /// The tab button box (recoloured active / inactive / flashing).
    tab_button: Entity,
    /// The tab's label text node (title + unread badge).
    tab_label: Entity,
    /// The pane node (displayed only while this is the active conversation).
    panel: Entity,
    /// The pending-invite bar (Accept / Decline), shown only while invited.
    invite_bar: Entity,
    /// The single transcript text node, rebuilt when the revision advances.
    transcript_text: Entity,
    /// The transcript scroll container, pinned to the bottom on a new line.
    transcript_scroll: Entity,
    /// The "X is typing…" status node, shown only while someone is typing.
    typing_text: Entity,
    /// The tab's chat input field, for routing its submits to the right send.
    input_field: Entity,
    /// The revision this view last rendered, so an unchanged transcript is not
    /// rebuilt.
    rendered_revision: u64,
}

/// A request to make `key`'s conversation the active one — written by a tab
/// button's press observer, applied by [`apply_conversation_selection`].
#[derive(Message, Debug, Clone, Copy)]
struct SelectConversation {
    /// The conversation to activate.
    key: ConversationKey,
}

/// A request to close `key`'s conversation — written by a tab's close button.
#[derive(Message, Debug, Clone, Copy)]
struct CloseConversation {
    /// The conversation to close.
    key: ConversationKey,
}

/// A request to accept (`accept`) or decline a pending invite `key` — written by
/// the invite bar's buttons.
#[derive(Message, Debug, Clone, Copy)]
struct RespondToInvite {
    /// The invited conversation.
    key: ConversationKey,
    /// Whether to accept (else decline).
    accept: bool,
}

/// A request to open (create if needed) and activate `key`'s conversation — the
/// hook another module uses to start an IM from outside the floater. The
/// [`crate::people`] Friends list writes this to open a one-to-one IM tab for a
/// selected friend in this same floater.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenConversation {
    /// The conversation to open and select.
    pub(crate) key: ConversationKey,
}

/// Which surface currently owns the conversations floater's shared strip and
/// panel area: a **conversation** pane, or an **external** pane hosted in the
/// same strip (the People / Contacts tab, [`crate::people`]). The two are
/// mutually exclusive so exactly one pane is ever displayed — when an external
/// pane owns the strip, every conversation pane is suppressed and no conversation
/// tab reads as active.
///
/// Kept deliberately generic (an `external` flag, not "people") so this module
/// stays unaware of what the other pane *is* — it only needs to know that
/// something outside its own tab set is currently front.
#[derive(Resource, Debug, Default)]
pub(crate) struct StripFocus {
    /// `true` when a non-conversation (external) pane owns the strip.
    external: bool,
}

impl StripFocus {
    /// Whether an external (non-conversation) pane currently owns the strip.
    pub(crate) const fn is_external(&self) -> bool {
        self.external
    }

    /// Give the strip to the external pane (the People tab was selected).
    pub(crate) const fn take_external(&mut self) {
        self.external = true;
    }
}

/// One-shot latch for the **nearby-chat history recall**: set once the recall
/// query has been issued so it fires exactly once. The viewer logs in once per
/// process, so it need not reset within a session (a relogin re-runs the app).
#[derive(Resource, Debug, Default)]
struct NearbyRecallState {
    /// Whether the recall query has already been sent.
    requested: bool,
}

/// The plugin: the model + UI resources, the floater spawn, and the systems that
/// ingest events, spawn / close tabs, refresh the view and route input.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ConversationsPlugin;

impl Plugin for ConversationsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConversationModel>()
            .init_resource::<StripFocus>()
            .init_resource::<NearbyRecallState>()
            .add_message::<SelectConversation>()
            .add_message::<CloseConversation>()
            .add_message::<RespondToInvite>()
            .add_message::<OpenConversation>()
            .add_systems(
                Startup,
                spawn_conversations_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    ingest_conversation_events,
                    open_conversations,
                    apply_conversation_selection,
                    respond_to_invites,
                    close_conversations,
                    spawn_conversation_tabs,
                    refresh_conversations,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    request_nearby_recall,
                    route_conversation_input,
                    scroll_active_transcript,
                    position_conversations_dock_host,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Startup: spawn the Conversations floater's own dock host, then the floater
/// (wired to that host) and seed the Nearby tab.
fn spawn_conversations_floater(mut commands: Commands, root: Res<UiRoot>) {
    // This floater's own dock host, at the bottom leading corner — an absolute,
    // bottom-pinned container the floater docks into instead of the shared
    // top-trailing host. Empty (and so invisible) until docked;
    // [`position_conversations_dock_host`] keeps it pinned above the nearby-chat
    // bar. Sits one above the bottom bar ([`DOCK_HOST_Z`]) so a floater docked
    // against the bar still takes clicks on its chat input.
    let dock_host = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..column(Val::Px(4.0))
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(DOCK_INSET),
                block_end: Val::Px(0.0),
                ..LogicalRect::AUTO
            }),
            LogicalPadding(LogicalRect::all(Val::Px(4.0))),
            BackgroundColor(DOCK_HOST_BACKGROUND),
            GlobalZIndex(DOCK_HOST_Z),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("conversations-dock-host"),
            ChildOf(root.0),
        ))
        .id();

    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: CONVERSATIONS_FLOATER_ID,
            title: "Conversations".to_owned(),
            position: Vec2::new(60.0, 80.0),
            default_size: Some(DEFAULT_SIZE),
            min_size: Some(MIN_SIZE),
            // Docks into this floater's own host beside the nearby-chat bar (see
            // `dock_host` above) rather than the shared top-trailing host — the
            // dock button and free-floating tear-off work as usual.
            dock_host: Some(dock_host),
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: true,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(crate::i18n::Translated::new("conversations-title"));

    // The content slot is a column; fill it with one row: [strip | divider | pane].
    let split = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::ZERO)
            },
            Name::new("conversations-split"),
            ChildOf(handle.content),
        ))
        .id();

    // The vertical tab strip — a scrolling column of tab buttons. It reuses the
    // tab widget's `TabStrip` / `TabStripWidth` so the split is a draggable,
    // persisted divider (crate::floater_persist keys on those); no ui_tab system
    // drives a bare `TabStrip`, so it only supplies the width + persistence key.
    let strip = commands
        .spawn((
            Node {
                width: Val::Px(STRIP_WIDTH),
                flex_shrink: 0.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..column(Val::Px(2.0))
            },
            ScrollPosition::default(),
            BackgroundColor(TAB_INACTIVE_BACKGROUND),
            TabStrip {
                element: STRIP_ELEMENT,
                active: 0,
            },
            TabStripWidth(STRIP_WIDTH),
            Name::new("conversations-strip"),
            ChildOf(split),
        ))
        .id();

    spawn_divider(&mut commands, split, strip);

    // The panel area — the panes stack here, only the active one displayed.
    let panel_area = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::ZERO)
            },
            BackgroundColor(PANEL_BACKGROUND),
            Name::new("conversations-panel-area"),
            ChildOf(split),
        ))
        .id();

    // Seed the Nearby tab's view directly (its input is the local-chat widget).
    let mut views = BTreeMap::new();
    let view = spawn_conversation_view(&mut commands, strip, panel_area, ConversationKey::Nearby);
    let nearby_field = view.input_field;
    views.insert(ConversationKey::Nearby, view);

    commands.insert_resource(ConversationsUi {
        panel: handle.root,
        strip,
        panel_area,
        dock_host,
        nearby_field,
        views,
    });
}

/// Spawn the draggable divider between the strip and the pane area — reuses the
/// tab widget's width math ([`resize_strip_width`]) so a leading strip grows and
/// shrinks correctly under LTR and RTL.
fn spawn_divider(commands: &mut Commands, split: Entity, strip: Entity) {
    let divider = commands
        .spawn((
            Node {
                width: Val::Px(DIVIDER_THICKNESS),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(DIVIDER_COLOR),
            TabDivider { strip },
            Pickable::default(),
            Name::new("conversations-divider"),
            ChildOf(split),
        ))
        .id();
    commands.spawn((
        Node {
            width: Val::Px(DIVIDER_THICKNESS * 0.5),
            height: Val::Px(DIVIDER_GRIP_LENGTH),
            border_radius: BorderRadius::all(Val::Px(DIVIDER_THICKNESS * 0.25)),
            ..default()
        },
        BackgroundColor(DIVIDER_GRIP_COLOR),
        Pickable::IGNORE,
        Name::new("conversations-divider-grip"),
        ChildOf(divider),
    ));
    commands.entity(divider).observe(
        move |drag: On<Pointer<Drag>>,
              mut widths: Query<&mut TabStripWidth>,
              direction: Res<UiDirection>| {
            if drag.button != PointerButton::Primary {
                return;
            }
            if let Ok(mut width) = widths.get_mut(strip) {
                width.0 = resize_strip_width(
                    width.0,
                    drag.delta.x,
                    TabPlacement::InlineStart,
                    *direction,
                );
            }
        },
    );
}

/// Spawn one conversation's tab button and pane, returning the view. The Nearby
/// tab uses the local-chat-input widget and has no close button; every other tab
/// uses the plain chat-input and a close button.
fn spawn_conversation_view(
    commands: &mut Commands,
    strip: Entity,
    panel_area: Entity,
    key: ConversationKey,
) -> ConversationView {
    let nearby = key.is_nearby();
    // The tab button — a row of [label | close], padded and bordered.
    let tab_button = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..row(Val::Px(4.0))
            },
            BorderColor::all(TAB_BORDER),
            BackgroundColor(TAB_INACTIVE_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("conversations-tab"),
            ChildOf(strip),
        ))
        .observe(
            move |press: On<Pointer<Press>>, mut select: MessageWriter<SelectConversation>| {
                if press.button == PointerButton::Primary {
                    select.write(SelectConversation { key });
                }
            },
        )
        .id();
    let tab_label = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(TAB_LABEL_COLOR),
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            },
            Pickable::IGNORE,
            Name::new("conversations-tab-label"),
            ChildOf(tab_button),
        ))
        .id();
    // The pane — [invite bar | transcript scroll | typing | input], hidden unless
    // active. A closable tab's close button lives in the pane's top-trailing
    // corner (below), not on the strip tab — the reference viewer's arrangement.
    let panel = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                // Fill the (fixed / user-resized) panel area and allow the flex
                // children to shrink below their content — `min_height: 0` is what
                // lets the transcript scroll inside a bounded height instead of
                // growing the pane (and so the whole floater). Mirrors the
                // inventory viewport's recipe.
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                display: Display::None,
                padding: UiRect::all(Val::Px(6.0)),
                ..column(Val::Px(6.0))
            },
            Name::new("conversations-pane"),
            ChildOf(panel_area),
        ))
        .id();
    let invite_bar = spawn_invite_bar(commands, panel, key);
    let transcript_scroll = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(6.0)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(TRANSCRIPT_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("conversations-transcript-scroll"),
            ChildOf(panel),
        ))
        .id();
    let transcript_text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(TRANSCRIPT_FONT_SIZE),
            TextColor(TRANSCRIPT_COLOR),
            Node {
                width: Val::Percent(100.0),
                ..default()
            },
            Name::new("conversations-transcript"),
            ChildOf(transcript_scroll),
        ))
        .id();
    let typing_text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(TYPING_COLOR),
            Node {
                display: Display::None,
                ..default()
            },
            Name::new("conversations-typing"),
            ChildOf(panel),
        ))
        .id();

    // The input widget: local-chat for Nearby, plain chat-input otherwise.
    let input_field = if nearby {
        spawn_local_chat_input(
            commands,
            panel,
            &ChatInputSpec {
                font_size: CHROME_FONT_SIZE,
                width: Some(Val::Percent(100.0)),
                ..ChatInputSpec::new("conversations-nearby-input")
            },
        )
        .field
    } else {
        spawn_chat_input(
            commands,
            panel,
            &ChatInputSpec {
                font_size: CHROME_FONT_SIZE,
                width: Some(Val::Percent(100.0)),
                ..ChatInputSpec::new("conversations-im-input")
            },
        )
        .field
    };

    // Every closable (non-Nearby) tab carries a small ✕ in the **top-trailing
    // corner of its pane** — spawned last so it paints over the transcript, and
    // RTL-aware via `LogicalInset`. Nearby Chat cannot be closed, so it gets none.
    if !nearby {
        spawn_pane_close_button(commands, panel, key);
    }

    ConversationView {
        tab_button,
        tab_label,
        panel,
        invite_bar,
        transcript_text,
        transcript_scroll,
        typing_text,
        input_field,
        rendered_revision: u64::MAX,
    }
}

/// Spawn a conversation pane's close button — a small ✕ pinned to the pane's
/// **top-trailing corner** (`LogicalInset` so it is top-right under LTR and
/// top-left under RTL), wired to a [`CloseConversation`] press. It carries the
/// panel background so it cleanly occludes the transcript line behind it, and is
/// spawned as the pane's last child so it paints on top. The reference viewer puts
/// the session-close control on the conversation content, not the tab.
fn spawn_pane_close_button(commands: &mut Commands, panel: Entity, key: ConversationKey) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            LogicalInset(LogicalRect {
                block_start: Val::Px(2.0),
                inline_end: Val::Px(2.0),
                ..LogicalRect::AUTO
            }),
            BackgroundColor(PANEL_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("conversations-pane-close"),
            ChildOf(panel),
        ))
        .with_child((
            Text::new(CLOSE_GLYPH.to_owned()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(CLOSE_GLYPH_COLOR),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>, mut close: MessageWriter<CloseConversation>| {
                // Don't let the press bubble to the transcript / pane behind it.
                press.propagate(false);
                if press.button == PointerButton::Primary {
                    close.write(CloseConversation { key });
                }
            },
        );
}

/// Spawn a pane's pending-invite bar (a prompt plus Accept / Decline), hidden
/// until the conversation is a pending invite.
fn spawn_invite_bar(commands: &mut Commands, panel: Entity, key: ConversationKey) -> Entity {
    let bar = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                align_items: AlignItems::Center,
                padding: UiRect::all(Val::Px(6.0)),
                ..row(Val::Px(8.0))
            },
            BackgroundColor(INVITE_BACKGROUND),
            Name::new("conversations-invite-bar"),
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(CHROME_FONT_SIZE),
        TextColor(INVITE_TEXT_COLOR),
        crate::i18n::Translated::new(INVITE_PROMPT_KEY),
        Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            ..default()
        },
        Name::new("conversations-invite-prompt"),
        ChildOf(bar),
    ));
    spawn_invite_button(
        commands,
        bar,
        key,
        true,
        INVITE_ACCEPT_KEY,
        ACCEPT_BACKGROUND,
    );
    spawn_invite_button(
        commands,
        bar,
        key,
        false,
        INVITE_DECLINE_KEY,
        DECLINE_BACKGROUND,
    );
    bar
}

/// Spawn one Accept / Decline invite button.
fn spawn_invite_button(
    commands: &mut Commands,
    bar: Entity,
    key: ConversationKey,
    accept: bool,
    label_key: &'static str,
    background: Color,
) {
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(background),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("conversations-invite-button"),
            ChildOf(bar),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(INVITE_TEXT_COLOR),
            crate::i18n::Translated::new(label_key),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>, mut respond: MessageWriter<RespondToInvite>| {
                press.propagate(false);
                if press.button == PointerButton::Primary {
                    respond.write(RespondToInvite { key, accept });
                }
            },
        );
}

// ---------------------------------------------------------------------------
// Ingest
// ---------------------------------------------------------------------------

/// Fold every relevant inbound event into the model: chat / IM / group /
/// conference lines, typing notifications, invites, and the name caches behind
/// the tab titles.
fn ingest_conversation_events(
    mut events: MessageReader<SlEvent>,
    mut model: ResMut<ConversationModel>,
    identity: Res<SlIdentity>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ChatReceived(message) => {
                // Skip the typing-animation triggers and any empty line, like the
                // overlay does. Our own local chat is echoed here too, under our
                // name — the same as the overlay shows it.
                if is_displayable(&message.chat_type, &message.message) {
                    model.push_nearby(&message.from_name, &message.message);
                }
            }
            SlSessionEvent::ChatTyping {
                from_name,
                source_id,
                typing,
            } => {
                model.set_typing(
                    ConversationKey::Nearby,
                    AgentKey::from(*source_id),
                    from_name,
                    *typing,
                );
            }
            SlSessionEvent::InstantMessageReceived(im)
                if im.dialog == ImDialog::Message && !im.from_group =>
            {
                model.note_agent_name(im.from_agent_id, &im.from_agent_name);
                model.push_remote(
                    ConversationKey::Direct(im.from_agent_id),
                    im.from_agent_id,
                    &im.from_agent_name,
                    &im.message,
                );
            }
            SlSessionEvent::ImTyping {
                from_agent_id,
                from_agent_name,
                session_id,
                typing,
            } => {
                let key = model.typing_key(*from_agent_id, *session_id);
                model.note_agent_name(*from_agent_id, from_agent_name);
                model.set_typing(key, *from_agent_id, from_agent_name, *typing);
            }
            SlSessionEvent::GroupSessionMessage {
                group_id,
                from_agent_id,
                from_name,
                message,
            } => {
                // The grid broadcasts our own group message back to us; we already
                // echo it locally as "You:" on send (`route_conversation_input`),
                // so drop the self-echo to avoid showing it twice (once as "You:"
                // and once under our own name).
                if identity.agent_id != Some(*from_agent_id) {
                    model.note_agent_name(*from_agent_id, from_name);
                    model.push_remote(
                        ConversationKey::Group(*group_id),
                        *from_agent_id,
                        from_name,
                        message,
                    );
                }
            }
            SlSessionEvent::ConferenceSessionMessage {
                session_id,
                from_agent_id,
                from_name,
                message,
            } => {
                // Same self-echo suppression as group sessions (a conference
                // session likewise echoes the sender's own line back).
                if identity.agent_id != Some(*from_agent_id) {
                    model.note_agent_name(*from_agent_id, from_name);
                    model.push_remote(
                        ConversationKey::Conference(ImSessionId::from(*session_id)),
                        *from_agent_id,
                        from_name,
                        message,
                    );
                }
            }
            // An invitation opens the tab (and records its name) as a *pending
            // invite* — Accept / Decline in the pane — before any message arrives.
            SlSessionEvent::ConferenceInvited {
                session_id,
                from_group,
                session_name,
                ..
            } => {
                if *from_group {
                    let group = GroupKey::from(*session_id);
                    model.note_group_name(group, session_name);
                    model.mark_invite(ConversationKey::Group(group));
                } else {
                    let id = ImSessionId::from(*session_id);
                    model.note_conference_name(id, session_name);
                    model.mark_invite(ConversationKey::Conference(id));
                }
            }
            // Harvest group names so a group tab is titled for the group, not a
            // raw id (mirrors the chat-log's name harvesting).
            SlSessionEvent::GroupMemberships(memberships) => {
                for membership in memberships {
                    model.note_group_name(membership.group_id, &membership.group_name);
                }
            }
            SlSessionEvent::GroupNames(names) => {
                for group in names {
                    model.note_group_name(group.id, &group.name);
                }
            }
            SlSessionEvent::GroupProfileReceived(profile) => {
                model.note_group_name(profile.group_id, &profile.name);
            }
            // Persisted nearby-chat recall (reply to our one-shot
            // `QueryNearbyChatHistoryPage`): the page is newest-first, so reverse it
            // to oldest-first and set it as the Nearby tab's recall, rendered above
            // the live lines. `prev` (older pages) is ignored for the single recall
            // window today.
            SlSessionEvent::NearbyChatHistoryPage { lines, prev: _prev } => {
                let recalled: Vec<TranscriptLine> = lines
                    .iter()
                    .rev()
                    .map(|line| TranscriptLine {
                        own: false,
                        speaker: line.speaker.clone().unwrap_or_default(),
                        body: line.text.clone(),
                    })
                    .collect();
                model.set_nearby_recall(recalled);
            }
            _other => {}
        }
    }
}

/// Fire the one-shot **nearby-chat history recall** once we are logged in: ask the
/// runtime for a page of persisted local-chat history from the on-disk transcript
/// ([`crate::chat_log`]), which [`ingest_conversation_events`] renders above the
/// live lines in the Nearby tab (the reference viewer's "load previous
/// conversation"). `already_shown` is the current live nearby line count, so the
/// recall only surfaces history *older* than what is already on screen.
fn request_nearby_recall(
    identity: Res<SlIdentity>,
    model: Res<ConversationModel>,
    mut state: ResMut<NearbyRecallState>,
    mut commands: MessageWriter<SlCommand>,
) {
    if state.requested || identity.agent_id.is_none() {
        return;
    }
    commands.write(SlCommand(Command::QueryNearbyChatHistoryPage {
        already_shown: model.nearby_live_len(),
        before: None,
        limit: RECALL_LIMIT,
    }));
    state.requested = true;
}

/// Whether a nearby chat line should appear: it carries text and is not a
/// typing-animation trigger (mirrors [`crate::chat`]).
const fn is_displayable(chat_type: &ChatType, message: &str) -> bool {
    !matches!(chat_type, ChatType::StartTyping | ChatType::StopTyping) && !message.is_empty()
}

// ---------------------------------------------------------------------------
// Selection / invites / close / spawn / refresh
// ---------------------------------------------------------------------------

/// Open (create if needed) and activate conversations requested from outside the
/// floater — the [`crate::people`] Friends "IM" action opening a one-to-one tab.
/// Ensures the conversation exists so the next [`spawn_conversation_tabs`] gives
/// it a view, selects it, and hands the strip back from any external pane.
fn open_conversations(
    mut opens: MessageReader<OpenConversation>,
    mut model: ResMut<ConversationModel>,
    mut focus: ResMut<StripFocus>,
) {
    for open in opens.read() {
        model.ensure(open.key);
        model.select(open.key);
        focus.external = false;
    }
}

/// Apply the pending tab selections to the model. Selecting a conversation tab
/// also hands the strip back from any external pane ([`StripFocus`]), so its pane
/// shows and the external one is suppressed.
fn apply_conversation_selection(
    mut selections: MessageReader<SelectConversation>,
    mut model: ResMut<ConversationModel>,
    mut focus: ResMut<StripFocus>,
) {
    for selection in selections.read() {
        model.select(selection.key);
        focus.external = false;
    }
}

/// Accept / decline pending invites: send the grid command, then clear the
/// pending state (accept) or close the conversation (decline).
fn respond_to_invites(
    mut responses: MessageReader<RespondToInvite>,
    mut model: ResMut<ConversationModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    for response in responses.read() {
        if let Some(command) = invite_command(response.key, response.accept) {
            commands.write(SlCommand(command));
        }
        if response.accept {
            model.accept_invite(response.key);
        } else {
            model.close(response.key);
        }
    }
}

/// Close conversations: remove them from the model and despawn their view nodes.
fn close_conversations(
    mut closes: MessageReader<CloseConversation>,
    mut model: ResMut<ConversationModel>,
    mut ui: Option<ResMut<ConversationsUi>>,
    mut commands: Commands,
) {
    let Some(ui) = ui.as_deref_mut() else {
        return;
    };
    for close in closes.read() {
        if model.close(close.key)
            && let Some(view) = ui.views.remove(&close.key)
        {
            // Despawn the tab and pane (their children go with them).
            commands.entity(view.tab_button).despawn();
            commands.entity(view.panel).despawn();
        }
    }
}

/// Spawn a view for any model conversation that does not have one yet (a newly
/// discovered IM / group / conference). Nearby is seeded at startup.
fn spawn_conversation_tabs(
    mut commands: Commands,
    model: Res<ConversationModel>,
    mut ui: Option<ResMut<ConversationsUi>>,
) {
    let Some(ui) = ui.as_deref_mut() else {
        return;
    };
    let (strip, panel_area) = (ui.strip, ui.panel_area);
    for entry in &model.entries {
        if ui.views.contains_key(&entry.key) {
            continue;
        }
        let view = spawn_conversation_view(&mut commands, strip, panel_area, entry.key);
        ui.views.insert(entry.key, view);
    }
}

/// Keep the view in step with the model: each tab's label + colours (with the
/// unread flash), the active pane's visibility, the invite bar, the typing line,
/// and each transcript node when its revision has advanced.
#[expect(
    clippy::too_many_arguments,
    reason = "reflecting one model into its view genuinely touches the model, the view map, the \
              translator, the blink clock, and the node aspects it writes — text, background, \
              border and display/scroll; splitting them would scatter one coherent refresh across \
              systems"
)]
fn refresh_conversations(
    model: Res<ConversationModel>,
    focus: Res<StripFocus>,
    mut ui: Option<ResMut<ConversationsUi>>,
    translator: Translator,
    time: Res<Time>,
    mut texts: Query<&mut Text>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
    mut nodes: Query<&mut Node>,
    mut scrolls: Query<&mut ScrollPosition>,
) {
    let Some(ui) = ui.as_deref_mut() else {
        return;
    };
    let active_key = model.active_key();
    let you = translator.get(YOU_LABEL_KEY);
    let nearby_title = translator.get(NEARBY_TITLE_KEY);
    // The blink phase: on for the first half of each period, off for the second.
    let blink_on = (time.elapsed_secs() * BLINK_HZ).fract() < 0.5;

    for entry in &model.entries {
        let Some(view) = ui.views.get_mut(&entry.key) else {
            continue;
        };
        // A conversation reads as active only while a conversation pane owns the
        // strip; if an external pane (the People tab) holds it, every conversation
        // pane is suppressed and no conversation tab highlights.
        let is_active = !focus.is_external() && entry.key == active_key;
        let flashing = entry.unread > 0 && !is_active;

        // Tab label: resolved title + unread badge.
        let title = match model.title(entry.key) {
            ConversationTitle::Nearby => nearby_title.clone(),
            ConversationTitle::Named(name) => name,
        };
        let label = tab_label(&title, entry.unread, is_active);
        set_text(&mut texts, view.tab_label, &label);

        // Tab colours track the active one, and flash while it has unread lines.
        let (background, border) = if is_active {
            (TAB_ACTIVE_BACKGROUND, TAB_ACTIVE_BORDER)
        } else if flashing && blink_on {
            (TAB_ATTENTION_BACKGROUND, TAB_ACTIVE_BORDER)
        } else {
            (TAB_INACTIVE_BACKGROUND, TAB_BORDER)
        };
        set_background(&mut backgrounds, view.tab_button, background);
        if let Ok(mut color) = borders.get_mut(view.tab_button) {
            let wanted = BorderColor::all(border);
            if *color != wanted {
                *color = wanted;
            }
        }

        // Pane visibility — only the active pane is laid out.
        set_display(&mut nodes, view.panel, is_active);

        // The pending-invite bar shows only while invited.
        set_display(&mut nodes, view.invite_bar, entry.pending_invite);

        // The typing line: "X is typing…", hidden when nobody is.
        let typing = typing_status(&translator, &entry.typing);
        set_display(&mut nodes, view.typing_text, typing.is_some());
        if let Some(status) = typing {
            set_text(&mut texts, view.typing_text, &status);
        }

        // Transcript, only when a new line landed.
        if view.rendered_revision != entry.revision {
            view.rendered_revision = entry.revision;
            // Persisted recall lines first, then the live lines, as one block.
            let rendered = format_transcript(entry.recall.iter().chain(entry.lines.iter()), &you);
            set_text(&mut texts, view.transcript_text, &rendered);
            // Pin the scroll to the newest line.
            if let Ok(mut scroll) = scrolls.get_mut(view.transcript_scroll) {
                scroll.0.y = SCROLL_TO_BOTTOM;
            }
        }
    }
}

/// The "X is typing…" / "several are typing…" status for a typing set, or `None`
/// when nobody is typing.
fn typing_status(translator: &Translator, typing: &BTreeMap<AgentKey, String>) -> Option<String> {
    let mut names = typing.values();
    let first = names.next()?;
    if names.next().is_some() {
        Some(translator.get(TYPING_MANY_KEY))
    } else {
        Some(translator.format(TYPING_ONE_KEY, &TransArgs::new().text("name", first)))
    }
}

/// Write a text node's string only on a real change.
fn set_text(texts: &mut Query<&mut Text>, entity: Entity, value: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// Write a node's background only on a real change.
fn set_background(backgrounds: &mut Query<&mut BackgroundColor>, entity: Entity, color: Color) {
    if let Ok(mut background) = backgrounds.get_mut(entity)
        && background.0 != color
    {
        background.0 = color;
    }
}

/// Toggle a node between shown (`Flex`) and hidden (`None`) only on a real change.
fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

// ---------------------------------------------------------------------------
// Input routing
// ---------------------------------------------------------------------------

/// Route each tab's chat-input submit to the right send, and echo our own IM /
/// group / conference line into its transcript.
///
/// The Nearby tab is a local-chat widget, so its line arrives as a
/// [`LocalChatSubmit`] mapped to `Command::Chat` (the sim echoes it back, so no
/// local echo); every other tab is a plain chat-input, so its line arrives as a
/// [`ChatInputSubmit`] mapped to the session's IM / group / conference send.
fn route_conversation_input(
    ui: Option<Res<ConversationsUi>>,
    mut model: ResMut<ConversationModel>,
    mut chat_submits: MessageReader<ChatInputSubmit>,
    mut local_submits: MessageReader<LocalChatSubmit>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui.as_deref() else {
        // Drain so a pre-spawn submit is not replayed once the floater exists.
        chat_submits.clear();
        local_submits.clear();
        return;
    };

    // Nearby: the local-chat widget's resolved channel / volume / message.
    for submit in local_submits.read() {
        if submit.field != ui.nearby_field {
            continue;
        }
        commands.write(SlCommand(Command::Chat {
            message: submit.message.clone(),
            chat_type: submit.chat_type,
            channel: submit.channel,
        }));
    }

    // IM / group / conference: find the tab whose input field this came from.
    for submit in chat_submits.read() {
        let Some((&key, _view)) = ui
            .views
            .iter()
            .find(|(_key, view)| view.input_field == submit.field)
        else {
            continue;
        };
        // The Nearby field also emits a raw ChatInputSubmit (its widget wraps the
        // base one); it is handled above via LocalChatSubmit, so skip it here so a
        // local line is never re-sent as an IM.
        if key.is_nearby() {
            continue;
        }
        let message = submit.text.clone();
        if let Some(command) = command_for(key, message.clone()) {
            commands.write(SlCommand(command));
            // Echo our own line: the grid does not send IM / group / conference
            // messages back to their sender.
            model.push_own(key, &message);
        }
    }
}

/// Scroll the active conversation's transcript with the mouse wheel.
///
/// `bevy_ui` clips a scroll node but does not move it — the app owns the wheel,
/// mirroring [`crate::gallery`] and the virtual list. The offset floors at zero;
/// `bevy_ui` clamps the far end to the scrollable range at layout.
fn scroll_active_transcript(
    wheel: Res<AccumulatedMouseScroll>,
    ui: Option<Res<ConversationsUi>>,
    model: Res<ConversationModel>,
    mut scrolls: Query<&mut ScrollPosition>,
) {
    if wheel.delta.y.abs() < f32::EPSILON {
        return;
    }
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let Some(view) = ui.views.get(&model.active_key()) else {
        return;
    };
    let delta = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * LINE_SCROLL_PIXELS,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    if let Ok(mut scroll) = scrolls.get_mut(view.transcript_scroll) {
        scroll.0.y = (scroll.0.y - delta).max(0.0);
    }
}

/// Keep the Conversations floater's **dock host** anchored to the nearby-chat bar
/// at the bottom leading corner: its bottom edge sits at the **top of the bottom
/// area**, so a floater docked into it rests directly **above the nearby-chat
/// bar** when the bar is shown, and drops into the bar's place when the bar is
/// toggled off (the bottom area shrinks with it).
///
/// The host is an absolute, bottom-pinned container (empty and invisible until the
/// floater docks into it); pinning its `block_end` to the measured bottom-area
/// height makes it track the bar. This is what relocates *where* this floater
/// docks — from the shared top-trailing host to the bottom-left — while leaving
/// the dock button and free-floating tear-off intact. Only written on a real
/// change, so an idle UI does not churn layout. Mirrors
/// [`crate::chat::position_chat_overlay`], which anchors the chat overlay above
/// the same area.
fn position_conversations_dock_host(
    ui: Option<Res<ConversationsUi>>,
    bottom_area: Option<Res<BottomArea>>,
    computed: Query<&ComputedNode>,
    mut insets: Query<&mut LogicalInset>,
) {
    let (Some(ui), Some(bottom_area)) = (ui, bottom_area) else {
        return;
    };
    let Ok(area_node) = computed.get(bottom_area.area) else {
        return;
    };
    let area_height = area_node.size().y * area_node.inverse_scale_factor();
    // Pin the host's bottom edge to the top of the bottom area (above the chat bar,
    // or in its place when the bar is off); the host content-sizes upward from
    // there to fit the docked floater.
    let wanted = LogicalInset(LogicalRect {
        inline_start: Val::Px(DOCK_INSET),
        block_end: Val::Px(area_height),
        ..LogicalRect::AUTO
    });
    if let Ok(mut inset) = insets.get_mut(ui.dock_host)
        && *inset != wanted
    {
        *inset = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Command, ConversationKey, ConversationModel, ConversationTitle, TranscriptLine,
        command_for, format_transcript, invite_command, tab_label,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, GroupKey, ImSessionId, Uuid};
    use std::collections::VecDeque;

    /// A shared reference to `key`'s conversation in `model`, if it exists — the
    /// test-side lookup (the model has no non-test accessor for it).
    fn get(model: &ConversationModel, key: ConversationKey) -> Option<&super::Conversation> {
        model
            .index_of(key)
            .and_then(|index| model.entries.get(index))
    }

    /// A fresh model has the Nearby tab, active, and nothing else.
    #[test]
    fn seeds_the_nearby_tab_active() {
        let model = ConversationModel::default();
        assert_eq!(model.active_key(), ConversationKey::Nearby);
        assert_eq!(model.entries.len(), 1);
    }

    /// A remote IM creates a Direct tab, appends the line, and — because it is not
    /// the active tab — counts one unread.
    #[test]
    fn remote_im_creates_tab_and_counts_unread() {
        let mut model = ConversationModel::default();
        let peer = AgentKey::from(Uuid::from_u128(2));
        model.note_agent_name(peer, "Avatar One");
        model.push_remote(ConversationKey::Direct(peer), peer, "Avatar One", "hi");
        assert_eq!(model.index_of(ConversationKey::Direct(peer)), Some(1));
        assert_eq!(model.entries.get(1).map(|entry| entry.lines.len()), Some(1));
        assert_eq!(model.entries.get(1).map(|entry| entry.unread), Some(1));
    }

    /// Selecting a tab makes it active and clears its unread count.
    #[test]
    fn selecting_clears_unread() {
        let mut model = ConversationModel::default();
        let peer = AgentKey::from(Uuid::from_u128(3));
        model.push_remote(ConversationKey::Direct(peer), peer, "Avatar Two", "yo");
        model.select(ConversationKey::Direct(peer));
        assert_eq!(model.active_key(), ConversationKey::Direct(peer));
        assert_eq!(model.entries.get(1).map(|entry| entry.unread), Some(0));
    }

    /// A line to the active tab never counts as unread.
    #[test]
    fn active_tab_has_no_unread() {
        let mut model = ConversationModel::default();
        model.push_nearby("Avatar Three", "hello");
        assert_eq!(model.entries.first().map(|entry| entry.unread), Some(0));
    }

    /// The transcript is bounded to the history cap.
    #[test]
    fn transcript_is_bounded() {
        let mut model = ConversationModel::default();
        let peer = AgentKey::from(Uuid::from_u128(4));
        for index in 0..(super::HISTORY_CAP + 50) {
            model.push_remote(
                ConversationKey::Direct(peer),
                peer,
                "Avatar Four",
                &format!("line {index}"),
            );
        }
        assert_eq!(
            model.index_of(ConversationKey::Direct(peer)),
            Some(1),
            "the direct tab exists"
        );
        assert_eq!(
            model.entries.get(1).map(|entry| entry.lines.len()),
            Some(super::HISTORY_CAP)
        );
    }

    /// A group tab is titled by its harvested name once known, and by a short id
    /// until then.
    #[test]
    fn group_title_resolves_from_the_name_cache() {
        let mut model = ConversationModel::default();
        let group = GroupKey::from(Uuid::from_u128(0x1234_5678_9abc));
        model.ensure(ConversationKey::Group(group));
        assert!(matches!(
            model.title(ConversationKey::Group(group)),
            ConversationTitle::Named(_)
        ));
        model.note_group_name(group, "My Cool Group");
        assert_eq!(
            model.title(ConversationKey::Group(group)),
            ConversationTitle::Named("My Cool Group".to_owned())
        );
    }

    /// Closing a conversation removes it; if it was active the selection falls
    /// back to Nearby. The Nearby tab itself cannot be closed.
    #[test]
    fn closing_removes_and_falls_back_to_nearby() {
        let mut model = ConversationModel::default();
        let peer = AgentKey::from(Uuid::from_u128(5));
        model.push_remote(ConversationKey::Direct(peer), peer, "Avatar Five", "hi");
        model.select(ConversationKey::Direct(peer));
        assert_eq!(model.close(ConversationKey::Direct(peer)), true);
        assert_eq!(model.index_of(ConversationKey::Direct(peer)), None);
        assert_eq!(model.active_key(), ConversationKey::Nearby);
        // Nearby is never closable.
        assert_eq!(model.close(ConversationKey::Nearby), false);
        assert_eq!(model.entries.len(), 1);
    }

    /// An invite marks the conversation pending; a message clears it; typing sets
    /// and clears the typist.
    #[test]
    fn invite_typing_and_message_lifecycle() {
        let mut model = ConversationModel::default();
        let group = GroupKey::from(Uuid::from_u128(6));
        model.mark_invite(ConversationKey::Group(group));
        assert_eq!(
            get(&model, ConversationKey::Group(group)).map(|c| c.pending_invite),
            Some(true)
        );
        let typist = AgentKey::from(Uuid::from_u128(7));
        model.set_typing(ConversationKey::Group(group), typist, "Avatar Six", true);
        assert_eq!(
            get(&model, ConversationKey::Group(group)).map(|c| c.typing.len()),
            Some(1)
        );
        // A message clears both the pending flag and the sender's typing flag.
        model.push_remote(ConversationKey::Group(group), typist, "Avatar Six", "hey");
        let entry = get(&model, ConversationKey::Group(group));
        assert_eq!(entry.map(|c| c.pending_invite), Some(false));
        assert_eq!(entry.map(|c| c.typing.is_empty()), Some(true));
    }

    /// ImTyping resolves to an existing group / conference session, else the
    /// one-to-one IM with the typist.
    #[test]
    fn typing_key_prefers_existing_sessions() {
        let mut model = ConversationModel::default();
        let session = Uuid::from_u128(8);
        let agent = AgentKey::from(Uuid::from_u128(9));
        // No session yet → a one-to-one IM with the typist.
        assert_eq!(
            model.typing_key(agent, session),
            ConversationKey::Direct(agent)
        );
        // With a group session on that id → the group.
        model.ensure(ConversationKey::Group(GroupKey::from(session)));
        assert_eq!(
            model.typing_key(agent, session),
            ConversationKey::Group(GroupKey::from(session))
        );
    }

    /// Only IM / group / conference unread flashes the toolbar button; nearby
    /// unread does not.
    #[test]
    fn only_im_unread_is_attention() {
        let mut model = ConversationModel::default();
        let peer = AgentKey::from(Uuid::from_u128(10));
        // Nearby unread (arrives while another tab is active) is not attention.
        model.select(ConversationKey::Direct(peer));
        model.ensure(ConversationKey::Direct(peer));
        model.push_nearby("Avatar Ten", "hello");
        assert_eq!(model.has_im_attention(), false);
        // A direct IM to a non-active tab is.
        model.select(ConversationKey::Nearby);
        model.push_remote(ConversationKey::Direct(peer), peer, "Avatar Ten", "hi");
        assert_eq!(model.has_im_attention(), true);
    }

    /// Each conversation key maps to its matching send command; Nearby maps to
    /// none (it goes through the local-chat path).
    #[test]
    fn command_mapping_is_per_kind() {
        assert!(command_for(ConversationKey::Nearby, "x".to_owned()).is_none());
        let peer = AgentKey::from(Uuid::from_u128(11));
        assert!(matches!(
            command_for(ConversationKey::Direct(peer), "hi".to_owned()),
            Some(Command::InstantMessage { .. })
        ));
        let group = GroupKey::from(Uuid::from_u128(12));
        assert!(matches!(
            command_for(ConversationKey::Group(group), "hi".to_owned()),
            Some(Command::SendGroupMessage { .. })
        ));
        let conf = ImSessionId::from(Uuid::from_u128(13));
        assert!(matches!(
            command_for(ConversationKey::Conference(conf), "hi".to_owned()),
            Some(Command::SendConferenceMessage { .. })
        ));
    }

    /// Invite accept / decline map to the accept / decline commands; a group
    /// invite is flagged `from_group`, a conference is not.
    #[test]
    fn invite_command_mapping() {
        let group = GroupKey::from(Uuid::from_u128(14));
        assert!(matches!(
            invite_command(ConversationKey::Group(group), true),
            Some(Command::AcceptChatInvite {
                from_group: true,
                ..
            })
        ));
        let conf = ImSessionId::from(Uuid::from_u128(15));
        assert!(matches!(
            invite_command(ConversationKey::Conference(conf), false),
            Some(Command::DeclineChatInvite {
                from_group: false,
                ..
            })
        ));
        assert!(invite_command(ConversationKey::Nearby, true).is_none());
    }

    /// The transcript renders one `"name: body"` line each, labelling our own
    /// lines with the "you" string.
    #[test]
    fn transcript_formats_own_and_remote_lines() {
        let mut lines = VecDeque::new();
        lines.push_back(TranscriptLine {
            own: false,
            speaker: "Avatar Five".to_owned(),
            body: "hi".to_owned(),
        });
        lines.push_back(TranscriptLine {
            own: true,
            speaker: String::new(),
            body: "hey".to_owned(),
        });
        assert_eq!(
            format_transcript(lines.iter(), "You"),
            "Avatar Five: hi\nYou: hey"
        );
    }

    /// Nearby recall lines render *above* the live lines, and only the Nearby tab
    /// carries them.
    #[test]
    fn nearby_recall_renders_above_live_lines() {
        let mut model = ConversationModel::default();
        // A live line arrives first…
        model.push_nearby("Avatar Live", "live line");
        assert_eq!(model.nearby_live_len(), 1);
        // …then persisted history is recalled (oldest-first, as the ingest builds).
        model.set_nearby_recall(vec![
            TranscriptLine {
                own: false,
                speaker: "Avatar Past".to_owned(),
                body: "older".to_owned(),
            },
            TranscriptLine {
                own: false,
                speaker: "Avatar Past".to_owned(),
                body: "newer".to_owned(),
            },
        ]);
        let rendered = get(&model, ConversationKey::Nearby)
            .map(|entry| format_transcript(entry.recall.iter().chain(entry.lines.iter()), "You"));
        assert_eq!(
            rendered,
            Some("Avatar Past: older\nAvatar Past: newer\nAvatar Live: live line".to_owned())
        );
    }

    /// The unread badge shows only on an inactive tab with unread lines.
    #[test]
    fn tab_label_badges_only_inactive_unread() {
        assert_eq!(tab_label("Group", 3, false), "Group (3)");
        assert_eq!(tab_label("Group", 3, true), "Group");
        assert_eq!(tab_label("Group", 0, false), "Group");
    }
}
