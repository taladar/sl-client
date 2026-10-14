//! A census of friendship: what an offer, its answer and its end bring both
//! avatars, what a grid says of the rights one friend grants another, when it
//! says a friend came and went, what it does with a calling card, and what it
//! keeps of an offer for an avatar that is away.
//!
//! Avatar **A** offers, grants and ends; avatar **B** answers and goes away
//! four times. One question a leg:
//!
//! 1. **An end without a friendship**: A ends a friendship it does not have.
//! 2. **An offer and a decline**, then **an offer and an accept** into B's
//!    calling-card folder: what each avatar is sent, and what lands in
//!    either inventory.
//! 3. **An offer to a friend**.
//! 4. **Rights**: A grants B every right, none, and seeing it online again;
//!    B grants A seeing it online and on the map. What each is told.
//! 5. **The map right in use**: A asks where B is (`FindAgent`) without the
//!    right and with it, and tracks B (`TrackAgent`): whether the coarse
//!    feed's `Prey` then points at B.
//! 6. **A calling card**: A offers its card, B accepts; A offers again, B
//!    declines.
//! 7. **Away**, four times:
//!    - B logs out (A is told B went), A ends the friendship and offers a new
//!      one while B is away; B comes back, asks with
//!      `RetrieveInstantMessages` and accepts over UDP.
//!    - B vanishes without logging out (A is told B went, and when); B comes
//!      back (A is told B came, B is told A is there), with the rights each
//!      grants the other in its login's buddy list.
//!    - A ends the friendship, B logs out, A offers; B comes back, asks with
//!      `RetrieveInstantMessages` and accepts as the client does for an offer
//!      that was kept ([`Command::AcceptFriendship`] with `use_offline_cap`:
//!      the `AcceptFriendship` capability where the region grants it).
//!    - A ends the friendship, B logs out, A offers; B comes back, reads the
//!      `ReadOfflineMsgs` capability itself and declines by deleting through
//!      the `DeclineFriendship` capability.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/friends.md`; every run is held to it.
//!
//! `2av`, `[both]`. The fake grid keeps no friendships yet
//! (`server-fake-grid-friends-presence`). The friendship is ended and every
//! calling card the census made is removed, also when a leg fails.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, Command, Event, FriendKey, FriendRights, ImDialog, InstantMessage, InventoryFolder,
    InventoryFolderKey, InventoryItem, InventoryKey, TransactionId, Uuid,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::asset_http::without_ids;
use crate::cases::instant_messages::session_of;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{
    DRAIN, OPENSIM_START, cap_get, hold_to_measured, http_client, shape_of,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, secs_metric};

/// What every text of the census begins with.
const TEXT_TAG: &str = "fp-";

/// How long the acting avatar listens after a gesture.
const LISTEN: Duration = Duration::from_secs(6);

/// How long the acting avatar waits for what a leg came for, at most.
const ECHO_WINDOW: Duration = Duration::from_secs(20);

/// How long a grid is given to act on a change before the next leg.
const SETTLE: Duration = Duration::from_secs(3);

/// How long the coarse feed is watched after a `TrackAgent`.
const TRACK: Duration = Duration::from_secs(12);

/// How long A waits to be told that B logged out.
const LOGOUT_WINDOW: Duration = Duration::from_secs(45);

/// How long A waits to be told that B, which vanished, went.
const VANISH_WINDOW: Duration = Duration::from_secs(300);

/// How long a grid is given to drop an avatar that logged out, so that an
/// offer is kept for it rather than sent after a closing session.
const AWAY_SETTLE: Duration = Duration::from_secs(8);

/// How long an avatar that came back is left alone, to see what it is sent
/// without asking.
const UNASKED_WINDOW: Duration = Duration::from_secs(25);

/// How long an avatar waits for what it asked for.
const FETCH_WINDOW: Duration = Duration::from_secs(20);

/// How long after the first kept message a fetch is still listened to.
const FETCH_QUIET: Duration = Duration::from_secs(5);

/// How long the capability map may take to arrive after a login.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// How often a login refused after a vanishing is tried again.
const RELOGIN_TRIES: u8 = 4;

/// The pause before a refused login is tried again.
const RELOGIN_AGAIN: Duration = Duration::from_secs(30);

/// The capability that serves what was kept for an avatar.
const READ_OFFLINE_MSGS: &str = "ReadOfflineMsgs";

/// The capability that declines an offer that was kept.
const DECLINE_FRIENDSHIP: &str = "DeclineFriendship";

/// The capability that accepts an offer that was kept.
const ACCEPT_FRIENDSHIP: &str = "AcceptFriendship";

/// What a metric reads where the grid has no such capability.
const NO_CAPABILITY: &str = "no capability";

/// The calling-card folder's type code.
const CALLING_CARD_FOLDER: i8 = 2;

/// The Trash's type code.
const TRASH_FOLDER: i8 = 14;

/// Every right a friend may grant.
const ALL_RIGHTS: i32 =
    FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP | FriendRights::CAN_MODIFY_OBJECTS;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/friends.md (friends-presence, 2026-10-14)";

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
/// Not held: how many coarse updates the tracking leg saw, and the timings.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "accept_told_a",
        each(
            Answer::Text(
                "FriendshipAccepted from avatar B, B's name, to avatar A, id nobody, no region, not kept, text \"\", a bucket of one zero byte | online: avatar B",
            ),
            Answer::Text(
                "FriendshipAccepted from avatar B, B's name, to avatar A, id the session of the two avatars, a region, not kept, text \"<id>\", no bucket | item \"<B>\" of type 2, in A's calling-card folder, created by avatar B",
            ),
        ),
    ),
    (
        "accept_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "online: avatar A | item \"<A>\" of type 2, in B's calling-card folder, created by avatar A",
            ),
        ),
    ),
    (
        "answer_capabilities_granted",
        each(
            Answer::Text("ReadOfflineMsgs, AcceptFriendship, DeclineFriendship"),
            Answer::Text("none"),
        ),
    ),
    (
        "cap_decline_answer",
        each(
            Answer::Text("200, {success=true}"),
            Answer::Text("no capability"),
        ),
    ),
    ("cap_decline_told_a", both(Answer::Text("nothing"))),
    ("cap_decline_told_b", both(Answer::Text("nothing"))),
    (
        "cap_end_told_a",
        both(Answer::Text("friendship ended with avatar B")),
    ),
    (
        "cap_end_told_b",
        both(Answer::Text("friendship ended with avatar A")),
    ),
    (
        "cap_kept_after_the_decline",
        each(Answer::Text("0 messages"), Answer::Text("no capability")),
    ),
    (
        "cap_kept_offer",
        each(
            Answer::Text(
                "{asset_id=the session of the two avatars, binary_bucket=1 bytes, dialog=38, estate_id=1, from_agent_id=avatar A, from_agent_name=A's name, from_group=false, message=as sent, message_time=a date and time, offline=1, parent_estate_id=1, position=[3], region_id=another id, timestamp=a time, to_agent_id=avatar B, transaction-id=nobody}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_logout_of_a_stranger_told_a",
        both(Answer::Text("nothing")),
    ),
    ("cap_offer_while_away_told_a", both(Answer::Text("nothing"))),
    ("cap_return_buddy_list", both(Answer::Text("no entry"))),
    ("cap_return_refused_logins", both(Answer::Text("none"))),
    ("cap_return_told_a", both(Answer::Text("nothing"))),
    (
        "cap_return_unasked_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id avatar A, a region, not kept, text \"Will you be my friend?\", no bucket",
            ),
        ),
    ),
    (
        "card_again_answer_told_a",
        each(
            Answer::Text("calling card declined by avatar A, transaction the transaction as sent"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "card_again_answer_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("item \"<A>\" of type 2, in the Trash, created by avatar A"),
        ),
    ),
    ("card_again_offer_told_a", both(Answer::Text("nothing"))),
    (
        "card_again_offer_told_b",
        each(
            Answer::Text("calling card offered by avatar A, transaction the transaction as sent"),
            Answer::Text(
                "calling card offered by avatar A, transaction the new card's item id | item \"<A>\" of type 2, in B's calling-card folder, created by avatar A",
            ),
        ),
    ),
    (
        "card_answer_told_a",
        each(
            Answer::Text("calling card accepted by avatar A, transaction the transaction as sent"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "card_answer_told_b",
        each(
            Answer::Text("item \"<A>\" of type 2, in B's calling-card folder, created by avatar A"),
            Answer::Text("nothing"),
        ),
    ),
    ("card_offer_told_a", both(Answer::Text("nothing"))),
    (
        "card_offer_told_b",
        each(
            Answer::Text("calling card offered by avatar A, transaction the transaction as sent"),
            Answer::Text(
                "calling card offered by avatar A, transaction the new card's item id | item \"<A>\" of type 2, in B's calling-card folder, created by avatar A",
            ),
        ),
    ),
    (
        "client_accept_told_a",
        each(
            Answer::Text(
                "FriendshipAccepted from avatar B, B's name, to avatar A, id nobody, no region, not kept, text \"\", a bucket of one zero byte | online: avatar B",
            ),
            Answer::Text(
                "FriendshipAccepted from avatar B, B's name, to avatar A, id the session of the two avatars, a region, not kept, text \"<id>\", no bucket | item \"<B>\" of type 2, in A's calling-card folder, created by avatar B",
            ),
        ),
    ),
    (
        "client_accept_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "online: avatar A | item \"<A>\" of type 2, in B's calling-card folder, created by avatar A",
            ),
        ),
    ),
    (
        "client_end_told_a",
        both(Answer::Text("friendship ended with avatar B")),
    ),
    (
        "client_end_told_b",
        both(Answer::Text("friendship ended with avatar A")),
    ),
    (
        "client_kept_offer",
        each(
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id another id, no region, kept, text as sent, a bucket of one zero byte",
            ),
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id avatar A, a region, not kept, text \"Will you be my friend?\", no bucket",
            ),
        ),
    ),
    (
        "client_logout_of_a_stranger_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "client_offer_while_away_told_a",
        both(Answer::Text("nothing")),
    ),
    ("client_return_buddy_list", both(Answer::Text("no entry"))),
    ("client_return_refused_logins", both(Answer::Text("none"))),
    ("client_return_told_a", both(Answer::Text("nothing"))),
    (
        "client_return_unasked_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id avatar A, a region, not kept, text \"Will you be my friend?\", no bucket",
            ),
        ),
    ),
    (
        "decline_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "FriendshipDeclined from avatar B, B's name, to avatar A, id the session of the two avatars, a region, not kept, text \"<id>\", no bucket",
            ),
        ),
    ),
    ("decline_told_b", both(Answer::Text("nothing"))),
    (
        "end_while_away_told_a",
        both(Answer::Text("friendship ended with avatar B")),
    ),
    (
        "end_without_a_friendship_told_a",
        both(Answer::Text("friendship ended with avatar B")),
    ),
    (
        "end_without_a_friendship_told_b",
        both(Answer::Text("friendship ended with avatar A")),
    ),
    (
        "find_with_the_map_right_told_a",
        both(Answer::Text("avatar B found at 1 places")),
    ),
    (
        "find_without_the_map_right_told_a",
        both(Answer::Text("nothing")),
    ),
    ("friends_before_the_census", both(Answer::Flag(false))),
    (
        "grant_all_told_a",
        both(Answer::Text("rights we grant avatar B 7")),
    ),
    (
        "grant_all_told_b",
        each(
            Answer::Text("rights granted us by avatar A 6"),
            Answer::Text("rights granted us by avatar A 7"),
        ),
    ),
    (
        "grant_map_told_a",
        each(
            Answer::Text("rights granted us by avatar B 2"),
            Answer::Text("rights granted us by avatar B 3"),
        ),
    ),
    (
        "grant_map_told_b",
        both(Answer::Text("rights we grant avatar A 3")),
    ),
    (
        "grant_none_told_a",
        both(Answer::Text("rights we grant avatar B 0")),
    ),
    (
        "grant_none_told_b",
        both(Answer::Text(
            "offline: avatar A | rights granted us by avatar A 0",
        )),
    ),
    (
        "grant_online_told_a",
        both(Answer::Text("rights we grant avatar B 1")),
    ),
    (
        "grant_online_told_b",
        each(
            Answer::Text("online: avatar A"),
            Answer::Text("online: avatar A | rights granted us by avatar A 1"),
        ),
    ),
    ("logout_told_a", both(Answer::Text("offline: avatar B"))),
    (
        "offer_after_a_decline_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "offer_after_a_decline_told_b",
        each(
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id another id, no region, not kept, text as sent, a bucket of one zero byte",
            ),
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id avatar A, a region, not kept, text as sent, no bucket",
            ),
        ),
    ),
    (
        "offer_to_a_friend_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "alert \"/This person is already your friend. Please delete it first if you want to reestablish the friendship.\"",
            ),
        ),
    ),
    (
        "offer_to_a_friend_told_b",
        each(
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id another id, no region, not kept, text as sent, a bucket of one zero byte",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("offer_told_a", both(Answer::Text("nothing"))),
    (
        "offer_told_b",
        each(
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id another id, no region, not kept, text as sent, a bucket of one zero byte",
            ),
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id avatar A, a region, not kept, text as sent, no bucket",
            ),
        ),
    ),
    (
        "regrant_all_told_a",
        both(Answer::Text("rights we grant avatar B 7")),
    ),
    (
        "regrant_all_told_b",
        each(
            Answer::Text("rights granted us by avatar A 6"),
            Answer::Text("rights granted us by avatar A 7"),
        ),
    ),
    (
        "regrant_map_told_a",
        each(
            Answer::Text("rights granted us by avatar B 2"),
            Answer::Text("rights granted us by avatar B 3"),
        ),
    ),
    (
        "regrant_map_told_b",
        both(Answer::Text("rights we grant avatar A 3")),
    ),
    (
        "track_with_the_map_right_sets_prey",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "udp_accept_told_a",
        each(
            Answer::Text(
                "FriendshipAccepted from avatar B, B's name, to avatar A, id nobody, no region, not kept, text \"\", a bucket of one zero byte | online: avatar B",
            ),
            Answer::Text(
                "FriendshipAccepted from avatar B, B's name, to avatar A, id the session of the two avatars, a region, not kept, text \"<id>\", no bucket | item \"<B>\" of type 2, in A's calling-card folder, created by avatar B",
            ),
        ),
    ),
    (
        "udp_accept_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "online: avatar A | item \"<A>\" of type 2, in B's calling-card folder, created by avatar A",
            ),
        ),
    ),
    (
        "udp_kept_offer",
        each(
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id another id, no region, kept, text as sent, a bucket of one zero byte",
            ),
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id avatar A, a region, not kept, text \"Will you be my friend?\", no bucket",
            ),
        ),
    ),
    ("udp_offer_while_away_told_a", both(Answer::Text("nothing"))),
    ("udp_return_buddy_list", both(Answer::Text("no entry"))),
    ("udp_return_refused_logins", both(Answer::Text("none"))),
    ("udp_return_told_a", both(Answer::Text("nothing"))),
    (
        "udp_return_unasked_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "FriendshipOffered from avatar A, A's name, to avatar B, id avatar A, a region, not kept, text \"Will you be my friend?\", no bucket",
            ),
        ),
    ),
    (
        "vanish_return_buddy_list",
        both(Answer::Text("given 3, has 7")),
    ),
    ("vanish_return_refused_logins", both(Answer::Text("none"))),
    (
        "vanish_return_told_a",
        both(Answer::Text("online: avatar B")),
    ),
    (
        "vanish_return_unasked_b",
        both(Answer::Text("online: avatar A")),
    ),
    ("vanish_told_a", both(Answer::Text("offline: avatar B"))),
];

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every instant message, in arrival order.
    ims: Vec<InstantMessage>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// Every friend said to have come online.
    online: Vec<Uuid>,
    /// Every friend said to have gone offline.
    offline: Vec<Uuid>,
    /// Every change of rights: whose, the rights, and whether they are the
    /// ones the friend grants this avatar.
    rights: Vec<(Uuid, i32, bool)>,
    /// Every friend the grid said the friendship with ended.
    ended: Vec<Uuid>,
    /// Every calling card offered: by whom, and the transaction.
    cards_offered: Vec<(Uuid, Uuid)>,
    /// Every calling card of ours accepted: by whom, and the transaction.
    cards_accepted: Vec<(Uuid, Uuid)>,
    /// Every calling card of ours declined: by whom, and the transaction.
    cards_declined: Vec<(Uuid, Uuid)>,
    /// Every inventory item created or updated.
    items: Vec<InventoryItem>,
    /// How many inventory items were said to be removed.
    removed: usize,
    /// Every answer to a `FindAgent`: whom, and at how many places.
    found: Vec<(Uuid, usize)>,
    /// Whom the coarse feed's `Prey` pointed at, one entry an update.
    prey: Vec<Option<Uuid>>,
}

impl Got {
    /// Whether anything that was kept for the avatar is among the messages.
    fn any_kept(&self) -> bool {
        self.ims.iter().any(|im| im.offline)
    }

    /// Adds everything `other` holds.
    fn absorb(&mut self, mut other: Self) {
        self.ims.append(&mut other.ims);
        self.alerts.append(&mut other.alerts);
        self.online.append(&mut other.online);
        self.offline.append(&mut other.offline);
        self.rights.append(&mut other.rights);
        self.ended.append(&mut other.ended);
        self.cards_offered.append(&mut other.cards_offered);
        self.cards_accepted.append(&mut other.cards_accepted);
        self.cards_declined.append(&mut other.cards_declined);
        self.items.append(&mut other.items);
        self.removed = self.removed.saturating_add(other.removed);
        self.found.append(&mut other.found);
        self.prey.append(&mut other.prey);
    }
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(got: &mut Got, event: &Event) -> Option<()> {
    match event {
        Event::InstantMessageReceived(im) => got.ims.push((**im).clone()),
        Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
            got.alerts.push(message.clone());
        }
        Event::FriendsOnline(friends) => got.online.extend(friends.iter().map(|id| id.uuid())),
        Event::FriendsOffline(friends) => got.offline.extend(friends.iter().map(|id| id.uuid())),
        Event::FriendRightsChanged {
            friend_id,
            rights,
            granted_to_us,
        } => got
            .rights
            .push((friend_id.uuid(), rights.0, *granted_to_us)),
        Event::FriendshipTerminated { other } => got.ended.push(other.uuid()),
        Event::CallingCardOffered {
            offering_agent,
            transaction,
        } => got
            .cards_offered
            .push((offering_agent.uuid(), transaction.get())),
        Event::CallingCardAccepted { agent, transaction } => {
            got.cards_accepted.push((agent.uuid(), transaction.get()));
        }
        Event::CallingCardDeclined { agent, transaction } => {
            got.cards_declined.push((agent.uuid(), transaction.get()));
        }
        Event::InventoryItemCreated { item, .. } => got.items.push(item.clone()),
        Event::InventoryBulkUpdate { items, .. } => got.items.extend(items.iter().cloned()),
        Event::InventoryItemsRemoved { items } => {
            got.removed = got.removed.saturating_add(items.len());
        }
        Event::FindAgentReply {
            prey, locations, ..
        } => got.found.push((*prey, locations.len())),
        Event::CoarseLocationUpdate {
            locations, prey, ..
        } => got.prey.push(
            prey.and_then(|index| locations.get(index))
                .map(|location| location.agent_id.uuid()),
        ),
        _ => return None,
    }
    Some(())
}

/// Listens on `session` until `done` says the leg has what it came for and
/// `quiet` more has passed, or for `limit` at most.
async fn watch_for(
    session: &mut Session,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Got) -> bool,
) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    let mut done_at: Option<Instant> = None;
    loop {
        if done_at.is_none() && done(&got) {
            done_at = Some(Instant::now());
        }
        let remaining = done_at.map_or_else(
            || limit.saturating_sub(started.elapsed()),
            |at| quiet.saturating_sub(at.elapsed()),
        );
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

/// Listens on `session` for `window`.
async fn watch(session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    watch_for(session, window, window, |_got| false).await
}

/// Which of the two avatars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The one that offers, grants and ends.
    A,
    /// The one that answers and goes away.
    B,
}

/// What a leg brought each avatar.
struct Brought {
    /// What A was sent.
    a: Got,
    /// What B was sent.
    b: Got,
}

/// The ids and names a metric names by what they are rather than by their
/// value.
#[derive(Debug, Clone)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// The name A logged in under.
    a_name: String,
    /// The name B logged in under.
    b_name: String,
    /// A's calling-card folder.
    a_cards: Uuid,
    /// B's calling-card folder.
    b_cards: Uuid,
    /// A's and B's Trash, as each login named them.
    trash: Vec<Uuid>,
    /// Ids the census made or was given, by what a metric calls them.
    named: Vec<(Uuid, &'static str)>,
}

impl Scene {
    /// What `id` is the id of.
    fn id_text(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else if id == session_of(self.a, self.b) {
            "the session of the two avatars"
        } else if id == self.a_cards {
            "A's calling-card folder"
        } else if id == self.b_cards {
            "B's calling-card folder"
        } else if self.trash.contains(&id) {
            "the Trash"
        } else {
            self.named
                .iter()
                .rev()
                .find(|(named, _what)| *named == id)
                .map_or("another id", |(_named, what)| *what)
        }
    }

    /// Whose name `name` is.
    fn name_text(&self, name: &str) -> String {
        if name.is_empty() {
            "no name".to_owned()
        } else if name.eq_ignore_ascii_case(&self.a_name) {
            "A's name".to_owned()
        } else if name.eq_ignore_ascii_case(&self.b_name) {
            "B's name".to_owned()
        } else {
            format!("the name {:?}", self.scrubbed(name))
        }
    }

    /// `text` with nothing in it that is one run's alone: the avatars' names
    /// and every id.
    fn scrubbed(&self, text: &str) -> String {
        let mut out = without_ids(text);
        for (name, word) in [(&self.a_name, "<A>"), (&self.b_name, "<B>")] {
            if !name.is_empty() {
                out = out.replace(name.as_str(), word);
            }
        }
        if out.len() > 200 {
            let cut = (0..=200)
                .rev()
                .find(|at| out.is_char_boundary(*at))
                .unwrap_or(0);
            out.truncate(cut);
            out.push('…');
        }
        out
    }

    /// What a bucket holds.
    fn bucket_text(&self, bucket: &[u8]) -> String {
        if bucket.is_empty() {
            "no bucket".to_owned()
        } else if bucket == [0] {
            "a bucket of one zero byte".to_owned()
        } else {
            let text = String::from_utf8_lossy(bucket);
            format!(
                "a bucket of {} bytes reading {:?}",
                bucket.len(),
                self.scrubbed(text.trim_end_matches('\0'))
            )
        }
    }

    /// One message as a metric states it; `sent` is the text the leg sent,
    /// where the message is the leg's own.
    fn im_text(&self, im: &InstantMessage, sent: Option<&str>) -> String {
        let text = match sent {
            Some(sent) if im.message == sent => "as sent".to_owned(),
            _ => format!("{:?}", self.scrubbed(&im.message)),
        };
        format!(
            "{:?} from {}, {}, to {}, id {}, {}, {}, text {text}, {}",
            im.dialog,
            self.id_text(im.from_agent_id.uuid()),
            self.name_text(&im.from_agent_name),
            self.id_text(im.to_agent_id.uuid()),
            self.id_text(im.id),
            if im.region_id.is_some() {
                "a region"
            } else {
                "no region"
            },
            if im.offline { "kept" } else { "not kept" },
            self.bucket_text(&im.binary_bucket),
        )
    }

    /// One inventory item as a metric states it.
    fn item_text(&self, item: &InventoryItem) -> String {
        format!(
            "item {:?} of type {}, in {}, created by {}",
            self.scrubbed(&item.name),
            item.item_type,
            self.id_text(item.folder_id.uuid()),
            self.id_text(item.creator_id.uuid()),
        )
    }

    /// Everything `got` holds: each message as the grid worded it, every
    /// notice, every item; `sent` is the text of the leg's own message,
    /// where it sent one.
    fn everything(&self, got: &Got, sent: Option<&str>) -> String {
        let mut all: Vec<String> = got.ims.iter().map(|im| self.im_text(im, sent)).collect();
        all.extend(
            got.alerts
                .iter()
                .map(|alert| format!("alert {:?}", self.scrubbed(alert))),
        );
        all.extend(
            got.online
                .iter()
                .map(|id| format!("online: {}", self.id_text(*id))),
        );
        all.extend(
            got.offline
                .iter()
                .map(|id| format!("offline: {}", self.id_text(*id))),
        );
        all.extend(got.rights.iter().map(|(id, rights, to_us)| {
            format!(
                "rights {} {} {rights}",
                if *to_us { "granted us by" } else { "we grant" },
                self.id_text(*id),
            )
        }));
        all.extend(
            got.ended
                .iter()
                .map(|id| format!("friendship ended with {}", self.id_text(*id))),
        );
        for (word, list) in [
            ("offered by", &got.cards_offered),
            ("accepted by", &got.cards_accepted),
            ("declined by", &got.cards_declined),
        ] {
            all.extend(list.iter().map(|(id, transaction)| {
                format!(
                    "calling card {word} {}, transaction {}",
                    self.id_text(*id),
                    self.id_text(*transaction),
                )
            }));
        }
        all.extend(got.items.iter().map(|item| self.item_text(item)));
        if got.removed > 0 {
            all.push(format!("{} items removed", got.removed));
        }
        all.extend(
            got.found
                .iter()
                .map(|(id, places)| format!("{} found at {places} places", self.id_text(*id))),
        );
        if all.is_empty() {
            "nothing".to_owned()
        } else {
            all.join(" | ")
        }
    }

    /// One record of the `ReadOfflineMsgs` capability as a metric states it:
    /// every key, and what its value is.
    fn record_text(&self, record: &Llsd, sent: &str) -> String {
        let Llsd::Map(map) = record else {
            return shape_of(record);
        };
        let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
        entries.sort_unstable_by_key(|(key, _value)| *key);
        let entries: Vec<String> = entries
            .into_iter()
            .map(|(key, value)| {
                let text = match value {
                    Llsd::Uuid(id) => self.id_text(*id).to_owned(),
                    Llsd::String(text) => match (key.as_str(), Uuid::parse_str(text)) {
                        (_, Ok(id)) => format!("the text of {}", self.id_text(id)),
                        ("message", _) if text == sent => "as sent".to_owned(),
                        ("from_agent_name", _) => self.name_text(text),
                        ("message_time", _) => "a date and time".to_owned(),
                        _ => format!("{:?}", self.scrubbed(text)),
                    },
                    Llsd::Integer(_) if key == "timestamp" => "a time".to_owned(),
                    Llsd::Integer(number) => number.to_string(),
                    Llsd::Real(number) if *number == 0.0 => "0".to_owned(),
                    Llsd::Real(_) => "a number".to_owned(),
                    Llsd::Boolean(flag) => flag.to_string(),
                    Llsd::Binary(bytes) => format!("{} bytes", bytes.len()),
                    Llsd::Array(items) => format!("[{}]", items.len()),
                    other => shape_of(other),
                };
                format!("{key}={text}")
            })
            .collect();
        format!("{{{}}}", entries.join(", "))
    }
}

/// The messages a `ReadOfflineMsgs` answer holds.
pub(crate) fn kept_records(body: Option<&Llsd>) -> Vec<Llsd> {
    match body {
        Some(Llsd::Array(items)) => match items.first() {
            Some(Llsd::Array(inner)) => inner.clone(),
            Some(Llsd::Map(_)) => items.clone(),
            _ => Vec::new(),
        },
        Some(map @ Llsd::Map(_)) => match map.get("messages") {
            Some(Llsd::Array(items)) => items.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// A capability's answer as a metric states it: the status, and the body's
/// keys and values where it is LLSD, its first words where it is not.
pub(crate) fn answer_text(status: u16, text: &str) -> String {
    let body = if text.contains("<llsd") {
        match parse_llsd_xml(text) {
            Ok(Llsd::Map(map)) => {
                let mut entries: Vec<String> = map
                    .iter()
                    .map(|(key, value)| match value {
                        Llsd::Boolean(flag) => format!("{key}={flag}"),
                        Llsd::Integer(number) => format!("{key}={number}"),
                        Llsd::String(text) => format!("{key}={:?}", without_ids(text)),
                        other => format!("{key}={}", shape_of(other)),
                    })
                    .collect();
                entries.sort_unstable();
                format!("{{{}}}", entries.join(", "))
            }
            Ok(other) => shape_of(&other),
            Err(_) => "LLSD that does not parse".to_owned(),
        }
    } else if text.trim().is_empty() {
        "no body".to_owned()
    } else {
        let words: Vec<&str> = text.split_whitespace().take(6).collect();
        format!("{:?}", without_ids(&words.join(" ")))
    };
    format!("{status}, {body}")
}

/// The rights in B's login buddy list for A, as a metric states them.
fn buddy_text(session: &Session, other: Uuid) -> String {
    session
        .login_success()
        .and_then(|login| {
            login
                .buddy_list
                .iter()
                .find(|entry| entry.buddy_id == other)
        })
        .map_or_else(
            || "no entry".to_owned(),
            |entry| format!("given {}, has {}", entry.rights_granted, entry.rights_has),
        )
}

/// The folder of type `folder_type` in a login skeleton.
fn folder_of(folders: &[InventoryFolder], folder_type: i8) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == folder_type)
        .map(|folder| folder.folder_id)
}

/// Waits for `session`'s inventory skeleton and returns its calling-card
/// folder and its Trash.
async fn card_folder(
    session: &mut Session,
) -> Result<(InventoryFolderKey, InventoryFolderKey), TestFailure> {
    let folders = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _ => None,
        })
        .await?;
    folder_of(&folders, CALLING_CARD_FOLDER)
        .zip(folder_of(&folders, TRASH_FOLDER))
        .ok_or_else(|| {
            TestFailure::Assertion(
                "the inventory skeleton has no calling-card folder or no Trash".to_owned(),
            )
        })
}

/// How the avatar that comes back answers the offer that was kept for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answering {
    /// `RetrieveInstantMessages`, then `AcceptFriendship` over UDP.
    Udp,
    /// `RetrieveInstantMessages`, then the client's accept for a kept offer.
    Client,
    /// The `ReadOfflineMsgs` capability read by the case, then a delete
    /// through the `DeclineFriendship` capability.
    Capability,
}

impl Answering {
    /// The word the round's metrics begin with.
    const fn word(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Client => "client",
            Self::Capability => "cap",
        }
    }
}

/// Everything the legs share.
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// The ids and names the metrics name.
    scene: Scene,
    /// How many texts the census has made.
    said: u32,
    /// Whether B is logged in.
    b_is_here: bool,
    /// Every calling card the census saw made, and in whose inventory.
    cards: Vec<(Who, InventoryKey)>,
}

impl Census<'_> {
    /// A text of the census for `word`.
    fn text(&mut self, word: &str) -> String {
        self.said = self.said.saturating_add(1);
        format!("{TEXT_TAG}{}-{word}", self.said)
    }

    /// Notes every calling card in `brought`.
    fn note_cards(&mut self, brought: &Brought) {
        for (who, got) in [(Who::A, &brought.a), (Who::B, &brought.b)] {
            self.cards.extend(
                got.items
                    .iter()
                    .filter(|item| item.item_type == 2)
                    .map(|item| (who, item.item_id)),
            );
        }
    }

    /// Has `who` send `commands` and returns what the two avatars were then
    /// sent: the actor listens until `done` says it has what the leg came for
    /// (or [`LISTEN`]), the other one for a moment after. An avatar that is
    /// away is sent nothing.
    async fn act_until(
        &mut self,
        who: Who,
        commands: Vec<Command>,
        done: impl FnMut(&Got) -> bool,
    ) -> Result<Brought, TestFailure> {
        let (actor, listener) = match who {
            Who::A => (&mut *self.a, &mut *self.b),
            Who::B => (&mut *self.b, &mut *self.a),
        };
        for command in commands {
            actor.send(command).await?;
        }
        let acted = watch_for(actor, ECHO_WINDOW, LISTEN, done).await?;
        let listened = if who == Who::B || self.b_is_here {
            watch(listener, DRAIN).await?
        } else {
            Got::default()
        };
        let brought = match who {
            Who::A => Brought {
                a: acted,
                b: listened,
            },
            Who::B => Brought {
                a: listened,
                b: acted,
            },
        };
        self.note_cards(&brought);
        Ok(brought)
    }

    /// Has `who` send `commands` and returns what the two avatars were then
    /// sent, each listening for [`LISTEN`].
    async fn act(&mut self, who: Who, commands: Vec<Command>) -> Result<Brought, TestFailure> {
        let (actor, listener) = match who {
            Who::A => (&mut *self.a, &mut *self.b),
            Who::B => (&mut *self.b, &mut *self.a),
        };
        for command in commands {
            actor.send(command).await?;
        }
        let acted = watch(actor, LISTEN).await?;
        let listened = if who == Who::B || self.b_is_here {
            watch(listener, DRAIN).await?
        } else {
            Got::default()
        };
        let brought = match who {
            Who::A => Brought {
                a: acted,
                b: listened,
            },
            Who::B => Brought {
                a: listened,
                b: acted,
            },
        };
        self.note_cards(&brought);
        Ok(brought)
    }

    /// Records under `key` what each avatar was told: `{key}_told_a` and
    /// `{key}_told_b`.
    fn record(&self, metrics: &mut Metrics, key: &str, brought: &Brought, sent: Option<&str>) {
        metrics.set(
            &format!("{key}_told_a"),
            self.scene.everything(&brought.a, sent),
        );
        if self.b_is_here {
            metrics.set(
                &format!("{key}_told_b"),
                self.scene.everything(&brought.b, sent),
            );
        }
    }

    /// The offer A sends B, with `text`.
    fn offer(&self, text: &str) -> Command {
        Command::OfferFriendship {
            to_agent_id: AgentKey::from(self.scene.b),
            message: text.to_owned(),
        }
    }

    /// A ends its friendship with B.
    fn end(&self) -> Command {
        Command::TerminateFriendship(FriendKey::from(self.scene.b))
    }

    /// A grants B `rights`.
    fn grant(&self, rights: i32) -> Command {
        Command::GrantUserRights {
            target: FriendKey::from(self.scene.b),
            rights: FriendRights(rights),
        }
    }

    /// B's accept over UDP of the offer `offer`.
    fn accept(&self, offer: &InstantMessage, use_offline_cap: bool) -> Command {
        Command::AcceptFriendship {
            transaction_id: TransactionId::from(offer.id),
            friend_id: FriendKey::from(self.scene.a),
            calling_card_folder: InventoryFolderKey::from(self.scene.b_cards),
            use_offline_cap,
        }
    }

    /// The offer with `text` among what `got` holds — or, where none carries
    /// it, the last offer from A: OpenSim hands over an offer that was kept
    /// with a text of its own.
    fn the_offer(&self, got: &Got, text: &str) -> Option<InstantMessage> {
        let offers = || {
            got.ims.iter().filter(|im| {
                im.dialog == ImDialog::FriendshipOffered && im.from_agent_id.uuid() == self.scene.a
            })
        };
        offers()
            .find(|im| im.message == text)
            .or_else(|| offers().next_back())
            .cloned()
    }

    /// Ends any friendship the two have, then has A end the one it does not
    /// have.
    async fn end_without_a_friendship(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let was = self.a.login_success().is_some_and(|login| {
            login
                .buddy_list
                .iter()
                .any(|entry| entry.buddy_id == self.scene.b)
        });
        metrics.set("friends_before_the_census", was);
        if was {
            self.act(Who::A, vec![self.end()]).await?;
            tokio::time::sleep(SETTLE).await;
        }
        let brought = self.act(Who::A, vec![self.end()]).await?;
        self.record(metrics, "end_without_a_friendship", &brought, None);
        Ok(())
    }

    /// An offer B declines, an offer B accepts, and an offer to a friend.
    async fn offers(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let text = self.text("declined");
        let brought = self
            .act_until(Who::A, vec![self.offer(&text)], |_got| true)
            .await?;
        self.record(metrics, "offer", &brought, Some(&text));
        let offer = self
            .the_offer(&brought.b, &text)
            .ok_or_else(|| TestFailure::Assertion("B was never sent A's offer".to_owned()))?;
        self.scene.named.push((offer.id, "the offer's id"));
        let brought = self
            .act(
                Who::B,
                vec![Command::DeclineFriendship {
                    transaction_id: TransactionId::from(offer.id),
                    friend_id: FriendKey::from(self.scene.a),
                    use_offline_cap: false,
                }],
            )
            .await?;
        self.record(metrics, "decline", &brought, None);

        let text = self.text("accepted");
        let brought = self.act(Who::A, vec![self.offer(&text)]).await?;
        self.record(metrics, "offer_after_a_decline", &brought, Some(&text));
        let offer = self.the_offer(&brought.b, &text).ok_or_else(|| {
            TestFailure::Assertion("B was never sent A's second offer".to_owned())
        })?;
        self.scene.named.push((offer.id, "the offer's id"));
        let brought = self.act(Who::B, vec![self.accept(&offer, false)]).await?;
        self.record(metrics, "accept", &brought, None);
        tokio::time::sleep(SETTLE).await;

        let text = self.text("to-a-friend");
        let brought = self.act(Who::A, vec![self.offer(&text)]).await?;
        self.record(metrics, "offer_to_a_friend", &brought, Some(&text));
        if let Some(offer) = self.the_offer(&brought.b, &text) {
            // Answer it, so that nothing is left pending.
            self.b
                .send(Command::DeclineFriendship {
                    transaction_id: TransactionId::from(offer.id),
                    friend_id: FriendKey::from(self.scene.a),
                    use_offline_cap: false,
                })
                .await?;
            watch(self.b, SETTLE).await?;
        }
        Ok(())
    }

    /// A grants B every right, none, and seeing it online; B grants A seeing
    /// it online and on the map; and the map right in use.
    async fn rights(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        for (key, rights) in [
            ("grant_all", ALL_RIGHTS),
            ("grant_none", 0),
            ("grant_online", FriendRights::CAN_SEE_ONLINE),
        ] {
            // Second Life's echo of A's own change can take longer than a
            // moment: wait for it, so it is not taken for the next leg's.
            let b = self.scene.b;
            let brought = self
                .act_until(Who::A, vec![self.grant(rights)], |got| {
                    got.rights
                        .iter()
                        .any(|(id, granted, to_us)| *id == b && *granted == rights && !to_us)
                })
                .await?;
            self.record(metrics, key, &brought, None);
        }

        let find = Command::FindAgent {
            hunter: AgentKey::from(self.scene.a),
            prey: AgentKey::from(self.scene.b),
        };
        let brought = self.act(Who::A, vec![find.clone()]).await?;
        metrics.set(
            "find_without_the_map_right_told_a",
            self.scene.everything(&brought.a, None),
        );

        let brought = self
            .act(
                Who::B,
                vec![Command::GrantUserRights {
                    target: FriendKey::from(self.scene.a),
                    rights: FriendRights(
                        FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP,
                    ),
                }],
            )
            .await?;
        self.record(metrics, "grant_map", &brought, None);
        tokio::time::sleep(SETTLE).await;

        let brought = self.act(Who::A, vec![find]).await?;
        metrics.set(
            "find_with_the_map_right_told_a",
            self.scene.everything(&brought.a, None),
        );

        self.a
            .send(Command::TrackAgent {
                prey_id: AgentKey::from(self.scene.b),
            })
            .await?;
        let tracked = watch(self.a, TRACK).await?;
        let b = self.scene.b;
        metrics.set(
            "track_with_the_map_right_sets_prey",
            tracked.prey.contains(&Some(b)),
        );
        metrics.set(
            "track_coarse_updates_count",
            i64::try_from(tracked.prey.len()).unwrap_or(i64::MAX),
        );
        self.a
            .send(Command::TrackAgent {
                prey_id: AgentKey::from(Uuid::nil()),
            })
            .await?;
        watch(self.a, SETTLE).await?;
        Ok(())
    }

    /// A offers its calling card; B accepts; A offers again; B declines.
    async fn calling_cards(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        for (key, accept) in [("card", true), ("card_again", false)] {
            let transaction = Uuid::new_v4();
            self.scene
                .named
                .push((transaction, "the transaction as sent"));
            let brought = self
                .act(
                    Who::A,
                    vec![Command::OfferCallingCard {
                        to_agent_id: AgentKey::from(self.scene.b),
                        transaction_id: TransactionId::from(transaction),
                    }],
                )
                .await?;
            for item in brought.a.items.iter().chain(&brought.b.items) {
                self.scene
                    .named
                    .push((item.item_id.uuid(), "the new card's item id"));
            }
            self.record(metrics, &format!("{key}_offer"), &brought, None);
            let Some((_from, offered)) = brought.b.cards_offered.first().copied() else {
                metrics.set(&format!("{key}_answer_told_a"), "no offer to answer");
                metrics.set(&format!("{key}_answer_told_b"), "no offer to answer");
                continue;
            };
            let answer = if accept {
                Command::AcceptCallingCard {
                    transaction_id: TransactionId::from(offered),
                    calling_card_folder: InventoryFolderKey::from(self.scene.b_cards),
                }
            } else {
                Command::DeclineCallingCard(TransactionId::from(offered))
            };
            let brought = self.act(Who::B, vec![answer]).await?;
            self.record(metrics, &format!("{key}_answer"), &brought, None);
        }
        Ok(())
    }

    /// B logs out, or vanishes: what A is told, and after how long.
    async fn b_goes(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        vanish: bool,
        window: Duration,
    ) -> Result<(), TestFailure> {
        let left = Instant::now();
        if vanish {
            self.b.vanish().await?;
        } else {
            self.b.disconnect().await?;
        }
        self.b_is_here = false;
        let b = self.scene.b;
        let got = watch_for(self.a, window, Duration::from_secs(2), |got| {
            got.offline.contains(&b)
        })
        .await?;
        if got.offline.contains(&b) {
            metrics.set_timing(
                &secs_metric(&format!("{key}_offline_after")),
                left.elapsed().as_secs_f64(),
            );
        }
        metrics.set(&format!("{key}_told_a"), self.scene.everything(&got, None));
        Ok(())
    }

    /// B comes back: its login's buddy list, what each is told while B is
    /// left alone, and what B was told before anything was asked.
    async fn b_comes(&mut self, metrics: &mut Metrics, key: &str) -> Result<Got, TestFailure> {
        let mut refused = Vec::new();
        let mut tries: u8 = 0;
        loop {
            tries = tries.saturating_add(1);
            match self.b.relogin().await {
                Ok(()) => break,
                Err(error) if tries < RELOGIN_TRIES => {
                    refused.push(self.scene.scrubbed(&error.to_string()));
                    tokio::time::sleep(RELOGIN_AGAIN).await;
                }
                Err(error) => return Err(error),
            }
        }
        self.b_is_here = true;
        metrics.set(
            &format!("{key}_refused_logins"),
            if refused.is_empty() {
                "none".to_owned()
            } else {
                refused.join(" | ")
            },
        );
        metrics.set(
            &format!("{key}_buddy_list"),
            buddy_text(self.b, self.scene.a),
        );
        let (cards, trash) = card_folder(self.b).await?;
        self.scene.b_cards = cards.uuid();
        self.scene.trash.push(trash.uuid());
        // Nothing waits for the region here: that wait would throw away what
        // the arrival brings.
        let unasked = watch(self.b, UNASKED_WINDOW).await?;
        let told_a = watch(self.a, DRAIN).await?;
        metrics.set(
            &format!("{key}_told_a"),
            self.scene.everything(&told_a, None),
        );
        Ok(unasked)
    }

    /// The first absence: B logs out; A ends the friendship and offers
    /// again; B comes back, asks over UDP and answers as `answering` says.
    async fn away_round(
        &mut self,
        metrics: &mut Metrics,
        answering: Answering,
    ) -> Result<(), TestFailure> {
        let round = answering.word();
        if answering == Answering::Udp {
            // Still friends: A is told that B goes, then ends the friendship.
            self.b_goes(metrics, "logout", false, LOGOUT_WINDOW).await?;
            tokio::time::sleep(AWAY_SETTLE).await;
            let brought = self.act(Who::A, vec![self.end()]).await?;
            self.record(metrics, "end_while_away", &brought, None);
        } else {
            let brought = self.act(Who::A, vec![self.end()]).await?;
            self.record(metrics, &format!("{round}_end"), &brought, None);
            tokio::time::sleep(SETTLE).await;
            self.b_goes(
                metrics,
                &format!("{round}_logout_of_a_stranger"),
                false,
                LISTEN,
            )
            .await?;
            tokio::time::sleep(AWAY_SETTLE).await;
        }
        let text = self.text(&format!("away-{round}"));
        let brought = self.act(Who::A, vec![self.offer(&text)]).await?;
        self.record(
            metrics,
            &format!("{round}_offer_while_away"),
            &brought,
            Some(&text),
        );

        let unasked = self.b_comes(metrics, &format!("{round}_return")).await?;
        metrics.set(
            &format!("{round}_return_unasked_b"),
            self.scene.everything(&unasked, Some(&text)),
        );
        let waited = Instant::now();
        while self.b.cap(READ_OFFLINE_MSGS).is_none() && waited.elapsed() < CAP_WAIT {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if answering == Answering::Capability {
            return self.capability_round(metrics, &text).await;
        }
        self.b.send(Command::RetrieveInstantMessages).await?;
        let got = watch_for(self.b, FETCH_WINDOW, FETCH_QUIET, Got::any_kept).await?;
        let mut all = unasked;
        all.absorb(got);
        let offer = self.the_offer(&all, &text);
        metrics.set(
            &format!("{round}_kept_offer"),
            offer.as_ref().map_or_else(
                || "nothing".to_owned(),
                |offer| self.scene.im_text(offer, Some(&text)),
            ),
        );
        let Some(offer) = offer else {
            return Ok(());
        };
        self.scene.named.push((offer.id, "the offer's id"));
        let brought = self
            .act(
                Who::B,
                vec![self.accept(&offer, answering == Answering::Client)],
            )
            .await?;
        self.record(metrics, &format!("{round}_accept"), &brought, None);
        tokio::time::sleep(SETTLE).await;
        Ok(())
    }

    /// B reads the capability itself and declines through the capability.
    async fn capability_round(
        &mut self,
        metrics: &mut Metrics,
        text: &str,
    ) -> Result<(), TestFailure> {
        let http = http_client()?;
        let records = match self.b.cap(READ_OFFLINE_MSGS) {
            Some(url) => {
                let answer = cap_get(&http, &url).await?;
                kept_records(answer.body.as_ref())
            }
            None => Vec::new(),
        };
        let record = records
            .iter()
            .find(|record| record.get("message").and_then(Llsd::as_str) == Some(text));
        metrics.set(
            "cap_kept_offer",
            match (self.b.cap(READ_OFFLINE_MSGS), record) {
                (None, _) => NO_CAPABILITY.to_owned(),
                (Some(_), None) => "nothing".to_owned(),
                (Some(_), Some(record)) => self.scene.record_text(record, text),
            },
        );
        // Without the capability the offer is read over UDP, so that its
        // decline below has an id to name.
        let udp_offer = if self.b.cap(READ_OFFLINE_MSGS).is_none() {
            self.b.send(Command::RetrieveInstantMessages).await?;
            let got = watch_for(self.b, FETCH_WINDOW, FETCH_QUIET, Got::any_kept).await?;
            self.the_offer(&got, text)
        } else {
            None
        };
        let answer = match self.b.cap(DECLINE_FRIENDSHIP) {
            Some(url) => {
                let response = http
                    .delete(format!("{url}?from={}", self.scene.a))
                    .send()
                    .await
                    .map_err(|error| {
                        TestFailure::Assertion(format!(
                            "the DeclineFriendship request failed: {error}"
                        ))
                    })?;
                let status = response.status().as_u16();
                let body = response.text().await.unwrap_or_default();
                answer_text(status, &body)
            }
            None => {
                if let Some(offer) = &udp_offer {
                    self.b
                        .send(Command::DeclineFriendship {
                            transaction_id: TransactionId::from(offer.id),
                            friend_id: FriendKey::from(self.scene.a),
                            use_offline_cap: false,
                        })
                        .await?;
                }
                NO_CAPABILITY.to_owned()
            }
        };
        metrics.set("cap_decline_answer", answer);
        let brought = self.act(Who::B, Vec::new()).await?;
        self.record(metrics, "cap_decline", &brought, None);
        // What is kept for B now, by both roads.
        let again = match self.b.cap(READ_OFFLINE_MSGS) {
            Some(url) => {
                let answer = cap_get(&http, &url).await?;
                format!("{} messages", kept_records(answer.body.as_ref()).len())
            }
            None => NO_CAPABILITY.to_owned(),
        };
        metrics.set("cap_kept_after_the_decline", again);
        Ok(())
    }

    /// The vanishing: B goes without logging out and comes back, with every
    /// right granted both ways first so that the login's buddy list shows
    /// them.
    async fn vanish_round(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let brought = self.act(Who::A, vec![self.grant(ALL_RIGHTS)]).await?;
        self.record(metrics, "regrant_all", &brought, None);
        let brought = self
            .act(
                Who::B,
                vec![Command::GrantUserRights {
                    target: FriendKey::from(self.scene.a),
                    rights: FriendRights(
                        FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP,
                    ),
                }],
            )
            .await?;
        self.record(metrics, "regrant_map", &brought, None);
        tokio::time::sleep(SETTLE).await;
        self.b_goes(metrics, "vanish", true, VANISH_WINDOW).await?;
        let unasked = self.b_comes(metrics, "vanish_return").await?;
        metrics.set(
            "vanish_return_unasked_b",
            self.scene.everything(&unasked, None),
        );
        Ok(())
    }

    /// Ends the friendship and removes every calling card the census saw
    /// made.
    async fn clean_up(&mut self) -> Result<(), TestFailure> {
        if self.b_is_here || self.a.is_connected() {
            self.a.send(self.end()).await?;
            watch(self.a, SETTLE).await?;
        }
        for who in [Who::A, Who::B] {
            let items: Vec<InventoryKey> = self
                .cards
                .iter()
                .filter(|(owner, _item)| *owner == who)
                .map(|(_owner, item)| *item)
                .collect();
            if items.is_empty() {
                continue;
            }
            let session = match who {
                Who::A => &mut *self.a,
                Who::B => &mut *self.b,
            };
            if session.is_connected() {
                session.send(Command::RemoveInventoryItems(items)).await?;
                watch(session, SETTLE).await?;
            }
        }
        Ok(())
    }
}

/// A census of friendship: offers and their answers, rights, presence,
/// calling cards, and an offer kept for an avatar that is away.
#[derive(Debug)]
pub struct FriendsPresence;

impl GridTest for FriendsPresence {
    fn name(&self) -> &'static str {
        "friends-presence"
    }

    fn description(&self) -> &'static str {
        "What a friendship offer, its answer, its rights and its end bring both avatars, when a \
         friend is said to come and go, what becomes of a calling card, and what is kept of an \
         offer for an avatar that is away"
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
        Duration::from_secs(3000)
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

/// The census: both avatars in, the legs, B's four absences, and the clean
/// up whatever became of them.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "friends-presence needs a secondary avatar".to_owned(),
        ));
    };
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let (a_cards, a_trash) = card_folder(a).await?;
    let (b_cards, b_trash) = card_folder(b).await?;
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let scene = Scene {
        a: a_id.uuid(),
        b: b_id.uuid(),
        a_name: a.avatar_legacy_name(),
        b_name: b.avatar_legacy_name(),
        a_cards: a_cards.uuid(),
        b_cards: b_cards.uuid(),
        trash: vec![a_trash.uuid(), b_trash.uuid()],
        named: Vec::new(),
    };
    let mut census = Census {
        a,
        b,
        scene,
        said: 0,
        b_is_here: true,
        cards: Vec::new(),
    };
    let outcome = legs(&mut census, metrics).await;
    let cleaned = census.clean_up().await;
    outcome.and(cleaned)
}

/// Every leg, in order.
async fn legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    watch(census.b, DRAIN).await?;
    let granted: Vec<&str> = [READ_OFFLINE_MSGS, ACCEPT_FRIENDSHIP, DECLINE_FRIENDSHIP]
        .into_iter()
        .filter(|name| census.b.cap(name).is_some())
        .collect();
    metrics.set(
        "answer_capabilities_granted",
        if granted.is_empty() {
            "none".to_owned()
        } else {
            granted.join(", ")
        },
    );
    census.end_without_a_friendship(metrics).await?;
    census.offers(metrics).await?;
    census.rights(metrics).await?;
    census.calling_cards(metrics).await?;
    census.away_round(metrics, Answering::Udp).await?;
    census.vanish_round(metrics).await?;
    census.away_round(metrics, Answering::Client).await?;
    census.away_round(metrics, Answering::Capability).await
}

#[cfg(test)]
mod tests {
    use super::{answer_text, kept_records};
    use pretty_assertions::assert_eq;
    use sl_wire::Llsd;

    #[test]
    fn what_a_metric_says_of_a_capability_answer() {
        assert_eq!(
            answer_text(
                200,
                "<?xml version=\"1.0\" ?><llsd><map><key>success</key><boolean>1</boolean></map></llsd>"
            ),
            "200, {success=true}"
        );
        assert_eq!(answer_text(404, ""), "404, no body");
        assert_eq!(
            answer_text(500, "Internal Server Error, try again later please"),
            "500, \"Internal Server Error, try again later\""
        );
    }

    #[test]
    fn how_an_answer_holds_its_messages() {
        let message =
            Llsd::Map(std::iter::once(("dialog".to_owned(), Llsd::Integer(38))).collect());
        let nested = Llsd::Array(vec![Llsd::Array(vec![message.clone(), message.clone()])]);
        assert_eq!(kept_records(Some(&nested)).len(), 2);
        assert_eq!(kept_records(Some(&Llsd::Array(vec![message]))).len(), 1);
        assert!(kept_records(None).is_empty());
    }
}
