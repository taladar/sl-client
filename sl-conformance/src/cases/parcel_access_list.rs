//! Read a parcel's allow and ban lists, replace them, and record what the grid
//! answers at each step — the list half of
//! `gridspec-parcel-access-and-ban-lines`.
//!
//! A parcel keeps two per-avatar lists that gate entry: the *access* (allow)
//! list (`AL_ACCESS`) and the *ban* list (`AL_BAN`). A viewer reads either with
//! a UDP `ParcelAccessListRequest` ([`Command::RequestParcelAccessList`]),
//! answered by one or more `ParcelAccessListReply` packets
//! ([`Event::ParcelAccessList`]), and replaces a whole list with a
//! `ParcelAccessListUpdate` ([`Command::UpdateParcelAccessList`]), which a grid
//! does not answer at all.
//!
//! What the case does depends on whether the agent may edit the parcel at the
//! region centre.
//!
//! **Everyone** reads both lists and the record says whether each request was
//! answered, in how many packets, and what the entries carry.
//!
//! **An agent with land rights** (the estate owner on OpenSim, the primary
//! avatar on the fake grid, which enforces no rights):
//!
//! 1. replaces the ban list with a permanent and a timed entry and the allow
//!    list with one entry, then reads both back, with the parcel record, to see
//!    what the grid kept of each entry (its flags, its expiry) and whether
//!    saving a list switched the parcel's `USE_BAN_LIST` / `USE_ACCESS_LIST`
//!    flags on;
//! 2. replaces the ban list with `LONG_LIST` entries — more than one
//!    `ParcelAccessListUpdate` holds — and reads it back, counting the reply's
//!    packets;
//! 3. restores both lists and, where the grid changed them, the parcel's flags.
//!
//! **An agent without land rights** (any resident on aditi) sends the same
//! one-entry ban list and records what a refusal looks like: the alerts in the
//! next seconds, whether the parcel is pushed back, and whether the list read
//! afterwards holds the entry. A grid that took it means the avatar can edit
//! the land after all; the case restores the list and records `partial`.
//!
//! Every entry is a synthetic id: a list is a list of ids and a simulator does
//! not resolve them, so a real avatar would only add a fixture. The enforcement
//! of the lists — what a banned avatar meets at the parcel line — is
//! [`super::parcel_ban_enforcement`].
//!
//! `1av`, `[both]`, and offline against both fake flavours. A live OpenSim run
//! has to be the **estate-owner** avatar to take the owner's path.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sl_client_tokio::{
    Command, Event, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelInfo,
    RegionLocalParcelId, ScopedParcelId, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, check_eq, is_fake};

/// The western/southern edge of the queried square, in region metres — a 4×4 m
/// square at the region centre, the parcel the other parcel cases read.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the queried square, in region metres.
const SQUARE_EAST_NORTH: f32 = 128.0;

/// The first sequence id of this case's parcel queries; each query takes the
/// next. Distinct from every other case's ids so the replies never alias.
const SEQUENCE_BASE: i32 = 5152;

/// How long a list request waits for its first reply packet before the record
/// says the grid did not answer.
const LIST_REPLY_WINDOW: Duration = Duration::from_secs(10);

/// How long after a reply packet the case keeps collecting further packets of
/// the same list.
const LIST_PACKET_WINDOW: Duration = Duration::from_secs(2);

/// How long to watch for alerts and parcel pushes after an update.
const UPDATE_WATCH: Duration = Duration::from_secs(4);

/// How many entries the long list holds: more than the 48 the reference viewer
/// puts in one `ParcelAccessListUpdate` (`PARCEL_MAX_ENTRIES_PER_PACKET`), so
/// the update goes out in two sections and the reply cannot fit one datagram.
const LONG_LIST: usize = 60;

/// How far in the future the timed ban expires, in seconds.
const TIMED_BAN_SECS: i32 = 3600;

/// The base of the synthetic agent ids the case puts on the lists; entry `n`
/// is this plus `n`.
const SYNTHETIC_BASE: u128 = 0x0000_0000_0000_4000_8000_0000_acce_5500;

/// Whether a grid answers a list request from an agent with no rights over the
/// parcel — (allow list, ban list).
const ANSWERS_A_STRANGER: Measured<(bool, bool)> = Measured {
    second_life: (true, true),
    opensim: (true, true),
    source: "parcel-access-list on aditi and OpenSim (2026-10-05, book/src/gridspec/land.md)",
};

/// The flags a grid returns on an entry of the (allow list, ban list): OpenSim
/// stamps every entry with its list's own bit, whatever the update carried.
const ENTRY_FLAGS: Measured<(u32, u32)> = Measured {
    second_life: (0x1, 0x2),
    opensim: (0x1, 0x2),
    source: "parcel-access-list on OpenSim (2026-10-05, book/src/gridspec/land.md); \
             Second Life is not measured (no land) and follows the reference viewer",
};

/// Whether saving a non-empty list switches the parcel's own
/// (`USE_ACCESS_LIST`, `USE_BAN_LIST`) flag on. OpenSim does, and switches it
/// off again when the list is emptied; a Second Life parcel's flags are the
/// About Land checkboxes and nothing else.
const UPDATE_SETS_THE_FLAG: Measured<(bool, bool)> = Measured {
    second_life: (false, false),
    opensim: (true, true),
    source: "parcel-access-list on OpenSim (2026-10-05, book/src/gridspec/land.md); \
             Second Life is not measured (no land) and follows the reference viewer",
};

/// What a refused update is answered with: whether any alert arrived, and
/// whether the parcel was pushed back. Second Life says no in a plain
/// `AlertMessage` ("You do not have permission to update the ban list on your
/// group land." on a group's parcel); OpenSim drops the update silently.
const REFUSAL: Measured<(bool, bool)> = Measured {
    second_life: (true, false),
    opensim: (false, false),
    source: "parcel-access-list on aditi and OpenSim (2026-10-05, book/src/gridspec/land.md)",
};

/// Reads and replaces a parcel's allow and ban lists, recording each answer.
#[derive(Debug)]
pub struct ParcelAccessList;

impl GridTest for ParcelAccessList {
    fn name(&self) -> &'static str {
        "parcel-access-list"
    }

    fn description(&self) -> &'static str {
        "Read a parcel's allow and ban lists, replace them, and record each answer"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            let mut sequence = SEQUENCE_BASE;
            let parcel = read_parcel(session, next(&mut sequence)).await?;
            if !parcel.request_result.has_data() {
                ctx.mark_partial("the agent is standing on land the region has no parcel for");
                return Ok(());
            }
            let lists = Lists {
                scoped: ScopedParcelId::new(circuit, parcel.local_id),
                local_id: parcel.local_id,
            };
            // The fake grid enforces no land rights, so its avatar edits any
            // parcel.
            let may_edit = is_fake(grid) || parcel.owner.uuid() == agent.uuid();

            // What anyone may read.
            let allow = lists.read(session, ParcelAccessScope::Access).await?;
            let ban = lists.read(session, ParcelAccessScope::Ban).await?;
            let metrics = ctx.metrics();
            metrics.set("may_edit", may_edit);
            metrics.set("owner_id", parcel.owner.uuid().to_string());
            metrics.set("initial_allow", allow.describe());
            metrics.set("initial_ban", ban.describe());
            metrics.set("initial_flags", describe_flags(&parcel));

            if may_edit {
                check(
                    allow.answered && ban.answered,
                    "the grid did not answer the land owner's list requests",
                )?;
                let outcome = edit(ctx, &lists, &mut sequence).await;
                // Whatever the edit legs did, the lists and the flags go back.
                let restored = restore(ctx.primary(), &lists, &parcel, &allow, &ban).await;
                outcome.and(restored)
            } else {
                ANSWERS_A_STRANGER.check(
                    "list requests of an agent without land rights answered (allow, ban)",
                    grid,
                    &(allow.answered, ban.answered),
                )?;
                refused(ctx, &lists, &ban).await
            }
        })
    }
}

/// The owner's legs: a short list on each scope, then a long ban list.
///
/// # Errors
///
/// Returns a [`TestFailure`] for a send or wait failure, a list that does not
/// read back as it was written, or an answer that differs from the measured
/// one.
async fn edit(ctx: &mut TestContext, lists: &Lists, sequence: &mut i32) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let session = ctx.primary();
    let expiry = unix_now().saturating_add(TIMED_BAN_SECS);
    let permanent = entry(0, 0);
    let timed = entry(1, expiry);
    let allowed = entry(2, 0);

    // 1. A short list on each scope.
    lists
        .write(session, ParcelAccessScope::Ban, vec![permanent, timed])
        .await?;
    lists
        .write(session, ParcelAccessScope::Access, vec![allowed])
        .await?;
    let (alerts, pushed) = watch(session, lists.local_id).await?;
    let ban = lists.read(session, ParcelAccessScope::Ban).await?;
    let allow = lists.read(session, ParcelAccessScope::Access).await?;
    let after = read_parcel(session, next(sequence)).await?;
    let kept_permanent = ban.entry(permanent.id);
    let kept_timed = ban.entry(timed.id);
    let kept_allowed = allow.entry(allowed.id);

    // 2. A list longer than one update message holds.
    let long: Vec<ParcelAccessEntry> = (0..LONG_LIST)
        .map(|index| entry(u128::try_from(index).unwrap_or(0).saturating_add(100), 0))
        .collect();
    lists
        .write(session, ParcelAccessScope::Ban, long.clone())
        .await?;
    let long_read = lists.read(session, ParcelAccessScope::Ban).await?;

    let metrics = ctx.metrics();
    metrics.set("update_alerts", alerts.join(" | "));
    metrics.set("update_pushed_parcel", pushed);
    metrics.set("ban_after_update", ban.describe());
    metrics.set("allow_after_update", allow.describe());
    metrics.set("flags_after_update", describe_flags(&after));
    metrics.set(
        "timed_ban_expiry_offset",
        kept_timed.map_or_else(
            || "missing".to_owned(),
            |kept| kept.time.saturating_sub(expiry).to_string(),
        ),
    );
    metrics.set("long_list_read", long_read.describe());

    check_eq("ban list size after the update", &ban.entries.len(), &2)?;
    check_eq("allow list size after the update", &allow.entries.len(), &1)?;
    let (Some(kept_permanent), Some(kept_timed), Some(kept_allowed)) =
        (kept_permanent, kept_timed, kept_allowed)
    else {
        return Err(TestFailure::Assertion(
            "an entry the update wrote did not read back".to_owned(),
        ));
    };
    check_eq("the permanent ban's expiry", &kept_permanent.time, &0)?;
    check_eq("the timed ban's expiry", &kept_timed.time, &expiry)?;
    ENTRY_FLAGS.check(
        "entry flags read back (allow list, ban list)",
        grid,
        &(kept_allowed.flags.0, kept_permanent.flags.0),
    )?;
    UPDATE_SETS_THE_FLAG.check(
        "saving a list switched the parcel flag on (access list, ban list)",
        grid,
        &(after.use_access_list(), after.use_ban_list()),
    )?;
    check_eq(
        "long ban list size read back",
        &long_read.entries.len(),
        &LONG_LIST,
    )?;
    check(
        long.iter()
            .all(|written| long_read.entry(written.id).is_some()),
        "an entry of the long ban list did not read back",
    )
}

/// Puts both lists back as they were read, and the parcel's flags where the
/// grid changed them on the way.
///
/// # Errors
///
/// Returns a [`TestFailure`] for a send or wait failure, or a list that does
/// not read back at its original size.
async fn restore(
    session: &mut Session,
    lists: &Lists,
    original: &ParcelInfo,
    allow: &ListRead,
    ban: &ListRead,
) -> Result<(), TestFailure> {
    lists
        .write(session, ParcelAccessScope::Ban, ban.entries.clone())
        .await?;
    lists
        .write(session, ParcelAccessScope::Access, allow.entries.clone())
        .await?;
    let ban_now = lists.read(session, ParcelAccessScope::Ban).await?;
    let allow_now = lists.read(session, ParcelAccessScope::Access).await?;
    check_eq(
        "ban list size restored",
        &ban_now.entries.len(),
        &ban.entries.len(),
    )?;
    check_eq(
        "allow list size restored",
        &allow_now.entries.len(),
        &allow.entries.len(),
    )?;
    let now = read_parcel(session, SEQUENCE_BASE.saturating_add(90)).await?;
    if now.raw_parcel_flags != original.raw_parcel_flags {
        session
            .send(Command::UpdateParcel(Box::new(original.to_update())))
            .await?;
        let echoed = super::parcel_edit::await_echo(session, lists.local_id).await?;
        check_eq(
            "parcel flags restored",
            &echoed.raw_parcel_flags,
            &original.raw_parcel_flags,
        )?;
    }
    Ok(())
}

/// The stranger's leg: send a ban list the grid should refuse, and record what
/// comes back.
///
/// # Errors
///
/// Returns a [`TestFailure`] for a send or wait failure or an answer that
/// differs from the measured one.
async fn refused(
    ctx: &mut TestContext,
    lists: &Lists,
    ban_before: &ListRead,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let session = ctx.primary();
    let unwanted = entry(0, 0);
    let mut attempted = ban_before.entries.clone();
    attempted.push(unwanted);
    lists
        .write(session, ParcelAccessScope::Ban, attempted)
        .await?;
    let (alerts, pushed) = watch(session, lists.local_id).await?;
    let ban_after = lists.read(session, ParcelAccessScope::Ban).await?;
    let taken = ban_after.entry(unwanted.id).is_some();
    let metrics = ctx.metrics();
    metrics.set("refused_alerts", alerts.join(" | "));
    metrics.set("refused_pushed_parcel", pushed);
    metrics.set("ban_after_refusal", ban_after.describe());
    if taken {
        let session = ctx.primary();
        lists
            .write(session, ParcelAccessScope::Ban, ban_before.entries.clone())
            .await?;
        ctx.mark_partial("the grid took the list: the avatar can edit this land");
        return Ok(());
    }
    REFUSAL.check(
        "refused list update (an alert arrived, the parcel was pushed back)",
        grid,
        &(!alerts.is_empty(), pushed),
    )
}

/// The parcel whose lists the case reads and writes.
struct Lists {
    /// The parcel's id, scoped to the root circuit.
    scoped: ScopedParcelId,
    /// The parcel's region-local id, to match replies by.
    local_id: RegionLocalParcelId,
}

impl Lists {
    /// Requests the `scope` list and collects every packet of the answer.
    ///
    /// # Errors
    ///
    /// Propagates the send and wait failures other than the waits' own
    /// expected timeouts.
    async fn read(
        &self,
        session: &mut Session,
        scope: ParcelAccessScope,
    ) -> Result<ListRead, TestFailure> {
        session
            .send(Command::RequestParcelAccessList {
                local_id: self.scoped,
                scope,
            })
            .await?;
        let mut read = ListRead::default();
        let mut window = LIST_REPLY_WINDOW;
        loop {
            let packet = session
                .wait_for(window, |event| match event {
                    Event::ParcelAccessList {
                        local_id,
                        scope: reply_scope,
                        entries,
                    } if local_id.id() == self.local_id && *reply_scope == scope => {
                        Some(entries.clone())
                    }
                    _ => None,
                })
                .await;
            match packet {
                Ok(entries) => {
                    read.answered = true;
                    read.packets = read.packets.saturating_add(1);
                    for entry in entries {
                        if !read.entries.iter().any(|held| held.id == entry.id) {
                            read.entries.push(entry);
                        }
                    }
                    window = LIST_PACKET_WINDOW;
                }
                Err(TestFailure::Timeout(_)) => return Ok(read),
                Err(other) => return Err(other),
            }
        }
    }

    /// Replaces the `scope` list with `entries`.
    ///
    /// # Errors
    ///
    /// Propagates the send failure.
    async fn write(
        &self,
        session: &Session,
        scope: ParcelAccessScope,
        entries: Vec<ParcelAccessEntry>,
    ) -> Result<(), TestFailure> {
        session
            .send(Command::UpdateParcelAccessList {
                local_id: self.scoped,
                scope,
                entries,
            })
            .await
    }
}

/// One list as a grid answered it.
#[derive(Default)]
struct ListRead {
    /// Whether any reply packet arrived.
    answered: bool,
    /// How many reply packets arrived.
    packets: usize,
    /// The entries of every packet, by first arrival.
    entries: Vec<ParcelAccessEntry>,
}

impl ListRead {
    /// The entry for `id`, if the list holds one.
    fn entry(&self, id: Uuid) -> Option<ParcelAccessEntry> {
        self.entries.iter().find(|entry| entry.id == id).copied()
    }

    /// The read as the record shows it: `unanswered`, or the entry and packet
    /// counts with the distinct entry flags.
    fn describe(&self) -> String {
        if !self.answered {
            return "unanswered".to_owned();
        }
        let mut flags: Vec<u32> = self.entries.iter().map(|entry| entry.flags.0).collect();
        flags.sort_unstable();
        flags.dedup();
        format!(
            "{} entries in {} packets, entry flags {flags:?}",
            self.entries.len(),
            self.packets
        )
    }
}

/// Collects the text of every alert that arrives in [`UPDATE_WATCH`], and
/// whether the parcel `local_id` was pushed in it. The predicate never
/// matches, so the wait always ends in its own timeout.
///
/// # Errors
///
/// Propagates the wait's failures other than that timeout.
async fn watch(
    session: &mut Session,
    local_id: RegionLocalParcelId,
) -> Result<(Vec<String>, bool), TestFailure> {
    let mut alerts = Vec::new();
    let mut pushed = false;
    let outcome = session
        .wait_for(UPDATE_WATCH, |event| {
            match event {
                Event::ParcelProperties(parcel) if parcel.local_id == local_id => pushed = true,
                Event::AlertMessage {
                    message,
                    alert_info,
                    ..
                } => {
                    // A keyed-only alert has an empty plain message; its first
                    // structured id says what arrived.
                    let text = if message.trim().is_empty() {
                        alert_info
                            .first()
                            .map(|info| info.message.clone())
                            .unwrap_or_default()
                    } else {
                        message.clone()
                    };
                    alerts.push(text);
                }
                Event::AgentAlertMessage { message, .. } => alerts.push(message.clone()),
                _ => {}
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok((alerts, pushed)),
        Err(other) => Err(other),
    }
}

/// Reads the parcel at the region centre under `sequence_id`, waiting for the
/// reply that echoes it.
///
/// # Errors
///
/// Propagates the send and wait failures.
async fn read_parcel(session: &mut Session, sequence_id: i32) -> Result<ParcelInfo, TestFailure> {
    session
        .send(Command::RequestParcelProperties {
            west: SQUARE_WEST_SOUTH,
            south: SQUARE_WEST_SOUTH,
            east: SQUARE_EAST_NORTH,
            north: SQUARE_EAST_NORTH,
            sequence_id,
            snap_selection: false,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == sequence_id => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await
}

/// The synthetic list entry `index`, expiring at `time` (`0` for never).
const fn entry(index: u128, time: i32) -> ParcelAccessEntry {
    ParcelAccessEntry {
        id: Uuid::from_u128(SYNTHETIC_BASE.saturating_add(index)),
        time,
        flags: ParcelAccessFlags::NONE,
    }
}

/// The two list flags of a parcel, as the record shows them.
fn describe_flags(parcel: &ParcelInfo) -> String {
    format!(
        "use_access_list={} use_ban_list={}",
        parcel.use_access_list(),
        parcel.use_ban_list()
    )
}

/// The current Unix time in seconds, as a list entry's expiry counts it.
fn unix_now() -> i32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i32::try_from(elapsed.as_secs()).ok())
        .unwrap_or(i32::MAX)
}

/// Takes the next sequence id.
const fn next(sequence: &mut i32) -> i32 {
    let id = *sequence;
    *sequence = sequence.wrapping_add(1);
    id
}
