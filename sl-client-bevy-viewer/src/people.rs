//! The **People / Contacts surface** (`viewer-social-people-panel`), hosted as a
//! single pinned tab inside the [Conversations floater](crate::conversations).
//!
//! # Why it lives in the Conversations floater
//!
//! The reference viewer's default skin keeps People (`floater_people`) and
//! Conversations (`llfloaterimcontainer`) as two windows, but the **Vintage**
//! skin folds them into one: its Conversations floater is a `multi_floater` with a
//! left-positioned tab strip, and the "Contacts" floater is *hosted* as one left
//! tab whose content is a horizontal `tab_container` (Friends / Groups / Contact
//! Sets). This module reproduces that arrangement — a pinned **People** tab in the
//! conversations vertical strip whose pane carries a horizontal sub-tab strip.
//!
//! # Scope of this task
//!
//! Only the **Friends** list is wired here; the **Groups** sub-tab's content slot
//! is spawned here but filled by [`crate::groups`] (the `viewer-social-groups`
//! task). The nearby-avatars-with-distances list is the separate **radar**
//! (`viewer-avatar-radar`), and the reference's Recent / Blocked tabs are not
//! built in this task.
//!
//! # Model + ECS mirror
//!
//! [`FriendsModel`] is a plain, unit-tested resource fed **only** from the
//! [`SlEvent`] stream — the buddy list ([`SlSessionEvent::FriendList`] /
//! [`SlSessionEvent::FriendsSnapshot`]), presence
//! ([`SlSessionEvent::FriendsOnline`] / [`SlSessionEvent::FriendsOffline`]),
//! rights changes, termination, and name replies — mirroring
//! [`crate::conversations`]'s pure model. [`FriendsView`] is the ordered,
//! render-ready projection the virtualized list ([`crate::virtual_list`]) binds
//! its recycled rows to.
//!
//! # Sharing the strip with conversations
//!
//! The People tab and pane are added into the conversations floater's own strip /
//! panel area (via [`crate::conversations::ConversationsUi`]); which of the two
//! surfaces is front is arbitrated by [`crate::conversations::StripFocus`] so
//! exactly one pane ever shows. Selecting the People tab takes the strip; the
//! Friends "IM" action hands it back by opening a one-to-one conversation
//! ([`crate::conversations::OpenConversation`]).
//!
//! Reference (Firestorm, read-only): `llpanelpeople`, `llavatarlist`,
//! Vintage `floater_fs_contacts` / `panel_fs_contacts_friends`.
//!
//! [`viewer-social-groups`]: the separate ready roadmap task for the group list.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use bevy::asset::RenderAssetUsages;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_client_bevy::{
    AgentKey, Command, Friend, FriendKey, FriendPresence, FriendRights, MuteFlags, MuteType,
    SlCommand, SlEvent, SlSessionEvent, Uuid,
};

use sl_settings::SettingValue;

use crate::conversations::{ConversationKey, ConversationsUi, OpenConversation, StripFocus};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::settings::{ViewerSettings, load_account_settings};
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};
use crate::virtual_list::{VirtualList, VirtualRow, VirtualViewport, layout_virtual_lists};

/// A friend-list row's uniform height, in logical pixels — matched to the
/// conversation-transcript density so the whole floater reads as one surface.
const ROW_HEIGHT: f32 = 22.0;

/// The chrome / label font size, in logical pixels (tabs, buttons).
const CHROME_FONT_SIZE: f32 = 13.0;

/// A friend row's font size, in logical pixels.
const ROW_FONT_SIZE: f32 = 13.0;

/// The width of the trailing status column (the presence dot), in logical
/// pixels — wide enough to sit its "Status" header above it.
const STATUS_COL_WIDTH: f32 = 56.0;

/// The width of the trailing action-button column, in logical pixels — enough for
/// the longest label ("Offer Teleport") at the chrome font size.
const ACTION_COL_WIDTH: f32 = 128.0;

/// The width of one permission column (a single rights icon), in logical pixels.
const RIGHT_COL_WIDTH: f32 = 22.0;

/// The on-screen size of a generated icon (header icon / row checkbox), in logical
/// pixels.
const ICON_DISPLAY: f32 = 15.0;

/// An inactive tab's background — recessed, matching [`crate::conversations`]'s
/// tab palette so the People tab is visually one of the strip's tabs.
const TAB_INACTIVE_BACKGROUND: Color = Color::srgb(0.11, 0.13, 0.17);

/// The active tab's background — the panel shade, so the selected tab merges into
/// its pane.
const TAB_ACTIVE_BACKGROUND: Color = Color::srgb(0.19, 0.23, 0.31);

/// An inactive tab's border.
const TAB_BORDER: Color = Color::srgb(0.28, 0.33, 0.42);

/// The active tab's border — the bright "this one is selected" accent.
const TAB_ACTIVE_BORDER: Color = Color::srgb(0.52, 0.68, 0.95);

/// A tab / label's text colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The friends-list scroll surface background — a touch darker, a sunken well.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The background of the currently-selected friend row.
const SELECTED_ROW_BACKGROUND: Color = Color::srgba(0.30, 0.42, 0.62, 0.55);

/// An online friend's presence dot colour — a friendly green.
const ONLINE_COLOR: Color = Color::srgb(0.40, 0.80, 0.42);

/// An offline (or not-visible) friend's presence dot colour — dim grey.
const OFFLINE_COLOR: Color = Color::srgb(0.42, 0.46, 0.52);

/// An action button's background.
const ACTION_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The table header row's background — a recessed strip above the list.
const HEADER_BACKGROUND: Color = Color::srgb(0.14, 0.17, 0.22);

/// The table header text colour — dim, so the headers read as chrome.
const HEADER_TEXT_COLOR: Color = Color::srgb(0.66, 0.70, 0.78);

/// A right this agent **grants** (an editable "They can …" checkbox), when ticked
/// — a clear, interactive accent.
const RIGHT_SET_COLOR: Color = Color::srgb(0.55, 0.75, 0.95);

/// A right the friend **grants** us (a read-only "You can …" checkbox), when
/// ticked — the same hue, dimmed, so it reads as informational not interactive.
const RIGHT_RECEIVED_COLOR: Color = Color::srgb(0.42, 0.55, 0.70);

/// A withheld permission (an empty checkbox), either direction — dim.
const RIGHT_UNSET_COLOR: Color = Color::srgb(0.40, 0.44, 0.50);

/// The tint for a rights checkbox, by whether it is set and which direction (an
/// editable granted right ticks brighter than a read-only received one).
const fn right_tint(set: bool, received: bool) -> Color {
    if !set {
        RIGHT_UNSET_COLOR
    } else if received {
        RIGHT_RECEIVED_COLOR
    } else {
        RIGHT_SET_COLOR
    }
}

/// The filled presence dot glyph, shown for an online friend.
const ONLINE_GLYPH: &str = "\u{25CF}";

/// The hollow presence dot glyph, shown for an offline / not-visible friend.
const OFFLINE_GLYPH: &str = "\u{25CB}";

/// The Fluent key for the People strip tab's label.
const PEOPLE_TAB_KEY: &str = "people-tab";

/// The Fluent key for the Friends sub-tab's label.
const FRIENDS_TAB_KEY: &str = "people-friends-tab";

/// The Fluent key for the Groups sub-tab's label.
const GROUPS_TAB_KEY: &str = "people-groups-tab";

/// The Fluent key for the friends-table "Name" column header.
const HEADER_NAME_KEY: &str = "people-header-name";

/// The Fluent key for the friends-table "Status" column header.
const HEADER_STATUS_KEY: &str = "people-header-status";

/// The Fluent key for the "They can …" permission-group header (rights this agent
/// grants the friend).
const GROUP_THEY_KEY: &str = "people-rights-they";

/// The Fluent key for the "You can …" permission-group header (rights the friend
/// grants this agent).
const GROUP_YOU_KEY: &str = "people-rights-you";

/// The Fluent key for the "IM" action button.
const ACTION_IM_KEY: &str = "people-action-im";

/// The Fluent key for the "Offer Teleport" action button.
const ACTION_TELEPORT_KEY: &str = "people-action-teleport";

/// The Fluent key for the "Remove Friend" action button.
const ACTION_REMOVE_KEY: &str = "people-action-remove";

/// The Fluent key for the "Block" action button.
const ACTION_BLOCK_KEY: &str = "people-action-block";

/// The Fluent key for the grant-edit-objects confirmation prompt (arg `name`).
const GRANT_CONFIRM_PROMPT_KEY: &str = "people-grant-confirm-prompt";

/// The Fluent key for the confirm dialog's Grant button.
const GRANT_CONFIRM_YES_KEY: &str = "people-grant-confirm-yes";

/// The Fluent key for the confirm dialog's Cancel button.
const GRANT_CONFIRM_NO_KEY: &str = "people-grant-confirm-no";

/// The modal dim behind the confirm dialog.
const CONFIRM_SCRIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

/// The confirm dialog box's background.
const CONFIRM_BOX_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// The confirm dialog box's border — a warning accent, since it gates a dangerous
/// grant.
const CONFIRM_BOX_BORDER: Color = Color::srgb(0.62, 0.44, 0.20);

/// The confirm dialog's Grant button background (a muted, cautionary amber).
const CONFIRM_GRANT_BACKGROUND: Color = Color::srgb(0.52, 0.38, 0.16);

/// The confirm dialog's Cancel button background.
const CONFIRM_CANCEL_BACKGROUND: Color = Color::srgb(0.24, 0.29, 0.38);

/// The z-order of the confirm modal — far above the floaters' monotonically-
/// climbing bring-to-front counter (which starts at 1), so it is never occluded.
const CONFIRM_Z: i32 = 1_000_000;

/// The sub-tab strip element id (its selection is not persisted per host — a
/// light strip, distinct from the divider the floater persists).
const SUB_STRIP_ELEMENT: &str = "people-sub-tabs";

/// The Friends sub-tab's index in the sub-strip.
const FRIENDS_TAB_INDEX: usize = 0;

/// The persisted-setting name for the friends-table sort order.
const FRIENDS_SORT_SETTING: &str = "friends_sort";

/// The `[people]` section the friends-sort setting lives under in the account
/// settings file.
const PEOPLE_SETTINGS_SECTION: &[&str] = &["people"];

/// The sort-direction arrow shown on the primary sort column's header — ascending.
const SORT_ASCENDING_GLYPH: &str = "\u{25B2}";

/// The sort-direction arrow shown on the primary sort column's header —
/// descending.
const SORT_DESCENDING_GLYPH: &str = "\u{25BC}";

/// The longest gap between two clicks on the same row still counted as a
/// double-click, in seconds — a double-click opens a one-to-one IM, like the IM
/// button (the reference viewer's list double-click).
const DOUBLE_CLICK_SECS: f32 = 0.4;

// ---------------------------------------------------------------------------
// Pure model
// ---------------------------------------------------------------------------

/// One friend's cached state: the friendship rights in both directions and the
/// last-known presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FriendEntry {
    /// The rights this agent grants the friend.
    rights_granted: FriendRights,
    /// The rights the friend grants this agent.
    rights_received: FriendRights,
    /// Whether the friend is currently known-online (`false` is "offline or not
    /// visible", never provably offline).
    online: bool,
}

impl FriendEntry {
    /// A fresh entry from a login / snapshot [`Friend`] record, offline until a
    /// presence notification says otherwise.
    const fn new(friend: Friend, online: bool) -> Self {
        Self {
            rights_granted: friend.rights_granted,
            rights_received: friend.rights_received,
            online,
        }
    }
}

/// The pure friends model: the buddy cache keyed by friend id, the resolved name
/// cache, and a revision stamp bumped on every change so the view rebuilds only
/// when something actually moved. Fed solely from the event stream.
#[derive(Resource, Debug, Default)]
pub(crate) struct FriendsModel {
    /// The buddy list, by friend id.
    friends: BTreeMap<FriendKey, FriendEntry>,
    /// Last-seen legacy display name per agent, for the row labels.
    names: BTreeMap<AgentKey, String>,
    /// The current multi-column sort order (persisted per avatar).
    sort: SortState,
    /// Bumped on each mutation; the view compares its last-built value to skip an
    /// unchanged rebuild.
    revision: u64,
}

impl FriendsModel {
    /// Bump the revision after a mutation.
    const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Merge a buddy-list record set (login `FriendList`), keeping any presence
    /// already learned for a friend that is being refreshed.
    fn note_friends(&mut self, friends: &[Friend]) {
        for friend in friends {
            let online = self
                .friends
                .get(&friend.id)
                .is_some_and(|entry| entry.online);
            self.friends
                .insert(friend.id, FriendEntry::new(*friend, online));
        }
        self.touch();
    }

    /// Replace the model from a presence snapshot (the [`Command::QueryFriends`]
    /// reply): authoritative for both rights and the online flag.
    fn apply_snapshot(&mut self, presence: &[FriendPresence]) {
        self.friends.clear();
        for entry in presence {
            self.friends.insert(
                entry.friend.id,
                FriendEntry::new(entry.friend, entry.online),
            );
        }
        self.touch();
    }

    /// Set the online flag on a set of friends (an online / offline notification).
    fn set_online(&mut self, friends: &[FriendKey], online: bool) {
        let mut changed = false;
        for id in friends {
            if let Some(entry) = self.friends.get_mut(id)
                && entry.online != online
            {
                entry.online = online;
                changed = true;
            }
        }
        if changed {
            self.touch();
        }
    }

    /// Update one friend's rights from a [`SlSessionEvent::FriendRightsChanged`]:
    /// `granted_to_us` distinguishes the rights the friend now grants us from a
    /// server echo of the rights we grant them.
    fn update_rights(&mut self, friend: FriendKey, rights: FriendRights, granted_to_us: bool) {
        if let Some(entry) = self.friends.get_mut(&friend) {
            if granted_to_us {
                entry.rights_received = rights;
            } else {
                entry.rights_granted = rights;
            }
            self.touch();
        }
    }

    /// Drop a friend (friendship terminated by either side).
    fn remove(&mut self, friend: FriendKey) {
        if self.friends.remove(&friend).is_some() {
            self.touch();
        }
    }

    /// Record a resolved legacy name for an agent (ignoring empties).
    fn note_name(&mut self, id: AgentKey, name: &str) {
        if !name.is_empty() && self.names.get(&id).map(String::as_str) != Some(name) {
            self.names.insert(id, name.to_owned());
            self.touch();
        }
    }

    /// The resolved name for an agent, if known.
    fn name_of(&self, id: AgentKey) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    /// Whether `agent` is already in the buddy cache — a friend.
    ///
    /// The avatar context menu reads this to disable "Add as Friend" for someone
    /// who already is one, matching the reference viewer's `on_enable`.
    pub(crate) fn is_friend(&self, agent: AgentKey) -> bool {
        self.friends.contains_key(&FriendKey::from(agent.uuid()))
    }

    /// The friends whose name is not yet resolved — the set to request names for.
    fn unnamed(&self) -> Vec<AgentKey> {
        self.friends
            .keys()
            .map(|id| AgentKey::from(*id))
            .filter(|agent| !self.names.contains_key(agent))
            .collect()
    }

    /// The ordered, render-ready row list: online friends first, then by
    /// case-folded name (an unresolved name sorts by its short-id placeholder).
    fn ordered(&self) -> Vec<FriendRow> {
        let mut rows: Vec<FriendRow> = self
            .friends
            .iter()
            .map(|(id, entry)| {
                let agent = AgentKey::from(*id);
                let name = self
                    .name_of(agent)
                    .map_or_else(|| short_id(agent.uuid()), ToOwned::to_owned);
                FriendRow {
                    friend: *id,
                    agent,
                    name,
                    online: entry.online,
                    rights_granted: entry.rights_granted,
                    rights_received: entry.rights_received,
                }
            })
            .collect();
        rows.sort_by(|left, right| self.sort.compare(left, right));
        rows
    }

    /// Apply a header click to the sort order (bumping the revision so the view
    /// re-sorts). Returns the encoded sort for persistence.
    fn sort_by(&mut self, column: SortColumn) -> String {
        self.sort.click(column);
        self.touch();
        self.sort.encode()
    }

    /// Replace the sort order (from a persisted value at login), re-sorting.
    fn set_sort(&mut self, sort: SortState) {
        self.sort = sort;
        self.touch();
    }

    /// The primary (most-significant) sort key, for the header arrow indicator.
    fn primary_sort(&self) -> Option<SortKey> {
        self.sort.primary()
    }

    /// The rights this agent currently grants `friend`, if known.
    fn granted_rights(&self, friend: FriendKey) -> Option<FriendRights> {
        self.friends.get(&friend).map(|entry| entry.rights_granted)
    }

    /// Optimistically set the rights this agent grants `friend` (so a toggled
    /// checkbox flips immediately; the server echo re-confirms the same value).
    fn set_granted(&mut self, friend: FriendKey, rights: FriendRights) {
        if let Some(entry) = self.friends.get_mut(&friend)
            && entry.rights_granted != rights
        {
            entry.rights_granted = rights;
            self.touch();
        }
    }
}

/// The rights bitfield with `kind`'s bit flipped.
const fn toggled_rights(rights: FriendRights, kind: RightKind) -> FriendRights {
    let bit = match kind {
        RightKind::SeeOnline => FriendRights::CAN_SEE_ONLINE,
        RightKind::Map => FriendRights::CAN_SEE_ON_MAP,
        RightKind::Edit => FriendRights::CAN_MODIFY_OBJECTS,
    };
    FriendRights(rights.0 ^ bit)
}

/// A short, readable stand-in for an unresolved agent id — its first eight hex
/// digits (mirrors [`crate::conversations`]'s placeholder).
fn short_id(id: Uuid) -> String {
    id.simple().to_string().chars().take(8).collect()
}

// ---------------------------------------------------------------------------
// Generated icons
// ---------------------------------------------------------------------------

/// The generated icon edge length, in texels — drawn once, displayed small and
/// tinted, so a crisp source keeps the down-scaled glyph clean.
const ICON_TEXELS: u32 = 32;

/// The procedurally-drawn, white-on-transparent table icons, generated once at
/// startup and tinted per use via [`ImageNode::color`] (the reference viewer ships
/// these as art assets; we draw them so the crate carries no binary art).
#[derive(Debug, Clone)]
struct PeopleIcons {
    /// The see-online column icon (an eye).
    online: Handle<Image>,
    /// The see-on-map column icon (a location pin).
    map: Handle<Image>,
    /// The edit-objects column icon (a pencil).
    edit: Handle<Image>,
    /// A ticked checkbox (a granted right).
    check_on: Handle<Image>,
    /// An empty checkbox (a withheld right).
    check_off: Handle<Image>,
}

impl PeopleIcons {
    /// Generate every icon and register it in the image assets.
    fn generate(images: &mut Assets<Image>) -> Self {
        Self {
            online: images.add(build_icon(&icon_eye)),
            map: images.add(build_icon(&icon_pin)),
            edit: images.add(build_icon(&icon_pencil)),
            check_on: images.add(build_icon(&icon_check_on)),
            check_off: images.add(build_icon(&icon_check_off)),
        }
    }

    /// The column-header icon for a right kind.
    fn kind_icon(&self, kind: RightKind) -> Handle<Image> {
        match kind {
            RightKind::SeeOnline => self.online.clone(),
            RightKind::Map => self.map.clone(),
            RightKind::Edit => self.edit.clone(),
        }
    }

    /// The checkbox icon for a set / withheld right.
    fn checkbox(&self, set: bool) -> Handle<Image> {
        if set {
            self.check_on.clone()
        } else {
            self.check_off.clone()
        }
    }
}

/// Rasterise a coverage function into a white-on-transparent RGBA icon.
///
/// `shape(nx, ny)` returns a signed coverage in normalised `[0, 1]` icon space
/// (positive inside the shape), which is anti-aliased to an alpha over roughly one
/// texel. The RGB stays white so [`ImageNode::color`] can tint the glyph.
fn build_icon(shape: &dyn Fn(f32, f32) -> f32) -> Image {
    let extent = f32::from(u16::try_from(ICON_TEXELS).unwrap_or(u16::MAX));
    let mut data: Vec<u8> = Vec::new();
    for y in 0..ICON_TEXELS {
        for x in 0..ICON_TEXELS {
            let nx = (texel_to_f32(x) + 0.5) / extent;
            let ny = (texel_to_f32(y) + 0.5) / extent;
            let coverage = (shape(nx, ny) * extent + 0.5).clamp(0.0, 1.0);
            data.extend_from_slice(&[255, 255, 255, alpha_byte(coverage)]);
        }
    }
    Image::new(
        Extent3d {
            width: ICON_TEXELS,
            height: ICON_TEXELS,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Widen a small texel coordinate to `f32` without an `as` cast.
fn texel_to_f32(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

/// Quantise a `0.0..=1.0` coverage to an alpha byte.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "coverage is clamped to 0.0..=1.0, so the scaled, rounded value is in 0..=255"
)]
fn alpha_byte(coverage: f32) -> u8 {
    (coverage.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Euclidean distance between two points.
fn distance(px: f32, py: f32, qx: f32, qy: f32) -> f32 {
    let dx = px - qx;
    let dy = py - qy;
    (dx * dx + dy * dy).sqrt()
}

/// Signed coverage of a stroked circle (ring) — positive within `half` of the
/// circle of radius `r` centred at `(cx, cy)`.
fn ring(px: f32, py: f32, cx: f32, cy: f32, r: f32, half: f32) -> f32 {
    half - (distance(px, py, cx, cy) - r).abs()
}

/// Signed coverage of a filled disc.
fn disc(px: f32, py: f32, cx: f32, cy: f32, r: f32) -> f32 {
    r - distance(px, py, cx, cy)
}

/// Signed coverage of a thick line segment `(ax, ay)`–`(bx, by)` with half-width
/// `half` (a capsule).
fn segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, half: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq <= f32::EPSILON {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0)
    };
    half - distance(px, py, ax + t * dx, ay + t * dy)
}

/// The larger (union) of two signed coverages.
const fn union(a: f32, b: f32) -> f32 {
    a.max(b)
}

/// Subtract `hole` from `shape` (a signed-coverage set difference).
fn cut(shape: f32, hole: f32) -> f32 {
    shape.min(-hole)
}

/// The see-online icon: an eye — a lens ring with a filled pupil.
fn icon_eye(nx: f32, ny: f32) -> f32 {
    // A wide, short lens ring plus a central pupil, both centred.
    let outer = ring(nx, ny, 0.5, 0.5, 0.30, 0.045);
    // Squash the vertical so the ring reads as a lens, not a circle.
    let squashed = ring(nx, (ny - 0.5) * 1.7 + 0.5, 0.5, 0.5, 0.26, 0.05);
    let pupil = disc(nx, ny, 0.5, 0.5, 0.085);
    union(union(outer.min(squashed), squashed), pupil)
}

/// The see-on-map icon: a location pin — a disc over a triangle, with a hole.
fn icon_pin(nx: f32, ny: f32) -> f32 {
    let head = disc(nx, ny, 0.5, 0.4, 0.24);
    let tip = triangle(nx, ny, 0.5, 0.86, 0.3, 0.5, 0.7, 0.5);
    let hole = disc(nx, ny, 0.5, 0.4, 0.10);
    cut(union(head, tip), hole)
}

/// The edit icon: a pencil — a thick body segment tapering to a thin tip.
fn icon_pencil(nx: f32, ny: f32) -> f32 {
    let body = segment(nx, ny, 0.70, 0.24, 0.36, 0.58, 0.075);
    let tip = segment(nx, ny, 0.36, 0.58, 0.20, 0.80, 0.035);
    union(body, tip)
}

/// A ticked checkbox: a square outline with a check mark.
fn icon_check_on(nx: f32, ny: f32) -> f32 {
    let check = union(
        segment(nx, ny, 0.28, 0.52, 0.44, 0.68, 0.06),
        segment(nx, ny, 0.44, 0.68, 0.74, 0.30, 0.06),
    );
    union(square_ring(nx, ny), check)
}

/// An empty checkbox: just the square outline.
fn icon_check_off(nx: f32, ny: f32) -> f32 {
    square_ring(nx, ny)
}

/// Signed coverage of the checkbox's square outline (a stroked, centred square).
fn square_ring(nx: f32, ny: f32) -> f32 {
    let half = 0.32;
    let stroke = 0.055;
    let chebyshev = (nx - 0.5).abs().max((ny - 0.5).abs());
    stroke - (chebyshev - half).abs()
}

/// Signed coverage (inside-positive) of a filled triangle, via the sign of the
/// three edge half-planes (assumes the vertices wind consistently).
#[expect(
    clippy::too_many_arguments,
    reason = "a triangle is its point plus three vertices as x/y scalars; grouping them into \
              tuples would obscure the per-coordinate rasteriser math"
)]
fn triangle(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    let e0 = edge(px, py, ax, ay, bx, by);
    let e1 = edge(px, py, bx, by, cx, cy);
    let e2 = edge(px, py, cx, cy, ax, ay);
    let inside = (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0);
    let margin = e0.abs().min(e1.abs()).min(e2.abs());
    if inside { margin } else { -margin }
}

/// The signed area (cross product) of the edge `(ax, ay)`–`(bx, by)` against `p`.
fn edge(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

/// One render-ready friend row: the ids the actions need, the display name, the
/// presence flag, and the friendship rights in both directions (the table's
/// permission columns).
#[derive(Debug, Clone, PartialEq, Eq)]
struct FriendRow {
    /// The friend id (for remove / grant-rights, which take a [`FriendKey`]).
    friend: FriendKey,
    /// The agent id (for IM / teleport / mute, which take an [`AgentKey`]).
    agent: AgentKey,
    /// The display name (or a short-id placeholder until the name resolves).
    name: String,
    /// Whether the friend is currently known-online.
    online: bool,
    /// The rights this agent grants the friend (the "They can …" columns).
    rights_granted: FriendRights,
    /// The rights the friend grants this agent (the "You can …" columns).
    rights_received: FriendRights,
}

/// One of the three friendship rights a permission column can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightKind {
    /// See the other party's online status (`CAN_SEE_ONLINE`).
    SeeOnline,
    /// Locate the other party on the world map (`CAN_SEE_ON_MAP`).
    Map,
    /// Edit / delete / take the other party's objects (`CAN_MODIFY_OBJECTS`).
    Edit,
}

impl RightKind {
    /// Whether this right's bit is set in `rights`.
    const fn is_set(self, rights: FriendRights) -> bool {
        match self {
            Self::SeeOnline => rights.can_see_online(),
            Self::Map => rights.can_see_on_map(),
            Self::Edit => rights.can_modify_objects(),
        }
    }
}

/// The six permission columns, in display order: the three rights this agent
/// grants the friend ("They can …"), then the three the friend grants this agent
/// ("You can …"). `received` selects which rights field a column reads.
const RIGHT_COLUMNS: [(bool, RightKind); 6] = [
    (false, RightKind::SeeOnline),
    (false, RightKind::Map),
    (false, RightKind::Edit),
    (true, RightKind::SeeOnline),
    (true, RightKind::Map),
    (true, RightKind::Edit),
];

/// Whether a friend row's column `column` (a `(received, kind)` pair) is set.
const fn column_is_set(row: &FriendRow, column: (bool, RightKind)) -> bool {
    let (received, kind) = column;
    let rights = if received {
        row.rights_received
    } else {
        row.rights_granted
    };
    kind.is_set(rights)
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

/// A column the friends table can be sorted by (every clickable header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    /// The display name.
    Name,
    /// Online presence.
    Online,
    /// A permission column: `(received, kind)`, matching a [`RIGHT_COLUMNS`] entry.
    Right(bool, RightKind),
}

/// One level of the multi-column sort: a column and its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SortKey {
    /// The column this level orders by.
    column: SortColumn,
    /// Ascending (else descending).
    ascending: bool,
}

/// The most sort levels remembered; clicks past this drop the least significant.
const MAX_SORT_KEYS: usize = 6;

/// The ordered multi-column sort — most-significant key first. A header click
/// promotes its column to the front (or flips its direction if already front),
/// demoting the previous order to tie-breakers: "sort by the last-clicked column,
/// then the one before that, …". Persisted per avatar
/// ([`FRIENDS_SORT_SETTING`]).
#[derive(Debug, Clone)]
struct SortState {
    /// The sort levels, most-significant first.
    keys: Vec<SortKey>,
}

impl Default for SortState {
    fn default() -> Self {
        // The viewer's original order, expressed as a two-level sort: online first
        // (descending, so online precedes offline), then name ascending.
        Self {
            keys: vec![
                SortKey {
                    column: SortColumn::Online,
                    ascending: false,
                },
                SortKey {
                    column: SortColumn::Name,
                    ascending: true,
                },
            ],
        }
    }
}

impl SortState {
    /// Apply a header click: toggle the front column's direction if it is already
    /// primary, else promote `column` to the front (demoting the rest).
    fn click(&mut self, column: SortColumn) {
        if let Some(front) = self.keys.first_mut()
            && front.column == column
        {
            front.ascending = !front.ascending;
            return;
        }
        self.keys.retain(|key| key.column != column);
        self.keys.insert(
            0,
            SortKey {
                column,
                ascending: default_ascending(column),
            },
        );
        self.keys.truncate(MAX_SORT_KEYS);
    }

    /// The primary (most-significant) sort key, if any.
    fn primary(&self) -> Option<SortKey> {
        self.keys.first().copied()
    }

    /// Order two rows by the full key stack, with a stable name / id tie-break.
    fn compare(&self, left: &FriendRow, right: &FriendRow) -> Ordering {
        for key in &self.keys {
            let base = column_ordering(key.column, left, right);
            let ord = if key.ascending { base } else { base.reverse() };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.friend.uuid().cmp(&right.friend.uuid()))
    }

    /// Encode the sort as a compact `col:dir,col:dir` string for persistence.
    fn encode(&self) -> String {
        self.keys
            .iter()
            .map(|key| {
                let dir = if key.ascending { "a" } else { "d" };
                format!("{}:{dir}", column_token(key.column))
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse a persisted sort string, falling back to [`Self::default`] when it is
    /// empty or wholly unrecognised (dropping duplicate / unknown columns).
    fn parse(text: &str) -> Self {
        let mut keys: Vec<SortKey> = Vec::new();
        for part in text.split(',') {
            let mut fields = part.split(':');
            let (Some(token), Some(dir)) = (fields.next(), fields.next()) else {
                continue;
            };
            let Some(column) = parse_column_token(token) else {
                continue;
            };
            if keys.iter().any(|key| key.column == column) {
                continue;
            }
            keys.push(SortKey {
                column,
                ascending: dir == "a",
            });
        }
        if keys.is_empty() {
            Self::default()
        } else {
            Self { keys }
        }
    }
}

/// The default direction for a freshly-clicked column: online descending
/// (online-first, the natural expectation), everything else ascending.
const fn default_ascending(column: SortColumn) -> bool {
    !matches!(column, SortColumn::Online)
}

/// Order two rows by a single column (before the direction is applied).
fn column_ordering(column: SortColumn, left: &FriendRow, right: &FriendRow) -> Ordering {
    match column {
        SortColumn::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        SortColumn::Online => left.online.cmp(&right.online),
        SortColumn::Right(received, kind) => {
            column_is_set(left, (received, kind)).cmp(&column_is_set(right, (received, kind)))
        }
    }
}

/// The persistence token for a sort column (the inverse of [`parse_column_token`]).
const fn column_token(column: SortColumn) -> &'static str {
    match column {
        SortColumn::Name => "name",
        SortColumn::Online => "online",
        SortColumn::Right(false, RightKind::SeeOnline) => "g_o",
        SortColumn::Right(false, RightKind::Map) => "g_m",
        SortColumn::Right(false, RightKind::Edit) => "g_e",
        SortColumn::Right(true, RightKind::SeeOnline) => "r_o",
        SortColumn::Right(true, RightKind::Map) => "r_m",
        SortColumn::Right(true, RightKind::Edit) => "r_e",
    }
}

/// Parse a persisted sort column token, or `None` if unrecognised.
fn parse_column_token(token: &str) -> Option<SortColumn> {
    Some(match token {
        "name" => SortColumn::Name,
        "online" => SortColumn::Online,
        "g_o" => SortColumn::Right(false, RightKind::SeeOnline),
        "g_m" => SortColumn::Right(false, RightKind::Map),
        "g_e" => SortColumn::Right(false, RightKind::Edit),
        "r_o" => SortColumn::Right(true, RightKind::SeeOnline),
        "r_m" => SortColumn::Right(true, RightKind::Map),
        "r_e" => SortColumn::Right(true, RightKind::Edit),
        _other => return None,
    })
}

// ---------------------------------------------------------------------------
// Row actions
// ---------------------------------------------------------------------------

/// A per-friend action offered by the action bar under the Friends list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FriendAction {
    /// Open a one-to-one IM tab for the friend (via
    /// [`OpenConversation`], not a wire command).
    Im,
    /// Offer the friend a teleport to us.
    OfferTeleport,
    /// Remove the friendship.
    RemoveFriend,
    /// Block (mute) the friend.
    Block,
}

impl FriendAction {
    /// The Fluent key for this action's button label.
    const fn label_key(self) -> &'static str {
        match self {
            Self::Im => ACTION_IM_KEY,
            Self::OfferTeleport => ACTION_TELEPORT_KEY,
            Self::RemoveFriend => ACTION_REMOVE_KEY,
            Self::Block => ACTION_BLOCK_KEY,
        }
    }
}

/// The wire [`Command`] an action produces for `friend` (named `name` for the
/// mute entry), or `None` for [`FriendAction::Im`] — which opens a conversation
/// tab rather than sending a command. Pure so the routing is unit-testable.
fn friend_command(action: FriendAction, friend: FriendKey, name: &str) -> Option<Command> {
    let agent = AgentKey::from(friend);
    match action {
        FriendAction::Im => None,
        FriendAction::OfferTeleport => Some(Command::OfferTeleport {
            targets: vec![agent],
            message: String::new(),
        }),
        FriendAction::RemoveFriend => Some(Command::TerminateFriendship(friend)),
        FriendAction::Block => Some(Command::Mute {
            id: agent.uuid(),
            name: name.to_owned(),
            mute_type: MuteType::Agent,
            flags: MuteFlags::default(),
        }),
    }
}

// ---------------------------------------------------------------------------
// ECS side
// ---------------------------------------------------------------------------

/// The People tab / pane entities — the ECS mirror of [`FriendsModel`].
#[derive(Resource, Debug)]
pub(crate) struct PeopleUi {
    /// The People tab button in the conversations strip (recoloured active /
    /// inactive).
    tab_button: Entity,
    /// The People pane, displayed only while the strip focus is external.
    pane: Entity,
    /// The Friends / Groups sub-tab strip (its [`TabStrip::active`] switches the
    /// pane contents).
    sub_strip: Entity,
    /// The Friends content column (list + action bar), shown for the Friends tab.
    friends_content: Entity,
    /// The virtualized friends-list viewport (carries [`VirtualList`]).
    friends_viewport: Entity,
    /// The Groups sub-tab content container, shown for the Groups tab. It is
    /// spawned here (so the sub-tab switch in [`refresh_people`] can toggle it) but
    /// filled by [`crate::groups`], which owns the group list — the same
    /// deferred-into-another-plugin arrangement this pane itself uses with the
    /// [`ConversationsUi`] strip.
    groups_content: Entity,
    /// The Name header's sort-direction arrow node (updated from the primary sort).
    name_arrow: Entity,
    /// The Status header's sort-direction arrow node.
    status_arrow: Entity,
    /// The generated table icons, for the row checkboxes' bind-time swap.
    icons: PeopleIcons,
    /// The edit-objects grant-confirm modal overlay (shown while a grant is
    /// pending).
    confirm_overlay: Entity,
    /// The confirm modal's prompt text node (rewritten with the friend's name).
    confirm_text: Entity,
}

impl PeopleUi {
    /// The Groups sub-tab content container, so [`crate::groups`] can build the
    /// group list into the same pane the People surface owns (this pane still
    /// toggles its visibility from the Friends / Groups sub-tab in
    /// [`refresh_people`]).
    pub(crate) const fn groups_content(&self) -> Entity {
        self.groups_content
    }
}

/// The ordered, render-ready friends projection the virtualized list binds to.
#[derive(Resource, Debug, Default)]
pub(crate) struct FriendsView {
    /// The rows in display order.
    rows: Vec<FriendRow>,
    /// The model revision this view was last built from.
    built_revision: u64,
}

/// The currently-selected friend, which the action bar acts on.
#[derive(Resource, Debug, Default)]
pub(crate) struct SelectedFriend(Option<FriendKey>);

/// The last friend-row click, for detecting a double-click (two presses on the
/// same friend within [`DOUBLE_CLICK_SECS`] open a one-to-one IM, like the IM
/// button). Tracked by friend id, not row entity, since the virtualized rows are
/// recycled.
#[derive(Resource, Debug, Default)]
pub(crate) struct FriendClickTracker {
    /// The friend the last press selected, if any.
    friend: Option<FriendKey>,
    /// When that press landed, in seconds since startup ([`Time::elapsed_secs`]).
    time: f32,
}

/// A pending, not-yet-confirmed grant of the **edit-my-objects** right — the one
/// right dangerous enough to gate behind a confirm dialog (the reference does the
/// same). `None` when no confirm is open. The see-online / see-on-map grants and
/// every revoke are immediate, so they never set this.
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingGrantConfirm(Option<PendingGrant>);

/// The friend and full rights bitfield a confirmed grant would apply.
#[derive(Debug, Clone, Copy)]
struct PendingGrant {
    /// The friend whose granted rights would change.
    friend: FriendKey,
    /// The full rights bitfield to send on confirm (the current rights with the
    /// edit-objects bit set).
    rights: FriendRights,
}

/// A request to make the People tab the front surface — written by the People tab
/// button's press observer.
#[derive(Message, Debug, Clone, Copy)]
struct SelectPeople;

/// A request to sort the friends table by a column — written by a header click.
#[derive(Message, Debug, Clone, Copy)]
struct SortByColumn {
    /// The clicked column.
    column: SortColumn,
}

/// The persistent inner parts of a pooled friend row, updated in place on bind.
#[derive(Component)]
struct FriendRowParts {
    /// The presence-dot glyph node.
    presence: Entity,
    /// The name label node.
    label: Entity,
    /// The six permission-icon glyph nodes, in [`RIGHT_COLUMNS`] order (the three
    /// "They can …" columns, then the three "You can …").
    rights: [Entity; 6],
}

/// The friend a pooled row currently presents (so a press knows who to select),
/// or `None` when the row is parked.
#[derive(Component, Debug, Clone, Copy)]
struct BoundFriend(Option<FriendKey>);

/// Which permission an **editable** (granted) rights checkbox toggles — only the
/// "They can …" cells carry the toggle observer that reads this.
#[derive(Component, Debug, Clone, Copy)]
struct RightCell {
    /// Which of the three rights this cell shows.
    kind: RightKind,
}

/// The friend a rights checkbox cell currently applies to (updated on bind), so a
/// toggle click acts on the right row without walking the hierarchy.
#[derive(Component, Debug, Clone, Copy)]
struct CellFriend(Option<FriendKey>);

/// The People plugin: the model + view + selection resources, the deferred tab
/// spawn, event ingest, name requests, selection, refresh, and row binding.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PeoplePlugin;

impl Plugin for PeoplePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FriendsModel>()
            .init_resource::<FriendsView>()
            .init_resource::<SelectedFriend>()
            .init_resource::<FriendClickTracker>()
            .init_resource::<PendingGrantConfirm>()
            .add_message::<SelectPeople>()
            .add_message::<SortByColumn>()
            .add_systems(Startup, register_people_settings)
            .add_systems(
                Update,
                (
                    spawn_people_tab.after(UiScaffoldSystems::SpawnRoot),
                    ingest_friend_events,
                    request_friend_names,
                    apply_people_selection,
                    seed_sort_from_settings.after(load_account_settings),
                    apply_sort,
                    rebuild_friends_view,
                    refresh_people,
                    drive_grant_confirm,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (populate_friend_rows, bind_friend_rows)
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn (deferred until the conversations floater exists)
// ---------------------------------------------------------------------------

/// Spawn the People tab and pane into the conversations floater's strip / panel
/// area, once ([`PeopleUi`] absent) and only after that floater exists
/// ([`ConversationsUi`] present). Runs each frame until it succeeds, then no-ops —
/// the robust alternative to Startup ordering across two plugins whose resources
/// are inserted by deferred commands.
fn spawn_people_tab(
    mut commands: Commands,
    conversations: Option<Res<ConversationsUi>>,
    people: Option<Res<PeopleUi>>,
    root: Res<UiRoot>,
    mut images: ResMut<Assets<Image>>,
) {
    if people.is_some() {
        return;
    }
    let Some(conversations) = conversations else {
        return;
    };
    let strip = conversations.strip();
    let panel_area = conversations.panel_area();
    let icons = PeopleIcons::generate(&mut images);

    // The pinned People tab button — a label, no close button (un-closable like
    // Nearby Chat), styled to match the conversation tabs.
    let tab_button = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..row(Val::Px(4.0))
            },
            BorderColor::all(TAB_BORDER),
            BackgroundColor(TAB_INACTIVE_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("people-tab"),
        ))
        .observe(
            |press: On<Pointer<Press>>, mut select: MessageWriter<SelectPeople>| {
                if press.button == PointerButton::Primary {
                    select.write(SelectPeople);
                }
            },
        )
        .id();
    // Insert the People tab as the **first** tab in the strip, above Nearby Chat,
    // so every chat tab (nearby + IMs + groups) stays grouped below it.
    commands.entity(strip).insert_child(0, tab_button);
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(CHROME_FONT_SIZE),
        TextColor(LABEL_COLOR),
        Translated::new(PEOPLE_TAB_KEY),
        Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            ..default()
        },
        Pickable::IGNORE,
        Name::new("people-tab-label"),
        ChildOf(tab_button),
    ));

    // The pane — hidden unless the People tab owns the strip.
    let pane = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                display: Display::None,
                padding: UiRect::all(Val::Px(6.0)),
                ..column(Val::Px(6.0))
            },
            Name::new("people-pane"),
            ChildOf(panel_area),
        ))
        .id();

    // The horizontal Friends / Groups sub-tab strip.
    let sub_strip = spawn_tab_strip(
        &mut commands,
        pane,
        &TabSpec {
            element: SUB_STRIP_ELEMENT,
            placement: TabPlacement::BlockStart,
            labels: &[FRIENDS_TAB_KEY.to_owned(), GROUPS_TAB_KEY.to_owned()],
            active: FRIENDS_TAB_INDEX,
            tab_index: 1,
            font_size: CHROME_FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );

    let (friends_content, friends_viewport, name_arrow, status_arrow) =
        spawn_friends_content(&mut commands, pane, &icons);
    let groups_content = spawn_groups_content(&mut commands, pane);
    let (confirm_overlay, confirm_text) = spawn_grant_confirm_modal(&mut commands, root.0);

    commands.insert_resource(PeopleUi {
        tab_button,
        pane,
        sub_strip,
        friends_content,
        friends_viewport,
        groups_content,
        name_arrow,
        status_arrow,
        icons,
        confirm_overlay,
        confirm_text,
    });
}

/// Spawn the edit-objects grant-confirm modal: a full-window scrim (blocking
/// clicks behind it) centred on a warning box with the prompt and Cancel / Grant
/// buttons. Hidden until a grant is pending. Returns `(overlay, prompt_text)`.
fn spawn_grant_confirm_modal(commands: &mut Commands, root: Entity) -> (Entity, Entity) {
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
            // Block clicks (and the checkbox behind) while the modal is up.
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("people-grant-confirm-overlay"),
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
            Name::new("people-grant-confirm-box"),
            ChildOf(overlay),
        ))
        .id();
    let confirm_text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            Name::new("people-grant-confirm-text"),
            ChildOf(box_node),
        ))
        .id();
    let buttons = commands
        .spawn((
            Node {
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            Name::new("people-grant-confirm-buttons"),
            ChildOf(box_node),
        ))
        .id();
    spawn_confirm_button(
        commands,
        buttons,
        GRANT_CONFIRM_NO_KEY,
        CONFIRM_CANCEL_BACKGROUND,
        false,
    );
    spawn_confirm_button(
        commands,
        buttons,
        GRANT_CONFIRM_YES_KEY,
        CONFIRM_GRANT_BACKGROUND,
        true,
    );
    (overlay, confirm_text)
}

/// Spawn one confirm-modal button (`grant` = the Grant button, else Cancel).
fn spawn_confirm_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    background: Color,
    grant: bool,
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
            Name::new("people-grant-confirm-button"),
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
                  mut pending: ResMut<PendingGrantConfirm>,
                  mut model: ResMut<FriendsModel>,
                  mut sl: MessageWriter<SlCommand>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                let taken = pending.0.take();
                if grant && let Some(grant) = taken {
                    model.set_granted(grant.friend, grant.rights);
                    sl.write(SlCommand(Command::GrantUserRights {
                        target: grant.friend,
                        rights: grant.rights,
                    }));
                }
            },
        );
}

/// Spawn the Friends sub-tab content: a list column (a persistent, sortable table
/// header above the virtualized avatar list) beside a **trailing** column of
/// per-friend action buttons. Returns
/// `(content, viewport, name_arrow, status_arrow)`.
fn spawn_friends_content(
    commands: &mut Commands,
    pane: Entity,
    icons: &PeopleIcons,
) -> (Entity, Entity, Entity, Entity) {
    // The content is a row: the list column takes the width, the action column
    // sits at its trailing edge.
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..row(Val::Px(6.0))
            },
            Name::new("people-friends-content"),
            ChildOf(pane),
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
            Name::new("people-friends-list-column"),
            ChildOf(content),
        ))
        .id();
    let (name_arrow, status_arrow) = spawn_friends_header(commands, list_column, icons);

    // The virtualized list viewport fills the remaining height and clips + owns
    // its own scroll, exactly like the inventory viewport.
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
            Name::new("people-friends-viewport"),
            ChildOf(list_column),
        ))
        .observe(
            |press: On<Pointer<Press>>, ui: Res<PeopleUi>, mut focus: ResMut<InputFocus>| {
                if press.button == PointerButton::Primary {
                    focus.set(ui.friends_viewport, FocusCause::Navigated);
                }
            },
        )
        .id();

    // The trailing action column — one button per [`FriendAction`], stacked and
    // acting on the current selection.
    let actions = commands
        .spawn((
            Node {
                width: Val::Px(ACTION_COL_WIDTH),
                flex_shrink: 0.0,
                align_items: AlignItems::Stretch,
                ..column(Val::Px(4.0))
            },
            Name::new("people-friends-actions"),
            ChildOf(content),
        ))
        .id();
    for action in [
        FriendAction::Im,
        FriendAction::OfferTeleport,
        FriendAction::RemoveFriend,
        FriendAction::Block,
    ] {
        spawn_action_button(commands, actions, action);
    }

    (content, viewport, name_arrow, status_arrow)
}

/// Spawn the persistent friends-table header row (always shown, even for an empty
/// list): a clickable "Name" column over the row labels, a fixed "Status" column
/// over the presence dots, and the two permission groups ("They can …" / "You can
/// …") over their three rights columns. Every header sorts on click; returns the
/// `(name_arrow, status_arrow)` sort-direction indicator nodes.
fn spawn_friends_header(
    commands: &mut Commands,
    list_column: Entity,
    icons: &PeopleIcons,
) -> (Entity, Entity) {
    let header = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexEnd,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                column_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(HEADER_BACKGROUND),
            Name::new("people-friends-header"),
            ChildOf(list_column),
        ))
        .id();
    let name_arrow = spawn_sortable_header(
        commands,
        header,
        HeaderWidth::Grow,
        HEADER_NAME_KEY,
        SortColumn::Name,
    );
    let status_arrow = spawn_sortable_header(
        commands,
        header,
        HeaderWidth::Fixed(STATUS_COL_WIDTH),
        HEADER_STATUS_KEY,
        SortColumn::Online,
    );
    // The two rights groups: a group label above a row of clickable icon column
    // headers — one group over "They can …", one over "You can …".
    spawn_rights_group_header(commands, header, GROUP_THEY_KEY, false, icons);
    spawn_rights_group_header(commands, header, GROUP_YOU_KEY, true, icons);
    (name_arrow, status_arrow)
}

/// How a sortable header cell is sized: growing to fill the row (Name) or a fixed
/// width (Status).
#[derive(Debug, Clone, Copy)]
enum HeaderWidth {
    /// Fill the remaining width.
    Grow,
    /// A fixed logical-pixel width.
    Fixed(f32),
}

/// Spawn a clickable header cell (`label` + a sort-direction arrow) that sorts by
/// `column` on click; returns the arrow node (updated from the primary sort).
fn spawn_sortable_header(
    commands: &mut Commands,
    header: Entity,
    width: HeaderWidth,
    key: &'static str,
    column: SortColumn,
) -> Entity {
    let sizing = match width {
        HeaderWidth::Grow => Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(2.0),
            ..default()
        },
        HeaderWidth::Fixed(pixels) => Node {
            width: Val::Px(pixels),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: Val::Px(2.0),
            ..default()
        },
    };
    let cell = commands
        .spawn((
            sizing,
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("people-header-sortable"),
            ChildOf(header),
        ))
        .observe(sort_on_press(column))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(ROW_FONT_SIZE),
        TextColor(HEADER_TEXT_COLOR),
        Translated::new(key),
        Pickable::IGNORE,
        ChildOf(cell),
    ));
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(ROW_FONT_SIZE),
            TextColor(TAB_ACTIVE_BORDER),
            Pickable::IGNORE,
            Name::new("people-header-arrow"),
            ChildOf(cell),
        ))
        .id()
}

/// An observer that sorts by `column` on a primary-button press.
fn sort_on_press(column: SortColumn) -> impl Fn(On<Pointer<Press>>, MessageWriter<SortByColumn>) {
    move |mut press: On<Pointer<Press>>, mut sort: MessageWriter<SortByColumn>| {
        press.propagate(false);
        if press.button == PointerButton::Primary {
            sort.write(SortByColumn { column });
        }
    }
}

/// Spawn one permission-group header — a group label above a row of three
/// one-letter, clickable column headers (online / map / edit), sized to sit over
/// the row's matching three rights cells. `received` selects the granted-vs-
/// received sort column each letter maps to.
fn spawn_rights_group_header(
    commands: &mut Commands,
    header: Entity,
    group_key: &'static str,
    received: bool,
    icons: &PeopleIcons,
) {
    let group = commands
        .spawn((
            Node {
                width: Val::Px(RIGHT_COL_WIDTH * 3.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..column(Val::Px(1.0))
            },
            Pickable::IGNORE,
            ChildOf(header),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        UiFont::Sans.at(ROW_FONT_SIZE),
        TextColor(HEADER_TEXT_COLOR),
        Translated::new(group_key),
        Pickable::IGNORE,
        ChildOf(group),
    ));
    let letters = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(group),
        ))
        .id();
    for kind in [RightKind::SeeOnline, RightKind::Map, RightKind::Edit] {
        let cell = commands
            .spawn((
                Node {
                    width: Val::Px(RIGHT_COL_WIDTH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable {
                    should_block_lower: true,
                    is_hoverable: true,
                },
                ChildOf(letters),
            ))
            .observe(sort_on_press(SortColumn::Right(received, kind)))
            .id();
        commands.spawn((
            ImageNode {
                color: HEADER_TEXT_COLOR,
                ..ImageNode::new(icons.kind_icon(kind))
            },
            Node {
                width: Val::Px(ICON_DISPLAY),
                height: Val::Px(ICON_DISPLAY),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(cell),
        ));
    }
}

/// Spawn one action-column button wired to `action`.
fn spawn_action_button(commands: &mut Commands, actions: Entity, action: FriendAction) {
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("people-friends-action"),
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
                  selected: Res<SelectedFriend>,
                  model: Res<FriendsModel>,
                  mut sl: MessageWriter<SlCommand>,
                  mut open: MessageWriter<OpenConversation>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                let Some(friend) = selected.0 else {
                    return;
                };
                let agent = AgentKey::from(friend);
                if action == FriendAction::Im {
                    open.write(OpenConversation {
                        key: ConversationKey::Direct(agent),
                    });
                    return;
                }
                let name = model.name_of(agent).unwrap_or_default();
                if let Some(command) = friend_command(action, friend, name) {
                    sl.write(SlCommand(command));
                }
            },
        );
}

/// Spawn the Groups sub-tab content container — an empty column, hidden until the
/// Groups sub-tab is selected. The group **list** that fills it is
/// [`crate::groups`]'s job (the `viewer-social-groups` task); this pane only owns
/// the slot and its Friends / Groups visibility toggle, exactly as the People pane
/// itself is a slot hosted in the [`ConversationsUi`] strip.
fn spawn_groups_content(commands: &mut Commands, pane: Entity) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                display: Display::None,
                ..column(Val::ZERO)
            },
            Name::new("people-groups-content"),
            ChildOf(pane),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Ingest / names
// ---------------------------------------------------------------------------

/// Fold every friend-relevant inbound event into [`FriendsModel`].
fn ingest_friend_events(mut events: MessageReader<SlEvent>, mut model: ResMut<FriendsModel>) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::FriendList(friends) => model.note_friends(friends),
            SlSessionEvent::FriendsSnapshot(presence) => model.apply_snapshot(presence),
            SlSessionEvent::FriendsOnline(friends) => model.set_online(friends, true),
            SlSessionEvent::FriendsOffline(friends) => model.set_online(friends, false),
            SlSessionEvent::FriendRightsChanged {
                friend_id,
                rights,
                granted_to_us,
            } => model.update_rights(*friend_id, *rights, *granted_to_us),
            SlSessionEvent::FriendshipTerminated { other } => model.remove(*other),
            SlSessionEvent::AvatarNames(names) => {
                for name in names {
                    model.note_name(name.id, &name.legacy_name());
                }
            }
            _other => {}
        }
    }
}

/// Request legacy names for any friend whose name is not yet resolved, once each
/// (the `requested` guard mirrors [`crate::avatars`]'s name-request pattern).
fn request_friend_names(
    model: Res<FriendsModel>,
    mut requested: Local<BTreeSet<AgentKey>>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !model.is_changed() {
        return;
    }
    let wanted: Vec<AgentKey> = model
        .unnamed()
        .into_iter()
        .filter(|agent| !requested.contains(agent))
        .collect();
    if wanted.is_empty() {
        return;
    }
    for agent in &wanted {
        requested.insert(*agent);
    }
    commands.write(SlCommand(Command::RequestAvatarNames(wanted)));
}

// ---------------------------------------------------------------------------
// Selection / view / refresh
// ---------------------------------------------------------------------------

/// Give the strip to the People pane when its tab is pressed, and seed the buddy
/// list with a [`Command::QueryFriends`] the first time (later kept live by the
/// granular friend events).
fn apply_people_selection(
    mut selects: MessageReader<SelectPeople>,
    mut focus: ResMut<StripFocus>,
    mut seeded: Local<bool>,
    mut commands: MessageWriter<SlCommand>,
) {
    let mut selected = false;
    for _select in selects.read() {
        selected = true;
    }
    if !selected {
        return;
    }
    focus.take_external();
    if !*seeded {
        *seeded = true;
        commands.write(SlCommand(Command::QueryFriends));
    }
}

/// Register the persisted friends-sort setting (its declared default is the
/// natural order) so the account file that loads at login is coerced to a string.
fn register_people_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        PEOPLE_SETTINGS_SECTION,
        FRIENDS_SORT_SETTING,
        SettingValue::String(SortState::default().encode()),
        "Friends-list sort order, most-significant column first (col:dir, …).",
    );
}

/// Seed the sort order from the persisted value, once, after the per-avatar
/// account scope is loaded (mirrors [`crate::floater_persist`]'s seed stage).
fn seed_sort_from_settings(
    settings: Option<Res<ViewerSettings>>,
    mut model: ResMut<FriendsModel>,
    mut seeded: Local<bool>,
) {
    if *seeded {
        return;
    }
    let Some(settings) = settings else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    *seeded = true;
    if let Ok(encoded) = settings.store().get_str(FRIENDS_SORT_SETTING) {
        model.set_sort(SortState::parse(encoded));
    }
}

/// Apply header-click sort requests to the model and persist the new order to the
/// account settings (saved immediately, since a sort click is a rare action).
fn apply_sort(
    mut events: MessageReader<SortByColumn>,
    mut model: ResMut<FriendsModel>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let mut encoded = None;
    for event in events.read() {
        encoded = Some(model.sort_by(event.column));
    }
    let Some(encoded) = encoded else {
        return;
    };
    if let Some(mut settings) = settings
        && settings.account_loaded()
    {
        settings.set_account(FRIENDS_SORT_SETTING, SettingValue::String(encoded));
        settings.save();
    }
}

/// Rebuild [`FriendsView`] whenever the model's revision advances, resetting the
/// list scroll to the top so the new order is read from its start.
fn rebuild_friends_view(
    model: Res<FriendsModel>,
    mut view: ResMut<FriendsView>,
    ui: Option<Res<PeopleUi>>,
    mut lists: Query<&mut VirtualList>,
) {
    if view.built_revision == model.revision {
        return;
    }
    view.built_revision = model.revision;
    view.rows = model.ordered();
    if let Some(ui) = ui
        && let Ok(mut list) = lists.get_mut(ui.friends_viewport)
    {
        list.item_count = view.rows.len();
        list.scroll_to_top();
    }
}

/// Keep the People surface in step: the tab colours (active while the strip focus
/// is external), the pane visibility, the Friends / Groups sub-content switch, and
/// the Name / Status sort-direction arrows from the primary sort key.
#[expect(
    clippy::too_many_arguments,
    reason = "reflecting the model + focus into the People chrome touches the tab background, \
              border, pane / content display, sub-strip selection and the two sort arrows — \
              distinct node aspects that belong in one coherent refresh"
)]
fn refresh_people(
    focus: Res<StripFocus>,
    ui: Option<Res<PeopleUi>>,
    model: Res<FriendsModel>,
    strips: Query<&TabStrip>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    let active = focus.is_external();

    // Sort-direction arrows: only the primary (most-significant) column shows one.
    let primary = model.primary_sort();
    set_arrow(
        &mut texts,
        ui.name_arrow,
        sort_arrow(primary, SortColumn::Name),
    );
    set_arrow(
        &mut texts,
        ui.status_arrow,
        sort_arrow(primary, SortColumn::Online),
    );

    let (background, border) = if active {
        (TAB_ACTIVE_BACKGROUND, TAB_ACTIVE_BORDER)
    } else {
        (TAB_INACTIVE_BACKGROUND, TAB_BORDER)
    };
    set_background(&mut backgrounds, ui.tab_button, background);
    if let Ok(mut color) = borders.get_mut(ui.tab_button) {
        let wanted = BorderColor::all(border);
        if *color != wanted {
            *color = wanted;
        }
    }

    set_display(&mut nodes, ui.pane, active);

    // Switch the Friends / Groups content from the sub-strip's active tab.
    let friends_active = strips
        .get(ui.sub_strip)
        .map_or(true, |strip| strip.active == FRIENDS_TAB_INDEX);
    set_display(&mut nodes, ui.friends_content, friends_active);
    set_display(&mut nodes, ui.groups_content, !friends_active);
}

/// Set a node's background only on a real change.
fn set_background(backgrounds: &mut Query<&mut BackgroundColor>, entity: Entity, color: Color) {
    if let Ok(mut background) = backgrounds.get_mut(entity)
        && background.0 != color
    {
        background.0 = color;
    }
}

/// Toggle a node shown / hidden only on a real change.
fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// The arrow glyph a header shows: an up / down arrow when `column` is the primary
/// sort key, else empty (only the most-significant column is marked).
fn sort_arrow(primary: Option<SortKey>, column: SortColumn) -> &'static str {
    match primary {
        Some(key) if key.column == column => {
            if key.ascending {
                SORT_ASCENDING_GLYPH
            } else {
                SORT_DESCENDING_GLYPH
            }
        }
        _other => "",
    }
}

/// Set a header arrow node's glyph only on a real change.
fn set_arrow(texts: &mut Query<&mut Text>, entity: Entity, glyph: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != glyph
    {
        glyph.clone_into(&mut text.0);
    }
}

// ---------------------------------------------------------------------------
// Row pool: populate + bind
// ---------------------------------------------------------------------------

/// Build the inner nodes of each freshly-pooled friend row once (a presence dot
/// and a name label) and wire its click to select the friend.
fn populate_friend_rows(
    mut commands: Commands,
    ui: Option<Res<PeopleUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.friends_viewport {
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
        // Name first (fills the row), presence dot last in a fixed-width status
        // cell — aligning under the "Name" / "Status" header columns.
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
        let status_cell = commands
            .spawn((
                Node {
                    width: Val::Px(STATUS_COL_WIDTH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row_entity),
            ))
            .id();
        let presence = commands
            .spawn((
                Text::new(String::new()),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(OFFLINE_COLOR),
                Pickable::IGNORE,
                ChildOf(status_cell),
            ))
            .id();
        // The two rights groups, in the same order as the header: "They can …"
        // (granted, editable checkboxes) then "You can …" (received, read-only).
        let [they_online, they_map, they_edit] =
            spawn_row_rights_group(&mut commands, row_entity, false, &ui.icons);
        let [you_online, you_map, you_edit] =
            spawn_row_rights_group(&mut commands, row_entity, true, &ui.icons);
        let rights = [
            they_online,
            they_map,
            they_edit,
            you_online,
            you_map,
            you_edit,
        ];
        commands
            .entity(row_entity)
            .insert((
                FriendRowParts {
                    presence,
                    label,
                    rights,
                },
                BoundFriend(None),
            ))
            .observe(on_friend_row_press);
    }
}

/// Spawn one row-side permission group (three checkbox cells) and return the three
/// checkbox [`ImageNode`] entities, in online / map / edit order.
///
/// A granted (`received == false`) cell is an editable checkbox with a toggle
/// observer; a received cell is a read-only indicator. The default image is the
/// empty checkbox — [`bind_friend_rows`] swaps in the ticked one and the tint.
fn spawn_row_rights_group(
    commands: &mut Commands,
    row_entity: Entity,
    received: bool,
    icons: &PeopleIcons,
) -> [Entity; 3] {
    let group = commands
        .spawn((
            Node {
                width: Val::Px(RIGHT_COL_WIDTH * 3.0),
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    let kinds = [RightKind::SeeOnline, RightKind::Map, RightKind::Edit];
    kinds.map(|kind| {
        let cell = commands
            .spawn((
                Node {
                    width: Val::Px(RIGHT_COL_WIDTH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(group),
            ))
            .id();
        let checkbox = commands
            .spawn((
                ImageNode {
                    color: RIGHT_UNSET_COLOR,
                    ..ImageNode::new(icons.checkbox(false))
                },
                Node {
                    width: Val::Px(ICON_DISPLAY),
                    height: Val::Px(ICON_DISPLAY),
                    ..default()
                },
                RightCell { kind },
                CellFriend(None),
                // Only granted rights are editable; a received cell is read-only.
                if received {
                    Pickable::IGNORE
                } else {
                    Pickable {
                        should_block_lower: true,
                        is_hoverable: true,
                    }
                },
                ChildOf(cell),
            ))
            .id();
        if !received {
            commands.entity(checkbox).observe(on_toggle_right);
        }
        checkbox
    })
}

/// A granted-rights checkbox was clicked: flip that right and send the update
/// (optimistically flipping the model too, so the box ticks immediately).
///
/// **Granting** the dangerous **edit-my-objects** right instead opens a confirm
/// modal ([`PendingGrantConfirm`]) — nothing is sent until it is confirmed.
/// Revoking edit-objects, and toggling see-online / see-on-map either way, apply
/// immediately.
fn on_toggle_right(
    press: On<Pointer<Press>>,
    cells: Query<(&RightCell, &CellFriend)>,
    mut model: ResMut<FriendsModel>,
    mut pending: ResMut<PendingGrantConfirm>,
    mut sl: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok((cell, friend)) = cells.get(press.entity) else {
        return;
    };
    let Some(friend) = friend.0 else {
        return;
    };
    let Some(current) = model.granted_rights(friend) else {
        return;
    };
    let updated = toggled_rights(current, cell.kind);
    // Granting edit-my-objects (the bit is going from off to on) needs a confirm;
    // everything else applies at once.
    if cell.kind == RightKind::Edit && !current.can_modify_objects() {
        pending.0 = Some(PendingGrant {
            friend,
            rights: updated,
        });
        return;
    }
    model.set_granted(friend, updated);
    sl.write(SlCommand(Command::GrantUserRights {
        target: friend,
        rights: updated,
    }));
}

/// Show / hide the grant-confirm modal from [`PendingGrantConfirm`], filling the
/// prompt with the pending friend's name.
fn drive_grant_confirm(
    pending: Res<PendingGrantConfirm>,
    ui: Option<Res<PeopleUi>>,
    model: Res<FriendsModel>,
    translator: Translator,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    set_display(&mut nodes, ui.confirm_overlay, pending.0.is_some());
    if let Some(grant) = &pending.0 {
        let agent = AgentKey::from(grant.friend);
        let name = model
            .name_of(agent)
            .map_or_else(|| short_id(agent.uuid()), ToOwned::to_owned);
        let prompt = translator.format(
            GRANT_CONFIRM_PROMPT_KEY,
            &TransArgs::new().text("name", &name),
        );
        if let Ok(mut text) = texts.get_mut(ui.confirm_text)
            && text.0 != prompt
        {
            text.0 = prompt;
        }
    }
}

/// Bind each pooled friend row to the [`FriendRow`] it now points at — on the
/// frame the view rebuilt, the selection changed, or this row's index changed.
fn bind_friend_rows(
    view: Res<FriendsView>,
    selected: Res<SelectedFriend>,
    ui: Option<Res<PeopleUi>>,
    mut rows: Query<(
        Entity,
        Ref<VirtualRow>,
        &ChildOf,
        &FriendRowParts,
        &mut BoundFriend,
    )>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut checkboxes: Query<(&mut ImageNode, &mut CellFriend)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh_all = view.is_changed() || selected.is_changed();
    for (row_entity, row, child_of, parts, mut bound) in &mut rows {
        if child_of.parent() != ui.friends_viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let Some(index) = row.index else {
            continue;
        };
        let Some(friend_row) = view.rows.get(index) else {
            continue;
        };
        bound.0 = Some(friend_row.friend);
        if let Ok((mut text, mut color)) = texts.get_mut(parts.presence) {
            set_text(&mut text, presence_glyph(friend_row.online));
            *color = TextColor(if friend_row.online {
                ONLINE_COLOR
            } else {
                OFFLINE_COLOR
            });
        }
        if let Ok((mut text, _color)) = texts.get_mut(parts.label) {
            set_text(&mut text, &friend_row.name);
        }
        // The six permission checkboxes — the ticked / empty icon and the tint by
        // set-ness and direction, plus the friend the (editable) cell now acts on.
        for (checkbox, column) in parts.rights.iter().zip(RIGHT_COLUMNS) {
            let (received, _kind) = column;
            let set = column_is_set(friend_row, column);
            if let Ok((mut image, mut cell_friend)) = checkboxes.get_mut(*checkbox) {
                let wanted = ui.icons.checkbox(set);
                if image.image != wanted {
                    image.image = wanted;
                }
                let tint = right_tint(set, received);
                if image.color != tint {
                    image.color = tint;
                }
                cell_friend.0 = Some(friend_row.friend);
            }
        }
        let is_selected = selected.0 == Some(friend_row.friend);
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

/// A friend row was clicked: focus the list (so the wheel scrolls it), select the
/// friend it presents, and — on a **double-click** (two presses on the same friend
/// within [`DOUBLE_CLICK_SECS`]) — open a one-to-one IM, exactly like the IM button.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected queries / resources: the picked row, the \
              viewport to focus, the click clock + tracker for double-click detection, the \
              selection to set, and the writer a double-click opens the IM through"
)]
fn on_friend_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&BoundFriend>,
    ui: Res<PeopleUi>,
    time: Res<Time>,
    mut tracker: ResMut<FriendClickTracker>,
    mut focus: ResMut<InputFocus>,
    mut selected: ResMut<SelectedFriend>,
    mut open: MessageWriter<OpenConversation>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.friends_viewport, FocusCause::Navigated);
    let Ok(bound) = rows.get(press.entity) else {
        return;
    };
    let Some(friend) = bound.0 else {
        return;
    };
    selected.0 = Some(friend);
    let now = time.elapsed_secs();
    if tracker.friend == Some(friend) && now - tracker.time <= DOUBLE_CLICK_SECS {
        // Second quick click on the same friend: open the one-to-one IM. Clear the
        // tracker so a third click does not re-fire.
        open.write(OpenConversation {
            key: ConversationKey::Direct(AgentKey::from(friend)),
        });
        tracker.friend = None;
    } else {
        tracker.friend = Some(friend);
        tracker.time = now;
    }
}

/// The presence-dot glyph for an online / offline friend.
const fn presence_glyph(online: bool) -> &'static str {
    if online { ONLINE_GLYPH } else { OFFLINE_GLYPH }
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
    use super::{Command, FriendAction, FriendsModel, MuteType, friend_command};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Friend, FriendKey, FriendPresence, FriendRights, Uuid};

    /// A friend record with the given id and default (no) rights.
    fn friend(id: u128) -> Friend {
        Friend {
            id: FriendKey::from(Uuid::from_u128(id)),
            rights_granted: FriendRights(0),
            rights_received: FriendRights(0),
        }
    }

    /// A login `FriendList` seeds the buddy cache, offline until presence says
    /// otherwise, and a later online notification flips just that friend.
    #[test]
    fn friend_list_then_presence() {
        let mut model = FriendsModel::default();
        model.note_friends(&[friend(1), friend(2)]);
        assert_eq!(model.friends.len(), 2);
        assert!(model.friends.values().all(|entry| !entry.online));
        model.set_online(&[FriendKey::from(Uuid::from_u128(1))], true);
        let rows = model.ordered();
        // Online sorts first.
        assert_eq!(rows.first().map(|row| row.online), Some(true));
        assert_eq!(rows.get(1).map(|row| row.online), Some(false));
    }

    /// A snapshot replaces the cache wholesale, carrying its own presence.
    #[test]
    fn snapshot_replaces_and_carries_presence() {
        let mut model = FriendsModel::default();
        model.note_friends(&[friend(1)]);
        model.apply_snapshot(&[FriendPresence {
            friend: friend(9),
            online: true,
        }]);
        assert_eq!(model.friends.len(), 1);
        assert!(
            model
                .friends
                .contains_key(&FriendKey::from(Uuid::from_u128(9)))
        );
        assert_eq!(model.ordered().first().map(|row| row.online), Some(true));
    }

    /// The six permission columns read the right rights field per direction:
    /// granted → the "They can …" triad, received → the "You can …" triad.
    #[test]
    fn rights_columns_map_both_directions() {
        let mut model = FriendsModel::default();
        model.note_friends(&[Friend {
            id: FriendKey::from(Uuid::from_u128(1)),
            // They see me online + on map (bits 0 and 1); not edit.
            rights_granted: FriendRights(0b011),
            // I can edit their objects (bit 2); not see-online / map.
            rights_received: FriendRights(0b100),
        }]);
        let columns = model.ordered().first().map(|row| {
            super::RIGHT_COLUMNS
                .iter()
                .map(|column| super::column_is_set(row, *column))
                .collect::<Vec<bool>>()
        });
        // [they_online, they_map, they_edit, you_online, you_map, you_edit]
        assert_eq!(columns, Some(vec![true, true, false, false, false, true]));
    }

    /// The default sort is online-first, then name ascending; clicking Name makes
    /// it primary (asc), clicking again flips to descending — earlier keys become
    /// tie-breakers.
    #[test]
    fn sort_click_promotes_and_toggles() {
        let mut sort = super::SortState::default();
        // Default primary is Online, descending (online-first).
        let primary = sort.primary();
        assert_eq!(
            primary.map(|key| (key.column, key.ascending)),
            Some((super::SortColumn::Online, false))
        );
        // Click Name → Name primary ascending.
        sort.click(super::SortColumn::Name);
        assert_eq!(
            sort.primary().map(|key| (key.column, key.ascending)),
            Some((super::SortColumn::Name, true))
        );
        // Click Name again → same column, direction flips to descending.
        sort.click(super::SortColumn::Name);
        assert_eq!(
            sort.primary().map(|key| (key.column, key.ascending)),
            Some((super::SortColumn::Name, false))
        );
    }

    /// The sort round-trips through its persisted string form.
    #[test]
    fn sort_encode_parse_round_trip() {
        let mut sort = super::SortState::default();
        sort.click(super::SortColumn::Name);
        sort.click(super::SortColumn::Right(false, super::RightKind::Edit));
        let encoded = sort.encode();
        let parsed = super::SortState::parse(&encoded);
        assert_eq!(parsed.encode(), encoded);
        // A wholly unknown string falls back to the default order.
        assert_eq!(
            super::SortState::parse("garbage").encode(),
            super::SortState::default().encode()
        );
    }

    /// Toggling a right flips just that bit; toggling twice restores it.
    #[test]
    fn toggling_a_right_flips_one_bit() {
        let none = FriendRights(0);
        let online = super::toggled_rights(none, super::RightKind::SeeOnline);
        assert!(online.can_see_online());
        assert!(!online.can_see_on_map());
        // Toggle a second right on, then the first back off.
        let both = super::toggled_rights(online, super::RightKind::Edit);
        assert!(both.can_see_online() && both.can_modify_objects());
        let off = super::toggled_rights(both, super::RightKind::SeeOnline);
        assert!(!off.can_see_online() && off.can_modify_objects());
    }

    /// Termination drops a friend; an unknown id is a no-op.
    #[test]
    fn termination_removes() {
        let mut model = FriendsModel::default();
        model.note_friends(&[friend(1), friend(2)]);
        model.remove(FriendKey::from(Uuid::from_u128(1)));
        assert_eq!(model.friends.len(), 1);
        model.remove(FriendKey::from(Uuid::from_u128(42)));
        assert_eq!(model.friends.len(), 1);
    }

    /// Names resolve the row label and drop out of the unnamed request set; an
    /// unresolved friend shows a short-id placeholder.
    #[test]
    fn names_resolve_labels_and_unnamed() {
        let mut model = FriendsModel::default();
        model.note_friends(&[friend(0x1234_5678_9abc)]);
        assert_eq!(model.unnamed().len(), 1);
        let placeholder = model
            .ordered()
            .first()
            .map(|row| row.name.clone())
            .unwrap_or_default();
        // The placeholder is the first eight hex digits of the id.
        assert_eq!(placeholder.len(), 8);
        let agent =
            sl_client_bevy::AgentKey::from(FriendKey::from(Uuid::from_u128(0x1234_5678_9abc)));
        model.note_name(agent, "Avatar One");
        assert!(model.unnamed().is_empty());
        assert_eq!(
            model.ordered().first().map(|row| row.name.clone()),
            Some("Avatar One".to_owned())
        );
    }

    /// Rows sort online-first, then case-folded by name.
    #[test]
    fn ordering_is_online_then_name() {
        let mut model = FriendsModel::default();
        model.note_friends(&[friend(1), friend(2), friend(3)]);
        let a1 = sl_client_bevy::AgentKey::from(FriendKey::from(Uuid::from_u128(1)));
        let a2 = sl_client_bevy::AgentKey::from(FriendKey::from(Uuid::from_u128(2)));
        let a3 = sl_client_bevy::AgentKey::from(FriendKey::from(Uuid::from_u128(3)));
        model.note_name(a1, "zoe");
        model.note_name(a2, "Bob");
        model.note_name(a3, "amy");
        model.set_online(&[FriendKey::from(Uuid::from_u128(1))], true);
        let names: Vec<String> = model.ordered().into_iter().map(|row| row.name).collect();
        // zoe online first; then amy, Bob offline (case-folded ascending).
        assert_eq!(names, vec!["zoe", "amy", "Bob"]);
    }

    /// Each action maps to its command; IM produces none (it opens a tab), and
    /// Block carries the agent id + name + Agent type.
    #[test]
    fn action_command_mapping() {
        let friend = FriendKey::from(Uuid::from_u128(7));
        assert!(friend_command(FriendAction::Im, friend, "x").is_none());
        assert!(matches!(
            friend_command(FriendAction::OfferTeleport, friend, ""),
            Some(Command::OfferTeleport { .. })
        ));
        assert!(matches!(
            friend_command(FriendAction::RemoveFriend, friend, ""),
            Some(Command::TerminateFriendship(_))
        ));
        assert!(matches!(
            friend_command(FriendAction::Block, friend, "Avatar One"),
            Some(Command::Mute {
                mute_type: MuteType::Agent,
                ..
            })
        ));
    }
}
