//! A census of the mute (block) list: how a grid serves the list and over
//! which road, what it keeps of each kind of entry and each edit, how it
//! answers a request that names a cached copy, and what — if anything — it
//! withholds from an avatar that is muted.
//!
//! Avatar **A** keeps the list; avatar **B** is the resident it mutes. One
//! question a leg:
//!
//! 1. **The empty list**: A's list read with no cached copy (CRC 0), and with
//!    a CRC that names none.
//! 2. **What a muted avatar can still send**: B sends A an instant message, a
//!    typing notice, a friendship offer, a teleport offer and a shout, once
//!    while B is not muted, once muted outright, once muted with text chat
//!    excepted and once with every aspect but text chat excepted. What reaches
//!    A, and what B is told.
//! 3. **A mute and its read-back**: B muted as the reference mutes a resident
//!    (by id, under the user name); the road of the read and the row.
//! 4. **A cached copy**: the list asked for with the CRC of the list as A
//!    holds it, and with another CRC.
//! 5. **Edits**: the flags changed by a second mute of B; B muted again under
//!    another name; B unmuted under a name it was never muted under, then
//!    under each name.
//! 6. **Every kind of entry**: by name (nil id), an agent that is nobody, an
//!    object, a group and an external entry, added in that order; which are
//!    kept, in what order the list names them, and their removal.
//! 7. **A self-mute**.
//! 8. **The emptied list**: the read once every entry the census made is
//!    gone.
//!
//! The circuit is probed throughout, so a read's road — the `emptymutelist`
//! `GenericMessage`, a `MuteListUpdate` and the file over `Xfer`, or
//! `UseCachedMuteList` — is recorded with it, and any event-queue message the
//! client does not read is named.
//!
//! Names are described by the rule they follow (the user name, the legacy
//! name, the lowercase account name), never by their value, and every id by
//! what it is, so no metric names an avatar.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/friends.md` (§ Mute list); every run is held to it.
//!
//! `2av`, live grids: the fake grid keeps no mute list yet
//! (`server-fake-grid-mute-list`). Every entry the census made is removed
//! again, also when a leg fails, so a re-run starts from the list it found.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AnyMessage, ChatChannel, ChatSource, ChatType, CircuitProbe, Command, Diagnostic,
    Event, FriendKey, ImDialog, InstantMessage, MuteEntry, MuteFlags, MuteType, Reliability,
    TransactionId, Uuid, XferListing,
};
use sl_wire::messages::{
    MuteListRequest, MuteListRequestAgentDataBlock, MuteListRequestMuteDataBlock,
};

use crate::cases::asset_http::without_ids;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{DRAIN, OPENSIM_START, hold_to_measured};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, secs_metric};

/// The agent marker: an id no account has, muted as an agent.
const STRANGER_ID: &str = "c04f0117-0000-4000-8000-000000000001";
/// The object marker.
const OBJECT_ID: &str = "c04f0117-0000-4000-8000-000000000002";
/// The group marker.
const GROUP_ID: &str = "c04f0117-0000-4000-8000-000000000003";
/// The external marker.
const EXTERNAL_ID: &str = "c04f0117-0000-4000-8000-000000000004";

/// The name the agent marker is muted under (the name the one-avatar case
/// used, so a leftover of it is swept too).
const STRANGER_NAME: &str = "SL-Conformance Mute Target";
/// The name the object marker is muted under.
const OBJECT_NAME: &str = "SL-Conformance Mute Object";
/// The name the group marker is muted under.
const GROUP_NAME: &str = "SL-Conformance Mute Group";
/// The name the external marker is muted under.
const EXTERNAL_NAME: &str = "SL-Conformance Mute External";
/// The by-name entry.
const BY_NAME: &str = "SL-Conformance Mute By Name";
/// The other name B is muted under in the rename leg.
const OTHER_NAME: &str = "SL-Conformance Other Name";
/// A name B is never muted under.
const WRONG_NAME: &str = "SL-Conformance Not The Name";

/// How long a read is waited for at most.
const LISTEN: Duration = Duration::from_secs(10);

/// How long the list is re-read for an edit to show.
const VERIFY: Duration = Duration::from_secs(20);

/// How long the list is re-read for an edit the census expects may never
/// show.
const VERIFY_SHORT: Duration = Duration::from_secs(10);

/// The pause between two reads while waiting for an edit.
const POLL: Duration = Duration::from_secs(1);

/// How long A listens for what B sent in a delivery round.
const DELIVERY: Duration = Duration::from_secs(10);

/// How long B listens after a delivery round, for what it was told.
const AFTERWARDS: Duration = Duration::from_secs(3);

/// How long a grid is given to act on a change before the next leg.
const SETTLE: Duration = Duration::from_secs(2);

/// The datagram that points at a mute file.
const MUTE_LIST_UPDATE: &str = "MuteListUpdate";
/// The datagram that says the cached copy is current.
const USE_CACHED: &str = "UseCachedMuteList";
/// The datagram that carries `emptymutelist`.
const GENERIC_MESSAGE: &str = "GenericMessage";

/// What every text of the census begins with.
const TEXT_TAG: &str = "ml-";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/friends.md (mute-list, 2026-10-16)";

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer, by the metric the census records it
/// under: every run is held to it.
///
/// Not held: how long the census and the first read-back took.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "all_but_text_excepted_friendship_offer",
        both(Answer::Text("passed")),
    ),
    ("all_but_text_excepted_im", both(Answer::Text("passed"))),
    (
        "all_but_text_excepted_rows",
        both(Answer::Text("agent, avatar B, B's user name, flags 14")),
    ),
    ("all_but_text_excepted_shout", both(Answer::Text("passed"))),
    (
        "all_but_text_excepted_teleport_offer",
        both(Answer::Text("passed")),
    ),
    (
        "all_but_text_excepted_told_b",
        both(Answer::Text("nothing")),
    ),
    ("all_but_text_excepted_typing", both(Answer::Text("passed"))),
    (
        "cached_copy_current",
        both(Answer::Text("UseCachedMuteList")),
    ),
    (
        "cached_copy_stale",
        both(Answer::Text("MuteListUpdate, then the file over Xfer")),
    ),
    (
        "emptied_read",
        both(Answer::Text("emptymutelist (GenericMessage)")),
    ),
    (
        "empty_read",
        both(Answer::Text("emptymutelist (GenericMessage)")),
    ),
    (
        "empty_read_with_a_crc",
        both(Answer::Text("emptymutelist (GenericMessage)")),
    ),
    ("empty_rows", both(Answer::Count(0))),
    (
        "kind_agent",
        both(Answer::Text(
            "agent, the agent marker, the name sent, flags 0",
        )),
    ),
    (
        "kind_by_name",
        both(Answer::Text("by name, nil, the name sent, flags 0")),
    ),
    (
        "kind_external",
        both(Answer::Text(
            "external, the external marker, the name sent, flags 0",
        )),
    ),
    (
        "kind_group",
        both(Answer::Text(
            "group, the group marker, the name sent, flags 0",
        )),
    ),
    (
        "kind_object",
        both(Answer::Text(
            "object, the object marker, the name sent, flags 0",
        )),
    ),
    ("kinds_left_after_removal", both(Answer::Count(0))),
    (
        "kinds_order",
        both(Answer::Text("by_name, agent, object, group, external")),
    ),
    (
        "mute_read",
        both(Answer::Text("MuteListUpdate, then the file over Xfer")),
    ),
    (
        "mute_row",
        both(Answer::Text("agent, avatar B, B's user name, flags 0")),
    ),
    ("muted_friendship_offer", both(Answer::Text("passed"))),
    ("muted_im", both(Answer::Text("passed"))),
    ("muted_shout", both(Answer::Text("passed"))),
    ("muted_teleport_offer", both(Answer::Text("passed"))),
    ("muted_told_b", both(Answer::Text("nothing"))),
    ("muted_typing", both(Answer::Text("passed"))),
    (
        "renamed_rows",
        each(
            Answer::Text("agent, avatar B, the name sent, flags 0"),
            Answer::Text(
                "agent, avatar B, B's user name, flags 14 + agent, avatar B, the name sent, flags 0",
            ),
        ),
    ),
    ("self_mute", both(Answer::Text("no row"))),
    (
        "text_excepted_friendship_offer",
        both(Answer::Text("passed")),
    ),
    ("text_excepted_im", both(Answer::Text("passed"))),
    (
        "text_excepted_rows",
        both(Answer::Text("agent, avatar B, B's user name, flags 1")),
    ),
    ("text_excepted_shout", both(Answer::Text("passed"))),
    ("text_excepted_teleport_offer", both(Answer::Text("passed"))),
    ("text_excepted_told_b", both(Answer::Text("nothing"))),
    ("text_excepted_typing", both(Answer::Text("passed"))),
    ("unmuted_friendship_offer", both(Answer::Text("passed"))),
    ("unmuted_im", both(Answer::Text("passed"))),
    ("unmuted_shout", both(Answer::Text("passed"))),
    ("unmuted_teleport_offer", both(Answer::Text("passed"))),
    ("unmuted_told_b", both(Answer::Text("nothing"))),
    ("unmuted_typing", both(Answer::Text("passed"))),
    (
        "unmuted_under_a_name_never_muted",
        each(
            Answer::Text("no row"),
            Answer::Text(
                "agent, avatar B, B's user name, flags 14 + agent, avatar B, the name sent, flags 0",
            ),
        ),
    ),
    ("unmuted_under_the_other_name", both(Answer::Text("no row"))),
    (
        "unmuted_under_the_user_name",
        each(
            Answer::Text("no row"),
            Answer::Text("agent, avatar B, the name sent, flags 0"),
        ),
    ),
];

/// The CRC-32 a viewer names its cached copy of the list by (`LLCRC`: the
/// IEEE polynomial, reflected, `0xEDB88320`).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _step in 0..8 {
            let carry = crc & 1;
            crc >>= 1;
            if carry != 0 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    crc ^ u32::MAX
}

/// The list as the reference writes its cached copy (`LLMuteList::saveToFile`):
/// one `<type> <id> <name>|<flags>` line an entry, in the order given.
fn file_text(entries: &[MuteEntry]) -> String {
    let mut text = String::new();
    for entry in entries {
        // Writing to a `String` cannot fail.
        let _infallible = writeln!(
            text,
            "{} {} {}|{}",
            entry.mute_type.to_i32(),
            entry.id,
            entry.name,
            entry.flags.0
        );
    }
    text
}

/// The word a metric names a kind of entry by.
fn type_word(mute_type: MuteType) -> String {
    match mute_type {
        MuteType::ByName => "by name".to_owned(),
        MuteType::Agent => "agent".to_owned(),
        MuteType::Object => "object".to_owned(),
        MuteType::Group => "group".to_owned(),
        MuteType::External => "external".to_owned(),
        other => format!("type {}", other.to_i32()),
    }
}

/// The reference's user name for a legacy `first` / `last`
/// (`LLAvatarName::getUserName`): the first name alone for a Resident,
/// otherwise both.
fn user_name(first: &str, last: &str) -> String {
    if last.is_empty() || last.eq_ignore_ascii_case("Resident") {
        first.to_owned()
    } else {
        format!("{first} {last}")
    }
}

/// The lowercase account name Second Life writes in places: `first.last`, or
/// the first name alone for a Resident.
fn account_name(first: &str, last: &str) -> String {
    if last.is_empty() || last.eq_ignore_ascii_case("Resident") {
        first.to_lowercase()
    } else {
        format!("{}.{}", first.to_lowercase(), last.to_lowercase())
    }
}

/// One avatar's names, by the rule each follows.
#[derive(Debug, Clone)]
struct Names {
    /// The legacy `First Last`, `Resident` and all.
    legacy: String,
    /// The user name the reference mutes a resident under.
    user: String,
    /// The lowercase account name.
    account: String,
}

impl Names {
    /// The names of the avatar logged in as `legacy` (`First Last`).
    fn of(legacy: &str) -> Self {
        let (first, last) = legacy.split_once(' ').unwrap_or((legacy, ""));
        Self {
            legacy: legacy.to_owned(),
            user: user_name(first, last),
            account: account_name(first, last),
        }
    }

    /// Which of these names `name` is, or `None`.
    fn rule_of(&self, name: &str) -> Option<&'static str> {
        if name == self.user {
            Some("user name")
        } else if name == self.legacy {
            Some("legacy name")
        } else if name == self.account {
            Some("account name")
        } else if name.eq_ignore_ascii_case(&self.user) {
            Some("user name in another case")
        } else {
            None
        }
    }
}

/// The ids and names a metric names by what they are rather than by their
/// value.
#[derive(Debug, Clone)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// A's names.
    a_names: Names,
    /// B's names.
    b_names: Names,
    /// The markers, with their names and the word a metric calls them.
    markers: Vec<(Uuid, &'static str, &'static str)>,
}

impl Scene {
    /// What `id` is the id of.
    fn id_text(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nil"
        } else if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else {
            self.markers
                .iter()
                .find(|(marker, _name, _word)| *marker == id)
                .map_or("another id", |(_marker, _name, word)| *word)
        }
    }

    /// Whose name `name` is, by rule.
    fn name_text(&self, name: &str) -> String {
        if name.is_empty() {
            return "no name".to_owned();
        }
        if let Some(rule) = self.a_names.rule_of(name) {
            return format!("A's {rule}");
        }
        if let Some(rule) = self.b_names.rule_of(name) {
            return format!("B's {rule}");
        }
        if [BY_NAME, OTHER_NAME, WRONG_NAME]
            .into_iter()
            .chain(self.markers.iter().map(|(_id, marker, _word)| *marker))
            .any(|known| known == name)
        {
            return "the name sent".to_owned();
        }
        format!("the name {:?}", self.scrubbed(name))
    }

    /// `text` with every id and every name of the two avatars replaced.
    fn scrubbed(&self, text: &str) -> String {
        let mut out = without_ids(text);
        for (names, word) in [(&self.a_names, "<A>"), (&self.b_names, "<B>")] {
            for name in [&names.legacy, &names.user, &names.account] {
                if !name.is_empty() {
                    out = out.replace(name.as_str(), word);
                }
            }
        }
        out.chars().take(160).collect()
    }

    /// One row of the list, as a metric states it.
    fn row_text(&self, entry: &MuteEntry) -> String {
        format!(
            "{}, {}, {}, flags {}",
            type_word(entry.mute_type),
            self.id_text(entry.id),
            self.name_text(&entry.name),
            entry.flags.0
        )
    }

    /// Every row of `entries` about `id`, as a metric states them.
    fn rows_text(&self, entries: &[MuteEntry], id: Uuid) -> String {
        let rows: Vec<String> = entries
            .iter()
            .filter(|entry| entry.id == id)
            .map(|entry| self.row_text(entry))
            .collect();
        if rows.is_empty() {
            "no row".to_owned()
        } else {
            rows.join(" + ")
        }
    }

    /// Whether `entry` is one the census made.
    fn is_ours(&self, entry: &MuteEntry) -> bool {
        entry.id == self.a
            || entry.id == self.b
            || (entry.id.is_nil() && entry.name == BY_NAME)
            || self
                .markers
                .iter()
                .any(|(marker, _name, _word)| *marker == entry.id)
    }
}

/// What a read of the list was answered with.
#[derive(Debug, Clone)]
enum Answered {
    /// A list (empty or not).
    List(Vec<MuteEntry>),
    /// `UseCachedMuteList`.
    Unchanged,
    /// A file that did not parse.
    Unparsed(String),
    /// Nothing, for as long as the read was waited for.
    Nothing,
}

/// One read of the list: what answered it, and the road it came by.
#[derive(Debug, Clone)]
struct Read {
    /// The answer.
    answered: Answered,
    /// The road, as a metric states it.
    road: String,
}

impl Read {
    /// The list, if the read brought one.
    fn entries(&self) -> Option<&[MuteEntry]> {
        match &self.answered {
            Answered::List(entries) => Some(entries),
            Answered::Unchanged | Answered::Unparsed(_) | Answered::Nothing => None,
        }
    }
}

/// Asks for the list with `crc` and waits up to `listen` for the answer.
async fn read(session: &mut Session, crc: u32, listen: Duration) -> Result<Read, TestFailure> {
    let agent_id = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let session_id = session
        .session_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
    let before = session.diagnostics().len();
    session
        .send(Command::Send {
            message: Box::new(AnyMessage::MuteListRequest(MuteListRequest {
                agent_data: MuteListRequestAgentDataBlock {
                    agent_id,
                    session_id,
                },
                mute_data: MuteListRequestMuteDataBlock { mute_crc: crc },
            })),
            reliability: Reliability::Reliable,
        })
        .await?;
    let answered = match session
        .wait_for(listen, |event| match event {
            Event::MuteList(entries) => Some(Answered::List(entries.clone())),
            Event::MuteListUnchanged => Some(Answered::Unchanged),
            Event::XferDecodeFailed {
                file: XferListing::MuteList,
                error,
                ..
            } => Some(Answered::Unparsed(error.to_string())),
            _ => None,
        })
        .await
    {
        Ok(answered) => answered,
        Err(TestFailure::Timeout(_)) => Answered::Nothing,
        Err(other) => return Err(other),
    };
    // The circuit probe's datagram reports trail the events a little.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let mut names: Vec<&'static str> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    for diagnostic in session.diagnostics().iter().skip(before) {
        match diagnostic {
            Diagnostic::Datagram {
                name: Some(name), ..
            } if [MUTE_LIST_UPDATE, USE_CACHED, GENERIC_MESSAGE].contains(name) => {
                names.push(name);
            }
            Diagnostic::UnknownCapsEvent { message } => {
                unread.push(format!("unread event-queue message {message}"));
            }
            Diagnostic::CapsDecodeFailed { message, .. } => {
                unread.push(format!("event-queue message {message} that did not decode"));
            }
            _ => {}
        }
    }
    unread.sort();
    unread.dedup();
    let mut road = road_text(&answered, &names);
    for line in unread {
        road.push_str("; ");
        road.push_str(&line);
    }
    Ok(Read { answered, road })
}

/// The road an answer came by, from the datagrams that came with it.
fn road_text(answered: &Answered, names: &[&str]) -> String {
    let update = names.contains(&MUTE_LIST_UPDATE);
    let cached = names.contains(&USE_CACHED);
    let generic = names.contains(&GENERIC_MESSAGE);
    match answered {
        Answered::List(entries) if update && entries.is_empty() => {
            "MuteListUpdate, then an empty file over Xfer".to_owned()
        }
        Answered::List(_) if update => "MuteListUpdate, then the file over Xfer".to_owned(),
        Answered::List(entries) if entries.is_empty() && generic => {
            "emptymutelist (GenericMessage)".to_owned()
        }
        Answered::List(_) => "a list by no road the probe saw".to_owned(),
        Answered::Unchanged if cached => "UseCachedMuteList".to_owned(),
        Answered::Unchanged => "UseCachedMuteList by no road the probe saw".to_owned(),
        Answered::Unparsed(error) => {
            format!("MuteListUpdate, then a file that did not parse: {error}")
        }
        Answered::Nothing if update => "MuteListUpdate, then no file".to_owned(),
        Answered::Nothing => "nothing".to_owned(),
    }
}

/// Re-reads the list (CRC 0) until `done` holds over it, or for `limit` at
/// most; the last read, and whether `done` held.
async fn read_until(
    session: &mut Session,
    limit: Duration,
    done: impl Fn(&[MuteEntry]) -> bool,
) -> Result<(Read, bool), TestFailure> {
    let started = Instant::now();
    loop {
        let read = read(session, 0, LISTEN).await?;
        if read.entries().is_some_and(&done) {
            return Ok((read, true));
        }
        if started.elapsed() >= limit {
            return Ok((read, false));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every instant message, in arrival order.
    ims: Vec<InstantMessage>,
    /// Every line of local chat: who said it and what.
    chat: Vec<(Option<Uuid>, String)>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// Everyone said to have started typing to this avatar.
    typing: Vec<Uuid>,
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(got: &mut Got, event: &Event) -> Option<()> {
    match event {
        Event::InstantMessageReceived(im) => got.ims.push((**im).clone()),
        Event::ChatReceived(chat) => {
            let from = match chat.source {
                ChatSource::Agent(agent) => Some(agent.uuid()),
                _ => None,
            };
            got.chat.push((from, chat.message.clone()));
        }
        Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
            got.alerts.push(message.clone());
        }
        Event::ImTyping {
            from_agent_id,
            typing: true,
            ..
        } => got.typing.push(from_agent_id.uuid()),
        _ => return None,
    }
    Some(())
}

/// Listens on `session` for `window`.
async fn watch(session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(got);
        }
        match session
            .wait_for(remaining, |event| fold(&mut got, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(got),
            Err(other) => return Err(other),
        }
    }
}

/// The word a metric names an instant-message dialog by.
fn dialog_word(dialog: ImDialog) -> String {
    format!("{dialog:?}")
}

/// Everything the legs share.
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// The ids and names the metrics name.
    scene: Scene,
    /// This run's tag, so a line of an earlier run is never taken for one of
    /// this run's.
    run_tag: String,
    /// How many texts the census has made.
    said: u32,
}

impl Census<'_> {
    /// A fresh text of the census for `word`.
    fn text(&mut self, word: &str) -> String {
        self.said = self.said.saturating_add(1);
        format!("{TEXT_TAG}{}-{}-{word}", self.run_tag, self.said)
    }

    /// A mutes `id` as `mute_type` under `name` with `flags`.
    async fn mute(
        &self,
        id: Uuid,
        name: &str,
        mute_type: MuteType,
        flags: u32,
    ) -> Result<(), TestFailure> {
        self.a
            .send(Command::Mute {
                id,
                name: name.to_owned(),
                mute_type,
                flags: MuteFlags(flags),
            })
            .await?;
        // OpenSim answers a read taken in the same instant as an edit from
        // the middle of the edit (the old row and the new one both listed);
        // no viewer reads that soon, so neither does the census.
        tokio::time::sleep(SETTLE).await;
        Ok(())
    }

    /// A removes the entry for `id` under `name`.
    async fn unmute(&self, id: Uuid, name: &str) -> Result<(), TestFailure> {
        self.a
            .send(Command::Unmute {
                id,
                name: name.to_owned(),
            })
            .await?;
        tokio::time::sleep(SETTLE).await;
        Ok(())
    }

    /// Leg 1: the empty list, read with no cached copy and with a CRC that
    /// names none.
    async fn empty_list(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let first = read(self.a, 0, LISTEN).await?;
        let rows = first.entries().map_or(-1, |entries| {
            i64::try_from(entries.len()).unwrap_or(i64::MAX)
        });
        metrics.set("empty_rows", rows);
        metrics.set("empty_read", first.road);
        let with_crc = read(self.a, 1, LISTEN).await?;
        metrics.set("empty_read_with_a_crc", with_crc.road);
        Ok(())
    }

    /// Leg 2: B sends A everything a muted resident might be kept from
    /// sending; what reaches A and what B is told, under `round`.
    async fn delivery(&mut self, round: &str, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let a = AgentKey::from(self.scene.a);
        let im = self.text("im");
        let offer = self.text("offer");
        let lure = self.text("lure");
        let shout = self.text("shout");
        let commands = vec![
            Command::InstantMessage {
                to_agent_id: a,
                message: im.clone(),
            },
            Command::ImTyping {
                to_agent_id: a,
                typing: true,
            },
            Command::ImTyping {
                to_agent_id: a,
                typing: false,
            },
            Command::OfferFriendship {
                to_agent_id: a,
                message: offer.clone(),
            },
            Command::OfferTeleport {
                targets: vec![a],
                message: lure.clone(),
            },
            Command::Chat {
                message: shout.clone(),
                chat_type: ChatType::Shout,
                channel: ChatChannel(0),
            },
        ];
        for command in commands {
            self.b.send(command).await?;
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        let got_a = watch(self.a, DELIVERY).await?;
        let got_b = watch(self.b, AFTERWARDS).await?;
        let b_id = self.scene.b;
        let from_b = |dialog: ImDialog, text: Option<&str>| {
            got_a.ims.iter().find(|message| {
                message.from_agent_id.uuid() == b_id
                    && message.dialog == dialog
                    && text.is_none_or(|text| message.message == text)
            })
        };
        let passed = |found: bool| if found { "passed" } else { "kept from A" };
        metrics.set(
            &format!("{round}_im"),
            passed(from_b(ImDialog::Message, Some(&im)).is_some()),
        );
        metrics.set(
            &format!("{round}_typing"),
            passed(got_a.typing.contains(&b_id)),
        );
        let offered = from_b(ImDialog::FriendshipOffered, None).cloned();
        metrics.set(
            &format!("{round}_friendship_offer"),
            passed(offered.is_some()),
        );
        metrics.set(
            &format!("{round}_teleport_offer"),
            passed(from_b(ImDialog::LureUser, None).is_some()),
        );
        metrics.set(
            &format!("{round}_shout"),
            passed(
                got_a
                    .chat
                    .iter()
                    .any(|(from, text)| *from == Some(b_id) && *text == shout),
            ),
        );
        metrics.set(&format!("{round}_told_b"), self.told_text(&got_b));
        // An offer A was sent is answered, so no friendship is left pending.
        if let Some(offer) = offered {
            self.a
                .send(Command::DeclineFriendship {
                    transaction_id: TransactionId::from(offer.id),
                    friend_id: FriendKey::from(b_id),
                    use_offline_cap: false,
                })
                .await?;
            let _declined = watch(self.b, SETTLE).await?;
        }
        Ok(())
    }

    /// What B was told during a delivery round, as a metric states it: every
    /// instant message to it but its own echoes, and every alert.
    fn told_text(&self, got: &Got) -> String {
        let mut told: Vec<String> = got
            .ims
            .iter()
            .filter(|message| message.from_agent_id.uuid() != self.scene.b)
            .map(|message| {
                format!(
                    "{} from {}: {:?}",
                    dialog_word(message.dialog),
                    self.scene.id_text(message.from_agent_id.uuid()),
                    self.scene.scrubbed(&message.message)
                )
            })
            .collect();
        told.extend(
            got.alerts
                .iter()
                .map(|alert| format!("alert {:?}", self.scene.scrubbed(alert))),
        );
        if told.is_empty() {
            "nothing".to_owned()
        } else {
            told.join("; ")
        }
    }

    /// Leg 3: B muted as the reference mutes a resident; the road of the read
    /// and the row.
    async fn mute_b(&mut self, metrics: &mut Metrics) -> Result<Vec<MuteEntry>, TestFailure> {
        let b = self.scene.b;
        let name = self.scene.b_names.user.clone();
        self.mute(b, &name, MuteType::Agent, 0).await?;
        let started = Instant::now();
        let (read, kept) = read_until(self.a, VERIFY, |entries| {
            entries.iter().any(|entry| entry.id == b)
        })
        .await?;
        if kept {
            metrics.set_timing(&secs_metric("mute_shown"), started.elapsed().as_secs_f64());
        }
        metrics.set("mute_read", read.road.clone());
        let entries = read
            .entries()
            .map(<[MuteEntry]>::to_vec)
            .unwrap_or_default();
        metrics.set("mute_row", self.scene.rows_text(&entries, b));
        Ok(entries)
    }

    /// Leg 4: the list asked for with the CRC of the copy A holds, and with
    /// another.
    async fn cached_copy(
        &mut self,
        entries: &[MuteEntry],
        metrics: &mut Metrics,
    ) -> Result<(), TestFailure> {
        let crc = crc32(file_text(entries).as_bytes());
        let matching = read(self.a, crc, LISTEN).await?;
        metrics.set("cached_copy_current", matching.road);
        let other = read(self.a, crc ^ 0x5A5A_5A5A, LISTEN).await?;
        metrics.set("cached_copy_stale", other.road);
        Ok(())
    }

    /// Leg 5a: the flags changed by a second mute of B with `flags`; the rows
    /// of B after, then a delivery round under `round`.
    async fn flags_round(
        &mut self,
        flags: u32,
        round: &str,
        metrics: &mut Metrics,
    ) -> Result<(), TestFailure> {
        let b = self.scene.b;
        let name = self.scene.b_names.user.clone();
        self.mute(b, &name, MuteType::Agent, flags).await?;
        let (read, _shown) = read_until(self.a, VERIFY, |entries| {
            entries
                .iter()
                .any(|entry| entry.id == b && entry.flags.0 == flags)
        })
        .await?;
        let entries = read
            .entries()
            .map(<[MuteEntry]>::to_vec)
            .unwrap_or_default();
        metrics.set(&format!("{round}_rows"), self.scene.rows_text(&entries, b));
        self.delivery(round, metrics).await
    }

    /// Leg 5b: B muted again under another name, then unmuted under a name it
    /// was never muted under, then under each name.
    async fn renames(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let b = self.scene.b;
        let user = self.scene.b_names.user.clone();
        self.mute(b, OTHER_NAME, MuteType::Agent, 0).await?;
        let (read, _shown) = read_until(self.a, VERIFY, |entries| {
            entries
                .iter()
                .any(|entry| entry.id == b && entry.name == OTHER_NAME)
        })
        .await?;
        let mut rows = read
            .entries()
            .map(<[MuteEntry]>::to_vec)
            .unwrap_or_default();
        metrics.set("renamed_rows", self.scene.rows_text(&rows, b));

        for (name, key) in [
            (WRONG_NAME, "unmuted_under_a_name_never_muted"),
            (user.as_str(), "unmuted_under_the_user_name"),
            (OTHER_NAME, "unmuted_under_the_other_name"),
        ] {
            let before = rows.clone();
            self.unmute(b, name).await?;
            let (read, _changed) = read_until(self.a, VERIFY_SHORT, |entries| {
                rows_of(entries, b) != rows_of(&before, b)
            })
            .await?;
            rows = read
                .entries()
                .map(<[MuteEntry]>::to_vec)
                .unwrap_or_default();
            metrics.set(key, self.scene.rows_text(&rows, b));
        }
        Ok(())
    }

    /// Leg 6: every kind of entry, which are kept, the order the list names
    /// them in, and their removal.
    async fn kinds(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let stranger = marker(STRANGER_ID)?;
        let object = marker(OBJECT_ID)?;
        let group = marker(GROUP_ID)?;
        let external = marker(EXTERNAL_ID)?;
        let kinds = [
            ("by_name", Uuid::nil(), BY_NAME, MuteType::ByName),
            ("agent", stranger, STRANGER_NAME, MuteType::Agent),
            ("object", object, OBJECT_NAME, MuteType::Object),
            ("group", group, GROUP_NAME, MuteType::Group),
            ("external", external, EXTERNAL_NAME, MuteType::External),
        ];
        for (_word, id, name, mute_type) in kinds {
            self.mute(id, name, mute_type, 0).await?;
        }
        let is_kind = |entry: &MuteEntry, id: Uuid, name: &str| {
            if id.is_nil() {
                entry.id.is_nil() && entry.name == name
            } else {
                entry.id == id
            }
        };
        let (read, _all) = read_until(self.a, VERIFY, |entries| {
            kinds.iter().all(|(_word, id, name, _type)| {
                entries.iter().any(|entry| is_kind(entry, *id, name))
            })
        })
        .await?;
        let entries = read
            .entries()
            .map(<[MuteEntry]>::to_vec)
            .unwrap_or_default();
        for (word, id, name, _type) in kinds {
            let row = entries
                .iter()
                .find(|entry| is_kind(entry, id, name))
                .map_or_else(|| "not kept".to_owned(), |entry| self.scene.row_text(entry));
            metrics.set(&format!("kind_{word}"), row);
        }
        let order: Vec<&str> = entries
            .iter()
            .filter_map(|entry| {
                kinds
                    .iter()
                    .find(|(_word, id, name, _type)| is_kind(entry, *id, name))
                    .map(|(word, ..)| *word)
            })
            .collect();
        metrics.set("kinds_order", order.join(", "));

        for (_word, id, name, _type) in kinds {
            self.unmute(id, name).await?;
        }
        let (read, _gone) = read_until(self.a, VERIFY, |entries| {
            !entries.iter().any(|entry| {
                kinds
                    .iter()
                    .any(|(_word, id, name, _type)| is_kind(entry, *id, name))
            })
        })
        .await?;
        let left = read.entries().map_or(-1, |entries| {
            let left = entries
                .iter()
                .filter(|entry| {
                    kinds
                        .iter()
                        .any(|(_word, id, name, _type)| is_kind(entry, *id, name))
                })
                .count();
            i64::try_from(left).unwrap_or(i64::MAX)
        });
        metrics.set("kinds_left_after_removal", left);
        Ok(())
    }

    /// Leg 7: A mutes itself.
    async fn self_mute(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let a = self.scene.a;
        let name = self.scene.a_names.user.clone();
        self.mute(a, &name, MuteType::Agent, 0).await?;
        let (read, _kept) = read_until(self.a, VERIFY_SHORT, |entries| {
            entries.iter().any(|entry| entry.id == a)
        })
        .await?;
        let entries = read
            .entries()
            .map(<[MuteEntry]>::to_vec)
            .unwrap_or_default();
        metrics.set("self_mute", self.scene.rows_text(&entries, a));
        self.unmute(a, &name).await?;
        Ok(())
    }

    /// Leg 8: the list once every entry the census made is gone.
    async fn emptied(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let scene = self.scene.clone();
        let (read, _empty) = read_until(self.a, VERIFY, |entries| {
            !entries.iter().any(|entry| scene.is_ours(entry))
        })
        .await?;
        metrics.set("emptied_read", read.road);
        Ok(())
    }

    /// Removes every entry the census may have made, under every name it may
    /// have made it under, whatever became of the legs.
    async fn clean_up(&self) -> Result<(), TestFailure> {
        let b = self.scene.b;
        let a = self.scene.a;
        let mut removals: Vec<(Uuid, String)> = vec![
            (b, self.scene.b_names.user.clone()),
            (b, OTHER_NAME.to_owned()),
            (a, self.scene.a_names.user.clone()),
            (Uuid::nil(), BY_NAME.to_owned()),
        ];
        removals.extend(
            self.scene
                .markers
                .iter()
                .map(|(id, name, _word)| (*id, (*name).to_owned())),
        );
        for (id, name) in removals {
            self.unmute(id, &name).await?;
        }
        tokio::time::sleep(SETTLE).await;
        Ok(())
    }
}

/// The rows of `entries` about `id`.
fn rows_of(entries: &[MuteEntry], id: Uuid) -> Vec<MuteEntry> {
    entries
        .iter()
        .filter(|entry| entry.id == id)
        .cloned()
        .collect()
}

/// A marker id.
fn marker(literal: &str) -> Result<Uuid, TestFailure> {
    Uuid::parse_str(literal)
        .map_err(|error| TestFailure::Assertion(format!("bad marker id {literal}: {error}")))
}

/// A census of the mute list: its roads, what it keeps of each entry and
/// edit, its cached copy, and what a muted avatar can still send.
#[derive(Debug)]
pub struct MuteList;

impl GridTest for MuteList {
    fn name(&self) -> &'static str {
        "mute-list"
    }

    fn description(&self) -> &'static str {
        "How a grid serves the mute list and over which road, what it keeps of each kind of \
         entry and each edit, how it answers a cached copy, and what a muted avatar can still \
         send"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1200)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The census: both avatars in, the legs, and the clean up whatever became of
/// them.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "mute-list needs a secondary avatar".to_owned(),
        ));
    };
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let scene = Scene {
        a: a_id.uuid(),
        b: b_id.uuid(),
        a_names: Names::of(&a.avatar_legacy_name()),
        b_names: Names::of(&b.avatar_legacy_name()),
        markers: vec![
            (marker(STRANGER_ID)?, STRANGER_NAME, "the agent marker"),
            (marker(OBJECT_ID)?, OBJECT_NAME, "the object marker"),
            (marker(GROUP_ID)?, GROUP_NAME, "the group marker"),
            (marker(EXTERNAL_ID)?, EXTERNAL_NAME, "the external marker"),
        ],
    };
    let run_tag = format!("{:06x}", Uuid::new_v4().as_u128() & 0xFF_FFFF);
    a.send(Command::ProbeCircuits(CircuitProbe::Observe))
        .await?;
    let mut census = Census {
        a,
        b,
        scene,
        run_tag,
        said: 0,
    };
    let started = Instant::now();
    census.clean_up().await?;
    let _drained = watch(census.a, DRAIN).await?;
    let _drained = watch(census.b, SETTLE).await?;
    let outcome = legs(&mut census, metrics).await;
    let cleaned = census.clean_up().await;
    metrics.set_timing(&secs_metric("census"), started.elapsed().as_secs_f64());
    let probe_off = census
        .a
        .send(Command::ProbeCircuits(CircuitProbe::Off))
        .await;
    outcome.and(cleaned).and(probe_off)
}

/// Every leg, in order.
async fn legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    census.empty_list(metrics).await?;
    census.delivery("unmuted", metrics).await?;
    let entries = census.mute_b(metrics).await?;
    census.cached_copy(&entries, metrics).await?;
    census.delivery("muted", metrics).await?;
    census
        .flags_round(MuteFlags::ALLOW_TEXT_CHAT, "text_excepted", metrics)
        .await?;
    census
        .flags_round(
            MuteFlags::ALLOW_VOICE_CHAT
                | MuteFlags::ALLOW_PARTICLES
                | MuteFlags::ALLOW_OBJECT_SOUNDS,
            "all_but_text_excepted",
            metrics,
        )
        .await?;
    census.renames(metrics).await?;
    census.kinds(metrics).await?;
    census.self_mute(metrics).await?;
    census.emptied(metrics).await
}

#[cfg(test)]
mod tests {
    use super::{Names, crc32, file_text};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{MuteEntry, MuteFlags, MuteType, Uuid};

    #[test]
    fn the_crc_is_the_ieee_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn the_cached_copy_is_written_as_the_reference_writes_it() {
        let entries = [
            MuteEntry {
                id: Uuid::from_u128(1),
                name: "Some One".to_owned(),
                mute_type: MuteType::Agent,
                flags: MuteFlags(0),
            },
            MuteEntry {
                id: Uuid::nil(),
                name: "Spam".to_owned(),
                mute_type: MuteType::ByName,
                flags: MuteFlags(1),
            },
        ];
        assert_eq!(
            file_text(&entries),
            "1 00000000-0000-0000-0000-000000000001 Some One|0\n\
             0 00000000-0000-0000-0000-000000000000 Spam|1\n"
        );
    }

    #[test]
    fn names_by_rule() {
        let resident = Names::of("Some Resident");
        assert_eq!(resident.user, "Some");
        assert_eq!(resident.account, "some");
        assert_eq!(resident.rule_of("Some"), Some("user name"));
        assert_eq!(resident.rule_of("Some Resident"), Some("legacy name"));
        let legacy = Names::of("Avatar Tester");
        assert_eq!(legacy.user, "Avatar Tester");
        assert_eq!(legacy.account, "avatar.tester");
        assert_eq!(legacy.rule_of("avatar.tester"), Some("account name"));
        assert_eq!(
            legacy.rule_of("avatar tester"),
            Some("user name in another case")
        );
        assert_eq!(legacy.rule_of("Someone Else"), None);
    }
}
