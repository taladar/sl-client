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
//! Only the **list** and its **Info / IM / Activate / Leave** actions are built
//! here. Of those, IM opens the group's chat tab, Activate sets the worn group and
//! Leave (behind a confirm) leaves it; **Info** is present in the layout but inert
//! — the group **profile** floater (general / roles / notices, and any role or
//! membership editing) is a separate, deliberately out-of-scope task.
//!
//! # Model + ECS mirror
//!
//! [`GroupsModel`] is a plain, unit-tested resource fed **only** from the
//! [`SlEvent`] stream — the agent's memberships
//! ([`SlSessionEvent::GroupMemberships`], pushed on login and whenever membership
//! changes), the active group ([`SlSessionEvent::ActiveGroupChanged`]), and the
//! drop / leave lifecycle events — mirroring [`crate::people`]'s pure model. Unlike
//! the friends list it needs no name-resolution pass: each membership record
//! already carries its group name. [`GroupsView`] is the ordered, render-ready
//! projection the virtualized list ([`crate::virtual_list`]) binds its recycled
//! rows to.
//!
//! # Sharing the pane with the People surface
//!
//! The list is built into [`crate::people::PeopleUi::groups_content`], the slot the
//! People pane already toggles between its Friends and Groups sub-tabs. This module
//! never touches that visibility — it only owns what is *inside* the Groups slot.
//! The IM action hands the strip back to a conversation the same way the Friends
//! list does, via [`crate::conversations::OpenConversation`].
//!
//! Reference (Firestorm, read-only): `llgrouplist`, `llgroupactions`,
//! Vintage `panel_fs_contacts_groups`.

use std::collections::BTreeMap;

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use sl_client_bevy::{
    Command, GroupKey, GroupMembership, SlCommand, SlEvent, SlSessionEvent, Uuid,
};

use crate::conversations::{ConversationKey, OpenConversation};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::people::PeopleUi;
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::virtual_list::{VirtualList, VirtualRow, VirtualViewport, layout_virtual_lists};

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

/// An accent used for the active group's row text and its marker — the same bright
/// "this one is selected" hue the sibling panes use.
const ACTIVE_COLOR: Color = Color::srgb(0.52, 0.68, 0.95);

/// A group / label's text colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The group-list scroll surface background — a touch darker, a sunken well (same
/// as the friends list).
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The background of the currently-selected group row.
const SELECTED_ROW_BACKGROUND: Color = Color::srgba(0.30, 0.42, 0.62, 0.55);

/// An action button's background.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// An **inert** action button's background — dimmer, so the Info button reads as
/// present-but-not-yet-wired (its profile floater is a separate task).
const ACTION_INERT_BACKGROUND: Color = Color::srgb(0.17, 0.20, 0.26);

/// The table header row's background — a recessed strip above the list.
const HEADER_BACKGROUND: Color = Color::srgb(0.14, 0.17, 0.22);

/// The table header / count text colour — dim, so it reads as chrome.
const HEADER_TEXT_COLOR: Color = Color::srgb(0.66, 0.70, 0.78);

/// The filled marker glyph shown in the Active column for the active group.
const ACTIVE_GLYPH: &str = "\u{25CF}";

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

/// The Fluent key for the confirm dialog's leave prompt (arg `name`).
const LEAVE_CONFIRM_PROMPT_KEY: &str = "groups-leave-confirm-prompt";

/// The Fluent key for the confirm dialog's Leave button.
const LEAVE_CONFIRM_YES_KEY: &str = "groups-leave-confirm-yes";

/// The Fluent key for the confirm dialog's Cancel button.
const LEAVE_CONFIRM_NO_KEY: &str = "groups-leave-confirm-no";

// ---------------------------------------------------------------------------
// Pure model
// ---------------------------------------------------------------------------

/// The pure groups model: the agent's group memberships keyed by group id (to its
/// display name), the active (worn) group, and a revision stamp bumped on every
/// change so the view rebuilds only when something actually moved. Fed solely from
/// the event stream. The list and its actions need only the name; the membership
/// record's powers / contribution belong to the (out-of-scope) profile.
#[derive(Resource, Debug, Default)]
pub(crate) struct GroupsModel {
    /// The agent's groups, by group id, mapped to the group's display name.
    groups: BTreeMap<GroupKey, String>,
    /// The currently-active (worn) group, if any.
    active: Option<GroupKey>,
    /// Bumped on each mutation; the view compares its last-built value to skip an
    /// unchanged rebuild.
    revision: u64,
}

impl GroupsModel {
    /// Bump the revision after a mutation.
    const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Replace the membership set from an `AgentGroupDataUpdate`
    /// ([`SlSessionEvent::GroupMemberships`]) — the wire message carries the
    /// agent's **full** group list, so it is authoritative and replaces the cache
    /// wholesale. The active group is left untouched (it is tracked separately from
    /// [`SlSessionEvent::ActiveGroupChanged`]).
    fn apply_memberships(&mut self, memberships: &[GroupMembership]) {
        self.groups.clear();
        for membership in memberships {
            self.groups
                .insert(membership.group_id, membership.group_name.clone());
        }
        self.touch();
    }

    /// The display name of `group`, if the agent is a member. The build
    /// floater's General tab shows the selected object's group through this
    /// (an object set to a group the agent is not in falls back to its id).
    pub(crate) fn group_name(&self, group: GroupKey) -> Option<&str> {
        self.groups.get(&group).map(String::as_str)
    }

    /// The agent's group ids, in the map's stable id order — the build
    /// floater's set-group cycle walks these (with "none" between the wrap).
    pub(crate) fn group_ids(&self) -> Vec<GroupKey> {
        self.groups.keys().copied().collect()
    }

    /// Set the active (worn) group, bumping the revision only on a real change.
    fn set_active(&mut self, active: Option<GroupKey>) {
        if self.active != active {
            self.active = active;
            self.touch();
        }
    }

    /// Drop a group the agent is no longer in (left, ejected, or dissolved),
    /// clearing the active marker if it was the active group.
    fn remove(&mut self, group: GroupKey) {
        if self.groups.remove(&group).is_some() {
            if self.active == Some(group) {
                self.active = None;
            }
            self.touch();
        }
    }

    /// The ordered, render-ready row list: case-folded by group name, with a stable
    /// id tie-break so equal names keep a fixed order.
    fn ordered(&self) -> Vec<GroupRow> {
        let mut rows: Vec<GroupRow> = self
            .groups
            .iter()
            .map(|(id, group_name)| {
                let name = if group_name.is_empty() {
                    short_id(id.uuid())
                } else {
                    group_name.clone()
                };
                GroupRow {
                    group: *id,
                    name,
                    active: self.active == Some(*id),
                }
            })
            .collect();
        rows.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.group.uuid().cmp(&right.group.uuid()))
        });
        rows
    }

    /// The number of groups the agent is in — the count line under the list.
    fn len(&self) -> usize {
        self.groups.len()
    }

    /// The display name for a group, if known (for the leave-confirm prompt).
    fn name_of(&self, group: GroupKey) -> Option<&str> {
        self.groups.get(&group).map(String::as_str)
    }
}

/// A short, readable stand-in for a group with no name yet — its first eight hex
/// digits (mirrors [`crate::people`]'s placeholder).
fn short_id(id: Uuid) -> String {
    id.simple().to_string().chars().take(8).collect()
}

/// One render-ready group row: the id its actions need, the display name, and
/// whether it is the active (worn) group.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupRow {
    /// The group id (for every action).
    group: GroupKey,
    /// The display name (or a short-id placeholder for an unnamed group).
    name: String,
    /// Whether this is the agent's active (worn) group.
    active: bool,
}

// ---------------------------------------------------------------------------
// Row actions
// ---------------------------------------------------------------------------

/// A per-group action offered by the action column beside the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupAction {
    /// Open the group's profile — **inert** in this task (the profile floater is a
    /// separate, out-of-scope job); present so the layout matches the reference.
    Info,
    /// Open (and join) the group's IM chat tab.
    Im,
    /// Make this the agent's active (worn) group.
    Activate,
    /// Leave the group (behind a confirm dialog).
    Leave,
}

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

    /// Whether this action is inert (the Info placeholder) — rendered dimmer and
    /// wired to no command.
    const fn is_inert(self) -> bool {
        matches!(self, Self::Info)
    }
}

/// The wire [`Command`] an action produces for `group`, or `None` for the actions
/// that are not a plain fire-and-forget command: [`GroupAction::Info`] (inert),
/// and [`GroupAction::Im`] (which opens a conversation tab and starts the session
/// through separate paths). Pure so the routing is unit-testable.
const fn group_command(action: GroupAction, group: GroupKey) -> Option<Command> {
    match action {
        GroupAction::Info | GroupAction::Im => None,
        GroupAction::Activate => Some(Command::ActivateGroup(Some(group))),
        GroupAction::Leave => Some(Command::LeaveGroup(group)),
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
    /// The leave-confirm modal overlay (shown while a leave is pending).
    confirm_overlay: Entity,
    /// The confirm modal's prompt text node (rewritten with the group's name).
    confirm_text: Entity,
}

/// The ordered, render-ready groups projection the virtualized list binds to.
#[derive(Resource, Debug, Default)]
pub(crate) struct GroupsView {
    /// The rows in display order.
    rows: Vec<GroupRow>,
    /// The model revision this view was last built from.
    built_revision: u64,
}

/// The currently-selected group, which the action column acts on.
#[derive(Resource, Debug, Default)]
pub(crate) struct SelectedGroup(Option<GroupKey>);

/// The last group-row click, for detecting a double-click (two presses on the same
/// group within [`DOUBLE_CLICK_SECS`] open its IM). Tracked by group id, not row
/// entity, since the virtualized rows are recycled.
#[derive(Resource, Debug, Default)]
pub(crate) struct GroupClickTracker {
    /// The group the last press selected, if any.
    group: Option<GroupKey>,
    /// When that press landed, in seconds since startup ([`Time::elapsed_secs`]).
    time: f32,
}

/// A pending, not-yet-confirmed **leave** — leaving a group is destructive enough
/// to gate behind a confirm dialog (the reference does the same). `None` when no
/// confirm is open.
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingLeaveConfirm(Option<GroupKey>);

/// The group a pooled row currently presents (so a press knows which to select),
/// or `None` when the row is parked.
#[derive(Component, Debug, Clone, Copy)]
struct BoundGroup(Option<GroupKey>);

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
pub(crate) struct GroupsPlugin;

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
                    ingest_group_events,
                    rebuild_groups_view,
                    refresh_groups,
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
            BackgroundColor(LIST_BACKGROUND),
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

    // The count line under the list.
    let count_text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(HEADER_TEXT_COLOR),
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
    for action in [
        GroupAction::Info,
        GroupAction::Im,
        GroupAction::Activate,
        GroupAction::Leave,
    ] {
        spawn_action_button(&mut commands, actions, action);
    }

    let (confirm_overlay, confirm_text) = spawn_leave_confirm_modal(&mut commands, root.0);

    commands.insert_resource(GroupsUi {
        viewport,
        count_text,
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
        TextColor(HEADER_TEXT_COLOR),
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
        TextColor(HEADER_TEXT_COLOR),
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

/// Spawn one action-column button wired to `action`. The inert Info button is
/// styled dimmer and its observer does nothing (its profile floater is a separate
/// task); the rest act on the current selection.
fn spawn_action_button(commands: &mut Commands, actions: Entity, action: GroupAction) {
    let background = if action.is_inert() {
        ACTION_INERT_BACKGROUND
    } else {
        ACTION_BACKGROUND
    };
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(background),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("groups-action"),
            ChildOf(actions),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new(action.label_key()),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  selected: Res<SelectedGroup>,
                  mut pending: ResMut<PendingLeaveConfirm>,
                  mut sl: MessageWriter<SlCommand>,
                  mut open: MessageWriter<OpenConversation>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                // The inert Info button does nothing (yet).
                if action.is_inert() {
                    return;
                }
                let Some(group) = selected.0 else {
                    return;
                };
                match action {
                    // IM opens (and joins) the group's chat tab. Mirrors the
                    // Friends list's IM, which opens a one-to-one tab.
                    GroupAction::Im => open_group_im(group, &mut open, &mut sl),
                    // Leaving is destructive — open the confirm modal instead of
                    // sending straight away.
                    GroupAction::Leave => {
                        pending.0 = Some(group);
                    }
                    // Activate (and any future direct command) fires immediately.
                    GroupAction::Info | GroupAction::Activate => {
                        if let Some(command) = group_command(action, group) {
                            sl.write(SlCommand(command));
                        }
                    }
                }
            },
        );
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
            TextColor(LABEL_COLOR),
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
            TextColor(LABEL_COLOR),
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
fn ingest_group_events(mut events: MessageReader<SlEvent>, mut model: ResMut<GroupsModel>) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::GroupMemberships(memberships) => model.apply_memberships(memberships),
            SlSessionEvent::ActiveGroupChanged(active) => model.set_active(active.active_group_id),
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

/// Rebuild [`GroupsView`] whenever the model's revision advances, resetting the
/// list scroll to the top so the new order is read from its start.
fn rebuild_groups_view(
    model: Res<GroupsModel>,
    mut view: ResMut<GroupsView>,
    ui: Option<Res<GroupsUi>>,
    mut lists: Query<&mut VirtualList>,
) {
    if view.built_revision == model.revision {
        return;
    }
    view.built_revision = model.revision;
    view.rows = model.ordered();
    if let Some(ui) = ui
        && let Ok(mut list) = lists.get_mut(ui.viewport)
    {
        list.item_count = view.rows.len();
        list.scroll_to_top();
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
    if !model.is_changed() {
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
        commands.entity(row_entity).insert((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                padding: UiRect::horizontal(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::default(),
        ));
        // Name fills the row; the active marker sits in a fixed-width cell under the
        // "Active" header.
        let label = commands
            .spawn((
                Text::new(String::new()),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(LABEL_COLOR),
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
                Text::new(String::new()),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(ACTIVE_COLOR),
                Pickable::IGNORE,
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
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &GroupRowParts,
        &mut BoundGroup,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
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
        // The name — brighter (accent) for the active group, so it reads as worn.
        if let Ok((mut text, mut color)) = texts.get_mut(parts.label) {
            set_text(&mut text, &group_row.name);
            *color = TextColor(if group_row.active {
                ACTIVE_COLOR
            } else {
                LABEL_COLOR
            });
        }
        // The active marker — the filled glyph for the active group, else empty.
        if let Ok((mut text, _color)) = texts.get_mut(parts.marker) {
            set_text(&mut text, if group_row.active { ACTIVE_GLYPH } else { "" });
        }
        let is_selected = selected.0 == Some(group_row.group);
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if is_selected {
                SELECTED_ROW_BACKGROUND
            } else {
                Color::NONE
            };
            if background.0 != wanted {
                background.0 = wanted;
            }
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

/// A group row was clicked: focus the list (so the wheel scrolls it), select the
/// group it presents, and — on a **double-click** (two presses on the same group
/// within [`DOUBLE_CLICK_SECS`]) — open its IM, exactly like the IM button.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected queries / resources: the picked row, the \
              viewport to focus, the click clock + tracker for double-click detection, the \
              selection to set, and the two writers a double-click opens the IM through"
)]
fn on_group_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&BoundGroup>,
    ui: Res<GroupsUi>,
    time: Res<Time>,
    mut tracker: ResMut<GroupClickTracker>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedGroup>,
    mut open: MessageWriter<OpenConversation>,
    mut sl: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.viewport, FocusCause::Navigated);
    let Ok(bound) = rows.get(press.entity) else {
        return;
    };
    let Some(group) = bound.0 else {
        return;
    };
    selected.0 = Some(group);
    let now = time.elapsed_secs();
    if tracker.group == Some(group) && now - tracker.time <= DOUBLE_CLICK_SECS {
        // Second quick click on the same group: open its IM. Clear the tracker so a
        // third click does not re-fire.
        open_group_im(group, &mut open, &mut sl);
        tracker.group = None;
    } else {
        tracker.group = Some(group);
        tracker.time = now;
    }
}

/// Set a text node's string only when it actually changed, so a re-bind of an
/// unchanged row does not needlessly re-measure it.
fn set_text(text: &mut Text, value: &str) {
    if text.0 != value {
        value.clone_into(&mut text.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, GroupAction, GroupsModel, group_command};
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

    /// Memberships seed the model, replacing wholesale (the wire list is the full
    /// membership set), and rows come out case-folded by name.
    #[test]
    fn memberships_seed_and_order() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(1, "zeta guild"), membership(2, "Alpha club")]);
        let names: Vec<String> = model.ordered().into_iter().map(|row| row.name).collect();
        assert_eq!(names, vec!["Alpha club", "zeta guild"]);
        // A second (full) push replaces the set.
        model.apply_memberships(&[membership(3, "Only one")]);
        assert_eq!(model.len(), 1);
        assert_eq!(
            model.ordered().first().map(|row| row.name.clone()),
            Some("Only one".to_owned())
        );
    }

    /// The active group is marked on its row and cleared when it changes / is left.
    #[test]
    fn active_group_marks_its_row() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(1, "One"), membership(2, "Two")]);
        let one = GroupKey::from(Uuid::from_u128(1));
        model.set_active(Some(one));
        let active: Vec<bool> = model.ordered().into_iter().map(|row| row.active).collect();
        // Sorted "One" then "Two"; only "One" is active.
        assert_eq!(active, vec![true, false]);
        // Clearing the active group unmarks it.
        model.set_active(None);
        assert!(model.ordered().iter().all(|row| !row.active));
    }

    /// Removing a group drops it and clears the active marker if it was active; an
    /// unknown id is a no-op.
    #[test]
    fn remove_drops_and_clears_active() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(1, "One"), membership(2, "Two")]);
        let one = GroupKey::from(Uuid::from_u128(1));
        model.set_active(Some(one));
        model.remove(one);
        assert_eq!(model.len(), 1);
        assert_eq!(model.active, None);
        // An unknown id changes nothing.
        let before = model.revision;
        model.remove(GroupKey::from(Uuid::from_u128(42)));
        assert_eq!(model.revision, before);
    }

    /// An unnamed group falls back to a short-id placeholder row label.
    #[test]
    fn unnamed_group_uses_short_id() {
        let mut model = GroupsModel::default();
        model.apply_memberships(&[membership(0x1234_5678_9abc, "")]);
        let name = model
            .ordered()
            .first()
            .map(|row| row.name.clone())
            .unwrap_or_default();
        assert_eq!(name.len(), 8);
    }

    /// Each action maps to its command: Activate / Leave produce a command, while
    /// Info (inert) and IM (opens a tab + starts the session elsewhere) produce
    /// none.
    #[test]
    fn action_command_mapping() {
        let group = GroupKey::from(Uuid::from_u128(7));
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

    /// Only the Info action is inert.
    #[test]
    fn only_info_is_inert() {
        assert!(GroupAction::Info.is_inert());
        assert!(!GroupAction::Im.is_inert());
        assert!(!GroupAction::Activate.is_inert());
        assert!(!GroupAction::Leave.is_inert());
    }
}
