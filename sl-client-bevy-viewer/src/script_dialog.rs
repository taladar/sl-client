//! The **script-dialog toast host** (`viewer-dialog-lldialog`): the panel a
//! scripted object's `llDialog` / `llTextBox` pops, mirroring the reference
//! `LLToastNotifyPanel` in its script-dialog mode.
//!
//! # What it renders
//!
//! When a scripted object calls `llDialog` (or `llTextBox`), the simulator sends a
//! `ScriptDialog` message. This host decodes it
//! ([`SlSessionEvent::ScriptDialog`]) and raises a card into the **shared
//! notification-host channel** ([`crate::notification_host`]) — top-trailing,
//! priority-ordered, overflow-cycled — with:
//!
//! - a **title** line naming the object and its owner
//!   (`Owner Name's 'Object Name'`, the reference `[NAME]'s '[TITLE]'`);
//! - the dialog **message**;
//! - the interaction, one of two shapes:
//!   - **buttons** — up to twelve, in a three-column grid filled **bottom-up**
//!     (button 0 at the bottom-left, the reference "we arrange buttons from bottom
//!     to top for backward support of old scripts"); or
//!   - a **text field** for an `llTextBox` ([`ScriptDialog::is_text_box`]), with a
//!     Submit button;
//! - the two built-in **Block** (mute the object) and **Ignore** (dismiss) actions
//!   the reference `ScriptDialog` form carries.
//!
//! A card **sticks** ([`NotificationKind::Alert`]) until the user answers it —
//! clicking a script button (or Submit) sends the reply
//! ([`Command::ReplyScriptDialog`]) on the dialog's hidden chat channel and tears
//! the card down; Ignore and the close **×** tear it down with no reply. Unlike a
//! group notice a script dialog is **not persisted** across a relog: the script's
//! outstanding dialog does not survive the session, so a stored card would reply
//! into the void.
//!
//! # Links in the body are deferred
//!
//! The message is rendered as plain text: turning its URLs / SLURLs into clickable
//! links is the shared linkification layer, tracked for the script dialog as
//! [[viewer-script-dialog-body-links]] (the sibling of the group-notice
//! [[viewer-group-notice-body-links]]).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use sl_client_bevy::{
    ChatChannel, Command, MuteType, ObjectKey, ScriptDialog, SlCommand, SlEvent, SlSessionEvent,
};

use crate::i18n::{TransArgs, Translator};
use crate::linkified_text::{LinkTextStyle, spawn_linkified_text};
use crate::mutes::RequestBlock;
use crate::notification_host::{NotificationChannelRoot, ResolveNotification, adopt_toast};
use crate::notifications::{
    NotificationId, NotificationKind, NotificationManager, NotificationPriority,
};
use crate::ui::{column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The catalogue-template sentinel a script-dialog toast reports as (it is not a
/// real [`crate::notifications::NOTIFICATIONS`] entry — the card is bespoke — but
/// the shared toast machinery wants a stable name for its history / response
/// bookkeeping).
const SCRIPT_DIALOG_TEMPLATE: &str = "ScriptDialog";

/// The element id the buttons gallery specimen and its inert actions report under.
const SCRIPT_DIALOG_ELEMENT: &str = "script-dialog-toast";

/// The element id the text-box gallery specimen reports under.
const SCRIPT_TEXTBOX_ELEMENT: &str = "script-dialog-textbox-toast";

/// The skin class a card wears (`.sk-toast`), so the card inherits the toast
/// surface styling shared with the catalogue toasts and the group-notice card.
const CARD_CLASS: &str = "sk-toast";

/// The skin class the title / body text wears (`.sk-toast-text`).
const TEXT_CLASS: &str = "sk-toast-text";

/// The skin class a card button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The close-button glyph (a multiplication sign), matching the reference toast.
const CLOSE_GLYPH: &str = "\u{00d7}";

/// The most script buttons a dialog can carry (`SCRIPT_DIALOG_MAX_BUTTONS`); a
/// simulator never sends more, but a malformed message is clamped so the grid
/// stays bounded.
const MAX_SCRIPT_BUTTONS: usize = 12;

/// The script buttons' grid width, in columns — the reference three-wide layout.
const GRID_COLUMNS: usize = 3;

/// A card's widest allowed width, in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

/// A card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// A card's border width, in logical pixels — the script-dialog accent is painted
/// on it.
const CARD_BORDER: f32 = 2.0;

/// The gap between a card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 6.0;

/// The gap between grid buttons, in logical pixels.
const BUTTON_GAP: f32 = 6.0;

/// The card body / button text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The title line's text size, in logical pixels.
const TITLE_FONT_SIZE: f32 = 15.0;

/// The width bound for a full-width text line (title / body), spanning the card
/// content width less its padding and border — so a wrapped paragraph is the sole
/// inline occupant of a decoration-free box (the
/// `viewer-text-node-padding-measure` constraint).
const FULL_TEXT_MAX_WIDTH: f32 = CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER;

/// One grid button's fixed width, in logical pixels — three plus their gaps fit
/// the card content width, so the columns line up like the reference grid. The
/// literal three is kept in sync with [`GRID_COLUMNS`] by a compile-time assert.
const GRID_BUTTON_WIDTH: f32 = (FULL_TEXT_MAX_WIDTH - BUTTON_GAP * 2.0) / 3.0;

/// A card's fallback background, used when no skin is loaded — the skin's
/// `.sk-toast` (`var(--surface-bg)`) overrides it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// A card's fallback body text colour — the skin's `.sk-toast-text` overrides it.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (the object / owner title line).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The script-dialog accent painted on a card's border and its default button — a
/// teal distinct from the group-notice blue, so the two cards read apart.
const ACCENT_COLOR: Color = Color::srgb(0.40, 0.78, 0.66);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The plugin: drives the script-dialog cards into the shared notification channel.
pub(crate) struct ScriptDialogPlugin;

impl Plugin for ScriptDialogPlugin {
    /// Ingest received `ScriptDialog` messages into the shared toast channel.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, ingest_script_dialogs);
    }
}

/// Read the event stream; for each received `ScriptDialog`, build its card and
/// raise it into the shared toast channel — so a script dialog stacks, orders and
/// overflow-cycles alongside the catalogue notifications and the group-notice card
/// ([`crate::notification_host`]).
fn ingest_script_dialogs(
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
        let SlSessionEvent::ScriptDialog(dialog) = &event.0 else {
            continue;
        };
        spawn_script_dialog_card(&mut commands, &channel, &mut manager, dialog, &translator);
    }
}

/// Which interaction a script-dialog card offers: a button grid, or a free-text
/// field for an `llTextBox`.
enum DialogForm {
    /// The script buttons, in message order (`buttons[0]` renders bottom-left).
    Buttons(Vec<String>),
    /// An `llTextBox` free-text prompt.
    TextBox,
}

/// The resolved content of one script-dialog card, ready to render — the live path
/// resolves the decoded dialog + i18n into this; the gallery specimen builds it
/// from literals, so both render through the one [`build_script_dialog_card`] (the
/// registry rule, [`crate::ui_element`]).
struct ScriptDialogContent {
    /// The title line naming the object and its owner.
    title: String,
    /// The dialog message (plain text; linkification deferred).
    message: String,
    /// The interaction the card offers.
    form: DialogForm,
    /// The Block (mute) button label.
    block_label: String,
    /// The Ignore (dismiss) button label.
    ignore_label: String,
    /// The Submit button label (used only for a [`DialogForm::TextBox`]).
    submit_label: String,
}

/// The entities [`build_script_dialog_card`] produced that a caller wires: the
/// card root, the script buttons (each with its message index and label), the
/// text field + Submit box (for an `llTextBox`), and the Block / Ignore / close
/// boxes.
struct ScriptDialogCard {
    /// The card root node (left with no parent — the caller adopts / parents it).
    root: Entity,
    /// Each script button box, paired with its message index and label (the reply
    /// carries both).
    buttons: Vec<(Entity, i32, String)>,
    /// The `llTextBox` field, when the dialog is a text prompt.
    text_field: Option<Entity>,
    /// The Submit button box, when the dialog is a text prompt.
    submit: Option<Entity>,
    /// The Block (mute) button box.
    block: Entity,
    /// The Ignore (dismiss) button box.
    ignore: Entity,
    /// The close (×) button box.
    close: Entity,
}

/// Build a script-dialog card's node tree from resolved [`ScriptDialogContent`],
/// returning the entities a caller wires. The **root is left with no parent**: the
/// live host adopts it into the shared toast channel via [`adopt_toast`], the
/// gallery specimen parents it under its cell.
fn build_script_dialog_card(
    commands: &mut Commands,
    content: &ScriptDialogContent,
) -> ScriptDialogCard {
    let root = commands
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
            Name::new("script-dialog-card"),
        ))
        .id();

    // Close (×), top-trailing — the early-dismiss affordance.
    let close = spawn_close_button(commands, root);

    // The title line (dim, secondary) then the message (primary), each a
    // width-bounded box so a long name / paragraph wraps within the card.
    spawn_bounded_text(
        commands,
        root,
        content.title.clone(),
        TITLE_FONT_SIZE,
        DIM_TEXT_COLOR,
    );
    // The message is linkified — its http(s) URLs / SLURLs render as clickable
    // links (viewer-script-dialog-body-links), exactly as nearby chat and the
    // other toast bodies do.
    spawn_bounded_linked_text(
        commands,
        root,
        content.message.clone(),
        FONT_SIZE,
        TEXT_COLOR,
    );

    // The interaction: a button grid, or the text field.
    let mut buttons = Vec::new();
    let mut text_field = None;
    match &content.form {
        DialogForm::Buttons(labels) => {
            buttons = spawn_button_grid(commands, root, labels);
        }
        DialogForm::TextBox => {
            text_field = Some(spawn_text_input(
                commands,
                root,
                &TextInputSpec {
                    tab_index: 1,
                    width_glyphs: 28.0,
                    // `llTextBox` caps the reply at 254 characters.
                    max_characters: Some(254),
                    ..TextInputSpec::new("script-dialog-textbox", TextInputKind::Line)
                },
            ));
        }
    }

    // The bottom action row: Submit (text box only), then the built-in Block and
    // Ignore, trailing-aligned.
    let action_row = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(BUTTON_GAP),
                justify_content: JustifyContent::End,
                ..row(Val::Px(BUTTON_GAP))
            },
            Name::new("script-dialog-actions"),
            ChildOf(root),
        ))
        .id();
    let submit = matches!(content.form, DialogForm::TextBox).then(|| {
        // Submit is the default action of a text prompt, so it wears the accent.
        spawn_action_button(commands, action_row, &content.submit_label, true, 2)
    });
    let block = spawn_action_button(commands, action_row, &content.block_label, false, 3);
    let ignore = spawn_action_button(commands, action_row, &content.ignore_label, false, 4);

    ScriptDialogCard {
        root,
        buttons,
        text_field,
        submit,
        block,
        ignore,
        close,
    }
}

/// Build one script-dialog card from a decoded [`ScriptDialog`], adopt it into the
/// shared toast channel, and wire the live actions. The card is an
/// [`Alert`](NotificationKind::Alert): it **sticks** (never auto-fades) and only
/// leaves when the user answers it — a script button / Submit sends the reply on
/// the hidden chat channel, Ignore / × tears it down with no reply, Block mutes
/// the object.
fn spawn_script_dialog_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    dialog: &ScriptDialog,
    translator: &Translator,
) -> NotificationId {
    let content = ScriptDialogContent {
        title: translator.format(
            "script-dialog-from",
            &TransArgs::new()
                .text("owner", &owner_display(dialog))
                .text("object", &dialog.object_name),
        ),
        message: dialog.message.clone(),
        form: dialog_form(dialog),
        block_label: translator.get("script-dialog-button-block"),
        ignore_label: translator.get("script-dialog-button-ignore"),
        submit_label: translator.get("script-dialog-button-submit"),
    };
    let card = build_script_dialog_card(commands, &content);

    // Adopt the card into the shared toast channel so it stacks / orders /
    // overflow-cycles with the catalogue notifications. An `Alert` never
    // auto-expires — only a user answer ends it. Not persisted: the script's
    // outstanding dialog does not survive a relog.
    let id = adopt_toast(
        commands,
        manager,
        channel,
        card.root,
        NotificationKind::Alert,
        NotificationPriority::Normal,
        SCRIPT_DIALOG_TEMPLATE,
        None,
        content.message.clone(),
    );

    let root = card.root;
    let object_id = dialog.object_id;
    let chat_channel = dialog.chat_channel;

    // Each script button: reply with its index / label on the hidden channel and
    // tear the card down (a chosen button ends the dialog).
    for (button, index, label) in card.buttons {
        commands.entity(button).observe(
            move |_activate: On<Activate>,
                  mut sl: MessageWriter<SlCommand>,
                  mut resolves: MessageWriter<ResolveNotification>| {
                reply_script_button(&mut sl, object_id, chat_channel, index, label.clone());
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }

    // Submit (text box): reply with the typed text on index 0 and tear the card
    // down. Reads the field's live value at click time.
    if let (Some(field), Some(submit)) = (card.text_field, card.submit) {
        commands.entity(submit).observe(
            move |_activate: On<Activate>,
                  fields: Query<&EditableText>,
                  mut sl: MessageWriter<SlCommand>,
                  mut resolves: MessageWriter<ResolveNotification>| {
                let typed = fields
                    .get(field)
                    .map(|editable| editable.value().to_string())
                    .unwrap_or_default();
                reply_script_button(&mut sl, object_id, chat_channel, 0, typed);
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }

    // Block: mute the object, then tear the card down (no script reply).
    let object_name = dialog.object_name.clone();
    commands.entity(card.block).observe(
        move |_activate: On<Activate>,
              mut blocks: MessageWriter<RequestBlock>,
              mut resolves: MessageWriter<ResolveNotification>| {
            blocks.write(RequestBlock::new(
                object_id.uuid(),
                object_name.clone(),
                MuteType::Object,
            ));
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Ignore / close ×: tear the card down with no reply — the reference "Ignore"
    // dismisses the dialog without answering the script.
    for button in [card.ignore, card.close] {
        commands.entity(button).observe(
            move |_activate: On<Activate>, mut resolves: MessageWriter<ResolveNotification>| {
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }
    id
}

/// Queue a `ScriptDialogReply` for the chosen `button_index` / `button_label` on
/// the dialog's hidden `chat_channel` — the shared reply for a button click and an
/// `llTextBox` Submit (which passes the typed text as the label on index 0).
fn reply_script_button(
    sl: &mut MessageWriter<SlCommand>,
    object_id: ObjectKey,
    chat_channel: ChatChannel,
    button_index: i32,
    button_label: String,
) {
    sl.write(SlCommand(Command::ReplyScriptDialog {
        object_id,
        chat_channel,
        button_index,
        button_label,
    }));
}

/// The owner display for a dialog's title: the owner's `First Last` name, or — for
/// an object owned by a group (the sim leaves the first name empty and puts the
/// group name in the last-name field) — the group name.
fn owner_display(dialog: &ScriptDialog) -> String {
    if dialog.owner_first_name.is_empty() {
        dialog.owner_last_name.clone()
    } else {
        format!("{} {}", dialog.owner_first_name, dialog.owner_last_name)
            .trim()
            .to_owned()
    }
}

/// The interaction a decoded dialog offers: a text field for an `llTextBox`, else
/// its buttons (clamped to [`MAX_SCRIPT_BUTTONS`]). A dialog with no buttons falls
/// back to a single `OK`, matching the reference `addDefaultButton`.
fn dialog_form(dialog: &ScriptDialog) -> DialogForm {
    if dialog.is_text_box() {
        return DialogForm::TextBox;
    }
    if dialog.buttons.is_empty() {
        return DialogForm::Buttons(vec!["OK".to_owned()]);
    }
    let labels = dialog
        .buttons
        .iter()
        .take(MAX_SCRIPT_BUTTONS)
        .cloned()
        .collect();
    DialogForm::Buttons(labels)
}

/// The message-index groups for the button grid's display rows, top row first: the
/// buttons chunked [`GRID_COLUMNS`] wide and the chunks **reversed**, so the first
/// chunk (`buttons[0..3]`) becomes the bottom row and `buttons[0]` lands
/// bottom-left — the reference bottom-up fill.
fn bottom_up_rows(count: usize) -> Vec<Vec<usize>> {
    let mut chunks: Vec<Vec<usize>> = (0..count)
        .collect::<Vec<usize>>()
        .chunks(GRID_COLUMNS)
        .map(<[usize]>::to_vec)
        .collect();
    chunks.reverse();
    chunks
}

/// Spawn the script-button grid under `card`: rows of [`GRID_COLUMNS`], filled
/// bottom-up ([`bottom_up_rows`]). Returns each button box with its message index
/// and label for the caller to wire the reply onto.
fn spawn_button_grid(
    commands: &mut Commands,
    card: Entity,
    labels: &[String],
) -> Vec<(Entity, i32, String)> {
    let grid = commands
        .spawn((
            Node {
                align_items: AlignItems::Stretch,
                ..column(Val::Px(BUTTON_GAP))
            },
            Name::new("script-dialog-buttons"),
            ChildOf(card),
        ))
        .id();
    let mut wired = Vec::new();
    for indices in bottom_up_rows(labels.len()) {
        let button_row = commands
            .spawn((
                Node {
                    row_gap: Val::Px(BUTTON_GAP),
                    ..row(Val::Px(BUTTON_GAP))
                },
                Name::new("script-dialog-button-row"),
                ChildOf(grid),
            ))
            .id();
        for index in indices {
            let Some(label) = labels.get(index) else {
                continue;
            };
            let tab = i32::try_from(index).unwrap_or(0).saturating_add(5);
            let button = spawn_grid_button(commands, button_row, label, tab);
            let message_index = i32::try_from(index).unwrap_or(0);
            wired.push((button, message_index, label.clone()));
        }
    }
    wired
}

/// Spawn one fixed-width grid button, its label the sole occupant of a bounded box
/// (measure-safe) so a long label wraps within the button rather than widening the
/// grid. Returns the clickable box for the caller to wire an observer onto.
fn spawn_grid_button(commands: &mut Commands, parent: Entity, label: &str, tab: i32) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                width: Val::Px(GRID_BUTTON_WIDTH),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BUTTON_BORDER),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new(format!("script-dialog-button:{label}")),
            ChildOf(parent),
        ))
        .id();
    let label_box = commands
        .spawn((
            Node {
                max_width: Val::Px(GRID_BUTTON_WIDTH - 16.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        Text::new(label.to_owned()),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(label_box),
    ));
    button
}

/// Spawn one bottom-row action button (Submit / Block / Ignore), accent-bordered
/// when it is the default. Returns the clickable box for the caller to wire onto.
fn spawn_action_button(
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
            Name::new(format!("script-dialog-action:{label}")),
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
            Name::new("script-dialog-close-row"),
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
            Name::new("script-dialog-close"),
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
                max_width: Val::Px(FULL_TEXT_MAX_WIDTH),
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
/// its URLs / SLURLs render as clickable links and a long message still wraps
/// within the card. An empty string spawns nothing.
fn spawn_bounded_linked_text(
    commands: &mut Commands,
    parent: Entity,
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
                max_width: Val::Px(FULL_TEXT_MAX_WIDTH),
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

/// The gallery / `ui_test` specimen (buttons): a static script-dialog card with a
/// five-button grid, so the three-column bottom-up layout is swept login-free (a
/// live dialog needs a scripted object). Registered in
/// [`crate::ui_element::ELEMENTS`]; its buttons report an inert [`UiAction`].
pub(crate) fn spawn_script_dialog_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = ScriptDialogContent {
        title: cx.text("Board Member's 'Vendor'"),
        message: cx.text(SPECIMEN_MESSAGE),
        form: DialogForm::Buttons(vec![
            cx.text("Buy"),
            cx.text("Info"),
            cx.text("Cancel"),
            cx.text("Gift"),
            cx.text("Redeliver"),
        ]),
        block_label: cx.text("Block"),
        ignore_label: cx.text("Ignore"),
        submit_label: cx.text("Submit"),
    };
    let card = build_script_dialog_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, SCRIPT_DIALOG_ELEMENT);
    card.root
}

/// The gallery / `ui_test` specimen (text box): a static `llTextBox` card — a
/// text field plus Submit / Block / Ignore — so the text-prompt layout is swept.
pub(crate) fn spawn_script_textbox_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = ScriptDialogContent {
        title: cx.text("Sign Poster's 'Guest Book'"),
        message: cx.text("Please type your name for the guest book:"),
        form: DialogForm::TextBox,
        block_label: cx.text("Block"),
        ignore_label: cx.text("Ignore"),
        submit_label: cx.text("Submit"),
    };
    let card = build_script_dialog_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card, SCRIPT_TEXTBOX_ELEMENT);
    card.root
}

/// Wire a specimen card's buttons to inert [`UiAction`]s (the registry rule: a
/// specimen reaches no session), keyed by the given element id.
fn wire_specimen_actions(commands: &mut Commands, card: &ScriptDialogCard, element: &'static str) {
    for (button, _index, _label) in &card.buttons {
        commands.entity(*button).observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element,
                    action: "button",
                });
            },
        );
    }
    for (button, action) in [
        (card.submit, "submit"),
        (Some(card.block), "block"),
        (Some(card.ignore), "ignore"),
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

/// The buttons specimen's message prose — long enough to force the wrap the matrix
/// sweeps.
const SPECIMEN_MESSAGE: &str = "Welcome to the shop! Choose an option below to buy this item, get \
    more information, or send it as a gift to a friend.";

/// Compile-time guard: the grid button width stays positive after the gaps are
/// subtracted — a non-positive width would collapse every button to nothing.
const _: () = assert!(
    GRID_BUTTON_WIDTH > 0.0,
    "script-dialog grid button width must stay positive"
);

/// Compile-time guard: the [`GRID_BUTTON_WIDTH`] literal assumes three columns, so
/// a change to [`GRID_COLUMNS`] must revisit that width.
const _: () = assert!(
    GRID_COLUMNS == 3,
    "grid button width literal assumes three columns"
);

#[cfg(test)]
mod tests {
    use super::{DialogForm, GRID_COLUMNS, bottom_up_rows, dialog_form, owner_display};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ChatChannel, ObjectKey, ScriptDialog, TextureKey, Uuid};

    /// A minimal dialog with the given owner-name and buttons, for the helper tests.
    fn dialog(first: &str, last: &str, buttons: &[&str]) -> ScriptDialog {
        ScriptDialog {
            object_id: ObjectKey::from(Uuid::from_u128(0x5c01)),
            object_name: "Vendor".to_owned(),
            owner_first_name: first.to_owned(),
            owner_last_name: last.to_owned(),
            owner_id: None,
            message: "Pick one".to_owned(),
            chat_channel: ChatChannel(-42),
            image_id: TextureKey::from(Uuid::nil()),
            buttons: buttons.iter().map(|button| (*button).to_owned()).collect(),
        }
    }

    /// The grid fills bottom-up: `buttons[0]` lands in the bottom (last-displayed)
    /// row's leading slot, and a partial final chunk becomes the top row.
    #[test]
    fn grid_rows_fill_bottom_up() {
        // Five buttons over three columns: top row [3, 4], bottom row [0, 1, 2].
        assert_eq!(bottom_up_rows(5), vec![vec![3, 4], vec![0, 1, 2]]);
        // An exact two rows: top [3, 4, 5], bottom [0, 1, 2].
        assert_eq!(bottom_up_rows(6), vec![vec![3, 4, 5], vec![0, 1, 2]]);
        // A single short row is both the top and the bottom.
        assert_eq!(bottom_up_rows(2), vec![vec![0, 1]]);
        // No buttons yield no rows.
        assert!(bottom_up_rows(0).is_empty());
    }

    /// Every grid row holds at most [`GRID_COLUMNS`] buttons, and the union of all
    /// rows is exactly the input indices (nothing dropped or duplicated).
    #[test]
    fn grid_rows_cover_every_index() {
        let rows = bottom_up_rows(12);
        assert!(rows.iter().all(|row| row.len() <= GRID_COLUMNS));
        let mut flat: Vec<usize> = rows.into_iter().flatten().collect();
        flat.sort_unstable();
        assert_eq!(flat, (0..12).collect::<Vec<usize>>());
    }

    /// The owner display is `First Last` for an agent, and the group name (the
    /// last-name field) for a group-owned object (empty first name).
    #[test]
    fn owner_display_handles_agent_and_group() {
        assert_eq!(
            owner_display(&dialog("Bob", "Resident", &[])),
            "Bob Resident"
        );
        assert_eq!(
            owner_display(&dialog("", "Builders United", &[])),
            "Builders United"
        );
    }

    /// The button labels of a form, or `None` for a text-box form — a panic-free
    /// projection so the classification tests can assert with `assert_eq!`.
    fn form_labels(form: DialogForm) -> Option<Vec<String>> {
        match form {
            DialogForm::Buttons(labels) => Some(labels),
            DialogForm::TextBox => None,
        }
    }

    /// A text-box dialog picks the field form; a buttons dialog keeps its labels;
    /// an empty-button dialog falls back to a lone `OK`.
    #[test]
    fn dialog_form_classifies_the_three_shapes() {
        let text = dialog("Sign", "Poster", &[ScriptDialog::TEXT_BOX_BUTTON]);
        assert!(form_labels(dialog_form(&text)).is_none());

        let buttons = dialog("Bob", "Resident", &["Yes", "No"]);
        assert_eq!(
            form_labels(dialog_form(&buttons)),
            Some(vec!["Yes".to_owned(), "No".to_owned()])
        );

        let empty = dialog("Bob", "Resident", &[]);
        assert_eq!(
            form_labels(dialog_form(&empty)),
            Some(vec!["OK".to_owned()])
        );
    }

    /// More than twelve buttons are clamped to the reference maximum.
    #[test]
    fn dialog_form_clamps_button_count() {
        let many = vec!["Btn"; 20];
        let dialog = dialog("Bob", "Resident", &many);
        assert_eq!(
            form_labels(dialog_form(&dialog)).map(|labels| labels.len()),
            Some(super::MAX_SCRIPT_BUTTONS)
        );
    }
}
