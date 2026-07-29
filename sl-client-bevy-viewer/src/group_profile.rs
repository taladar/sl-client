//! The **group profile floater** (`viewer-social-group-profile`): a separate,
//! subject-bound floater — General / Members & Roles / Notices tabs — reached
//! from the [Groups list](crate::groups)'s **Info** button.
//!
//! # Layout follows the Vintage skin
//!
//! The reference's Vintage skin opens `FSFloaterGroup` /
//! `panel_group_info_sidetray` (not embedded in Contacts). Tabs and controls
//! mirror `llpanelgroup` / `llpanelgroupgeneral` / `llpanelgrouproles` /
//! `llpanelgroupnotices`. Not in scope (as the reference's other tabs): Land /
//! Assets, Money / accounting, Experiences, Banned Residents, and the group
//! create / search / invite dialogs — all filed as `viewer-social-group-extras`.
//!
//! # Subject-bound → persistence-exempt
//!
//! Like the [avatar profile](crate::avatar_profile) and the item previews, the
//! floater opens on a particular **subject** (a group) rather than persistent app
//! state, so it carries [`crate::floater_persist::FloaterPersistExempt`] on its
//! root: no restored rectangle, no restored "open".
//!
//! # Rebuilt per change
//!
//! The **General** tab and the (small) roles list, member/role **details** area,
//! and notice **compose** / **body** sub-panels are torn down and rebuilt from
//! [`GroupProfileState`] when the floater opens on a group and when a reply
//! arrives — the same picker-list pattern as [`crate::avatar_profile`]. The
//! (potentially large) **member** and **notice** lists are virtualized
//! ([`crate::virtual_list`]) with persistent viewports driven by view resources,
//! like the [groups list](crate::groups).
//!
//! # SL member-list caveat
//!
//! On Second Life the first member fetch deliberately returns only a **limited**
//! set — the group's officers and owners — on the assumption that most people who
//! open a group profile do not scroll the whole roster; a **Refresh** pulls the
//! full list (capped at ~5000). Members therefore accumulate across the multiple
//! [`SlSessionEvent::GroupMembers`] replies (deduplicated by agent id), and the
//! members header shows "loaded N of TOTAL" beside a **Refresh** button that
//! re-issues the fetch to pull the rest.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AgentKey, Command, GroupKey, GroupMember, GroupNotice, GroupNoticeKey, GroupProfile, GroupRole,
    GroupRoleChange, GroupRoleEdit, GroupRoleKey, GroupRoleMember, GroupRoleMemberChange,
    GroupRoleUpdateType, GroupTitle, ImDialog, LindenAmount, SlCommand, SlEvent, SlSessionEvent,
    TextureKey, UpdateGroupInfoParams, Uuid, group_powers, to_bevy_image,
};

use crate::avatars::AvatarState;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::groups::GroupsModel;
use crate::i18n::{TransArgs, Translated, Translator};
use crate::inventory_properties::format_unix_date;
use crate::render_priority::AVATAR_BOOST_PRIORITY;
use crate::settings::ViewerSettings;
use crate::textures::TextureManager;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabContainerHandle, TabPlacement, TabSpec, fill_tab_container,
    spawn_tab_container,
};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableRowCells, TableSelectionMode,
    TableSortDefault, TableSortKey, TableSpec, TableState, register_table_settings, set_table_cell,
    spawn_table, spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};

/// The chrome font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A list row's uniform height, in logical pixels.
const ROW_HEIGHT: f32 = 22.0;

/// The primary label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer secondary label.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A toggle's check-glyph colour when on.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// An accent for the selected row / active marker.
const ACCENT_COLOR: Color = Color::srgb(0.52, 0.68, 0.95);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A button's border colour.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// A list scroll surface's sunken background.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The background of the currently-selected list row.
const SELECTED_ROW_BACKGROUND: Color = Color::srgba(0.30, 0.42, 0.62, 0.55);

/// The checked glyph.
const CHECKED_GLYPH: &str = "\u{2611}";

/// The unchecked glyph.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// The group insignia's edge, in logical pixels.
const INSIGNIA_EDGE: f32 = 128.0;

/// The members list's "Title" column width, in logical pixels.
const TITLE_COL_WIDTH: f32 = 110.0;

/// The members list's "Contribution" column width, in logical pixels.
const CONTRIB_COL_WIDTH: f32 = 64.0;

/// The members list's "Status" column width, in logical pixels.
const STATUS_COL_WIDTH: f32 = 64.0;

/// The roles list's bounded (scrollable) height below the members list, in logical
/// pixels — a little taller than the old plain list to seat the table's own
/// column-header row.
const ROLES_LIST_HEIGHT: f32 = 150.0;

/// The notices list's "From" / "Date" column width, in logical pixels.
const NOTICE_COL_WIDTH: f32 = 110.0;

/// The `[group_profile]` section the table sort / width settings live under.
const TABLE_SETTINGS_SECTION: &[&str] = &["group_profile"];

/// The persisted-setting name for the members table's sort order.
const MEMBERS_SORT_SETTING: &str = "members_sort";

/// The persisted-setting name for the members table's column widths.
const MEMBERS_WIDTHS_SETTING: &str = "members_widths";

/// The persisted-setting name for the notices table's sort order.
const NOTICES_SORT_SETTING: &str = "notices_sort";

/// The persisted-setting name for the notices table's column widths.
const NOTICES_WIDTHS_SETTING: &str = "notices_widths";

/// The roles list's "Title" column width, in logical pixels.
const ROLE_TITLE_COL_WIDTH: f32 = 110.0;

/// The roles list's "Members" column width, in logical pixels.
const ROLE_MEMBERS_COL_WIDTH: f32 = 56.0;

/// The persisted-setting name for the roles table's sort order.
const ROLES_SORT_SETTING: &str = "roles_sort";

/// The persisted-setting name for the roles table's column widths.
const ROLES_WIDTHS_SETTING: &str = "roles_widths";

/// The members table: a flexible Name over fixed Title / Land / Status columns,
/// all sortable, defaulting to name-ascending. Column indices match
/// [`member_column_ordering`].
const MEMBERS_TABLE: TableSpec = TableSpec {
    element: "group-members",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "group-members-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "group-members-title",
            token: "title",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: TITLE_COL_WIDTH,
            },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "group-members-contribution",
            token: "land",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: CONTRIB_COL_WIDTH,
            },
            align: TableAlign::End,
            sortable: true,
        },
        TableColumn {
            header_key: "group-members-status",
            token: "status",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: STATUS_COL_WIDTH,
            },
            align: TableAlign::Start,
            sortable: true,
        },
    ],
    default_sort: &[TableSortDefault {
        column: 0,
        ascending: true,
    }],
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: Some(MEMBERS_SORT_SETTING),
    widths_setting: Some(MEMBERS_WIDTHS_SETTING),
};

/// The notices table: a flexible Subject over fixed From / Date columns, all
/// sortable, defaulting to date-descending (newest first). Column indices match
/// [`notice_column_ordering`].
const NOTICES_TABLE: TableSpec = TableSpec {
    element: "group-notices",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "group-notices-subject",
            token: "subject",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "group-notices-from",
            token: "from",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: NOTICE_COL_WIDTH,
            },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "group-notices-date",
            token: "date",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: NOTICE_COL_WIDTH,
            },
            align: TableAlign::Start,
            sortable: true,
        },
    ],
    default_sort: &[TableSortDefault {
        column: 2,
        ascending: false,
    }],
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: Some(NOTICES_SORT_SETTING),
    widths_setting: Some(NOTICES_WIDTHS_SETTING),
};

/// The roles table: a flexible Name over fixed Title / Members columns, all
/// sortable, defaulting to name-ascending. Column indices match
/// [`role_column_ordering`].
const ROLES_TABLE: TableSpec = TableSpec {
    element: "group-roles",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "group-roles-col-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "group-roles-col-title",
            token: "title",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: ROLE_TITLE_COL_WIDTH,
            },
            align: TableAlign::Start,
            sortable: true,
        },
        TableColumn {
            header_key: "group-roles-col-members",
            token: "members",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed {
                default: ROLE_MEMBERS_COL_WIDTH,
            },
            align: TableAlign::End,
            sortable: true,
        },
    ],
    default_sort: &[TableSortDefault {
        column: 0,
        ascending: true,
    }],
    builtin_sort: true,
    row_height: ROW_HEIGHT,
    font_size: FONT_SIZE,
    header_color: DIM_LABEL_COLOR,
    cell_color: LABEL_COLOR,
    column_gap: 4.0,
    row_padding: 4.0,
    sort_setting: Some(ROLES_SORT_SETTING),
    widths_setting: Some(ROLES_WIDTHS_SETTING),
};

/// The named group powers shown in the abilities viewer, each with the Fluent key
/// for its human-readable label. Only the commonly-set powers from
/// [`group_powers`] are named; the full 64-bit space is not enumerated.
const ABILITIES: [(u64, &str); 14] = [
    (group_powers::MEMBER_INVITE, "group-power-member-invite"),
    (group_powers::MEMBER_EJECT, "group-power-member-eject"),
    (group_powers::MEMBER_OPTIONS, "group-power-member-options"),
    (group_powers::ROLE_CREATE, "group-power-role-create"),
    (group_powers::ROLE_DELETE, "group-power-role-delete"),
    (group_powers::ROLE_PROPERTIES, "group-power-role-properties"),
    (
        group_powers::ROLE_ASSIGN_MEMBER_LIMITED,
        "group-power-role-assign-limited",
    ),
    (group_powers::ROLE_ASSIGN_MEMBER, "group-power-role-assign"),
    (group_powers::ROLE_REMOVE_MEMBER, "group-power-role-remove"),
    (
        group_powers::ROLE_CHANGE_ACTIONS,
        "group-power-role-change-actions",
    ),
    (
        group_powers::GROUP_CHANGE_IDENTITY,
        "group-power-change-identity",
    ),
    (group_powers::LAND_DEED, "group-power-land-deed"),
    (group_powers::NOTICES_SEND, "group-power-notices-send"),
    (group_powers::NOTICES_RECEIVE, "group-power-notices-receive"),
];

// ---------------------------------------------------------------------------
// Message.
// ---------------------------------------------------------------------------

/// Open the group profile floater on a group (from the Groups list's Info button).
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenGroupProfile {
    /// The group whose profile to show.
    pub(crate) group: GroupKey,
}

/// The set of group-notice ids the Notices tab has explicitly **requested** (by
/// clicking a notice in the list). The reply arrives as an
/// [`ImDialog::GroupNotice`] IM indistinguishable from an unsolicited push, so
/// the group-notice toast host ([`crate::group_notice`]) consults this set to
/// suppress a toast for a notice the user pulled up to read here — the reference
/// distinction between `IM_GROUP_NOTICE_REQUESTED` (no popup) and a fresh
/// `IM_GROUP_NOTICE`.
#[derive(Resource, Debug, Default)]
pub(crate) struct RequestedGroupNotices(HashSet<GroupNoticeKey>);

impl RequestedGroupNotices {
    /// Mark `notice` as explicitly requested by the Notices tab.
    pub(crate) fn mark_requested(&mut self, notice: GroupNoticeKey) {
        self.0.insert(notice);
    }

    /// Consume a requested `notice` — returns `true` (and forgets it) when the
    /// notice was one the tab requested, so the toast host can suppress its popup.
    pub(crate) fn take_requested(&mut self, notice: GroupNoticeKey) -> bool {
        self.0.remove(&notice)
    }
}

// ---------------------------------------------------------------------------
// Pure member roster (accumulated, deduplicated).
// ---------------------------------------------------------------------------

/// The accumulating member roster: SL returns the roster over several
/// [`SlSessionEvent::GroupMembers`] replies and caps it, so members are collected
/// across replies and deduplicated by agent id, with the simulator's reported
/// total kept for the "loaded N of TOTAL" line. Pure, so it is unit-testable.
#[derive(Debug, Default)]
struct MemberRoster {
    /// The members collected so far, in arrival order.
    members: Vec<GroupMember>,
    /// The agent ids already collected, to drop duplicate replies.
    seen: HashSet<AgentKey>,
    /// The total member count the simulator last reported (0 until a reply).
    total: usize,
}

impl MemberRoster {
    /// Fold one [`SlSessionEvent::GroupMembers`] reply chunk in, updating the total
    /// and appending only members not already seen.
    fn apply(&mut self, member_count: usize, chunk: &[GroupMember]) {
        self.total = member_count.max(self.total);
        for member in chunk {
            if self.seen.insert(member.agent_id) {
                self.members.push(member.clone());
            }
        }
    }

    /// The number of members collected so far.
    const fn loaded(&self) -> usize {
        self.members.len()
    }
}

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// Which detail the Members & Roles tab's lower area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DetailsFocus {
    /// Nothing selected — a hint line.
    #[default]
    None,
    /// A member is selected (eject + role-assignment).
    Member(AgentKey),
    /// A role is selected (abilities + edit + delete).
    Role(Option<GroupRoleKey>),
}

/// The group profile floater's live state: the shown group and everything received
/// about it so far, plus the transient edit drafts kept outside the rebuilt
/// widgets so a repaint does not lose them.
#[derive(Resource, Debug, Default)]
pub(crate) struct GroupProfileState {
    /// The group shown, or `None` before the first open.
    target: Option<GroupKey>,
    /// The group's profile (general facts + the requesting agent's powers).
    profile: Option<GroupProfile>,
    /// The accumulated member roster.
    roster: MemberRoster,
    /// The group's roles, once received.
    roles: Vec<GroupRole>,
    /// The group's role↔member pairings, accumulated and deduplicated.
    role_members: Vec<GroupRoleMember>,
    /// The agent's selectable titles in the group, once received.
    titles: Vec<GroupTitle>,
    /// The index into [`titles`](Self::titles) the title cycle currently shows.
    title_index: usize,
    /// The group's notice headers, once received.
    notices: Vec<GroupNotice>,
    /// The selected notice's index into [`notices`](Self::notices).
    selected_notice: Option<usize>,
    /// The notice whose full body was last requested but not yet received.
    pending_notice: Option<GroupNoticeKey>,
    /// Fetched full notice bodies, by notice id.
    notice_bodies: HashMap<GroupNoticeKey, String>,
    /// Which detail the Members & Roles lower area shows.
    focus: DetailsFocus,
    /// The general-tab identity edit draft (open enrollment / mature / show-in-list),
    /// initialised from the profile and flipped by the toggles.
    general_draft: GeneralDraft,
    /// The agent's "receive notices" toggle for this group (local, sent on change).
    accept_notices: bool,
    /// The agent's "list in my profile" toggle for this group (local; the wire
    /// membership does not carry it, so it defaults on — the reference default).
    list_in_profile: bool,
    /// The selected role's powers edit draft, initialised when a role is selected.
    role_power_draft: u64,
    /// Insignia texture awaited from the pipeline, with the node to hand it to.
    pending_texture: Option<(TextureKey, Entity)>,
    /// Bumped when the member roster changes, so the members view rebuilds.
    members_revision: u64,
    /// Bumped when the notice list changes, so the notices view rebuilds.
    notices_revision: u64,
    /// Bumped when the role list changes, so the roles view rebuilds.
    roles_revision: u64,
}

impl GroupProfileState {
    /// Reset everything to a fresh open on `target`.
    fn reset(&mut self, target: GroupKey) {
        *self = Self {
            target: Some(target),
            list_in_profile: true,
            ..Self::default()
        };
    }

    /// Whether the requesting agent holds `power` in this group.
    fn has_power(&self, power: u64) -> bool {
        self.profile
            .as_ref()
            .is_some_and(|profile| has_power(profile.powers, power))
    }

    /// The roles a member holds, from the accumulated role↔member pairs (the
    /// "Everyone" role, id `None`, is implicit for every member and not listed
    /// here).
    fn member_roles(&self, member: AgentKey) -> HashSet<Option<GroupRoleKey>> {
        self.role_members
            .iter()
            .filter(|pair| pair.member_id == member)
            .map(|pair| pair.role_id)
            .collect()
    }
}

/// The general-tab identity edit draft — the toggled booleans, kept live across a
/// repaint (the text fields carry their own values).
#[derive(Debug, Clone, Copy, Default)]
struct GeneralDraft {
    /// Whether enrollment is open (no invitation needed).
    open_enrollment: bool,
    /// Whether the group is flagged mature.
    mature: bool,
    /// Whether the group is shown in search / the group list.
    show_in_list: bool,
}

impl GeneralDraft {
    /// The draft matching a profile's stored flags.
    const fn from_profile(profile: &GroupProfile) -> Self {
        Self {
            open_enrollment: profile.open_enrollment,
            mature: profile.mature_publish,
            show_in_list: profile.show_in_list,
        }
    }
}

// ---------------------------------------------------------------------------
// Views (virtualized-list projections).
// ---------------------------------------------------------------------------

/// The ordered, render-ready member projection the virtualized members list binds
/// its recycled rows to.
#[derive(Resource, Debug, Default)]
struct MembersView {
    /// The rows in display order.
    rows: Vec<MemberRow>,
    /// The state revision this view was last built from.
    built_revision: u64,
    /// The table sort revision this view was last ordered at, so a header-click
    /// re-sort rebuilds the order without a data change.
    built_sort_revision: u64,
}

/// One member row's render-ready fields.
#[derive(Debug, Clone)]
struct MemberRow {
    /// The member's agent id.
    agent: AgentKey,
    /// The member's group title.
    title: String,
    /// The member's land contribution, formatted (e.g. "512 m²").
    contribution: String,
    /// The member's online status string.
    status: String,
    /// Whether the member is a group owner.
    is_owner: bool,
}

/// The ordered notice projection the virtualized notices list binds to.
#[derive(Resource, Debug, Default)]
struct NoticesView {
    /// The rows in display order (newest first by default).
    rows: Vec<NoticeRow>,
    /// The state revision this view was last built from.
    built_revision: u64,
    /// The table sort revision this view was last ordered at.
    built_sort_revision: u64,
}

/// The ordered role projection the virtualized roles list binds to.
#[derive(Resource, Debug, Default)]
struct RolesView {
    /// The rows in display order.
    rows: Vec<RoleRowData>,
    /// The state revision this view was last built from.
    built_revision: u64,
    /// The table sort revision this view was last ordered at.
    built_sort_revision: u64,
}

/// One role row's render-ready fields.
#[derive(Debug, Clone)]
struct RoleRowData {
    /// The role id (`None` = the "Everyone" default role).
    role_id: Option<GroupRoleKey>,
    /// The role name.
    name: String,
    /// The role title (worn over members holding it).
    title: String,
    /// The number of members holding the role.
    members: u32,
}

/// One notice row's render-ready fields.
#[derive(Debug, Clone)]
struct NoticeRow {
    /// The notice's index into [`GroupProfileState::notices`].
    index: usize,
    /// The notice subject.
    subject: String,
    /// The poster's name.
    from_name: String,
    /// The posted date, formatted.
    date: String,
    /// The raw posted timestamp, for sorting the Date column.
    timestamp: u32,
    /// Whether the notice carries an inventory attachment.
    has_attachment: bool,
}

// ---------------------------------------------------------------------------
// Dirty flags for the rebuilt sub-panels.
// ---------------------------------------------------------------------------

/// Which rebuilt sub-panels need repainting from [`GroupProfileState`].
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent repaint flag per rebuilt sub-panel; a bitflags newtype \
              would only obscure the field-per-panel intent"
)]
#[derive(Resource, Debug, Default)]
struct GroupProfileDirty {
    /// The General tab's **structure** — set only when the layout could change
    /// (profile powers / membership); the tab is despawned+rebuilt only then, never
    /// on a value update.
    general: bool,
    /// The General tab's **values** — set when a displayed value changes (founder
    /// name, counts, fee, a toggled flag, the active title); handled in place by
    /// [`update_general_values`] with no respawn.
    general_values: bool,
    /// The Members & Roles lower details area.
    details: bool,
    /// The notice compose area.
    compose: bool,
    /// The notice body view.
    notice_body: bool,
}

impl GroupProfileDirty {
    /// Mark everything dirty (a fresh open, or a profile reply that feeds every
    /// sub-panel).
    const fn mark_all(&mut self) {
        self.general = true;
        self.general_values = true;
        self.details = true;
        self.compose = true;
        self.notice_body = true;
    }
}

// ---------------------------------------------------------------------------
// UI handles.
// ---------------------------------------------------------------------------

/// The General tab's **structure** signature: its layout (which controls exist)
/// depends only on these, so the tab is despawned+rebuilt only when one changes
/// (rare — powers / membership), never on a value update.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent layout predicates; a bitflags newtype would only obscure \
              the one-predicate-per-branch intent"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeneralSig {
    /// Whether the agent is a member (drives the membership controls vs Join).
    is_member: bool,
    /// Whether the agent can edit the group's identity (charter/mature/list).
    can_edit_identity: bool,
    /// Whether the agent can edit membership options (open enrollment / fee).
    can_edit_options: bool,
    /// Whether a Join button is shown (non-member + open enrollment).
    join_shown: bool,
}

/// Retained handles to the General tab's value nodes, so a reply updates values in
/// place ([`update_general_values`]) instead of respawning the tab. `None` for a
/// node this structure variant does not show.
#[derive(Debug, Default)]
struct GeneralHandles {
    /// The founder name value node.
    founder: Option<Entity>,
    /// The "members / roles" counts value node.
    counts: Option<Entity>,
    /// The join-fee display value node.
    fee_display: Option<Entity>,
    /// The open-enrollment flag glyph.
    open_enrollment_glyph: Option<Entity>,
    /// The mature flag glyph.
    mature_glyph: Option<Entity>,
    /// The show-in-list flag glyph.
    show_in_list_glyph: Option<Entity>,
    /// The receive-notices flag glyph (members only).
    accept_notices_glyph: Option<Entity>,
    /// The list-in-profile flag glyph (members only).
    list_in_profile_glyph: Option<Entity>,
    /// The active-title text node (members only).
    title_text: Option<Entity>,
}

/// Entity handles for the group profile floater: the shell spawned once at
/// startup, the persistent list viewports, the rebuild-target containers, and the
/// per-rebuild field entities the action handlers read.
#[derive(Resource)]
pub(crate) struct GroupProfileUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The title text node (set to the group's name once known).
    title_text: Entity,
    /// The General tab panel (rebuilt on `general`).
    general_panel: Entity,
    /// The members table root (carries the widget's [`TableState`] — read for the
    /// current sort order).
    members_table: Entity,
    /// The virtualized members list viewport.
    members_viewport: Entity,
    /// The members header line ("loaded N of TOTAL").
    members_count_text: Entity,
    /// The roles table root (carries the widget's [`TableState`]).
    roles_table: Entity,
    /// The virtualized roles list viewport.
    roles_viewport: Entity,
    /// The persistent container below the roles table holding the New Role button
    /// (built once, so it never disturbs the pooled rows).
    roles_new_container: Entity,
    /// Whether the New Role button has been built (once, when the create power is
    /// known).
    roles_new_built: bool,
    /// The Members & Roles lower details area (rebuilt on `details`).
    details_area: Entity,
    /// The focus the details area was last built for — the hint (focus `None`) is
    /// built once and not respawned during the reply burst; a member/role selection
    /// (user-paced) rebuilds it.
    details_built: Option<DetailsFocus>,
    /// Whether the compose area was last built for a notices-send-capable agent
    /// (`None` = not built) — built once when the power is known, so the reply burst
    /// never respawns it.
    compose_can_send: Option<bool>,
    /// The `(selected notice, body-present)` the notice-body view was last built
    /// for — built once for the hint and rebuilt only on a user-paced selection /
    /// its body arriving.
    notice_body_built: Option<(Option<usize>, bool)>,
    /// The notices table root (carries the widget's [`TableState`]).
    notices_table: Entity,
    /// The virtualized notices list viewport.
    notices_viewport: Entity,
    /// The notice body view container (rebuilt on `notice_body`).
    notice_body_area: Entity,
    /// The notice compose area (rebuilt on `compose`).
    compose_area: Entity,
    /// The General tab's current structure signature — its layout is rebuilt only
    /// when this changes, never on a value update.
    general_sig: Option<GeneralSig>,
    /// Retained handles to the General tab's value nodes (in-place updates).
    general_handles: GeneralHandles,
    /// The General tab's charter field (identity editors only).
    charter_field: Option<Entity>,
    /// The General tab's membership-fee field (identity editors only).
    fee_field: Option<Entity>,
    /// The selected role's name field (role-properties editors only).
    role_name_field: Option<Entity>,
    /// The selected role's title field (role-properties editors only).
    role_title_field: Option<Entity>,
    /// The selected role's description field (role-properties editors only).
    role_desc_field: Option<Entity>,
    /// The notice compose subject field.
    notice_subject_field: Option<Entity>,
    /// The notice compose message field.
    notice_message_field: Option<Entity>,
}

/// A button in the group profile floater, naming what it does. One observer
/// ([`on_group_profile_action`]) dispatches on this.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum GroupProfileAction {
    /// Join this open-enrollment group.
    Join,
    /// Save the General tab's identity edits (`UpdateGroupInfo`).
    SaveGeneral,
    /// Toggle "receive notices" for the agent's membership.
    ToggleAcceptNotices,
    /// Toggle "list this group in my profile".
    ToggleListInProfile,
    /// Toggle the identity draft's "open enrollment".
    ToggleOpenEnrollment,
    /// Toggle the identity draft's "mature".
    ToggleMature,
    /// Toggle the identity draft's "show in list / search".
    ToggleShowInList,
    /// Cycle the agent's active title.
    CycleTitle,
    /// Re-fetch the member roster (top up a partial / capped first load).
    RefreshMembers,
    /// Clear the member/role selection, returning the details area to its hint.
    CloseDetails,
    /// Eject the selected member.
    EjectMember,
    /// Add the selected member to, or remove them from, `role` (the toggle knows
    /// the member from [`DetailsFocus::Member`]).
    ToggleMemberRole(Option<GroupRoleKey>),
    /// Create a new role.
    NewRole,
    /// Delete the selected role.
    DeleteRole,
    /// Save the selected role's name / title / description.
    SaveRoleData,
    /// Toggle a power bit in the selected role's powers draft.
    ToggleRolePower(u64),
    /// Save the selected role's powers draft.
    SaveRolePowers,
    /// Send the composed notice.
    SendNotice,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin owning the group profile floater.
pub(crate) struct GroupProfilePlugin;

impl Plugin for GroupProfilePlugin {
    /// Register the state, the open message, and the spawn / open / ingest /
    /// rebuild / poll systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<GroupProfileState>()
            .init_resource::<GroupProfileDirty>()
            .init_resource::<MembersView>()
            .init_resource::<NoticesView>()
            .init_resource::<RolesView>()
            .init_resource::<RequestedGroupNotices>()
            .add_message::<OpenGroupProfile>()
            .add_systems(
                Startup,
                (
                    register_group_profile_settings,
                    spawn_group_profile_floater.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (
                    open_group_profile,
                    ingest_group_profile_events,
                    sync_members_view,
                    sync_notices_view,
                    sync_roles_view,
                    build_general_tab,
                    build_roles_new_button,
                    rebuild_details_area,
                    rebuild_compose_area,
                    rebuild_notice_body,
                    poll_group_profile_texture,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    populate_member_rows,
                    bind_member_rows,
                    populate_notice_rows,
                    bind_notice_rows,
                    populate_role_rows,
                    bind_role_rows,
                )
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// Register the members / notices tables' persisted sort-order and column-width
/// settings, so the account file that loads at login is coerced to the right
/// types (the widget's seed / persist systems then drive them).
fn register_group_profile_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    register_table_settings(&mut settings, TABLE_SETTINGS_SECTION, &MEMBERS_TABLE);
    register_table_settings(&mut settings, TABLE_SETTINGS_SECTION, &NOTICES_TABLE);
    register_table_settings(&mut settings, TABLE_SETTINGS_SECTION, &ROLES_TABLE);
}

/// Spawn the (hidden) group profile floater shell: the floater, the three-tab
/// container, and the persistent list viewports + rebuild-target containers.
fn spawn_group_profile_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "group-profile",
            title: "Group".to_owned(),
            position: Vec2::new(340.0, 90.0),
            default_size: Some(Vec2::new(520.0, 620.0)),
            min_size: Some(Vec2::new(420.0, 440.0)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    // Subject-bound: the target group is not persisted, so neither is the floater
    // — no restored rectangle, no restored "open" (an empty shell).
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands
        .entity(handle.title_text)
        .insert(Translated::new("group-profile-title"));
    let labels: Vec<String> = [
        "group-profile-tab-general",
        "group-profile-tab-members",
        "group-profile-tab-notices",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let tabs: TabContainerHandle = spawn_tab_container(
        &mut commands,
        handle.content,
        &TabSpec {
            element: "group-profile-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);
    let general_panel = tabs.panels.first().copied().unwrap_or(handle.content);
    let members_panel = tabs.panels.get(1).copied().unwrap_or(handle.content);
    let notices_panel = tabs.panels.get(2).copied().unwrap_or(handle.content);

    let (
        members_table,
        members_viewport,
        members_count_text,
        roles_table,
        roles_viewport,
        roles_new_container,
        details_area,
    ) = build_members_scaffold(&mut commands, members_panel);
    let (notices_table, notices_viewport, notice_body_area, compose_area) =
        build_notices_scaffold(&mut commands, notices_panel);

    commands.insert_resource(GroupProfileUi {
        panel: handle.root,
        title_text: handle.title_text,
        general_panel,
        members_table,
        members_viewport,
        members_count_text,
        roles_table,
        roles_viewport,
        roles_new_container,
        roles_new_built: false,
        details_area,
        details_built: None,
        compose_can_send: None,
        notice_body_built: None,
        notices_table,
        notices_viewport,
        notice_body_area,
        compose_area,
        general_sig: None,
        general_handles: GeneralHandles::default(),
        charter_field: None,
        fee_field: None,
        role_name_field: None,
        role_title_field: None,
        role_desc_field: None,
        notice_subject_field: None,
        notice_message_field: None,
    });
}

/// Build the persistent Members & Roles tab skeleton: a members column (the table
/// widget's header + virtualized viewport) over a roles table, over a details
/// area. Returns the `(members_table, members_viewport, members_count_text,
/// roles_table, roles_viewport, roles_new_container, details_area)`.
fn build_members_scaffold(
    commands: &mut Commands,
    panel: Entity,
) -> (Entity, Entity, Entity, Entity, Entity, Entity, Entity) {
    // Vertical stack: members list (grows) over the roles list over the details
    // area — the reference lays the roles list below the members, not beside.
    let members_column = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(2.0))
            },
            ChildOf(panel),
        ))
        .id();
    // The count line ("loaded N of TOTAL") beside a Refresh button — the first SL
    // fetch is just officers / owners, so Refresh pulls the full roster.
    let count_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            ChildOf(members_column),
        ))
        .id();
    let members_count_text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(count_row),
        ))
        .id();
    spawn_action_button(
        commands,
        count_row,
        "group-members-refresh",
        GroupProfileAction::RefreshMembers,
        0,
    );
    // The members table (widget-owned header + virtualized, sortable, resizable
    // columns) fills the space below the count line.
    let members_table = spawn_table(commands, members_column, &MEMBERS_TABLE);
    let members_viewport = members_table.viewport;
    commands
        .entity(members_viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(2)));

    // The roles column below the members: the roles table (widget-owned header +
    // virtualized, sortable, resizable columns), bounded so it scrolls rather than
    // pushing the details area away.
    let roles_column = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                // A *definite* height, not just a max: the roles table's virtualized
                // rows are absolute-positioned and contribute no content height, so a
                // content-sized container would collapse its viewport to nothing.
                height: Val::Px(ROLES_LIST_HEIGHT),
                min_height: Val::Px(0.0),
                ..column(Val::Px(2.0))
            },
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("group-roles-header"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(roles_column),
    ));
    let roles_table = spawn_table(commands, roles_column, &ROLES_TABLE);
    let roles_viewport = roles_table.viewport;
    commands
        .entity(roles_viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(2)));
    // The New Role button lives in its own persistent container below the roles
    // table, so it is built once and never disturbs the pooled rows.
    let roles_new_container = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                ..column(Val::Px(2.0))
            },
            Name::new("group-roles-new"),
            ChildOf(roles_column),
        ))
        .id();

    // The lower details area, rebuilt on selection.
    let details_area = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(6.0)),
                ..column(Val::Px(4.0))
            },
            BorderColor::all(BUTTON_BORDER),
            Name::new("group-details-area"),
            ChildOf(panel),
        ))
        .id();

    (
        members_table.root,
        members_viewport,
        members_count_text,
        roles_table.root,
        roles_viewport,
        roles_new_container,
        details_area,
    )
}

/// Build the persistent Notices tab skeleton: the notices table (widget-owned
/// header + virtualized, sortable, resizable columns), a body view, and a compose
/// area. Returns `(notices_table, notices_viewport, notice_body_area,
/// compose_area)`.
fn build_notices_scaffold(
    commands: &mut Commands,
    panel: Entity,
) -> (Entity, Entity, Entity, Entity) {
    let notices_table = spawn_table(commands, panel, &NOTICES_TABLE);
    let notices_viewport = notices_table.viewport;
    commands
        .entity(notices_viewport)
        .insert((BackgroundColor(LIST_BACKGROUND), TabIndex(2)));
    let notice_body_area = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                min_height: Val::Px(60.0),
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(6.0)),
                ..column(Val::Px(4.0))
            },
            BorderColor::all(BUTTON_BORDER),
            Name::new("group-notice-body"),
            ChildOf(panel),
        ))
        .id();
    let compose_area = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(6.0)),
                ..column(Val::Px(4.0))
            },
            BorderColor::all(BUTTON_BORDER),
            Name::new("group-notice-compose"),
            ChildOf(panel),
        ))
        .id();
    (
        notices_table.root,
        notices_viewport,
        notice_body_area,
        compose_area,
    )
}

// ---------------------------------------------------------------------------
// Open / ingest.
// ---------------------------------------------------------------------------

/// Open the floater on a group: reset the state, fire the profile requests, and
/// mark every sub-panel for rebuild. Opening on a **different** group is the one
/// place the retained structure is torn down (a single, user-paced teardown —
/// never a per-reply respawn).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the open \
              messages, the state, the dirty flags, the UI handles + children query for the \
              teardown, the groups model, the panel-shown query, and the command sinks"
)]
fn open_group_profile(
    mut opens: MessageReader<OpenGroupProfile>,
    mut state: ResMut<GroupProfileState>,
    mut dirty: ResMut<GroupProfileDirty>,
    ui: Option<ResMut<GroupProfileUi>>,
    groups: Res<GroupsModel>,
    children: Query<&Children>,
    mut panels: Query<&mut UiPanelShown>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut commands: Commands,
) {
    let Some(open) = opens.read().last().copied() else {
        return;
    };
    let Some(mut ui) = ui else {
        return;
    };
    let group = open.group;
    if state.target != Some(group) {
        state.reset(group);
        // Seed the membership toggles from the groups model (login-time push).
        state.accept_notices = groups.accepts_notices(group).unwrap_or(true);
        fetch_group(group, &mut sl_commands);
        // Clear the previous group's retained structure so each panel rebuilds for
        // the new subject (the settled old content is safe to despawn here — a
        // single user-paced teardown, never a per-reply respawn).
        despawn_children(&children, &mut commands, ui.general_panel);
        ui.general_sig = None;
        ui.general_handles = GeneralHandles::default();
        ui.charter_field = None;
        ui.fee_field = None;
        // The roles table's pooled rows are the widget's — they rebind to the new
        // group's roles (RolesView rebuilds on the fetch), so only the once-built
        // New Role button is torn down here.
        despawn_children(&children, &mut commands, ui.roles_new_container);
        ui.roles_new_built = false;
        // The details / compose / notice-body panels self-despawn+rebuild once via
        // their guards; resetting the guards makes them rebuild for the new subject.
        ui.details_built = None;
        ui.compose_can_send = None;
        ui.notice_body_built = None;
    }
    dirty.mark_all();
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
}

/// Fire every group-profile request for `group` (both the CAPS and UDP member
/// fetches, so both grids answer — duplicates are deduplicated).
fn fetch_group(group: GroupKey, sl_commands: &mut MessageWriter<SlCommand>) {
    sl_commands.write(SlCommand(Command::RequestGroupProfile(group)));
    sl_commands.write(SlCommand(Command::FetchGroupMembers(group)));
    sl_commands.write(SlCommand(Command::RequestGroupMembers(group)));
    sl_commands.write(SlCommand(Command::RequestGroupRoles(group)));
    sl_commands.write(SlCommand(Command::RequestGroupRoleMembers(group)));
    sl_commands.write(SlCommand(Command::RequestGroupTitles(group)));
    sl_commands.write(SlCommand(Command::RequestGroupNotices(group)));
}

/// Fold group-related session events for the shown group into the state, marking
/// the affected sub-panels dirty and bumping the list revisions.
fn ingest_group_profile_events(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<GroupProfileState>,
    mut dirty: ResMut<GroupProfileDirty>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(target) = state.target else {
        return;
    };
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::GroupProfileReceived(profile) if profile.group_id == target => {
                state.general_draft = GeneralDraft::from_profile(profile);
                state.profile = Some((**profile).clone());
                // Structure may change (powers/membership) + values need a refresh.
                dirty.general = true;
                dirty.general_values = true;
                dirty.compose = true;
                dirty.details = true;
            }
            SlSessionEvent::GroupMembers {
                group_id,
                member_count,
                members,
                ..
            } if *group_id == target => {
                let total = usize::try_from(*member_count).unwrap_or(0);
                state.roster.apply(total, members);
                // Resolve the newly-loaded members' names.
                let ids: Vec<AgentKey> = members.iter().map(|member| member.agent_id).collect();
                if !ids.is_empty() {
                    sl_commands.write(SlCommand(Command::RequestAvatarNames(ids)));
                }
                state.members_revision = state.members_revision.wrapping_add(1);
            }
            SlSessionEvent::GroupRoleData {
                group_id, roles, ..
            } if *group_id == target => {
                state.roles.clone_from(roles);
                state.roles_revision = state.roles_revision.wrapping_add(1);
                dirty.details = true;
            }
            SlSessionEvent::GroupRoleMembers {
                group_id, pairs, ..
            } if *group_id == target => {
                for pair in pairs {
                    if !state.role_members.contains(pair) {
                        state.role_members.push(*pair);
                    }
                }
                dirty.details = true;
            }
            SlSessionEvent::GroupTitles {
                group_id, titles, ..
            } if *group_id == target => {
                state.title_index = titles.iter().position(|title| title.selected).unwrap_or(0);
                state.titles.clone_from(titles);
                // The title text updates in place — no structure change.
                dirty.general_values = true;
            }
            SlSessionEvent::GroupNotices {
                group_id, notices, ..
            } if *group_id == target => {
                state.notices.clone_from(notices);
                state.notices_revision = state.notices_revision.wrapping_add(1);
            }
            SlSessionEvent::InstantMessageReceived(im)
                if im.dialog == ImDialog::GroupNotice && state.pending_notice.is_some() =>
            {
                if let Some(notice_id) = state.pending_notice.take() {
                    let body = split_notice(&im.message).1.to_owned();
                    state.notice_bodies.insert(notice_id, body);
                    dirty.notice_body = true;
                }
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// View sync (virtualized lists).
// ---------------------------------------------------------------------------

/// Rebuild [`MembersView`] when the roster **or the table sort** advances,
/// keeping the viewport's item count and the header count in step, and ordering
/// by the table widget's current (persisted / clicked) sort.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the state, the \
              view, the UI handles, the name source, the table-state + list + text queries"
)]
fn sync_members_view(
    state: Res<GroupProfileState>,
    mut view: ResMut<MembersView>,
    ui: Option<Res<GroupProfileUi>>,
    avatars: Res<AvatarState>,
    translator: Translator,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    // The current sort (revision + keys) from the widget; an empty default before
    // the table exists just leaves the roster in arrival order.
    let sort = tables
        .get(ui.members_table)
        .ok()
        .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
    let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
    if view.built_revision == state.members_revision && view.built_sort_revision == sort_revision {
        return;
    }
    view.built_revision = state.members_revision;
    view.built_sort_revision = sort_revision;
    view.rows = state
        .roster
        .members
        .iter()
        .map(|member| MemberRow {
            agent: member.agent_id,
            title: member.title.clone(),
            contribution: member.contribution.to_string(),
            status: member.online_status.clone(),
            is_owner: member.is_owner,
        })
        .collect();
    let keys = sort.map(|(_revision, keys)| keys).unwrap_or_default();
    view.rows
        .sort_by(|left, right| compare_members(&keys, left, right, &avatars));
    if let Ok(mut list) = lists.get_mut(ui.members_viewport) {
        list.item_count = view.rows.len();
        list.scroll_to_top();
    }
    let loaded = i64::try_from(state.roster.loaded()).unwrap_or(i64::MAX);
    let total = i64::try_from(state.roster.total).unwrap_or(i64::MAX);
    let label = translator.format(
        "group-members-count",
        &TransArgs::new().int("loaded", loaded).int("total", total),
    );
    if let Ok(mut text) = texts.get_mut(ui.members_count_text)
        && text.0 != label
    {
        text.0 = label;
    }
}

/// Order two member rows by the table's full sort-key stack, breaking ties by
/// resolved name then agent id so the order is stable across rebinds.
fn compare_members(
    keys: &[TableSortKey],
    left: &MemberRow,
    right: &MemberRow,
    avatars: &AvatarState,
) -> Ordering {
    for key in keys {
        let base = member_column_ordering(key.column, left, right, avatars);
        let ord = if key.ascending { base } else { base.reverse() };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    member_sort_key(left.agent, avatars)
        .cmp(&member_sort_key(right.agent, avatars))
        .then_with(|| left.agent.to_string().cmp(&right.agent.to_string()))
}

/// Order two member rows by a single column (before the direction is applied):
/// name / title / status case-folded, contribution numerically.
fn member_column_ordering(
    column: usize,
    left: &MemberRow,
    right: &MemberRow,
    avatars: &AvatarState,
) -> Ordering {
    match column {
        1 => left.title.to_lowercase().cmp(&right.title.to_lowercase()),
        2 => contribution_value(&left.contribution).cmp(&contribution_value(&right.contribution)),
        3 => left.status.to_lowercase().cmp(&right.status.to_lowercase()),
        _name => member_sort_key(left.agent, avatars).cmp(&member_sort_key(right.agent, avatars)),
    }
}

/// The numeric value of a contribution cell (the leading integer), for sorting
/// the Land column numerically rather than lexically.
fn contribution_value(text: &str) -> i64 {
    text.split_whitespace()
        .next()
        .and_then(|number| number.parse::<i64>().ok())
        .unwrap_or(0)
}

/// A member row's sort key: its resolved name lower-cased, falling back to its id.
fn member_sort_key(agent: AgentKey, avatars: &AvatarState) -> String {
    avatars
        .name_of(agent)
        .map_or_else(|| agent.to_string(), str::to_lowercase)
}

/// Rebuild [`NoticesView`] when the notice revision **or the table sort**
/// advances, keeping the viewport's item count in step and ordering by the table
/// widget's current sort (default: newest first).
fn sync_notices_view(
    state: Res<GroupProfileState>,
    mut view: ResMut<NoticesView>,
    ui: Option<Res<GroupProfileUi>>,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    let sort = tables
        .get(ui.notices_table)
        .ok()
        .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
    let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
    if view.built_revision == state.notices_revision && view.built_sort_revision == sort_revision {
        return;
    }
    view.built_revision = state.notices_revision;
    view.built_sort_revision = sort_revision;
    let mut rows: Vec<NoticeRow> = state
        .notices
        .iter()
        .enumerate()
        .map(|(index, notice)| NoticeRow {
            index,
            subject: notice.subject.clone(),
            from_name: notice.from_name.clone(),
            date: format_unix_date(i64::from(notice.timestamp)),
            timestamp: notice.timestamp,
            has_attachment: notice.has_attachment,
        })
        .collect();
    let keys = sort.map(|(_revision, keys)| keys).unwrap_or_default();
    rows.sort_by(|left, right| compare_notices(&keys, left, right));
    view.rows = rows;
    if let Ok(mut list) = lists.get_mut(ui.notices_viewport) {
        list.item_count = view.rows.len();
        list.scroll_to_top();
    }
}

/// Order two notice rows by the table's full sort-key stack, breaking ties by the
/// timestamp (newest first) so the order is stable.
fn compare_notices(keys: &[TableSortKey], left: &NoticeRow, right: &NoticeRow) -> Ordering {
    for key in keys {
        let base = notice_column_ordering(key.column, left, right);
        let ord = if key.ascending { base } else { base.reverse() };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    right.timestamp.cmp(&left.timestamp)
}

/// Order two notice rows by a single column (before the direction is applied):
/// subject / from case-folded, date by timestamp.
fn notice_column_ordering(column: usize, left: &NoticeRow, right: &NoticeRow) -> Ordering {
    match column {
        0 => left
            .subject
            .to_lowercase()
            .cmp(&right.subject.to_lowercase()),
        1 => left
            .from_name
            .to_lowercase()
            .cmp(&right.from_name.to_lowercase()),
        _date => left.timestamp.cmp(&right.timestamp),
    }
}

// ---------------------------------------------------------------------------
// Rebuild: General tab.
// ---------------------------------------------------------------------------

/// Drive the General tab: build its **structure once** (per subject / signature)
/// and update its **values in place** — never respawning the tab on a value
/// reply, so no node is spawned and despawned in the same frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the dirty \
              flags, the state, the UI handles, name / membership sources, the texture \
              pipeline, and the spawn outputs"
)]
fn build_general_tab(
    mut dirty: ResMut<GroupProfileDirty>,
    mut state: ResMut<GroupProfileState>,
    mut ui: ResMut<GroupProfileUi>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut textures: ResMut<TextureManager>,
    children: Query<&Children>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut commands: Commands,
) {
    if !dirty.general && !dirty.general_values {
        return;
    }
    let structure_dirty = dirty.general;
    dirty.general = false;
    dirty.general_values = false;
    let Some(target) = state.target else {
        return;
    };
    // Title: the group name once known (a plain string, not the Fluent key).
    if let Some(profile) = state.profile.as_ref()
        && !profile.name.is_empty()
        && let Ok((mut text, _)) = texts.get_mut(ui.title_text)
    {
        profile.name.clone_into(&mut text.0);
        commands.entity(ui.title_text).remove::<Translated>();
    }
    let panel = ui.general_panel;
    let Some(profile) = state.profile.clone() else {
        // No profile yet: a loading placeholder, built once on the fresh open (the
        // open teardown cleared the panel and the signature). Not respawned on a
        // value-only tick.
        if structure_dirty && ui.general_sig.is_none() {
            spawn_key_label(
                &mut commands,
                panel,
                "group-profile-loading",
                DIM_LABEL_COLOR,
            );
        }
        return;
    };
    let is_member = groups.is_member(target);
    let sig = GeneralSig {
        is_member,
        can_edit_identity: has_power(profile.powers, group_powers::GROUP_CHANGE_IDENTITY),
        can_edit_options: has_power(profile.powers, group_powers::MEMBER_OPTIONS),
        join_shown: !is_member && profile.open_enrollment,
    };
    // (Re)build the structure only when the layout could differ — first profile,
    // or a powers/membership change on a re-fetch (both rare and user-paced).
    if ui.general_sig != Some(sig) {
        despawn_children(&children, &mut commands, panel);
        ui.charter_field = None;
        ui.fee_field = None;
        ui.general_handles = GeneralHandles::default();
        build_general_structure(
            &mut commands,
            panel,
            target,
            &profile,
            sig,
            &mut state,
            &mut textures,
            &mut ui,
        );
        ui.general_sig = Some(sig);
    }
    update_general_values(&ui, &profile, &state, &avatars, &mut texts);
}

/// Spawn the General tab's fixed skeleton for `sig`, storing handles to every
/// value node in [`GroupProfileUi::general_handles`]. Called once per structure.
#[expect(
    clippy::too_many_arguments,
    reason = "a build helper threading the spawn target, the group + profile, the \
              signature, and the state / texture / handle sinks"
)]
fn build_general_structure(
    commands: &mut Commands,
    panel: Entity,
    target: GroupKey,
    profile: &GroupProfile,
    sig: GeneralSig,
    state: &mut GroupProfileState,
    textures: &mut TextureManager,
    ui: &mut GroupProfileUi,
) {
    // Insignia beside the identity facts.
    let top = commands
        .spawn((
            Node {
                align_items: AlignItems::FlexStart,
                ..row(Val::Px(8.0))
            },
            ChildOf(panel),
        ))
        .id();
    spawn_insignia(commands, top, profile.insignia_id, state, textures);
    let facts = commands
        .spawn((
            Node {
                ..column(Val::Px(4.0))
            },
            ChildOf(top),
        ))
        .id();
    let name_row = spawn_labeled_row(commands, facts, "group-profile-name");
    spawn_value_label(commands, name_row, profile.name.clone(), LABEL_COLOR);
    let key_row = spawn_labeled_row(commands, facts, "group-profile-key");
    spawn_value_label(commands, key_row, target.to_string(), DIM_LABEL_COLOR);
    let founder_row = spawn_labeled_row(commands, facts, "group-profile-founder");
    ui.general_handles.founder = Some(spawn_value_node(commands, founder_row));
    let counts_row = spawn_labeled_row(commands, facts, "group-profile-members-roles");
    ui.general_handles.counts = Some(spawn_value_node(commands, counts_row));
    let fee_row = spawn_labeled_row(commands, facts, "group-profile-join-fee");
    ui.general_handles.fee_display = Some(spawn_value_node(commands, fee_row));

    // Charter — a build-once editor for identity-holders (its value is never
    // overwritten so edits survive), a read block otherwise.
    spawn_section_label(commands, panel, "group-profile-charter");
    if sig.can_edit_identity {
        ui.charter_field = Some(spawn_text_input(
            commands,
            panel,
            &TextInputSpec {
                initial: profile.charter.clone(),
                font_size: FONT_SIZE,
                visible_lines: 4.0,
                tab_index: 2,
                max_characters: Some(511),
                ..TextInputSpec::new("group-charter", TextInputKind::Multiline)
            },
        ));
    } else {
        spawn_text_block(commands, panel, profile.charter.clone());
    }

    // Identity flags — editable toggles for identity-holders, read-only glyphs
    // otherwise. "Open enrollment" and the fee are gated on member-options.
    ui.general_handles.open_enrollment_glyph = Some(spawn_flag_row(
        commands,
        panel,
        "group-profile-open-enrollment",
        state.general_draft.open_enrollment,
        sig.can_edit_options
            .then_some(GroupProfileAction::ToggleOpenEnrollment),
    ));
    ui.general_handles.mature_glyph = Some(spawn_flag_row(
        commands,
        panel,
        "group-profile-mature",
        state.general_draft.mature,
        sig.can_edit_identity
            .then_some(GroupProfileAction::ToggleMature),
    ));
    ui.general_handles.show_in_list_glyph = Some(spawn_flag_row(
        commands,
        panel,
        "group-profile-show-in-list",
        state.general_draft.show_in_list,
        sig.can_edit_identity
            .then_some(GroupProfileAction::ToggleShowInList),
    ));
    if sig.can_edit_options {
        let fee_edit = spawn_labeled_row(commands, panel, "group-profile-join-fee");
        ui.fee_field = Some(spawn_text_input(
            commands,
            fee_edit,
            &TextInputSpec {
                initial: profile.membership_fee.0.to_string(),
                font_size: FONT_SIZE,
                width_glyphs: 8.0,
                tab_index: 3,
                ..TextInputSpec::new("group-fee", TextInputKind::NonNegativeInteger)
            },
        ));
    }
    if sig.can_edit_identity || sig.can_edit_options {
        let save_row = spawn_button_row(commands, panel);
        spawn_action_button(
            commands,
            save_row,
            "group-profile-save",
            GroupProfileAction::SaveGeneral,
            4,
        );
    }

    // The agent's own membership controls.
    spawn_section_label(commands, panel, "group-profile-my-membership");
    if sig.is_member {
        ui.general_handles.accept_notices_glyph = Some(spawn_flag_row(
            commands,
            panel,
            "group-profile-receive-notices",
            state.accept_notices,
            Some(GroupProfileAction::ToggleAcceptNotices),
        ));
        ui.general_handles.list_in_profile_glyph = Some(spawn_flag_row(
            commands,
            panel,
            "group-profile-list-in-profile",
            state.list_in_profile,
            Some(GroupProfileAction::ToggleListInProfile),
        ));
        let title_row = spawn_labeled_row(commands, panel, "group-profile-active-title");
        let cycle = spawn_cycle_button(commands, title_row, GroupProfileAction::CycleTitle, 5);
        ui.general_handles.title_text = Some(spawn_value_node(commands, cycle));
    } else {
        let join_row = spawn_button_row(commands, panel);
        if sig.join_shown {
            spawn_action_button(
                commands,
                join_row,
                "group-profile-join",
                GroupProfileAction::Join,
                5,
            );
        } else {
            spawn_key_label(
                commands,
                join_row,
                "group-profile-invite-only",
                DIM_LABEL_COLOR,
            );
        }
    }
}

/// Update the General tab's value nodes in place from the current state — the
/// founder name (resolves async), the counts / fee (a re-fetch), the flag glyphs
/// (toggled), and the active title (cycled). No respawn.
fn update_general_values(
    ui: &GroupProfileUi,
    profile: &GroupProfile,
    state: &GroupProfileState,
    avatars: &AvatarState,
    texts: &mut Query<(&mut Text, &mut TextColor)>,
) {
    let handles = &ui.general_handles;
    set_value_node(
        texts,
        handles.founder,
        &name_of(profile.founder_id, avatars),
    );
    set_value_node(
        texts,
        handles.counts,
        &format!("{} / {}", profile.member_count, profile.role_count),
    );
    set_value_node(
        texts,
        handles.fee_display,
        &format!("L$ {}", profile.membership_fee.0),
    );
    if let Some(glyph) = handles.open_enrollment_glyph {
        set_toggle_glyph(texts, glyph, state.general_draft.open_enrollment);
    }
    if let Some(glyph) = handles.mature_glyph {
        set_toggle_glyph(texts, glyph, state.general_draft.mature);
    }
    if let Some(glyph) = handles.show_in_list_glyph {
        set_toggle_glyph(texts, glyph, state.general_draft.show_in_list);
    }
    if let Some(glyph) = handles.accept_notices_glyph {
        set_toggle_glyph(texts, glyph, state.accept_notices);
    }
    if let Some(glyph) = handles.list_in_profile_glyph {
        set_toggle_glyph(texts, glyph, state.list_in_profile);
    }
    let title = state
        .titles
        .get(state.title_index)
        .map_or("", |title| title.title.as_str());
    set_value_node(texts, handles.title_text, title);
}

// ---------------------------------------------------------------------------
// Rebuild: roles list.
// ---------------------------------------------------------------------------

/// Rebuild [`RolesView`] when the role list **or the table sort** advances,
/// keeping the viewport's item count in step and ordering by the roles table's
/// current sort (default: name ascending).
fn sync_roles_view(
    state: Res<GroupProfileState>,
    mut view: ResMut<RolesView>,
    ui: Option<Res<GroupProfileUi>>,
    tables: Query<&TableState>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    let sort = tables
        .get(ui.roles_table)
        .ok()
        .map(|table| (table.sort_revision(), table.sort().keys().to_vec()));
    let sort_revision = sort.as_ref().map_or(0, |(revision, _keys)| *revision);
    if view.built_revision == state.roles_revision && view.built_sort_revision == sort_revision {
        return;
    }
    view.built_revision = state.roles_revision;
    view.built_sort_revision = sort_revision;
    view.rows = state
        .roles
        .iter()
        .map(|role| RoleRowData {
            role_id: role.role_id,
            name: role.name.clone(),
            title: role.title.clone(),
            members: role.members,
        })
        .collect();
    let keys = sort.map(|(_revision, keys)| keys).unwrap_or_default();
    view.rows
        .sort_by(|left, right| compare_roles(&keys, left, right));
    if let Ok(mut list) = lists.get_mut(ui.roles_viewport) {
        list.item_count = view.rows.len();
        list.scroll_to_top();
    }
}

/// Order two role rows by the table's full sort-key stack, breaking ties by name
/// (case-folded) so the order is stable.
fn compare_roles(keys: &[TableSortKey], left: &RoleRowData, right: &RoleRowData) -> Ordering {
    for key in keys {
        let base = role_column_ordering(key.column, left, right);
        let ord = if key.ascending { base } else { base.reverse() };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    left.name.to_lowercase().cmp(&right.name.to_lowercase())
}

/// Order two role rows by a single column (before the direction is applied):
/// name / title case-folded, members numerically.
fn role_column_ordering(column: usize, left: &RoleRowData, right: &RoleRowData) -> Ordering {
    match column {
        1 => left.title.to_lowercase().cmp(&right.title.to_lowercase()),
        2 => left.members.cmp(&right.members),
        _name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    }
}

/// Build the New Role button once, when the create power is known — its own
/// build-once system now the roles list is a widget-owned table.
fn build_roles_new_button(
    state: Res<GroupProfileState>,
    ui: Option<ResMut<GroupProfileUi>>,
    mut commands: Commands,
) {
    let Some(mut ui) = ui else {
        return;
    };
    if ui.roles_new_built || !state.has_power(group_powers::ROLE_CREATE) {
        return;
    }
    let new_container = ui.roles_new_container;
    let row = spawn_button_row(&mut commands, new_container);
    spawn_action_button(
        &mut commands,
        row,
        "group-role-new",
        GroupProfileAction::NewRole,
        0,
    );
    ui.roles_new_built = true;
}

/// The role a pooled row currently presents (its id, `None` = the "Everyone"
/// default role), or [`Parked`](BoundRole::Parked) when the pool row is hidden.
#[derive(Component, Debug, Clone, Copy)]
enum BoundRole {
    /// The row is hidden (the pool has more rows than the window needs).
    Parked,
    /// The row shows the role with this id (`None` = "Everyone").
    Bound(Option<GroupRoleKey>),
}

/// Build each newly-pooled role row's cells once, attaching the selection state
/// and press observer.
fn populate_role_rows(
    mut commands: Commands,
    ui: Option<Res<GroupProfileUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.roles_viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.roles_table, &ROLES_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundRole::Parked)
            .observe(on_role_row_press);
    }
}

/// Bind each pooled role row to the [`RoleRowData`] it now points at.
fn bind_role_rows(
    view: Res<RolesView>,
    state: Res<GroupProfileState>,
    ui: Option<Res<GroupProfileUi>>,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundRole,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || state.is_changed();
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.roles_viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let Some(index) = row.index else {
            continue;
        };
        let Some(role_row) = view.rows.get(index) else {
            continue;
        };
        *bound = BoundRole::Bound(role_row.role_id);
        let selected = state.focus == DetailsFocus::Role(role_row.role_id);
        set_row_cell(&mut texts, cells, 0, &role_row.name, selected);
        set_row_cell(&mut texts, cells, 1, &role_row.title, false);
        set_row_cell(&mut texts, cells, 2, &role_row.members.to_string(), false);
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if selected {
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

/// Select a role on press, showing its details and seeding its powers draft.
fn on_role_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&BoundRole>,
    ui: Res<GroupProfileUi>,
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<GroupProfileState>,
    mut dirty: ResMut<GroupProfileDirty>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.roles_viewport, FocusCause::Navigated);
    let Ok(bound) = rows.get(press.entity) else {
        return;
    };
    let BoundRole::Bound(role_id) = *bound else {
        return;
    };
    state.focus = DetailsFocus::Role(role_id);
    state.role_power_draft = state
        .roles
        .iter()
        .find(|role| role.role_id == role_id)
        .map_or(0, |role| role.powers);
    dirty.details = true;
}

// ---------------------------------------------------------------------------
// Rebuild: details area.
// ---------------------------------------------------------------------------

/// Rebuild the Members & Roles details area when it is dirty, from the current
/// selection focus (a member, a role, or nothing).
fn rebuild_details_area(
    mut dirty: ResMut<GroupProfileDirty>,
    mut ui: ResMut<GroupProfileUi>,
    state: Res<GroupProfileState>,
    avatars: Res<AvatarState>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    if !dirty.details {
        return;
    }
    dirty.details = false;
    // The hint (nothing selected) is built once and left alone through the reply
    // burst; a member/role selection (user-paced) is what rebuilds the area.
    if state.focus == DetailsFocus::None && ui.details_built == Some(DetailsFocus::None) {
        return;
    }
    ui.details_built = Some(state.focus);
    let area = ui.details_area;
    despawn_children(&children, &mut commands, area);
    ui.role_name_field = None;
    ui.role_title_field = None;
    ui.role_desc_field = None;
    match state.focus {
        DetailsFocus::None => {
            spawn_key_label(&mut commands, area, "group-details-hint", DIM_LABEL_COLOR);
        }
        DetailsFocus::Member(member) => {
            spawn_details_header(&mut commands, area);
            build_member_details(&mut commands, area, member, &state, &avatars);
        }
        DetailsFocus::Role(role_id) => {
            spawn_details_header(&mut commands, area);
            build_role_details(&mut commands, area, role_id, &state, &mut ui);
        }
    }
}

/// A details-area header: a Close button that clears the selection (returning the
/// area to its hint), so the user is never stranded in a member/role detail.
fn spawn_details_header(commands: &mut Commands, area: Entity) {
    let row = spawn_button_row(commands, area);
    spawn_action_button(
        commands,
        row,
        "group-details-close",
        GroupProfileAction::CloseDetails,
        0,
    );
}

/// Build the selected member's details: name, an Eject button (power-gated), and a
/// role-assignment checklist.
fn build_member_details(
    commands: &mut Commands,
    area: Entity,
    member: AgentKey,
    state: &GroupProfileState,
    avatars: &AvatarState,
) {
    let name_row = spawn_labeled_row(commands, area, "group-details-member");
    spawn_value_label(commands, name_row, name_of(member, avatars), LABEL_COLOR);
    if state.has_power(group_powers::MEMBER_EJECT) {
        let row = spawn_button_row(commands, area);
        spawn_action_button(
            commands,
            row,
            "group-member-eject",
            GroupProfileAction::EjectMember,
            0,
        );
    }
    spawn_section_label(commands, area, "group-details-roles-of-member");
    let held = state.member_roles(member);
    let can_assign = state.has_power(group_powers::ROLE_ASSIGN_MEMBER);
    let can_remove = state.has_power(group_powers::ROLE_REMOVE_MEMBER);
    for role in &state.roles {
        // Everyone (id None) is implicit and cannot be assigned or removed.
        let everyone = role.role_id.is_none();
        let on = everyone || held.contains(&role.role_id);
        let action = if everyone {
            None
        } else if on {
            can_remove.then_some(GroupProfileAction::ToggleMemberRole(role.role_id))
        } else {
            can_assign.then_some(GroupProfileAction::ToggleMemberRole(role.role_id))
        };
        spawn_toggle_row(commands, area, &role.name, on, action);
    }
}

/// Build the selected role's details: name / title / description editors
/// (power-gated), the abilities checklist, Save Powers, and Delete.
fn build_role_details(
    commands: &mut Commands,
    area: Entity,
    role_id: Option<GroupRoleKey>,
    state: &GroupProfileState,
    ui: &mut GroupProfileUi,
) {
    let Some(role) = state.roles.iter().find(|role| role.role_id == role_id) else {
        return;
    };
    let can_props = state.has_power(group_powers::ROLE_PROPERTIES) && role_id.is_some();
    let can_actions = state.has_power(group_powers::ROLE_CHANGE_ACTIONS) && role_id.is_some();
    let can_delete = state.has_power(group_powers::ROLE_DELETE) && role_id.is_some();

    let name_row = spawn_labeled_row(commands, area, "group-role-name-label");
    if can_props {
        ui.role_name_field = Some(spawn_text_input(
            commands,
            name_row,
            &TextInputSpec {
                initial: role.name.clone(),
                font_size: FONT_SIZE,
                width_glyphs: 16.0,
                tab_index: 0,
                max_characters: Some(63),
                ..TextInputSpec::new("group-role-name", TextInputKind::Line)
            },
        ));
        let title_row = spawn_labeled_row(commands, area, "group-role-title-label");
        ui.role_title_field = Some(spawn_text_input(
            commands,
            title_row,
            &TextInputSpec {
                initial: role.title.clone(),
                font_size: FONT_SIZE,
                width_glyphs: 16.0,
                tab_index: 0,
                max_characters: Some(63),
                ..TextInputSpec::new("group-role-title", TextInputKind::Line)
            },
        ));
        spawn_section_label(commands, area, "group-role-desc-label");
        ui.role_desc_field = Some(spawn_text_input(
            commands,
            area,
            &TextInputSpec {
                initial: role.description.clone(),
                font_size: FONT_SIZE,
                visible_lines: 2.0,
                tab_index: 0,
                max_characters: Some(255),
                ..TextInputSpec::new("group-role-desc", TextInputKind::Multiline)
            },
        ));
        let row = spawn_button_row(commands, area);
        spawn_action_button(
            commands,
            row,
            "group-role-save",
            GroupProfileAction::SaveRoleData,
            0,
        );
    } else {
        spawn_value_label(commands, name_row, role.name.clone(), LABEL_COLOR);
        if !role.title.is_empty() {
            let title_row = spawn_labeled_row(commands, area, "group-role-title-label");
            spawn_value_label(commands, title_row, role.title.clone(), LABEL_COLOR);
        }
        if !role.description.is_empty() {
            spawn_text_block(commands, area, role.description.clone());
        }
    }

    // Abilities checklist — driven by the powers draft when editable, else the
    // role's stored powers.
    spawn_section_label(commands, area, "group-role-abilities");
    let powers = if can_actions {
        state.role_power_draft
    } else {
        role.powers
    };
    for (bit, key) in ABILITIES {
        let on = has_power(powers, bit);
        let action = can_actions.then_some(GroupProfileAction::ToggleRolePower(bit));
        spawn_toggle_key_row(commands, area, key, on, action);
    }
    if can_actions {
        let row = spawn_button_row(commands, area);
        spawn_action_button(
            commands,
            row,
            "group-role-save-powers",
            GroupProfileAction::SaveRolePowers,
            0,
        );
    }
    if can_delete {
        let row = spawn_button_row(commands, area);
        spawn_action_button(
            commands,
            row,
            "group-role-delete",
            GroupProfileAction::DeleteRole,
            0,
        );
    }
}

// ---------------------------------------------------------------------------
// Rebuild: notice compose + body.
// ---------------------------------------------------------------------------

/// Rebuild the notice compose area when dirty: a subject + message editor and a
/// Send button, shown only to members who may send notices.
fn rebuild_compose_area(
    mut dirty: ResMut<GroupProfileDirty>,
    mut ui: ResMut<GroupProfileUi>,
    state: Res<GroupProfileState>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    if !dirty.compose {
        return;
    }
    dirty.compose = false;
    // Built once when the send power is known; the reply burst never respawns it.
    let can_send = state.has_power(group_powers::NOTICES_SEND);
    if ui.compose_can_send == Some(can_send) {
        return;
    }
    ui.compose_can_send = Some(can_send);
    let area = ui.compose_area;
    despawn_children(&children, &mut commands, area);
    ui.notice_subject_field = None;
    ui.notice_message_field = None;
    if !can_send {
        return;
    }
    spawn_section_label(&mut commands, area, "group-notice-compose");
    let subject_row = spawn_labeled_row(&mut commands, area, "group-notice-subject");
    ui.notice_subject_field = Some(spawn_text_input(
        &mut commands,
        subject_row,
        &TextInputSpec {
            font_size: FONT_SIZE,
            width_glyphs: 24.0,
            tab_index: 0,
            max_characters: Some(63),
            ..TextInputSpec::new("group-notice-subject", TextInputKind::Line)
        },
    ));
    ui.notice_message_field = Some(spawn_text_input(
        &mut commands,
        area,
        &TextInputSpec {
            font_size: FONT_SIZE,
            visible_lines: 3.0,
            tab_index: 0,
            max_characters: Some(511),
            ..TextInputSpec::new("group-notice-message", TextInputKind::Multiline)
        },
    ));
    let row = spawn_button_row(&mut commands, area);
    spawn_action_button(
        &mut commands,
        row,
        "group-notice-send",
        GroupProfileAction::SendNotice,
        0,
    );
}

/// Rebuild the notice body view when dirty: the selected notice's subject and full
/// body (or a loading / hint line).
fn rebuild_notice_body(
    mut dirty: ResMut<GroupProfileDirty>,
    mut ui: ResMut<GroupProfileUi>,
    state: Res<GroupProfileState>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    if !dirty.notice_body {
        return;
    }
    dirty.notice_body = false;
    // Built once for the hint, then rebuilt only when the user-paced selection or
    // its (later-arriving) body changes — never in the reply burst.
    let body_present = state
        .selected_notice
        .and_then(|index| state.notices.get(index))
        .is_some_and(|notice| state.notice_bodies.contains_key(&notice.notice_id));
    let sig = (state.selected_notice, body_present);
    if ui.notice_body_built == Some(sig) {
        return;
    }
    ui.notice_body_built = Some(sig);
    let area = ui.notice_body_area;
    despawn_children(&children, &mut commands, area);
    let Some(index) = state.selected_notice else {
        spawn_key_label(&mut commands, area, "group-notice-hint", DIM_LABEL_COLOR);
        return;
    };
    let Some(notice) = state.notices.get(index) else {
        return;
    };
    let subject_row = spawn_labeled_row(&mut commands, area, "group-notice-subject");
    spawn_value_label(
        &mut commands,
        subject_row,
        notice.subject.clone(),
        LABEL_COLOR,
    );
    match state.notice_bodies.get(&notice.notice_id) {
        Some(body) => spawn_text_block(&mut commands, area, body.clone()),
        None => spawn_key_label(
            &mut commands,
            area,
            "group-profile-loading",
            DIM_LABEL_COLOR,
        ),
    }
    if notice.has_attachment {
        spawn_key_label(
            &mut commands,
            area,
            "group-notice-has-attachment",
            DIM_LABEL_COLOR,
        );
    }
}

// ---------------------------------------------------------------------------
// Virtualized row pools.
// ---------------------------------------------------------------------------

/// The member a pooled row currently presents, or `None` when parked.
#[derive(Component, Debug, Clone, Copy)]
struct BoundMember(Option<AgentKey>);

/// Build each newly-pooled member row's cells once (widget-owned columns +
/// clip + locale ellipsis), attaching the selection state and press observer.
fn populate_member_rows(
    mut commands: Commands,
    ui: Option<Res<GroupProfileUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.members_viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.members_table, &MEMBERS_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundMember(None))
            .observe(on_member_row_press);
    }
}

/// Bind each pooled member row to the [`MemberRow`] it now points at.
fn bind_member_rows(
    view: Res<MembersView>,
    state: Res<GroupProfileState>,
    avatars: Res<AvatarState>,
    ui: Option<Res<GroupProfileUi>>,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundMember,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || state.is_changed() || avatars.is_changed();
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.members_viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let Some(index) = row.index else {
            continue;
        };
        let Some(member_row) = view.rows.get(index) else {
            continue;
        };
        bound.0 = Some(member_row.agent);
        let selected = state.focus == DetailsFocus::Member(member_row.agent);
        let name = name_of(member_row.agent, &avatars);
        set_row_cell(&mut texts, cells, 0, &name, member_row.is_owner);
        set_row_cell(&mut texts, cells, 1, &member_row.title, false);
        set_row_cell(&mut texts, cells, 2, &member_row.contribution, false);
        set_row_cell(&mut texts, cells, 3, &member_row.status, false);
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if selected {
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

/// Set the `column`-th cell of a pooled table row's value, accenting owners /
/// selection — resolves the cell entity from the row's [`TableRowCells`] and
/// writes it through the widget's [`set_table_cell`].
fn set_row_cell(
    texts: &mut Query<(&mut Text, &mut TextColor)>,
    cells: &TableRowCells,
    column: usize,
    value: &str,
    accent: bool,
) {
    if let Some(cell) = cells.cell(column) {
        let color = if accent { ACCENT_COLOR } else { LABEL_COLOR };
        set_table_cell(texts, cell, value, color);
    }
}

/// Select a member on press, showing its details.
fn on_member_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&BoundMember>,
    ui: Res<GroupProfileUi>,
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<GroupProfileState>,
    mut dirty: ResMut<GroupProfileDirty>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.members_viewport, FocusCause::Navigated);
    let Ok(bound) = rows.get(press.entity) else {
        return;
    };
    let Some(member) = bound.0 else {
        return;
    };
    state.focus = DetailsFocus::Member(member);
    dirty.details = true;
}

/// The notice index a pooled row currently presents, or `None` when parked.
#[derive(Component, Debug, Clone, Copy)]
struct BoundNotice(Option<usize>);

/// Build each newly-pooled notice row's cells once (widget-owned columns), and
/// attach the selection state and press observer.
fn populate_notice_rows(
    mut commands: Commands,
    ui: Option<Res<GroupProfileUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.notices_viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, ui.notices_table, &NOTICES_TABLE);
        commands
            .entity(row_entity)
            .insert(BoundNotice(None))
            .observe(on_notice_row_press);
    }
}

/// Bind each pooled notice row to the [`NoticeRow`] it now points at.
fn bind_notice_rows(
    view: Res<NoticesView>,
    state: Res<GroupProfileState>,
    ui: Option<Res<GroupProfileUi>>,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &TableRowCells,
        &mut BoundNotice,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || state.is_changed();
    for (row_entity, row, child_of, cells, mut bound) in &mut rows {
        if child_of.parent() != ui.notices_viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let Some(row_index) = row.index else {
            continue;
        };
        let Some(notice_row) = view.rows.get(row_index) else {
            continue;
        };
        bound.0 = Some(notice_row.index);
        let subject = if notice_row.has_attachment {
            format!("\u{1F4CE} {}", notice_row.subject)
        } else {
            notice_row.subject.clone()
        };
        set_row_cell(&mut texts, cells, 0, &subject, false);
        set_row_cell(&mut texts, cells, 1, &notice_row.from_name, false);
        set_row_cell(&mut texts, cells, 2, &notice_row.date, false);
        let selected = state.selected_notice == Some(notice_row.index);
        if let Ok(mut background) = backgrounds.get_mut(row_entity) {
            let wanted = if selected {
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

/// Select a notice on press: show it, and request its full body if not cached.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected queries / resources: the pick, the row \
              query, the UI handles, the focus, the state + dirty flags, the requested-notice \
              set it records into, and the command writer it fetches through"
)]
fn on_notice_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&BoundNotice>,
    ui: Res<GroupProfileUi>,
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<GroupProfileState>,
    mut dirty: ResMut<GroupProfileDirty>,
    mut requested: ResMut<RequestedGroupNotices>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.notices_viewport, FocusCause::Navigated);
    let Ok(bound) = rows.get(press.entity) else {
        return;
    };
    let Some(index) = bound.0 else {
        return;
    };
    state.selected_notice = Some(index);
    if let Some(notice) = state.notices.get(index) {
        let notice_id = notice.notice_id;
        if !state.notice_bodies.contains_key(&notice_id) {
            state.pending_notice = Some(notice_id);
            // Suppress the toast for a notice we're pulling up to read here.
            requested.mark_requested(notice_id);
            sl_commands.write(SlCommand(Command::RequestGroupNotice(notice_id)));
        }
    }
    dirty.notice_body = true;
}

// ---------------------------------------------------------------------------
// Actions.
// ---------------------------------------------------------------------------

/// Dispatch a clicked group-profile button to the behaviour behind it.
#[expect(
    clippy::too_many_lines,
    reason = "one dispatch over every group-profile button kind, each arm a few lines"
)]
fn on_group_profile_action(
    press: On<Pointer<Press>>,
    actions: Query<&GroupProfileAction>,
    mut state: ResMut<GroupProfileState>,
    mut dirty: ResMut<GroupProfileDirty>,
    ui: Res<GroupProfileUi>,
    fields: Query<&EditableText>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    let Some(target) = state.target else {
        return;
    };
    let read = |entity: Option<Entity>| {
        entity
            .and_then(|field| fields.get(field).ok())
            .map(|field| field.value().to_string())
    };
    match *action {
        GroupProfileAction::Join => {
            sl_commands.write(SlCommand(Command::JoinGroup(target)));
        }
        GroupProfileAction::SaveGeneral => {
            let Some(profile) = state.profile.as_ref() else {
                return;
            };
            let fee = read(ui.fee_field)
                .and_then(|value| value.trim().parse::<u64>().ok())
                .unwrap_or(profile.membership_fee.0);
            let charter = read(ui.charter_field).unwrap_or_else(|| profile.charter.clone());
            sl_commands.write(SlCommand(Command::UpdateGroupInfo(UpdateGroupInfoParams {
                group_id: target,
                charter,
                show_in_list: state.general_draft.show_in_list,
                insignia_id: profile.insignia_id,
                membership_fee: LindenAmount(fee),
                open_enrollment: state.general_draft.open_enrollment,
                allow_publish: profile.allow_publish,
                mature_publish: state.general_draft.mature,
            })));
            sl_commands.write(SlCommand(Command::RequestGroupProfile(target)));
        }
        GroupProfileAction::ToggleAcceptNotices => {
            state.accept_notices = !state.accept_notices;
            send_accept_notices(&state, target, &mut sl_commands);
            dirty.general_values = true;
        }
        GroupProfileAction::ToggleListInProfile => {
            state.list_in_profile = !state.list_in_profile;
            send_accept_notices(&state, target, &mut sl_commands);
            dirty.general_values = true;
        }
        GroupProfileAction::ToggleOpenEnrollment => {
            state.general_draft.open_enrollment = !state.general_draft.open_enrollment;
            dirty.general_values = true;
        }
        GroupProfileAction::ToggleMature => {
            state.general_draft.mature = !state.general_draft.mature;
            dirty.general_values = true;
        }
        GroupProfileAction::ToggleShowInList => {
            state.general_draft.show_in_list = !state.general_draft.show_in_list;
            dirty.general_values = true;
        }
        GroupProfileAction::CycleTitle => {
            if state.titles.is_empty() {
                return;
            }
            state.title_index = next_cycle_index(state.title_index, state.titles.len());
            if let Some(title) = state.titles.get(state.title_index) {
                sl_commands.write(SlCommand(Command::UpdateGroupTitle {
                    group_id: target,
                    title_role_id: role_or_everyone(title.role_id),
                }));
            }
            dirty.general_values = true;
        }
        GroupProfileAction::RefreshMembers => {
            sl_commands.write(SlCommand(Command::FetchGroupMembers(target)));
            sl_commands.write(SlCommand(Command::RequestGroupMembers(target)));
        }
        GroupProfileAction::CloseDetails => {
            state.focus = DetailsFocus::None;
            dirty.details = true;
        }
        GroupProfileAction::EjectMember => {
            if let DetailsFocus::Member(member) = state.focus {
                sl_commands.write(SlCommand(Command::EjectGroupMembers {
                    group_id: target,
                    member_ids: vec![member],
                }));
                sl_commands.write(SlCommand(Command::FetchGroupMembers(target)));
            }
        }
        GroupProfileAction::ToggleMemberRole(role_id) => {
            let DetailsFocus::Member(member) = state.focus else {
                return;
            };
            let currently = state.member_roles(member).contains(&role_id);
            let change = if currently {
                GroupRoleChange::Remove
            } else {
                GroupRoleChange::Add
            };
            sl_commands.write(SlCommand(Command::ChangeGroupRoleMembers {
                group_id: target,
                changes: vec![GroupRoleMemberChange {
                    role_id,
                    member_id: member,
                    change,
                }],
            }));
            // Reflect locally, then re-request to confirm.
            if currently {
                state
                    .role_members
                    .retain(|pair| !(pair.role_id == role_id && pair.member_id == member));
            } else {
                state.role_members.push(GroupRoleMember {
                    role_id,
                    member_id: member,
                });
            }
            sl_commands.write(SlCommand(Command::RequestGroupRoleMembers(target)));
            dirty.details = true;
        }
        GroupProfileAction::NewRole => {
            sl_commands.write(SlCommand(Command::UpdateGroupRoles {
                group_id: target,
                roles: vec![GroupRoleEdit {
                    role_id: Some(GroupRoleKey::from(Uuid::new_v4())),
                    name: "New Role".to_owned(),
                    description: String::new(),
                    title: "New Member Title".to_owned(),
                    powers: group_powers::NONE,
                    update_type: GroupRoleUpdateType::Create,
                }],
            }));
            sl_commands.write(SlCommand(Command::RequestGroupRoles(target)));
        }
        GroupProfileAction::DeleteRole => {
            if let DetailsFocus::Role(Some(role_id)) = state.focus {
                sl_commands.write(SlCommand(Command::UpdateGroupRoles {
                    group_id: target,
                    roles: vec![GroupRoleEdit {
                        role_id: Some(role_id),
                        name: String::new(),
                        description: String::new(),
                        title: String::new(),
                        powers: group_powers::NONE,
                        update_type: GroupRoleUpdateType::Delete,
                    }],
                }));
                state.focus = DetailsFocus::None;
                sl_commands.write(SlCommand(Command::RequestGroupRoles(target)));
                dirty.details = true;
            }
        }
        GroupProfileAction::SaveRoleData => {
            let DetailsFocus::Role(Some(role_id)) = state.focus else {
                return;
            };
            let Some(role) = state
                .roles
                .iter()
                .find(|role| role.role_id == Some(role_id))
            else {
                return;
            };
            let name = read(ui.role_name_field).unwrap_or_else(|| role.name.clone());
            let title = read(ui.role_title_field).unwrap_or_else(|| role.title.clone());
            let description = read(ui.role_desc_field).unwrap_or_else(|| role.description.clone());
            sl_commands.write(SlCommand(Command::UpdateGroupRoles {
                group_id: target,
                roles: vec![GroupRoleEdit {
                    role_id: Some(role_id),
                    name,
                    description,
                    title,
                    powers: role.powers,
                    update_type: GroupRoleUpdateType::UpdateData,
                }],
            }));
            sl_commands.write(SlCommand(Command::RequestGroupRoles(target)));
        }
        GroupProfileAction::ToggleRolePower(bit) => {
            state.role_power_draft ^= bit;
            dirty.details = true;
        }
        GroupProfileAction::SaveRolePowers => {
            let DetailsFocus::Role(Some(role_id)) = state.focus else {
                return;
            };
            let Some(role) = state
                .roles
                .iter()
                .find(|role| role.role_id == Some(role_id))
            else {
                return;
            };
            sl_commands.write(SlCommand(Command::UpdateGroupRoles {
                group_id: target,
                roles: vec![GroupRoleEdit {
                    role_id: Some(role_id),
                    name: role.name.clone(),
                    description: role.description.clone(),
                    title: role.title.clone(),
                    powers: state.role_power_draft,
                    update_type: GroupRoleUpdateType::UpdatePowers,
                }],
            }));
            sl_commands.write(SlCommand(Command::RequestGroupRoles(target)));
        }
        GroupProfileAction::SendNotice => {
            let subject = read(ui.notice_subject_field).unwrap_or_default();
            let message = read(ui.notice_message_field).unwrap_or_default();
            if subject.trim().is_empty() {
                return;
            }
            sl_commands.write(SlCommand(Command::SendGroupNotice {
                group_id: target,
                subject,
                message,
                attachment: None,
            }));
            // Re-request the notice list so the new notice appears.
            sl_commands.write(SlCommand(Command::RequestGroupNotices(target)));
            dirty.compose = true;
        }
    }
}

/// Send the current membership notice preferences (`SetGroupAcceptNotices` carries
/// both flags together).
fn send_accept_notices(
    state: &GroupProfileState,
    target: GroupKey,
    sl_commands: &mut MessageWriter<SlCommand>,
) {
    sl_commands.write(SlCommand(Command::SetGroupAcceptNotices {
        group_id: target,
        accept_notices: state.accept_notices,
        list_in_profile: state.list_in_profile,
    }));
}

// ---------------------------------------------------------------------------
// Insignia texture polling.
// ---------------------------------------------------------------------------

/// Swap the group insignia into its box once the pipeline decodes it.
fn poll_group_profile_texture(
    mut state: ResMut<GroupProfileState>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let Some((key, node)) = state.pending_texture else {
        return;
    };
    let Ok(mut entity) = commands.get_entity(node) else {
        state.pending_texture = None;
        return;
    };
    if let Some(decoded) = manager.decoded(key) {
        let handle = images.add(to_bevy_image(decoded));
        entity.insert(ImageNode::new(handle));
        // Drop the "(loading)" label under the image.
        despawn_children(&children, &mut commands, node);
        state.pending_texture = None;
    }
}

// ---------------------------------------------------------------------------
// Small spawn helpers.
// ---------------------------------------------------------------------------

/// The group insignia: request the texture and show a placeholder until it
/// decodes.
fn spawn_insignia(
    commands: &mut Commands,
    parent: Entity,
    insignia_id: Option<TextureKey>,
    state: &mut GroupProfileState,
    textures: &mut TextureManager,
) {
    let node = commands
        .spawn((
            Node {
                width: Val::Px(INSIGNIA_EDGE),
                height: Val::Px(INSIGNIA_EDGE),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            ChildOf(parent),
        ))
        .id();
    let key = insignia_id.filter(|key| *key != TextureKey::from(Uuid::nil()));
    let Some(key) = key else {
        spawn_key_label(commands, node, "group-profile-no-insignia", DIM_LABEL_COLOR);
        return;
    };
    spawn_key_label(commands, node, "group-profile-loading", DIM_LABEL_COLOR);
    textures.request_boosted(key, AVATAR_BOOST_PRIORITY);
    state.pending_texture = Some((key, node));
}

/// A labelled row: the translated label leading, the caller's content after.
fn spawn_labeled_row(commands: &mut Commands, parent: Entity, label_key: &'static str) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    row_entity
}

/// A translated section label on its own line.
fn spawn_section_label(commands: &mut Commands, parent: Entity, label_key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// A plain value label.
fn spawn_value_label(commands: &mut Commands, parent: Entity, value: String, color: Color) {
    commands.spawn((
        Text::new(value),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(color),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// A translated label.
fn spawn_key_label(commands: &mut Commands, parent: Entity, key: &'static str, color: Color) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(color),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// A wrapped read-only text block (charter, descriptions, notice bodies).
fn spawn_text_block(commands: &mut Commands, parent: Entity, text: String) {
    commands
        .spawn((
            Node {
                max_height: Val::Px(160.0),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            ChildOf(parent),
        ))
        .with_child((
            Text::new(text),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ));
}

/// A wrapping row for action buttons.
fn spawn_button_row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id()
}

/// A bordered translated button dispatching `action` via
/// [`on_group_profile_action`].
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: GroupProfileAction,
    tab_index: i32,
) {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            action,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("group-profile-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_group_profile_action)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// A borderless cycle button, returning the entity the caller labels.
fn spawn_cycle_button(
    commands: &mut Commands,
    parent: Entity,
    action: GroupProfileAction,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            action,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            ChildOf(parent),
        ))
        .observe(on_group_profile_action)
        .id()
}

/// A flag row: a translated label leading a clickable check-glyph toggle (or a
/// read-only glyph when `action` is `None`). Returns the glyph text node so its
/// checked state can be updated in place.
fn spawn_flag_row(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    on: bool,
    action: Option<GroupProfileAction>,
) -> Entity {
    let row_entity = spawn_labeled_row(commands, parent, label_key);
    spawn_toggle_glyph(commands, row_entity, on, action)
}

/// A toggle row: a clickable (or read-only) check-glyph leading a plain-text
/// label. Returns the glyph text node.
fn spawn_toggle_row(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    on: bool,
    action: Option<GroupProfileAction>,
) -> Entity {
    let (host, glyph) = spawn_toggle_host(commands, parent, on, action);
    spawn_value_label(commands, host, label.to_owned(), LABEL_COLOR);
    glyph
}

/// A toggle row whose label is a translated Fluent key (the abilities checklist).
/// Returns the glyph text node.
fn spawn_toggle_key_row(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    on: bool,
    action: Option<GroupProfileAction>,
) -> Entity {
    let (host, glyph) = spawn_toggle_host(commands, parent, on, action);
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(host),
    ));
    glyph
}

/// The shared toggle-row host: a centred row carrying the check glyph, returning
/// the host entity for the caller to append its label to, and the glyph text node.
fn spawn_toggle_host(
    commands: &mut Commands,
    parent: Entity,
    on: bool,
    action: Option<GroupProfileAction>,
) -> (Entity, Entity) {
    let host = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(4.0))
            },
            ChildOf(parent),
        ))
        .id();
    let glyph = spawn_toggle_glyph(commands, host, on, action);
    (host, glyph)
}

/// The check-glyph itself: a [`Button`] carrying `action` when interactive, or a
/// plain non-picking glyph when read-only. Returns the glyph text node so a
/// caller can update its checked/unchecked state in place ([`set_toggle_glyph`]).
fn spawn_toggle_glyph(
    commands: &mut Commands,
    parent: Entity,
    on: bool,
    action: Option<GroupProfileAction>,
) -> Entity {
    let glyph = if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH };
    let color = if on { CHECK_COLOR } else { DIM_LABEL_COLOR };
    let host = match action {
        Some(action) => commands
            .spawn((
                Button,
                action,
                Node {
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::default(),
                ChildOf(parent),
            ))
            .observe(on_group_profile_action)
            .id(),
        None => parent,
    };
    commands
        .spawn((
            Text::new(glyph),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(color),
            Pickable::IGNORE,
            ChildOf(host),
        ))
        .id()
}

/// Set a toggle glyph text node to its checked/unchecked state in place (no
/// respawn), for the retained value-update path.
fn set_toggle_glyph(texts: &mut Query<(&mut Text, &mut TextColor)>, glyph: Entity, on: bool) {
    if let Ok((mut text, mut color)) = texts.get_mut(glyph) {
        let wanted = if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH };
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
        let wanted_color = TextColor(if on { CHECK_COLOR } else { DIM_LABEL_COLOR });
        if *color != wanted_color {
            *color = wanted_color;
        }
    }
}

/// Spawn an empty value label under `parent`, returning it so a value-update path
/// can set its text in place ([`set_value_node`]).
fn spawn_value_node(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// Set a retained value node's text in place (only on change).
fn set_value_node(
    texts: &mut Query<(&mut Text, &mut TextColor)>,
    node: Option<Entity>,
    value: &str,
) {
    if let Some(node) = node
        && let Ok((mut text, _)) = texts.get_mut(node)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// Despawn every child of `parent`.
fn despawn_children(children: &Query<&Children>, commands: &mut Commands, parent: Entity) {
    if let Ok(existing) = children.get(parent) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
}

/// The display name for an agent, falling back to its id in parentheses.
fn name_of(agent: AgentKey, avatars: &AvatarState) -> String {
    avatars
        .name_of(agent)
        .map_or_else(|| format!("({agent})"), str::to_owned)
}

// ---------------------------------------------------------------------------
// Pure helpers.
// ---------------------------------------------------------------------------

/// Whether the `powers` bitfield holds `power` (nonzero intersection).
const fn has_power(powers: u64, power: u64) -> bool {
    powers & power != 0
}

/// Split a group-notice IM `message` into `(subject, body)` on the first `|`, the
/// wire join the simulator sends. A message with no `|` is all subject, empty body.
fn split_notice(message: &str) -> (&str, &str) {
    match message.split_once('|') {
        Some((subject, body)) => (subject, body),
        None => (message, ""),
    }
}

/// The next index when cycling through `len` items (wrapping), avoiding the
/// arithmetic-overflow lint on the increment / modulo.
const fn next_cycle_index(current: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    match current.checked_add(1) {
        Some(next) if next < len => next,
        _ => 0,
    }
}

/// The role id to wear a title from: an explicit role, or the nil "Everyone" role
/// (the wire wants a concrete [`GroupRoleKey`], and "Everyone" is `None`).
fn role_or_everyone(role_id: Option<GroupRoleKey>) -> GroupRoleKey {
    role_id.unwrap_or_else(|| GroupRoleKey::from(Uuid::nil()))
}

#[cfg(test)]
mod tests {
    use super::{
        GroupMember, MemberRoster, has_power, next_cycle_index, role_or_everyone, split_notice,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, GroupRoleKey, LandArea, Uuid, group_powers};

    /// A test member with the given agent id.
    fn member(id: u128) -> GroupMember {
        GroupMember {
            agent_id: AgentKey::from(Uuid::from_u128(id)),
            contribution: LandArea::ZERO,
            online_status: "Online".to_owned(),
            agent_powers: 0,
            title: "Member".to_owned(),
            is_owner: false,
        }
    }

    /// The roster accumulates across replies, deduplicating by agent id and keeping
    /// the largest reported total.
    #[test]
    fn roster_accumulates_and_dedups() {
        let mut roster = MemberRoster::default();
        roster.apply(3, &[member(1), member(2)]);
        assert_eq!(roster.loaded(), 2);
        assert_eq!(roster.total, 3);
        // A second (partial) reply adds a new one and ignores the duplicate.
        roster.apply(3, &[member(2), member(3)]);
        assert_eq!(roster.loaded(), 3);
        // A smaller reported total does not shrink the kept total.
        roster.apply(1, &[]);
        assert_eq!(roster.total, 3);
    }

    /// Power gating is a plain bitfield intersection.
    #[test]
    fn power_gating_is_bit_intersection() {
        let powers = group_powers::MEMBER_EJECT | group_powers::NOTICES_SEND;
        assert!(has_power(powers, group_powers::MEMBER_EJECT));
        assert!(has_power(powers, group_powers::NOTICES_SEND));
        assert!(!has_power(powers, group_powers::ROLE_DELETE));
        assert!(!has_power(group_powers::NONE, group_powers::MEMBER_EJECT));
    }

    /// A group notice's `subject|body` wire join splits on the first pipe.
    #[test]
    fn notice_splits_on_first_pipe() {
        assert_eq!(split_notice("Subject|Body text"), ("Subject", "Body text"));
        // A body containing pipes keeps them.
        assert_eq!(split_notice("S|a|b"), ("S", "a|b"));
        // No pipe → all subject, empty body.
        assert_eq!(split_notice("just a subject"), ("just a subject", ""));
    }

    /// The title cycle wraps and is a no-op on an empty list.
    #[test]
    fn title_cycle_wraps() {
        assert_eq!(next_cycle_index(0, 3), 1);
        assert_eq!(next_cycle_index(2, 3), 0);
        assert_eq!(next_cycle_index(0, 0), 0);
        assert_eq!(next_cycle_index(5, 3), 0);
    }

    /// The "Everyone" role (id `None`) maps to the nil role id for wearing a title.
    #[test]
    fn everyone_role_is_nil() {
        assert_eq!(role_or_everyone(None), GroupRoleKey::from(Uuid::nil()));
        let role = GroupRoleKey::from(Uuid::from_u128(7));
        assert_eq!(role_or_everyone(Some(role)), role);
    }
}
