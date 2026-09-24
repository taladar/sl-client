//! The **Groups list** (`viewer-social-groups`), hosted in the **Groups** sub-tab
//! of the [People pane](crate::people) inside the [Conversations
//! floater](crate::conversations).
//!
//! # Why it lives in the People pane's Groups sub-tab
//!
//! The reference viewer's **Vintage** skin folds People / Contacts into the
//! Conversations floater: its Contacts floater is a `tab_container` with Friends /
//! Groups / Contact Sets tabs, and the group list is the Groups tab (the reusable
//! `LLGroupList` widget with a bottom action bar). [`crate::people`] already builds
//! that Friends / Groups horizontal sub-tab strip and owns the empty Groups content
//! slot; this module fills that slot with the group **list**, laid out like the
//! Friends list beside it — a virtualized list of the member's own groups plus a
//! trailing column of per-group actions.
//!
//! # Scope of this task
//!
//! The **list** and its **Info / IM / Activate / Leave** actions are built here.
//! IM opens the group's chat tab, Activate sets the worn group and Leave (behind a
//! confirm) leaves it; **Info** opens the group [profile
//! floater](crate::group_profile) (general / members / roles / notices), which is
//! a separate module.
//!
//! # Model + ECS mirror
//!
//! [`GroupsModel`] is a plain, unit-tested resource fed **only** from the
//! [`SlEvent`] stream — the agent's memberships
//! ([`SlSessionEvent::GroupMemberships`], pushed on login and whenever membership
//! changes), the active group ([`SlSessionEvent::ActiveGroupChanged`]), and the
//! drop / leave lifecycle events — mirroring [`crate::people`]'s pure model. Unlike
//! the friends list it needs no name-resolution pass: each membership record
//! already carries its group name. `GroupsView` is the ordered, render-ready
//! projection the virtualized list ([`crate::virtual_list`]) binds its recycled
//! rows to.
//!
//! # Sharing the pane with the People surface
//!
//! The list is built into `crate::people::PeopleUi::groups_content`, the slot the
//! People pane already toggles between its Friends and Groups sub-tabs. This module
//! never touches that visibility — it only owns what is *inside* the Groups slot.
//! The IM action hands the strip back to a conversation the same way the Friends
//! list does, via [`crate::intents::OpenConversation`].
//!
//! Reference (Firestorm, read-only): `llgrouplist`, `llgroupactions`,
//! Vintage `panel_fs_contacts_groups`.

use crate::skin::{
    ACTION_BUTTON_CLASS, ACTIVE_CLASS, ACTIVE_TEXT_CLASS, DisabledButtons, LIST_SURFACE_CLASS,
    TEXT_CLASS, set_action_button_enabled, set_state_class, set_state_class_on, text_role,
};
use crate::skin_palette::SkinPalette;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{Command, GroupKey, SlCommand, SlEvent, SlSessionEvent};

use crate::i18n::{TransArgs, Translated, Translator};
use crate::intents::OpenGroupProfile;
use crate::intents::{ConversationKey, OpenConversation};
use crate::people::PeopleUi;
use crate::social::{GroupChoice, GroupRow, GroupsModel, short_id};
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_spawn::{self, ButtonSpec, UiLabel};
use crate::ui_text::set_text;
use crate::virtual_list::{
    VirtualList, VirtualRow, VirtualViewport, amend_row_node, layout_virtual_lists,
    spawn_virtual_scrollbar,
};
use sl_viewer_ui_core::glyph;

/// A group-list row's uniform height, in logical pixels — matched to the friends
/// list beside it so the whole pane reads as one surface.
const ROW_HEIGHT: f32 = 22.0;

/// The chrome / label font size, in logical pixels (headers, buttons).
const CHROME_FONT_SIZE: f32 = 13.0;

/// A group row's font size, in logical pixels.
const ROW_FONT_SIZE: f32 = 13.0;

/// The width of the trailing "Active" column (the active-group marker), in logical
/// pixels — wide enough to sit its "Active" header above it.
const ACTIVE_COL_WIDTH: f32 = 56.0;

/// The width of the trailing action-button column, in logical pixels — enough for
/// the longest label ("Activate") at the chrome font size.
const ACTION_COL_WIDTH: f32 = 96.0;

/// A group / label's text colour.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// An action button's background.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The table header row's background — a recessed strip above the list.
const HEADER_BACKGROUND: Color = Color::srgb(0.14, 0.17, 0.22);

/// The table header / count text colour — dim, so it reads as chrome.
const HEADER_TEXT_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// The longest gap between two clicks on the same row still counted as a
/// double-click, in seconds — a double-click opens the group's IM, like the IM
/// button (the reference viewer's list double-click).
const DOUBLE_CLICK_SECS: f32 = 0.4;

/// The modal dim behind the leave-confirm dialog.
const CONFIRM_SCRIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

/// The confirm dialog box's background.
const CONFIRM_BOX_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// The confirm dialog box's border — a warning accent, since leaving a group is
/// not casually reversible (re-joining may need an invite or a fee).
const CONFIRM_BOX_BORDER: Color = Color::srgb(0.62, 0.44, 0.20);

/// The confirm dialog's Leave button background (a cautionary red).
const CONFIRM_LEAVE_BACKGROUND: Color = Color::srgb(0.45, 0.22, 0.24);

/// The confirm dialog's Cancel button background.
const CONFIRM_CANCEL_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The z-order of the confirm modal — far above the floaters' bring-to-front
/// counter, so it is never occluded (matches [`crate::people`]'s confirm modal).
const CONFIRM_Z: i32 = 1_000_000;

/// The Fluent key for the groups-table "Name" column header.
const HEADER_NAME_KEY: &str = "groups-header-name";

/// The Fluent key for the groups-table "Active" column header.
const HEADER_ACTIVE_KEY: &str = "groups-header-active";

/// The Fluent key for the group-count line.
const COUNT_KEY: &str = "groups-count";

/// The Fluent key for the leading "wear no group" row's label.
const NONE_KEY: &str = "groups-none";

/// The Fluent key for the confirm dialog's leave prompt (arg `name`).
const LEAVE_CONFIRM_PROMPT_KEY: &str = "groups-leave-confirm-prompt";

/// The Fluent key for the confirm dialog's Leave button.
const LEAVE_CONFIRM_YES_KEY: &str = "groups-leave-confirm-yes";

/// The Fluent key for the confirm dialog's Cancel button.
const LEAVE_CONFIRM_NO_KEY: &str = "groups-leave-confirm-no";

// ---------------------------------------------------------------------------
// Pure model
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Row actions
// ---------------------------------------------------------------------------

/// A per-group action offered by the action column beside the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupAction {
    /// Open the group's [profile floater](crate::group_profile) for the selected
    /// group.
    Info,
    /// Open (and join) the group's IM chat tab.
    Im,
    /// Make this the agent's active (worn) group.
    Activate,
    /// Leave the group (behind a confirm dialog).
    Leave,
}

/// Every action the column offers, in the order it stacks them.
const ACTIONS: [GroupAction; 4] = [
    GroupAction::Info,
    GroupAction::Im,
    GroupAction::Activate,
    GroupAction::Leave,
];

impl GroupAction {
    /// The Fluent key for this action's button label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::Info => "groups-action-info",
            Self::Im => "groups-action-im",
            Self::Activate => "groups-action-activate",
            Self::Leave => "groups-action-leave",
        }
    }
}

/// Whether `action` can do anything for the current `selection` — the single
/// predicate both the button's greying and its press refusal read, so a greyed
/// button is exactly an inert one (Bevy's own disabled marker is advisory, and
/// would not stop the observer).
///
/// Mirrors the reference's `LLGroupList::onContextMenuItemEnable`: Info, IM and
/// Leave need a **real** group, so the "no group" row disables all three; every
/// row can be activated, except the one already worn, which activating would not
/// change. Nothing selected disables everything.
fn action_enabled(action: GroupAction, selection: Option<GroupChoice>, view: &GroupsView) -> bool {
    let Some(choice) = selection else {
        return false;
    };
    match action {
        GroupAction::Info | GroupAction::Im | GroupAction::Leave => {
            matches!(choice, GroupChoice::Group(_))
        }
        GroupAction::Activate => !view
            .rows
            .iter()
            .any(|row| row.group == choice && row.active),
    }
}

/// The wire [`Command`] an action produces for `choice`, or `None` for the
/// actions that are not a plain fire-and-forget command: [`GroupAction::Info`]
/// (which opens the profile floater via a message) and [`GroupAction::Im`] (which
/// opens a conversation tab and starts the session through separate paths). Pure
/// so the routing is unit-testable.
///
/// [`GroupChoice::NoGroup`] activates as `ActivateGroup(None)` — the only way to
/// wear no group and so carry no title. Leave produces nothing for it: there is
/// no group to leave. What the user may *reach* is [`action_enabled`]'s business;
/// this is only what the reachable action sends.
const fn group_command(action: GroupAction, choice: GroupChoice) -> Option<Command> {
    match action {
        GroupAction::Info | GroupAction::Im => None,
        GroupAction::Activate => Some(Command::ActivateGroup(choice.key())),
        GroupAction::Leave => match choice {
            GroupChoice::NoGroup => None,
            GroupChoice::Group(group) => Some(Command::LeaveGroup(group)),
        },
    }
}

// ---------------------------------------------------------------------------
// ECS side
// ---------------------------------------------------------------------------

/// The group-list UI entities — the ECS mirror of [`GroupsModel`], built into the
/// People pane's Groups content slot.
#[derive(Resource, Debug)]
pub(crate) struct GroupsUi {
    /// The virtualized group-list viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The group-count line under the list.
    count_text: Entity,
    /// The action column's buttons, each with the label node to dim alongside it
    /// — read by [`refresh_group_actions`] to grey the ones the current selection
    /// cannot support.
    action_buttons: Vec<ActionButton>,
    /// The leave-confirm modal overlay (shown while a leave is pending).
    confirm_overlay: Entity,
    /// The confirm modal's prompt text node (rewritten with the group's name).
    confirm_text: Entity,
}

/// One spawned action button: which action it fires, the button node to recolour
/// and the label node to dim with it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActionButton {
    /// The action this button fires.
    action: GroupAction,
    /// The button node. It carries `InteractionDisabled` when the selection
    /// cannot support the action; everything the refusal looks like is the
    /// skin's (`.sk-action-button:disabled`).
    button: Entity,
}

/// The ordered, render-ready groups projection the virtualized list binds to.
#[derive(Resource, Debug, Default)]
pub(crate) struct GroupsView {
    /// The rows in display order.
    rows: Vec<GroupRow>,
    /// The model revision this view was last built from.
    built_revision: u64,
}

/// The currently-selected row, which the action column acts on — `None` when
/// nothing is selected, which is distinct from a selected
/// [`GroupChoice::NoGroup`] row.
#[derive(Resource, Debug, Default)]
pub(crate) struct SelectedGroup(Option<GroupChoice>);

/// The last group-row click, for detecting a double-click (two presses on the same
/// group within [`DOUBLE_CLICK_SECS`] open its IM). Tracked by choice, not row
/// entity, since the virtualized rows are recycled.
#[derive(Resource, Debug, Default)]
pub(crate) struct GroupClickTracker {
    /// The row the last press selected, if any.
    group: Option<GroupChoice>,
    /// When that press landed, in seconds since startup ([`Time::elapsed_secs`]).
    time: f32,
}

/// A pending, not-yet-confirmed **leave** — leaving a group is destructive enough
/// to gate behind a confirm dialog (the reference does the same). `None` when no
/// confirm is open.
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingLeaveConfirm(Option<GroupKey>);

/// The choice a pooled row currently presents (so a press knows which to select),
/// or `None` when the row is parked.
#[derive(Component, Debug, Clone, Copy)]
struct BoundGroup(Option<GroupChoice>);

/// The persistent inner parts of a pooled group row, updated in place on bind.
#[derive(Component)]
struct GroupRowParts {
    /// The name label node.
    label: Entity,
    /// The active-marker glyph node (shown only for the active group).
    marker: Entity,
}

/// The Groups plugin: the model + view + selection resources, the deferred list
/// spawn (into the People pane), event ingest, selection, refresh, and row binding.
#[derive(Debug, Clone, Copy, Default)]
pub struct GroupsPlugin;

impl Plugin for GroupsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GroupsModel>()
            .init_resource::<GroupsView>()
            .init_resource::<SelectedGroup>()
            .init_resource::<GroupClickTracker>()
            .init_resource::<PendingLeaveConfirm>()
            .add_systems(
                Update,
                (
                    spawn_groups_panel.after(UiScaffoldSystems::SpawnRoot),
                    // Declared a member of the shared set so the world's name
                    // tags can order their group-title composition after it
                    // without naming a system in this crate, which sits above
                    // them.
                    ingest_group_events.in_set(sl_viewer_social::groups::GroupsSystems::Ingested),
                    rebuild_groups_view,
                    refresh_groups,
                    refresh_group_actions,
                    drive_leave_confirm,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (populate_group_rows, bind_group_rows)
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn (deferred until the People pane exists)
// ---------------------------------------------------------------------------

/// Spawn the group list into the People pane's Groups content slot, once
/// ([`GroupsUi`] absent) and only after that pane exists ([`PeopleUi`] present).
/// Runs each frame until it succeeds, then no-ops — the same robust deferral the
/// People pane uses to wait for the conversations floater.
fn spawn_groups_panel(
    mut commands: Commands,
    people: Option<Res<PeopleUi>>,
    groups: Option<Res<GroupsUi>>,
    root: Res<UiRoot>,
) {
    if groups.is_some() {
        return;
    }
    let Some(people) = people else {
        return;
    };
    let content = people.groups_content();

    // The body row: the list column takes the width, the action column sits at its
    // trailing edge (mirroring the Friends content layout).
    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::Px(6.0))
            },
            Name::new("groups-body"),
            ChildOf(content),
        ))
        .id();

    // The list column: a fixed table header, then the scrolling list under it.
    let list_column = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::ZERO)
            },
            Name::new("groups-list-column"),
            ChildOf(body),
        ))
        .id();
    spawn_groups_header(&mut commands, list_column);

    // The virtualized list viewport fills the remaining height and clips + owns its
    // own scroll, exactly like the friends viewport.
    let viewport = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..default()
            },
            // The rows' face, from the skin (`--list-bg`): the scrim this
            // used to paint by hand, plus the field-family text roles the
            // class re-roots for a light list. See `LIST_SURFACE_CLASS`.
            ClassList::new_with_classes([LIST_SURFACE_CLASS]),
            VirtualList::new(ROW_HEIGHT),
            VirtualViewport,
            Pickable::default(),
            TabIndex(2),
            Name::new("groups-viewport"),
            ChildOf(list_column),
        ))
        .observe(
            |press: On<Pointer<Press>>, ui: Res<GroupsUi>, mut focus: ResMut<InputFocus>| {
                if press.button == PointerButton::Primary {
                    focus.set(ui.viewport, FocusCause::Navigated);
                }
            },
        )
        .id();
    // The list's own scrollbar: the rows stop clear of it while it shows.
    spawn_virtual_scrollbar(&mut commands, viewport);

    // The count line under the list.
    let count_text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            text_role(HEADER_TEXT_COLOR),
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
            Name::new("groups-count"),
            ChildOf(list_column),
        ))
        .id();

    // The trailing action column — one button per [`GroupAction`], stacked and
    // acting on the current selection.
    let actions = commands
        .spawn((
            Node {
                width: Val::Px(ACTION_COL_WIDTH),
                flex_shrink: 0.0,
                align_items: AlignItems::Stretch,
                ..column(Val::Px(4.0))
            },
            Name::new("groups-actions"),
            ChildOf(body),
        ))
        .id();
    let action_buttons = ACTIONS
        .into_iter()
        .map(|action| spawn_action_button(&mut commands, actions, action))
        .collect();

    let (confirm_overlay, confirm_text) = spawn_leave_confirm_modal(&mut commands, root.0);

    commands.insert_resource(GroupsUi {
        viewport,
        count_text,
        action_buttons,
        confirm_overlay,
        confirm_text,
    });
}

/// Spawn the group-list table header: a "Name" column over the row labels and a
/// fixed "Active" column over the active markers. Static labels — unlike the
/// friends table, the group list is not column-sortable in this task.
fn spawn_groups_header(commands: &mut Commands, list_column: Entity) {
    let header = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                column_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(HEADER_BACKGROUND),
            Name::new("groups-header"),
            ChildOf(list_column),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(ROW_FONT_SIZE),
        text_role(HEADER_TEXT_COLOR),
        Translated::new(HEADER_NAME_KEY),
        Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(header),
    ));
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(ROW_FONT_SIZE),
        text_role(HEADER_TEXT_COLOR),
        Translated::new(HEADER_ACTIVE_KEY),
        Node {
            width: Val::Px(ACTIVE_COL_WIDTH),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(header),
    ));
}

/// Spawn one action-column button wired to `action`, acting on the current
/// selection: Info opens the profile floater, IM opens the chat tab, Activate
/// wears the group, and Leave opens a confirm modal.
fn spawn_action_button(
    commands: &mut Commands,
    actions: Entity,
    action: GroupAction,
) -> ActionButton {
    let spawned = ui_spawn::spawn_button(
        commands,
        actions,
        ButtonSpec::flat(UiLabel::key(action.label_key()), "groups-action")
            .colors(ACTION_BACKGROUND, ACTION_BACKGROUND)
            .label_color(LABEL_COLOR)
            .class(ACTION_BUTTON_CLASS)
            .label_class(TEXT_CLASS)
            .font_size(CHROME_FONT_SIZE),
    );
    let button = spawned.button;
    commands.entity(button).observe(
        move |mut press: On<Pointer<Press>>,
              selected: Res<SelectedGroup>,
              view: Res<GroupsView>,
              mut pending: ResMut<PendingLeaveConfirm>,
              mut sl: MessageWriter<SlCommand>,
              mut open: MessageWriter<OpenConversation>,
              mut profile: MessageWriter<OpenGroupProfile>| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            let Some(choice) = selected.0 else {
                return;
            };
            // A greyed button must really be inert: Bevy's disabled marker is
            // advisory, so the refusal lives here and the greying in
            // `refresh_group_actions` reads from the same predicate.
            if !action_enabled(action, Some(choice), &view) {
                return;
            }
            match action {
                // Info opens the subject-bound group profile floater.
                GroupAction::Info => {
                    if let GroupChoice::Group(group) = choice {
                        profile.write(OpenGroupProfile { group });
                    }
                }
                // IM opens (and joins) the group's chat tab. Mirrors the
                // Friends list's IM, which opens a one-to-one tab.
                GroupAction::Im => {
                    if let GroupChoice::Group(group) = choice {
                        open_group_im(group, &mut open, &mut sl);
                    }
                }
                // Leaving is destructive — open the confirm modal instead of
                // sending straight away.
                GroupAction::Leave => {
                    if let GroupChoice::Group(group) = choice {
                        pending.0 = Some(group);
                    }
                }
                // Activate fires immediately — including for the "no group"
                // row, which is what takes the title off.
                GroupAction::Activate => {
                    if let Some(command) = group_command(action, choice) {
                        sl.write(SlCommand(command));
                    }
                }
            }
        },
    );
    ActionButton { action, button }
}

/// Spawn the leave-confirm modal: a full-window scrim (blocking clicks behind it)
/// centred on a warning box with the prompt and Cancel / Leave buttons. Hidden
/// until a leave is pending. Returns `(overlay, prompt_text)`. Mirrors
/// [`crate::people`]'s grant-confirm modal.
fn spawn_leave_confirm_modal(commands: &mut Commands, root: Entity) -> (Entity, Entity) {
    let overlay = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                display: Display::None,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(CONFIRM_SCRIM),
            GlobalZIndex(CONFIRM_Z),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("groups-leave-confirm-overlay"),
            ChildOf(root),
        ))
        .id();
    let box_node = commands
        .spawn((
            Node {
                max_width: Val::Px(360.0),
                padding: UiRect::all(Val::Px(14.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Stretch,
                ..column(Val::Px(12.0))
            },
            BorderColor::all(CONFIRM_BOX_BORDER),
            BackgroundColor(CONFIRM_BOX_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("groups-leave-confirm-box"),
            ChildOf(overlay),
        ))
        .id();
    let confirm_text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            text_role(LABEL_COLOR),
            Pickable::IGNORE,
            Name::new("groups-leave-confirm-text"),
            ChildOf(box_node),
        ))
        .id();
    let buttons = commands
        .spawn((
            Node {
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            Name::new("groups-leave-confirm-buttons"),
            ChildOf(box_node),
        ))
        .id();
    spawn_confirm_button(
        commands,
        buttons,
        LEAVE_CONFIRM_NO_KEY,
        CONFIRM_CANCEL_BACKGROUND,
        false,
    );
    spawn_confirm_button(
        commands,
        buttons,
        LEAVE_CONFIRM_YES_KEY,
        CONFIRM_LEAVE_BACKGROUND,
        true,
    );
    (overlay, confirm_text)
}

/// Spawn one confirm-modal button (`leave` = the Leave button, else Cancel). On
/// Leave it sends [`Command::LeaveGroup`] and optimistically drops the group from
/// the model (the drop / leave-result event confirms it); either button closes the
/// modal by clearing [`PendingLeaveConfirm`].
fn spawn_confirm_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    background: Color,
    leave: bool,
) {
    commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(background),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("groups-leave-confirm-button"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            text_role(LABEL_COLOR),
            Translated::new(label_key),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  mut pending: ResMut<PendingLeaveConfirm>,
                  mut model: ResMut<GroupsModel>,
                  mut sl: MessageWriter<SlCommand>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                let taken = pending.0.take();
                if leave && let Some(group) = taken {
                    model.remove(group);
                    sl.write(SlCommand(Command::LeaveGroup(group)));
                }
            },
        );
}

// ---------------------------------------------------------------------------
// Ingest
// ---------------------------------------------------------------------------

/// Fold every group-relevant inbound event into [`GroupsModel`].
pub fn ingest_group_events(mut events: MessageReader<SlEvent>, mut model: ResMut<GroupsModel>) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::GroupMemberships(memberships) => model.apply_memberships(memberships),
            // On-demand names for non-member groups (a group-owned parcel / object).
            SlSessionEvent::GroupNames(names) => {
                for group in names {
                    model.note_resolved_name(group.id, &group.name);
                }
            }
            SlSessionEvent::GroupProfileReceived(profile) => {
                model.note_resolved_name(profile.group_id, &profile.name);
            }
            SlSessionEvent::ActiveGroupChanged(active) => {
                model.set_active(active.active_group_id, &active.group_title);
            }
            // Both the UDP `AgentDropGroup` and its CAPS event-queue twin drop the
            // agent from a group (leaving, ejection, or dissolution).
            SlSessionEvent::DroppedFromGroup { group_id } => model.remove(*group_id),
            SlSessionEvent::AgentDroppedFromGroup { group } => model.remove(*group),
            // A confirmed leave drops the group too (the drop event usually follows,
            // but this keeps the list responsive if it does not).
            SlSessionEvent::LeaveGroupResult { group_id, success } if *success => {
                model.remove(*group_id);
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// View / refresh
// ---------------------------------------------------------------------------

/// Rebuild `GroupsView` whenever the model's revision advances, resetting the
/// list scroll to the top so the new order is read from its start.
///
/// The list is **also** re-sized whenever its count disagrees with the view,
/// which is what keeps the pane from being empty for a whole session: the
/// memberships are a single push the grid sends within a frame or two of login
/// (nothing ever asks for them again), while the pane itself is two deferred
/// spawns behind — the People pane waits for the conversations strip, and the
/// group list waits for the People pane. Stamping the revision on the frame the
/// push lands and only writing `item_count` under `ui` would leave the list at
/// zero rows with no second revision to rebuild it from.
fn rebuild_groups_view(
    model: Res<GroupsModel>,
    mut view: ResMut<GroupsView>,
    ui: Option<Res<GroupsUi>>,
    mut lists: Query<&mut VirtualList>,
) {
    let rebuilt = view.built_revision != model.revision();
    if rebuilt {
        view.built_revision = model.revision();
        view.rows = model.ordered();
    }
    let Some(ui) = ui else {
        return;
    };
    let Ok(mut list) = lists.get_mut(ui.viewport) else {
        return;
    };
    if rebuilt {
        list.item_count = view.rows.len();
        list.scroll_to_top();
    } else if list.item_count != view.rows.len() {
        // A pane that appeared after the push: adopt the rows already built,
        // leaving the scroll where the user put it.
        list.item_count = view.rows.len();
    }
}

/// Keep the group count line in step with the model (the rows themselves are kept
/// in step by [`bind_group_rows`]).
fn refresh_groups(
    model: Res<GroupsModel>,
    ui: Option<Res<GroupsUi>>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    // `ui.is_added()` is the pane catching up with a model that changed before it
    // existed (see [`rebuild_groups_view`]); `translator.changed()` relocalises the
    // line on a locale switch, which no model change would.
    if !model.is_changed() && !ui.is_added() && !translator.changed() {
        return;
    }
    let count = i64::try_from(model.len()).unwrap_or(i64::MAX);
    let label = translator.format(COUNT_KEY, &TransArgs::new().int("count", count));
    if let Ok(mut text) = texts.get_mut(ui.count_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Grey each action button the current selection cannot support, from the same
/// [`action_enabled`] predicate its press refusal uses — so the "no group" row
/// shows Info, IM and Leave as unavailable instead of offering three buttons that
/// do nothing, and the worn group shows Activate the same way.
fn refresh_group_actions(
    selected: Res<SelectedGroup>,
    view: Res<GroupsView>,
    ui: Option<Res<GroupsUi>>,
    mut commands: Commands,
    disabled: DisabledButtons,
) {
    let Some(ui) = ui else {
        return;
    };
    if !selected.is_changed() && !view.is_changed() && !ui.is_added() {
        return;
    }
    for entry in &ui.action_buttons {
        let enabled = action_enabled(entry.action, selected.0, &view);
        set_action_button_enabled(&mut commands, &disabled, entry.button, enabled);
    }
}

/// Show / hide the leave-confirm modal from [`PendingLeaveConfirm`], filling the
/// prompt with the pending group's name.
fn drive_leave_confirm(
    pending: Res<PendingLeaveConfirm>,
    ui: Option<Res<GroupsUi>>,
    model: Res<GroupsModel>,
    translator: Translator,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let shown = pending.0.is_some();
    if let Ok(mut node) = nodes.get_mut(ui.confirm_overlay) {
        let wanted = if shown { Display::Flex } else { Display::None };
        if node.display != wanted {
            node.display = wanted;
        }
    }
    if let Some(group) = pending.0 {
        let name = model
            .name_of(group)
            .map_or_else(|| short_id(group.uuid()), ToOwned::to_owned);
        let prompt = translator.format(
            LEAVE_CONFIRM_PROMPT_KEY,
            &TransArgs::new().text("name", &name),
        );
        if let Ok(mut text) = texts.get_mut(ui.confirm_text)
            && text.0 != prompt
        {
            text.0 = prompt;
        }
    }
}

// ---------------------------------------------------------------------------
// Row pool: populate + bind
// ---------------------------------------------------------------------------

/// Build the inner nodes of each freshly-pooled group row once (a name label and a
/// trailing active marker) and wire its click to select the group.
fn populate_group_rows(
    mut commands: Commands,
    ui: Option<Res<GroupsUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        // Amended, not inserted: `top` and `display` are the virtual list's.
        amend_row_node(&mut commands, row_entity, |node| {
            node.position_type = PositionType::Absolute;
            node.left = Val::Px(0.0);
            node.right = Val::Px(0.0);
            node.height = Val::Px(ROW_HEIGHT);
            node.align_items = AlignItems::Center;
            node.column_gap = Val::Px(4.0);
            node.padding = UiRect::horizontal(Val::Px(4.0));
        });
        commands
            .entity(row_entity)
            .insert((BackgroundColor(Color::NONE), Pickable::default()));
        // Name fills the row; the active marker sits in a fixed-width cell under the
        // "Active" header.
        let label = commands
            .spawn((
                Text::new(String::new()),
                UiFont::Sans.at(ROW_FONT_SIZE),
                ClassList::new_with_classes([TEXT_CLASS]),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row_entity),
            ))
            .id();
        let active_cell = commands
            .spawn((
                Node {
                    width: Val::Px(ACTIVE_COL_WIDTH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row_entity),
            ))
            .id();
        let marker = commands
            .spawn((
                // The skin's `glyph::CURRENT_MARK`, lit by `glyph::CURRENT`.
                // Always the accent: the marker is either the mark or nothing
                // at all, so it has no second colour to take.
                glyph::glyph_host(
                    glyph::CURRENT_MARK,
                    UiFont::Sans.at(ROW_FONT_SIZE),
                    [ACTIVE_TEXT_CLASS],
                ),
                ChildOf(active_cell),
            ))
            .id();
        commands
            .entity(row_entity)
            .insert((GroupRowParts { label, marker }, BoundGroup(None)))
            .observe(on_group_row_press);
    }
}

/// Bind each pooled group row to the [`GroupRow`] it now points at — on the frame
/// the view rebuilt, the selection changed, or this row's index changed.
fn bind_group_rows(
    view: Res<GroupsView>,
    selected: Res<SelectedGroup>,
    ui: Option<Res<GroupsUi>>,
    translator: Translator,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &GroupRowParts,
        &mut BoundGroup,
    )>,
    mut classes: Query<&mut ClassList>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    // The "no group" row carries no name of its own — its label is a localised
    // string, so a locale switch has to re-bind the rows that show it.
    let none_label = translator.get(NONE_KEY);
    let refresh_all = view.is_changed() || selected.is_changed() || translator.changed();
    for (row_entity, row, child_of, parts, mut bound) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let Some(index) = row.index else {
            continue;
        };
        let Some(group_row) = view.rows.get(index) else {
            continue;
        };
        bound.0 = Some(group_row.group);
        if let Ok(mut text) = texts.get_mut(parts.label) {
            set_text(
                &mut text,
                match group_row.group {
                    GroupChoice::NoGroup => &none_label,
                    GroupChoice::Group(_) => &group_row.name,
                },
            );
        }
        // The name reads as worn in the accent — the same class the sibling
        // panes' selected text takes, which is what the local copy of that
        // colour was trying to be.
        set_state_class_on(
            &mut classes,
            parts.label,
            ACTIVE_TEXT_CLASS,
            group_row.active,
        );
        // The active marker — lit for the active group; the mark is the skin's.
        if let Ok(mut marker) = classes.get_mut(parts.marker) {
            set_state_class(&mut marker, glyph::CURRENT, group_row.active);
        }
        let is_selected = selected.0 == Some(group_row.group);
        if let Ok(mut classes) = classes.get_mut(row_entity) {
            set_state_class(&mut classes, ACTIVE_CLASS, is_selected);
        }
    }
}

/// Open (and join) a group's IM: the conversation tab via [`OpenConversation`],
/// and the session via [`Command::StartGroupSession`] so its messages flow. Shared
/// by the IM button and a row double-click.
fn open_group_im(
    group: GroupKey,
    open: &mut MessageWriter<OpenConversation>,
    sl: &mut MessageWriter<SlCommand>,
) {
    open.write(OpenConversation {
        key: ConversationKey::Group(group),
    });
    sl.write(SlCommand(Command::StartGroupSession(group)));
}

/// What a press on a group row moves, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the double-click clock and
/// stash, the keyboard focus the viewport takes, and the selected row.
#[derive(Debug, bevy::ecs::system::SystemParam)]
struct GroupRowClick<'w> {
    /// The clock the double-click window is measured on.
    time: Res<'w, Time>,
    /// The last click, for the double-click test.
    tracker: ResMut<'w, GroupClickTracker>,
    /// The keyboard focus, which the clicked viewport takes.
    focus: ResMut<'w, InputFocus>,
    /// The selected row.
    selected: ResMut<'w, SelectedGroup>,
}

/// A group row was clicked: focus the list (so the wheel scrolls it), select the
/// group it presents, and — on a **double-click** (two presses on the same group
/// within [`DOUBLE_CLICK_SECS`]) — open its IM, exactly like the IM button.
fn on_group_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&BoundGroup>,
    ui: Res<GroupsUi>,
    mut click: GroupRowClick,
    mut open: MessageWriter<OpenConversation>,
    mut sl: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    click.focus.set(ui.viewport, FocusCause::Navigated);
    let Ok(bound) = rows.get(press.entity) else {
        return;
    };
    let Some(choice) = bound.0 else {
        return;
    };
    click.selected.0 = Some(choice);
    let now = click.time.elapsed_secs();
    if click.tracker.group == Some(choice) && now - click.tracker.time <= DOUBLE_CLICK_SECS {
        // Second quick click on the same row: open its IM — for a real group only,
        // since the "no group" row has no conversation to open. Clear the tracker
        // so a third click does not re-fire either way.
        if let GroupChoice::Group(group) = choice {
            open_group_im(group, &mut open, &mut sl);
        }
        click.tracker.group = None;
    } else {
        click.tracker.group = Some(choice);
        click.tracker.time = now;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIONS, COUNT_KEY, Command, GroupAction, GroupChoice, GroupRow, GroupsModel, GroupsUi,
        GroupsView, ROW_HEIGHT, VirtualList, action_enabled, group_command, rebuild_groups_view,
        refresh_groups,
    };
    use crate::i18n::install_untranslated;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{GroupKey, GroupMembership, LandArea, TextureKey, Uuid};

    /// A membership record with the given id and name (default powers / land).
    fn membership(id: u128, name: &str) -> GroupMembership {
        GroupMembership {
            group_id: GroupKey::from(Uuid::from_u128(id)),
            group_powers: 0,
            accept_notices: true,
            group_insignia_id: TextureKey::from(Uuid::nil()),
            contribution: LandArea::ZERO,
            group_name: name.to_owned(),
        }
    }

    /// The real-group rows, without the leading "no group" choice.
    fn real_rows(model: &GroupsModel) -> Vec<GroupRow> {
        model
            .ordered()
            .into_iter()
            .filter(|row| matches!(row.group, GroupChoice::Group(_)))
            .collect()
    }

    /// Memberships seed the model, replacing wholesale (the wire list is the full
    /// membership set), and rows come out case-folded by name.
    #[test]
    fn memberships_seed_and_order() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(1, "zeta guild"), membership(2, "Alpha club")]);
        let names: Vec<String> = real_rows(&model).into_iter().map(|row| row.name).collect();
        assert_eq!(names, vec!["Alpha club", "zeta guild"]);
        // A second (full) push replaces the set.
        model.apply_memberships(&[membership(3, "Only one")]);
        assert_eq!(model.len(), 1);
        assert_eq!(
            real_rows(&model).first().map(|row| row.name.clone()),
            Some("Only one".to_owned())
        );
    }

    /// The "no group" choice leads the list whenever there is a group to wear
    /// instead — it is the only way back to no title — and is absent for a member
    /// of nothing, where it would do nothing.
    #[test]
    fn the_no_group_row_leads_a_non_empty_list() {
        let mut model = GroupsModel::default();
        assert!(
            model.ordered().is_empty(),
            "a member of no groups is offered nothing at all"
        );
        model.apply_memberships(&[membership(1, "zeta guild"), membership(2, "Alpha club")]);
        let rows = model.ordered();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.first().map(|row| row.group),
            Some(GroupChoice::NoGroup)
        );
        assert_eq!(
            rows.first().map(|row| row.name.as_str()),
            Some(""),
            "its label is the UI's localised string, not the model's"
        );
    }

    /// The active group is marked on its row and cleared when it changes / is left
    /// — and with nothing worn the mark moves to the "no group" row, which is what
    /// makes "wearing no title" visible as a state rather than an absence.
    #[test]
    fn active_group_marks_its_row() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(1, "One"), membership(2, "Two")]);
        let one = GroupKey::from(Uuid::from_u128(1));
        model.set_active(Some(one), "");
        let active: Vec<bool> = model.ordered().into_iter().map(|row| row.active).collect();
        // The none row, then "One" then "Two"; only "One" is active.
        assert_eq!(active, vec![false, true, false]);
        // Clearing the active group unmarks it and marks the none row.
        model.set_active(None, "");
        let active: Vec<bool> = model.ordered().into_iter().map(|row| row.active).collect();
        assert_eq!(active, vec![true, false, false]);
    }

    /// Removing a group drops it and clears the active marker if it was active; an
    /// unknown id is a no-op.
    #[test]
    fn remove_drops_and_clears_active() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(1, "One"), membership(2, "Two")]);
        let one = GroupKey::from(Uuid::from_u128(1));
        model.set_active(Some(one), "");
        model.remove(one);
        assert_eq!(model.len(), 1);
        assert!(real_rows(&model).iter().all(|row| !row.active));
        // An unknown id changes nothing.
        let before = model.revision();
        model.remove(GroupKey::from(Uuid::from_u128(42)));
        assert_eq!(model.revision(), before);
    }

    /// An unnamed group falls back to a short-id placeholder row label.
    #[test]
    fn unnamed_group_uses_short_id() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(0x1234_5678_9abc, "")]);
        let name = real_rows(&model)
            .first()
            .map(|row| row.name.clone())
            .unwrap_or_default();
        assert_eq!(name.len(), 8);
    }

    /// Each action maps to its command: Activate / Leave produce a command, while
    /// Info (opens the profile floater via a message) and IM (opens a tab + starts
    /// the session elsewhere) produce none.
    #[test]
    fn action_command_mapping() {
        let group = GroupChoice::Group(GroupKey::from(Uuid::from_u128(7)));
        assert!(group_command(GroupAction::Info, group).is_none());
        assert!(group_command(GroupAction::Im, group).is_none());
        assert!(matches!(
            group_command(GroupAction::Activate, group),
            Some(Command::ActivateGroup(Some(_)))
        ));
        assert!(matches!(
            group_command(GroupAction::Leave, group),
            Some(Command::LeaveGroup(_))
        ));
    }

    /// Activating the "no group" row is the whole point of it: it goes out as
    /// `ActivateGroup(None)`, the wire's "wear nothing", which no real group's
    /// activation can express. Leaving it produces nothing — there is no group to
    /// leave.
    #[test]
    fn the_no_group_row_activates_as_no_group() {
        assert!(matches!(
            group_command(GroupAction::Activate, GroupChoice::NoGroup),
            Some(Command::ActivateGroup(None))
        ));
        assert!(group_command(GroupAction::Leave, GroupChoice::NoGroup).is_none());
        assert!(group_command(GroupAction::Info, GroupChoice::NoGroup).is_none());
        assert!(group_command(GroupAction::Im, GroupChoice::NoGroup).is_none());
    }

    /// What the action column offers, from the same predicate the press refusal
    /// reads: Info / IM / Leave need a real group, every row but the worn one can
    /// be activated, and an empty selection offers nothing.
    #[test]
    fn only_the_actions_a_row_supports_are_offered() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(1, "One"), membership(2, "Two")]);
        let one = GroupChoice::Group(GroupKey::from(Uuid::from_u128(1)));
        model.set_active(one.key(), "");
        let view = GroupsView {
            rows: model.ordered(),
            built_revision: model.revision(),
        };

        // Nothing selected: nothing to do.
        for action in ACTIONS {
            assert!(!action_enabled(action, None, &view), "{action:?} with none");
        }
        // The "no group" row: only Activate, since no group is worn... it is not
        // worn here, "One" is.
        assert!(action_enabled(
            GroupAction::Activate,
            Some(GroupChoice::NoGroup),
            &view
        ));
        for action in [GroupAction::Info, GroupAction::Im, GroupAction::Leave] {
            assert!(
                !action_enabled(action, Some(GroupChoice::NoGroup), &view),
                "{action:?} needs a real group"
            );
        }
        // The worn group: everything but Activate, which would change nothing.
        assert!(!action_enabled(GroupAction::Activate, Some(one), &view));
        for action in [GroupAction::Info, GroupAction::Im, GroupAction::Leave] {
            assert!(
                action_enabled(action, Some(one), &view),
                "{action:?} on a group"
            );
        }

        // With nothing worn, the "no group" row is the one that cannot be activated.
        model.set_active(None, "");
        let view = GroupsView {
            rows: model.ordered(),
            built_revision: model.revision(),
        };
        assert!(!action_enabled(
            GroupAction::Activate,
            Some(GroupChoice::NoGroup),
            &view
        ));
        assert!(action_enabled(GroupAction::Activate, Some(one), &view));
    }

    /// The memberships are a **single push** the grid sends within a frame or two
    /// of login, and the pane is two deferred spawns behind it (the People pane
    /// waits for the conversations strip, the group list waits for the People
    /// pane). A list that appears after that push must adopt the rows already
    /// built — there is no second push to rebuild it from, and waiting for one is
    /// what left the pane empty for a whole session.
    #[test]
    fn a_list_spawned_after_the_push_still_lists_the_groups() {
        let mut app = App::new();
        install_untranslated(&mut app);
        app.init_resource::<GroupsModel>()
            .init_resource::<GroupsView>()
            .add_systems(Update, (rebuild_groups_view, refresh_groups).chain());

        // The memberships land with no pane to put them in.
        app.world_mut()
            .resource_mut::<GroupsModel>()
            .apply_memberships(&[membership(1, "One"), membership(2, "Two")]);
        app.update();

        // The pane arrives afterwards: an empty list and a blank count line.
        let viewport = app.world_mut().spawn(VirtualList::new(ROW_HEIGHT)).id();
        let count_text = app.world_mut().spawn(Text::new(String::new())).id();
        let confirm_overlay = app.world_mut().spawn_empty().id();
        let confirm_text = app.world_mut().spawn_empty().id();
        app.world_mut().insert_resource(GroupsUi {
            viewport,
            count_text,
            action_buttons: Vec::new(),
            confirm_overlay,
            confirm_text,
        });
        app.update();

        assert_eq!(
            app.world()
                .entity(viewport)
                .get::<VirtualList>()
                .map(|list| list.item_count),
            // Two memberships plus the leading "no group" row.
            Some(3),
            "the list adopts the memberships pushed before it existed"
        );
        assert_eq!(
            app.world()
                .entity(count_text)
                .get::<Text>()
                .map(|text| text.0.clone()),
            Some(COUNT_KEY.to_owned()),
            "and the count line is filled in too (untranslated: the key itself)"
        );
    }
}
