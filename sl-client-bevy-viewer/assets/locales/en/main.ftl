# The viewer's English (base) string bundle — Project Fluent, loaded through
# `bevy_fluent` by `src/i18n.rs`. Every UI-bearing panel looks its strings up
# here by key rather than embedding an English literal, so a translator can
# ship another locale without the panel changing.
#
# This is the i18n *scaffold*: it carries only the handful of strings the
# scaffold itself needs plus the demonstrations the `F6` panel drives. Panels
# add their own keys as they land.

## Typographic conventions — punctuation the UI inserts itself, which is a
## translator's call, not a hardcoded literal (see the task file).

# The truncation ellipsis the tab widget appends to a clipped label. Latin
# convention is a single horizontal ellipsis; CJK locales override it with a
# centred six-dot form (see `ja`).
ui-ellipsis = …

## The `F6` internationalisation demo (`src/i18n.rs`).

# The demo panel's own title.
i18n-demo-title = Internationalisation

# This locale's endonym, shown by the locale switcher so each language names
# itself in its own script.
language-name = English

# A string argument: a name is inserted verbatim, never translated. Fluent wraps
# the inserted run in bidi isolation marks so a right-to-left name stays intact
# inside a left-to-right sentence.
greeting = Hello, { $name }!

# A number argument feeding a plural selector. English has two plural categories
# (`one` / `other`); Fluent chooses the branch from this locale's CLDR rules, so
# the same authoring is correct in a language with more categories (see `pl`,
# `ar`) — unlike the reference viewer's hardcoded three-language if-ladder.
items-selected =
    { $count ->
        [one] { $count } item selected
       *[other] { $count } items selected
    }

# A gender selector driven by a typed string argument.
friend-status =
    { $gender ->
        [male] He is online
        [female] She is online
       *[other] They are online
    }

## The inventory window (viewer-inventory-*).

inventory-title = Inventory
inventory-tab-everything = Everything
inventory-tab-recent = Recent
inventory-tab-worn = Worn
inventory-expand-all = Expand all
inventory-collapse-all = Collapse all

## The Conversations floater (viewer-social-im-conversations) — nearby chat, 1:1
## IMs, group chats and conferences as vertical tabs.

# The floater's title bar.
conversations-title = Conversations
# The always-present first tab: local (nearby) chat.
conversations-nearby = Nearby Chat
# The transcript speaker label for our own outbound lines.
conversations-you = You
# The "someone is typing" status line under a transcript.
conversations-typing-one = { $name } is typing…
conversations-typing-many = Several people are typing…
# The pending-invite bar shown until a group / conference invite is accepted.
conversations-invite-prompt = You're invited to this conversation.
conversations-invite-accept = Accept
conversations-invite-decline = Decline

## The People / Contacts surface (viewer-social-people-panel), hosted as a pinned
## tab inside the Conversations floater: the Friends list plus a Groups
## placeholder.

# The pinned People tab in the conversations strip.
people-tab = People
# The Friends / Groups sub-tabs inside the People pane.
people-friends-tab = Friends
people-groups-tab = Groups
# The friends-table column headers (always shown, even for an empty list).
people-header-name = Name
people-header-status = Status
# The two permission-column groups: rights this agent grants the friend
# ("They can …") and rights the friend grants this agent ("You can …"). Each group
# has three generated icon columns (see online status, find on map, edit objects).
people-rights-they = They
people-rights-you = You
# The per-friend action buttons under the Friends list.
people-action-im = IM
people-action-teleport = Offer Teleport
people-action-remove = Remove Friend
people-action-block = Block
# The confirm dialog shown before granting a friend the edit-my-objects right
# (the one dangerous grant); revokes and the other rights apply without a prompt.
people-grant-confirm-prompt = Give { $name } permission to edit, delete or take your objects?
people-grant-confirm-yes = Grant
people-grant-confirm-no = Cancel

## The Groups list (viewer-social-groups), hosted in the Groups sub-tab of the
## People pane inside the Conversations floater — the member's own groups, laid
## out like the Friends list.

# The groups-table "Name" column header.
groups-header-name = Name
# The groups-table "Active" column header (the currently-worn group title).
groups-header-active = Active
# The group-count line under the list ({ $count } is the number of groups).
groups-count =
    { $count ->
        [one] { $count } group
       *[other] { $count } groups
    }
# The per-group action buttons beside the list.
groups-action-info = Info
groups-action-im = IM
groups-action-activate = Activate
groups-action-leave = Leave
# The confirm dialog shown before leaving a group ({ $name } is the group name).
groups-leave-confirm-prompt = Leave the group "{ $name }"?
groups-leave-confirm-yes = Leave
groups-leave-confirm-no = Cancel

## The emoji-picker floater (viewer-emoji-picker-floater).

# The picker window's title bar.
emoji-picker-title = Emoji

## The status area (viewer-ui-status-bar) — the read-outs on the trailing edge
## of the top menu bar.

# Shown in the location read-out before the region is known (still logging in).
status-bar-connecting = Connecting…

# The L$ balance before the first reply from the grid.
status-bar-balance-unknown = L$ --

# The grid clock. The time is always Second Life Time (US Pacific), so the SLT
# marker is fixed; only its placement around the formatted time is a
# translator's call.
status-bar-time = { $time } SLT

# The frame rate read-out.
status-bar-fps = { $fps } fps

# The parcel permission icons carry no text (they are tinted glyph images), so
# there are no string keys for them here.

## The bottom toolbar (viewer-ui-bottom-toolbar) — the persistent strip of
## toggle buttons that open the main floaters. Only Inventory is wired today; the
## rest are disabled placeholders until their own floater tasks land.

bottom-toolbar-chat = Chat
bottom-toolbar-inventory = Inventory
bottom-toolbar-appearance = Appearance
bottom-toolbar-map = Map
bottom-toolbar-minimap = Mini-map
bottom-toolbar-people = People
# The chat *window* toggle (distinct from the always-visible nearby-chat input
# bar that will sit above the button row).
bottom-toolbar-conversations = Conversations
bottom-toolbar-camera = Camera
## The inventory filters floater (viewer-inventory-advanced-filters).

inventory-filters-title = Inventory Filters
inventory-filter-animations = Animations
inventory-filter-calling-cards = Calling cards
inventory-filter-clothing = Clothing
inventory-filter-gestures = Gestures
inventory-filter-landmarks = Landmarks
inventory-filter-materials = Materials
inventory-filter-notecards = Notecards
inventory-filter-objects = Objects
inventory-filter-scripts = Scripts
inventory-filter-sounds = Sounds
inventory-filter-textures = Textures
inventory-filter-snapshots = Snapshots
inventory-filter-settings = Settings
inventory-filter-all = All
inventory-filter-none = None
inventory-filter-worn = Worn only
inventory-filter-since-login = Since login
inventory-filter-newer-than = Newer than
inventory-filter-older-than = Older than
inventory-filter-hours-label = Hours
inventory-filter-days-label = Days
inventory-filter-reset = Reset

## The avatar picker floater (viewer-inventory-share-picker).

avatar-picker-title = Choose Resident
avatar-picker-tab-search = Search
avatar-picker-tab-friends = Friends
avatar-picker-tab-near-me = Near me
avatar-picker-go = Go
avatar-picker-ok = OK
avatar-picker-cancel = Cancel
## The item properties floater + Open previews
## (viewer-inventory-open-and-properties).

item-properties-title = Item Properties
item-properties-name = Name:
item-properties-description = Description:
item-properties-creator = Creator:
item-properties-owner = Owner:
item-properties-acquired = Acquired:
item-properties-you-can = You can:
item-properties-modify = Modify
item-properties-copy = Copy
item-properties-transfer = Transfer
item-properties-group = Group:
item-properties-share = Share
item-properties-anyone = Anyone:
item-properties-next-owner = Next owner:
item-properties-for-sale = For sale
item-properties-sale-original = Original
item-properties-sale-copy = Copy
item-properties-sale-contents = Contents
landmark-teleport = Teleport
animation-play-inworld = Play in world
animation-stop = Stop

## The inventory gallery (viewer-inventory-gallery).

inventory-gallery-title = Inventory Gallery
