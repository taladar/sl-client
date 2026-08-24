//! The **group-notice toast host** (`viewer-group-notice-display`): the panel a
//! received group notice pops, mirroring the reference `LLToastGroupNotifyPanel`.
//!
//! # What it renders
//!
//! When a group posts a notice, every member with notices enabled receives it as
//! an `ImDialog::GroupNotice` instant message. This host decodes that IM
//! (`InstantMessage::group_notice`) and raises a card into the **shared
//! notification-host channel** ([`crate::notification_host`]) — top-trailing,
//! priority-ordered, overflow-cycled — with the reference panel's five pieces:
//!
//! - the **group image** (the group's insignia texture, from [`GroupsModel`]);
//! - a **"Group Notice"** header and a **"Sent by … , group"** title line;
//! - the notice **subject** (emphasised) and its posting **date** in SLT
//!   (Second Life Time — US Pacific, the zone notices are written in);
//! - the notice **body**;
//! - the attached **item** (icon + name), when the notice carries one.
//!
//! Each card offers **OK** (dismiss), **Group Notices** (open the group's profile
//! Notices tab) and **Group Chat** (open the group IM), plus a close **×**. A card
//! **sticks** until dismissed — a group notice is an alert, not a fading tip.
//!
//! # Links and attachments are deferred
//!
//! The body is rendered as plain text: turning its URLs / SLURLs into clickable
//! links is the shared linkification layer ([[viewer-url-linkification]]), tracked
//! for group notices as [[viewer-group-notice-body-links]]. Accepting the
//! attachment into inventory (rather than only showing it) is the receive half of
//! [[viewer-group-notice-attachments]]; this host shows the item but does not yet
//! copy it.
//!
//! # Coordinating with the Notices tab
//!
//! The group profile's Notices tab also fetches a notice's body on demand, and its
//! reply is an `ImDialog::GroupNotice` IM indistinguishable from a fresh push.
//! The tab records the ids it requested in `RequestedGroupNotices`; this host
//! consults it and **suppresses** the toast for a notice the user pulled up to
//! read — the reference `IM_GROUP_NOTICE_REQUESTED` (no popup) vs. a fresh
//! `IM_GROUP_NOTICE`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use std::collections::BTreeMap;

use sl_client_bevy::{
    AssetType, Command, GroupKey, GroupNoticeItem, GroupNoticeKey, GroupNoticeReceived, SlCommand,
    SlEvent, SlSessionEvent, TextureKey, Uuid, to_bevy_image,
};
use sl_l10n::{DateTimeLength, DateTimeStyle};

use crate::group_profile::RequestedGroupNotices;
use crate::i18n::{TransArgs, Translator};
use crate::linkified_text::{LinkTextStyle, spawn_linkified_text};
use crate::notification_host::{NotificationChannelRoot, ResolveNotification, adopt_toast};
use crate::notification_persist::{
    PersistNotification, PersistedKind, ReloadPersistedNotification,
};
use crate::notifications::{
    NotificationId, NotificationKind, NotificationManager, NotificationPriority,
};
use crate::slt;
use crate::textures::TextureManager;
use crate::ui::{column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::world_api::AVATAR_BOOST_PRIORITY;
use crate::world_api::GroupsModel;
use crate::world_api::OpenGroupProfile;
use crate::world_api::{ConversationKey, OpenConversation};

/// The catalogue-template sentinel a group-notice toast reports as (it is not a
/// real [`crate::notifications::NOTIFICATIONS`] entry — the card is bespoke — but
/// the shared [`Toast`](crate::notification_host) machinery wants a stable name
/// for its history / response bookkeeping).
const GROUP_NOTICE_TEMPLATE: &str = "GroupNotice";

/// The account setting gating group-notice toasts (the Preferences alerts
/// tab's headline row; our own name — the reference has no single global
/// gate). While off, a received notice raises no card and is not persisted
/// for relogin re-raise (it stays readable in the group's Notices tab, which
/// pulls from the server). Lives in the `[notifications]` section with the
/// other notification preferences.
pub const SETTING_GROUP_NOTICE_TOASTS: &str = "ShowGroupNoticeToasts";

/// Startup: declare [`SETTING_GROUP_NOTICE_TOASTS`] (default on).
fn register_group_notice_settings(settings: Option<ResMut<crate::settings::ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        &[crate::notifications::NOTIFICATIONS_SECTION],
        SETTING_GROUP_NOTICE_TOASTS,
        sl_settings::SettingValue::Bool(true),
        "Show a toast when a group notice arrives",
    );
}

/// The renderer id a persisted group-notice card ([`PersistedKind::Custom`])
/// carries, so the persistent store routes its reload back here
/// ([`reload_group_notices`]).
const GROUP_NOTICE_RENDERER: &str = "group-notice";

/// The element id the gallery specimen and its inert actions report under.
const GROUP_NOTICE_ELEMENT: &str = "group-notice-toast";

/// The skin class a card wears (`.sk-toast`), so the group-notice card inherits
/// the toast surface styling.
const CARD_CLASS: &str = "sk-toast";

/// The skin class the body / title text wears (`.sk-toast-text`).
const TEXT_CLASS: &str = "sk-toast-text";

/// The skin class a card button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The close-button glyph (a multiplication sign), matching the reference toast's
/// close affordance.
const CLOSE_GLYPH: &str = "\u{00d7}";

/// The placeholder glyph for a group with no insignia, and the generic
/// attachment glyph fallback.
const GROUP_GLYPH: &str = "\u{1f465}";

/// A card's widest allowed width, in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

/// A card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// A card's border width, in logical pixels — the group-notice accent is painted
/// on it.
const CARD_BORDER: f32 = 2.0;

/// The insignia box edge, in logical pixels — the reference `group_icon` is 64px.
const ICON_EDGE: f32 = 64.0;

/// The gap between a card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 6.0;

/// The card body text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The header ("Group Notice") text size, in logical pixels.
const HEADER_FONT_SIZE: f32 = 17.0;

/// The width bound for a card's text column: the card content width, less its
/// padding and border, less the insignia column and its gap — so a wrapped
/// paragraph is the sole inline occupant of a decoration-free box (the
/// `viewer-text-node-padding-measure` constraint).
const TEXT_MAX_WIDTH: f32 =
    CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER - ICON_EDGE - CARD_ROW_GAP;

/// The width bound for a full-width text line (header / title), spanning the card
/// content width less its padding and border.
const FULL_TEXT_MAX_WIDTH: f32 = CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER;

/// A card's fallback background, used when no skin is loaded — the skin's
/// `.sk-toast` (`var(--surface-bg)`) overrides it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// A card's fallback body text colour — the skin's `.sk-toast-text` overrides it.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (the "Sent by" title, the date).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The group-notice accent painted on a card's border and its default button.
const ACCENT_COLOR: Color = Color::srgb(0.52, 0.68, 0.95);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The insignia box's sunken backdrop behind the loading placeholder.
const ICON_BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);

/// The plugin: drives the group-notice cards into the shared notification channel.
#[derive(Debug)]
pub struct GroupNoticePlugin;

impl Plugin for GroupNoticePlugin {
    /// Ingest received notices (into the shared toast channel), re-raise ones
    /// persisted from a previous session, and poll their insignia textures.
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_group_notice_settings)
            .add_systems(
                Update,
                (
                    ingest_group_notices,
                    reload_group_notices,
                    poll_group_notice_insignia,
                ),
            );
    }
}

/// Marks an insignia image box awaiting its texture, carrying the texture id the
/// pipeline decodes. [`poll_group_notice_insignia`] swaps the [`ImageNode`] in and
/// clears the marker once the texture is ready.
#[derive(Component, Debug)]
struct PendingInsignia(TextureKey);

/// Read the event stream; for each received group notice (that the Notices tab did
/// not itself request), decode it and raise a card into the shared toast channel —
/// so a group notice stacks, orders and overflow-cycles alongside the catalogue
/// notifications ([`crate::notification_host`]) rather than in a channel of its own.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the event \
              stream, the shared channel + manager it raises into, the group model + texture \
              manager + translator it renders from, the requested-notice set it consults, the \
              toast-gate settings, and the command writer + commands it acts through"
)]
fn ingest_group_notices(
    mut events: MessageReader<SlEvent>,
    settings: Option<Res<crate::settings::ViewerSettings>>,
    channel: Option<Res<NotificationChannelRoot>>,
    mut manager: ResMut<NotificationManager>,
    groups: Res<GroupsModel>,
    mut requested: ResMut<RequestedGroupNotices>,
    mut textures: ResMut<TextureManager>,
    translator: Translator,
    mut sl_commands: MessageWriter<SlCommand>,
    mut persist: MessageWriter<PersistNotification>,
    mut commands: Commands,
) {
    let Some(channel) = channel else {
        return;
    };
    for event in events.read() {
        let SlSessionEvent::InstantMessageReceived(im) = &event.0 else {
            continue;
        };
        let Some(notice) = im.group_notice() else {
            continue;
        };
        // The IM's `id` is the notice id (its `imSessionID`). Suppress the toast
        // for a notice the Notices tab pulled up to read itself.
        let notice_id = GroupNoticeKey::from(im.id);
        if requested.take_requested(notice_id) {
            continue;
        }
        // The alerts-tab gate: while group-notice toasts are off, raise no
        // card and skip the relogin persist too (a persisted notice would
        // re-raise a card at the next login, defeating the setting); the
        // notice stays readable in the group's server-side Notices tab.
        let show_toasts = settings
            .as_deref()
            .and_then(|settings| settings.store().get_bool(SETTING_GROUP_NOTICE_TOASTS).ok())
            .unwrap_or(true);
        if !show_toasts {
            continue;
        }
        // Resolve the group name for the title; request it if this notice's group
        // is not in the membership cache (unusual — notices come from member
        // groups — but a name is better than a raw id).
        if groups.group_name(notice.group_id).is_none() {
            groups.request_name(notice.group_id, &mut sl_commands);
        }
        let id = spawn_group_notice_card(
            &mut commands,
            &channel,
            &mut manager,
            &notice,
            &groups,
            &translator,
            &mut textures,
        );
        persist_group_notice(&mut persist, id, &notice);
    }
}

/// Re-raise the group-notice cards persisted from a previous session (the
/// [`PersistedKind::Custom`] entries [`crate::notification_persist`] reloads at
/// login): decode each back into a [`GroupNoticeReceived`], pop its card, and
/// re-persist it (a fresh id) so it keeps surviving relogs until answered.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the reload \
              stream, the shared channel + manager it raises into, the group model + texture \
              manager + translator it renders from, and the persistence channel it re-persists \
              through"
)]
fn reload_group_notices(
    mut reloads: MessageReader<ReloadPersistedNotification>,
    channel: Option<Res<NotificationChannelRoot>>,
    mut manager: ResMut<NotificationManager>,
    groups: Res<GroupsModel>,
    mut textures: ResMut<TextureManager>,
    translator: Translator,
    mut persist: MessageWriter<PersistNotification>,
    mut commands: Commands,
) {
    let Some(channel) = channel else {
        return;
    };
    for reload in reloads.read() {
        if reload.renderer != GROUP_NOTICE_RENDERER {
            continue;
        }
        let Some(notice) = decode_group_notice(&reload.data) else {
            warn!("group notice: dropping a malformed persisted card");
            continue;
        };
        let id = spawn_group_notice_card(
            &mut commands,
            &channel,
            &mut manager,
            &notice,
            &groups,
            &translator,
            &mut textures,
        );
        persist_group_notice(&mut persist, id, &notice);
    }
}

/// Persist a group-notice card under its toast `id`, so it re-displays after a
/// relog until the user answers it — a [`PersistedKind::Custom`] payload the
/// [`reload_group_notices`] system rebuilds from.
fn persist_group_notice(
    persist: &mut MessageWriter<PersistNotification>,
    id: NotificationId,
    notice: &GroupNoticeReceived,
) {
    persist.write(PersistNotification {
        id,
        kind: PersistedKind::Custom {
            renderer: GROUP_NOTICE_RENDERER.to_owned(),
            data: encode_group_notice(notice),
        },
    });
}

/// The already-resolved content of one group-notice card, ready to render — the
/// live path resolves the decoded notice + i18n + group model into this; the
/// gallery specimen builds it from literals, so both render through the one
/// [`build_group_notice_card`] (the registry rule, [`crate::ui_element`]).
struct GroupNoticeCardContent {
    /// The "Group Notice" header text.
    header: String,
    /// The "Sent by …, group" title line.
    sent_by: String,
    /// The notice subject (emphasised).
    subject: String,
    /// The posting date in SLT, or `None` when the notice carried no timestamp.
    date: Option<String>,
    /// The notice body (plain text; linkification deferred).
    body: String,
    /// The attached item's class and name, when the notice carries one.
    attachment: Option<(AssetType, String)>,
    /// The group image to show.
    insignia: Insignia,
    /// The insignia's loading-placeholder label (shown until the texture decodes).
    loading_label: String,
    /// The OK button label.
    ok_label: String,
    /// The Group Notices button label.
    notices_label: String,
    /// The Group Chat button label.
    chat_label: String,
}

/// The group image a card shows: a texture to request and swap in, or the generic
/// group glyph (a group with no insignia, and the gallery specimen).
enum Insignia {
    /// No insignia: show the generic group glyph.
    Glyph,
    /// Request this texture and swap it in once decoded ([`poll_group_notice_insignia`]).
    Pending(TextureKey),
}

/// The entities [`build_group_notice_card`] produced that a caller wires: the card
/// root and its four action boxes (OK / Group Notices / Group Chat / close ×).
struct GroupNoticeCard {
    /// The card root node.
    root: Entity,
    /// The OK (dismiss) button box.
    ok: Entity,
    /// The Group Notices button box.
    notices: Entity,
    /// The Group Chat button box.
    chat: Entity,
    /// The close (×) button box.
    close: Entity,
}

/// Build a group-notice card's node tree from resolved [`GroupNoticeCardContent`],
/// returning the entities a caller wires. The **root is left with no parent**: the
/// live host parents it into the shared toast channel via [`adopt_toast`], the
/// gallery specimen parents it under its cell. Shared by both so the two render
/// identically.
fn build_group_notice_card(
    commands: &mut Commands,
    content: &GroupNoticeCardContent,
) -> GroupNoticeCard {
    let card = commands
        .spawn((
            Node {
                max_width: Val::Px(CARD_MAX_WIDTH),
                padding: UiRect::all(Val::Px(CARD_PADDING)),
                border: UiRect::all(Val::Px(CARD_BORDER)),
                ..column(Val::Px(CARD_ROW_GAP))
            },
            BackgroundColor(CARD_BACKGROUND),
            BorderColor::all(ACCENT_COLOR),
            ClassList::new_with_classes([CARD_CLASS]),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("group-notice-card"),
        ))
        .id();

    // Close (×), top-trailing — the early-dismiss affordance.
    let close = spawn_close_button(commands, card);

    // The "Group Notice" header, then the "Sent by …" title, each a full-width
    // bounded line so a long name wraps within the card.
    spawn_bounded_text(
        commands,
        card,
        FULL_TEXT_MAX_WIDTH,
        content.header.clone(),
        HEADER_FONT_SIZE,
        TEXT_COLOR,
    );
    spawn_bounded_text(
        commands,
        card,
        FULL_TEXT_MAX_WIDTH,
        content.sent_by.clone(),
        FONT_SIZE,
        DIM_TEXT_COLOR,
    );

    // The insignia + message row: the group image leading, the subject / date /
    // body stacked after it.
    let body_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Start,
                ..row(Val::Px(CARD_ROW_GAP))
            },
            Name::new("group-notice-body-row"),
            ChildOf(card),
        ))
        .id();
    spawn_insignia(
        commands,
        body_row,
        &content.insignia,
        &content.loading_label,
    );

    let message_column = commands
        .spawn((
            Node {
                max_width: Val::Px(TEXT_MAX_WIDTH),
                ..column(Val::Px(4.0))
            },
            Name::new("group-notice-message"),
            ChildOf(body_row),
        ))
        .id();
    // The subject (emphasised) and, when present, the posting date in SLT.
    spawn_bounded_text(
        commands,
        message_column,
        TEXT_MAX_WIDTH,
        content.subject.clone(),
        FONT_SIZE,
        TEXT_COLOR,
    );
    if let Some(date) = &content.date {
        spawn_bounded_text(
            commands,
            message_column,
            TEXT_MAX_WIDTH,
            date.clone(),
            FONT_SIZE,
            DIM_TEXT_COLOR,
        );
    }
    // The body is linkified — its http(s) URLs / SLURLs render as clickable links
    // (viewer-group-notice-body-links), exactly as nearby chat and the other toast
    // bodies do.
    spawn_bounded_linked_text(
        commands,
        message_column,
        TEXT_MAX_WIDTH,
        content.body.clone(),
        FONT_SIZE,
        TEXT_COLOR,
    );

    // The attachment row (icon + name), when the notice carries an item.
    if let Some((asset_type, item_name)) = &content.attachment {
        spawn_attachment_row(commands, card, *asset_type, item_name);
    }

    // The action buttons: OK (dismiss), Group Notices, Group Chat.
    let (ok, notices, chat) = spawn_action_buttons(
        commands,
        card,
        &content.ok_label,
        &content.notices_label,
        &content.chat_label,
    );

    GroupNoticeCard {
        root: card,
        ok,
        notices,
        chat,
        close,
    }
}

/// Build one group-notice card from a decoded notice, adopt it into the shared
/// toast channel, request its insignia texture, and wire the live actions. The
/// card is an [`Alert`](NotificationKind::Alert): it **sticks** (never auto-fades)
/// and only leaves when the user actively clicks OK / × — display alone never
/// marks the notice seen, so the server may redeliver an unclosed notice on the
/// next login.
fn spawn_group_notice_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    notice: &GroupNoticeReceived,
    groups: &GroupsModel,
    translator: &Translator,
    textures: &mut TextureManager,
) -> NotificationId {
    let group_name = groups
        .group_name(notice.group_id)
        .map_or_else(|| notice.group_id.uuid().to_string(), ToOwned::to_owned);
    let insignia_key = groups.group_insignia(notice.group_id);
    let content = GroupNoticeCardContent {
        header: translator.get("group-notice-header"),
        sent_by: translator.format(
            "group-notice-sent-by",
            &TransArgs::new()
                .text("sender", &notice.sender_name)
                .text("group", &group_name),
        ),
        subject: notice.subject.clone(),
        date: notice
            .timestamp
            .map(|timestamp| notice_datetime_slt(translator, timestamp)),
        body: notice.body.clone(),
        attachment: notice
            .attachment
            .as_ref()
            .map(|item| (item.asset_type, item.item_name.clone())),
        insignia: insignia_key.map_or(Insignia::Glyph, Insignia::Pending),
        loading_label: translator.get("group-notice-loading"),
        ok_label: translator.get("group-notice-button-ok"),
        notices_label: translator.get("group-notice-button-notices"),
        chat_label: translator.get("group-notice-button-chat"),
    };
    let card = build_group_notice_card(commands, &content);

    // Adopt the card into the shared toast channel so it stacks / orders /
    // overflow-cycles with the catalogue notifications. An `Alert` never
    // auto-expires — only a user click ends it.
    let id = adopt_toast(
        commands,
        manager,
        channel,
        card.root,
        NotificationKind::Alert,
        NotificationPriority::Normal,
        GROUP_NOTICE_TEMPLATE,
        Some("OK"),
        notice.subject.clone(),
    );

    // Request the insignia texture the build marked pending.
    if let Some(key) = insignia_key {
        textures.request_boosted(key, AVATAR_BOOST_PRIORITY);
    }

    // OK / close ×: resolve the toast through the host's teardown — a user close,
    // the only thing that ends the notice.
    let root = card.root;
    for (button, name) in [(card.ok, Some("OK")), (card.close, None)] {
        commands.entity(button).observe(
            move |_activate: On<Activate>, mut resolves: MessageWriter<ResolveNotification>| {
                resolves.write(ResolveNotification {
                    toast: root,
                    button: name,
                });
            },
        );
    }
    // Group Notices: open the group's profile (its Notices tab). The card stays —
    // opening the profile does not acknowledge the notice.
    let group = notice.group_id;
    commands.entity(card.notices).observe(
        move |_activate: On<Activate>, mut open: MessageWriter<OpenGroupProfile>| {
            open.write(OpenGroupProfile { group });
        },
    );
    // Group Chat: open (and join) the group's IM session. The card likewise stays.
    commands.entity(card.chat).observe(
        move |_activate: On<Activate>,
              mut open: MessageWriter<OpenConversation>,
              mut sl: MessageWriter<SlCommand>| {
            open.write(OpenConversation {
                key: ConversationKey::Group(group),
            });
            sl.write(SlCommand(Command::StartGroupSession(group)));
        },
    );
    id
}

/// Spawn the insignia box and its placeholder: the group image once decoded (a
/// loading label until then, via [`PendingInsignia`]), or a generic group glyph
/// when there is no insignia.
fn spawn_insignia(
    commands: &mut Commands,
    parent: Entity,
    insignia: &Insignia,
    loading_label: &str,
) {
    let box_entity = commands
        .spawn((
            Node {
                width: Val::Px(ICON_EDGE),
                height: Val::Px(ICON_EDGE),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ICON_BACKDROP),
            Name::new("group-notice-insignia"),
            ChildOf(parent),
        ))
        .id();
    match insignia {
        Insignia::Glyph => {
            // No insignia: a generic group glyph stands in for the reference's
            // `Generic_Group` fallback icon.
            commands.spawn((
                Text::new(GROUP_GLYPH),
                UiFont::Sans.at(HEADER_FONT_SIZE),
                TextColor(DIM_TEXT_COLOR),
                Pickable::IGNORE,
                ChildOf(box_entity),
            ));
        }
        Insignia::Pending(key) => {
            commands.spawn((
                Text::new(loading_label.to_owned()),
                UiFont::Sans.at(FONT_SIZE),
                TextColor(DIM_TEXT_COLOR),
                Pickable::IGNORE,
                ChildOf(box_entity),
            ));
            commands.entity(box_entity).insert(PendingInsignia(*key));
        }
    }
}

/// Swap each pending insignia into its box once the pipeline decodes the texture,
/// dropping the loading placeholder.
fn poll_group_notice_insignia(
    pending: Query<(Entity, &PendingInsignia)>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    for (box_entity, PendingInsignia(key)) in &pending {
        let Some(decoded) = manager.decoded(*key) else {
            continue;
        };
        let handle = images.add(to_bevy_image(decoded));
        // Drop the loading placeholder, then show the image on the box.
        if let Ok(kids) = children.get(box_entity) {
            for child in kids {
                commands.entity(*child).despawn();
            }
        }
        commands
            .entity(box_entity)
            .insert(ImageNode::new(handle))
            .remove::<PendingInsignia>();
    }
}

/// Spawn the attachment row: an asset-type glyph and the item's name.
fn spawn_attachment_row(
    commands: &mut Commands,
    card: Entity,
    asset_type: AssetType,
    item_name: &str,
) {
    let attach_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("group-notice-attachment"),
            ChildOf(card),
        ))
        .id();
    commands.spawn((
        Text::new(attachment_glyph(asset_type)),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(attach_row),
    ));
    // The name is the sole occupant of a width-bounded box so a long item name
    // wraps within the card rather than overflowing it.
    let name_box = commands
        .spawn((
            Node {
                max_width: Val::Px(FULL_TEXT_MAX_WIDTH - 24.0),
                ..default()
            },
            ChildOf(attach_row),
        ))
        .id();
    commands.spawn((
        Text::new(item_name.to_owned()),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        ClassList::new_with_classes([TEXT_CLASS]),
        Pickable::IGNORE,
        ChildOf(name_box),
    ));
}

/// Spawn the OK / Group Notices / Group Chat button row, returning the three
/// button boxes `(ok, notices, chat)` for the caller to wire actions onto.
fn spawn_action_buttons(
    commands: &mut Commands,
    card: Entity,
    ok_label: &str,
    notices_label: &str,
    chat_label: &str,
) -> (Entity, Entity, Entity) {
    let button_row = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(6.0),
                justify_content: JustifyContent::End,
                ..row(Val::Px(6.0))
            },
            Name::new("group-notice-buttons"),
            ChildOf(card),
        ))
        .id();
    // OK is the default action (Enter), so it wears the accent border.
    let ok = spawn_button(commands, button_row, ok_label, true, 1);
    let notices = spawn_button(commands, button_row, notices_label, false, 2);
    let chat = spawn_button(commands, button_row, chat_label, false, 3);
    (ok, notices, chat)
}

/// Spawn one card button (accented border when it is the default) and return its
/// clickable box for the caller to wire an observer onto.
fn spawn_button(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    is_default: bool,
    tab: i32,
) -> Entity {
    let border = if is_default {
        ACCENT_COLOR
    } else {
        BUTTON_BORDER
    };
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
            Name::new(format!("group-notice-button:{label}")),
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
            Name::new("group-notice-close-row"),
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
            Name::new("group-notice-close"),
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
/// decoration-free box of the given max width, so it wraps within the card (the
/// `viewer-text-node-padding-measure` constraint). An empty string spawns nothing.
fn spawn_bounded_text(
    commands: &mut Commands,
    parent: Entity,
    max_width: f32,
    text: String,
    font_size: f32,
    color: Color,
) {
    if text.is_empty() {
        return;
    }
    let box_entity = commands
        .spawn((
            Node {
                max_width: Val::Px(max_width),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(text),
        UiFont::Sans.at(font_size),
        TextColor(color),
        ClassList::new_with_classes([TEXT_CLASS]),
        Pickable::IGNORE,
        ChildOf(box_entity),
    ));
}

/// A width-bounded **linkified** text line: the caller's text run through the
/// shared linkification widget ([`spawn_linkified_text`]) inside a bounded box, so
/// its URLs / SLURLs render as clickable links and a long body still wraps within
/// the card. An empty string spawns nothing.
fn spawn_bounded_linked_text(
    commands: &mut Commands,
    parent: Entity,
    max_width: f32,
    text: String,
    font_size: f32,
    color: Color,
) {
    if text.is_empty() {
        return;
    }
    let box_entity = commands
        .spawn((
            Node {
                max_width: Val::Px(max_width),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    let mut style = LinkTextStyle::at(font_size);
    style.plain_color = color;
    spawn_linkified_text(commands, box_entity, &text, style);
}

/// Format a notice's Unix timestamp as a localized date-time in **SLT** (Second
/// Life Time — US Pacific), the zone group notices are written in — with an `SLT`
/// marker, matching the status bar's clock.
fn notice_datetime_slt(translator: &Translator, timestamp: u32) -> String {
    let when = slt::current_slt(i64::from(timestamp));
    let formatted = translator.datetime(when, DateTimeStyle::DateTime, DateTimeLength::Medium);
    translator.format(
        "group-notice-timestamp",
        &TransArgs::new().text("when", &formatted),
    )
}

/// The emoji glyph for an attached item's asset class — mirroring the inventory
/// list's type glyphs (the reference viewer uses type-icon textures we do not
/// ship). A class without a distinct glyph falls back to a generic package.
const fn attachment_glyph(asset_type: AssetType) -> &'static str {
    match asset_type {
        AssetType::Texture | AssetType::ImageJpeg | AssetType::ImageTga | AssetType::TextureTga => {
            "\u{1f5bc}\u{fe0f}"
        }
        AssetType::Sound | AssetType::SoundWav => "\u{1f50a}",
        AssetType::CallingCard => "\u{1f4c7}",
        AssetType::Landmark => "\u{1f4cd}",
        AssetType::Clothing | AssetType::Bodypart => "\u{1f455}",
        AssetType::Object => "\u{1f4e6}",
        AssetType::Notecard => "\u{1f4c4}",
        AssetType::ScriptText | AssetType::ScriptBytecode => "\u{1f4dc}",
        AssetType::Animation => "\u{1f3c3}",
        AssetType::Gesture => "\u{1f44b}",
        AssetType::Mesh => "\u{1f4d0}",
        AssetType::Settings => "\u{2699}\u{fe0f}",
        AssetType::Material => "\u{1f3a8}",
        _other => "\u{1f4e6}",
    }
}

/// Serialize a received group notice into the flat string map the persistent
/// store ([`crate::notification_persist`]) saves — the fields
/// [`decode_group_notice`] rebuilds the card from. The group insignia is **not**
/// stored: it is re-derived from [`GroupsModel`] on reload (the group is still a
/// membership), matching the reference (which re-reads the insignia from its group
/// data rather than the notice).
fn encode_group_notice(notice: &GroupNoticeReceived) -> BTreeMap<String, String> {
    let mut data = BTreeMap::new();
    data.insert("group_id".to_owned(), notice.group_id.uuid().to_string());
    data.insert("sender".to_owned(), notice.sender_name.clone());
    data.insert("subject".to_owned(), notice.subject.clone());
    data.insert("body".to_owned(), notice.body.clone());
    if let Some(timestamp) = notice.timestamp {
        data.insert("timestamp".to_owned(), timestamp.to_string());
    }
    if let Some(item) = &notice.attachment {
        data.insert(
            "asset_type".to_owned(),
            item.asset_type.to_code().to_string(),
        );
        data.insert("item_name".to_owned(), item.item_name.clone());
    }
    data
}

/// Rebuild a [`GroupNoticeReceived`] from a persisted string map (the inverse of
/// [`encode_group_notice`]). Returns `None` only when the group id is missing /
/// unparsable — the one field a card cannot be shown without.
fn decode_group_notice(data: &BTreeMap<String, String>) -> Option<GroupNoticeReceived> {
    let group_id = data
        .get("group_id")
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .map(GroupKey::from)?;
    let attachment = data.get("asset_type").map(|code| {
        let asset_type = code
            .parse::<i32>()
            .map_or(AssetType::Texture, AssetType::from_code);
        GroupNoticeItem {
            asset_type,
            item_name: data.get("item_name").cloned().unwrap_or_default(),
        }
    });
    Some(GroupNoticeReceived {
        group_id,
        sender_name: data.get("sender").cloned().unwrap_or_default(),
        subject: data.get("subject").cloned().unwrap_or_default(),
        body: data.get("body").cloned().unwrap_or_default(),
        timestamp: data
            .get("timestamp")
            .and_then(|raw| raw.parse::<u32>().ok()),
        attachment,
    })
}

/// The gallery / `ui_test` specimen: a static group-notice card with an
/// attachment, so its layout is swept by the harness login-free (a live notice
/// needs a grid). Registered in `crate::ui_element::ELEMENTS`; its buttons
/// report an inert [`UiAction`] rather than opening anything.
pub fn spawn_group_notice_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = GroupNoticeCardContent {
        header: cx.text("Group Notice"),
        sent_by: cx.text("Sent by Board Member, Example Group"),
        subject: cx.text("Board meeting"),
        date: Some(cx.text("Jul 29, 2026, 12:00 SLT")),
        body: cx.text(SPECIMEN_BODY),
        attachment: Some((AssetType::Notecard, cx.text("Meeting agenda"))),
        // A group glyph, so the specimen needs no texture pipeline.
        insignia: Insignia::Glyph,
        loading_label: cx.text("(loading)"),
        ok_label: cx.text("OK"),
        notices_label: cx.text("Group Notices"),
        chat_label: cx.text("Group Chat"),
    };
    let card = build_group_notice_card(commands, &content);
    // The specimen parents the card under its gallery cell (the live host instead
    // adopts it into the shared toast channel).
    commands.entity(card.root).insert(ChildOf(parent));
    for (button, action) in [
        (card.ok, "ok"),
        (card.notices, "notices"),
        (card.chat, "chat"),
        (card.close, "close"),
    ] {
        commands.entity(button).observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element: GROUP_NOTICE_ELEMENT,
                    action,
                });
            },
        );
    }
    card.root
}

/// The specimen's body prose — long enough to force the wrap the matrix sweeps.
const SPECIMEN_BODY: &str = "Please join us for this month's board meeting to \
    review the budget and vote on the new build guidelines. Bring any questions \
    you have for the officers.";

/// Compile-time guard: the text column bound stays positive after the insignia
/// column and paddings are subtracted — a negative bound would collapse the body
/// to nothing.
const _: () = assert!(
    TEXT_MAX_WIDTH > 0.0,
    "group-notice text column bound must stay positive"
);

#[cfg(test)]
mod tests {
    use super::{attachment_glyph, decode_group_notice, encode_group_notice};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AssetType, GroupKey, GroupNoticeItem, GroupNoticeReceived, Uuid};

    /// A few asset classes map to distinct glyphs, and an unknown class falls back
    /// to the generic package — so the attachment row always shows something.
    #[test]
    fn attachment_glyphs_are_assigned() {
        assert_eq!(attachment_glyph(AssetType::Notecard), "\u{1f4c4}");
        assert_eq!(attachment_glyph(AssetType::Landmark), "\u{1f4cd}");
        assert_eq!(attachment_glyph(AssetType::Other(999)), "\u{1f4e6}");
    }

    /// A notice with an attachment survives the persist encode → decode round trip
    /// bit-for-bit (minus the insignia, which is re-derived) — the guarantee a
    /// reloaded card rebuilds identically.
    #[test]
    fn group_notice_encode_decode_round_trips() -> Result<(), String> {
        let notice = GroupNoticeReceived {
            group_id: GroupKey::from(Uuid::from_u128(0x9401)),
            sender_name: "Board Member".to_owned(),
            subject: "Board meeting".to_owned(),
            body: "Tuesday at noon SLT".to_owned(),
            timestamp: Some(1_700_000_000),
            attachment: Some(GroupNoticeItem {
                asset_type: AssetType::Notecard,
                item_name: "Agenda".to_owned(),
            }),
        };
        let decoded = decode_group_notice(&encode_group_notice(&notice))
            .ok_or_else(|| "expected a decoded notice".to_owned())?;
        assert_eq!(decoded, notice);
        Ok(())
    }

    /// A notice with no attachment and no timestamp round-trips too (the optional
    /// fields simply absent), and a map missing the group id decodes to `None`.
    #[test]
    fn group_notice_optional_fields_and_missing_group() -> Result<(), String> {
        let notice = GroupNoticeReceived {
            group_id: GroupKey::from(Uuid::from_u128(0x9402)),
            sender_name: "Officer".to_owned(),
            subject: "Notice".to_owned(),
            body: String::new(),
            timestamp: None,
            attachment: None,
        };
        let encoded = encode_group_notice(&notice);
        let decoded =
            decode_group_notice(&encoded).ok_or_else(|| "expected a decoded notice".to_owned())?;
        assert_eq!(decoded, notice);
        // A payload with no group id cannot rebuild a card.
        let mut without_group = encoded;
        without_group.remove("group_id");
        assert!(decode_group_notice(&without_group).is_none());
        Ok(())
    }
}
