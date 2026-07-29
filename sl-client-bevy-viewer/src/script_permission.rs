//! The **script permission-request toast host** (`viewer-permission-request-dialog`):
//! the panel a scripted object's `llRequestPermissions` pops, mirroring the
//! reference `ScriptQuestion` / `ScriptQuestionCaution` notifications.
//!
//! # What it renders
//!
//! When an in-world script calls `llRequestPermissions`, the simulator sends a
//! `ScriptQuestion` naming the holding object, its owner and the requested
//! permission bits (take controls, animate, attach, debit money, control camera,
//! teleport, …). `sl-proto` decodes it
//! ([`SlSessionEvent::ScriptPermissionRequest`]). This host raises a card into the
//! **shared notification-host channel** ([`crate::notification_host`]) —
//! top-trailing, priority-ordered, overflow-cycled — in one of two shapes:
//!
//! - the **standard** card ([`ScriptQuestion`]): an intro naming the object and
//!   its owner (`'Object', an object owned by Owner, would like to:`), a line per
//!   requested permission, `Is this OK?`, and the **Yes** / **No** / **Block**
//!   actions; or
//! - the **caution** card ([`ScriptQuestionCaution`], `priority="critical"`): the
//!   money-access warning the reference shows when a script asks to **debit** the
//!   agent's L$ account, with any *other* requested permissions listed below, and
//!   **Allow access** / **Deny** actions.
//!
//! A card **sticks** ([`NotificationKind::Alert`]) until the user answers it. The
//! answer is a `ScriptAnswerYes` ([`Command::AnswerScriptPermissions`]): a grant
//! carries the recognised requested mask, a deny carries an empty mask (the
//! reference sends the reply either way, and the session's grant mirror records
//! it so downstream consumers — control capture, script camera — read the grant).
//! Block additionally mutes the object. The close **×** denies conservatively,
//! never leaving the object with a silent grant. Like the sibling script dialogs
//! a permission request is **not persisted** across a relog: the simulator's
//! outstanding request does not survive the session.
//!
//! # Auto-grant is the simulator's job, not this host's
//!
//! The reference viewer does **not** auto-grant permissions for an attachment, a
//! sat-on object or an accepted experience — the **simulator** computes those
//! implicit grants (`llRequestPermissions`'s `implicitPerms`) and only sends a
//! `ScriptQuestion` for the non-implicit remainder. So this host faithfully
//! prompts for whatever the sim actually asks and adds no client-side auto-grant
//! (which would be dead: the sim never sends an implicit-only request as a
//! question). The accepted-experience management surface — the experience name,
//! the *Block Experience* action, and the reference `ScriptQuestionExperience`
//! card — is [[viewer-experience-permission-dialog]]'s; here an experience
//! request lists `Participate in an experience` and its grant carries the
//! experience id for the mirror. Active grants are tracked / revoked by
//! [[viewer-permission-active-grants]].
//!
//! There are no free-text URLs in a permission prompt (the strings are the fixed
//! reference permission wording), so — unlike the script dialog / group notice —
//! this host needs no body-linkification follow-up.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use sl_client_bevy::{
    Command, InventoryKey, MuteFlags, MuteType, ObjectKey, ScriptPermissionRequest,
    ScriptPermissions, SlCommand, SlEvent, SlSessionEvent,
};

use crate::i18n::{TransArgs, Translator};
use crate::notification_host::{NotificationChannelRoot, ResolveNotification, adopt_toast};
use crate::notifications::{
    NotificationId, NotificationKind, NotificationManager, NotificationPriority,
};
use crate::ui::{column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;

/// The catalogue-template sentinel a standard permission card reports as (it is
/// not a real [`crate::notifications::NOTIFICATIONS`] entry — the card is bespoke
/// — but the shared toast machinery wants a stable name for its history /
/// response bookkeeping). Named for the reference `ScriptQuestion` notification.
const SCRIPT_QUESTION_TEMPLATE: &str = "ScriptQuestion";

/// The catalogue-template sentinel the caution (money) card reports as, named for
/// the reference `ScriptQuestionCaution` notification.
const SCRIPT_QUESTION_CAUTION_TEMPLATE: &str = "ScriptQuestionCaution";

/// The element id the standard-card gallery specimen and its inert actions report
/// under.
const SCRIPT_PERMISSION_ELEMENT: &str = "script-permission-toast";

/// The element id the caution-card gallery specimen and its inert actions report
/// under.
const SCRIPT_PERMISSION_CAUTION_ELEMENT: &str = "script-permission-caution-toast";

/// The skin class a card wears (`.sk-toast`), so it inherits the toast surface
/// styling shared with the catalogue toasts and the sibling cards.
const CARD_CLASS: &str = "sk-toast";

/// The skin class the intro / body text wears (`.sk-toast-text`).
const TEXT_CLASS: &str = "sk-toast-text";

/// The skin class a card button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The close-button glyph (a multiplication sign), matching the reference toast.
const CLOSE_GLYPH: &str = "\u{00d7}";

/// The bullet glyph prefixing each requested-permission line.
const BULLET_GLYPH: &str = "\u{2022}\u{00a0}";

/// A card's widest allowed width, in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

/// A card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// A card's border width, in logical pixels — the permission accent is painted on
/// it.
const CARD_BORDER: f32 = 2.0;

/// The gap between a card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 6.0;

/// The gap between the action buttons, in logical pixels.
const BUTTON_GAP: f32 = 6.0;

/// The card body / button text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The intro line's text size, in logical pixels.
const INTRO_FONT_SIZE: f32 = 15.0;

/// The width bound for a full-width text line (intro / body / question), spanning
/// the card content width less its padding and border — so a wrapped paragraph is
/// the sole inline occupant of a decoration-free box (the
/// `viewer-text-node-padding-measure` constraint).
const FULL_TEXT_MAX_WIDTH: f32 = CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER;

/// A card's fallback background, used when no skin is loaded — the skin's
/// `.sk-toast` (`var(--surface-bg)`) overrides it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// A card's fallback body text colour — the skin's `.sk-toast-text` overrides it.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (the requested-permission lines).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The standard-card accent painted on its border and its default (Yes) button —
/// an indigo distinct from the script-dialog teal, the load-url amber and the
/// group-notice blue, so the cards read apart.
const ACCENT_COLOR: Color = Color::srgb(0.62, 0.55, 0.90);

/// The caution-card accent — a red, reading as "this can take your money: look
/// before you leap".
const CAUTION_COLOR: Color = Color::srgb(0.90, 0.42, 0.40);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The requested-permission bits this host recognises, each paired with its
/// fluent question key, in the reference `SCRIPT_PERMISSIONS` table order. A bit
/// absent from this table is one `sl-types` does not model — the reference logs
/// and drops such a bit, and so do we (it is never granted). `DEBIT` leads the
/// table but is handled specially: it selects the caution card rather than
/// appearing as an ordinary question line.
const PERMISSION_QUESTIONS: &[(i32, &str)] = &[
    (ScriptPermissions::DEBIT, "script-permission-q-debit"),
    (
        ScriptPermissions::TAKE_CONTROLS,
        "script-permission-q-controls",
    ),
    (
        ScriptPermissions::TRIGGER_ANIMATION,
        "script-permission-q-animation",
    ),
    (ScriptPermissions::ATTACH, "script-permission-q-attach"),
    (ScriptPermissions::CHANGE_LINKS, "script-permission-q-links"),
    (
        ScriptPermissions::TRACK_CAMERA,
        "script-permission-q-track-camera",
    ),
    (
        ScriptPermissions::CONTROL_CAMERA,
        "script-permission-q-control-camera",
    ),
    (ScriptPermissions::TELEPORT, "script-permission-q-teleport"),
    (
        ScriptPermissions::EXPERIENCE,
        "script-permission-q-experience",
    ),
    (
        ScriptPermissions::SILENT_ESTATE_MANAGEMENT,
        "script-permission-q-estate",
    ),
    (
        ScriptPermissions::OVERRIDE_ANIMATIONS,
        "script-permission-q-override-anim",
    ),
    (
        ScriptPermissions::RETURN_OBJECTS,
        "script-permission-q-return-objects",
    ),
];

/// The union of every bit in [`PERMISSION_QUESTIONS`] — the mask of permissions
/// this host recognises. A grant reply carries only these bits of a request; an
/// unmodelled bit is dropped, matching the reference.
fn known_mask() -> i32 {
    PERMISSION_QUESTIONS
        .iter()
        .fold(0, |mask, (bit, _key)| mask | bit)
}

/// The recognised subset of a request's permission bits (the requested bits this
/// host models, unmodelled bits dropped). This is the mask a grant reply sends.
fn recognized_mask(permissions: ScriptPermissions) -> i32 {
    permissions.0 & known_mask()
}

/// Whether a request asks for the **debit** (take-money) permission — the one
/// caution permission, which routes to the reference `ScriptQuestionCaution` card.
fn is_caution(permissions: ScriptPermissions) -> bool {
    recognized_mask(permissions) & ScriptPermissions::DEBIT != 0
}

/// The fluent keys of a request's recognised permission lines **excluding**
/// `DEBIT` (money is the caution card's headline, never an ordinary line), in
/// table order. Drives the standard card's body and the caution card's
/// "also requesting" list alike.
fn other_permission_keys(permissions: ScriptPermissions) -> Vec<&'static str> {
    let recognized = recognized_mask(permissions);
    PERMISSION_QUESTIONS
        .iter()
        .filter(|(bit, _key)| *bit != ScriptPermissions::DEBIT && recognized & bit != 0)
        .map(|(_bit, key)| *key)
        .collect()
}

/// The plugin: drives the script permission-request cards into the shared
/// notification channel.
pub(crate) struct ScriptPermissionPlugin;

impl Plugin for ScriptPermissionPlugin {
    /// Ingest received `ScriptQuestion` messages into the shared toast channel.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, ingest_script_permissions);
    }
}

/// Read the event stream; for each received `ScriptQuestion`, build its card and
/// raise it into the shared toast channel — so a permission request stacks,
/// orders and overflow-cycles alongside the catalogue notifications and the
/// sibling cards ([`crate::notification_host`]).
fn ingest_script_permissions(
    mut events: MessageReader<SlEvent>,
    channel: Option<Res<NotificationChannelRoot>>,
    mut manager: ResMut<NotificationManager>,
    translator: Translator,
    mut commands: Commands,
) {
    let Some(channel) = channel else {
        return;
    };
    for event in events.read() {
        let SlSessionEvent::ScriptPermissionRequest(request) = &event.0 else {
            continue;
        };
        spawn_script_permission_card(&mut commands, &channel, &mut manager, request, &translator);
    }
}

/// The resolved content of one permission card, ready to render — the live path
/// resolves the decoded request + i18n into this; the gallery specimens build it
/// from literals, so both render through the one [`build_script_permission_card`]
/// (the registry rule, [`crate::ui_element`]).
struct ScriptPermissionContent {
    /// Whether this is the caution (money) card rather than the standard card.
    caution: bool,
    /// The intro / warning paragraphs shown above the permission lines.
    intro: Vec<String>,
    /// The header preceding the permission lines (the caution "also requesting"
    /// header), or empty for the standard card (whose intro ends in a colon).
    lines_header: String,
    /// One line per recognised, listed permission (bulleted).
    permission_lines: Vec<String>,
    /// The trailing confirm line ("Is this OK?"), empty on the caution card.
    confirm: String,
    /// The grant (accept) button label ("Yes" / "Allow access").
    grant_label: String,
    /// The deny (decline) button label ("No" / "Deny").
    deny_label: String,
    /// The Block (mute) button label, or `None` when the card omits Block (the
    /// caution card, following the reference).
    block_label: Option<String>,
}

/// The entities [`build_script_permission_card`] produced that a caller wires: the
/// card root, the grant / deny boxes, the optional Block box, and the close box.
struct ScriptPermissionCard {
    /// The card root node (left with no parent — the caller adopts / parents it).
    root: Entity,
    /// The grant (accept) button box.
    grant: Entity,
    /// The deny (decline) button box.
    deny: Entity,
    /// The Block (mute) button box, when the card carries one.
    block: Option<Entity>,
    /// The close (×) button box.
    close: Entity,
}

/// Build a permission card's node tree from resolved [`ScriptPermissionContent`],
/// returning the entities a caller wires. The **root is left with no parent**: the
/// live host adopts it into the shared toast channel via [`adopt_toast`], the
/// gallery specimen parents it under its cell.
fn build_script_permission_card(
    commands: &mut Commands,
    content: &ScriptPermissionContent,
) -> ScriptPermissionCard {
    let accent = if content.caution {
        CAUTION_COLOR
    } else {
        ACCENT_COLOR
    };
    let root = commands
        .spawn((
            Node {
                max_width: Val::Px(CARD_MAX_WIDTH),
                padding: UiRect::all(Val::Px(CARD_PADDING)),
                border: UiRect::all(Val::Px(CARD_BORDER)),
                ..column(Val::Px(CARD_ROW_GAP))
            },
            BackgroundColor(CARD_BACKGROUND),
            BorderColor::all(accent),
            ClassList::new_with_classes([CARD_CLASS]),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("script-permission-card"),
        ))
        .id();

    // Close (×), top-trailing — the early-dismiss (conservative deny) affordance.
    let close = spawn_close_button(commands, root);

    // The intro / warning paragraphs (primary), then the optional "also
    // requesting" header (dim), each requested-permission line (dim, bulleted),
    // then the confirm line (primary) — each a width-bounded box so a long
    // paragraph wraps within the card.
    for (index, paragraph) in content.intro.iter().enumerate() {
        // The first intro paragraph is the lead, at the larger intro size.
        let size = if index == 0 {
            INTRO_FONT_SIZE
        } else {
            FONT_SIZE
        };
        spawn_bounded_text(commands, root, paragraph, size, TEXT_COLOR);
    }
    spawn_bounded_text(
        commands,
        root,
        &content.lines_header,
        FONT_SIZE,
        DIM_TEXT_COLOR,
    );
    for line in &content.permission_lines {
        spawn_bounded_text(
            commands,
            root,
            &format!("{BULLET_GLYPH}{line}"),
            FONT_SIZE,
            DIM_TEXT_COLOR,
        );
    }
    spawn_bounded_text(commands, root, &content.confirm, FONT_SIZE, TEXT_COLOR);

    // The bottom action row: grant (the accent default), deny, then the optional
    // Block, trailing-aligned.
    let action_row = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(BUTTON_GAP),
                justify_content: JustifyContent::End,
                ..row(Val::Px(BUTTON_GAP))
            },
            Name::new("script-permission-actions"),
            ChildOf(root),
        ))
        .id();
    // Grant is the default action, so it wears the accent.
    let grant = spawn_action_button(commands, action_row, &content.grant_label, accent, 1);
    let deny = spawn_action_button(commands, action_row, &content.deny_label, BUTTON_BORDER, 2);
    let block = content
        .block_label
        .as_ref()
        .map(|label| spawn_action_button(commands, action_row, label, BUTTON_BORDER, 3));

    ScriptPermissionCard {
        root,
        grant,
        deny,
        block,
        close,
    }
}

/// Build one permission card from a decoded [`ScriptPermissionRequest`], adopt it
/// into the shared toast channel, and wire the live actions. The card is an
/// [`Alert`](NotificationKind::Alert): it **sticks** (never auto-fades) and only
/// leaves when the user answers it. Grant sends `ScriptAnswerYes` with the
/// recognised requested mask; deny (and the conservative close ×) sends an empty
/// mask; Block additionally mutes the object. Not persisted: the outstanding
/// request does not survive a relog.
fn spawn_script_permission_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    request: &ScriptPermissionRequest,
    translator: &Translator,
) -> NotificationId {
    let content = permission_content(translator, request);
    let caution = content.caution;
    let card = build_script_permission_card(commands, &content);

    // The history line is the intro lead, so the notification well shows what was
    // asked at a glance.
    let history = content.intro.first().cloned().unwrap_or_default();
    let (template, priority) = if caution {
        (
            SCRIPT_QUESTION_CAUTION_TEMPLATE,
            NotificationPriority::Critical,
        )
    } else {
        (SCRIPT_QUESTION_TEMPLATE, NotificationPriority::Normal)
    };
    let id = adopt_toast(
        commands,
        manager,
        channel,
        card.root,
        NotificationKind::Alert,
        priority,
        template,
        None,
        history,
    );

    let root = card.root;
    let task_id = request.task_id;
    let item_id = request.item_id;
    let experience_id = request.experience_id;
    let granted = ScriptPermissions(recognized_mask(request.permissions));

    // Grant: reply with the recognised requested mask and tear the card down.
    commands.entity(card.grant).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            answer_permissions(&mut sl, task_id, item_id, granted, experience_id);
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Deny: reply with an empty mask (an explicit deny) and tear the card down.
    commands.entity(card.deny).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            answer_permissions(
                &mut sl,
                task_id,
                item_id,
                ScriptPermissions::default(),
                experience_id,
            );
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Block: deny (empty mask) and mute the object, then tear the card down.
    if let Some(block) = card.block {
        let object_id: ObjectKey = request.task_id;
        let object_name = request.object_name.clone();
        commands.entity(block).observe(
            move |_activate: On<Activate>,
                  mut sl: MessageWriter<SlCommand>,
                  mut resolves: MessageWriter<ResolveNotification>| {
                answer_permissions(
                    &mut sl,
                    task_id,
                    item_id,
                    ScriptPermissions::default(),
                    experience_id,
                );
                sl.write(SlCommand(Command::Mute {
                    id: object_id.uuid(),
                    name: object_name.clone(),
                    mute_type: MuteType::Object,
                    flags: MuteFlags::default(),
                }));
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }

    // Close ×: deny conservatively — dismissing a permission prompt must never
    // leave the object with a silent grant, so the close is an explicit deny.
    commands.entity(card.close).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            answer_permissions(
                &mut sl,
                task_id,
                item_id,
                ScriptPermissions::default(),
                experience_id,
            );
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );
    id
}

/// Queue a `ScriptAnswerYes` granting `permissions` (an empty set denies) to the
/// script `item_id` in object `task_id`, carrying the `experience_id` the request
/// was made under so the session's grant mirror records it.
fn answer_permissions(
    sl: &mut MessageWriter<SlCommand>,
    task_id: ObjectKey,
    item_id: InventoryKey,
    permissions: ScriptPermissions,
    experience_id: Option<sl_client_bevy::ExperienceKey>,
) {
    sl.write(SlCommand(Command::AnswerScriptPermissions {
        task_id,
        item_id,
        permissions,
        experience_id,
    }));
}

/// Resolve a decoded [`ScriptPermissionRequest`] + i18n into the card content: the
/// caution (money) shape when debit is asked, else the standard shape.
fn permission_content(
    translator: &Translator,
    request: &ScriptPermissionRequest,
) -> ScriptPermissionContent {
    let lines = other_permission_keys(request.permissions)
        .into_iter()
        .map(|key| translator.get(key))
        .collect::<Vec<_>>();
    if is_caution(request.permissions) {
        // The caution card: the money-access warning, then any *other* requested
        // permissions under the "also requesting" header. No confirm line (the
        // warning ends in a Deny instruction) and no Block (per the reference).
        let lines_header = if lines.is_empty() {
            String::new()
        } else {
            translator.get("script-permission-caution-additional")
        };
        ScriptPermissionContent {
            caution: true,
            intro: vec![
                translator.format(
                    "script-permission-caution-warning",
                    &TransArgs::new().text("object", &request.object_name),
                ),
                translator.get("script-permission-caution-advice"),
            ],
            lines_header,
            permission_lines: lines,
            confirm: String::new(),
            grant_label: translator.get("script-permission-button-allow"),
            deny_label: translator.get("script-permission-button-deny"),
            block_label: None,
        }
    } else {
        // The standard card: the "would like to:" intro, the permission lines,
        // then "Is this OK?" with Yes / No / Block.
        ScriptPermissionContent {
            caution: false,
            intro: vec![
                translator.format(
                    "script-permission-intro",
                    &TransArgs::new()
                        .text("object", &request.object_name)
                        .text("owner", &request.object_owner),
                ),
            ],
            lines_header: String::new(),
            permission_lines: lines,
            confirm: translator.get("script-permission-confirm"),
            grant_label: translator.get("script-permission-button-yes"),
            deny_label: translator.get("script-permission-button-no"),
            block_label: Some(translator.get("script-permission-button-block")),
        }
    }
}

/// Spawn one bottom-row action button (grant / deny / Block), bordered in the
/// given accent (the grant default wears the card accent, the rest the neutral
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
            Name::new(format!("script-permission-action:{label}")),
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
            Name::new("script-permission-close-row"),
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
            Name::new("script-permission-close"),
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

/// The gallery / `ui_test` specimen (standard card): a static permission request
/// with a few listed permissions, so the intro / line / confirm / action layout
/// is swept login-free (a live card needs a scripted object). Registered in
/// [`crate::ui_element::ELEMENTS`]; its buttons report an inert [`UiAction`].
pub(crate) fn spawn_script_permission_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = ScriptPermissionContent {
        caution: false,
        intro: vec![
            cx.text("'Dance HUD', an object owned by Choreographer Resident, would like to:"),
        ],
        lines_header: cx.text(""),
        permission_lines: vec![
            cx.text("\u{2022}\u{00a0}Act on your control inputs"),
            cx.text("\u{2022}\u{00a0}Animate your avatar"),
            cx.text("\u{2022}\u{00a0}Control your camera"),
        ],
        confirm: cx.text("Is this OK?"),
        grant_label: cx.text("Yes"),
        deny_label: cx.text("No"),
        block_label: Some(cx.text("Block")),
    };
    let card = build_script_permission_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, SCRIPT_PERMISSION_ELEMENT);
    card.root
}

/// The gallery / `ui_test` specimen (caution card): a static money-access request
/// with additional permissions, so the caution warning / footer / action layout
/// is swept.
pub(crate) fn spawn_script_permission_caution_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = ScriptPermissionContent {
        caution: true,
        intro: vec![
            cx.text(
                "The object 'Tip Jar' wants access to take money from your Linden Dollar \
                 account. If you allow this, it can take any or all of your money from you at \
                 any time, with no further warning or request.",
            ),
            cx.text(
                "Before allowing this access, make sure you know what the object is and why it \
                 is making this request, as well as whether you trust the creator. If you're \
                 not certain, click Deny.",
            ),
        ],
        lines_header: cx.text("It is also requesting the following permissions:"),
        permission_lines: vec![cx.text("\u{2022}\u{00a0}Animate your avatar")],
        confirm: cx.text(""),
        grant_label: cx.text("Allow access"),
        deny_label: cx.text("Deny"),
        block_label: None,
    };
    let card = build_script_permission_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, SCRIPT_PERMISSION_CAUTION_ELEMENT);
    card.root
}

/// Wire a specimen card's buttons to inert [`UiAction`]s (the registry rule: a
/// specimen reaches no session), keyed by the given element id.
fn wire_specimen_actions(
    commands: &mut Commands,
    card: &ScriptPermissionCard,
    element: &'static str,
) {
    for (button, action) in [
        (Some(card.grant), "grant"),
        (Some(card.deny), "deny"),
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
    use super::{
        PERMISSION_QUESTIONS, is_caution, known_mask, other_permission_keys, recognized_mask,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::ScriptPermissions;

    /// The recognised mask keeps the modelled requested bits and drops the rest:
    /// an unmodelled bit (here `RemapControlInputs`, `1 << 3`) is stripped, and a
    /// request of only unmodelled bits recognises nothing.
    #[test]
    fn recognized_mask_drops_unmodelled_bits() {
        let take_controls = ScriptPermissions(ScriptPermissions::TAKE_CONTROLS);
        assert_eq!(
            recognized_mask(take_controls),
            ScriptPermissions::TAKE_CONTROLS
        );

        // TAKE_CONTROLS (modelled) OR RemapControlInputs `1 << 3` (unmodelled).
        let mixed = ScriptPermissions(ScriptPermissions::TAKE_CONTROLS | (1 << 3));
        assert_eq!(recognized_mask(mixed), ScriptPermissions::TAKE_CONTROLS);

        // A request of only the unmodelled bit recognises nothing.
        assert_eq!(recognized_mask(ScriptPermissions(1 << 3)), 0);
    }

    /// `is_caution` is exactly "the recognised request asks to debit", so a debit
    /// request (alone or combined) is caution and a non-debit request is not.
    #[test]
    fn caution_tracks_the_debit_bit() {
        assert!(is_caution(ScriptPermissions(ScriptPermissions::DEBIT)));
        assert!(is_caution(ScriptPermissions(
            ScriptPermissions::DEBIT | ScriptPermissions::TRIGGER_ANIMATION
        )));
        assert!(!is_caution(ScriptPermissions(
            ScriptPermissions::TRIGGER_ANIMATION
        )));
        // An unmodelled-only request is never caution.
        assert!(!is_caution(ScriptPermissions(1 << 3)));
    }

    /// The listed permission keys exclude `DEBIT` (money is the caution headline),
    /// keep table order, and drop unmodelled bits.
    #[test]
    fn other_permission_keys_exclude_debit_and_keep_order() {
        let request = ScriptPermissions(
            ScriptPermissions::DEBIT
                | ScriptPermissions::CONTROL_CAMERA
                | ScriptPermissions::TAKE_CONTROLS
                | (1 << 3),
        );
        // TAKE_CONTROLS precedes CONTROL_CAMERA in the table; DEBIT and the
        // unmodelled bit are absent.
        assert_eq!(
            other_permission_keys(request),
            vec![
                "script-permission-q-controls",
                "script-permission-q-control-camera",
            ]
        );

        // A debit-only request lists no other permissions.
        assert!(other_permission_keys(ScriptPermissions(ScriptPermissions::DEBIT)).is_empty());
    }

    /// The known mask is the union of every table bit, and every table entry has a
    /// distinct bit (no duplicate rows).
    #[test]
    fn known_mask_unions_the_table() {
        let expected = PERMISSION_QUESTIONS
            .iter()
            .fold(0, |mask, (bit, _key)| mask | bit);
        assert_eq!(known_mask(), expected);
        // Every row contributes a new bit: the popcount equals the row count.
        assert_eq!(
            known_mask().count_ones(),
            u32::try_from(PERMISSION_QUESTIONS.len()).unwrap_or(0)
        );
    }
}
