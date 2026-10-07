//! End-to-end tests for what two residents see of each other
//! ([[test-e2e-sweep-two-avatars]]).
//!
//! On the fake grid two stage viewers do not see each other's avatars and
//! their IMs are not relayed, so the other resident is played by the grid:
//! a friend account that never logs in, a catalogue NPC, or a message the
//! test sends through the viewer's own session. What only a real grid
//! relays runs there (`SL_E2E_GRID=opensim|aditi`) and skips here:
//!
//! - a friend's edit-objects right asks before it is granted, Cancel sends
//!   nothing, and every toggle — and a right the friend grants — reaches the
//!   other side;
//! - an Unavailable reply is sent again once its conversation is closed, and
//!   a contact set's own reply is the one a member hears;
//! - the own typing is told to the grid, and another resident's typing shows;
//! - the radar's row menu tracks, teleports to, offers friendship to, blocks
//!   and derenders a resident (with what they wear); its sort survives a
//!   relog; an arrival raises a toast and a sound; a neighbour region's
//!   resident is drawn as an approximate position;
//! - the minimap draws a resident and a tracking beacon in different colours;
//! - an About Land window left open follows the owner's rename;
//! - the viewer's own bake reaches the grid, and (OpenSim) the other viewer;
//! - a teleport offer shows its destination's rating where the grid states
//!   one, and each answer to an offer or a request reaches the grid as that
//!   grid expects it;
//! - two residents befriend each other with a message and trade rights
//!   (live);
//! - a teleport offer is declined without a word to the offerer, and a second
//!   one is taken (live);
//! - two editors of one prim: what the loser is told (live).

#[cfg(test)]
mod test {
    use core::time::Duration;
    use std::path::Path;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{Locator, LogStream, NodeVisibility, Probe, Role};
    use sl_e2e::{BodyError, FIRST_NAME, Need, Stage, StageBuilder};
    use sl_fake_grid::fixtures::scenarios;
    use sl_fake_grid::scenario::{STOCK_PARCEL_LOCAL_ID, STOCK_PARCEL_NAME};
    use sl_fake_grid::{AccountConfig, AvatarIdentity, ImitatedGrid, NpcFixture, RegionConfig};
    use sl_proto::{
        AgentKey, AnyMessage, ChatSource, ChatType, CoarseLocation, FriendKey, FriendRights,
        ImDialog, InstantMessage, OwnerKey, RegionCoordinates, RegionLocalObjectId, ServerEvent,
        UserRightsEntry, Uuid, Vector,
    };
    use sl_viewer_driver::{UiLocator, Viewer};
    use tokio::sync::broadcast;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// A stage for `name` with the viewers `labels`.
    fn stage(name: &str, labels: &[&str]) -> StageBuilder {
        labels.iter().fold(
            StageBuilder::new(name).viewer_binary(VIEWER),
            |builder, label| builder.viewer(*label),
        )
    }

    /// The catalogue scene as the home region: the one with the NPCs.
    fn catalogue() -> Result<RegionConfig, TestError> {
        let scenario =
            scenarios::scenario("catalogue").ok_or("the catalogue scenario is not registered")?;
        Ok(scenario.dress(RegionConfig::default()))
    }

    /// Wait until the grid's session reports an event `wanted` accepts: the
    /// viewer's `what` reaching the grid. The event itself, for a closer look.
    async fn grid_hears(
        heard: &mut broadcast::Receiver<ServerEvent>,
        what: &str,
        wanted: impl Fn(&ServerEvent) -> bool + Send + Sync,
    ) -> Result<ServerEvent, BodyError> {
        let seen = tokio::time::timeout(WAIT, async {
            loop {
                match heard.recv().await {
                    Ok(event) if wanted(&event) => return Ok(event),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        })
        .await;
        match seen {
            Ok(Ok(event)) => Ok(event),
            Ok(Err(error)) => Err(format!("the grid's event stream: {error}").into()),
            Err(_elapsed) => Err(format!("the grid never heard the {what}").into()),
        }
    }

    /// How many entries of `kind` the viewer's `stream` log holds.
    async fn logged(viewer: &Viewer, stream: LogStream, kind: &str) -> Result<usize, BodyError> {
        Ok(viewer
            .events_from_start()
            .read(&[stream])
            .await?
            .iter()
            .filter(|entry| entry.kind == kind)
            .count())
    }

    /// The `First Last` name of the stage account `last`.
    fn account(last: &str) -> String {
        format!("{FIRST_NAME} {last}")
    }

    /// The agent id of the stage account `last`.
    fn account_id(stage: &Stage, last: &str) -> Result<AgentKey, BodyError> {
        Ok(stage
            .grid()?
            .account_agent_id(FIRST_NAME, last)
            .ok_or_else(|| format!("no account {last}"))?)
    }

    /// The Conversations window, opened on its People tab's `tab`.
    async fn people_tab(viewer: &Viewer, tab: &str) -> Result<UiLocator, BodyError> {
        let window = viewer.open_floater("conversations").await?;
        let _people = window
            .get(Locator::role(Role::Tab).name_key("people-tab"))
            .click()
            .await?;
        let _tab = window
            .get(Locator::role(Role::Tab).name_key(tab))
            .click()
            .await?;
        Ok(window)
    }

    /// Send viewer `label` `im` from the grid.
    async fn deliver(stage: &Stage, label: &str, im: &InstantMessage) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        agent
            .with_sim(|sim| sim.send_instant_message(im, now))
            .await
            .map_err(|error| format!("delivering the IM: {error}"))?;
        Ok(())
    }

    /// A typed IM from `from` (called `from_name`) to viewer `label`.
    fn im_from(
        stage: &Stage,
        label: &str,
        from: AgentKey,
        from_name: &str,
        message: &str,
    ) -> Result<InstantMessage, BodyError> {
        let to = stage.agent_id(label)?;
        Ok(InstantMessage {
            from_agent_id: from,
            from_agent_name: from_name.to_owned(),
            to_agent_id: to,
            dialog: ImDialog::Message,
            from_group: false,
            region_id: None,
            position: RegionCoordinates::new(128.0, 128.0, 25.0),
            offline: false,
            timestamp: None,
            id: Uuid::from_u128(to.uuid().as_u128() ^ from.uuid().as_u128()),
            parent_estate_id: 1,
            message: message.to_owned(),
            binary_bucket: Vec::new(),
        })
    }

    /// The viewer's do-not-disturb reply to `to`, as the grid heard it.
    fn busy_reply_to(event: &ServerEvent, to: AgentKey) -> Option<String> {
        match event {
            ServerEvent::InstantMessage(im)
                if im.to_agent_id == to && im.dialog == ImDialog::DoNotDisturbAutoResponse =>
            {
                Some(im.message.clone())
            }
            _ => None,
        }
    }

    // ---- Teleport offers and requests --------------------------------------

    /// The template a teleport offer's card reports as.
    const TELEPORT_OFFER: &str = "TeleportOffered";

    /// The template a teleport request's card reports as.
    const TELEPORT_REQUEST: &str = "TeleportRequest";

    /// How the offer card's line about its destination's rating begins. (The
    /// rating itself follows inside the isolation marks a translated
    /// placeable is wrapped in, so the two are matched apart.)
    const RATING_LINE: &str = "Destination rating:";

    /// A teleport offer or request from the stranger to viewer `label`, with
    /// the lure id and binary bucket a grid would give it.
    fn lure_from_stranger(
        stage: &Stage,
        label: &str,
        dialog: ImDialog,
        id: Uuid,
        bucket: &[u8],
    ) -> Result<InstantMessage, BodyError> {
        let stranger = AgentKey::from(Uuid::from_u128(STRANGER));
        Ok(InstantMessage {
            dialog,
            id,
            binary_bucket: bucket.to_vec(),
            ..im_from(stage, label, stranger, STRANGER_NAME, "Join me")?
        })
    }

    /// The one offer card on screen.
    fn offer_card(viewer: &Viewer) -> UiLocator {
        viewer.ui().test_id("offer-invite-card")
    }

    /// Press `button` of the one offer card on screen and wait for it to go.
    async fn answer_offer(viewer: &Viewer, button: &str) -> Result<(), BodyError> {
        let card = offer_card(viewer);
        let _pressed = card
            .test_id(&format!("offer-invite-action:{button}"))
            .click()
            .await?;
        let _gone = viewer.expect(&card).timeout(WAIT).to_be_detached().await?;
        Ok(())
    }

    /// **Teleport offers and requests**, as each grid words them
    /// (`book/src/gridspec/teleport.md`). A Second Life offer carries an opaque
    /// lure id and names its destination's rating in the binary bucket, which
    /// the card shows; an OpenSim offer's id is the place and its bucket is
    /// empty, and the card says nothing of a rating. Either way Decline sends
    /// the `IM_LURE_DECLINED` naming the lure and Teleport sends the
    /// `TeleportLureRequest` for it. A teleport *request* raises a card of its
    /// own, whose Decline sends nothing — there is no message for it — and
    /// whose Offer Teleport answers with a `StartLure` to the requester.
    #[test]
    fn a_teleport_offer_shows_its_rating_and_every_answer_reaches_the_grid() -> Result<(), TestError>
    {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "lure_cards_second_life"),
            (ImitatedGrid::OpenSim, "lure_cards_open_sim"),
        ] {
            stage(name, &["Alpha"])
                .needs(Need::GridControl)
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;
                    let stranger = AgentKey::from(Uuid::from_u128(STRANGER));
                    let home = stage
                        .grid()?
                        .region_handle(&RegionConfig::default().name)
                        .ok_or("the stage has no home region")?;
                    // The offer as each grid sends it.
                    let (lure, bucket): (Uuid, &[u8]) = match flavour {
                        ImitatedGrid::SecondLife => (
                            Uuid::from_u128(0x3b6b_7c62_8f8f_4e34_9c1a_79c2_e2ba_0fd1),
                            b"256000|256000|128|128|25|-1|0|-0|M \0",
                        ),
                        ImitatedGrid::OpenSim => (
                            sl_proto::FakeParcelId {
                                region_handle: home,
                                x: 100,
                                y: 100,
                                z: 30,
                            }
                            .to_uuid(),
                            b"",
                        ),
                    };
                    let mut heard = stage.agent("Alpha").await?.events();

                    // An offer, declined.
                    deliver(
                        stage,
                        "Alpha",
                        &lure_from_stranger(stage, "Alpha", ImDialog::LureUser, lure, bucket)?,
                    )
                    .await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(TELEPORT_OFFER)
                        .await?;
                    let rating = offer_card(alpha)
                        .get(Locator::role(Role::Text).name_containing(RATING_LINE));
                    match flavour {
                        ImitatedGrid::SecondLife => {
                            let _rated = alpha
                                .expect(&rating)
                                .timeout(WAIT)
                                .to_contain_text("Moderate")
                                .await?;
                        }
                        ImitatedGrid::OpenSim => assert_eq!(
                            rating.count().await?,
                            0,
                            "an offer that states no rating shows none"
                        ),
                    }
                    answer_offer(alpha, "Decline").await?;
                    let _declined = grid_hears(&mut heard, "lure decline", |event| {
                        matches!(event, ServerEvent::InstantMessage(im)
                            if im.dialog == ImDialog::LureDeclined
                                && im.to_agent_id == stranger
                                && im.id == lure)
                    })
                    .await?;

                    // A request: Decline is silent, Offer Teleport is an offer.
                    let request = lure_from_stranger(
                        stage,
                        "Alpha",
                        ImDialog::TeleportRequest,
                        Uuid::nil(),
                        b"",
                    )?;
                    deliver(stage, "Alpha", &request).await?;
                    let _asked = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(TELEPORT_REQUEST)
                        .await?;
                    let sent_before = logged(alpha, LogStream::Command, "OfferTeleport").await?;
                    answer_offer(alpha, "Decline").await?;
                    assert_eq!(
                        logged(alpha, LogStream::Command, "OfferTeleport").await?,
                        sent_before,
                        "declining a teleport request sends nothing"
                    );
                    deliver(stage, "Alpha", &request).await?;
                    let _asked_again = alpha
                        .expect(&offer_card(alpha))
                        .timeout(WAIT)
                        .to_be_visible()
                        .await?;
                    answer_offer(alpha, "Accept").await?;
                    let _offered = grid_hears(&mut heard, "answering offer", |event| {
                        matches!(event, ServerEvent::ClientMessage(message)
                            if matches!(&**message, AnyMessage::StartLure(offer)
                                if offer.target_data.len() == 1
                                    && offer.target_data.iter().all(|target|
                                        target.target_id == stranger.uuid())))
                    })
                    .await?;

                    // An offer, accepted: the lure it named is the one asked
                    // for. (Last: a Second-Life-flavoured grid holds no such
                    // lure and answers with nothing, as aditi does.)
                    deliver(
                        stage,
                        "Alpha",
                        &lure_from_stranger(stage, "Alpha", ImDialog::LureUser, lure, bucket)?,
                    )
                    .await?;
                    let _offered_again = alpha
                        .expect(&offer_card(alpha))
                        .timeout(WAIT)
                        .to_be_visible()
                        .await?;
                    answer_offer(alpha, "Accept").await?;
                    let _accepted = grid_hears(&mut heard, "lure acceptance", |event| {
                        matches!(event, ServerEvent::TeleportViaLure { lure_id, .. }
                            if lure_id.get() == lure)
                    })
                    .await?;
                    Ok(())
                })?;
        }
        Ok(())
    }

    // ---- Friendship rights -------------------------------------------------

    /// The friend the rights test toggles.
    const FRIEND: &str = "Rights";

    /// A rights checkbox of the friend `last`'s row in `window`, by its key.
    fn right(window: &UiLocator, last: &str, key: &str) -> UiLocator {
        window
            .get(Locator::role(Role::ListItem).name_containing(account(last)))
            .get(Locator::role(Role::Checkbox).name_key(key))
    }

    /// A button of the edit-rights confirmation, by its caption's key.
    fn confirm_button(viewer: &Viewer, key: &str) -> UiLocator {
        viewer
            .ui()
            .locator(Locator::role(Role::Button).name_key(key))
    }

    /// Whether `event` grants `friend` exactly `rights`.
    fn grants(event: &ServerEvent, friend: AgentKey, rights: i32) -> bool {
        matches!(event, ServerEvent::UserRightsGranted { rights: entries }
            if entries.iter().any(|entry| entry.agent.uuid() == friend.uuid()
                && entry.rights.0 == rights))
    }

    /// **Friendship rights**: ticking a friend's edit-my-objects box asks
    /// first — Cancel sends nothing and leaves it empty, Grant sends the
    /// grant; the map box and the edit box are taken back without asking;
    /// and a right the friend grants ticks the read-only "You can" box,
    /// which cannot be clicked.
    #[test]
    fn granting_edit_rights_asks_first_and_every_toggle_reaches_the_grid() -> Result<(), TestError>
    {
        stage("friend_rights", &["Alpha"])
            .needs(Need::GridControl)
            .configure_grid(|grid| {
                grid.account(AccountConfig::new(FIRST_NAME, FRIEND, "password"))
                    .friends((FIRST_NAME, "Alpha"), (FIRST_NAME, FRIEND))
            })
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let friend = account_id(stage, FRIEND)?;
                let mut heard = stage.agent("Alpha").await?.events();
                let window = people_tab(alpha, "people-friends-tab").await?;
                let edit = right(&window, FRIEND, "people-right-granted-edit");
                let map = right(&window, FRIEND, "people-right-granted-map");
                let _empty = alpha.expect(&edit).timeout(WAIT).to_be_unchecked().await?;
                let _shown = alpha.expect(&map).to_be_checked().await?;

                let _asked = edit.click().await?;
                let prompt = alpha.ui().test_id("people-grant-confirm-text");
                let _prompt = alpha
                    .expect(&prompt)
                    .to_contain_text(&account(FRIEND))
                    .await?;
                let _cancel = confirm_button(alpha, "people-grant-confirm-no")
                    .click()
                    .await?;
                let _gone = alpha.expect(&prompt).to_be_hidden().await?;
                let _still_empty = alpha.expect(&edit).to_be_unchecked().await?;
                assert_eq!(
                    logged(alpha, LogStream::Command, "GrantUserRights").await?,
                    0,
                    "Cancel sent a grant"
                );

                let _asked_again = edit.click().await?;
                let _grant = confirm_button(alpha, "people-grant-confirm-yes")
                    .click()
                    .await?;
                let all = FriendRights::CAN_SEE_ONLINE
                    | FriendRights::CAN_SEE_ON_MAP
                    | FriendRights::CAN_MODIFY_OBJECTS;
                let _granted =
                    grid_hears(&mut heard, "edit grant", |event| grants(event, friend, all))
                        .await?;
                let _ticked = alpha.expect(&edit).to_be_checked().await?;

                // Taking a right back needs no confirmation.
                let _unmapped = map.click().await?;
                let _map_off = grid_hears(&mut heard, "map revoke", |event| {
                    grants(
                        event,
                        friend,
                        FriendRights::CAN_SEE_ONLINE | FriendRights::CAN_MODIFY_OBJECTS,
                    )
                })
                .await?;
                let _revoked = edit.click().await?;
                let _edit_off = grid_hears(&mut heard, "edit revoke", |event| {
                    grants(event, friend, FriendRights::CAN_SEE_ONLINE)
                })
                .await?;
                let _empty_again = alpha.expect(&edit).to_be_unchecked().await?;
                assert!(
                    !alpha
                        .ui()
                        .test_id("people-grant-confirm-text")
                        .is_visible()
                        .await?,
                    "a revoke asked for a confirmation"
                );

                // The friend lets Alpha edit their objects.
                let received = right(&window, FRIEND, "people-right-received-edit");
                let _not_yet = alpha.expect(&received).to_be_unchecked().await?;
                let agent = stage.agent("Alpha").await?;
                let now = agent.now();
                let entry = UserRightsEntry {
                    agent: FriendKey::from(stage.agent_id("Alpha")?.uuid()),
                    rights: FriendRights(all),
                };
                agent
                    .with_sim(|sim| sim.send_change_user_rights(friend, &[entry], now))
                    .await
                    .map_err(|error| format!("pushing the friend's grant: {error}"))?;
                let _received = alpha
                    .expect(&received)
                    .timeout(WAIT)
                    .to_be_checked()
                    .await?;
                let _read_only = alpha.expect(&received).to_be_disabled().await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- Busy replies ------------------------------------------------------

    /// The Comm menu's path to the Unavailable (do-not-disturb) mode.
    const UNAVAILABLE: [&str; 3] = [
        "menu-bar-comm",
        "menu-bar-online-status",
        "menu-bar-unavailable",
    ];

    /// The resident who IMs: nobody the grid knows.
    const STRANGER: u128 = 0x5752_0000_0000_0000_0000_0000_0000_0001;

    /// The stranger's name, as an IM carries it.
    const STRANGER_NAME: &str = "Stranger Resident";

    /// The friend filed in the contact set.
    const MEMBER: &str = "Member";

    /// The contact set the busy-reply test makes.
    const SET: &str = "Close Friends";

    /// The set's own Unavailable reply.
    const SET_REPLY: &str = "Busy, but not for you for long.";

    /// The stranger's conversation in the conversations probe.
    fn stranger_conversation() -> serde_json::Value {
        json!({ "kind": "direct", "id": Uuid::from_u128(STRANGER) })
    }

    /// **Busy replies**: while Unavailable, a stranger's IM is answered once;
    /// once the conversation is closed the next IM is answered again; and a
    /// friend filed in a contact set with its own reply hears that reply, not
    /// the general one.
    #[test]
    fn an_unavailable_reply_rearms_on_close_and_a_set_answers_with_its_own() -> Result<(), TestError>
    {
        stage("busy_replies", &["Alpha"])
            .needs(Need::GridControl)
            .configure_grid(|grid| {
                grid.account(AccountConfig::new(FIRST_NAME, MEMBER, "password"))
                    .friends((FIRST_NAME, "Alpha"), (FIRST_NAME, MEMBER))
            })
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let stranger = AgentKey::from(Uuid::from_u128(STRANGER));
                let _on = alpha.menu_path(&UNAVAILABLE).await?;
                let mut heard = stage.agent("Alpha").await?.events();

                deliver(
                    stage,
                    "Alpha",
                    &im_from(stage, "Alpha", stranger, STRANGER_NAME, "Hello?")?,
                )
                .await?;
                let reply = grid_hears(&mut heard, "first busy reply", |event| {
                    busy_reply_to(event, stranger).is_some()
                })
                .await?;
                let general = busy_reply_to(&reply, stranger).unwrap_or_default();

                // The conversation is open now; close it, and the next IM is
                // a new conversation, answered again.
                let window = alpha.open_floater("conversations").await?;
                let tab = window.get(Locator::role(Role::Tab).name_containing(STRANGER_NAME));
                let _shown = tab.click().await?;
                let _closed = window
                    .get(Locator::role(Role::Button).name_key("conversations-pane-close-name"))
                    .click()
                    .await?;
                let _gone = alpha.expect(&tab).to_be_detached().await?;
                let replies_before = logged(alpha, LogStream::Command, "AutoResponse").await?;
                deliver(
                    stage,
                    "Alpha",
                    &im_from(stage, "Alpha", stranger, STRANGER_NAME, "Still there?")?,
                )
                .await?;
                let _again = grid_hears(&mut heard, "busy reply after the close", |event| {
                    busy_reply_to(event, stranger).is_some()
                })
                .await?;
                let _arrived = alpha
                    .expect_state(Probe::Conversations)
                    .timeout(WAIT)
                    .to_include(json!([{
                        "conversation": stranger_conversation(),
                        "lines": [{ "text": "Still there?" }],
                    }]))
                    .await?;
                assert_eq!(
                    logged(alpha, LogStream::Command, "AutoResponse").await?,
                    replies_before + 1,
                    "the closed conversation's next IM is answered once"
                );

                // A contact set with its own reply.
                let sets = people_tab(alpha, "people-contact-sets-tab").await?;
                let _asked = sets.button_key("contact-sets-action-new").click().await?;
                let _named = alpha.ui().test_id("toast-input:field").fill(SET).await?;
                let _created = alpha.ui().test_id("toast-button:Create").click().await?;
                file_friend(alpha, stage, MEMBER).await?;
                let sets = people_tab(alpha, "people-contact-sets-tab").await?;
                let chooser = sets.test_id("contact-sets-chooser:combo");
                let _picked = chooser
                    .select_option(Locator::role(Role::ListItem).named(SET))
                    .await?;
                let _configure = sets
                    .button_key("contact-sets-action-configure")
                    .click()
                    .await?;
                let config = alpha.ui().window("contact-set-config");
                config
                    .get(Locator::role(Role::Checkbox).name_key("contact-set-config-reply-busy"))
                    .check()
                    .await?;
                let _typed = config
                    .test_id("contact-set-config-reply-busy-field:field")
                    .fill(SET_REPLY)
                    .await?;
                // The reply commits when the field loses the focus.
                alpha.press("Tab").await?;

                let member = account_id(stage, MEMBER)?;
                deliver(
                    stage,
                    "Alpha",
                    &im_from(stage, "Alpha", member, &account(MEMBER), "Got a minute?")?,
                )
                .await?;
                let reply = grid_hears(&mut heard, "the set's busy reply", |event| {
                    busy_reply_to(event, member).is_some()
                })
                .await?;
                assert_eq!(
                    busy_reply_to(&reply, member).as_deref(),
                    Some(SET_REPLY),
                    "a set member hears the set's reply (the general one is {general:?})"
                );
                Ok(())
            })?;
        Ok(())
    }

    /// File the friend `last` under the one contact set there is, from their
    /// profile: Friends ▸ the row ▸ Profile ▸ Add to Set… ▸ Add.
    async fn file_friend(viewer: &Viewer, stage: &Stage, last: &str) -> Result<(), BodyError> {
        let window = people_tab(viewer, "people-friends-tab").await?;
        let _row = window
            .get(Locator::role(Role::ListItem).name_containing(account(last)))
            .timeout(WAIT)
            .click()
            .await?;
        let _profile = window.button_key("people-action-profile").click().await?;
        let friend = account_id(stage, last)?;
        let profile = viewer
            .ui()
            .window(&format!("avatar-profile#{}", friend.uuid()));
        let _add = profile
            .button_key("profile-add-to-contact-set")
            .click()
            .await?;
        let _filed = viewer
            .ui()
            .window("add-to-contact-set")
            .button_key("add-to-contact-set-add")
            .click()
            .await?;
        let _closed = profile.test_id("floater-button:close").click().await?;
        Ok(())
    }

    // ---- Typing -----------------------------------------------------------

    /// The catalogue NPC standing nearer the login point.
    const NEAR_NPC: &str = "Catalogue Resident";

    /// The catalogue NPC sitting farther away.
    const FAR_NPC: &str = "Seated Resident";

    /// The built-in typing animation (`ANIM_AGENT_TYPE`).
    const TYPE_ANIMATION: Uuid = Uuid::from_u128(0xc541_c47f_e0c0_058b_ad1a_d6ae_3a45_84d9);

    /// Whether `event` is the viewer telling the grid it started typing.
    fn is_typing_start(event: &ServerEvent) -> bool {
        matches!(
            event,
            ServerEvent::Chat {
                chat_type: ChatType::StartTyping,
                ..
            }
        )
    }

    /// Whether `event` is the viewer starting the typing animation.
    fn is_typing_animation(event: &ServerEvent) -> bool {
        matches!(event, ServerEvent::ClientMessage(message)
            if matches!(&**message, AnyMessage::AgentAnimation(animation)
                if animation.animation_list.iter().any(|entry|
                    entry.anim_id == TYPE_ANIMATION && entry.start_anim)))
    }

    /// The radar's floater id.
    const RADAR: &str = "radar";

    /// The radar row of the resident `name`.
    fn radar_row(viewer: &Viewer, name: &str) -> UiLocator {
        viewer
            .ui()
            .window(RADAR)
            .get(Locator::role(Role::ListItem).name_containing(name))
    }

    /// Open the radar, or leave it open: a relogged viewer reopens the
    /// windows it had open, so the World menu's toggle could close it.
    async fn open_radar(viewer: &Viewer) -> Result<(), BodyError> {
        let _shown = viewer.open_floater(RADAR).await?;
        Ok(())
    }

    /// **Typing**: a draft in the chat bar tells the grid the agent is typing
    /// — the indicator and the typing animation — and chirps; clearing it
    /// tells the grid it stopped. A resident the grid says is typing shows a
    /// `T` on the radar and plays the typing animation.
    #[test]
    fn the_own_typing_reaches_the_grid_and_a_neighbours_shows() -> Result<(), TestError> {
        stage("typing", &["Alpha"])
            .region(catalogue()?)
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let mut heard = stage.agent("Alpha").await?.events();
                let bar = alpha.ui().test_id("nearby-chat-bar").role(Role::Textbox);
                let _typed = bar.fill("half a thought").await?;
                // The animation request goes out first, in the same frame.
                let (mut started, mut animated) = (false, false);
                while !(started && animated) {
                    let event = grid_hears(&mut heard, "typing start and animation", |event| {
                        is_typing_start(event) || is_typing_animation(event)
                    })
                    .await?;
                    started |= is_typing_start(&event);
                    animated |= is_typing_animation(&event);
                }
                assert_eq!(
                    logged(alpha, LogStream::Sound, "typing").await?,
                    1,
                    "the typing chirp, once on the rising edge"
                );
                let _cleared = bar.fill("").await?;
                let _stopped = grid_hears(&mut heard, "typing stop", |event| {
                    matches!(
                        event,
                        ServerEvent::Chat {
                            chat_type: ChatType::StopTyping,
                            ..
                        }
                    )
                })
                .await?;
                alpha.press("Escape").await?;

                // The NPC starts typing.
                open_radar(alpha).await?;
                let row = radar_row(alpha, NEAR_NPC);
                let _listed = alpha.expect(&row).timeout(WAIT).to_be_visible().await?;
                let npc = alpha.world().avatar(NEAR_NPC).node().await?;
                let agent = stage.agent("Alpha").await?;
                let now = agent.now();
                let position = npc.position.unwrap_or_default();
                agent
                    .with_sim(|sim| {
                        sim.send_chat_from_simulator(
                            NEAR_NPC,
                            ChatSource::Agent(AgentKey::from(npc.full_id)),
                            npc.full_id,
                            ChatType::StartTyping,
                            1,
                            Vector {
                                x: position[0],
                                y: position[1],
                                z: position[2],
                            },
                            "",
                            now,
                        )?;
                        sim.send_avatar_animation(
                            AgentKey::from(npc.full_id),
                            &[sl_proto::PlayingAnimation {
                                anim_id: TYPE_ANIMATION,
                                sequence_id: 7,
                                source_id: None,
                            }],
                            now,
                        )
                    })
                    .await
                    .map_err(|error| format!("the NPC's typing: {error}"))?;
                let _typing = alpha
                    .expect(&row)
                    .timeout(WAIT)
                    .to_contain_text("T")
                    .await?;
                let npc_id = npc.full_id.to_string();
                let animated = alpha
                    .events_from_start()
                    .read(&[LogStream::Event])
                    .await?
                    .iter()
                    .any(|entry| {
                        entry.kind == "AvatarAnimation"
                            && entry.detail.contains(&npc_id)
                            && entry.detail.contains(&TYPE_ANIMATION.to_string())
                    });
                assert!(animated, "the viewer heard the NPC's typing animation");
                Ok(())
            })?;
        Ok(())
    }

    // ---- Radar ------------------------------------------------------------

    /// Open the radar row menu of the resident `name` and click its entry
    /// `key`.
    ///
    /// The right-click must open the row's menu and nothing else: once the
    /// viewer stands where the radar's rows cover the avatars' name tags (as
    /// after Teleport To), the tag test used to answer before the window did
    /// and an avatar pie opened behind the menu.
    async fn radar_menu(viewer: &Viewer, name: &str, key: &str) -> Result<(), BodyError> {
        let _menu = radar_row(viewer, name).right_click().await?;
        let _picked = viewer
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key(key))
            .click()
            .await?;
        assert_eq!(
            viewer.ui().test_id("pie-menu").count().await?,
            0,
            "a right-click on a radar row opened a pie behind the window"
        );
        Ok(())
    }

    /// Whether the radar row menu of `name` shows the entry `key`; the menu is
    /// closed again after.
    async fn radar_menu_offers(viewer: &Viewer, name: &str, key: &str) -> Result<bool, BodyError> {
        let _menu = radar_row(viewer, name).right_click().await?;
        let entry = viewer
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key(key));
        let _open = viewer
            .expect(
                &viewer
                    .ui()
                    .locator(Locator::role(Role::MenuItem).name_key("menu-radar-view-profile")),
            )
            .to_be_visible()
            .await?;
        let offered = entry.nodes().await?.iter().any(|node| {
            matches!(
                node.visibility,
                NodeVisibility::Visible | NodeVisibility::Covered
            )
        });
        viewer.press("Escape").await?;
        Ok(offered)
    }

    /// What the friendship offer says.
    const OFFER_MESSAGE: &str = "We met at the catalogue.";

    /// **Radar row menu**: Track starts a beacon (the menu then offers to
    /// stop it); Teleport To asks the grid for the resident's spot; Add
    /// Friend asks for a message and offers it with the friendship; Block
    /// mutes them (the menu then offers Unblock); Derender takes them out of
    /// the scene together with what they wear.
    #[test]
    fn the_radar_row_menu_tracks_teleports_befriends_blocks_and_derenders() -> Result<(), TestError>
    {
        stage("radar_menu", &["Alpha"])
            .region(catalogue()?)
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let mut heard = stage.agent("Alpha").await?.events();
                open_radar(alpha).await?;
                let _listed = alpha
                    .expect(&radar_row(alpha, NEAR_NPC))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let npc = alpha.world().avatar(NEAR_NPC).node().await?;

                assert!(
                    !radar_menu_offers(alpha, NEAR_NPC, "menu-radar-stop-tracking").await?,
                    "Stop Tracking before anything is tracked"
                );
                radar_menu(alpha, NEAR_NPC, "menu-radar-start-tracking").await?;
                assert!(
                    radar_menu_offers(alpha, NEAR_NPC, "menu-radar-stop-tracking").await?,
                    "Stop Tracking once the resident is tracked"
                );

                radar_menu(alpha, NEAR_NPC, "menu-radar-teleport-to").await?;
                let position = npc.position.unwrap_or_default();
                let _asked = grid_hears(&mut heard, "teleport to the resident", |event| {
                    matches!(event, ServerEvent::TeleportRequested { position: to, .. }
                        if (to.x() - position[0]).abs() < 4.0
                            && (to.y() - position[1]).abs() < 4.0)
                })
                .await?;
                let _arrived = alpha
                    .expect_state(Probe::Agent)
                    .at("/teleport/state")
                    .timeout(WAIT)
                    .to_equal(json!("idle"))
                    .await?;

                radar_menu(alpha, NEAR_NPC, "menu-radar-add-friend").await?;
                let _message = alpha
                    .ui()
                    .test_id("toast-input:field")
                    .fill(OFFER_MESSAGE)
                    .await?;
                let _offered = alpha.ui().test_id("toast-button:Offer").click().await?;
                let _offer = grid_hears(&mut heard, "friendship offer", |event| {
                    matches!(event, ServerEvent::InstantMessage(im)
                        if im.dialog == ImDialog::FriendshipOffered
                            && im.to_agent_id.uuid() == npc.full_id
                            && im.message == OFFER_MESSAGE)
                })
                .await?;
                radar_menu(alpha, NEAR_NPC, "menu-radar-block").await?;
                let _muted = grid_hears(&mut heard, "mute", |event| {
                    matches!(event, ServerEvent::ClientMessage(message)
                        if matches!(&**message, AnyMessage::UpdateMuteListEntry(entry)
                            if entry.mute_data.mute_id == npc.full_id))
                })
                .await?;
                assert!(
                    radar_menu_offers(alpha, NEAR_NPC, "menu-radar-unblock").await?,
                    "Unblock once the resident is blocked"
                );

                let worn: Vec<u32> = npc.children.clone();
                assert!(!worn.is_empty(), "the catalogue NPC wears an attachment");
                radar_menu(alpha, NEAR_NPC, "menu-radar-derender").await?;
                // The body goes; who it was stays known to the radar and the
                // minimap, as an avatar with no object behind it.
                let unrendered = tokio::time::timeout(WAIT, async {
                    loop {
                        let avatar = alpha.world().avatar(NEAR_NPC).node().await?;
                        if avatar.local_id.is_none() && avatar.children.is_empty() {
                            return Ok::<_, BodyError>(());
                        }
                    }
                })
                .await;
                unrendered.map_err(|_elapsed| "the derendered avatar's body stayed")??;
                for local_id in worn {
                    let _attachment_gone = alpha
                        .expect_world(
                            &alpha.world().locator(
                                sl_automation_proto::WorldLocator::kind(
                                    sl_automation_proto::WorldKind::Object,
                                )
                                .local_id(local_id),
                            ),
                        )
                        .to_be_detached()
                        .await?;
                }
                Ok(())
            })?;
        Ok(())
    }

    /// The Range column's header cell.
    const RANGE_HEADER: &str = "radar:table-header-cell:7";

    /// The region east of the radar test's home.
    const NEIGHBOUR: &str = "Neighbour";

    /// The account who arrives while the radar watches.
    const ARRIVAL: &str = "Arrival";

    /// The account standing in the neighbour region.
    const FARAWAY: &str = "Faraway";

    /// The region-local id the arriving avatar is streamed under.
    const ARRIVAL_LOCAL_ID: u32 = 0x00A2_0001;

    /// A radar alert row of the Alerts tab, by its key.
    fn preference(viewer: &Viewer, key: &str) -> UiLocator {
        viewer
            .ui()
            .window("preferences")
            .test_id(&format!("preferences:row:{key}"))
            .role(Role::Checkbox)
    }

    /// **Radar alerts and sorting**: the radar's sort survives a relog; with
    /// the region-entry report on and its output set to toasts, an avatar
    /// arriving raises a `RadarAlert` toast and the radar sound; and a
    /// resident a neighbour region reports only coarsely is listed with an
    /// approximate position, beside a streamed one's exact position.
    #[test]
    fn the_radar_keeps_its_sort_and_reports_arrivals_and_coarse_neighbours() -> Result<(), TestError>
    {
        let home = catalogue()?;
        let neighbour = RegionConfig {
            name: NEIGHBOUR.to_owned(),
            grid_x: home.grid_x.saturating_add(1),
            ..RegionConfig::default()
        };
        stage("radar_alerts", &["Alpha"])
            .region(home)
            .region(neighbour)
            .needs(Need::GridControl)
            .configure_grid(|grid| {
                grid.account(AccountConfig::new(FIRST_NAME, ARRIVAL, "password"))
                    .account(AccountConfig::new(FIRST_NAME, FARAWAY, "password"))
            })
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                open_radar(&alpha).await?;
                let top = alpha
                    .ui()
                    .window(RADAR)
                    .get(Locator::role(Role::ListItem))
                    .nth(0);
                let _listed = alpha
                    .expect(&radar_row(&alpha, FAR_NPC))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let _nearest_first = alpha.expect(&top).to_contain_text(NEAR_NPC).await?;
                let _sorted = alpha
                    .ui()
                    .window(RADAR)
                    .test_id(RANGE_HEADER)
                    .click()
                    .await?;
                let _farthest_first = alpha.expect(&top).to_contain_text(FAR_NPC).await?;

                let alpha = stage.relog("Alpha").await?;
                open_radar(&alpha).await?;
                let top = alpha
                    .ui()
                    .window(RADAR)
                    .get(Locator::role(Role::ListItem))
                    .nth(0);
                let _relisted = alpha
                    .expect(&radar_row(&alpha, NEAR_NPC))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let _kept = alpha.expect(&top).to_contain_text(FAR_NPC).await?;
                let _exact = alpha
                    .expect(
                        &radar_row(&alpha, NEAR_NPC)
                            .get(Locator::role(Role::Image).name_key("radar-position-exact")),
                    )
                    .to_be_visible()
                    .await?;

                // Toasts for a region arrival.
                let window = alpha.open_floater("preferences").await?;
                let _tab = window
                    .get(Locator::role(Role::Tab).name_key("preferences-tab-alerts"))
                    .click()
                    .await?;
                preference(&alpha, "preferences-row-radar-sim-enter")
                    .check()
                    .await?;
                let _toasts = window
                    .test_id("preferences-row-radar-output:combo")
                    .select_option(
                        Locator::role(Role::ListItem).name_key("preferences-radar-output-toast"),
                    )
                    .await?;
                let _ok = window
                    .test_id("preferences:button:preferences-ok")
                    .click()
                    .await?;
                let _closed = alpha.expect(&window).to_be_hidden().await?;
                let alerts_before = logged(&alpha, LogStream::Sound, "radar_alert").await?;
                let arrival = account_id(stage, ARRIVAL)?;
                let npc = alpha.world().avatar(NEAR_NPC).node().await?;
                let position = npc.position.unwrap_or_default();
                stage
                    .agent("Alpha")
                    .await?
                    .receive_crossing(
                        Vec::new(),
                        vec![NpcFixture::new(
                            RegionLocalObjectId(ARRIVAL_LOCAL_ID),
                            AvatarIdentity::new(arrival, FIRST_NAME, ARRIVAL),
                            Vector {
                                x: position[0] + 2.0,
                                y: position[1],
                                z: position[2],
                            },
                        )],
                    )
                    .await;
                let _toast = alpha
                    .expect_notification()
                    .timeout(WAIT)
                    .to_show("RadarAlert")
                    .await?;
                assert!(
                    logged(&alpha, LogStream::Sound, "radar_alert").await? > alerts_before,
                    "the arrival raised the radar sound"
                );

                // A resident the neighbour region knows only coarsely.
                let faraway = account_id(stage, FARAWAY)?;
                let own = stage.agent_id("Alpha")?;
                let child = stage
                    .grid()?
                    .sessions_in(NEIGHBOUR)
                    .await
                    .into_iter()
                    .find(|session| session.agent_id() == own)
                    .ok_or("Alpha has no child circuit to the neighbour")?;
                let now = child.now();
                child
                    .with_sim(|sim| {
                        sim.send_coarse_location_update(
                            &[CoarseLocation {
                                agent_id: faraway,
                                x: 20,
                                y: 128,
                                z: 24,
                            }],
                            None,
                            None,
                            now,
                        )
                    })
                    .await
                    .map_err(|error| format!("the neighbour's coarse update: {error}"))?;
                let _approximate = alpha
                    .expect(
                        &radar_row(&alpha, &account(FARAWAY))
                            .get(Locator::role(Role::Image).name_key("radar-position-approximate")),
                    )
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- Minimap ----------------------------------------------------------

    /// A pixel of the minimap's resident dot (`MapAvatarColor`, pure green),
    /// as drawn: an unfocused window is translucent, so the dot is the green
    /// over whatever the map shows, not `#00ff00` itself.
    fn avatar_dot(pixel: [u8; 3]) -> bool {
        let [red, green, blue] = pixel;
        green > 150 && green.saturating_sub(red) > 120 && green.saturating_sub(blue) > 120
    }

    /// A pixel of the tracking beacon (`MapTrackColor`, crimson `#ba001f`).
    fn track_mark(pixel: [u8; 3]) -> bool {
        let [red, green, blue] = pixel;
        red > 120 && red.saturating_sub(green) > 100 && red.saturating_sub(blue) > 70
    }

    /// A logical coordinate as a pixel column or row of a scale-1 frame.
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a coordinate on a 1280x720 frame, clamped at zero and rounded"
    )]
    fn to_px(value: f32) -> u32 {
        value.max(0.0).round() as u32
    }

    /// How many pixels `class` accepts the screenshot at `path` has inside
    /// `bounds`.
    fn pixels_of(
        path: &Path,
        bounds: sl_automation_proto::Bounds,
        class: fn([u8; 3]) -> bool,
    ) -> Result<usize, BodyError> {
        let image = image::open(path)
            .map_err(|error| format!("reading {}: {error}", path.display()))?
            .to_rgb8();
        let (left, top) = (to_px(bounds.x), to_px(bounds.y));
        let (right, bottom) = (
            to_px(bounds.x + bounds.width).min(image.width()),
            to_px(bounds.y + bounds.height).min(image.height()),
        );
        Ok((top..bottom)
            .flat_map(|y| (left..right).map(move |x| (x, y)))
            .filter(|&(x, y)| class(image.get_pixel(x, y).0))
            .count())
    }

    /// **Minimap colours**: a nearby resident is a green dot, and tracking
    /// them adds the crimson beacon — two different marks, where they used to
    /// be one colour.
    #[test]
    fn the_minimap_draws_a_resident_and_a_beacon_in_different_colours() -> Result<(), TestError> {
        stage("minimap_colours", &["Alpha"])
            .region(catalogue()?)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let minimap = alpha.open_floater("minimap").await?;
                open_radar(alpha).await?;
                let _listed = alpha
                    .expect(&radar_row(alpha, NEAR_NPC))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let artifacts = stage.artifacts("Alpha")?;
                let before = artifacts.join("minimap-untracked.png");
                let shot = alpha.screenshot(&before, None).await?;
                let bounds = minimap.node().await?.bounds;
                assert!(
                    pixels_of(&shot.path, bounds, avatar_dot)? > 0,
                    "the resident's green dot on the minimap"
                );
                assert_eq!(
                    pixels_of(&shot.path, bounds, track_mark)?,
                    0,
                    "no beacon before anything is tracked"
                );
                radar_menu(alpha, FAR_NPC, "menu-radar-start-tracking").await?;
                let after = artifacts.join("minimap-tracked.png");
                // The beacon is drawn by the next minimap redraw.
                let tracked = tokio::time::timeout(WAIT, async {
                    loop {
                        let shot = alpha.screenshot(&after, None).await?;
                        let bounds = minimap.node().await?.bounds;
                        if pixels_of(&shot.path, bounds, track_mark)? > 0 {
                            return Ok::<_, BodyError>(shot);
                        }
                    }
                })
                .await
                .map_err(|_elapsed| "the tracking beacon never showed on the minimap")??;
                assert!(
                    pixels_of(&tracked.path, minimap.node().await?.bounds, avatar_dot)? > 0,
                    "the resident's dot stays green beside the beacon"
                );
                Ok(())
            })?;
        Ok(())
    }

    // ---- About Land -------------------------------------------------------

    /// The parcel's name after the owner renames it.
    const RENAMED: &str = "Renamed While You Watched";

    /// Open About Land on the parcel the viewer stands on — the stock
    /// parcel — from the World menu, on its General tab. A window is keyed
    /// by its parcel, so it is found by its title (the parcel's name) and
    /// addressed by its test id from then on.
    async fn about_land(viewer: &Viewer) -> Result<UiLocator, BodyError> {
        let _opened = viewer
            .menu_path(&["menu-bar-world", "menu-bar-about-land"])
            .await?;
        let titled = viewer
            .ui()
            .locator(Locator::role(Role::Window).named(STOCK_PARCEL_NAME))
            .timeout(WAIT)
            .node()
            .await?;
        let window = viewer.ui().test_id(
            titled
                .test_id
                .as_deref()
                .ok_or("an About Land window with no test id")?,
        );
        let _general = window
            .get(Locator::role(Role::Tab).name_key("about-land-tab-general"))
            .click()
            .await?;
        Ok(window)
    }

    /// **An open window follows a push**: Alpha watches the parcel's About
    /// Land while Beta, who owns it, renames it; Alpha's window shows the new
    /// name without being reopened.
    #[test]
    fn an_open_about_land_follows_the_owners_rename() -> Result<(), TestError> {
        stage("about_land_push", &["Alpha", "Beta"])
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let beta = &stage.viewer("Beta")?;
                let owner = stage.agent_id("Beta")?;
                // Beta comes to own the parcel, and is told so (the grid
                // record is what a later read returns; the push is what the
                // viewer already holds).
                let agent = stage.agent("Beta").await?;
                let now = agent.now();
                agent
                    .with_world(|world, sim| {
                        let parcel = world
                            .parcel_mut(STOCK_PARCEL_LOCAL_ID)
                            .ok_or_else(|| "the stock parcel is missing".to_owned())?;
                        parcel.owner = OwnerKey::Agent(owner);
                        let record = parcel.clone();
                        sim.send_parcel_properties(&record, now)
                            .map_err(|error| format!("telling Beta it owns the parcel: {error}"))
                    })
                    .await?;
                let watching = about_land(alpha).await?;
                let alpha_name = watching.test_id("about-land-name-field:field");
                let _read_only = alpha.expect(&alpha_name).to_be_disabled().await?;

                let editing = about_land(beta).await?;
                let beta_name = editing.test_id("about-land-name-field:field");
                let _editable = beta
                    .expect(&beta_name)
                    .timeout(WAIT)
                    .to_be_enabled()
                    .await?;
                let _renamed = beta_name.fill(RENAMED).await?;
                // Every editable tab has an Apply; the General tab's is first.
                let _applied = editing
                    .button_key("about-land-apply")
                    .nth(0)
                    .click()
                    .await?;
                let _followed = alpha
                    .expect(&alpha_name)
                    .timeout(WAIT)
                    .to_have_text(RENAMED)
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- A fresh read after a lost write ---------------------------------

    /// The name the test gives its prim, and the one the grid keeps instead.
    const WRITTEN: &str = "Written Here";

    /// The name another editor's write left on the grid.
    const KEPT: &str = "Kept By The Grid";

    /// **A fresh selection reads what the grid holds**: a prim renamed here
    /// and then renamed again by somebody the grid does not tell this viewer
    /// about (OpenSim tells nobody of a rename) shows the grid's name once it
    /// is selected afresh — not the name this viewer wrote last.
    #[test]
    fn a_fresh_selection_shows_the_name_the_grid_kept() -> Result<(), TestError> {
        stage("reselect_rereads", &["Alpha"])
            .needs(Need::Content("the stock scene's unnamed box, to rez on"))
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                alpha.press("Ctrl+B").await?;
                let build = alpha.ui().window("build-tools");
                let _create = build
                    .get(Locator::role(Role::Radio).name_key("build-tool-create"))
                    .click()
                    .await?;
                let _placed = alpha.world().object_named("Object").place().await?;
                let _general = build
                    .get(Locator::role(Role::Tab).name_key("build-tab-general"))
                    .click()
                    .await?;
                let field = build.test_id("build-name:field");
                let mut heard = stage.agent("Alpha").await?.events();
                let _named = field.fill(WRITTEN).await?;
                field.press("Enter").await?;
                let _written = grid_hears(&mut heard, "rename", |event| {
                    matches!(event, ServerEvent::ObjectNameSet { .. })
                })
                .await?;
                let selected = alpha.selection().await?;
                let [prim] = selected.as_slice() else {
                    return Err(format!("the rez left {selected:?} selected").into());
                };
                let (prim_id, local_id) = (prim.full_id, prim.local_id);

                // Somebody else's rename, which nobody tells this viewer of.
                stage
                    .agent("Alpha")
                    .await?
                    .with_world(|world, _sim| {
                        let local = RegionLocalObjectId(local_id);
                        let mut properties = world
                            .properties_of(local)
                            .ok_or_else(|| "the prim is not on the grid".to_owned())?;
                        properties.name = KEPT.to_owned();
                        world
                            .object_mut(local)
                            .ok_or_else(|| "the prim is not on the grid".to_owned())?
                            .properties = Some(properties);
                        Ok::<_, String>(())
                    })
                    .await?;

                // Escape takes the focus out of the field, a second Escape the
                // selection; then the prim is selected afresh.
                alpha.press("Escape").await?;
                alpha.press("Escape").await?;
                let _cleared = alpha
                    .expect_state(Probe::Selection)
                    .to_equal(json!([]))
                    .await?;
                let _reselected = alpha
                    .world()
                    .locator(sl_automation_proto::WorldLocator::full_id(prim_id))
                    .select()
                    .await?;
                let _reread = alpha
                    .expect(&field)
                    .timeout(WAIT)
                    .to_have_text(KEPT)
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- Bakes ------------------------------------------------------------

    /// **The own bake is published**: on a grid that leaves baking to the
    /// viewer, the viewer uploads its baked textures and names them in its
    /// appearance, and the grid stores each one.
    #[test]
    fn the_own_bake_reaches_the_grid() -> Result<(), TestError> {
        stage("bake_publish", &["Alpha"])
            .needs(Need::GridControl)
            .configure_grid(|grid| grid.imitates(ImitatedGrid::OpenSim))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let _published = alpha
                    .expect_state(Probe::Agent)
                    .at("/published_bakes")
                    .timeout(WAIT)
                    .to_be_present()
                    .await?;
                let bakes = alpha.agent().await?.published_bakes;
                let agent = stage.agent("Alpha").await?;
                for bake in &bakes {
                    assert!(
                        agent
                            .stored_asset(sl_proto::AssetKey::from(*bake))
                            .await
                            .is_some(),
                        "the grid stores the published bake {bake}"
                    );
                }
                Ok(())
            })?;
        Ok(())
    }

    // ---- Live grids -------------------------------------------------------

    /// What only a live grid does yet: two viewers seeing each other and
    /// their messages reaching each other.
    const RELAYED: Need = Need::LiveGrid(
        "two viewers that see each other's avatars and reach each other \
         (server-fake-grid-agent-avatars-shared, server-fake-grid-im-relay)",
    );

    /// How long a live grid may take to tell one viewer what another did.
    const LIVE_WAIT: Duration = Duration::from_secs(120);

    /// The name `viewer` shows for viewer `label`'s avatar, once it has
    /// resolved: a live account's display name need not be its legacy name.
    async fn shown_name(stage: &Stage, viewer: &Viewer, label: &str) -> Result<String, BodyError> {
        let agent = stage.agent_id(label)?;
        let avatar = viewer
            .world()
            .locator(sl_automation_proto::WorldLocator::full_id(agent.uuid()));
        let named = tokio::time::timeout(LIVE_WAIT, async {
            loop {
                if let Some(name) = avatar.clone().timeout(LIVE_WAIT).node().await?.name {
                    return Ok::<_, BodyError>(name);
                }
            }
        })
        .await
        .map_err(|_elapsed| format!("{label}'s name never resolved"))??;
        Ok(named)
    }

    /// Bring Beta to Alpha when the grid started them apart (aditi logs each
    /// in where it last was): Alpha offers Beta a teleport from the radar, and
    /// Beta takes it. Two viewers within chat range of each other are left as
    /// they are.
    async fn gather(stage: &Stage, alpha: &Viewer, beta: &Viewer) -> Result<(), BodyError> {
        let (here, there) = (alpha.agent().await?, beta.agent().await?);
        let region = |readout: &sl_automation_proto::AgentReadout| {
            readout
                .region
                .as_ref()
                .and_then(|region| region.name.clone())
        };
        let near = match (here.position, there.position) {
            (Some(a), Some(b)) => (a[0] - b[0]).hypot(a[1] - b[1]) < 10.0,
            _ => false,
        };
        if region(&here) == region(&there) && near {
            return Ok(());
        }
        let beta_name = shown_name(stage, alpha, "Beta").await?;
        offer_teleport_from_radar(alpha, beta, &beta_name).await?;
        take_offered_teleport(beta, region(&here)).await
    }

    /// Alpha offers the avatar its radar lists as `name` a teleport, and the
    /// offer's card reaches `target`.
    async fn offer_teleport_from_radar(
        alpha: &Viewer,
        target: &Viewer,
        name: &str,
    ) -> Result<(), BodyError> {
        open_radar(alpha).await?;
        let _listed = alpha
            .expect(&radar_row(alpha, name))
            .timeout(LIVE_WAIT)
            .to_be_visible()
            .await?;
        radar_menu(alpha, name, "menu-radar-offer-teleport").await?;
        let _closed = alpha
            .ui()
            .window(RADAR)
            .test_id("floater-button:close")
            .click()
            .await?;
        let _offered = target
            .expect(&target.ui().test_id("offer-invite-action:Accept"))
            .timeout(LIVE_WAIT)
            .to_be_visible()
            .await?;
        Ok(())
    }

    /// `viewer` presses Teleport on the offer it is showing and arrives in
    /// the region named `region`, its teleport over.
    async fn take_offered_teleport(
        viewer: &Viewer,
        region: Option<String>,
    ) -> Result<(), BodyError> {
        let _taken = viewer
            .ui()
            .test_id("offer-invite-action:Accept")
            .click()
            .await?;
        let _arrived = viewer
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(LIVE_WAIT)
            .to_equal(json!(region))
            .await?;
        let _settled = viewer
            .expect_state(Probe::Agent)
            .at("/teleport/state")
            .timeout(LIVE_WAIT)
            .to_equal(json!("idle"))
            .await?;
        Ok(())
    }

    /// The friend row of `name` in viewer's Friends tab.
    fn friend_row(window: &UiLocator, name: &str) -> UiLocator {
        window.get(Locator::role(Role::ListItem).name_containing(name))
    }

    /// End a friendship `viewer` holds with `name`, if it holds one: left over
    /// from a run that failed before it parted them.
    async fn unfriend(viewer: &Viewer, name: &str) -> Result<(), BodyError> {
        let window = people_tab(viewer, "people-friends-tab").await?;
        let row = friend_row(&window, name);
        if row.count().await? == 0 {
            return Ok(());
        }
        let _selected = row.click().await?;
        let _removed = window.button_key("people-action-remove").click().await?;
        let _gone = viewer
            .expect(&row)
            .timeout(LIVE_WAIT)
            .to_be_detached()
            .await?;
        Ok(())
    }

    /// Accept every offer card `viewer` shows, one at a time.
    async fn accept_every_offer(viewer: &Viewer) -> Result<(), BodyError> {
        let cards = viewer.ui().test_id("offer-invite-card");
        loop {
            let shown = cards.count().await?;
            if shown == 0 {
                return Ok(());
            }
            let _accepted = cards
                .nth(0)
                .test_id("offer-invite-action:Accept")
                .click()
                .await?;
            let resolved = tokio::time::timeout(LIVE_WAIT, async {
                while cards.count().await? >= shown {}
                Ok::<_, BodyError>(())
            })
            .await;
            resolved.map_err(|_elapsed| "an accepted offer's card stayed")??;
        }
    }

    /// How long the offerer is watched for word of an offer its target
    /// declined. Neither grid sends any; the conformance cases watched as long.
    const DECLINE_WATCH: Duration = Duration::from_secs(10);

    /// How many notifications `viewer` has raised that say anything of a
    /// teleport.
    async fn teleport_notices(viewer: &Viewer) -> Result<usize, BodyError> {
        Ok(viewer
            .notifications()
            .await?
            .iter()
            .filter(|notification| notification.template.contains("Teleport"))
            .count())
    }

    /// **A teleport offer, live**: Alpha offers Beta a teleport from the
    /// radar. Beta's card says how the destination is rated when the grid's
    /// offers do (Second Life's; OpenSim's state no rating). Beta declines,
    /// and Alpha is told nothing — neither grid passes a decline on. Alpha
    /// offers again, Beta presses Teleport, and arrives beside Alpha.
    #[test]
    fn a_live_teleport_offer_is_declined_in_silence_and_then_taken() -> Result<(), TestError> {
        stage("live_lure", &["Alpha", "Beta"])
            .needs(RELAYED)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let beta = &stage.viewer("Beta")?;
                gather(stage, alpha, beta).await?;
                let beta_name = shown_name(stage, alpha, "Beta").await?;
                let home = alpha.agent().await?.region.and_then(|region| region.name);

                offer_teleport_from_radar(alpha, beta, &beta_name).await?;
                let rated = offer_card(beta)
                    .get(Locator::role(Role::Text).name_containing(RATING_LINE))
                    .count()
                    .await?;
                let states_a_rating = std::env::var("SL_E2E_GRID").as_deref() == Ok("aditi");
                assert_eq!(
                    rated,
                    usize::from(states_a_rating),
                    "the card shows a rating exactly when the grid's offers state one"
                );
                let told_before = teleport_notices(alpha).await?;
                answer_offer(beta, "Decline").await?;
                tokio::time::sleep(DECLINE_WATCH).await;
                assert_eq!(
                    teleport_notices(alpha).await?,
                    told_before,
                    "the offerer is told nothing of a declined offer"
                );

                offer_teleport_from_radar(alpha, beta, &beta_name).await?;
                take_offered_teleport(beta, home).await?;
                Ok(())
            })?;
        Ok(())
    }

    /// The prim the two editors fight over.
    const CONTESTED: &str = "E2E Contested Prim";

    /// **Two residents, live**: Alpha offers Beta friendship with a message
    /// from the radar, Beta sees the message and accepts, and each lists the
    /// other. Alpha's edit-objects grant (confirmed) ticks Beta's "You can
    /// edit" box. With it, both rename one prim of Alpha's at once; nothing
    /// arbitrates and nobody is told (OpenSim), but a fresh read in either
    /// viewer returns the one name the grid kept — one of the two writes.
    /// Beta's toggling the map right turns Alpha's box with it; Alpha's
    /// revoke clears Beta's; and parting leaves neither list naming the
    /// other.
    #[test]
    fn two_residents_befriend_trade_rights_and_contest_one_prim() -> Result<(), TestError> {
        stage("live_friendship", &["Alpha", "Beta"])
            .needs(RELAYED)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let beta = &stage.viewer("Beta")?;
                gather(stage, alpha, beta).await?;
                let beta_name = shown_name(stage, alpha, "Beta").await?;
                let alpha_name = shown_name(stage, beta, "Alpha").await?;
                unfriend(alpha, &beta_name).await?;
                let _alpha_gone = beta
                    .expect(&friend_row(
                        &people_tab(beta, "people-friends-tab").await?,
                        &alpha_name,
                    ))
                    .timeout(LIVE_WAIT)
                    .to_be_detached()
                    .await?;

                open_radar(alpha).await?;
                let _seen = alpha
                    .expect(&radar_row(alpha, &beta_name))
                    .timeout(LIVE_WAIT)
                    .to_be_visible()
                    .await?;
                radar_menu(alpha, &beta_name, "menu-radar-add-friend").await?;
                let _message = alpha
                    .ui()
                    .test_id("toast-input:field")
                    .fill(OFFER_MESSAGE)
                    .await?;
                let _offered = alpha.ui().test_id("toast-button:Offer").click().await?;
                // The radar sits over the Conversations window.
                let _radar_closed = alpha
                    .ui()
                    .window(RADAR)
                    .test_id("floater-button:close")
                    .click()
                    .await?;
                // An offer a failed run left behind arrives too, stored
                // offline: every card is Alpha's, so each is accepted.
                let _told = beta
                    .expect(
                        &beta
                            .ui()
                            .locator(Locator::role(Role::Text).name_containing(OFFER_MESSAGE))
                            .nth(0),
                    )
                    .timeout(LIVE_WAIT)
                    .to_be_visible()
                    .await?;
                accept_every_offer(beta).await?;

                let alpha_friends = people_tab(alpha, "people-friends-tab").await?;
                let _listed = alpha
                    .expect(&friend_row(&alpha_friends, &beta_name))
                    .timeout(LIVE_WAIT)
                    .to_be_visible()
                    .await?;
                let beta_friends = people_tab(beta, "people-friends-tab").await?;
                let _listed_back = beta
                    .expect(&friend_row(&beta_friends, &alpha_name))
                    .timeout(LIVE_WAIT)
                    .to_be_visible()
                    .await?;

                let grant = |window: &UiLocator, name: &str, key: &str| {
                    friend_row(window, name).get(Locator::role(Role::Checkbox).name_key(key))
                };
                let _asked = grant(&alpha_friends, &beta_name, "people-right-granted-edit")
                    .click()
                    .await?;
                let _granted = confirm_button(alpha, "people-grant-confirm-yes")
                    .click()
                    .await?;
                let may_edit = grant(&beta_friends, &alpha_name, "people-right-received-edit");
                let _may_edit = beta
                    .expect(&may_edit)
                    .timeout(LIVE_WAIT)
                    .to_be_checked()
                    .await?;

                contest_one_prim(stage, alpha, beta).await?;

                let alpha_friends = people_tab(alpha, "people-friends-tab").await?;
                let beta_friends = people_tab(beta, "people-friends-tab").await?;
                // A new friendship starts with "see online" alone on OpenSim
                // and with the map too on the fake grid: whichever Beta's box
                // says, Beta's click turns it round and Alpha's mirror follows.
                let beta_map = grant(&beta_friends, &alpha_name, "people-right-granted-map");
                let mapped = beta_map.is_checked().await?;
                let _toggled = beta_map.click().await?;
                let alpha_map = alpha.expect(&grant(
                    &alpha_friends,
                    &beta_name,
                    "people-right-received-map",
                ));
                let _followed = if mapped {
                    alpha_map.timeout(LIVE_WAIT).to_be_unchecked().await?
                } else {
                    alpha_map.timeout(LIVE_WAIT).to_be_checked().await?
                };
                let _revoked = grant(&alpha_friends, &beta_name, "people-right-granted-edit")
                    .click()
                    .await?;
                let _edit_gone = beta
                    .expect(&grant(
                        &beta_friends,
                        &alpha_name,
                        "people-right-received-edit",
                    ))
                    .timeout(LIVE_WAIT)
                    .to_be_unchecked()
                    .await?;

                unfriend(alpha, &beta_name).await?;
                let _parted = beta
                    .expect(&friend_row(&beta_friends, &alpha_name))
                    .timeout(LIVE_WAIT)
                    .to_be_detached()
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    /// Alpha rezzes a prim beside itself and names it; Alpha and Beta select
    /// it and rename it at once; a fresh selection in each viewer then shows
    /// the same name, whichever write the grid kept, and Alpha deletes the
    /// prim.
    async fn contest_one_prim(
        stage: &Stage,
        alpha: &Viewer,
        beta: &Viewer,
    ) -> Result<(), BodyError> {
        // The Conversations windows would cover the world.
        for viewer in [alpha, beta] {
            let _closed = viewer
                .ui()
                .window("conversations")
                .test_id("floater-button:close")
                .click()
                .await?;
        }
        alpha.press("Escape").await?;
        alpha.press("Ctrl+B").await?;
        let build = alpha.ui().window("build-tools");
        let _create = build
            .get(Locator::role(Role::Radio).name_key("build-tool-create"))
            .click()
            .await?;
        // A name of this run's own, and a spot: a run that failed half-way
        // leaves its prim on the grid, where the next would aim.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("the clock: {error}"))?
            .as_secs();
        let contested = format!("{CONTESTED} {stamp}");
        let aside = 3.0 + f32::from(u8::try_from(stamp % 6).unwrap_or(0));
        // The Build window takes the left of the screen: rez a few metres to
        // the camera's right of the avatar, and a little ahead of it.
        let readout = alpha.agent().await?;
        let spot = readout.position.ok_or("Alpha has no position")?;
        let eye = readout.camera_eye.ok_or("Alpha has no camera")?;
        let (ahead_x, ahead_y) = (spot[0] - eye[0], spot[1] - eye[1]);
        let length = ahead_x.hypot(ahead_y).max(0.001);
        let (ahead_x, ahead_y) = (ahead_x / length, ahead_y / length);
        let _placed = alpha
            .world()
            .ground(
                stage.home_region(),
                spot[0] + aside * ahead_y + 3.0 * ahead_x,
                spot[1] - aside * ahead_x + 3.0 * ahead_y,
            )
            .timeout(LIVE_WAIT)
            .place()
            .await?;
        let _general = build
            .get(Locator::role(Role::Tab).name_key("build-tab-general"))
            .click()
            .await?;
        let alpha_field = build.test_id("build-name:field");
        let _named = alpha_field.fill(&contested).await?;
        alpha_field.press("Enter").await?;
        // The rez leaves the new prim selected: that is how it is known, not
        // by a name a property reply has to bring.
        let selected = alpha.selection().await?;
        let [prim] = selected.as_slice() else {
            return Err(format!("the rez left {selected:?} selected").into());
        };
        let prim_id = prim.full_id;

        beta.press("Escape").await?;
        beta.press("Ctrl+B").await?;
        let beta_build = beta.ui().window("build-tools");
        // The window opens on Create with nothing selected; a click there
        // would rez rather than select.
        let _move = beta_build
            .get(Locator::role(Role::Radio).name_key("build-tool-move"))
            .click()
            .await?;
        let _picked = beta
            .world()
            .locator(sl_automation_proto::WorldLocator::full_id(prim_id))
            .timeout(LIVE_WAIT)
            .select()
            .await?;
        let _beta_general = beta_build
            .get(Locator::role(Role::Tab).name_key("build-tab-general"))
            .click()
            .await?;
        let beta_field = beta_build.test_id("build-name:field");
        let _beta_sees = beta
            .expect(&beta_field)
            .timeout(LIVE_WAIT)
            .to_have_text(&contested)
            .await?;

        let alpha_says = format!("{contested} (Alpha)");
        let beta_says = format!("{contested} (Beta)");
        let _typed = alpha_field.fill(&alpha_says).await?;
        let _beta_typed = beta_field.fill(&beta_says).await?;
        let (alpha_sent, beta_sent) =
            tokio::join!(alpha_field.press("Enter"), beta_field.press("Enter"));
        alpha_sent?;
        beta_sent?;

        // Nothing tells either window what the grid kept (OpenSim's rename
        // tells nobody, not even the writer): each re-reads by selecting the
        // prim afresh — Escape drops the field's focus, a second Escape the
        // selection — and both then show the one name the grid holds.
        for viewer in [alpha, beta] {
            viewer.press("Escape").await?;
            viewer.press("Escape").await?;
            let _reread = viewer
                .world()
                .locator(sl_automation_proto::WorldLocator::full_id(prim_id))
                .timeout(LIVE_WAIT)
                .select()
                .await?;
        }
        let last_seen = std::sync::Mutex::new((None, None));
        let settled = tokio::time::timeout(LIVE_WAIT, async {
            loop {
                let seen_by_alpha = alpha_field.value().await?;
                let seen_by_beta = beta_field.value().await?;
                if seen_by_alpha == seen_by_beta {
                    return Ok::<_, BodyError>(seen_by_alpha);
                }
                if let Ok(mut last) = last_seen.lock() {
                    *last = (seen_by_alpha, seen_by_beta);
                }
            }
        })
        .await
        .map_err(|_elapsed| {
            format!(
                "the two editors never agreed on the prim's name: (Alpha, Beta) show {:?}",
                last_seen.lock().map(|last| last.clone()).ok()
            )
        })??;
        let survivor = match settled {
            Some(sl_automation_proto::NodeValue::Text(name)) => name,
            other => return Err(format!("the name field reads {other:?}").into()),
        };
        assert!(
            survivor == alpha_says || survivor == beta_says,
            "the survivor is one of the two writes: {survivor:?}"
        );

        beta.press("Ctrl+B").await?;
        // Alpha has the prim selected again, and the world the keyboard.
        alpha.press("Delete").await?;
        let _gone = alpha
            .expect_world(
                &alpha
                    .world()
                    .locator(sl_automation_proto::WorldLocator::full_id(prim_id)),
            )
            .timeout(LIVE_WAIT)
            .to_be_detached()
            .await?;
        alpha.press("Ctrl+B").await?;
        Ok(())
    }

    /// **Typing, live**: Alpha's draft shows on Beta's radar as a `T`, and
    /// Beta hears Alpha's typing animation.
    #[test]
    fn a_resident_sees_another_type() -> Result<(), TestError> {
        stage("live_typing", &["Alpha", "Beta"])
            .needs(RELAYED)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let beta = &stage.viewer("Beta")?;
                gather(stage, alpha, beta).await?;
                let alpha_name = shown_name(stage, beta, "Alpha").await?;
                open_radar(beta).await?;
                let row = radar_row(beta, &alpha_name);
                let _seen = beta.expect(&row).timeout(LIVE_WAIT).to_be_visible().await?;
                let bar = alpha.ui().test_id("nearby-chat-bar").role(Role::Textbox);
                let _typed = bar.fill("a thought in progress").await?;
                let _typing = beta
                    .expect(&row)
                    .timeout(LIVE_WAIT)
                    .to_contain_text("T")
                    .await?;
                let alpha_id = stage.agent_id("Alpha")?.uuid().to_string();
                let animated = tokio::time::timeout(LIVE_WAIT, async {
                    loop {
                        let heard = beta
                            .events_from_start()
                            .read(&[LogStream::Event])
                            .await?
                            .iter()
                            .any(|entry| {
                                entry.kind == "AvatarAnimation"
                                    && entry.detail.contains(&alpha_id)
                                    && entry.detail.contains(&TYPE_ANIMATION.to_string())
                            });
                        if heard {
                            return Ok::<_, BodyError>(());
                        }
                    }
                })
                .await;
                let _cleared = bar.fill("").await?;
                animated.map_err(|_elapsed| "Beta never heard Alpha's typing animation")??;
                Ok(())
            })?;
        Ok(())
    }

    /// **The published bake, seen**: on OpenSim, where each viewer bakes its
    /// own avatar, the bakes Alpha's own viewer drapes it in are the ones
    /// Beta is told about.
    #[test]
    fn another_viewer_sees_the_published_bake() -> Result<(), TestError> {
        stage("live_bake", &["Alpha", "Beta"])
            .needs(RELAYED)
            .needs(Need::OpenSim("the viewer bakes and uploads its own avatar"))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let beta = &stage.viewer("Beta")?;
                let alpha_id = stage.agent_id("Alpha")?.uuid();
                let published = alpha
                    .expect_state(Probe::Agent)
                    .at("/published_bakes")
                    .timeout(LIVE_WAIT)
                    .to_be_present()
                    .await;
                published.map_err(|error| format!("Alpha never published a bake: {error}"))?;
                let published = alpha.agent().await?.published_bakes;
                let seen = beta
                    .world()
                    .locator(sl_automation_proto::WorldLocator::full_id(alpha_id));
                let agreed = tokio::time::timeout(LIVE_WAIT, async {
                    loop {
                        if seen.node().await?.bakes == published {
                            return Ok::<_, BodyError>(());
                        }
                    }
                })
                .await;
                let artifacts = stage.artifacts("Beta")?;
                let _shot = beta
                    .screenshot(&artifacts.join("alpha-as-beta-sees-it.png"), None)
                    .await?;
                agreed.map_err(|_elapsed| {
                    format!("Beta never saw Alpha's published bakes {published:?}")
                })??;
                Ok(())
            })?;
        Ok(())
    }
}
