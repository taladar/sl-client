//! Friendship: offers and their answers, the rights one friend grants
//! another, presence, calling cards, and an offer kept for an avatar that is
//! away.
//!
//! The store is the grid's (a `Social` on its `GridCore`), because a
//! friendship is between two avatars wherever they stand, and each session runs
//! a responder (`run_social_responder`) that reads what its client asks and
//! tells whichever avatar the grid would tell. What each live grid tells whom
//! was measured by the `friends-presence` conformance case on aditi and the
//! local OpenSim (2026-10-14, `book/src/gridspec/friends.md`):
//!
//! | | Second Life | OpenSim |
//! | --- | --- | --- |
//! | an offer's id | minted per offer; no region; a bucket of one zero byte | the offerer's id; the offerer's region; no bucket |
//! | a decline | the offerer is told nothing | the offerer is told, from the session of the two |
//! | an accept | the offerer is told bare (no id, region or text) and that the accepter is online | the offerer is told from the session of the two; a calling card each, and the accepter is told the offerer is online |
//! | an offer to a friend | passed on | refused with an alert |
//! | a grantee's rights | without the see-online bit, and nothing for a change of that bit alone | as granted |
//! | `TrackAgent` with the map right | the coarse feed's `Prey` points at the friend | never set |
//! | a calling card offered | under the offerer's transaction; made when it is accepted | made at once, in the calling-card folder, under its item id; to the Trash when it is declined |
//! | an offer kept for an avatar that is away | kept, read back by asking | handed over at its login, as if live, with a text of the grid's |
//!
//! Both end a friendship, even one that is not there, by telling both avatars;
//! both answer `FindAgent` only for a friend who grants the map right; both say
//! a friend went when its last session closes, a logout or the circuit's
//! timeout alike.
//!
//! Not modelled: a friendship across grids, the edit right in use, and every
//! kept message but a friendship offer (`server-fake-grid-im-relay`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

use sl_proto::{
    FriendKey, FriendRights, ImDialog, InstantMessage, InventoryFolderKey, InventoryItem,
    InventoryKey, KeptMessage, ServerEvent, SimSession, TransactionId, UserRightsEntry,
};
use sl_types::key::{AgentKey, OwnerKey};
use sl_types::map::RegionCoordinates;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::driver::{SharedSim, SimState};
use crate::runtime::{GridCore, SessionRole};

/// The calling-card folder's type code (`LLFolderType::FT_CALLINGCARD`).
const CALLING_CARD_FOLDER: i8 = 2;

/// The Trash's type code (`LLFolderType::FT_TRASH`).
const TRASH_FOLDER: i8 = 14;

/// A calling card's asset and inventory type code
/// (`LLAssetType::AT_CALLINGCARD`, `LLInventoryType::IT_CALLINGCARD`).
const CALLING_CARD_TYPE: i8 = 2;

/// The estate every region of the fake grid belongs to, as a mainland region
/// and the local OpenSim's both state it.
const ESTATE: u32 = 1;

/// What OpenSim tells an avatar that offers a friend friendship: its
/// `FriendsModule` text behind the slash `LLClientView` puts in front of a
/// non-modal alert.
pub const OPENSIM_ALREADY_FRIENDS: &str = "/This person is already your friend. Please delete it first if you want to reestablish the friendship.";

/// The text OpenSim hands a kept friendship offer over with, whatever the
/// offerer wrote (`FriendsModule.OnClientLogin`).
pub const OPENSIM_KEPT_OFFER_TEXT: &str = "Will you be my friend?";

/// How a grid runs friendship
/// ([`ImitatedGrid::friends_policy`](crate::ImitatedGrid::friends_policy)).
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one measured divergence between the two grids"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FriendsPolicy {
    /// What an offer is passed on as.
    pub offers: OfferForm,
    /// Whether the offerer is told of a decline.
    pub decline_told: bool,
    /// Whether an offer to a friend is passed on as any offer is, rather than
    /// refused with [`OPENSIM_ALREADY_FRIENDS`].
    pub offer_to_a_friend_passed_on: bool,
    /// What an accept tells whom.
    pub acceptance: Acceptance,
    /// Whether the see-online bit is masked out of what a grantee is told.
    pub masks_see_online: bool,
    /// Whether `TrackAgent` points the coarse feed's `Prey` at a friend who
    /// grants the map right.
    pub tracks: bool,
    /// What a calling card offer does.
    pub cards: CardOffers,
    /// What becomes of an offer to an avatar that is away.
    pub kept_offers: KeptOffers,
}

impl Default for FriendsPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.friends_policy()
    }
}

/// What an offer is passed on as ([`FriendsPolicy::offers`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferForm {
    /// Under an id minted for the offer, from no region, with a bucket of one
    /// zero byte: Second Life.
    Minted,
    /// Under the offerer's own id, from the offerer's region, with no bucket:
    /// OpenSim.
    OfferersId,
}

/// What an accept tells whom ([`FriendsPolicy::acceptance`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acceptance {
    /// The offerer is told by a bare `FriendshipAccepted` — no id, no region,
    /// no text, a bucket of one zero byte — and that the accepter is online;
    /// the accepter is told nothing, and nobody is given a calling card:
    /// Second Life.
    Bare,
    /// The offerer is told by a `FriendshipAccepted` from the session of the
    /// two, from the accepter's region, its text the accepter's id; each is
    /// given the other's calling card, and the accepter is told the offerer is
    /// online: OpenSim.
    Stated,
}

/// What a calling card offer does ([`FriendsPolicy::cards`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardOffers {
    /// Passed on under the offerer's transaction, the card made when it is
    /// accepted, and the offerer told of either answer: Second Life.
    AsSent,
    /// The card made at once in the calling-card folder and offered under its
    /// item id; an accept tells nobody, a decline moves it to the Trash:
    /// OpenSim.
    MadeAtOnce,
}

/// What becomes of an offer to an avatar that is away
/// ([`FriendsPolicy::kept_offers`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptOffers {
    /// Kept, and read back only by asking — over `RetrieveInstantMessages`
    /// as it would have arrived, or over `ReadOfflineMsgs` as a record:
    /// Second Life.
    Kept,
    /// Handed over at the avatar's login, as if live, with
    /// [`OPENSIM_KEPT_OFFER_TEXT`]: OpenSim.
    HandedOverAtLogin,
}

/// A friendship offer not yet answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Offer {
    /// The avatar that offered.
    from: AgentKey,
    /// The avatar offered to.
    to: AgentKey,
}

/// A calling card offered and not yet answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CardOffer {
    /// The avatar whose card it is.
    from: AgentKey,
    /// The avatar it was offered to.
    to: AgentKey,
}

/// The grid's friendships, and what is pending between its avatars.
#[derive(Debug, Default)]
pub(crate) struct Social {
    /// The rights `granter` grants `grantee`, by `(granter, grantee)`: two
    /// entries a friendship, one each way.
    rights: BTreeMap<(AgentKey, AgentKey), i32>,
    /// The friendship offers not yet answered, by the id they were passed on
    /// under.
    offers: HashMap<Uuid, Offer>,
    /// The calling cards offered and not yet answered, by the id they were
    /// offered under.
    cards: HashMap<Uuid, CardOffer>,
    /// What is kept for each avatar that was away when it was sent, in the
    /// order sent.
    kept: HashMap<AgentKey, Vec<KeptMessage>>,
    /// The avatars logged in.
    online: BTreeSet<AgentKey>,
}

impl Social {
    /// A store holding `friendships`, each granting the other seeing it online
    /// and on the map.
    pub(crate) fn with_friendships(friendships: &[(AgentKey, AgentKey)]) -> Self {
        let mut social = Self::default();
        for &(one, other) in friendships {
            let rights = FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_SEE_ON_MAP;
            social.rights.insert((one, other), rights);
            social.rights.insert((other, one), rights);
        }
        social
    }

    /// Keeps `message` for `agent`, which is away, until it comes back.
    pub(crate) fn keep(&mut self, agent: AgentKey, message: KeptMessage) {
        self.kept.entry(agent).or_default().push(message);
    }

    /// The rights `granter` grants `grantee`, where the two are friends.
    fn rights(&self, granter: AgentKey, grantee: AgentKey) -> Option<i32> {
        self.rights.get(&(granter, grantee)).copied()
    }

    /// Whether `granter` lets `grantee` see `right`.
    fn grants(&self, granter: AgentKey, grantee: AgentKey, right: i32) -> bool {
        self.rights(granter, grantee)
            .is_some_and(|rights| rights & right != 0)
    }

    /// Every friend of `agent`.
    fn friends_of(&self, agent: AgentKey) -> Vec<AgentKey> {
        self.rights
            .keys()
            .filter(|(granter, _grantee)| *granter == agent)
            .map(|(_granter, grantee)| *grantee)
            .collect()
    }

    /// The buddy list of `agent`'s login response.
    pub(crate) fn buddies_of(&self, agent: AgentKey) -> Vec<sl_wire::BuddyListEntry> {
        self.friends_of(agent)
            .into_iter()
            .map(|buddy| sl_wire::BuddyListEntry {
                buddy_id: buddy.uuid(),
                rights_granted: self.rights(agent, buddy).unwrap_or(0),
                rights_has: self.rights(buddy, agent).unwrap_or(0),
            })
            .collect()
    }
}

/// The session of two avatars: the exclusive-or of their ids, which both live
/// grids state as the id of what passes between them.
fn session_of(one: AgentKey, other: AgentKey) -> Uuid {
    let mut session = [0_u8; 16];
    for ((byte, left), right) in session
        .iter_mut()
        .zip(one.uuid().as_bytes())
        .zip(other.uuid().as_bytes())
    {
        *byte = left ^ right;
    }
    Uuid::from_bytes(session)
}

/// The wall-clock time as UNIX seconds, as an instant message states it.
pub(crate) fn now_epoch_seconds() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u32::try_from(elapsed.as_secs()).unwrap_or(0))
}

/// Where an avatar is, as what it sends states it.
#[derive(Debug, Clone)]
pub(crate) struct Whereabouts {
    /// The avatar.
    pub(crate) agent: AgentKey,
    /// Its legacy name.
    pub(crate) name: String,
    /// Its region's id.
    pub(crate) region_id: Uuid,
    /// Where it stands in that region.
    pub(crate) position: RegionCoordinates,
    /// Its region's handle.
    pub(crate) handle: sl_wire::RegionHandle,
}

impl Whereabouts {
    /// Where the avatar of `state` is.
    pub(crate) fn of(state: &SimState) -> Self {
        Self {
            agent: state.avatar.agent_id,
            name: format!("{} {}", state.avatar.first_name, state.avatar.last_name),
            region_id: state.identity.region_id,
            position: state.sim.arrival_position().position,
            handle: state.sim.region_handle(),
        }
    }
}

/// A message from `from` to `to` of `dialog`, from nowhere, kept for nobody,
/// with no time; the caller states the rest.
pub(crate) fn message_from(
    from: &Whereabouts,
    to: AgentKey,
    dialog: ImDialog,
    text: &str,
) -> InstantMessage {
    InstantMessage {
        from_agent_id: from.agent,
        from_agent_name: from.name.clone(),
        to_agent_id: to,
        dialog,
        from_group: false,
        region_id: None,
        position: RegionCoordinates::new(0.0, 0.0, 0.0),
        offline: false,
        timestamp: None,
        id: Uuid::nil(),
        parent_estate_id: ESTATE,
        message: text.to_owned(),
        binary_bucket: Vec::new(),
    }
}

/// Logs a failure to tell a client something; the grid goes on either way.
pub(crate) fn warn_on<E: std::fmt::Display>(result: Result<(), E>, what: &str) {
    if let Err(error) = result {
        tracing::warn!("telling a client of {what} failed: {error}");
    }
}

/// Runs `tell` against `agent`'s root session, where it is logged in; answers
/// whether it was.
pub(crate) async fn tell(
    core: &GridCore,
    agent: AgentKey,
    tell: impl FnOnce(&mut SimState, Instant),
) -> bool {
    let Some(shared) = core.root_session_of(agent).await else {
        return false;
    };
    let now = shared.now();
    shared.with_state(|state| tell(state, now)).await;
    true
}

/// Where `agent` is, where it is logged in.
pub(crate) async fn whereabouts_of(core: &GridCore, agent: AgentKey) -> Option<Whereabouts> {
    let shared = core.root_session_of(agent).await?;
    let state = shared.state.lock().await;
    Some(Whereabouts::of(&state))
}

/// The legacy name of the account `agent`, or an empty one for no account.
pub(crate) fn legacy_name(core: &GridCore, agent: AgentKey) -> String {
    core.account_by_agent(agent)
        .map_or_else(String::new, |account| {
            format!("{} {}", account.config.first_name, account.config.last_name)
        })
}

/// The folder of type `folder_type` in the agent's inventory.
pub(crate) fn folder_of_type(sim: &SimSession, folder_type: i8) -> Option<InventoryFolderKey> {
    sim.agent_inventory()
        .folders()
        .find(|folder| folder.folder_type == folder_type)
        .map(|folder| folder.folder_id)
}

/// A calling card for `of`, called `name`, in `owner`'s `folder`.
fn calling_card(
    item_id: Uuid,
    folder: InventoryFolderKey,
    owner: AgentKey,
    of: AgentKey,
    name: &str,
) -> InventoryItem {
    InventoryItem {
        item_id: InventoryKey::from(item_id),
        folder_id: folder,
        name: name.to_owned(),
        description: String::new(),
        asset_id: Uuid::nil(),
        item_type: CALLING_CARD_TYPE,
        inv_type: CALLING_CARD_TYPE,
        flags: 0,
        sale_type: 0,
        sale_price: None,
        creation_date: i32::try_from(now_epoch_seconds()).unwrap_or(i32::MAX),
        owner: OwnerKey::Agent(owner),
        last_owner_id: Uuid::nil(),
        creator_id: of,
        group: None,
        permissions: sl_wire::Permissions5 {
            base: sl_wire::Permissions::ALL,
            owner: sl_wire::Permissions::ALL,
            group: sl_wire::Permissions::NONE,
            everyone: sl_wire::Permissions::NONE,
            next_owner: sl_wire::Permissions::ALL,
        },
    }
}

/// Files `card` in its folder of `state`'s agent — the one named, or the
/// calling-card folder where none is — and tells the client.
fn give_card(state: &mut SimState, mut card: InventoryItem, now: Instant) {
    if state.sim.agent_inventory().folder(card.folder_id).is_none()
        && let Some(folder) = folder_of_type(&state.sim, CALLING_CARD_FOLDER)
    {
        card.folder_id = folder;
    }
    state.sim.agent_inventory_mut().insert_item(card.clone());
    crate::world::announce_created_items(
        state.inventory_announcement,
        &mut state.sim,
        &[card],
        TransactionId::from(Uuid::nil()),
        now,
    );
}

/// The per-session responder for friendship: answers what its client asks of
/// the grid's friendships and tells the other avatars, until the session
/// closes or the grid shuts down. A session that closes leaving its avatar no
/// other root session takes the avatar offline.
pub(crate) async fn run_social_responder(core: Arc<GridCore>, shared: SharedSim) {
    let agent = shared.state.lock().await.avatar.agent_id;
    let mut events = shared.subscribe_events();
    let mut closed_rx = shared.closed_tx.subscribe();
    let mut shutdown_rx = shared.shutdown_rx.clone();
    loop {
        if *closed_rx.borrow_and_update() || *shutdown_rx.borrow_and_update() {
            break;
        }
        let received = tokio::select! {
            received = events.recv() => received,
            changed = closed_rx.changed() => {
                if changed.is_err() || *closed_rx.borrow() {
                    break;
                }
                continue;
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return;
                }
                continue;
            }
        };
        match received {
            Ok(event) => answer(&core, &shared, agent, event).await,
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!("friendship responder missed {missed} events");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    if *shutdown_rx.borrow() {
        return;
    }
    went_away(&core, agent).await;
}

/// Answers one thing the client did.
async fn answer(core: &Arc<GridCore>, shared: &SharedSim, agent: AgentKey, event: ServerEvent) {
    match event {
        ServerEvent::AgentArrived => {
            let root = matches!(shared.state.lock().await.role, SessionRole::Root);
            if root {
                arrived(core, shared, agent).await;
            }
        }
        ServerEvent::InstantMessage(im) if im.dialog == ImDialog::FriendshipOffered => {
            offer(core, shared, &im).await;
        }
        ServerEvent::FriendshipAccepted {
            transaction,
            calling_card_folders,
        } => {
            let offer = take_offer(core, transaction.get(), agent);
            if let Some(offer) = offer {
                befriend(core, offer, calling_card_folders.first().copied()).await;
            }
        }
        ServerEvent::FriendshipDeclined { transaction } => {
            let offer = take_offer(core, transaction.get(), agent);
            if let Some(offer) = offer {
                declined(core, shared, offer).await;
            }
        }
        ServerEvent::KeptFriendshipAnswered { from, accept } => {
            kept_answered(core, shared, from, agent, accept).await;
        }
        ServerEvent::FriendshipTerminationRequested { other } => {
            end(core, agent, AgentKey::from(other.uuid())).await;
        }
        ServerEvent::UserRightsGranted { rights } => {
            for entry in rights {
                grant(core, agent, entry).await;
            }
        }
        ServerEvent::FindAgent { hunter, prey } => {
            find(core, agent, hunter, AgentKey::from(prey)).await;
        }
        ServerEvent::TrackAgent { prey_id } => track(core, shared, agent, prey_id).await,
        ServerEvent::CallingCardOffered { dest, transaction } => {
            offer_card(core, shared, dest, transaction).await;
        }
        ServerEvent::CallingCardAccepted {
            transaction,
            folder,
        } => card_answered(core, agent, transaction, Some(folder)).await,
        ServerEvent::CallingCardDeclined { transaction } => {
            card_answered(core, agent, transaction, None).await;
        }
        ServerEvent::ClientMessage(message)
            if matches!(*message, sl_proto::AnyMessage::RetrieveInstantMessages(_)) =>
        {
            retrieve(shared).await;
        }
        _ => {}
    }
}

/// An avatar's root session arrived: an avatar that was not online is now,
/// its friends who may see it are told, it is told which of its friends it may
/// see are, and what was kept for it is kept for it here — or handed over.
async fn arrived(core: &Arc<GridCore>, shared: &SharedSim, agent: AgentKey) {
    let (tell_friends, its_friends, kept) = {
        let mut social = core.social.lock();
        if !social.online.insert(agent) {
            return;
        }
        let online: Vec<AgentKey> = social
            .friends_of(agent)
            .into_iter()
            .filter(|friend| social.online.contains(friend))
            .collect();
        let tell_friends: Vec<AgentKey> = online
            .iter()
            .copied()
            .filter(|friend| social.grants(agent, *friend, FriendRights::CAN_SEE_ONLINE))
            .collect();
        let its_friends: Vec<FriendKey> = online
            .iter()
            .copied()
            .filter(|friend| social.grants(*friend, agent, FriendRights::CAN_SEE_ONLINE))
            .map(|friend| FriendKey::from(friend.uuid()))
            .collect();
        let kept = social.kept.remove(&agent).unwrap_or_default();
        drop(social);
        (tell_friends, its_friends, kept)
    };
    let policy = core.friends_policy;
    shared
        .with_state(|state| {
            let now = shared.now();
            if !its_friends.is_empty() {
                warn_on(
                    state.sim.send_online_notification(&its_friends, now),
                    "friends online",
                );
            }
            let here = Whereabouts::of(state);
            for kept in kept {
                let offer = kept.message.dialog == ImDialog::FriendshipOffered;
                if offer && policy.kept_offers == KeptOffers::HandedOverAtLogin {
                    let handed = InstantMessage {
                        region_id: Some(here.region_id),
                        offline: false,
                        message: OPENSIM_KEPT_OFFER_TEXT.to_owned(),
                        ..kept.message
                    };
                    warn_on(state.sim.send_instant_message(&handed, now), "a kept offer");
                } else {
                    state.sim.store_offline_message(kept);
                }
            }
        })
        .await;
    let came = [FriendKey::from(agent.uuid())];
    for friend in tell_friends {
        tell(core, friend, |state, now| {
            warn_on(
                state.sim.send_online_notification(&came, now),
                "a friend online",
            );
        })
        .await;
    }
}

/// An avatar's session closed: an avatar left with no root session is
/// offline, and its friends who may see it are told.
async fn went_away(core: &Arc<GridCore>, agent: AgentKey) {
    if core.root_session_of(agent).await.is_some() {
        return;
    }
    let tell_friends: Vec<AgentKey> = {
        let mut social = core.social.lock();
        if !social.online.remove(&agent) {
            return;
        }
        social
            .friends_of(agent)
            .into_iter()
            .filter(|friend| {
                social.online.contains(friend)
                    && social.grants(agent, *friend, FriendRights::CAN_SEE_ONLINE)
            })
            .collect()
    };
    let went = [FriendKey::from(agent.uuid())];
    for friend in tell_friends {
        tell(core, friend, |state, now| {
            warn_on(
                state.sim.send_offline_notification(&went, now),
                "a friend offline",
            );
        })
        .await;
    }
}

/// Takes the offer passed on under `id` to `to`, if there is one.
fn take_offer(core: &GridCore, id: Uuid, to: AgentKey) -> Option<Offer> {
    let mut social = core.social.lock();
    let offer = social
        .offers
        .get(&id)
        .copied()
        .filter(|offer| offer.to == to)?;
    social.offers.remove(&id);
    drop(social);
    Some(offer)
}

/// An avatar offered another friendship.
async fn offer(core: &Arc<GridCore>, shared: &SharedSim, im: &InstantMessage) {
    let policy = core.friends_policy;
    let from = {
        let state = shared.state.lock().await;
        Whereabouts::of(&state)
    };
    let to = im.to_agent_id;
    let friends = core.social.lock().rights(from.agent, to).is_some();
    if friends && !policy.offer_to_a_friend_passed_on {
        shared
            .with_sim(|sim| {
                warn_on(
                    sim.send_agent_alert_message(
                        from.agent,
                        false,
                        OPENSIM_ALREADY_FRIENDS,
                        shared.now(),
                    ),
                    "an offer to a friend",
                );
            })
            .await;
        return;
    }
    let mut passed = message_from(&from, to, ImDialog::FriendshipOffered, &im.message);
    match policy.offers {
        OfferForm::Minted => {
            passed.id = core.minter.uuid();
            passed.binary_bucket = vec![0];
        }
        OfferForm::OfferersId => {
            passed.id = from.agent.uuid();
            passed.region_id = Some(from.region_id);
            passed.position = from.position;
        }
    }
    core.social.lock().offers.insert(
        passed.id,
        Offer {
            from: from.agent,
            to,
        },
    );
    let delivered = tell(core, to, |state, now| {
        warn_on(
            state.sim.send_instant_message(&passed, now),
            "a friendship offer",
        );
    })
    .await;
    if !delivered {
        // Read back over UDP it is passed on as it would have been live, kept
        // and with the time it was sent; Second Life's record of it states the
        // session of the two and the offerer's region and place instead.
        let message = InstantMessage {
            offline: true,
            timestamp: Some(now_epoch_seconds()),
            ..passed
        };
        let record = InstantMessage {
            id: session_of(from.agent, to),
            region_id: Some(from.region_id),
            position: from.position,
            ..message.clone()
        };
        core.social.lock().keep(to, KeptMessage { message, record });
    }
}

/// An avatar declined an offer.
async fn declined(core: &Arc<GridCore>, shared: &SharedSim, offer: Offer) {
    if !core.friends_policy.decline_told {
        return;
    }
    let by = {
        let state = shared.state.lock().await;
        Whereabouts::of(&state)
    };
    let mut told = message_from(
        &by,
        offer.from,
        ImDialog::FriendshipDeclined,
        &by.agent.uuid().to_string(),
    );
    told.id = session_of(offer.from, offer.to);
    told.region_id = Some(by.region_id);
    tell(core, offer.from, |state, now| {
        warn_on(
            state.sim.send_instant_message(&told, now),
            "a declined offer",
        );
    })
    .await;
}

/// An offer kept for the avatar was answered through a capability, which
/// names its offerer: the offer is accepted or forgotten, and nothing of it
/// is kept for the avatar any more.
async fn kept_answered(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    from: AgentKey,
    to: AgentKey,
    accept: bool,
) {
    let offer = {
        let mut social = core.social.lock();
        let ids: Vec<Uuid> = social
            .offers
            .iter()
            .filter(|(_id, offer)| offer.from == from && offer.to == to)
            .map(|(id, _offer)| *id)
            .collect();
        for id in &ids {
            social.offers.remove(id);
        }
        drop(social);
        (!ids.is_empty()).then_some(Offer { from, to })
    };
    shared
        .with_sim(|sim| {
            let kept = sim.take_offline_messages();
            for kept in kept {
                let offer = &kept.message;
                if offer.dialog != ImDialog::FriendshipOffered || offer.from_agent_id != from {
                    sim.store_offline_message(kept);
                }
            }
        })
        .await;
    if accept && let Some(offer) = offer {
        befriend(core, offer, None).await;
    }
}

/// An offer was accepted: the two are friends, each seeing the other online,
/// and the grid tells them as it does; `folder` is where the accepter asked
/// for the offerer's calling card.
async fn befriend(core: &Arc<GridCore>, offer: Offer, folder: Option<InventoryFolderKey>) {
    {
        let mut social = core.social.lock();
        social
            .rights
            .insert((offer.from, offer.to), FriendRights::CAN_SEE_ONLINE);
        social
            .rights
            .insert((offer.to, offer.from), FriendRights::CAN_SEE_ONLINE);
    }
    let Some(accepter) = whereabouts_of(core, offer.to).await else {
        return;
    };
    let accepter_name = legacy_name(core, offer.to);
    let offerer_name = legacy_name(core, offer.from);
    match core.friends_policy.acceptance {
        Acceptance::Bare => {
            let mut told = message_from(&accepter, offer.from, ImDialog::FriendshipAccepted, "");
            told.binary_bucket = vec![0];
            let online = [FriendKey::from(offer.to.uuid())];
            tell(core, offer.from, |state, now| {
                warn_on(
                    state.sim.send_instant_message(&told, now),
                    "an accepted offer",
                );
                warn_on(
                    state.sim.send_online_notification(&online, now),
                    "a new friend online",
                );
            })
            .await;
        }
        Acceptance::Stated => {
            let mut told = message_from(
                &accepter,
                offer.from,
                ImDialog::FriendshipAccepted,
                &offer.to.uuid().to_string(),
            );
            told.id = session_of(offer.from, offer.to);
            told.region_id = Some(accepter.region_id);
            let card_of_accepter = core.minter.uuid();
            tell(core, offer.from, |state, now| {
                warn_on(
                    state.sim.send_instant_message(&told, now),
                    "an accepted offer",
                );
                let folder = folder_of_type(&state.sim, CALLING_CARD_FOLDER)
                    .unwrap_or_else(|| InventoryFolderKey::from(Uuid::nil()));
                let card = calling_card(
                    card_of_accepter,
                    folder,
                    offer.from,
                    offer.to,
                    &accepter_name,
                );
                give_card(state, card, now);
            })
            .await;
            let card_of_offerer = core.minter.uuid();
            let online = [FriendKey::from(offer.from.uuid())];
            tell(core, offer.to, |state, now| {
                warn_on(
                    state.sim.send_online_notification(&online, now),
                    "a new friend online",
                );
                let folder = folder.unwrap_or_else(|| InventoryFolderKey::from(Uuid::nil()));
                let card =
                    calling_card(card_of_offerer, folder, offer.to, offer.from, &offerer_name);
                give_card(state, card, now);
            })
            .await;
        }
    }
}

/// An avatar ended a friendship, which there may not have been: both are told.
async fn end(core: &Arc<GridCore>, agent: AgentKey, other: AgentKey) {
    {
        let mut social = core.social.lock();
        social.rights.remove(&(agent, other));
        social.rights.remove(&(other, agent));
    }
    tell(core, agent, |state, now| {
        warn_on(
            state
                .sim
                .send_terminate_friendship(FriendKey::from(other.uuid()), now),
            "an ended friendship",
        );
    })
    .await;
    tell(core, other, |state, now| {
        warn_on(
            state
                .sim
                .send_terminate_friendship(FriendKey::from(agent.uuid()), now),
            "an ended friendship",
        );
    })
    .await;
}

/// An avatar changed what it grants a friend: it is told back what it
/// granted, and the friend is told as the grid tells it.
async fn grant(core: &Arc<GridCore>, granter: AgentKey, entry: UserRightsEntry) {
    let grantee = AgentKey::from(entry.agent.uuid());
    let rights = entry.rights.0;
    let (old, granter_online) = {
        let mut social = core.social.lock();
        let Some(old) = social.rights(granter, grantee) else {
            return;
        };
        social.rights.insert((granter, grantee), rights);
        (old, social.online.contains(&granter))
    };
    let echo = [UserRightsEntry {
        agent: entry.agent,
        rights: FriendRights(rights),
    }];
    tell(core, granter, |state, now| {
        warn_on(
            state.sim.send_change_user_rights(granter, &echo, now),
            "rights granted",
        );
    })
    .await;
    let policy = core.friends_policy;
    let see_online = FriendRights::CAN_SEE_ONLINE;
    let presence =
        (granter_online && (old ^ rights) & see_online != 0).then_some(rights & see_online != 0);
    let told = if policy.masks_see_online {
        ((old | rights) & !see_online != 0).then_some(rights & !see_online)
    } else {
        Some(rights)
    };
    let granter_key = [FriendKey::from(granter.uuid())];
    tell(core, grantee, |state, now| {
        match presence {
            Some(true) => warn_on(
                state.sim.send_online_notification(&granter_key, now),
                "a friend online",
            ),
            Some(false) => warn_on(
                state.sim.send_offline_notification(&granter_key, now),
                "a friend offline",
            ),
            None => {}
        }
        if let Some(told) = told {
            let entry = [UserRightsEntry {
                agent: FriendKey::from(grantee.uuid()),
                rights: FriendRights(told),
            }];
            warn_on(
                state.sim.send_change_user_rights(granter, &entry, now),
                "rights granted",
            );
        }
    })
    .await;
}

/// An avatar asked where another is: answered with the one place it stands,
/// where the other lets it see it on the map.
async fn find(core: &Arc<GridCore>, agent: AgentKey, hunter: Uuid, prey: AgentKey) {
    if !core
        .social
        .lock()
        .grants(prey, agent, FriendRights::CAN_SEE_ON_MAP)
    {
        return;
    }
    let Some(there) = whereabouts_of(core, prey).await else {
        return;
    };
    let (west, south) = there.handle.global_coordinates();
    let place = [(
        f64::from(west) + f64::from(there.position.x()),
        f64::from(south) + f64::from(there.position.y()),
    )];
    tell(core, agent, |state, now| {
        warn_on(
            state
                .sim
                .send_find_agent_reply(hunter, prey.uuid(), &place, now),
            "where a friend is",
        );
    })
    .await;
}

/// An avatar tracks another, or stops: on a grid that tracks, the coarse
/// feed points at a friend who lets it see it on the map.
async fn track(core: &Arc<GridCore>, shared: &SharedSim, agent: AgentKey, prey: AgentKey) {
    let tracked = if prey.uuid().is_nil() || !core.friends_policy.tracks {
        None
    } else if core
        .social
        .lock()
        .grants(prey, agent, FriendRights::CAN_SEE_ON_MAP)
    {
        whereabouts_of(core, prey)
            .await
            .map(|there| (prey, there.position))
    } else {
        None
    };
    shared.with_sim(|sim| sim.set_coarse_prey(tracked)).await;
}

/// An avatar offered another its calling card.
async fn offer_card(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    to: AgentKey,
    transaction: TransactionId,
) {
    let from = {
        let state = shared.state.lock().await;
        Whereabouts::of(&state)
    };
    match core.friends_policy.cards {
        CardOffers::AsSent => {
            core.social.lock().cards.insert(
                transaction.get(),
                CardOffer {
                    from: from.agent,
                    to,
                },
            );
            tell(core, to, |state, now| {
                warn_on(
                    state
                        .sim
                        .send_offer_calling_card(from.agent, transaction, now),
                    "a calling card offered",
                );
            })
            .await;
        }
        CardOffers::MadeAtOnce => {
            let item = core.minter.uuid();
            core.social.lock().cards.insert(
                item,
                CardOffer {
                    from: from.agent,
                    to,
                },
            );
            tell(core, to, |state, now| {
                let folder = folder_of_type(&state.sim, CALLING_CARD_FOLDER)
                    .unwrap_or_else(|| InventoryFolderKey::from(Uuid::nil()));
                give_card(
                    state,
                    calling_card(item, folder, to, from.agent, &from.name),
                    now,
                );
                warn_on(
                    state
                        .sim
                        .send_offer_calling_card(from.agent, TransactionId::from(item), now),
                    "a calling card offered",
                );
            })
            .await;
        }
    }
}

/// An avatar answered a calling card offer: accepted into `folder`, or
/// declined.
async fn card_answered(
    core: &Arc<GridCore>,
    agent: AgentKey,
    transaction: TransactionId,
    folder: Option<InventoryFolderKey>,
) {
    let Some(card) = core
        .social
        .lock()
        .cards
        .remove(&transaction.get())
        .filter(|card| card.to == agent)
    else {
        return;
    };
    match core.friends_policy.cards {
        CardOffers::AsSent => {
            // The offerer is told under its own id, as Second Life tells it.
            tell(core, card.from, |state, now| {
                let told = match folder {
                    Some(_) => state
                        .sim
                        .send_accept_calling_card(card.from, transaction, now),
                    None => state
                        .sim
                        .send_decline_calling_card(card.from, transaction, now),
                };
                warn_on(told, "an answered calling card");
            })
            .await;
            if let Some(folder) = folder {
                let item = core.minter.uuid();
                let name = legacy_name(core, card.from);
                tell(core, agent, |state, now| {
                    give_card(
                        state,
                        calling_card(item, folder, agent, card.from, &name),
                        now,
                    );
                })
                .await;
            }
        }
        CardOffers::MadeAtOnce => {
            if folder.is_some() {
                return;
            }
            tell(core, agent, |state, now| {
                let item = InventoryKey::from(transaction.get());
                let trash = folder_of_type(&state.sim, TRASH_FOLDER);
                if let (Some(mut card), Some(trash)) =
                    (state.sim.agent_inventory_mut().take_item(item), trash)
                {
                    card.folder_id = trash;
                    give_card(state, card, now);
                }
            })
            .await;
        }
    }
}

/// An avatar asked for what was kept for it (`RetrieveInstantMessages`):
/// each is passed on as it would have arrived, marked kept, and nothing is
/// kept any more — by this road or the capability's.
async fn retrieve(shared: &SharedSim) {
    let now = shared.now();
    shared
        .with_sim(|sim| {
            for kept in sim.take_offline_messages() {
                warn_on(
                    sim.send_instant_message(&kept.message, now),
                    "a kept message",
                );
            }
        })
        .await;
}
