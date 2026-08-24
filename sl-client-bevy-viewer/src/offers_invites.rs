//! The **offers & invites toast host** (`viewer-dialog-offers-invites`): the
//! accept / decline cards the grid throws at the user over
//! `ImprovedInstantMessage` — an **inventory offer**, a **teleport offer / lure**,
//! a **friendship offer** and a **group-membership invitation**.
//!
//! # What it renders
//!
//! `sl-proto` decodes each of these as an
//! [`SlSessionEvent::InstantMessageReceived`] with a distinguishing
//! [`ImDialog`]. This host reads the event stream and, for each of the four
//! offer / invite dialogs, raises a card into the **shared notification-host
//! channel** ([`crate::notification_host`]) — top-trailing, priority-ordered,
//! overflow-cycled, so it stacks alongside the catalogue toasts and the sibling
//! script-dialog / permission cards:
//!
//! - **Inventory offer** ([`ImDialog::InventoryOffered`] /
//!   [`ImDialog::TaskInventoryOffered`]): "{giver} has given you an item", the
//!   item name, with **Accept** (file it into the type-appropriate folder),
//!   **Decline** (route it to Trash) and **Block** (mute the giver + decline).
//! - **Teleport offer / lure** ([`ImDialog::LureUser`]): "{offerer} has offered
//!   to teleport you", the offer message, with **Teleport** (accept the lure) and
//!   **Decline**.
//! - **Friendship offer** ([`ImDialog::FriendshipOffered`]): "{agent} is offering
//!   to be your friend", any custom message, with **Accept** (file the calling
//!   card) and **Decline**.
//! - **Group invitation** ([`ImDialog::GroupInvitation`]): "{inviter} has invited
//!   you to join a group", the invite message, any membership fee, with **Join**
//!   (accept the invitation) and **Decline**.
//!
//! Each card **sticks** ([`NotificationKind::Alert`]) until the user answers it.
//! The close **×** declines conservatively (never silently accepting), sending
//! the same protocol reply as the Decline button so no offer is left dangling on
//! the simulator. Like the sibling script dialogs a card is **not persisted**
//! across a relog; an offer that was stored-and-forwarded while the agent was
//! offline re-arrives as a fresh offline IM at login (drained by the session), so
//! nothing is lost by keeping the toast session-local.
//!
//! # What never reaches a card
//!
//! Two filters sit in front of the cards. The alerts-tab auto-accept
//! ([`SETTING_AUTO_ACCEPT_INVENTORY`]) files an inventory offer silently, and
//! the standing **auto-reject modes** ([`crate::auto_reject`]) answer a whole
//! class of offer — every teleport offer, every friendship request, every group
//! invitation — with the mode's canned reply plus the ordinary wire decline, so
//! the sender is answered and nothing is left pending on the simulator. Both run
//! ahead of the Do Not Disturb deferral: an offer that is being answered
//! automatically has nothing to defer.
//!
//! # The protocol replies
//!
//! Each button writes the reply [`Command`] the offer's protocol already models:
//! [`Command::AcceptInventoryOffer`] / [`Command::DeclineInventoryOffer`],
//! [`Command::AcceptTeleportLure`] / [`Command::DeclineTeleportLure`],
//! [`Command::AcceptFriendship`] / [`Command::DeclineFriendship`], and
//! [`Command::AcceptGroupInvitation`] / [`Command::DeclineGroupInvitation`]. The
//! two replies that need a destination folder (accept an inventory offer, accept
//! friendship) resolve it from the live [`InventoryModel`] **at click time** —
//! by then the login inventory fetch has completed — falling back to the agent
//! root when the type-specific system folder is absent.
//!
//! Free-text can appear in an offer body (an inventory item name, a lure /
//! friendship / invite message), so this host — like the script dialog and group
//! notice — pairs with a body-linkification follow-up
//! ([[viewer-url-linkification]]); until that lands the body renders as plain
//! text.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use sl_client_bevy::{
    AssetType, Command, FolderType, FriendKey, ImDialog, InstantMessage, InventoryFolderKey,
    InventoryOffer, LureId, MuteType, SlCommand, SlEvent, SlSessionEvent, TransactionId,
};

use crate::i18n::{TransArgs, Translator};
use crate::inventory::InventoryModel;
use crate::inventory_actions::default_folder_type;
use crate::notification_host::{NotificationChannelRoot, ResolveNotification, adopt_toast};
use crate::notifications::{NotificationKind, NotificationManager, NotificationPriority};
use crate::ui::{column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::world_api::RequestBlock;

/// The catalogue-template sentinel an inventory-offer card reports as. Like the
/// sibling bespoke cards these are not real [`crate::notifications::NOTIFICATIONS`]
/// entries — the cards are bespoke — but the shared toast machinery wants a stable
/// name for its history / response bookkeeping. Named for the reference
/// `UserGiveItem` notification.
const INVENTORY_OFFER_TEMPLATE: &str = "UserGiveItem";

/// The account setting for silently accepting inventory offers (the reference
/// `AutoAcceptNewInventory`, surfaced on the Preferences alerts tab; default
/// **off**). While on, an inventory offer is filed into its type folder with
/// no offer card; an offer whose destination cannot be resolved yet (the
/// inventory skeleton still loading) falls back to the card — an offer is
/// never dropped. Lives in the `[notifications]` section with the other
/// notification preferences.
pub(crate) const SETTING_AUTO_ACCEPT_INVENTORY: &str = "AutoAcceptNewInventory";

/// Startup: declare [`SETTING_AUTO_ACCEPT_INVENTORY`] (default off).
fn register_offers_settings(settings: Option<ResMut<crate::settings::ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        &[crate::notifications::NOTIFICATIONS_SECTION],
        SETTING_AUTO_ACCEPT_INVENTORY,
        sl_settings::SettingValue::Bool(false),
        "Silently accept inventory offers into the type-appropriate folder",
    );
}

/// The template sentinel a teleport-offer card reports as, named for the
/// reference `TeleportOffered` notification.
const TELEPORT_OFFER_TEMPLATE: &str = "TeleportOffered";

/// The template sentinel a friendship-offer card reports as, named for the
/// reference `OfferFriendship` notification.
const FRIENDSHIP_OFFER_TEMPLATE: &str = "OfferFriendship";

/// The template sentinel a group-invitation card reports as, named for the
/// reference `JoinGroup` notification.
const GROUP_INVITE_TEMPLATE: &str = "JoinGroup";

/// The element id the inventory-offer gallery specimen and its inert actions
/// report under.
const INVENTORY_OFFER_ELEMENT: &str = "inventory-offer-toast";

/// The element id the teleport-offer gallery specimen reports under.
const TELEPORT_OFFER_ELEMENT: &str = "teleport-offer-toast";

/// The element id the friendship-offer gallery specimen reports under.
const FRIENDSHIP_OFFER_ELEMENT: &str = "friendship-offer-toast";

/// The element id the group-invitation gallery specimen reports under.
const GROUP_INVITE_ELEMENT: &str = "group-invite-toast";

/// The skin class a card wears (`.sk-toast`), so it inherits the shared toast
/// surface styling.
const CARD_CLASS: &str = "sk-toast";

/// The skin class the body text wears (`.sk-toast-text`).
const TEXT_CLASS: &str = "sk-toast-text";

/// The skin class a card button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The close-button glyph (a multiplication sign), matching the reference toast.
const CLOSE_GLYPH: &str = "\u{00d7}";

/// A card's widest allowed width, in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

/// A card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// A card's border width, in logical pixels — the offer accent is painted on it.
const CARD_BORDER: f32 = 2.0;

/// The gap between a card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 6.0;

/// The gap between the action buttons, in logical pixels.
const BUTTON_GAP: f32 = 6.0;

/// The card body / button text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The heading line's text size, in logical pixels.
const HEADING_FONT_SIZE: f32 = 15.0;

/// The width bound for a full-width text line, spanning the card content width
/// less its padding and border — so a wrapped paragraph is the sole inline
/// occupant of a decoration-free box (the `viewer-text-node-padding-measure`
/// constraint).
const FULL_TEXT_MAX_WIDTH: f32 = CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER;

/// A card's fallback background, used when no skin is loaded — the skin's
/// `.sk-toast` (`var(--surface-bg)`) overrides it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// A card's fallback body text colour — the skin's `.sk-toast-text` overrides it.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (the fee line, the item / message detail).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The inventory-offer accent — a green (a gift), distinct from the sibling
/// cards so the four offer kinds read apart.
const GIFT_ACCENT: Color = Color::srgb(0.45, 0.78, 0.52);

/// The teleport-offer accent — an azure (go there).
const LURE_ACCENT: Color = Color::srgb(0.40, 0.72, 0.92);

/// The friendship-offer accent — a rose.
const FRIEND_ACCENT: Color = Color::srgb(0.92, 0.56, 0.74);

/// The group-invitation accent — a gold.
const GROUP_ACCENT: Color = Color::srgb(0.86, 0.72, 0.36);

/// The inventory-offer heading glyph (a wrapped gift).
const GIFT_GLYPH: &str = "\u{1f381}";

/// The teleport-offer heading glyph (a round pushpin — a location).
const LURE_GLYPH: &str = "\u{1f4cd}";

/// The friendship-offer heading glyph (a handshake).
const FRIEND_GLYPH: &str = "\u{1f91d}";

/// The group-invitation heading glyph (two busts — people).
const GROUP_GLYPH: &str = "\u{1f465}";

/// The plugin: drives the offer / invite cards into the shared notification
/// channel.
pub(crate) struct OffersInvitesPlugin;

impl Plugin for OffersInvitesPlugin {
    /// Ingest received offer / invite `ImprovedInstantMessage`s into the shared
    /// toast channel.
    fn build(&self, app: &mut App) {
        app.init_resource::<DeferredOffers>()
            .add_systems(Startup, register_offers_settings)
            .add_systems(Update, ingest_offers_invites);
    }
}

/// Offer / invite cards held back while **Do Not Disturb** is on
/// ([`crate::presence`]), the bespoke-card sibling of the catalogue toast queue
/// in [`crate::notification_host`]: the offer IM is kept verbatim and its card
/// is built for real when the mode is switched off, so an offer is deferred,
/// never dropped. The protocol needs no reply until the user answers, so a held
/// offer stays valid — the same as one the user simply has not clicked yet.
#[derive(Resource, Debug, Default)]
struct DeferredOffers {
    /// The offer / invite IMs held back, oldest first.
    held: Vec<InstantMessage>,
    /// Whether Do Not Disturb was on last frame, so the drain runs on the
    /// falling edge only.
    was_busy: bool,
}

/// Read the event stream; for each received offer / invite IM, build its card and
/// raise it into the shared toast channel (or, for an inventory offer under
/// [`SETTING_AUTO_ACCEPT_INVENTORY`], file it silently).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the event stream, the \
              shared channel + manager, i18n, the auto-accept setting with the inventory model \
              + command writer it files through, the friend and group models the auto-reject \
              policy reads, the do-not-disturb state and its deferral queue, and the commands \
              the cards spawn with"
)]
fn ingest_offers_invites(
    mut events: MessageReader<SlEvent>,
    channel: Option<Res<NotificationChannelRoot>>,
    mut manager: ResMut<NotificationManager>,
    translator: Translator,
    settings: Option<Res<crate::settings::ViewerSettings>>,
    presence: Option<Res<crate::world_api::PresenceState>>,
    friends: Option<Res<crate::world_api::FriendsModel>>,
    groups: Option<Res<crate::world_api::GroupsModel>>,
    mut deferred: ResMut<DeferredOffers>,
    inventory: Res<InventoryModel>,
    mut sl: MessageWriter<SlCommand>,
    mut commands: Commands,
) {
    let Some(channel) = channel else {
        return;
    };
    let auto_accept = settings
        .as_deref()
        .and_then(|settings| {
            settings
                .store()
                .get_bool(SETTING_AUTO_ACCEPT_INVENTORY)
                .ok()
        })
        .unwrap_or(false);
    let policy = crate::auto_reject::RejectPolicy::from_settings(settings.as_deref());
    // Do Not Disturb defers the cards and replays them on the way out, so this
    // frame's work is the fresh offers plus, on the falling edge, the held ones.
    let busy = presence.is_some_and(|presence| presence.is_do_not_disturb());
    let mut pending: Vec<InstantMessage> = if !busy && deferred.was_busy {
        let held = std::mem::take(&mut deferred.held);
        if !held.is_empty() {
            info!(
                "offers: do-not-disturb ended, showing {} deferred offer(s)",
                held.len()
            );
        }
        held
    } else {
        Vec::new()
    };
    deferred.was_busy = busy;
    for event in events.read() {
        let SlSessionEvent::InstantMessageReceived(im) = &event.0 else {
            continue;
        };
        if matches!(
            im.dialog,
            ImDialog::InventoryOffered
                | ImDialog::TaskInventoryOffered
                | ImDialog::LureUser
                | ImDialog::TeleportRequest
                | ImDialog::FriendshipOffered
                | ImDialog::GroupInvitation
        ) {
            pending.push((**im).clone());
        }
    }
    for im in &pending {
        // The standing auto-reject modes (`crate::auto_reject`) answer a whole
        // class of offer before it can raise a card: the canned reply goes out,
        // the offer is declined on the wire, and nothing reaches the screen.
        if let Some(class) = offer_class(im.dialog) {
            let is_friend = friends
                .as_deref()
                .is_some_and(|friends| friends.is_friend(im.from_agent_id));
            let already_member = im.group_invitation().is_some_and(|invite| {
                groups
                    .as_deref()
                    .is_some_and(|groups| groups.is_member(invite.group_id))
            });
            if let Some(kind) =
                crate::auto_reject::reject_for(policy, class, is_friend, already_member)
            {
                auto_reject_offer(im, kind, settings.as_deref(), &mut sl);
                continue;
            }
        }
        match im.dialog {
            ImDialog::InventoryOffered | ImDialog::TaskInventoryOffered => {
                // The alerts-tab auto-accept (the reference
                // `AutoAcceptNewInventory`): file the item silently instead of
                // raising the card. An unresolvable destination (inventory
                // skeleton still loading) falls back to the card so the offer
                // is never dropped. This runs *before* the do-not-disturb
                // deferral: silently filing an item interrupts nobody, so
                // there is nothing to defer.
                if auto_accept
                    && let Some(offer) = im.inventory_offer()
                    && let Some(folder_id) = inventory_destination(&inventory, offer.asset_type)
                {
                    sl.write(SlCommand(Command::AcceptInventoryOffer {
                        offer,
                        folder_id,
                    }));
                    continue;
                }
                if busy {
                    deferred.held.push(im.clone());
                    continue;
                }
                spawn_inventory_offer_card(&mut commands, &channel, &mut manager, &translator, im);
            }
            ImDialog::LureUser | ImDialog::FriendshipOffered | ImDialog::GroupInvitation
                if busy =>
            {
                deferred.held.push(im.clone());
            }
            ImDialog::LureUser => {
                spawn_lure_card(&mut commands, &channel, &mut manager, &translator, im);
            }
            ImDialog::FriendshipOffered => {
                spawn_friendship_card(&mut commands, &channel, &mut manager, &translator, im);
            }
            ImDialog::GroupInvitation => {
                spawn_group_invite_card(&mut commands, &channel, &mut manager, &translator, im);
            }
            _ => {}
        }
    }
}

/// The auto-reject class an offer dialog falls under, or `None` for one no
/// reject mode covers (an inventory offer — the reject family is about *people*
/// reaching for the user's attention, and an item can simply be auto-filed or
/// left on screen).
const fn offer_class(dialog: ImDialog) -> Option<crate::auto_reject::OfferClass> {
    match dialog {
        ImDialog::LureUser | ImDialog::TeleportRequest => {
            Some(crate::auto_reject::OfferClass::Teleport)
        }
        ImDialog::FriendshipOffered => Some(crate::auto_reject::OfferClass::Friendship),
        ImDialog::GroupInvitation => Some(crate::auto_reject::OfferClass::GroupInvite),
        _other => None,
    }
}

/// Answer an auto-rejected offer: send the mode's canned reply (when it has one
/// and the user has not blanked it), then decline it on the wire so nothing is
/// left pending on the simulator, and log what was swallowed — a mode that eats
/// offers invisibly should at least leave a trail.
fn auto_reject_offer(
    im: &InstantMessage,
    kind: crate::auto_reject::RejectKind,
    settings: Option<&crate::settings::ViewerSettings>,
    sl: &mut MessageWriter<SlCommand>,
) {
    info!(
        "offers: auto-rejecting {:?} from {} ({:?})",
        im.dialog, im.from_agent_name, kind
    );
    if let Some(message) = crate::auto_reject::response_text(settings, kind) {
        sl.write(SlCommand(Command::AutoResponse {
            to_agent_id: im.from_agent_id,
            message,
        }));
    }
    if let Some(decline) = decline_command(im, kind) {
        sl.write(SlCommand(decline));
    }
}

/// The wire decline an auto-rejected offer sends — the same reply the user's own
/// Decline button would have written. A teleport *request* has none: it carries
/// no lure to decline, so the canned reply is the whole answer.
fn decline_command(im: &InstantMessage, kind: crate::auto_reject::RejectKind) -> Option<Command> {
    use crate::auto_reject::RejectKind;
    match kind {
        RejectKind::Teleport if im.dialog == ImDialog::LureUser => {
            Some(Command::DeclineTeleportLure {
                from_agent_id: im.from_agent_id,
                lure_id: LureId::from(im.id),
            })
        }
        RejectKind::Teleport => None,
        RejectKind::Friendship => Some(Command::DeclineFriendship(TransactionId::from(im.id))),
        RejectKind::GroupInvite | RejectKind::AlreadyJoinedGroup => {
            let invite = im.group_invitation()?;
            Some(Command::DeclineGroupInvitation {
                group_id: invite.group_id,
                transaction_id: TransactionId::from(invite.transaction_id),
                use_offline_cap: uses_offline_cap(im),
            })
        }
    }
}

/// The reference `use_offline_cap`: an invitation delivered while the agent was
/// offline carries a null session id (transaction id) and must be answered over
/// the offline cap, since a nil id cannot be echoed back over UDP.
const fn uses_offline_cap(im: &InstantMessage) -> bool {
    im.id.is_nil() && im.offline
}

/// The resolved content of one offer / invite card, ready to render — the live
/// path resolves the decoded IM + i18n into this; the gallery specimens build it
/// from literals, so both render through the one [`build_offer_card`] (the
/// registry rule, [`crate::ui_element`]).
struct OfferContent {
    /// The accent painted on the card border and the default (accept) button.
    accent: Color,
    /// The heading glyph shown before the heading text.
    glyph: String,
    /// The heading line ("Inventory Offer", "Group Invitation", …).
    heading: String,
    /// The body paragraphs — the "who / what" lead, then any detail (item name,
    /// offer message, fee), each a width-bounded wrapping box.
    lines: Vec<String>,
    /// The accept button label ("Accept" / "Teleport" / "Join").
    accept_label: String,
    /// The decline button label ("Decline").
    decline_label: String,
    /// The Block (mute) button label, or `None` when the card omits Block.
    block_label: Option<String>,
}

/// The entities [`build_offer_card`] produced that a caller wires: the card root,
/// the accept / decline boxes, the optional Block box, and the close box.
struct OfferCard {
    /// The card root node (left with no parent — the caller adopts it).
    root: Entity,
    /// The accept button box.
    accept: Entity,
    /// The decline button box.
    decline: Entity,
    /// The Block (mute) button box, when the card carries one.
    block: Option<Entity>,
    /// The close (×) button box.
    close: Entity,
}

/// Build an offer / invite card's node tree from resolved [`OfferContent`],
/// returning the entities a caller wires. The **root is left with no parent**:
/// the live host adopts it into the shared toast channel via [`adopt_toast`], the
/// gallery specimen parents it under its cell.
fn build_offer_card(commands: &mut Commands, content: &OfferContent) -> OfferCard {
    let root = commands
        .spawn((
            Node {
                max_width: Val::Px(CARD_MAX_WIDTH),
                padding: UiRect::all(Val::Px(CARD_PADDING)),
                border: UiRect::all(Val::Px(CARD_BORDER)),
                ..column(Val::Px(CARD_ROW_GAP))
            },
            BackgroundColor(CARD_BACKGROUND),
            BorderColor::all(content.accent),
            ClassList::new_with_classes([CARD_CLASS]),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("offer-invite-card"),
        ))
        .id();

    // Close (×), top-trailing — the early-dismiss (conservative decline)
    // affordance.
    let close = spawn_close_button(commands, root);

    // The heading row: the accent glyph then the heading text.
    let heading_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..row(Val::Px(6.0))
            },
            Pickable::IGNORE,
            Name::new("offer-invite-heading"),
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Text::new(content.glyph.clone()),
        UiFont::Sans.at(HEADING_FONT_SIZE),
        TextColor(content.accent),
        Pickable::IGNORE,
        ChildOf(heading_row),
    ));
    commands.spawn((
        Text::new(content.heading.clone()),
        UiFont::Sans.at(HEADING_FONT_SIZE),
        TextColor(TEXT_COLOR),
        ClassList::new_with_classes([TEXT_CLASS]),
        Pickable::IGNORE,
        ChildOf(heading_row),
    ));

    // The body paragraphs — the lead in primary text, any following detail dim —
    // each a width-bounded box so a long line wraps within the card.
    for (index, line) in content.lines.iter().enumerate() {
        let color = if index == 0 {
            TEXT_COLOR
        } else {
            DIM_TEXT_COLOR
        };
        spawn_bounded_text(commands, root, line, FONT_SIZE, color);
    }

    // The bottom action row: accept (the accent default), decline, then the
    // optional Block, trailing-aligned.
    let action_row = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(BUTTON_GAP),
                justify_content: JustifyContent::End,
                ..row(Val::Px(BUTTON_GAP))
            },
            Name::new("offer-invite-actions"),
            ChildOf(root),
        ))
        .id();
    let accept = spawn_action_button(
        commands,
        action_row,
        &content.accept_label,
        content.accent,
        1,
    );
    let decline = spawn_action_button(
        commands,
        action_row,
        &content.decline_label,
        BUTTON_BORDER,
        2,
    );
    let block = content
        .block_label
        .as_ref()
        .map(|label| spawn_action_button(commands, action_row, label, BUTTON_BORDER, 3));

    OfferCard {
        root,
        accept,
        decline,
        block,
        close,
    }
}

/// Adopt a built card into the shared toast channel as a sticky
/// [`Alert`](NotificationKind::Alert), recording a history line, and return its
/// root for observer wiring. Shared by all four kinds.
fn adopt_offer_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    card: &OfferCard,
    template: &'static str,
    history: String,
) {
    adopt_toast(
        commands,
        manager,
        channel,
        card.root,
        NotificationKind::Alert,
        NotificationPriority::Normal,
        template,
        None,
        history,
    );
}

/// Build and wire an **inventory-offer** card. Accept files the item into the
/// type-appropriate system folder (agent root when there is none), Decline routes
/// it to Trash, and Block mutes the giver and declines. Accept / Decline resolve
/// their destination folder from the live [`InventoryModel`] at click time.
fn spawn_inventory_offer_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    translator: &Translator,
    im: &InstantMessage,
) {
    let Some(offer) = im.inventory_offer() else {
        return;
    };
    let lead = translator.format(
        "offer-inventory-from",
        &TransArgs::new().text("name", &im.from_agent_name),
    );
    let content = OfferContent {
        accent: GIFT_ACCENT,
        glyph: GIFT_GLYPH.to_owned(),
        heading: translator.get("offer-inventory-heading"),
        lines: vec![lead.clone(), format!("\u{201c}{}\u{201d}", im.message)],
        accept_label: translator.get("offer-button-accept"),
        decline_label: translator.get("offer-button-decline"),
        block_label: Some(translator.get("offer-button-block")),
    };
    let card = build_offer_card(commands, &content);
    adopt_offer_card(
        commands,
        channel,
        manager,
        &card,
        INVENTORY_OFFER_TEMPLATE,
        lead,
    );

    let root = card.root;
    let giver = offer.from_agent_id;
    let giver_name = im.from_agent_name.clone();

    // Accept: resolve the destination folder from the live inventory and file the
    // item into it (agent root when the type folder is absent).
    commands.entity(card.accept).observe(
        move |_activate: On<Activate>,
              inventory: Res<InventoryModel>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            if let Some(folder_id) = inventory_destination(&inventory, offer.asset_type) {
                sl.write(SlCommand(Command::AcceptInventoryOffer {
                    offer,
                    folder_id,
                }));
            }
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Decline: route the item to Trash.
    commands.entity(card.decline).observe(
        move |_activate: On<Activate>,
              inventory: Res<InventoryModel>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            decline_inventory(&inventory, &offer, &mut sl);
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Block: decline the item and mute the giver (the sending agent, or the owner
    // of the sending object for a task offer).
    if let Some(block) = card.block {
        commands.entity(block).observe(
            move |_activate: On<Activate>,
                  inventory: Res<InventoryModel>,
                  mut sl: MessageWriter<SlCommand>,
                  mut blocks: MessageWriter<RequestBlock>,
                  mut resolves: MessageWriter<ResolveNotification>| {
                decline_inventory(&inventory, &offer, &mut sl);
                blocks.write(RequestBlock::new(
                    giver.uuid(),
                    giver_name.clone(),
                    MuteType::Agent,
                ));
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }

    // Close ×: conservative decline (route to Trash), never a silent accept.
    commands.entity(card.close).observe(
        move |_activate: On<Activate>,
              inventory: Res<InventoryModel>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            decline_inventory(&inventory, &offer, &mut sl);
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );
}

/// The system folder a freshly accepted inventory offer of a given asset type is
/// filed into — the type-appropriate folder, or the agent root when the asset
/// type has no same-named system folder or the type folder has not loaded.
fn inventory_destination(
    inventory: &InventoryModel,
    asset_type: AssetType,
) -> Option<InventoryFolderKey> {
    let folder_type = default_folder_type(asset_type);
    let typed = if matches!(folder_type, FolderType::None) {
        None
    } else {
        inventory.folder_by_type(folder_type)
    };
    typed.or_else(|| inventory.folder_by_type(FolderType::RootInventory))
}

/// Write the decline reply for an inventory offer, routing the item to Trash (the
/// agent root when Trash has not loaded). A missing destination drops the reply
/// rather than sending a nil folder.
fn decline_inventory(
    inventory: &InventoryModel,
    offer: &InventoryOffer,
    sl: &mut MessageWriter<SlCommand>,
) {
    let trash = inventory
        .folder_by_type(FolderType::Trash)
        .or_else(|| inventory.folder_by_type(FolderType::RootInventory));
    if let Some(trash_folder_id) = trash {
        sl.write(SlCommand(Command::DeclineInventoryOffer {
            offer: *offer,
            trash_folder_id,
        }));
    }
}

/// Build and wire a **teleport-offer / lure** card. Teleport accepts the lure
/// (teleporting to the offer's location), Decline sends the lure-declined reply.
fn spawn_lure_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    translator: &Translator,
    im: &InstantMessage,
) {
    let from_agent_id = im.from_agent_id;
    let lure_id = LureId::from(im.id);
    let lead = translator.format(
        "offer-teleport-from",
        &TransArgs::new().text("name", &im.from_agent_name),
    );
    let mut lines = vec![lead.clone()];
    if !im.message.is_empty() {
        lines.push(im.message.clone());
    }
    let content = OfferContent {
        accent: LURE_ACCENT,
        glyph: LURE_GLYPH.to_owned(),
        heading: translator.get("offer-teleport-heading"),
        lines,
        accept_label: translator.get("offer-button-teleport"),
        decline_label: translator.get("offer-button-decline"),
        block_label: None,
    };
    let card = build_offer_card(commands, &content);
    adopt_offer_card(
        commands,
        channel,
        manager,
        &card,
        TELEPORT_OFFER_TEMPLATE,
        lead,
    );

    let root = card.root;

    // Accept: teleport to the offered location.
    commands.entity(card.accept).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            sl.write(SlCommand(Command::AcceptTeleportLure { lure_id }));
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Decline (and the conservative close ×): send the lure-declined reply.
    for button in [Some(card.decline), Some(card.close)].into_iter().flatten() {
        commands.entity(button).observe(
            move |_activate: On<Activate>,
                  mut sl: MessageWriter<SlCommand>,
                  mut resolves: MessageWriter<ResolveNotification>| {
                sl.write(SlCommand(Command::DeclineTeleportLure {
                    from_agent_id,
                    lure_id,
                }));
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }
}

/// Build and wire a **friendship-offer** card. Accept adds the friend (filing the
/// calling card resolved from the live inventory), Decline sends the
/// friendship-declined reply.
fn spawn_friendship_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    translator: &Translator,
    im: &InstantMessage,
) {
    let transaction_id = TransactionId::from(im.id);
    let lead = translator.format(
        "offer-friendship-from",
        &TransArgs::new().text("name", &im.from_agent_name),
    );
    let mut lines = vec![lead.clone()];
    if !im.message.is_empty() {
        lines.push(im.message.clone());
    }
    let content = OfferContent {
        accent: FRIEND_ACCENT,
        glyph: FRIEND_GLYPH.to_owned(),
        heading: translator.get("offer-friendship-heading"),
        lines,
        accept_label: translator.get("offer-button-accept"),
        decline_label: translator.get("offer-button-decline"),
        block_label: None,
    };
    let card = build_offer_card(commands, &content);
    adopt_offer_card(
        commands,
        channel,
        manager,
        &card,
        FRIENDSHIP_OFFER_TEMPLATE,
        lead,
    );

    let root = card.root;
    let friend_id = FriendKey::from(im.from_agent_id.uuid());

    // Accept: resolve the calling-card folder from the live inventory and add the
    // friend (agent root when there is no Calling Cards folder).
    commands.entity(card.accept).observe(
        move |_activate: On<Activate>,
              inventory: Res<InventoryModel>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            if let Some(calling_card_folder) = inventory
                .folder_by_type(FolderType::CallingCard)
                .or_else(|| inventory.folder_by_type(FolderType::RootInventory))
            {
                sl.write(SlCommand(Command::AcceptFriendship {
                    transaction_id,
                    friend_id,
                    calling_card_folder,
                }));
            }
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Decline (and the conservative close ×): send the friendship-declined reply.
    for button in [Some(card.decline), Some(card.close)].into_iter().flatten() {
        commands.entity(button).observe(
            move |_activate: On<Activate>,
                  mut sl: MessageWriter<SlCommand>,
                  mut resolves: MessageWriter<ResolveNotification>| {
                sl.write(SlCommand(Command::DeclineFriendship(transaction_id)));
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }
}

/// Build and wire a **group-membership invitation** card. Join accepts the
/// invitation (the simulator enrolls the agent and charges any fee), Decline
/// sends the invitation-declined reply.
fn spawn_group_invite_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    translator: &Translator,
    im: &InstantMessage,
) {
    let Some(invite) = im.group_invitation() else {
        return;
    };
    let lead = translator.format(
        "offer-group-from",
        &TransArgs::new().text("name", &invite.inviter_name),
    );
    let mut lines = vec![lead.clone()];
    if !invite.message.is_empty() {
        lines.push(invite.message.clone());
    }
    if invite.membership_fee > 0 {
        lines.push(translator.format(
            "offer-group-fee",
            &TransArgs::new().text("fee", &invite.membership_fee.to_string()),
        ));
    }
    let content = OfferContent {
        accent: GROUP_ACCENT,
        glyph: GROUP_GLYPH.to_owned(),
        heading: translator.get("offer-group-heading"),
        lines,
        accept_label: translator.get("offer-button-join"),
        decline_label: translator.get("offer-button-decline"),
        block_label: None,
    };
    let card = build_offer_card(commands, &content);
    adopt_offer_card(
        commands,
        channel,
        manager,
        &card,
        GROUP_INVITE_TEMPLATE,
        lead,
    );

    let root = card.root;
    let group_id = invite.group_id;
    let transaction_id = TransactionId::from(invite.transaction_id);
    let use_offline_cap = uses_offline_cap(im);

    // Accept: join the group.
    commands.entity(card.accept).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            sl.write(SlCommand(Command::AcceptGroupInvitation {
                group_id,
                transaction_id,
                use_offline_cap,
            }));
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Decline (and the conservative close ×): send the invitation-declined reply.
    for button in [Some(card.decline), Some(card.close)].into_iter().flatten() {
        commands.entity(button).observe(
            move |_activate: On<Activate>,
                  mut sl: MessageWriter<SlCommand>,
                  mut resolves: MessageWriter<ResolveNotification>| {
                sl.write(SlCommand(Command::DeclineGroupInvitation {
                    group_id,
                    transaction_id,
                    use_offline_cap,
                }));
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }
}

/// Spawn one bottom-row action button (accept / decline / Block), bordered in the
/// given accent (the accept default wears the card accent, the rest the neutral
/// button border). Returns the clickable box for the caller to wire onto.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    border: Color,
    tab: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(border),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new(format!("offer-invite-action:{label}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(label.to_owned()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
        ))
        .id()
}

/// Spawn the close (×) button in a top-trailing row, returning its box for the
/// caller to wire onto.
fn spawn_close_button(commands: &mut Commands, card: Entity) -> Entity {
    let close_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::End,
                ..row(Val::ZERO)
            },
            Name::new("offer-invite-close-row"),
            ChildOf(card),
        ))
        .id();
    commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                ..default()
            },
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new("offer-invite-close"),
            ChildOf(close_row),
        ))
        .with_child((
            Text::new(CLOSE_GLYPH),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
        ))
        .id()
}

/// A width-bounded text line: the caller's text as the sole child of a
/// decoration-free box, so it wraps within the card (the
/// `viewer-text-node-padding-measure` constraint). An empty string spawns nothing.
fn spawn_bounded_text(
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    font_size: f32,
    color: Color,
) {
    if text.is_empty() {
        return;
    }
    let box_entity = commands
        .spawn((
            Node {
                max_width: Val::Px(FULL_TEXT_MAX_WIDTH),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(text.to_owned()),
        UiFont::Sans.at(font_size),
        TextColor(color),
        ClassList::new_with_classes([TEXT_CLASS]),
        Pickable::IGNORE,
        ChildOf(box_entity),
    ));
}

/// The gallery / `ui_test` specimen (inventory offer): a static agent give, so the
/// heading / lead / item / action layout is swept login-free (a live card needs
/// another agent to make the offer). Registered in [`crate::ui_element::ELEMENTS`];
/// its buttons report an inert [`UiAction`].
pub(crate) fn spawn_inventory_offer_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = OfferContent {
        accent: GIFT_ACCENT,
        glyph: GIFT_GLYPH.to_owned(),
        heading: cx.text("Inventory Offer"),
        lines: vec![
            cx.text("Giver Resident has given you an item:"),
            cx.text("\u{201c}Welcome Gift\u{201d}"),
        ],
        accept_label: cx.text("Accept"),
        decline_label: cx.text("Decline"),
        block_label: Some(cx.text("Block")),
    };
    let card = build_offer_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, INVENTORY_OFFER_ELEMENT);
    card.root
}

/// The gallery / `ui_test` specimen (teleport offer).
pub(crate) fn spawn_teleport_offer_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = OfferContent {
        accent: LURE_ACCENT,
        glyph: LURE_GLYPH.to_owned(),
        heading: cx.text("Teleport Offer"),
        lines: vec![
            cx.text("Guide Resident has offered to teleport you to their location:"),
            cx.text("Come see the new build!"),
        ],
        accept_label: cx.text("Teleport"),
        decline_label: cx.text("Decline"),
        block_label: None,
    };
    let card = build_offer_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, TELEPORT_OFFER_ELEMENT);
    card.root
}

/// The gallery / `ui_test` specimen (friendship offer).
pub(crate) fn spawn_friendship_offer_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = OfferContent {
        accent: FRIEND_ACCENT,
        glyph: FRIEND_GLYPH.to_owned(),
        heading: cx.text("Friendship Offer"),
        lines: vec![
            cx.text("Neighbour Resident is offering to be your friend."),
            cx.text("We met at the sandbox — let's stay in touch!"),
        ],
        accept_label: cx.text("Accept"),
        decline_label: cx.text("Decline"),
        block_label: None,
    };
    let card = build_offer_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, FRIENDSHIP_OFFER_ELEMENT);
    card.root
}

/// The gallery / `ui_test` specimen (group invitation), with a membership fee.
pub(crate) fn spawn_group_invite_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = OfferContent {
        accent: GROUP_ACCENT,
        glyph: GROUP_GLYPH.to_owned(),
        heading: cx.text("Group Invitation"),
        lines: vec![
            cx.text("Officer Resident has invited you to join a group:"),
            cx.text("Explorers of the Grid"),
            cx.text("There is a fee of L$ 25 to join this group."),
        ],
        accept_label: cx.text("Join"),
        decline_label: cx.text("Decline"),
        block_label: None,
    };
    let card = build_offer_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, GROUP_INVITE_ELEMENT);
    card.root
}

/// Wire a specimen card's buttons to inert [`UiAction`]s (the registry rule: a
/// specimen reaches no session), keyed by the given element id.
fn wire_specimen_actions(commands: &mut Commands, card: &OfferCard, element: &'static str) {
    for (button, action) in [
        (Some(card.accept), "accept"),
        (Some(card.decline), "decline"),
        (card.block, "block"),
        (Some(card.close), "close"),
    ] {
        let Some(button) = button else {
            continue;
        };
        commands.entity(button).observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction { element, action });
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{decline_command, offer_class, uses_offline_cap};
    use crate::auto_reject::{OfferClass, RejectKind};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, Command, GroupKey, ImDialog, InstantMessage, RegionCoordinates, TransactionId,
        Uuid,
    };

    /// An offer IM of the given dialog, from `from`, carrying `id` as its
    /// dialog-dependent id (a lure id / transaction id) — the shape the reject
    /// path reads.
    fn offer(dialog: ImDialog, from: Uuid, id: Uuid) -> InstantMessage {
        InstantMessage {
            from_agent_id: AgentKey::from(from),
            from_agent_name: "Someone Resident".to_owned(),
            to_agent_id: AgentKey::from(Uuid::from_u128(0xffff)),
            dialog,
            from_group: false,
            region_id: None,
            position: RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id,
            parent_estate_id: 0,
            message: "come over".to_owned(),
            binary_bucket: Vec::new(),
        }
    }

    /// Every offer dialog a reject mode covers maps to its class; an inventory
    /// offer maps to none, so the modes never touch it.
    #[test]
    fn only_the_covered_dialogs_have_a_class() {
        assert_eq!(offer_class(ImDialog::LureUser), Some(OfferClass::Teleport));
        assert_eq!(
            offer_class(ImDialog::TeleportRequest),
            Some(OfferClass::Teleport)
        );
        assert_eq!(
            offer_class(ImDialog::FriendshipOffered),
            Some(OfferClass::Friendship)
        );
        assert_eq!(
            offer_class(ImDialog::GroupInvitation),
            Some(OfferClass::GroupInvite)
        );
        assert_eq!(offer_class(ImDialog::InventoryOffered), None);
        assert_eq!(offer_class(ImDialog::Message), None);
    }

    /// A rejected lure is declined with its own lure id, so the offerer's
    /// pending teleport is actually cleared rather than left hanging.
    #[test]
    fn a_rejected_lure_is_declined_by_id() {
        let from = Uuid::from_u128(0x11);
        let lure = Uuid::from_u128(0x22);
        let command = decline_command(&offer(ImDialog::LureUser, from, lure), RejectKind::Teleport);
        assert!(
            matches!(
                command,
                Some(Command::DeclineTeleportLure {
                    from_agent_id,
                    lure_id,
                }) if from_agent_id == AgentKey::from(from) && lure_id.get() == lure
            ),
            "expected the lure decline for this offerer and lure id"
        );
    }

    /// A teleport *request* carries no lure, so the canned reply is the whole
    /// answer — there is nothing to decline.
    #[test]
    fn a_rejected_teleport_request_declines_nothing() {
        let im = offer(
            ImDialog::TeleportRequest,
            Uuid::from_u128(0x11),
            Uuid::from_u128(0x22),
        );
        assert!(
            decline_command(&im, RejectKind::Teleport).is_none(),
            "a teleport request carries no lure to decline"
        );
    }

    /// A rejected friendship request is declined under the offer's transaction
    /// id.
    #[test]
    fn a_rejected_friendship_is_declined_by_transaction() {
        let transaction = Uuid::from_u128(0x33);
        let im = offer(
            ImDialog::FriendshipOffered,
            Uuid::from_u128(0x11),
            transaction,
        );
        assert!(
            matches!(
                decline_command(&im, RejectKind::Friendship),
                Some(Command::DeclineFriendship(id)) if id == TransactionId::from(transaction)
            ),
            "expected the friendship decline under the offer's transaction id"
        );
    }

    /// A rejected group invitation is declined for the inviting group, and an
    /// invitation that arrived while offline is answered over the offline cap
    /// (its nil transaction id cannot be echoed back over UDP).
    #[test]
    fn a_rejected_group_invite_is_declined_for_its_group() {
        let group = Uuid::from_u128(0x44);
        let im = offer(ImDialog::GroupInvitation, group, Uuid::from_u128(0x55));
        assert!(
            matches!(
                decline_command(&im, RejectKind::GroupInvite),
                Some(Command::DeclineGroupInvitation {
                    group_id,
                    transaction_id,
                    use_offline_cap,
                }) if group_id == GroupKey::from(group)
                    && transaction_id == TransactionId::from(Uuid::from_u128(0x55))
                    && !use_offline_cap
            ),
            "expected the invitation declined for its group, over UDP"
        );
        // The same invitation, stored and forwarded: nil id + offline.
        let mut offline = offer(ImDialog::GroupInvitation, group, Uuid::nil());
        offline.offline = true;
        assert!(uses_offline_cap(&offline));
        assert!(
            matches!(
                decline_command(&offline, RejectKind::AlreadyJoinedGroup),
                Some(Command::DeclineGroupInvitation {
                    group_id,
                    use_offline_cap,
                    ..
                }) if group_id == GroupKey::from(group) && use_offline_cap
            ),
            "an offline invitation is answered over the cap"
        );
    }
}
