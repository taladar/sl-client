//! The **friendship offer** path (`viewer-add-friend-offers-silently`): the one
//! place an Add Friend affordance turns into an `OfferFriendship` on the wire.
//!
//! Every Add Friend in the viewer used to write the command itself, with an
//! always-empty message and nothing afterwards to say the offer went out. The
//! offer *was* sent — but a working button and a dead one look identical when
//! neither draws anything, which is the whole complaint the bug records.
//!
//! # One guarded way in
//!
//! So the surfaces write a [`RequestFriendship`] instead, the way every Block
//! affordance writes a [`RequestBlock`](crate::world_api::RequestBlock)
//! ([`crate::mutes::apply_block_requests`]), and this module answers it the way
//! the reference's `LLAvatarActions::requestFriendshipDialog` does:
//!
//! 1. **Refuse the agent itself** with the `AddSelfFriend` tip, rather than
//!    offering friendship to oneself.
//! 2. **Ask for the accompanying message** with the `AddFriendWithMessage`
//!    dialog, pre-filled with the reference's "Would you be my friend?", and
//!    send nothing at all if it is cancelled.
//! 3. **Send what was typed** as the offer's message — the recipient sees it on
//!    their offer card.
//! 4. **Say it happened** with the reference's `FriendshipOffered` notice, so a
//!    working offer never again looks like a dead button.
//!
//! A fifth thing the reference does here — filing the target under **Recent
//! People** — waits on [[viewer-recent-people]], which is that whole model and
//! the ten other places the reference files from.
//!
//! # One prompt, however many residents
//!
//! A multi-selection (the radar's, the minimap's) is **one**
//! [`RequestFriendship`] naming everyone, asked once and offered to each with
//! the same typed message — the way the multi-avatar menus already treat one
//! action over a list. The reference has no multi-selection Add Friend at all,
//! so the list in the dialog body and in the confirmation is ours: the shown
//! labels, comma-joined.
//!
//! # Why a queue and not a slot
//!
//! A [`NotificationResponse`] carries the **template name**, not the id of the
//! raise it answers, so a second dialog raised while the first is still up
//! could not be told from it — and its answer would offer friendship to the
//! wrong people. Only one prompt is therefore outstanding at a time
//! (`FriendshipOfferQueue`); a request that arrives meanwhile waits its turn
//! rather than being dropped, so no click is ever silently discarded.
//!
//! # What is filtered before asking
//!
//! Residents who are **already friends** are dropped from the batch (the radar
//! did this itself before the path was shared): the reference's menus disable
//! Add Friend for a friend, so offering again is not an action the user can
//! ask for in the first place.
//!
//! Reference (Firestorm, read-only): `llavataractions.cpp`
//! (`requestFriendshipDialog`, `callbackAddFriendWithMessage`,
//! `requestFriendship`), `notifications.xml` (`AddFriendWithMessage`,
//! `AddSelfFriend`, `FriendshipOffered`).

use std::collections::VecDeque;

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, Command, SlCommand, SlIdentity};

use crate::notifications::{NotificationResponse, ShowNotification};
use crate::world_api::{AvatarState, FriendsModel, RequestFriendship};

/// The reference dialog that asks for the offer's message before sending it.
const ASK_TEMPLATE: &str = "AddFriendWithMessage";

/// The reference tip that refuses self-friendship.
const SELF_TEMPLATE: &str = "AddSelfFriend";

/// The reference notice confirming an offer went out.
const SENT_TEMPLATE: &str = "FriendshipOffered";

/// The [`ASK_TEMPLATE`] button that sends (`OFFER_CANCEL_FORM`'s default).
const OFFER_BUTTON: &str = "Offer";

/// What joins several residents' labels in the dialog body and the
/// confirmation. Ours, not the reference's — it never names more than one.
const NAME_SEPARATOR: &str = ", ";

/// The friendship-offer path: the queue and the two systems that ask and send.
#[derive(Debug)]
pub struct AddFriendPlugin;

impl Plugin for AddFriendPlugin {
    /// Register the queue and the ask / send pair.
    ///
    /// The send runs **first**: it is what frees the outstanding prompt, so an
    /// answered dialog and the next batch's dialog happen in the same frame
    /// rather than a frame apart.
    fn build(&self, app: &mut App) {
        app.init_resource::<FriendshipOfferQueue>()
            // Owned here, because this is what answers it: a `MessageWriter` for
            // an unregistered message is a system that never runs, so every Add
            // Friend surface in the viewer depends on this registration.
            .add_message::<RequestFriendship>()
            .add_systems(
                Update,
                (send_prompted_friendship_offers, prompt_friendship_requests).chain(),
            );
    }
}

/// The batches of residents waiting to be offered friendship.
///
/// One prompt at a time ([`asking`](Self::asking)), because a
/// [`NotificationResponse`] names the template it answers and not the raise —
/// two live `AddFriendWithMessage` dialogs would be indistinguishable, and the
/// first answer would offer to the second's residents.
#[derive(Resource, Debug, Default)]
struct FriendshipOfferQueue {
    /// The batch whose dialog is on screen, if one is.
    asking: Option<Vec<AgentKey>>,
    /// Batches that arrived while a dialog was up, in the order they were asked
    /// for.
    waiting: VecDeque<Vec<AgentKey>>,
}

/// The residents' shown labels, comma-joined — what the dialog body and the
/// confirmation name. [`AvatarState::label_text`] always answers, falling back
/// to a provisional id fragment while a name is still resolving, so the dialog
/// never shows an empty subject.
fn label_list(avatars: &AvatarState, targets: &[AgentKey]) -> String {
    targets
        .iter()
        .map(|agent| avatars.label_text(*agent))
        .collect::<Vec<_>>()
        .join(NAME_SEPARATOR)
}

/// Turn each [`RequestFriendship`] into a queued batch — refusing the agent
/// itself and dropping existing friends — and raise the next batch's dialog
/// whenever no other one is outstanding.
fn prompt_friendship_requests(
    mut requests: MessageReader<RequestFriendship>,
    identity: Res<SlIdentity>,
    friends: Option<Res<FriendsModel>>,
    avatars: Res<AvatarState>,
    mut queue: ResMut<FriendshipOfferQueue>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    let own = identity.agent_id;
    for request in requests.read() {
        let mut targets: Vec<AgentKey> = Vec::new();
        let mut asked_for_self = false;
        for agent in &request.targets {
            if Some(*agent) == own {
                asked_for_self = true;
                continue;
            }
            if friends
                .as_deref()
                .is_some_and(|friends| friends.is_friend(*agent))
            {
                continue;
            }
            // A selection can name the same resident twice (a radar row and the
            // avatar under the cursor); one offer each is enough.
            if !targets.contains(agent) {
                targets.push(*agent);
            }
        }
        // Said even when the rest of a mixed selection is still offered: the
        // user asked for something that cannot happen, and hearing so is the
        // reference's behaviour for the whole request.
        if asked_for_self {
            notifications.write(ShowNotification::new(SELF_TEMPLATE));
        }
        if !targets.is_empty() {
            queue.waiting.push_back(targets);
        }
    }
    if queue.asking.is_some() {
        return;
    }
    if let Some(next) = queue.waiting.pop_front() {
        notifications
            .write(ShowNotification::new(ASK_TEMPLATE).arg("NAME", label_list(&avatars, &next)));
        queue.asking = Some(next);
    }
}

/// Send the offers a raised dialog was answered **Offer** on, carrying the
/// message the user typed, and confirm what went out.
///
/// Any other answer — Cancel, the close ×, a programmatic dismissal — frees the
/// outstanding prompt without sending, so a cancelled dialog cannot leave a
/// batch stuck in front of the queue.
fn send_prompted_friendship_offers(
    mut responses: MessageReader<NotificationResponse>,
    avatars: Res<AvatarState>,
    mut queue: ResMut<FriendshipOfferQueue>,
    mut commands: MessageWriter<SlCommand>,
    mut notifications: MessageWriter<ShowNotification>,
) {
    for response in responses.read() {
        if response.template != ASK_TEMPLATE {
            continue;
        }
        let Some(targets) = queue.asking.take() else {
            continue;
        };
        if response.button != Some(OFFER_BUTTON) {
            continue;
        }
        // An inputless resolve (a dismissal that still chose the button) sends
        // the empty message the wire allows rather than nothing at all.
        let message = response.input.clone().unwrap_or_default();
        for agent in &targets {
            commands.write(SlCommand(Command::OfferFriendship {
                to_agent_id: *agent,
                message: message.clone(),
            }));
        }
        notifications.write(
            ShowNotification::new(SENT_TEMPLATE).arg("TO_NAME", label_list(&avatars, &targets)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ASK_TEMPLATE, AddFriendPlugin, FriendshipOfferQueue, OFFER_BUTTON, SELF_TEMPLATE,
        SENT_TEMPLATE,
    };
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, Command, Friend, FriendKey, FriendRights, SlCommand, SlIdentity, Uuid,
    };

    use crate::notifications::{
        NotificationId, NotificationManager, NotificationResponse, ShowNotification,
    };
    use crate::world_api::{AvatarState, FriendsModel, RequestFriendship};

    /// A stable test agent.
    fn agent(n: u128) -> AgentKey {
        AgentKey::from(Uuid::from_u128(n))
    }

    /// An id for a response the test writes. The host allocates the real ones;
    /// this path routes on the template name, so any id will do.
    fn some_id() -> NotificationId {
        let mut manager = NotificationManager::default();
        manager.allocate_id()
    }

    /// What the path produced over a whole test, in order.
    ///
    /// Accumulated by [`record`] rather than read off the message buffers at the
    /// end: Bevy drops a message two frames after it is written, so a test that
    /// answers a dialog would no longer see the raise that asked it.
    #[derive(Resource, Debug, Default)]
    struct Recorded {
        /// The catalogue templates raised, in order.
        raised: Vec<&'static str>,
        /// The friendship offers put on the wire, as `(target, message)`.
        offers: Vec<(AgentKey, String)>,
    }

    /// Fold the frame's raises and commands into [`Recorded`].
    fn record(
        mut shows: MessageReader<ShowNotification>,
        mut commands: MessageReader<SlCommand>,
        mut recorded: ResMut<Recorded>,
    ) {
        for show in shows.read() {
            recorded.raised.push(show.template);
        }
        for command in commands.read() {
            if let Command::OfferFriendship {
                to_agent_id,
                message,
            } = &command.0
            {
                recorded.offers.push((*to_agent_id, message.clone()));
            }
        }
    }

    /// An app with the offer path and the world it reads: an identity (so the
    /// self case has something to compare against) and an empty name cache.
    fn offer_app() -> App {
        let mut app = App::new();
        app.add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .add_message::<SlCommand>()
            .init_resource::<AvatarState>()
            .init_resource::<FriendsModel>()
            .init_resource::<Recorded>()
            .insert_resource(SlIdentity {
                agent_id: Some(agent(1)),
                ..SlIdentity::default()
            })
            .add_plugins(AddFriendPlugin)
            // After the path's own systems, so a frame's raises and sends are
            // recorded in the frame that produced them.
            .add_systems(Last, record);
        app
    }

    /// Ask to befriend `targets`, and run a frame.
    fn request(app: &mut App, targets: Vec<AgentKey>) {
        app.world_mut().write_message(RequestFriendship { targets });
        app.update();
    }

    /// Answer the outstanding dialog with `button` and `message`, and run a
    /// frame.
    fn answer(app: &mut App, button: Option<&'static str>, message: Option<&str>) {
        app.world_mut().write_message(NotificationResponse {
            id: some_id(),
            template: ASK_TEMPLATE,
            button,
            ignored: false,
            input: message.map(ToOwned::to_owned),
        });
        app.update();
    }

    /// The template names raised so far.
    fn raised(app: &App) -> Vec<&'static str> {
        app.world().resource::<Recorded>().raised.clone()
    }

    /// Every friendship offer put on the wire so far, as `(target, message)`.
    fn offers(app: &App) -> Vec<(AgentKey, String)> {
        app.world().resource::<Recorded>().offers.clone()
    }

    /// The whole point of the bug: a request **asks** before it sends, and the
    /// answer's typed message is what goes on the wire — with the reference's
    /// confirmation afterwards, so the offer is never silent.
    #[test]
    fn an_offer_asks_first_and_carries_the_typed_message() {
        let mut app = offer_app();
        request(&mut app, vec![agent(2)]);
        assert_eq!(
            raised(&app),
            vec![ASK_TEMPLATE],
            "the request must raise the message dialog, not send"
        );
        assert_eq!(offers(&app), vec![], "nothing may go out before the answer");

        answer(&mut app, Some(OFFER_BUTTON), Some("hello there"));
        assert_eq!(
            offers(&app),
            vec![(agent(2), "hello there".to_owned())],
            "the offer must carry what the user typed"
        );
        assert_eq!(
            raised(&app),
            vec![ASK_TEMPLATE, SENT_TEMPLATE],
            "and the send must be confirmed"
        );
    }

    /// Cancelling sends nothing, and does not leave the prompt outstanding.
    #[test]
    fn cancelling_sends_nothing_and_frees_the_prompt() {
        let mut app = offer_app();
        request(&mut app, vec![agent(2)]);
        answer(&mut app, Some("Cancel"), Some("unsent"));
        assert_eq!(offers(&app), vec![], "a cancelled dialog sends no offer");
        assert_eq!(
            raised(&app),
            vec![ASK_TEMPLATE],
            "and confirms nothing either"
        );
        assert!(
            app.world()
                .resource::<FriendshipOfferQueue>()
                .asking
                .is_none(),
            "the cancelled batch must not stay in front of the queue"
        );
    }

    /// A dismissal with no button chosen is a cancel, not a send.
    #[test]
    fn a_dismissal_is_not_a_send() {
        let mut app = offer_app();
        request(&mut app, vec![agent(2)]);
        answer(&mut app, None, None);
        assert_eq!(offers(&app), vec![]);
    }

    /// The agent cannot befriend itself: the reference's tip, and no dialog.
    #[test]
    fn befriending_oneself_is_refused() {
        let mut app = offer_app();
        request(&mut app, vec![agent(1)]);
        assert_eq!(raised(&app), vec![SELF_TEMPLATE]);
        assert_eq!(offers(&app), vec![]);
    }

    /// One dialog for a whole selection, offered to each — and an existing
    /// friend in it is dropped rather than offered friendship twice.
    #[test]
    fn a_selection_asks_once_and_offers_to_each() {
        let mut app = offer_app();
        {
            let mut friends = app.world_mut().resource_mut::<FriendsModel>();
            friends.note_friends(&[Friend {
                id: FriendKey::from(agent(4).uuid()),
                rights_granted: FriendRights(0),
                rights_received: FriendRights(0),
            }]);
        }
        request(&mut app, vec![agent(2), agent(3), agent(4), agent(2)]);
        assert_eq!(
            raised(&app),
            vec![ASK_TEMPLATE],
            "several residents are one dialog"
        );
        answer(&mut app, Some(OFFER_BUTTON), Some("hi"));
        assert_eq!(
            offers(&app),
            vec![(agent(2), "hi".to_owned()), (agent(3), "hi".to_owned()),],
            "each named resident is offered once; the existing friend is not"
        );
    }

    /// A second request while a dialog is up waits its turn instead of being
    /// dropped — and its own dialog goes up the moment the first is answered.
    #[test]
    fn a_second_request_waits_for_the_first_dialog() {
        let mut app = offer_app();
        request(&mut app, vec![agent(2)]);
        request(&mut app, vec![agent(3)]);
        assert_eq!(
            raised(&app),
            vec![ASK_TEMPLATE],
            "only one dialog is outstanding at a time"
        );

        answer(&mut app, Some(OFFER_BUTTON), Some("first"));
        assert_eq!(
            raised(&app),
            vec![ASK_TEMPLATE, SENT_TEMPLATE, ASK_TEMPLATE],
            "answering the first must raise the waiting one in the same frame"
        );

        answer(&mut app, Some(OFFER_BUTTON), Some("second"));
        assert_eq!(
            offers(&app),
            vec![
                (agent(2), "first".to_owned()),
                (agent(3), "second".to_owned()),
            ],
            "each batch is offered the message its own dialog was answered with"
        );
    }
}
