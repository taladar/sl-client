//! The thin file-I/O shell over the sans-IO chat-log core
//! ([`sl_proto::chat_log`](sl_proto)).
//!
//! The pure crate owns the configuration, the Firestorm line format/parse, and the
//! filename schemes; this shell supplies the two things it cannot: the **local
//! wall-clock** (via the [`time`] crate) and the actual filesystem **append** /
//! **seek** of transcripts. It also keeps the small runtime-only caches the format
//! needs — the agent-id → display-name map (so an outbound line and a read-back can
//! name a peer) and the chat-session → transcript-path map (so a history page can
//! find the file). The runtime's run loop owns one [`ChatLog`] and drives it from
//! the inbound event stream and our own outbound commands.

use fs_err::OpenOptions;
use sl_proto::{
    AgentKey, ChatLogConfig, ChatSessionKind, ConversationKind, GroupKey, ImDialog, ImSessionId,
    LogLineTime, MessageCursor, NearbyHistoryLine, Session, SessionMessage,
    conference_log_file_name, format_log_line, group_log_file_name, im_log_file_name,
    nearby_log_file_name, parse_log_lines,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

/// Breaks an [`OffsetDateTime`] (already shifted to the local zone) into the
/// [`LogLineTime`] fields a Firestorm line renders.
fn broken_down(when: OffsetDateTime) -> LogLineTime {
    LogLineTime {
        year: when.year(),
        month: u8::from(when.month()),
        day: when.day(),
        hour: when.hour(),
        minute: when.minute(),
        second: when.second(),
    }
}

/// Appends `line` (plus a trailing newline) to the transcript at `path`, creating
/// the parent directory if needed. Returns any I/O error for the caller to log;
/// chat logging is best-effort and never fails a session.
fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Reads the whole transcript at `path` as text, so the entire history is available
/// for on-demand paging. Invalid UTF-8 bytes are replaced rather than erroring (a
/// transcript is best-effort scrollback, not a strict format).
fn read_full(path: &Path) -> std::io::Result<String> {
    let bytes = fs_err::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Loads the `conversation.log` index from `base_dir`, dropping entries whose
/// last-activity time is older than `retention_days` (Firestorm's purge-on-load).
/// Each surviving entry is keyed by its transcript file name. A missing or unreadable
/// index yields an empty map.
fn load_conversation_index(base_dir: &Path, retention_days: u32) -> BTreeMap<String, String> {
    let path = base_dir.join("conversation.log");
    let Ok(text) = fs_err::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let now = u32::try_from(OffsetDateTime::now_utc().unix_timestamp()).unwrap_or(0);
    let cutoff = now.saturating_sub(retention_days.saturating_mul(86_400));
    let mut index = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        // Keep entries newer than the cutoff (and any whose time we cannot read).
        if sl_proto::conversation_log_unix(line).is_some_and(|unix| unix < cutoff) {
            continue;
        }
        if let Some(file) = sl_proto::conversation_log_file(line) {
            index.insert(file.to_owned(), line.to_owned());
        }
    }
    index
}

/// The current Unix time in seconds (`0` on the pre-1970 impossibility), for
/// `conversation.log` activity stamps.
fn current_unix() -> u32 {
    u32::try_from(OffsetDateTime::now_utc().unix_timestamp()).unwrap_or(0)
}

/// Converts a parsed [`LogLineTime`] back to a Unix timestamp using `offset`, for a
/// read-back [`SessionMessage`]. `None` if the time is absent or out of range.
fn to_unix(time: Option<LogLineTime>, offset: UtcOffset) -> Option<u32> {
    let time = time?;
    let month = time::Month::try_from(time.month).ok()?;
    let date = time::Date::from_calendar_date(time.year, month, time.day).ok()?;
    let clock = time::Time::from_hms(time.hour, time.minute, time.second).ok()?;
    let stamp = date.with_time(clock).assume_offset(offset).unix_timestamp();
    u32::try_from(stamp).ok()
}

/// The runtime chat-log writer/reader: the configuration, the per-account base
/// directory, the local UTC offset captured once at construction, and the
/// name/path caches the format needs.
#[derive(Debug)]
pub(crate) struct ChatLog {
    /// The pure configuration (which types to log, the format knobs, the window).
    config: ChatLogConfig,
    /// The per-account directory transcripts are written **directly** under
    /// (supplied verbatim by the runtime, already per-account), or `None` to
    /// disable all file output. When `None`, [`any_enabled`](Self::any_enabled) is
    /// false and every write short-circuits.
    base_dir: Option<PathBuf>,
    /// Our own agent id, once known — the sender of our outbound lines.
    own_id: Option<AgentKey>,
    /// Our own legacy name, used to label our outbound lines and to map a
    /// read-back line back to our own id.
    own_name: String,
    /// The local UTC offset, captured once at construction (re-resolving it per
    /// line is unsound under a multi-threaded runtime).
    offset: UtcOffset,
    /// Last-seen display name per agent, for naming peers we only hold a key for.
    names: BTreeMap<AgentKey, String>,
    /// Resolved group display names, harvested from the group-membership /
    /// group-name / group-profile / conference-invite events. A group transcript is
    /// named for the group, never its raw id, so a message for a group whose name is
    /// not yet known is held in [`pending_group`](Self::pending_group) until it is.
    group_names: BTreeMap<GroupKey, String>,
    /// Group messages received before the group's name was known, held until a name
    /// arrives and then flushed in arrival order to the (now correctly named) file.
    pending_group: BTreeMap<GroupKey, Vec<PendingGroupLine>>,
    /// The transcript path each chat session writes to, for locating a session's
    /// file on a history-page read-back.
    files: BTreeMap<ChatSessionKind, PathBuf>,
    /// The optional `conversation.log` index, keyed by transcript file name (the
    /// stable per-conversation identity) → the rendered index line. Loaded (and
    /// retention-purged) at construction, upserted and rewritten as conversations
    /// see activity. Empty / unused unless `config.conversation_log` is set.
    conversations: BTreeMap<String, String>,
}

/// A group message held until the group's name is known (so it can be written to
/// the human-readable `<group> (group).txt` rather than a raw-id file).
#[derive(Debug)]
struct PendingGroupLine {
    /// The local wall-clock captured when the message arrived.
    time: LogLineTime,
    /// The sender's display name.
    from_name: String,
    /// The message text.
    message: String,
}

impl ChatLog {
    /// Builds the writer for `own_name`'s account. `agent_chat_log_dir` is the
    /// per-account directory transcripts are written **directly** under, supplied
    /// verbatim by the runtime (no derived sub-directory); `None` disables all file
    /// output. The local offset is captured now (falling back to UTC if the
    /// platform will not report it under the running threads).
    pub(crate) fn new(
        config: ChatLogConfig,
        agent_chat_log_dir: Option<PathBuf>,
        own_name: String,
        own_id: Option<AgentKey>,
    ) -> Self {
        let base_dir = agent_chat_log_dir;
        let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        let mut names = BTreeMap::new();
        if let Some(id) = own_id {
            names.insert(id, own_name.clone());
        }
        let conversations = match (&base_dir, config.conversation_log) {
            (Some(dir), true) => {
                load_conversation_index(dir, config.conversation_log_retention_days)
            }
            _disabled => BTreeMap::new(),
        };
        Self {
            config,
            base_dir,
            own_id,
            own_name,
            offset,
            names,
            group_names: BTreeMap::new(),
            pending_group: BTreeMap::new(),
            files: BTreeMap::new(),
            conversations,
        }
    }

    /// Whether any logging is enabled — the run loop's cheap gate. Requires both an
    /// output directory and at least one enabled text-chat type.
    pub(crate) fn any_enabled(&self) -> bool {
        self.base_dir.is_some() && self.config.any_enabled()
    }

    /// The current local wall-clock as a [`LogLineTime`].
    fn now(&self) -> LogLineTime {
        broken_down(OffsetDateTime::now_utc().to_offset(self.offset))
    }

    /// Upserts a `conversation.log` entry for an active conversation and rewrites the
    /// index, when the index is enabled. Keyed by the transcript `file_name` (the
    /// stable per-conversation identity). The unread flag is always clear — the
    /// shell does not track read state (that lives in the sans-IO session); the
    /// index is for conversation discovery, not unread accounting.
    fn index_conversation(
        &mut self,
        kind: ConversationKind,
        name: &str,
        participant: AgentKey,
        session_id: Uuid,
        file_name: &str,
    ) {
        if !self.config.conversation_log {
            return;
        }
        let line = sl_proto::conversation_log_line(
            current_unix(),
            kind,
            false,
            name,
            participant,
            session_id,
            file_name,
        );
        self.conversations.insert(file_name.to_owned(), line);
        self.rewrite_conversation_index();
    }

    /// Rewrites the whole `conversation.log` from the in-memory index (entries
    /// ordered by file name). Best-effort: an I/O error is logged, never propagated.
    fn rewrite_conversation_index(&self) {
        let Some(base) = &self.base_dir else {
            return;
        };
        let path = base.join("conversation.log");
        let mut body = String::new();
        for line in self.conversations.values() {
            body.push_str(line);
            body.push('\n');
        }
        if let Some(parent) = path.parent()
            && let Err(error) = fs_err::create_dir_all(parent)
        {
            tracing::warn!(path = %path.display(), %error, "failed to create chat-log dir");
            return;
        }
        if let Err(error) = fs_err::write(&path, body) {
            tracing::warn!(path = %path.display(), %error, "failed to write conversation.log");
        }
    }

    /// The local wall-clock for a wire Unix timestamp, falling back to now if it is
    /// out of range.
    fn at_unix(&self, timestamp: u32) -> LogLineTime {
        OffsetDateTime::from_unix_timestamp(i64::from(timestamp)).map_or_else(
            |_unrepresentable| self.now(),
            |when| broken_down(when.to_offset(self.offset)),
        )
    }

    /// Records a display name for `id` (ignoring empty names).
    fn note_name(&mut self, id: AgentKey, name: &str) {
        if !name.is_empty() {
            self.names.insert(id, name.to_owned());
        }
    }

    /// The best name we hold for `id` — the cached display name, or the id's UUID
    /// string as a stable fallback.
    fn name_of(&self, id: AgentKey) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| id.uuid().to_string())
    }

    /// Writes `line` to `file_name` under the base directory, recording the path
    /// for `kind` (if any) so a later history read-back can find the file. I/O
    /// errors are logged, never propagated.
    fn append(&mut self, kind: Option<ChatSessionKind>, file_name: &str, line: &str) {
        let Some(base) = &self.base_dir else {
            return;
        };
        let path = base.join(file_name);
        if let Some(kind) = kind {
            self.files.insert(kind, path.clone());
        }
        if let Err(error) = append_line(&path, line) {
            tracing::warn!(path = %path.display(), %error, "failed to write chat-log line");
        }
    }

    /// Logs a region-local **nearby** chat line (`chat.txt`).
    pub(crate) fn log_nearby(&mut self, from_name: &str, message: &str) {
        if !self.config.logs_nearby() {
            return;
        }
        let time = self.now();
        let file = nearby_log_file_name(&self.config, &time);
        let line = format_log_line(&self.config, &time, from_name, message);
        self.append(None, &file, &line);
    }

    /// Logs an **inbound 1:1 IM**, preferring the message's wire timestamp (the
    /// original time for a replayed offline IM).
    pub(crate) fn log_inbound_im(
        &mut self,
        peer: AgentKey,
        from_name: &str,
        message: &str,
        timestamp: Option<u32>,
    ) {
        if !self.config.logs_conversation(ConversationKind::Direct) {
            return;
        }
        self.note_name(peer, from_name);
        let time = timestamp.map_or_else(|| self.now(), |stamp| self.at_unix(stamp));
        let file = self.im_file_name(peer, &time);
        let line = format_log_line(&self.config, &time, from_name, message);
        self.append(Some(ChatSessionKind::Direct { peer }), &file, &line);
        self.index_direct(peer, &file);
    }

    /// Logs an **outbound 1:1 IM** (our own line, stamped now).
    pub(crate) fn log_outbound_im(&mut self, peer: AgentKey, message: &str) {
        if !self.config.logs_conversation(ConversationKind::Direct) {
            return;
        }
        let time = self.now();
        let file = self.im_file_name(peer, &time);
        let name = self.own_name.clone();
        let line = format_log_line(&self.config, &time, &name, message);
        self.append(Some(ChatSessionKind::Direct { peer }), &file, &line);
        self.index_direct(peer, &file);
    }

    /// Updates the `conversation.log` entry for a 1:1 conversation with `peer`.
    fn index_direct(&mut self, peer: AgentKey, file: &str) {
        if let Some(own) = self.own_id {
            let session_id = ChatSessionKind::Direct { peer }.canonical_session_id(own);
            let name = self.name_of(peer);
            self.index_conversation(ConversationKind::Direct, &name, peer, session_id, file);
        }
    }

    /// The IM transcript filename for `peer`, honouring the legacy-name toggle.
    fn im_file_name(&self, peer: AgentKey, time: &LogLineTime) -> String {
        let resolved = self.name_of(peer);
        let name = if self.config.legacy_im_names {
            resolved.to_lowercase().replace(' ', ".")
        } else {
            resolved
        };
        im_log_file_name(&self.config, &name, time)
    }

    /// Records a resolved group name, harvested from a membership / name-reply /
    /// profile / invite event, and flushes any group messages that were waiting on
    /// it to their now-correctly-named transcript.
    fn note_group_name(&mut self, group_id: GroupKey, name: &str) {
        if name.is_empty() {
            return;
        }
        let changed = self
            .group_names
            .insert(group_id, name.to_owned())
            .as_deref()
            != Some(name);
        if changed {
            self.flush_pending_group(group_id);
        }
    }

    /// Writes a group message to its transcript, using the resolved group name. The
    /// group name keys a human-readable file (`<group> (group).txt`); a message for
    /// a group whose name we do not yet know is **buffered** (never written under a
    /// raw id) and flushed by [`note_group_name`](Self::note_group_name) once known.
    fn write_group_line(
        &mut self,
        group_id: GroupKey,
        time: LogLineTime,
        from_name: &str,
        message: &str,
    ) {
        match self.group_names.get(&group_id) {
            Some(group_name) => {
                let group_name = group_name.clone();
                let file = group_log_file_name(&self.config, &group_name, &time);
                let line = format_log_line(&self.config, &time, from_name, message);
                self.append(Some(ChatSessionKind::Group { group_id }), &file, &line);
                let participant = self.own_id.unwrap_or_else(|| AgentKey::from(Uuid::nil()));
                self.index_conversation(
                    ConversationKind::Group,
                    &group_name,
                    participant,
                    group_id.uuid(),
                    &file,
                );
            }
            None => {
                self.pending_group
                    .entry(group_id)
                    .or_default()
                    .push(PendingGroupLine {
                        time,
                        from_name: from_name.to_owned(),
                        message: message.to_owned(),
                    });
            }
        }
    }

    /// Flushes the buffered messages for `group_id` (now that its name is known) to
    /// the transcript, in arrival order.
    fn flush_pending_group(&mut self, group_id: GroupKey) {
        for line in self.pending_group.remove(&group_id).unwrap_or_default() {
            self.write_group_line(group_id, line.time, &line.from_name, &line.message);
        }
    }

    /// Logs a **group** session message.
    pub(crate) fn log_group(
        &mut self,
        group_id: GroupKey,
        from_agent: AgentKey,
        from_name: &str,
        message: &str,
    ) {
        if !self.config.logs_conversation(ConversationKind::Group) {
            return;
        }
        self.note_name(from_agent, from_name);
        let time = self.now();
        self.write_group_line(group_id, time, from_name, message);
    }

    /// Logs a **conference** session message. The filename is the MD5 hash of the
    /// sorted `participants` (the simulator roster); with an empty roster it falls
    /// back to the session id.
    pub(crate) fn log_conference(
        &mut self,
        session_id: ImSessionId,
        participants: &BTreeSet<AgentKey>,
        from_agent: AgentKey,
        from_name: &str,
        message: &str,
    ) {
        if !self.config.logs_conversation(ConversationKind::Conference) {
            return;
        }
        self.note_name(from_agent, from_name);
        let time = self.now();
        let file = if participants.is_empty() {
            format!("Ad-hoc Conference {}.txt", session_id.get())
        } else {
            conference_log_file_name(participants)
        };
        let line = format_log_line(&self.config, &time, from_name, message);
        self.append(
            Some(ChatSessionKind::Conference { id: session_id }),
            &file,
            &line,
        );
        let participant = self.own_id.unwrap_or_else(|| AgentKey::from(Uuid::nil()));
        self.index_conversation(
            ConversationKind::Conference,
            "Ad-hoc Conference",
            participant,
            session_id.get(),
            &file,
        );
    }

    /// Observes one inbound [`Event`](sl_proto::Event), writing the matching
    /// transcript line(s) when that type's logging is enabled. The conference
    /// roster is pulled from `session` (the sans-IO chat-session registry).
    pub(crate) fn observe_event(&mut self, session: &Session, event: &sl_proto::Event) {
        match event {
            sl_proto::Event::ChatReceived(chat) => {
                self.log_nearby(&chat.from_name, &chat.message);
            }
            sl_proto::Event::InstantMessageReceived(im) if im.dialog == ImDialog::Message => {
                self.log_inbound_im(
                    im.from_agent_id,
                    &im.from_agent_name,
                    &im.message,
                    im.timestamp,
                );
            }
            sl_proto::Event::GroupSessionMessage {
                group_id,
                from_agent_id,
                from_name,
                message,
            } => {
                self.log_group(*group_id, *from_agent_id, from_name, message);
            }
            sl_proto::Event::ConferenceSessionMessage {
                session_id,
                from_agent_id,
                from_name,
                message,
            } => {
                let id = ImSessionId::from(*session_id);
                let roster: BTreeSet<AgentKey> = session
                    .participants(ChatSessionKind::Conference { id })
                    .collect();
                self.log_conference(id, &roster, *from_agent_id, from_name, message);
            }
            // Harvest group names so group transcripts are named for the group, not
            // its raw id (and flush any messages that were waiting on the name).
            sl_proto::Event::GroupMemberships(memberships) => {
                for membership in memberships {
                    self.note_group_name(membership.group_id, &membership.group_name);
                }
            }
            sl_proto::Event::GroupNames(names) => {
                for group in names {
                    self.note_group_name(group.id, &group.name);
                }
            }
            sl_proto::Event::GroupProfileReceived(profile) => {
                self.note_group_name(profile.group_id, &profile.name);
            }
            sl_proto::Event::ConferenceInvited {
                session_id,
                from_group,
                session_name,
                ..
            } if *from_group => {
                self.note_group_name(GroupKey::from(*session_id), session_name);
            }
            _other => {}
        }
    }

    /// The [`ImDialog`] a read-back line of `kind` is reconstructed with.
    const fn dialog_for(kind: ChatSessionKind) -> ImDialog {
        match kind {
            ChatSessionKind::Direct { .. } => ImDialog::Message,
            ChatSessionKind::Group { .. } | ChatSessionKind::Conference { .. } => {
                ImDialog::SessionSend
            }
        }
    }

    /// Resolves a read-back line's recovered `name` to a sender key: our own id for
    /// our name, the peer for a 1:1 conversation, a reverse name-cache hit, else a
    /// nil key (the transcript does not store sender keys).
    fn sender_for(&self, kind: ChatSessionKind, name: Option<&str>) -> AgentKey {
        if let (Some(name), Some(own)) = (name, self.own_id)
            && name == self.own_name
        {
            return own;
        }
        if let Some(name) = name {
            for (id, known) in &self.names {
                if known == name {
                    return *id;
                }
            }
        }
        match kind {
            ChatSessionKind::Direct { peer } => peer,
            _other => AgentKey::from(Uuid::nil()),
        }
    }

    /// Serves an **older** history page for `kind` from its transcript, continuing
    /// past the in-memory tail. `consumed` is the cursor's newest-first count over
    /// the unified (memory ∪ file) view; `in_memory_len` is how many newest
    /// messages live in the sans-IO ring (and thus duplicate the file's newest
    /// lines). Returns the page (newest first) and the next older cursor, or `None`
    /// when the session has no known transcript (nothing to read).
    ///
    /// The **whole** transcript is read and paged so all history is reachable on
    /// demand (unlike Firestorm's fixed `LOG_RECALL_SIZE` recall window): paging
    /// stops only at the oldest line on disk. The window from a deep paging position
    /// is still bounded by `limit`, so a single page never materialises the whole
    /// file's messages.
    pub(crate) fn read_older_page(
        &self,
        kind: ChatSessionKind,
        in_memory_len: usize,
        consumed: usize,
        limit: usize,
    ) -> Option<(Vec<SessionMessage>, Option<MessageCursor>)> {
        let path = self.files.get(&kind)?;
        let text = read_full(path).ok()?;
        let parsed = parse_log_lines(&text);
        let dialog = Self::dialog_for(kind);
        // The file's newest `in_memory_len` lines are the in-memory ring; skip them
        // plus whatever older the cursor already consumed, then take one page.
        let skip = consumed.max(in_memory_len);
        let total = parsed.len();
        let page: Vec<SessionMessage> = parsed
            .into_iter()
            .rev()
            .skip(skip)
            .take(limit)
            .map(|line| {
                let sender = self.sender_for(kind, line.name.as_deref());
                let timestamp = to_unix(line.time, self.offset);
                line.into_session_message(sender, dialog, timestamp)
            })
            .collect();
        let next = skip.saturating_add(page.len());
        // Older history remains until the page reaches the oldest line on disk.
        let prev = (next < total).then(|| MessageCursor::from_consumed(next));
        Some((page, prev))
    }

    /// Serves an **older** page of **nearby (local) chat** history from the flat
    /// transcript the runtime is appending live chat to (`chat.txt`, or the current
    /// day's `chat-YYYY-MM-DD.txt` when the date suffix is on).
    ///
    /// Nearby chat has no [`ChatSessionKind`] session and no in-memory ring, so —
    /// unlike [`read_older_page`](Self::read_older_page) — the whole history lives
    /// on disk. `already_shown` is how many of the file's **newest** lines the
    /// caller is already displaying from the live `ChatReceived` stream (they
    /// duplicate the tail and are skipped); `consumed` is the cursor's newest-first
    /// count already paged past that. Returns the page (newest first) and the next
    /// older cursor, or `None` when nearby logging is disabled or the transcript
    /// does not exist yet.
    pub(crate) fn read_nearby_older_page(
        &self,
        already_shown: usize,
        consumed: usize,
        limit: usize,
    ) -> Option<(Vec<NearbyHistoryLine>, Option<MessageCursor>)> {
        if !self.config.logs_nearby() {
            return None;
        }
        let base = self.base_dir.as_ref()?;
        let path = base.join(nearby_log_file_name(&self.config, &self.now()));
        let text = read_full(&path).ok()?;
        let parsed = parse_log_lines(&text);
        // Skip the newest lines the caller already shows live, plus whatever older
        // the cursor consumed, then take one page — the same skip discipline as
        // [`read_older_page`](Self::read_older_page).
        let skip = consumed.max(already_shown);
        let total = parsed.len();
        let page: Vec<NearbyHistoryLine> = parsed
            .into_iter()
            .rev()
            .skip(skip)
            .take(limit)
            .map(|line| NearbyHistoryLine {
                speaker: line.name,
                timestamp: to_unix(line.time, self.offset),
                text: line.message,
            })
            .collect();
        let next = skip.saturating_add(page.len());
        let prev = (next < total).then(|| MessageCursor::from_consumed(next));
        Some((page, prev))
    }
}

#[cfg(test)]
mod tests {
    use super::ChatLog;
    use pretty_assertions::assert_eq;
    use sl_proto::{
        AgentKey, ChatLogConfig, ChatSessionKind, GroupKey, LoggedChatType, MessageCursor,
    };
    use std::collections::BTreeSet;
    use uuid::Uuid;

    /// A configuration that logs IMs (the output directory is supplied separately
    /// to [`ChatLog::new`]).
    fn im_config() -> ChatLogConfig {
        ChatLogConfig {
            enabled: [LoggedChatType::InstantMessage].into_iter().collect(),
            ..ChatLogConfig::default()
        }
    }

    /// A per-test temporary directory under the system temp dir, keyed by `tag`
    /// and namespaced by crate so the byte-identical tokio / bevy shells do not
    /// collide when their test binaries run in parallel (e.g. under `nextest`).
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let crate_name = env!("CARGO_PKG_NAME");
        let dir = std::env::temp_dir().join(format!("{crate_name}-chatlog-test-{tag}"));
        let _ignored = fs_err::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_the_firestorm_line_for_an_inbound_im() {
        let dir = temp_dir("inbound-im");
        let own = AgentKey::from(Uuid::from_u128(1));
        let peer = AgentKey::from(Uuid::from_u128(2));
        let mut log = ChatLog::new(
            im_config(),
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        log.log_inbound_im(peer, "Alice Resident", "hello", None);
        let file = dir.join("Alice Resident.txt");
        let contents = fs_err::read_to_string(&file).unwrap_or_default();
        assert_eq!(contents.contains("Alice Resident: hello"), true);
        assert_eq!(contents.ends_with('\n'), true);
    }

    #[test]
    fn disabled_config_writes_nothing() {
        let dir = temp_dir("disabled");
        let own = AgentKey::from(Uuid::from_u128(1));
        let peer = AgentKey::from(Uuid::from_u128(2));
        let config = ChatLogConfig::default();
        let mut log = ChatLog::new(
            config,
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        log.log_inbound_im(peer, "Alice", "hello", None);
        assert_eq!(dir.exists(), false);
    }

    #[test]
    fn reads_older_pages_from_file_past_the_tail() {
        let dir = temp_dir("paging");
        let own = AgentKey::from(Uuid::from_u128(1));
        let peer = AgentKey::from(Uuid::from_u128(2));
        let mut log = ChatLog::new(
            im_config(),
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        // Five inbound lines; pretend two of them are still in the in-memory ring.
        for index in 0..5 {
            log.log_inbound_im(peer, "Alice Resident", &format!("line {index}"), None);
        }
        let kind = ChatSessionKind::Direct { peer };
        let page = log.read_older_page(kind, 2, 2, 2);
        let texts = page
            .as_ref()
            .map(|(messages, _)| messages.iter().map(|m| m.text.clone()).collect::<Vec<_>>());
        // Newest-first, skipping the two in-memory tail lines (4 and 3): 2 then 1.
        assert_eq!(texts, Some(vec!["line 2".to_owned(), "line 1".to_owned()]));
        let prev = page.and_then(|(_, cursor)| cursor);
        assert_eq!(prev, Some(MessageCursor::from_consumed(4)));
    }

    #[test]
    fn group_message_waits_for_name_then_flushes_to_named_file() {
        let dir = temp_dir("group-pending");
        let own = AgentKey::from(Uuid::from_u128(1));
        let group = GroupKey::from(Uuid::from_u128(9));
        let sender = AgentKey::from(Uuid::from_u128(2));
        let config = ChatLogConfig {
            enabled: [LoggedChatType::Group].into_iter().collect(),
            ..ChatLogConfig::default()
        };
        let mut log = ChatLog::new(
            config,
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        // A message arrives before the group's name is known: it must be buffered,
        // and in particular NOT written to any raw-id file.
        log.log_group(group, sender, "Alice", "hello");
        assert_eq!(dir.exists(), false);
        // The name arrives (e.g. from the login membership push); the buffered
        // message is flushed to the human-readable, named transcript.
        log.note_group_name(group, "My Cool Group");
        let file = dir.join("My Cool Group (group).txt");
        let contents = fs_err::read_to_string(&file).unwrap_or_default();
        assert_eq!(contents.contains("Alice: hello"), true);
    }

    #[test]
    fn pages_the_whole_history_to_exhaustion() {
        let dir = temp_dir("full-history");
        let own = AgentKey::from(Uuid::from_u128(1));
        let peer = AgentKey::from(Uuid::from_u128(2));
        let mut log = ChatLog::new(
            im_config(),
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        for index in 0..6 {
            log.log_inbound_im(peer, "Alice Resident", &format!("line {index}"), None);
        }
        let kind = ChatSessionKind::Direct { peer };
        // Walk from the oldest end (nothing in memory) two at a time to the start.
        let mut collected: Vec<String> = Vec::new();
        let mut cursor = Some(MessageCursor::from_consumed(0));
        while let Some(active) = cursor {
            let Some((page, prev)) = log.read_older_page(kind, 0, active.consumed_count(), 2)
            else {
                break;
            };
            collected.extend(page.iter().map(|message| message.text.clone()));
            cursor = prev;
        }
        // All six lines, newest first, with no recall-window cap truncating history.
        assert_eq!(
            collected,
            vec![
                "line 5".to_owned(),
                "line 4".to_owned(),
                "line 3".to_owned(),
                "line 2".to_owned(),
                "line 1".to_owned(),
                "line 0".to_owned(),
            ]
        );
    }

    #[test]
    fn writes_a_conversation_log_index_entry() {
        let dir = temp_dir("conv-log");
        let own = AgentKey::from(Uuid::from_u128(1));
        let peer = AgentKey::from(Uuid::from_u128(2));
        let config = ChatLogConfig {
            enabled: [LoggedChatType::InstantMessage].into_iter().collect(),
            conversation_log: true,
            ..ChatLogConfig::default()
        };
        let mut log = ChatLog::new(
            config,
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        log.log_inbound_im(peer, "Alice Resident", "hello", None);
        let conv = dir.join("conversation.log");
        let contents = fs_err::read_to_string(&conv).unwrap_or_default();
        assert_eq!(contents.contains("Alice Resident| "), true);
        assert_eq!(contents.contains("Alice Resident.txt|"), true);
    }

    #[test]
    fn purges_stale_conversation_log_entries_on_load() {
        let dir = temp_dir("conv-purge");
        let _ignored = fs_err::create_dir_all(&dir);
        // A pre-existing entry timestamped at the epoch — far older than retention.
        let stale = "[1] 0 0 0 Old Friend| aa bb Old Friend.txt|\n";
        let _written = fs_err::write(dir.join("conversation.log"), stale);
        let own = AgentKey::from(Uuid::from_u128(1));
        let peer = AgentKey::from(Uuid::from_u128(2));
        let config = ChatLogConfig {
            enabled: [LoggedChatType::InstantMessage].into_iter().collect(),
            conversation_log: true,
            ..ChatLogConfig::default()
        };
        let mut log = ChatLog::new(
            config,
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        // New activity rewrites the index; the stale entry was purged on load.
        log.log_inbound_im(peer, "Alice Resident", "hi", None);
        let contents = fs_err::read_to_string(dir.join("conversation.log")).unwrap_or_default();
        assert_eq!(contents.contains("Old Friend"), false);
        assert_eq!(contents.contains("Alice Resident.txt|"), true);
    }

    #[test]
    fn read_older_page_is_none_without_a_known_file() {
        let dir = temp_dir("no-file");
        let own = AgentKey::from(Uuid::from_u128(1));
        let peer = AgentKey::from(Uuid::from_u128(2));
        let log = ChatLog::new(
            im_config(),
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        let _participants: BTreeSet<AgentKey> = BTreeSet::new();
        assert_eq!(
            log.read_older_page(ChatSessionKind::Direct { peer }, 0, 0, 10)
                .is_none(),
            true
        );
    }

    /// A configuration that logs nearby (local) chat, for the recall tests.
    fn nearby_config() -> ChatLogConfig {
        ChatLogConfig {
            enabled: [LoggedChatType::Nearby].into_iter().collect(),
            ..ChatLogConfig::default()
        }
    }

    #[test]
    fn reads_older_nearby_pages_skipping_the_shown_tail() {
        let dir = temp_dir("nearby-paging");
        let own = AgentKey::from(Uuid::from_u128(1));
        let mut log = ChatLog::new(
            nearby_config(),
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        for index in 0..5 {
            log.log_nearby("Alice Resident", &format!("line {index}"));
        }
        // The two newest lines (4, 3) are shown live; recall two older, newest first.
        let page = log.read_nearby_older_page(2, 0, 2);
        let texts = page.as_ref().map(|(lines, _cursor)| {
            lines
                .iter()
                .map(|line| line.text.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(texts, Some(vec!["line 2".to_owned(), "line 1".to_owned()]));
        // The speaker's display name round-trips (nearby chat has no key to resolve).
        let speaker = page
            .as_ref()
            .and_then(|(lines, _cursor)| lines.first())
            .and_then(|line| line.speaker.clone());
        assert_eq!(speaker, Some("Alice Resident".to_owned()));
        let prev = page.and_then(|(_lines, cursor)| cursor);
        assert_eq!(prev, Some(MessageCursor::from_consumed(4)));
    }

    #[test]
    fn nearby_recall_walks_to_the_oldest_line() {
        let dir = temp_dir("nearby-full");
        let own = AgentKey::from(Uuid::from_u128(1));
        let mut log = ChatLog::new(
            nearby_config(),
            Some(dir.clone()),
            "Me Resident".to_owned(),
            Some(own),
        );
        for index in 0..4 {
            log.log_nearby("Alice Resident", &format!("line {index}"));
        }
        // Nothing shown live: page the whole transcript two at a time to exhaustion.
        let mut collected: Vec<String> = Vec::new();
        let mut cursor = Some(MessageCursor::from_consumed(0));
        while let Some(active) = cursor {
            let Some((page, prev)) = log.read_nearby_older_page(0, active.consumed_count(), 2)
            else {
                break;
            };
            collected.extend(page.iter().map(|line| line.text.clone()));
            cursor = prev;
        }
        assert_eq!(
            collected,
            vec![
                "line 3".to_owned(),
                "line 2".to_owned(),
                "line 1".to_owned(),
                "line 0".to_owned(),
            ]
        );
    }

    #[test]
    fn nearby_recall_is_none_when_logging_disabled() {
        let dir = temp_dir("nearby-disabled");
        let own = AgentKey::from(Uuid::from_u128(1));
        let log = ChatLog::new(
            ChatLogConfig::default(),
            Some(dir),
            "Me Resident".to_owned(),
            Some(own),
        );
        assert_eq!(log.read_nearby_older_page(0, 0, 10).is_none(), true);
    }
}
