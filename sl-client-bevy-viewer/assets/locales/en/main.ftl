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

## The group profile floater (viewer-social-group-profile) — reached from the
## Groups list's Info button: General / Members & Roles / Notices.

# The floater title and its three tabs.
group-profile-title = Group Profile
group-profile-tab-general = General
group-profile-tab-members = Members & Roles
group-profile-tab-notices = Notices

# A "waiting for the reply" placeholder.
group-profile-loading = (loading)
# General-tab identity facts.
group-profile-name = Name:
group-profile-key = Key:
group-profile-founder = Founder:
group-profile-members-roles = Members / Roles:
group-profile-join-fee = Join fee:
group-profile-charter = Charter:
group-profile-no-insignia = (no insignia)
# General-tab editable identity flags.
group-profile-open-enrollment = Open enrollment
group-profile-mature = Mature content
group-profile-show-in-list = Show in search
group-profile-save = Save
# The agent's own membership controls.
group-profile-my-membership = My membership
group-profile-receive-notices = Receive notices
group-profile-list-in-profile = List this group in my profile
group-profile-active-title = Active title:
group-profile-join = Join
group-profile-invite-only = This group is invitation-only.
group-profile-copy-slurl = Copy SLURL

# The members table column headers.
group-members-name = Name
group-members-title = Title
group-members-contribution = Land
group-members-status = Status
# The members count line ({ $loaded } shown of the group's { $total }).
group-members-count = { $loaded } of { $total } loaded
# Re-fetch the roster (the first SL fetch is just officers / owners).
group-members-refresh = Refresh

# The roles column.
group-roles-header = Roles
# The roles table column headers.
group-roles-col-name = Name
group-roles-col-title = Title
group-roles-col-members = Members
group-role-new = New Role…
# The selected-member / selected-role details area.
group-details-hint = Select a member or role for details.
group-details-close = Close
group-details-member = Member:
group-details-roles-of-member = Assigned roles
group-member-eject = Eject…
# The selected-role editors.
group-role-name-label = Name:
group-role-title-label = Title:
group-role-desc-label = Description:
group-role-abilities = Abilities
group-role-save = Save Role
group-role-save-powers = Save Abilities
group-role-delete = Delete Role…

# The named group abilities (roles_constants.h GP_*).
group-power-member-invite = Invite members to this group
group-power-member-eject = Eject members from this group
group-power-member-options = Change open enrollment and the join fee
group-power-role-create = Create new roles
group-power-role-delete = Delete roles
group-power-role-properties = Change a role's name, title and description
group-power-role-assign-limited = Assign members to assigner's roles
group-power-role-assign = Assign members to any role
group-power-role-remove = Remove members from roles
group-power-role-change-actions = Change a role's abilities
group-power-change-identity = Change the group's identity
group-power-land-deed = Deed and buy land for the group
group-power-notices-send = Send group notices
group-power-notices-receive = Receive group notices

# The Notices tab.
# The notices table column headers.
group-notices-subject = Subject
group-notices-from = From
group-notices-date = Date
group-notice-hint = Select a notice to read it.
group-notice-subject = Subject:
group-notice-has-attachment = This notice has an attachment.
group-notice-compose = Send a notice
group-notice-send = Send Notice

## The About Land floater (viewer-parcel-options-general +
## viewer-parcel-options-access-media) — the parcel info surface, all nine
## reference tabs.

# The floater title and its nine tabs.
about-land-title = About Land
about-land-tab-general = General
about-land-tab-covenant = Covenant
about-land-tab-objects = Objects
about-land-tab-options = Options
about-land-tab-media = Media
about-land-tab-sound = Sound
about-land-tab-access = Access
about-land-tab-experiences = Experiences
about-land-tab-environment = Environment

# Shared placeholders and controls.
about-land-loading = (loading)
about-land-none = (none)
about-land-no-parcel = No parcel selected.
about-land-apply = Apply
about-land-add = Add…
about-land-remove = Remove
about-land-yes = Yes
about-land-no = No
about-land-always = Always

# General-tab labels.
about-land-name = Name:
about-land-parcel-id = Parcel ID:
about-land-description = Description:
about-land-type = Type:
about-land-rating = Rating:
about-land-owner = Owner:
about-land-group = Group:
about-land-area = Area:
about-land-claimed = Claimed:
about-land-traffic = Traffic:
about-land-for-sale = For Sale:
about-land-save = Save
about-land-group-owned = (Group Owned)
about-land-not-for-sale = Not for sale
# The sale-price line: { $price } L$ total, { $persqm } L$ per square metre.
about-land-sale-price = L$ { $price } (L$ { $persqm }/m²)

# Land-type (region product) values.
about-land-product-full = Estate / Full Region
about-land-product-homestead = Homestead
about-land-product-openspace = Openspace
about-land-product-unknown = Unknown

# Content-rating (maturity) values.
about-land-rating-pg = General
about-land-rating-mature = Moderate
about-land-rating-adult = Adult
about-land-rating-unknown = Unknown

# Covenant-tab labels and clauses.
about-land-estate = Estate:
about-land-estate-owner = Estate owner:
about-land-last-modified = Last modified:
about-land-region = Region:
about-land-region-type = Type:
about-land-region-rating = Rating:
about-land-resale = Resale:
about-land-subdivide = Subdivide:
about-land-covenant-none = There is no Covenant provided for this Estate.
about-land-covenant-loading = (loading covenant…)
about-land-resale-allowed = Land in this region may be resold.
about-land-resale-blocked = Land in this region may not be resold.
about-land-subdivide-allowed = Land in this region may be joined or subdivided.
about-land-subdivide-blocked = Land in this region may not be joined or subdivided.

# Objects-tab labels.
about-land-region-capacity = Region capacity:
about-land-parcel-capacity = Parcel capacity:
about-land-parcel-impact = Parcel land impact:
about-land-owner-objects = Owned by parcel owner:
about-land-group-objects = Set to group:
about-land-other-objects = Owned by others:
about-land-selected-objects = Selected / sat upon:
about-land-autoreturn = Auto-return (minutes):
about-land-object-owners = Object Owners:
about-land-refresh = Refresh
about-land-owners-empty = No objects on this parcel.
about-land-owner-agent = Resident
about-land-owner-group = Group
# One object-owner row's object count.
about-land-owner-count = { $count } objects
# The object-owners table column headers.
about-land-owners-type = Type
about-land-owners-name = Name
about-land-owners-count = Count
# The allow / ban list table column headers.
about-land-access-name = Name
about-land-access-expiry = Expires

# Options-tab section headers and checkboxes.
about-land-options-allow = Allow other Residents to:
about-land-options-land = Land options:
about-land-opt-terraform = Edit terrain
about-land-opt-fly = Fly
about-land-opt-build = Build objects (everyone)
about-land-opt-build-group = Build objects (group)
about-land-opt-entry = Object entry (everyone)
about-land-opt-entry-group = Object entry (group)
about-land-opt-scripts = Run scripts (everyone)
about-land-opt-scripts-group = Run scripts (group)
about-land-opt-safe = Safe (no damage)
about-land-opt-no-push = No pushing
about-land-opt-search = Show place in search
about-land-opt-mature = Moderate content
about-land-category = Category:
about-land-snapshot = Snapshot:
about-land-landing-point = Landing point:
about-land-landing-set = Set here
about-land-landing-clear = Clear
about-land-teleport-routing = Teleport routing:
# Search categories (ParcelCategory 0–7).
about-land-cat-none = Any category
about-land-cat-linden = Linden location
about-land-cat-residential = Residential
about-land-cat-commercial = Commercial
about-land-cat-industrial = Industrial
about-land-cat-park = Parks & Nature
about-land-cat-other = Other
about-land-cat-adult = Adult
# Teleport routing (LandingType 0/1/2).
about-land-routing-blocked = Blocked
about-land-routing-landing = Landing point
about-land-routing-anywhere = Anywhere

# Media-tab controls.
about-land-media-url = Media URL:
about-land-media-texture = Replace texture:
about-land-media-autoscale = Auto-scale content
about-land-media-type = Media type:
about-land-media-size = Media size:
about-land-media-loop = Media loops
about-land-media-auto-size = Auto

# Sound-tab controls.
about-land-music-url = Music URL:
about-land-sound-local = Restrict gesture and object sounds to this parcel
about-land-voice-enable = Enable voice
about-land-voice-local = Restrict voice to this parcel
about-land-av-sounds = Avatar sounds (everyone)
about-land-av-sounds-group = Avatar sounds (group)

# Access-tab controls and lists.
about-land-access-public = Allow public access
about-land-access-payment = Must have payment info on file
about-land-access-age = Must be age-verified
about-land-access-group = Allow group access
about-land-access-passes = Sell passes
about-land-pass-price = Pass price (L$):
about-land-pass-hours = Pass hours:
about-land-allowed = Allowed Residents
about-land-banned = Banned Residents

# Experiences tab (no per-parcel experience protocol yet).
about-land-experiences-unavailable = Per-parcel experience lists are not available yet.

# Environment-tab summary.
about-land-env-override = Parcel overrides allowed:
about-land-env-version = Parcel environment version:
about-land-env-day-cycle = Active day cycle:
about-land-env-edit-note = Per-parcel environment editing is a separate feature; this is the current environment.

## The Region / Estate ("About Region") floater (viewer-region-options-*).

# The floater's title bar.
about-region-title = Region / Estate
# Tab labels.
about-region-tab-region = Region
about-region-tab-debug = Debug
about-region-tab-terrain = Terrain
about-region-tab-estate = Estate
about-region-tab-covenant = Covenant
about-region-tab-access = Access
about-region-tab-environment = Environment
about-region-tab-experiences = Experiences

# Shared placeholders.
about-region-loading = (loading)
about-region-none = (none)
about-region-apply = Apply
about-region-remove = Remove

# Region tab: identity read-outs.
about-region-region = Region:
about-region-type = Type:
about-region-owner = Owner:
about-region-grid-position = Grid position:
about-region-maturity = Rating:
about-region-agent-limit = Agent limit:
about-region-object-bonus = Object bonus:

# Region tab: editable flags.
about-region-block-terraform = Block terraform
about-region-block-fly = Block fly
about-region-allow-damage = Allow damage
about-region-restrict-push = Restrict pushing
about-region-allow-resell = Allow land resell
about-region-allow-join-divide = Allow land join / divide

# Region tab: estate-manager actions.
about-region-teleport-home-one = Teleport Home One Resident…
about-region-teleport-home-all = Teleport Home All Residents…

# Debug tab.
about-region-disable-scripts = Disable scripts
about-region-disable-collisions = Disable collisions
about-region-disable-physics = Disable physics
about-region-restart-delay = Restart in (seconds):
about-region-restart = Restart Region
about-region-cancel-restart = Cancel Restart

# Terrain tab.
about-region-water-height = Water height:
about-region-terrain-raise = Terrain raise limit:
about-region-terrain-lower = Terrain lower limit:
about-region-terrain-textures = Terrain textures
about-region-terrain-tex-1 = 1 (low):
about-region-terrain-tex-2 = 2:
about-region-terrain-tex-3 = 3:
about-region-terrain-tex-4 = 4 (high):
about-region-terrain-elevation = Elevation ranges (low = start, high = range)
about-region-corner-sw-low = SW low
about-region-corner-sw-high = high
about-region-corner-se-low = SE low
about-region-corner-se-high = high
about-region-corner-nw-low = NW low
about-region-corner-nw-high = high
about-region-corner-ne-low = NE low
about-region-corner-ne-high = high

# Estate tab.
about-region-estate = Estate:
about-region-estate-owner = Estate owner:
about-region-abuse-email = Abuse email:
about-region-estate-note = Changes here affect every region in the estate.
about-region-estate-public = Anyone can visit (public access)
about-region-estate-direct-tp = Allow direct teleport
about-region-estate-payment = Must have payment info on file
about-region-estate-age = Must be age-verified
about-region-estate-bots = Deny scripted agents (bots)
about-region-estate-voice = Allow voice chat
about-region-estate-override = Parcel owners may restrict access further
about-region-apply-estate = Apply Estate Settings
about-region-estate-message = Message to estate:
about-region-send-estate-message = Send Message To Estate
about-region-kick-estate = Kick Resident From Estate…

# Covenant tab (read-only).
about-region-last-modified = Last modified:
about-region-resale = Resale:
about-region-subdivide = Subdivide:
about-region-covenant-none = There is no Covenant provided for this Estate.
about-region-covenant-loading = (loading covenant…)
about-region-resale-allowed = Land in this region may be resold.
about-region-resale-blocked = Land in this region may not be resold.
about-region-subdivide-allowed = Land in this region may be joined or subdivided.
about-region-subdivide-blocked = Land in this region may not be joined or subdivided.

# Access tab.
about-region-managers = Estate managers
about-region-allowed = Allowed residents
about-region-allowed-groups = Allowed groups
about-region-banned = Banned residents
about-region-add-manager = Add Manager…
about-region-add-allowed = Add Resident…
about-region-add-banned = Ban Resident…
about-region-allowed-groups-note = Adding an allowed group needs a group picker (a separate feature); existing groups can be removed here.
about-region-access-name = Name
about-region-access-remove = Remove

# Product-type labels.
about-region-product-full = Estate / Full Region
about-region-product-homestead = Homestead
about-region-product-openspace = Openspace
about-region-product-unknown = Unknown

# Maturity-rating labels.
about-region-rating-pg = General
about-region-rating-mature = Moderate
about-region-rating-adult = Adult
about-region-rating-unknown = Unknown

# Environment / Experiences placeholder tabs.
about-region-env-unimplemented = Region environment editing is not implemented yet.
about-region-experiences-unimplemented = Region experiences are not implemented yet.

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
# The snapshot (photo) floater toggle — the reference's toolbar "Snapshot"
# command (singular).
bottom-toolbar-snapshot = Snapshot

## The volume panel (viewer-volume-panel): the master control in the bottom bar
## and the per-category pulldown behind its ▲ button. Labels name the mixer buses.
volume-panel-title = Volume
volume-panel-master = Master
volume-panel-sfx = Sounds
volume-panel-ambient = Ambient
volume-panel-ui = UI
volume-panel-music = Music
volume-panel-media = Media
volume-panel-voice = Voice

## The Stand Up / Stop flycam state button (viewer-sit-target-and-stand-button) —
## the reference's combined stand / stop-flying panel, in the toolbar's reserved
## leading slot. Only one is ever shown: Stand while seated, Stop flycam in flycam.

stand-button-stand = Stand Up
stand-button-stop-flycam = Stop flycam
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

## The notecard viewer & editor floater (viewer-notecard-editor).

notecard-save = Save
notecard-view-preview = View items
notecard-view-edit = Edit text
notecard-readonly-note = You do not have permission to modify this notecard.
notecard-status-loading = Loading…
notecard-status-saving = Saving…
notecard-status-saved = Saved.
notecard-status-save-failed = Save failed.
notecard-status-decode-failed = This notecard could not be read.

## The LSL script editor floater (viewer-lsl-editor-save-compile).

script-save = Save & Compile
script-readonly-note = You do not have permission to modify this script.
script-running = Running
script-status-loading = Loading…
script-status-saving = Saving & compiling…
script-status-saved = Saved.
script-status-compile-failed = Compilation failed.
script-status-save-failed = Save failed.
script-error-at = Line { $line }, column { $column }: { $message }
script-error-nopos = { $message }

## The inventory gallery (viewer-inventory-gallery).

inventory-gallery-title = Inventory Gallery

## The avatar profile floater (viewer-social-profiles). Labels follow the
## reference's legacy in-viewer profile (the Vintage skin's panel_profile_*).

profile-title = Profile
profile-tab-second-life = 2nd Life
profile-tab-web = Web
profile-tab-picks = Picks
profile-tab-classifieds = Classifieds
profile-tab-first-life = 1st Life
profile-tab-notes = Notes
profile-name = Name:
profile-key = Key:
profile-online = Online
profile-offline = Offline
profile-birthdate = Birthdate:
profile-account = Account:
profile-account-resident = Resident
profile-account-trial = Trial
profile-account-charter = Charter Member
profile-account-employee = Linden Lab Employee
profile-payment-on-file = Payment Info On File
profile-payment-used = Payment Info Used
profile-payment-none = No Payment Info On File
profile-partner = Partner:
profile-partner-none = None
profile-groups = Groups:
profile-groups-none = None
profile-about = About:
profile-show-in-search = Show in search
profile-save = Save
profile-discard = Discard
profile-im = Instant Message
profile-offer-teleport = Offer Teleport
profile-add-friend = Add Friend
profile-remove-friend = Remove Friend
profile-block = Block
profile-copy-slurl = Copy SLURL
profile-find-on-map = Find on Map
profile-invite-to-group = Invite to Group
profile-pay = Pay
# The label leading the pay amount field (the currency sign).
profile-pay-amount = L$
profile-web-url = URL:
profile-web-none = (no profile URL)
profile-web-loading = Loading…
profile-web-loaded = Page loaded in { $seconds } s
profile-first-life-about = About:
profile-notes-hint = Make notes about this person here. No one else can see your notes.
profile-loading = (loading)
# Shown for a pick / classified location that moves to the agent's current
# parcel on the next save.
profile-location-pending = (will update after save)
profile-picks-header = Tell everyone about your favorite places.
profile-picks-none = No Picks
profile-pick-new = New…
profile-pick-delete = Delete…
profile-pick-name = Name:
profile-pick-desc = Description:
profile-pick-location = Location:
profile-pick-teleport = Teleport
profile-pick-show-on-map = Show on Map
profile-pick-set-location = Set Location
profile-pick-save = Save Pick
profile-classifieds-none = No Classifieds
profile-classified-new = New…
profile-classified-delete = Delete…
profile-classified-name = Title:
profile-classified-desc = Description:
profile-classified-location = Location:
profile-classified-category = Category:
profile-classified-content-type = Content Type:
profile-classified-general = General Content
profile-classified-moderate = Moderate Content
profile-classified-auto-renew = Auto renew each week
profile-classified-price = Price for listing:
profile-classified-creation-date = Creation date:
profile-classified-teleport = Teleport
profile-classified-map = Map
profile-classified-set-location = Set to Current Location
profile-classified-save = Save
profile-classified-publish = Publish
profile-classified-cancel = Cancel
profile-category-any = Any Category
profile-category-shopping = Shopping
profile-category-land-rental = Land Rental
profile-category-property-rental = Property Rental
profile-category-special-attraction = Special Attraction
profile-category-new-products = New Products
profile-category-employment = Employment
profile-category-wanted = Wanted
profile-category-service = Service
profile-category-personal = Personal
# The People list's per-friend Profile action button.
people-action-profile = Profile
# The Share area: the whole profile floater accepts inventory drops.
profile-share = Share:
profile-share-hint = Drop inventory items here to give them to this person.
# An unset profile / pick / classified image box.
profile-image-none = (no image)

## The in-viewer web browser floater (web_floater.rs).

web-floater-title = Web Browser

## The minimap floater (minimap.rs).

minimap-floater-title = Mini-map
# Compass labels around the map edge.
minimap-compass-north = N
minimap-compass-north-east = NE
minimap-compass-east = E
minimap-compass-south-east = SE
minimap-compass-south = S
minimap-compass-south-west = SW
minimap-compass-west = W
minimap-compass-north-west = NW
# Hover tooltip: an avatar's name and distance in metres.
minimap-tooltip-avatar = { $name } ({ $distance } m)
# Hover tooltip: an avatar whose altitude is unknown (beyond draw distance).
minimap-tooltip-avatar-far = { $name } (> { $distance } m)
minimap-tooltip-region = Region: { $name }
minimap-tooltip-parcel = Parcel: { $name }
minimap-tooltip-owner = Owner: { $name }
# A for-sale parcel's price and area.
minimap-tooltip-sale = For sale: L$ { $price } ({ $area } m²)
minimap-tooltip-hint-teleport = Double-click to teleport
minimap-tooltip-hint-map = Double-click to open the world map

## The world-map floater (world_map.rs).

worldmap-floater-title = World Map
worldmap-tooltip-region = Region: { $name }
# The region's agent count from the map data.
worldmap-tooltip-region-agents = { $count } avatars
worldmap-maturity-general = Rating: General
worldmap-maturity-moderate = Rating: Moderate
worldmap-maturity-adult = Rating: Adult
# An avatar-locations marker's count.
worldmap-tooltip-agents = { $count } avatars here
worldmap-tooltip-telehub = Telehub: { $name }
worldmap-tooltip-infohub = Infohub: { $name }
# A land-for-sale marker's parcel name, price and area.
worldmap-tooltip-land-sale = For sale: { $name } — L$ { $price } ({ $area } m²)
worldmap-tooltip-event = Event: { $name }
worldmap-location-none = Click the map to select a location
worldmap-button-teleport = Teleport
worldmap-button-copy-slurl = Copy SLURL
worldmap-layer-people = People
worldmap-layer-infohubs = Telehubs
worldmap-layer-land-sale = Land for Sale
worldmap-layer-events = Events
worldmap-layer-mature-events = Moderate Events
worldmap-layer-adult-events = Adult Events
worldmap-layer-region-names = Region Names

# The Search floater (viewer-search-floater): the Firestorm fsfloatersearch
# reproduction — a tab strip of result tables plus a shared details pane.
search-title = Search
search-query-label = Search:
search-button = Search
search-maturity-label = Show:
search-maturity-general = General
search-maturity-moderate = Moderate
search-maturity-adult = Adult
search-online-only = Online only
search-tab-web = Web
search-tab-people = People
search-tab-groups = Groups
search-tab-events = Events
search-tab-places = Places
search-tab-land = Land
search-tab-classifieds = Classifieds
search-label-category = Category:
search-label-saletype = For sale:
search-label-sort = Sort by:
search-prev = ‹ Prev
search-next = Next ›
# The result-table column headers.
search-col-name = Name
search-col-members = Members
search-col-traffic = Traffic
search-col-price = Price
search-col-area = Area
search-col-ppm = L$/m²
search-col-type = Type
search-col-date = Date
search-land-ascending = Ascending
search-label-price-max = Max price:
search-label-area-min = Min area:
# The Events tab date-mode radio.
search-events-current = Upcoming
search-events-bydate = By date
# The shared details pane's action buttons.
search-detail-profile = Open Profile
search-detail-message = Send Message
search-detail-friend = Add Friend
search-detail-chat = Join Chat
search-detail-join = Join Group
search-detail-teleport = Teleport
search-detail-map = Show on Map
search-detail-remind = Remind me

# Build tools (the object edit floater, viewer-object-edit-floater-shell).
build-tools-floater-title = Build Tools
build-tool-move = Move
build-tool-rotate = Rotate
build-tool-stretch = Stretch
build-toggle-snap = Snap to grid
build-toggle-local-frame = Local axes
build-toggle-edit-linked = Edit linked parts
build-toggle-stretch-both = Stretch both sides
build-grid-unit-label = Grid unit (m)
build-position-label = Position
build-rotation-label = Rotation
build-size-label = Size
build-tab-general = General
build-tab-object = Object
build-tab-features = Features
build-tab-texture = Texture
build-tab-content = Content
build-tab-placeholder = Not implemented yet

# Content tab + Object Contents floater (viewer-prim-inventory-editing).
build-content-no-target = No object selected
build-content-loading = Loading contents…
build-content-count = { $count ->
    [one] { $count } item
   *[other] { $count } items
}
build-content-new-script = New Script
build-content-new-script-name = New Script
build-content-rename = Rename
build-content-remove = Remove
build-content-refresh = Refresh
build-content-no-modify = You do not have permission to modify this object's contents.
build-content-item-no-modify = That item is not modifiable.
build-content-state-adding = …adding
build-content-state-deleting = …deleting
build-content-state-refreshing = …refreshing
object-contents-floater-title = Object Contents
object-contents-none = Nothing to show
object-contents-copy = Copy To Inventory
object-contents-copy-wear = Copy And Wear
object-contents-no-folder = Your inventory is not ready yet — try again in a moment.
object-contents-wear-note = Copied to your inventory; wear them from there.
build-selection-none = Nothing selected
build-selection-count = { $count ->
    [one] { $count } object selected
   *[other] { $count } objects selected
}
build-selection-prims = { $count ->
    [one] { $count } prim
   *[other] { $count } prims
}
build-selection-link = link { $number }
build-selection-no-modify = no modify
build-link-part-label = Linked part

# Build tools parameter tabs (viewer-prim-parameter-editing).
build-info-creator = Creator
build-info-owner = Owner
build-info-land-impact = Land Impact
build-info-you-can = You can
build-group-label = Group
build-group-none = (none)
build-deed = Deed
build-share-group = Share with group
build-next-owner-label = Next owner can
build-anyone-label = Anyone
build-perm-modify = Modify
build-perm-copy = Copy
build-perm-transfer = Transfer
build-perm-move = Move
build-object-name-label = Name
build-object-desc-label = Description
build-flag-physical = Physical
build-flag-temporary = Temporary
build-flag-phantom = Phantom
build-type-label = Type
build-type-box = Box
build-type-cylinder = Cylinder
build-type-prism = Prism
build-type-sphere = Sphere
build-type-torus = Torus
build-type-tube = Tube
build-type-ring = Ring
build-type-sculpt = Sculpted
build-type-mesh = Mesh
build-cut-label = Path Cut (B/E)
build-hollow-label = Hollow (%)
build-hole-default = Default
build-hole-circle = Circle
build-hole-square = Square
build-hole-triangle = Triangle
build-twist-label = Twist (B/E)
build-taper-label = Taper
build-hole-size-label = Hole Size
build-shear-label = Top Shear
build-adv-profile-cut-label = Profile Cut (B/E)
build-adv-dimple-label = Dimple (B/E)
build-adv-slice-label = Slice (B/E)
build-taper2-label = Taper Profile
build-radius-offset-label = Radius
build-revolutions-label = Revolutions
build-skew-label = Skew
build-material-label = Material
build-material-stone = Stone
build-material-metal = Metal
build-material-glass = Glass
build-material-wood = Wood
build-material-flesh = Flesh
build-material-plastic = Plastic
build-material-rubber = Rubber
build-material-light = Light (legacy)
build-feature-flexi = Flexible Path
build-flex-softness-label = Softness
build-flex-gravity-label = Gravity
build-flex-friction-label = Drag
build-flex-wind-label = Wind
build-flex-tension-label = Tension
build-flex-force-label = Force (X/Y/Z)
build-feature-light = Light
build-light-color-label = Color (sRGB)
build-light-intensity-label = Intensity
build-light-radius-label = Radius (m)
build-light-falloff-label = Falloff
build-spot-label = Spot (FOV/Focus/Amb)
build-tool-select-face = Select Face
build-tool-create = Create
build-create-box = Box
build-create-cylinder = Cylinder
build-create-prism = Prism
build-create-sphere = Sphere
build-create-torus = Torus
build-create-tube = Tube
build-create-ring = Ring
build-create-tree = Tree
build-create-grass = Grass
build-create-tree-species-label = Tree species
build-create-grass-species-label = Grass species
build-create-hint = Click a surface to create. Hold Shift to keep creating.
build-tex-selection-none = Select a face to edit its texture
build-tex-faces-all = All faces
build-tex-faces-count = { $count ->
    [one] { $count } face
   *[other] { $count } faces
}
build-tex-matmedia-label = Material type
build-tex-matmedia-material = Materials (Blinn-Phong)
build-tex-matmedia-pbr = PBR Metallic Roughness
build-tex-mattype-label = Map
build-tex-mattype-diffuse = Texture
build-tex-mattype-normal = Bumpiness
build-tex-mattype-specular = Shininess
build-tex-pbrtype-label = Channel
build-tex-pbrtype-material = Material
build-tex-pbrtype-base = Base
build-tex-pbrtype-metallic = Metallic
build-tex-pbrtype-emissive = Emissive
build-tex-pbrtype-normal = Normal
build-tex-texture-id-label = Texture
build-tex-color-label = Color (R/G/B)
build-tex-transparency-label = Transparency (%)
build-tex-glow-label = Glow
build-tex-fullbright = Full Bright
build-tex-bump-label = Bumpiness
build-tex-shiny-label = Shininess
build-tex-mapping-label = Mapping
build-tex-repeats-label = Repeats (U/V)
build-tex-offset-label = Offset (U/V)
build-tex-rotation-label = Rotation (°)
build-tex-align = Align planar faces
build-tex-normal-label = Normal map
build-tex-specular-label = Specular map
build-tex-glossiness-label = Glossiness
build-tex-environment-label = Environment
build-tex-shiny-color-label = Shiny color (R/G/B)
build-tex-alpha-mode-label = Alpha mode
build-tex-alpha-none = None
build-tex-alpha-blend = Alpha blending
build-tex-alpha-mask = Alpha masking
build-tex-alpha-emissive = Emissive mask
build-tex-mask-cutoff-label = Mask cutoff
build-tex-pbr-material-label = Material
build-pbr-new = New
build-pbr-save = Save
build-pbr-alpha-opaque = None
build-pbr-alpha-mask = Alpha masking
build-pbr-alpha-blend = Alpha blending
build-pbr-double-sided = Double-sided
build-pbr-base-texture-label = Base color
build-pbr-base-tint-label = Base tint (R/G/B)
build-pbr-metallic-texture-label = Metallic-roughness
build-pbr-metallic-factor-label = Metallic
build-pbr-roughness-factor-label = Roughness
build-pbr-emissive-texture-label = Emissive
build-pbr-emissive-tint-label = Emissive tint (R/G/B)
build-pbr-normal-texture-label = Normal map
build-bump-none = None
build-bump-bright = Brightness
build-bump-dark = Darkness
build-bump-woodgrain = Woodgrain
build-bump-bark = Bark
build-bump-bricks = Bricks
build-bump-checker = Checker
build-bump-concrete = Concrete
build-bump-crustytile = Crusty
build-bump-cutstone = Cutstone
build-bump-discs = Discs
build-bump-gravel = Gravel
build-bump-petridish = Petridish
build-bump-siding = Siding
build-bump-stonetile = Stonetile
build-bump-stucco = Stucco
build-bump-suction = Suction
build-bump-weave = Weave
build-shiny-none = None
build-shiny-low = Low
build-shiny-medium = Medium
build-shiny-high = High
build-texgen-default = Default
build-texgen-planar = Planar
color-picker-title = Color Picker
color-picker-preview = Preview
color-picker-original = Original
color-picker-ok = OK
color-picker-cancel = Cancel
texture-picker-title = Pick: Texture
texture-picker-title-material = Pick: Material
texture-picker-search = Search
texture-picker-none = None
texture-picker-blank = Blank
texture-picker-default = Default
texture-picker-ok = OK
texture-picker-cancel = Cancel
bottom-toolbar-build = Build
bottom-toolbar-search = Search

## The notification / toast host (viewer-ui-notification-host). Each message
## template's body may carry [KEY] substitution tokens the host fills from the
## raised notification's arguments (the reference [NAME] substitutions).

notification-button-ok = OK
notification-button-cancel = Cancel
notification-button-leave = Leave
notification-button-quit = Quit
notification-button-view-im-chat = View IM & Chat
notification-ignore-checkbox = Don't show me this again
# The session-only ignore variant (no current template uses it; kept for the
# reference IGNORE_WITH_DEFAULT_RESPONSE_SESSION_ONLY kind).
notification-ignore-checkbox-session = Don't show me this again (this session)
# The IGNORE_WITH_LAST_RESPONSE checkbox: replay the chosen button next time.
notification-ignore-choice = Always choose this option
# The [STATUS] values of notification-friend-online-offline.
notification-friend-status-online = online
notification-friend-status-offline = offline

## Dialog titles (the reference `label`): a header line on an alert / modal
## card. Shared across the entries that share a reference label.

notification-title-save-wearable = Save Wearable
notification-title-save-outfit = Save Outfit
notification-title-rename-outfit = Rename Outfit
notification-title-replace-existing-attachment = Replace Existing Attachment
notification-title-confirm-pose-overwrite = Confirm Pose Overwrite
notification-title-unknown-notification-message = Unknown Notification Message
notification-title-changed-region-maturity = Changed Region Maturity
notification-title-confirm-restart = Confirm restart
notification-title-message-everyone-in-this-region = Message everyone in this region
notification-title-message-everyone-in-your-estate = Message everyone in your Estate
notification-title-change-linden-estate = Change Linden Estate
notification-title-change-linden-estate-access = Change Linden Estate Access
notification-title-select-estate = Select estate
notification-title-confirm-ban = Confirm Ban
notification-title-confirm-kick = Confirm Kick
notification-title-confirm-teleport-home = Confirm Teleport Home
notification-system-tip = [MESSAGE]
notification-system-message = [MESSAGE]
notification-generic-alert = [MESSAGE]
notification-region-restart-minutes = The region you are in now will restart in [MINUTES] minutes. If you stay in this region you will be logged out.
notification-confirm-quit = Are you sure you want to quit?

## Keyed server alerts (viewer-notification-catalogue): the AlertInfo-keyed
## messages the simulator sends. Ported from the reference notifications.xml,
## trimmed of its bracketed knowledge-base URLs (the [KEY] engine would read a
## bracketed URL as a substitution token) pending the linkification layer.

notification-region-entry-access-blocked = The region you're trying to visit has a maturity rating exceeding your maximum maturity preference. Change this preference in your maturity preferences.
notification-teleport-entry-access-blocked = The region you're trying to teleport to has a maturity rating exceeding your maximum maturity preference. Change this preference in your maturity preferences.
notification-land-claim-access-blocked = The land you're trying to claim has a maturity rating exceeding your current preferences. You can change your preferences in your maturity preferences.
notification-land-buy-access-blocked = The land you're trying to buy has a maturity rating exceeding your current preferences. You can change your preferences in your maturity preferences.
notification-region-entry-access-blocked-notify = The region you're trying to visit contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content.
notification-region-restart-seconds = The region "[NAME]" will restart in [SECONDS] seconds. If you stay in this region when it shuts down, you will be logged out.
notification-too-many-scripts = Too many scripts.
notification-failed-to-place-object = Failed to place object at specified location. Please try again.
notification-failed-to-find-wearable = Failed to find [TYPE] in the database.
notification-home-position-set = Home position set.

## Standard action-confirmation modals (viewer-notification-catalogue): shared
## confirms raised by their owning feature (inventory / people / groups / login).

notification-confirm-empty-trash = [COUNT] items and folders will be permanently deleted. Are you sure you want to permanently delete the contents of your Trash?
notification-remove-from-friends = Are you sure you want to remove [NAME] from your Friends List?
notification-group-leave-confirm-member = Leave the group '[GROUP]'? Currently, the fee to join this group again is L$[COST].
notification-you-have-been-logged-out = You have been logged out of [CURRENT_GRID]. [MESSAGE]
notification-must-agree-to-login = You must agree to the Terms and Conditions, Privacy Policy, and Terms of Service to continue logging into [CURRENT_GRID].

## Info tips / notifies not routed to nearby chat (viewer-notification-catalogue).

notification-landmark-created = You have added "[LANDMARK_NAME]" to your [FOLDER_NAME] folder.
notification-granted-modify-rights = [NAME] has given you permission to edit their objects.
notification-teleport-to-person = To open a private conversation with someone, right-click on their avatar and choose 'IM' from the menu.
## Appearance & wearables (viewer-notification-catalogue-appearance-wearables):
## outfit / wearable / attachment notifications. Bodies follow the reference
## notifications.xml; deviations are noted per entry.

notification-button-yes = Yes
notification-button-no = No
notification-button-save = Save
notification-button-save-all = Save All
notification-button-dont-save = Don't Save
notification-button-discard = Discard
notification-button-keep-editing = Keep Editing
notification-wearable-save = Save changes to current clothing/body part?
notification-save-clothing-body-changes = Save all changes to clothing/body parts?
notification-unsaved-wearable-changes = You have unsaved changes.
notification-auto-wear-new-clothing = Would you like to automatically wear the clothing you are about to create?
notification-save-wearable-as = Save item to my inventory as:
notification-save-wearable-as-default = [DESC] (new)
notification-save-outfit-as = Save what I'm wearing as a new Outfit:
notification-save-outfit-as-default = [DESC] (new)
notification-rename-outfit = New outfit name:
notification-rename-outfit-default = [NAME]
notification-confirm-overwrite-outfit = This will replace the items in the selected outfit with the items you are wearing now.
notification-delete-outfits = Delete the selected outfit?
notification-delete-outfits-with-name = Delete outfit "[NAME]"?
notification-cant-delete-required-clothing = Some item(s) you wish to delete are required clothing layers (skin, shape, hair, eyes). You must replace those layers before deleting them.
notification-my-outfits-paste-failed = One or more items can't be used inside "Outfits"
notification-could-not-put-on-outfit = Could not put on outfit. The outfit folder contains no clothing, body parts, or attachments.
notification-cannot-wear-trash = You cannot wear clothes or body parts that are in the trash.
notification-cannot-wear-info-not-complete = You cannot wear this item because it has not yet loaded. Please try again in a minute.
notification-cannot-change-appearance-until-loaded = Cannot change appearance until clothing and shape are loaded.
# The reference body names the viewer via [APP_NAME]; nothing binds that token
# here, so the body says "the viewer" instead.
notification-clothing-loading = Your clothing is still downloading. You can use the viewer normally and other people will see you correctly.
# The reference's second sentence points at Firestorm's debug-settings menu
# (WearFolderLimit), which this viewer does not have; it is trimmed.
notification-too-many-wearables = You can't wear a folder containing more than [AMOUNT] items.
notification-max-attachments-on-outfit = Could not attach object. Exceeds the attachments limit of [MAX_ATTACHMENTS] objects. Please detach another object first.
notification-cannot-save-wearable-out-of-space = Unable to save '[NAME]' to wearable file. You will need to free up some space on your computer and save the wearable again.
notification-cannot-save-to-asset-store = Unable to save [NAME] to central asset store. This is usually a temporary failure. Please customize and save the wearable again in a few minutes.
notification-thumbnail-outfit-photo = To add an image to an outfit, use the Outfit Gallery window, or right-click on the outfit folder and select "Image..."
notification-outfit-photo-load-error = [REASON]
notification-large-outfits-warning = A large number of outfits were detected: [AMOUNT]. This may cause viewer hangs or disconnects. Consider reducing the number of outfits for better performance (below [MAX]). THIS IS ONLY A SUGGESTION - if your computer is functioning normally, you can safely ignore it.
notification-attachment-drop = You are about to drop your attachment. Are you sure you want to continue?
notification-replace-attachment = There is already an object attached to this point on your body. Do you want to replace it with the selected object?
notification-rigged-mesh-attached-to-hud = An object "[NAME]" attached to HUD point "[POINT]" contains rigged mesh. Rigged mesh objects are designed for attachment to the avatar. Neither you nor anyone else will see this object. If you want to see this object, remove it and re-attach it to an avatar attachment point.
notification-cancelled-attach = Cancelled Attach.
notification-replaced-missing-wearable = Replaced missing clothing/body part with default.
notification-attachment-saved = Attachment has been saved.
notification-failed-to-find-wearable-named = Failed to find [TYPE] named [DESC] in the database.
# The reference body names the viewer via [APP_NAME]; nothing binds that token
# here, so the body says "your viewer" instead.
notification-invalid-wearable = The item you are trying to wear uses a feature that your viewer cannot read. Please upgrade your viewer to wear this item.
notification-appearance-to-xml-saved = Appearance has been saved to XML to [PATH]
notification-appearance-to-xml-failed = Failed to save appearance to XML.
notification-shape-import-generic-fail = There was a problem importing [FILENAME]. Please see the log for more details.
notification-shape-import-version-fail = Shape import failed. Are you sure [FILENAME] is an avatar file?
notification-avatar-rez = ( [EXISTENCE] seconds alive ) Avatar '[NAME]' declouded after [TIME] seconds.
notification-avatar-rez-self-baked-done = ( [EXISTENCE] seconds alive ) You finished baking your outfit after [TIME] seconds.
notification-avatar-rez-self-baked-update = ( [EXISTENCE] seconds alive ) You sent out an update of your appearance after [TIME] seconds. [STATUS]
notification-avatar-rez-self-bake-force-update = The viewer has detected that you may appear as a cloud and is attempting to fix this automatically.
notification-avatar-rez-cloud = ( [EXISTENCE] seconds alive ) Avatar '[NAME]' became cloud.
notification-avatar-rez-arrived = ( [EXISTENCE] seconds alive ) Avatar '[NAME]' appeared.
notification-avatar-rez-left-cloud = ( [EXISTENCE] seconds alive ) Avatar '[NAME]' left after [TIME] seconds as cloud.
notification-avatar-rez-entered-appearance = ( [EXISTENCE] seconds alive ) Avatar '[NAME]' entered appearance mode.
notification-avatar-rez-left-appearance = ( [EXISTENCE] seconds alive ) Avatar '[NAME]' left appearance mode.
notification-avatar-rez-left = ( [EXISTENCE] seconds alive ) Avatar '[NAME]' left as fully loaded.
notification-avatar-rez-self-baked-texture-upload = ( [EXISTENCE] seconds alive ) You uploaded a [RESOLUTION] baked texture for '[BODYREGION]' after [TIME] seconds.
notification-avatar-rez-self-baked-texture-update = ( [EXISTENCE] seconds alive ) You locally updated a [RESOLUTION] baked texture for '[BODYREGION]' after [TIME] seconds.
notification-not-enough-resources-to-attach = Not enough script resources available to attach object!
notification-attachment-has-too-much-inventory = Your attachments contain too much inventory to add more.
notification-illegal-attachment = The attachment has requested a nonexistent point on the avatar. It has been attached to the chest instead.
notification-cant-attach-multiple-obj-one-spot = You can't attach multiple objects to one spot.
notification-no-perms-too-many-attached-animated-objects = Operation would cause the number of attached animated objects to exceed the limit.
notification-cant-attach-object-avatar-sitting-on-it = Cannot attach object because an avatar is sitting on it.
notification-why-are-you-trying-to-wear-shrubbery = Trees and grasses cannot be worn as attachments.
notification-cant-attach-group-owned-objs = Cannot attach group-owned objects.
notification-cant-attach-objects-not-owned = Cannot attach objects that you don't own.
notification-cant-attach-navmesh-objects = Cannot attach objects that contribute to navmesh.
notification-cant-attach-object-no-move-permissions = Cannot attach object because you do not have permission to move it.
notification-cant-attach-not-enough-script-resources = Not enough script resources available to attach object!
notification-cant-attach-object-being-removed = Cannot attach object because it is already being removed.
notification-cant-drop-item-trial-user = You can't drop objects here; try the Free Trial area.
notification-cant-drop-mesh-attachment = You can't drop mesh attachments. Detach to inventory and then rez in world.
notification-cant-drop-attachment-no-permission = Failed to drop attachment: you don't have permission to drop there.
notification-cant-drop-attachment-insufficient-land-resources = Failed to drop attachment: insufficient available land resource.
notification-cant-drop-attachment-insufficient-resources = Failed to drop attachments: insufficient available resources.
notification-cant-drop-object-full-parcel = Cannot drop object here. Parcel is full.
notification-cant-create-outfit = Cannot create outfit right now. Try again in a minute.

## Avatar movement (viewer-notification-catalogue-avatar-movement): animation
## upload, the animation overrider (AO), movement-mode toggles and the sit /
## stand refusals. Bodies follow the reference notifications.xml; deviations
## are noted per entry.

notification-button-remove = Remove
notification-write-animation-fail = There was a problem writing animation data. Please try again later.
# The reference body names the viewer via [APP_NAME]; nothing binds that token
# here, so the body says "The viewer" instead.
notification-do-not-support-bulk-animation-upload = The viewer does not currently support bulk upload of BVH format animation files.
notification-new-ao-set = Specify a name for the new AO set: (The name may contain any ASCII character, except for ":" or "|")
notification-new-ao-set-default = New AO Set
notification-new-ao-cant-contain-non-ascii = Could not create new AO set "[AO_SET_NAME]". The name may only contain ASCII characters, excluding ":" and "|".
notification-rename-ao-must-be-ascii = Could not rename AO set "[AO_SET_NAME]". The name may only contain ASCII characters, excluding ":" and "|".
notification-new-ao-name-cant-exist = An animation set with this name already exists.
notification-remove-ao-set = Remove AO set "[AO_SET_NAME]" from the list?
notification-ao-foreign-items-found = The animation overrider found at least one item that did not belong in the configuration. Please check your "Lost and Found" folder for items that were moved out of the animation overrider configuration.
notification-confirm-poser-overwrite = Overwrite existing pose “[POSE_NAME]”?
notification-first-override-keys = Your movement keys are now being handled by an object. Try the arrow keys or AWSD to see what they do. Some objects (like guns) require you to go into mouselook to use them. Press 'M' to do this.
notification-sit-fail-cant-move = You cannot sit because you cannot move at this time.
notification-sit-fail-not-allowed-on-land = You cannot sit because you are not allowed on that land.
notification-sit-fail-not-same-region = Try moving closer. Can't sit on object because it is not in the same region as you.
notification-stand-denied-by-object = '[OBJECT_NAME]' will not allow you to stand at this time.
notification-resit-denied-by-object = '[OBJECT_NAME]' will not allow you to change your seat at this time.
notification-cant-sit-no-suitable-surface = There is no suitable surface to sit on, try another spot.
notification-cant-sit-no-room = No room to sit here, try another spot.
notification-ao-import-complete = Animation Overrider notecard import complete!
notification-ao-import-set-already-exists = An animation set with this name already exists.
notification-ao-import-permission-denied = Insufficient permissions to read notecard.
notification-ao-import-create-set-failed = Error while creating import set.
notification-ao-import-download-failed = Could not download notecard.
notification-ao-import-no-text = Notecard is empty or unreadable.
notification-ao-import-no-folder = Couldn't find folder to read the animations.
notification-ao-import-no-state-prefix = Notecard line [LINE] has no valid [ state prefix.
notification-ao-import-no-valid-delimiter = Notecard line [LINE] has no valid ] delimiter.
notification-ao-import-state-name-not-found = State name [NAME] not found.
notification-ao-import-animation-not-found = Couldn't find animation [NAME]. Please make sure it's present in the same folder as the import notecard.
notification-ao-import-invalid = Notecard didn't contain any usable data. Aborting import.
notification-ao-import-retry-create-set = Could not create import folder for animation set [NAME]. Retrying ...
notification-ao-import-abort-create-set = Could not create import folder for animation set [NAME]. Giving up.
notification-ao-import-link-failed = Creating animation link for animation "[NAME]" failed!
notification-phantom-on = Phantom mode on.
notification-phantom-off = Phantom mode off.
notification-movelock-enabled = Movelock enabled. Use Avatar > Movement > Movelock to disable.
notification-movelock-disabled = Movelock disabled.
notification-movelock-enabling = Enabling movelock...
notification-movelock-disabling = Disabling movelock...
notification-flight-assist-enabled = Flight Assist is enabled

## Diagnostics (viewer-notification-catalogue-diagnostics): installation /
## hardware warnings, file-handling failures and local-file watcher errors.
## Bodies follow the reference notifications.xml; [APP_NAME] / "SL" viewer
## self-references are reworded ("the viewer" / "your viewer") since nothing
## binds that token, and bracketed knowledge-base URLs are trimmed (the [KEY]
## engine reads [...] as a substitution token). Other deviations are noted per
## entry.

notification-button-send = Send
notification-missing-alert = Your viewer does not know how to display the notification it just received. Please verify that you have the latest version of the viewer installed. Error details: The notification called '[_NAME]' was not found in notifications.xml.
notification-floater-not-found = Floater error: Could not find the following controls: [CONTROLS]
notification-bad-installation = Installation of the viewer is defective. Please download a new copy of the viewer and reinstall.
notification-found-legacy-nsis-installation = The viewer found an installation of an older version [VERSION]. Please uninstall the older version.
notification-message-template-not-found = Message Template [PATH] not found.
notification-allow-multiple-viewers = Running multiple viewers is not supported. It can lead to texture cache collisions, corruption and degraded visuals and performance.
notification-unsupported-hardware = Just so you know, your computer may not meet the viewer's minimum system requirements. You may experience poor performance. Unfortunately, the [SUPPORT_SITE] can't provide technical support for unsupported system configurations. [MINSPECS] Visit [_URL] for more information?
notification-old-gpu-driver = There is likely a newer driver for your graphics chip. Updating graphics drivers can substantially improve performance. Visit [URL] to check for driver updates?
# The reference points at Firestorm's "Avatar > Preferences > Graphics" menu
# path; ours is Preferences > Graphics.
notification-unknown-gpu = Your system contains a graphics card that the viewer doesn't recognize. This is often the case with new hardware that has not been tested yet with the viewer. It will probably be ok, but you may need to adjust your graphics settings. (Preferences > Graphics)
notification-display-settings-no-shaders = The viewer crashed while initializing graphics drivers. Graphics Quality will be set to Low to avoid some common driver errors. This will disable some graphics features. We recommend updating your graphics card drivers. Graphics Quality can be raised in Preferences > Graphics.
notification-no-havok = Some functions like [FEATURE] are not included in this version of the viewer. If you would like to use [FEATURE], please download a viewer containing Havok support from [DOWNLOAD_URL]
notification-no-support-gltf-shader = GLTF scenes are not yet supported on your graphics hardware.
notification-low-memory = Your memory pool is low. Some functions of the viewer are disabled to avoid crash. Please close other applications. Restart the viewer if this persists.
notification-force-quit-due-to-low-memory = The viewer will quit in 30 seconds due to out of memory.
notification-out-of-disk-space = The system is out of disk space. You will need to free up some space on your computer or clear the cache.
notification-region-capability-request-error = Could not get region capability '[CAPABILITY]'.
notification-missing-string = The string [STRING_NAME] is missing from strings.xml.
notification-failed-requirements-check = The following required components are missing from [FLOATER]: [COMPONENTS]
notification-compression-test-results = Test result for gzip level 6 file compression with [FILE] of size [SIZE] KB: Packing: [PACK_TIME]s [PSIZE]KB Unpacking: [UNPACK_TIME]s [USIZE]KB
notification-send-sysinfo-to-im = This will send the following information to the current IM session: [SYSINFO]
notification-firestorm-req-info = [NAME] is requesting that you send them information about your viewer setup. (This is the same information that can be found by going to Help > About) [REASON] Would you like to send them this information?
# The reference wraps the token in literal brackets ([[FILE]]), which the
# [KEY] engine would misparse; the plain token is kept instead.
notification-cannot-write-file = Unable to write file [FILE]
notification-no-file-extension = No file extension for the file: '[FILE]' Please make sure the file has a correct file extension.
notification-invalid-file-extension = Invalid file extension [EXTENSION]. Expected [VALIDS].
notification-problem-with-file = Problem with file [FILE]: [REASON]
notification-cannot-encode-file = Unable to encode file: [FILE]
notification-corrupt-resource-file = Corrupt resource file: [FILE]
notification-unknown-resource-file-version = Unknown Linden resource file version in file: [FILE]
notification-unable-to-create-output-file = Unable to create output file: [FILE]
notification-cannot-upload-reason = Unable to upload [FILE] due to the following reason: [REASON] Please try again later.
notification-cannot-open-file-too-big = Unable to open file. Viewer ran out of memory while opening file. File might be too big.
notification-cannot-load = Unable to load [WHAT]. [REASON]
notification-not-regular-file-error = Expected to find a regular file at: [FILE_NAME]
notification-not-folder-error = Expected to find a regular folder at: [FILE_NAME]
notification-generic-file-empty-error = File exists but is empty: [FILE_NAME] Error message: [ERROR_MESSAGE] ([ERROR_CODE])
notification-generic-file-open-read-error = Could not open file for reading: [FILE_NAME] Error message: [ERROR_MESSAGE] ([ERROR_CODE])
notification-generic-file-open-write-error = Could not open file for writing: [FILE_NAME] Error message: [ERROR_MESSAGE] ([ERROR_CODE])
notification-generic-file-read-error = Could not read from file: [FILE_NAME] Error message: [ERROR_MESSAGE] ([ERROR_CODE])
notification-generic-file-write-error = Could not write to file: [FILE_NAME] Error message: [ERROR_MESSAGE] ([ERROR_CODE])
notification-local-bitmaps-update-file-not-found = [FNAME] could not be updated because the file could no longer be found. Disabling future updates for this file.
notification-local-bitmaps-update-failed-final = [FNAME] could not be opened or decoded for [NRETRIES] attempts, and is now considered broken. Disabling future updates for this file.
notification-local-bitmaps-verify-fail = Attempted to add an invalid or unreadable image file [FNAME] which could not be opened or decoded. Attempt canceled.
notification-local-gltf-verify-fail = Attempted to add an invalid or unreadable GLTF material [FNAME] which could not be opened or decoded. Attempt cancelled.

## Estate & region management (viewer-notification-catalogue-estate-region):
## region tools, terrain validation, estate access lists, admin / god tools,
## pathfinding and the server-keyed freeze / eject / entry refusals. Bodies
## follow the reference notifications.xml; deviations are noted per entry.

notification-button-this-estate = This Estate
notification-button-all-estates = All Estates
notification-button-kick-all-residents = Kick All Residents
notification-button-dont-ask = Don't ask
notification-button-bake = Bake
notification-button-rebake = Rebake
notification-button-close = Close
notification-button-rebake-region = Rebake region
notification-return-all-top-objects = Are you sure you want to return all listed objects back to their owner's inventory? This will return ALL scripted objects in the region!
notification-disable-all-top-objects = Are you sure you want to disable all objects in this region?
notification-unable-to-disable-outside-scripts = Cannot disable scripts. This entire region is damage enabled. Scripts must be allowed to run for weapons to work.
notification-region-no-terraforming = The region [REGION] does not allow terraforming.
notification-flush-map-visibility-caches = This will flush the map caches on this region. This is really only useful for debugging. (In production, wait 5 minutes, then everyone's map will update after they relog.)
notification-kick-users-from-region = Teleport all residents in this region home?
notification-change-object-bonus-factor = Lowering the object bonus after builds have been established in a region may cause objects to be returned or deleted. Are you sure you want to change object bonus?
notification-estate-object-return = Are you sure you want to return objects owned by [USER_NAME]?
notification-raw-upload-started = Upload started. It may take up to two minutes, depending on your connection speed.
notification-confirm-bake-terrain = Do you really want to bake the current terrain, make it the center for terrain raise/lower limits, and the default for the 'Revert' tool?
notification-confirm-texture-heights = You're about to use low values greater than high ones for Elevation Ranges. Proceed?
notification-finished-raw-download = Finished download of raw terrain file to: [DOWNLOAD_PATH].
notification-region-maturity-change = The maturity rating for this region has been changed. It may take some time for this change to be reflected on the map.
notification-confirm-restart = Do you really want to schedule this region to restart?
notification-message-region = Type a short announcement which will be sent to everyone in this region.
notification-invalid-terrain-bit-depth = Couldn't set region textures: Terrain texture [TEXTURE_NUM] has an invalid bit depth of [TEXTURE_BIT_DEPTH]. Replace texture [TEXTURE_NUM] with an RGB [MAX_SIZE]x[MAX_SIZE] or smaller image then click "Apply" again.
notification-invalid-terrain-alpha-not-fully-loaded = Couldn't set region textures: Terrain texture [TEXTURE_NUM] is not fully loaded, but is assumed to contain transparency due to a bit depth of [TEXTURE_BIT_DEPTH]. Transparency is not currently supported for terrain textures. If texture [TEXTURE_NUM] is opaque, wait for the texture to fully load and then click "Apply" again. Alpha is only supported for terrain materials (PBR Metallic Roughness), when alphaMode="MASK" and doubleSided=false.
notification-invalid-terrain-alpha = Couldn't set region textures: Terrain texture [TEXTURE_NUM] contains transparency. Transparency is not currently supported for terrain textures. Replace texture [TEXTURE_NUM] with an opaque RGB image, then click "Apply" again. Alpha is only supported for terrain materials (PBR Metallic Roughness), when alphaMode="MASK" and doubleSided=false.
notification-invalid-terrain-size = Couldn't set region textures: Terrain texture [TEXTURE_NUM] is too large at [TEXTURE_SIZE_X]x[TEXTURE_SIZE_Y]. Replace texture [TEXTURE_NUM] with an RGB [MAX_SIZE]x[MAX_SIZE] or smaller image then click "Apply" again.
notification-invalid-terrain-material-not-loaded = Couldn't set region materials: Terrain material [MATERIAL_NUM] is not loaded. Wait for the material to load, or replace material [MATERIAL_NUM] with a valid material.
notification-invalid-terrain-material-load-failed = Couldn't set region materials: Terrain material [MATERIAL_NUM] failed to load. Replace material [MATERIAL_NUM] with a valid material.
notification-invalid-terrain-material-double-sided = Couldn't set region materials: Terrain material [MATERIAL_NUM] is double-sided. Double-sided materials are not currently supported for PBR terrain. Replace material [MATERIAL_NUM] with a material with doubleSided=false.
notification-invalid-terrain-material-alpha-mode = Couldn't set region materials: Terrain material [MATERIAL_NUM] is using the unsupported alphaMode="[MATERIAL_ALPHA_MODE]". Replace material [MATERIAL_NUM] with a material with alphaMode="OPAQUE" or alphaMode="MASK".
notification-max-allowed-agent-on-region = You can only have [MAX_AGENTS] allowed residents.
notification-max-banned-agents-on-region = You can only have [MAX_BANNED] banned residents.
notification-max-agent-on-region-batch = Failure while attempting to add [NUM_ADDED] agents: Exceeds the [MAX_AGENTS] [LIST_TYPE] limit by [NUM_EXCESS].
notification-max-allowed-groups-on-region = You can only have [MAX_GROUPS] groups.
notification-max-managers-on-region = You can only have [MAX_MANAGER] estate managers.
notification-owner-cannot-be-denied = Cannot add estate owner to estate 'Banned resident' list.
notification-problem-adding-estate-manager-banned = Unable to add banned resident to estate manager list.
notification-problem-banning-estate-manager = Unable to add estate manager [AGENT] to banned list.
# The reference wraps the group name in <nolink> markup; linkification is not
# ported yet, so the plain token is kept.
notification-group-is-already-in-list = [GROUP] is already in the Allowed Groups list.
notification-agent-is-already-in-list = [AGENT] is already in your [LIST_TYPE] list.
notification-agents-are-already-in-list = [AGENT] are already in your [LIST_TYPE] list.
notification-agent-was-added-to-list = [AGENT] was added to [LIST_TYPE] list of [ESTATE].
notification-agents-were-added-to-list = [AGENT] were added to [LIST_TYPE] list of [ESTATE].
notification-agent-was-removed-from-list = [AGENT] was removed from [LIST_TYPE] list of [ESTATE].
notification-agents-were-removed-from-list = [AGENT] were removed from [LIST_TYPE] list of [ESTATE].
notification-problem-importing-estate-covenant = Problem importing estate covenant.
notification-problem-adding-estate-manager = Problems adding a new estate manager. One or more estates may have a full manager list.
notification-problem-adding-estate-ban-manager = Unable to add estate owner or manager to ban list.
notification-problem-adding-estate-generic = Problems adding to this estate list. One or more estates may have a full list.
notification-estate-parcel-access-override = Unchecking this option may remove restrictions that parcel owners have added to prevent griefing, maintain privacy, or protect underage residents from adult material. Please discuss with your parcel owners as needed.
notification-estate-parcel-environment-override = (Estate-wide change: [ESTATENAME]) Unchecking this option will remove any custom environments that parcel owners have added to their parcels. Please discuss with your parcel owners as needed. Do you wish to proceed?
notification-estate-change-covenant = Are you sure you want to change the estate covenant?
notification-region-entry-access-blocked-preferences-out-of-sync = We are having technical difficulties with your region entry because your preferences are out of sync with the server.
notification-confirm-kick = Do you REALLY want to kick all residents off the grid?
notification-kick-user = Kick this Resident with what message?
notification-kick-user-default = An administrator has logged you off.
notification-kick-all-users = Kick everyone currently on the grid with what message?
notification-freeze-user = Freeze this Resident with what message?
notification-freeze-user-default = You have been frozen. You cannot move or chat. An administrator will contact you via instant message (IM).
notification-unfreeze-user = Unfreeze this Resident with what message?
notification-unfreeze-user-default = You are no longer frozen.
notification-message-estate = Type a short announcement which will be sent to everyone currently in your estate.
notification-change-linden-estate = You are about to change a Linden owned estate (mainland, teen grid, orientation, etc.). This is EXTREMELY DANGEROUS because it can fundamentally affect the resident experience. On the mainland, it will change thousands of regions and make the spaceserver hiccup. Proceed?
notification-change-linden-access = You are about to change the access list for a Linden owned estate (mainland, teen grid, orientation, etc.). This is DANGEROUS and should only be done to invoke the hack allowing objects/L$ to be transferred in/out of a grid. It will change thousands of regions and make the spaceserver hiccup.
notification-estate-allowed-agent-add = Add to allowed list for this estate only or for [ALL_ESTATES]?
notification-estate-allowed-agent-remove = Remove from allowed list for this estate only or for [ALL_ESTATES]?
notification-estate-allowed-group-add = Add to group allowed list for this estate only or for [ALL_ESTATES]?
notification-estate-allowed-group-remove = Remove from group allowed list for this estate only or [ALL_ESTATES]?
notification-estate-banned-agent-add = Deny access for this estate only or for [ALL_ESTATES]?
notification-estate-banned-agent-remove = Remove this Resident from the ban list for access for this estate only or for [ALL_ESTATES]?
notification-estate-manager-add = Add estate manager for this estate only or for [ALL_ESTATES]?
notification-estate-manager-remove = Remove estate manager for this estate only or for [ALL_ESTATES]?
notification-estate-allowed-experience-add = Add to allowed list for this estate only or for [ALL_ESTATES]?
notification-estate-allowed-experience-remove = Remove from allowed list for this estate only or for [ALL_ESTATES]?
notification-estate-blocked-experience-add = Add to blocked list for this estate only or for [ALL_ESTATES]?
notification-estate-blocked-experience-remove = Remove from blocked list for this estate only or for [ALL_ESTATES]?
notification-estate-trusted-experience-add = Add to key list for this estate only or for [ALL_ESTATES]?
notification-estate-trusted-experience-remove = Remove from key list for this estate only or for [ALL_ESTATES]?
notification-estate-ban-user = Deny access for [EVIL_USER] for this estate only or for [ALL_ESTATES]?
notification-estate-ban-user-multiple = Deny access for the following residents this estate only or for [ALL_ESTATES]? [RESIDENTS]
notification-estate-kick-user = Kick [EVIL_USER] from this estate?
notification-estate-kick-multiple = Kick the following residents from this estate? [RESIDENTS]
notification-estate-teleport-home-user = Teleport [AVATAR_NAME] home?
notification-estate-teleport-home-multiple = Teleport the following residents home? [RESIDENTS]
notification-pathfinding-dirty = The region has pending pathfinding changes. If you have build rights, you may rebake the region by clicking on the "Rebake" button.
notification-pathfinding-dirty-rebake = The region has pending pathfinding changes. If you have build rights, you may rebake the region by clicking on the "Rebake region" button.
notification-dynamic-pathfinding-disabled = Dynamic pathfinding is not enabled on this region. Scripted objects using pathfinding LSL calls may not operate as expected on this region.
notification-pathfinding-cannot-rebake-navmesh = An error occurred. There may be a network or server problem, or you may not have build rights. Sometimes logging out and back in will solve this problem.
notification-region-about-to-shutdown = The region you're trying to enter is about to shut down.
notification-ur-banned-from-region = You are banned from the region.
notification-no-teen-grid-access = Your account cannot connect to this teen grid region.
notification-improper-payment-status = You do not have proper payment status to enter this region.
notification-must-get-age-region = You must be age 18 or over to enter this region.
notification-region-restart-minutes-toast = The region "[NAME]" will restart in [MINUTES] minutes. If you stay in this region when it shuts down, you will be logged out.
notification-region-restart-seconds-toast = The region "[NAME]" will restart in [SECONDS] seconds. If you stay in this region when it shuts down, you will be logged out.
notification-avatar-frozen = [AV_FREEZER] has frozen you. You cannot move or interact with the world.
notification-avatar-frozen-duration = [AV_FREEZER] has frozen you for [AV_FREEZE_TIME] seconds. You cannot move or interact with the world.
notification-you-froze-avatar = Avatar frozen.
notification-avatar-has-unfrozen-you = [AV_FREEZER] has unfrozen you.
notification-avatar-unfrozen = Avatar unfrozen.
notification-avatar-freeze-failure = Freeze failed because you don't have admin permission for that parcel.
notification-avatar-freeze-thaw = Your freeze expired, go about your business.
notification-avatar-cant-freeze = Sorry, can't freeze that user.
notification-eject-coming-soon = You are no longer allowed here and have [EJECT_TIME] seconds to leave.
notification-no-enter-region-maybe-full = You can't enter region "[NAME]". It may be full or restarting soon.
notification-sorry-cant-eject-user = Sorry, can't eject that user.
notification-avatar-eject-failed = Eject failed because you don't have admin permission for that parcel.
notification-full-region-cant-enter = You can't enter this region because the region is full.
notification-estate-manager-failed-ll-teleport-home = The object '[OBJECT_NAME]' at [SLURL] cannot teleport estate managers home.
notification-cant-teleport-could-not-find-user = Could not find user to teleport home
notification-terrain-upload-failed = Terrain upload failed.
notification-terrain-file-written = Terrain file written.
notification-terrain-file-written-starting-download = Terrain file written, starting download...
notification-terrain-baked = Terrain baked.
notification-god-beats-freeze = Your godlike powers break the freeze!
notification-region-entry-access-blocked-notify-adults-only = The region you're trying to visit contains [REGIONMATURITY] content, which is accessible to adults only.
notification-terrain-downloaded = Terrain.raw downloaded.
notification-entering-god-mode = Entering god mode, level [LEVEL]
notification-leaving-god-mode = Now leaving god mode, level [LEVEL]
notification-avatar-ejected = Avatar ejected.
notification-server-version-changed = The region you have entered is running a different simulator version. Current simulator: [NEWVERSION] Previous simulator: [OLDVERSION]

## Button labels for the consolidated catalogue port.

notification-button-create-a-new-account = Create a new account
notification-button-try-again = Try again
notification-button-create-account = Create Account...
notification-button-continue = Continue
notification-button-confirm-and-log-out = Confirm and log out
notification-button-help = Help
notification-button-teleport = Teleport
notification-button-confirm = Confirm
notification-button-male = Male
notification-button-female = Female
notification-button-install = Install
notification-button-skip = Skip
notification-button-not-now = Not Now
notification-button-reset = Reset
notification-button-remind-me-next-time = Remind me next time
notification-button-move-items = Move item(s)
notification-button-dont-move-items = Don't move item(s)
notification-button-deed = Deed
notification-button-unlink = Unlink
notification-button-discard-changes = Discard changes
notification-button-keep-editing-2 = Keep editing
notification-button-strip-alpha = Strip Alpha
notification-button-use-as-is = Use As Is
notification-button-replace-current-list = Replace Current List
notification-button-use-new-name = Use New Name
notification-button-delete = Delete
notification-button-accept = Accept
notification-button-decline = Decline
notification-button-mute = Mute
notification-button-respond = Respond
notification-button-shutdown-now = Shutdown now
notification-button-later = Later
notification-button-go-to-knowledge-base = Go to Knowledge Base
notification-button-change-preferences = Change preferences
notification-button-dont-quit = Don't Quit
notification-button-fix-it = Fix it
notification-button-keep-it = Keep it
notification-button-all-modes = All modes
notification-button-current-mode = Current mode
notification-button-save-backup = Save backup
notification-button-restore-and-quit = Restore and Quit
notification-button-create = Create
notification-button-apply-changes = Apply Changes
notification-button-ignore-changes = Ignore Changes
notification-button-eject = Eject
notification-button-ban = Ban
notification-button-join = Join
notification-button-create-group-for-l-cost = Create group for L$[COST]
notification-button-info = Info
notification-button-freeze = Freeze
notification-button-unfreeze = Unfreeze
notification-button-eject-and-ban = Eject and Ban
notification-button-done = Done
notification-button-play-media-now = Play Media Now
notification-button-always-play-media = Always Play Media
notification-button-do-not-play-media = Do Not Play Media
notification-button-play = Play
notification-button-dont-play = Don't play
notification-button-enable = Enable
notification-button-disable = Disable
notification-button-allow = Allow
notification-button-deny = Deny
notification-button-action-now = [ACTION] Now
notification-button-condition-allow-this-domain = [CONDITION] Allow This Domain
notification-button-condition-allow-this-url = [CONDITION] Allow This URL
notification-button-blacklist = Blacklist
notification-button-whitelist = Whitelist
notification-button-add = Add
notification-button-pay = Pay
notification-button-upload = Upload
notification-button-details = Details
notification-button-copy = Copy
notification-button-remove-items-and-delete = Remove item(s) and delete
notification-button-check-trash-folder = Check trash folder
notification-button-i-will-empty-trash-later = I will empty trash later
notification-button-show = Show
notification-button-show-2 = (Show)
notification-button-accept-2 = (Accept)
notification-button-discard-2 = (Discard)
notification-button-mute-sender = Mute Sender
notification-button-okay = Okay
notification-button-go-to-page = Go to page
notification-button-go-now = Go Now...
notification-button-trust = Trust
notification-button-change-and-continue = Change and continue
notification-button-change-and-continue-2 = Change and Continue
notification-button-always-allow = Always Allow

## Dialog titles for the consolidated catalogue port.

notification-title-prompt-for-mfa-token = Prompt for MFA Token
notification-title-save-material = Save Material
notification-title-image-contains-empty-alpha-channel = Image Contains Empty Alpha Channel
notification-title-add-auto-replace-list = Add Auto-Replace List
notification-title-rename-auto-replace-list = Rename Auto-Replace List
notification-title-remove-auto-replace-list = Remove Auto-Replace List
notification-title-reset-all-settings = Reset all settings
notification-title-save-environmental-settings = Save Environmental Settings
notification-title-add-friend = Add Friend
notification-title-block-object-by-name-failed = Block object by name failed
notification-title-friendship-offer-from-name-label = Friendship offer from [NAME_LABEL]
notification-title-parcel-is-playing-media = Parcel is Playing Media
notification-title-cannot-buy-objects = Cannot Buy Objects
notification-title-cannot-buy-contents = Cannot Buy Contents
notification-title-unavailable-mode-warning = Unavailable Mode Warning
notification-title-create-folder = Create folder
notification-title-rename-landmark = Rename Landmark
notification-title-rename-gesture = Rename Gesture
notification-title-rename-selected-item = Rename Selected Item
notification-title-inventory-offer-from-name-label = Inventory offer from [NAME_LABEL]
notification-title-inventory-validation-errors = Inventory Validation Errors
notification-title-create-subfolder = Create subfolder
notification-title-ungroup-folder = Ungroup folder
notification-title-about-requests-for-the-debit-permission = About Requests for the Debit Permission
notification-title-teleport-offer-from-name-label = Teleport offer from [NAME_LABEL]
notification-title-voice-version-mismatch = Voice Version Mismatch
notification-title-restriction-request-from-name-label = Restriction request from [NAME_LABEL]

## UI hints (viewer-notification-catalogue-ui-hints). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-tutorial-not-found = No tutorial is currently available.
notification-teleport-to-landmark = To teleport to locations like '[NAME]', click on the "Places" button, then select the Landmarks tab in the window that opens. Click on any landmark to select it, then click 'Teleport' at the bottom of the window. (You can also double-click on the landmark, or right-click it and choose 'Teleport'.)
notification-unable-to-find-help-topic = Unable to find the help topic for this element.
notification-hint-chat = To join the conversation, type into the chat field below.
notification-hint-sit = To stand up and exit the sitting position, click the Stand button.
notification-hint-speak = Click the Speak button to turn your microphone on and off. Click on the up arrow to see the voice control panel. Hiding the Speak button will disable the voice feature.
notification-hint-destination-guide = The Destination Guide contains thousands of new places to discover. Select a location and choose Teleport to start exploring.
notification-hint-side-panel = Get quick access to your inventory, outfits, profiles and more in the side panel.
notification-hint-move = To walk or run, open the Move Panel and use the directional arrows to navigate. You can also use the directional keys on your keyboard.
notification-hint-move-click = 1. Click to Walk Click anywhere on the ground to walk to that spot. 2. Click and Drag to Rotate View Click and drag anywhere on the world to rotate your view
notification-hint-display-name = Set your customizable display name here. This is in addition to your unique username, which can't be changed. You can change how you see other people's names in your preferences.
notification-hint-view = To change your camera view, use the Orbit and Pan controls. Reset your view by pressing Escape or walking.
notification-hint-inventory = Check your inventory to find items. Newest items can be easily found in the Recent tab.
notification-hint-linden-dollar = Here's your current balance of L$. Click Buy L$ to purchase more Linden Dollars.
notification-first-use-fly-override = Caution: Use the Fly Override responsibly! Using the Fly Override without the land owner's permission may result in your avatar being banned from the parcel in which you are flying.

## Miscellaneous (viewer-notification-catalogue-misc). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-generic-alert-yes-cancel = [MESSAGE]
notification-generic-alert-ok = [MESSAGE]
notification-invalid-keystroke = There was an invalid keystroke entered. [REASON]. Please input a valid text.
notification-save-changes = Save Changes?
notification-no-frontmost-floater = No frontmost floater to save.
notification-error-message = [ERROR_MESSAGE]
notification-system-message-tip = [MESSAGE]
notification-cancelled = Cancelled.
notification-deactivated-gestures-trigger = Deactivated gestures with same trigger: [NAMES]
notification-god-message = [NAME] [MESSAGE]
notification-max-list-select-message = You may only select up to [MAX_SELECT] items from this list.
notification-godlike-request-failed = godlike request failed
notification-generic-request-failed = generic request failed
notification-special-powers-request-failed-logged = Request for special powers failed. This request has been logged.
notification-expire-explanation = The system is currently unable to process your request. The request timed out.
notification-die-explanation = The system is unable to process your request.
notification-reg-ex-fail = Error in the regular expression: [EWHAT]
notification-whitelist-reminder = To improve the viewer's performance, please whitelist it. Some antivirus programs may mistakenly block parts of the viewer, slowing down its performance and causing some features to malfunction. To prevent these issues, we strongly recommend adding the viewer to your antivirus program's whitelist (or exclusion list). This will ensure the viewer runs smoothly. For detailed instructions on how to whitelist the viewer - including a list of files and folders to exclude - please visit our guide: https://wiki.firestormviewer.org/antivirus_whitelisting

## Login & session (viewer-notification-catalogue-login-session). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-login-failed-no-network = Could not connect to the [CURRENT_GRID]. '[DIAGNOSTIC]' Make sure your Internet connection is working properly.
notification-login-failed-to-parse = Viewer received malformed response from server. Please, make sure your Internet connection is working properly and try again later. If you feel this is in error, please contact Support.
notification-must-enter-password-to-log-in = Please enter your Password to log in.
notification-must-have-account-to-log-in = You need to enter the Username name of your avatar. You need an account to enter [CURRENT_GRID]. Would you like to create one now?
notification-invalid-credential-format = You need to enter either the Username or both the First and Last name of your avatar into the Username field, then login again.
notification-invalid-grid = '[GRID]' is not a valid grid identifier.
notification-invalid-location-slurl = Your start location did not specify a valid grid.
notification-start-region-empty = Your Start Region is not defined. Please type the Region name in Start Location box or choose My Last Location or My Home as your Start Location.
notification-first-run = The viewer installation is complete. If this is your first time using [CURRENT_GRID], you will need to create an account before you can log in.
notification-login-cant-remove-username = Already remembered user can be forgotten from Me > Preferences > Advanced > Remembered Usernames.
notification-login-cant-remove-cur-username = Forgetting the logged-in user requires you to log out.
notification-login-packet-never-received = We're having trouble connecting. There may be a problem with your Internet connection or the [CURRENT_GRID]. You can either check your Internet connection and try again in a few minutes, click Help to view the [SUPPORT_SITE], or click Teleport to attempt to teleport home.
notification-login-packet-never-received-no-tp = We're having trouble connecting. There may be a problem with your Internet connection or the [CURRENT_GRID]. You can either check your Internet connection and try again in a few minutes or click Help to view the [SUPPORT_SITE].
notification-login-remove-multi-grid-user-data = Local Data you are deleting is shared between multiple grids, are you sure you want to delete it?
notification-welcome-choose-sex = Your character will appear in a moment. Use arrow keys to walk. Press the F1 key at any time for help or to learn more about [CURRENT_GRID]. Please choose the male or female avatar. You can change your mind later.
notification-required-update = Version [VERSION] is required for login. Please download from https://secondlife.com/support/downloads/
notification-pause-for-update = Version [VERSION] is required for login. Release notes: [URL] Click OK to download and install.
notification-downloading-update = Downloading update [VERSION]... The viewer will restart once the download is complete.
notification-optional-update-ready = Version [VERSION] has been downloaded and is ready to install. Release notes: [URL] Click OK to install.
notification-prompt-optional-update = Version [VERSION] has been downloaded and is ready to install. Release notes: [URL] Proceed?
notification-login-failed-unknown = Sorry, login failed for an unrecognized reason. If you continue to get this message, please check the [SUPPORT_SITE].
notification-caps-key-on = Your Caps Lock key is on. This might affect your password.
notification-failed-to-get-benefits = Unfortunately, we were unable to get benefits information for this session. This should not happen in a normal production environment. Please contact support. This session will not work normally and we recommend that you restart.
notification-no-connect = We're having trouble connecting using [PROTOCOL] [HOSTID]. Please check your network and firewall setup.
notification-socks-not-permitted = The SOCKS 5 proxy "[HOST]:[PORT]" refused the connection, not allowed by rule set.
notification-socks-connect-error = The SOCKS 5 proxy "[HOST]:[PORT]" refused the connection, could not open TCP channel.
notification-socks-not-acceptable = The SOCKS 5 proxy "[HOST]:[PORT]" refused the selected authentication system.
notification-socks-auth-fail = The SOCKS 5 proxy "[HOST]:[PORT]" reported your credentials are invalid.
notification-socks-udp-fwd-not-granted = The SOCKS 5 proxy "[HOST]:[PORT]" refused the UDP associate request.
notification-socks-host-connect-failed = Could not connect to SOCKS 5 proxy server "[HOST]:[PORT]".
notification-socks-unknown-status = Unknown proxy error with server "[HOST]:[PORT]".
notification-socks-invalid-host = Invalid SOCKS proxy address or port "[HOST]:[PORT]".
notification-confirm-remove-grid = Are you sure you want to remove [REMOVE_GRID] from the grid list?
notification-can-not-remove-connected-grid = You can not remove [REMOVE_GRID] while being connected to it.
notification-block-login-info = [REASON]
notification-testversion-expired = This test version of the viewer has expired and cannot be used any further.
notification-cant-add-grid = Could not add [GRID] to the grid list. [REASON] contact support of [GRID].
notification-confirm-remove-credential = Delete saved login for [NAME]?
notification-prompt-mfa-token = [MESSAGE]
notification-prompt-mfa-token-with-save = [MESSAGE]
notification-warn-force-login-url = Login splash screen URL is overridden for testing purposes. Reset the URL to default?

## Marketplace (viewer-notification-catalogue-marketplace). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-confirm-no-copy-to-outbox = You don't have permission to copy one or more of these items to the Merchant Outbox. You can move them or leave them behind.
notification-outbox-folder-created = A new folder has been created for each item you have transferred into the top level of your Merchant Outbox.
notification-outbox-import-complete = Success All folders were successfully sent to the Marketplace.
notification-outbox-import-had-errors = Some folders did not transfer Errors occurred when some folders were sent to the Marketplace. Those folders are still in your Merchant Outbox. See the error log for more information.
notification-outbox-import-failed = Transfer failed with error '[ERROR_CODE]' No folders were sent to the Marketplace because of a system or network error. Try again later.
notification-outbox-init-failed = Marketplace initialization failed with error '[ERROR_CODE]' Initialization with the Marketplace failed because of a system or network error. Try again later.
notification-stock-paste-failed = Copy or move to Stock Folder failed with error : '[ERROR_CODE]'
notification-merchant-paste-failed = Copy or move to Marketplace Listings failed with error : '[ERROR_CODE]'
notification-merchant-transaction-failed = The transaction with the Marketplace failed with the following error : [ERROR_REASON][ERROR_DESCRIPTION]
notification-merchant-unprocessable-entity = We are unable to list this product or activate the version folder. Usually this is caused by missing information in the listing description form, but it may be due to errors in the folder structure. Either edit the listing or check the listing folder for errors.
notification-merchant-listing-failed = Listing to Marketplace failed with error : '[ERROR_CODE]'
notification-merchant-folder-activation-failed = Activating this version folder failed with error : '[ERROR_CODE]'
notification-merchant-force-validate-listing = In order to create your listing, we fixed the hierarchy of your listing contents.
notification-confirm-merchant-active-change = This action will change the active content of this listing. Do you want to continue?
notification-confirm-merchant-move-inventory = Items dragged to the Marketplace Listings window are moved from their original locations, not copied. Do you want to continue?
notification-confirm-listing-cut-or-delete = Moving or deleting a listing folder will delete your Marketplace listing. If you would like to keep the Marketplace listing, move or delete the contents of the version folder you would like to modify. Do you want to continue?
notification-confirm-copy-to-marketplace = You don't have permission to copy one or more of these items to the Marketplace. You can move them or leave them behind.
notification-confirm-merchant-unlist = This action will unlist this listing. Do you want to continue?
notification-confirm-merchant-clear-version = This action will deactivate the version folder of the current listing. Do you want to continue?
notification-alert-merchant-listing-not-updated = This listing could not be updated. Click here to edit it on the Marketplace.
notification-alert-merchant-listing-cannot-wear = You cannot wear clothes or body parts that are in the Marketplace Listings folder.
notification-alert-merchant-listing-invalid-id = Invalid listing ID.
notification-alert-merchant-listing-activate-required = There are several or no version folders in this listing. You will need to select and activate one independently later.
notification-alert-merchant-stock-folder-split = We have separated stock items of different types into separate stock folders, so your folder is arranged in a way that we can list it.
notification-alert-merchant-stock-folder-empty = We have unlisted your listing because the stock is empty. You need to add more units to the stock folder to list the listing again.
notification-alert-merchant-version-folder-empty = We have unlisted your listing because the version folder is empty. You need to add items to the version folder to list the listing again.
notification-slm-update-folder = [MESSAGE]

## Snapshot & social (viewer-notification-catalogue-snapshot-social). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-upload-auction-snapshot-fail = There was a problem uploading the auction snapshot due to the following reason: [REASON]
notification-prompt-recipient-email = Please enter a valid email address for the recipient(s).
notification-prompt-self-email = Please enter your email address.
notification-prompt-missing-subj-msg = Email snapshot with the default subject or message?
notification-error-processing-snapshot = Error processing snapshot data.
notification-error-encoding-snapshot = Error encoding snapshot.
notification-error-photo-cannot-afford = You need L$[COST] to save a photo to your inventory. You may either buy L$ or save the photo to your computer instead.
notification-error-encoding-image = Failed to encode image, reason: [REASON]
notification-error-texture-cannot-afford = You need L$[COST] to save a texture to your inventory. You may either buy L$ or save the photo to your computer instead.
notification-error-uploading-postcard = There was a problem sending a snapshot due to the following reason: [REASON]
notification-error-uploading-report-screenshot = There was a problem uploading a report screenshot due to the following reason: [REASON]
notification-delete-classified = Delete classified '[NAME]'? There is no reimbursement for fees paid.
notification-classified-save = Save changes to classified [NAME]?
notification-classified-insufficient-funds = Insufficient funds to create classified.
notification-profile-delete-classified = Delete classified [CLASSIFIED]?
notification-profile-delete-pick = Delete pick [PICK]?
notification-profile-unpublished-classified = You have unpublished classifieds. They will be lost if you close the window.
notification-profile-unsaved-changes = You have unsaved changes.
notification-load-previous-report-screenshot = Do you want to use previous screenshot for your report?
notification-cannot-upload-snapshot-email-too-big = Unable to upload snapshot [FILE] due to the following reason: [REASON] File might be too big, try reducing resolution, quality or try again later.
notification-cannot-upload-snapshot-web-too-big = Unable to upload snapshot. File might be too big, try reducing resolution or try again later.
notification-blank-classified-name = You must specify a name for your classified.
notification-min-classified-price = Price to pay for listing must be at least L$[MIN_PRICE]. Please enter a higher price.
notification-classified-must-be-alphanumeric = The name of your classified must start with a letter from A to Z or a number. No punctuation is allowed.
notification-uploading-auction-snapshot = Uploading in-world and web site snapshots. (Takes about 5 minutes.)
notification-upload-web-snapshot-done = Web site snapshot upload done.
notification-upload-snapshot-done = In-world snapshot upload is done.
notification-flickr-connect = [MESSAGE]
notification-primfeed-connect = [MESSAGE]
notification-snapshot-to-computer-failed = Failed to save snapshot to [PATH]: Disk is full. [NEED_MEMORY]KB is required but only [FREE_MEMORY]KB is free.
notification-snapshot-to-local-dir-not-exist = Failed to save snapshot to [PATH]: Directory does not exist.
notification-cant-upload-postcard = Unable to upload postcard. Try again later.
notification-exodus-flickr-verification-explanation = To use the Flickr upload feature you must authorize the viewer to access your account. If you proceed, your web browser will open Flickr's website, where you will be prompted to log in and authorize the viewer. You will then be given a code to copy back into the viewer. Would you like to authorize the viewer to post to your Flickr account?
notification-exodus-flickr-verification-prompt = Please authorize the viewer to post to your Flickr account in your web browser, then type the code given by the website below:
notification-exodus-flickr-verification-failed = Flickr verification failed. Please try again, and be sure to double check the verification code.
notification-exodus-flickr-upload-complete = Your snapshot can now be viewed here].
notification-fs-primfeed-upload-complete = Your Primfeed post can now be viewed here.
notification-pick-limit-reached = Can't create another pick because the maximum number of picks have been created already.
notification-primfeed-login-request-failed = Login request denied by Primfeed.
notification-primfeed-authorization-failed = Primfeed authorization failed. The authorization sequence was not completed.
notification-primfeed-authorization-already-in-progress = Primfeed authorization is already in progress. Please complete the Primfeed authorization in your web browser before trying again.
notification-primfeed-authorization-successful = Primfeed authorization completed. You may now post images to Primfeed.
notification-primfeed-validate-failed = Primfeed user validation failed. Primfeed did not recognise this account, or the login failed.
notification-primfeed-already-authorized = You have already linked this account to Primfeed. Use the reset button if you wish to start over.
notification-primfeed-user-status-failed = Primfeed user login successful, but status checks have failed. Please check the Primfeed is working.

## Objects & edit (viewer-notification-catalogue-objects-edit). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-unable-to-view-contents-more-than-one = Unable to view the contents of more than one item at a time. Please select only one object and try again.
notification-return-objects-deeded-to-group = Are you sure you want to return all objects shared with the group '[NAME]' on this parcel of land back to their previous owner's inventory? *WARNING* This will delete the non-transferable objects deeded to the group! Objects: [N]
notification-return-objects-owned-by-user = Are you sure you want to return all objects owned by the resident '[NAME]' on this parcel of land back to their inventory? Objects: [N]
notification-return-objects-owned-by-self = Are you sure you want to return all objects owned by you on this parcel of land back to your inventory? Objects: [N]
notification-return-objects-not-owned-by-self = Are you sure you want to return all objects *NOT* owned by you on this parcel of land back to their owner's inventory? Transferable objects deeded to a group will be returned to their previous owners. *WARNING* This will delete the non-transferable objects deeded to the group! Objects: [N]
notification-return-objects-not-owned-by-user = Are you sure you want to return all objects *NOT* owned by [NAME] on this parcel of land back to their owner's inventory? Transferable objects deeded to a group will be returned to their previous owners. *WARNING* This will delete the non-transferable objects deeded to the group! Objects: [N]
notification-return-objects-not-owned-by-group = Return the objects on this parcel of land that are NOT shared with the group [NAME] back to their owners? Objects: [N]
notification-acquire-error-too-many-objects = ACQUIRE ERROR: Too many objects selected.
notification-acquire-error-object-span = ACQUIRE ERROR: Objects span more than one region. Please move all objects to be acquired onto the same region.
notification-unable-to-link-objects = Unable to link these [COUNT] objects. You can link a maximum of [MAX] objects.
notification-cannot-link-incomplete-set = You can only link complete sets of objects, and must select more than one object.
notification-cannot-link-modify = Unable to link because you do not have modify permission on all the objects. Please make sure none are locked, and that you own all of them.
notification-cannot-link-permanent = Objects cannot be linked across region boundaries.
notification-cannot-link-across-regions = Objects cannot be linked across region boundaries.
notification-cannot-link-different-owners = Unable to link because not all of the objects have the same owner. Please make sure you own all of the selected objects.
notification-model-uploader-missing-physics-apple = Model upload is not yet available on Apple Silicon, but will be supported in an upcoming release. Workaround: Right-click the the viewer app in Finder, select 'Get Info', then check 'Open using Rosetta'
notification-model-uploader-missing-physics = Physics library is not present, some of the model uploader's functionality might not work or might not work correctly.
notification-error-undefined-grasses = Error: Undefined grasses: [SPECIES]
notification-error-undefined-trees = Error: Undefined trees: [SPECIES]
notification-god-delete-all-scripted-public-objects-by-user = Are you sure you want to delete all scripted objects owned by ** [AVATAR_NAME] ** on all others land in this region?
notification-god-delete-all-scripted-objects-by-user = Are you sure you want to DELETE ALL scripted objects owned by ** [AVATAR_NAME] ** on ALL LAND in this region?
notification-god-delete-all-objects-by-user = Are you sure you want to DELETE ALL objects (scripted or not) owned by ** [AVATAR_NAME] ** on ALL LAND in this region?
notification-confirm-object-delete-lock = At least one of the items you have selected is locked. Are you sure you want to delete these items?
notification-confirm-object-delete-no-copy = At least one of the items you have selected is not copyable. Are you sure you want to delete these items?
notification-confirm-object-delete-no-own = You do not own at least one of the items you have selected. Are you sure you want to delete these items?
notification-confirm-object-delete-lock-no-copy = At least one object is locked. At least one object is not copyable. Are you sure you want to delete these items?
notification-confirm-object-delete-lock-no-own = At least one object is locked. You do not own at least one object. Are you sure you want to delete these items?
notification-confirm-object-delete-no-copy-no-own = At least one object is not copyable. You do not own at least one object. Are you sure you want to delete these items?
notification-confirm-object-delete-lock-no-copy-no-own = At least one object is locked. At least one object is not copyable. You do not own at least one object. Are you sure you want to delete these items?
notification-confirm-object-take-lock = At least one object is locked. Are you sure you want to take these items?
notification-confirm-object-take-no-own = You do not own all of the objects you are taking. If you continue, next owner permissions will be applied and possibly restrict your ability to modify or copy them. Are you sure you want to take these items?
notification-confirm-object-take-lock-no-own = At least one object is locked. You do not own all of the objects you are taking. If you continue, next owner permissions will be applied and possibly restrict your ability to modify or copy them. However, you can take the current selection. Are you sure you want to take these items?
notification-only-copy-contents-of-single-item = Unable to copy the contents of more than one item at a time. Please select only one object and try again.
notification-cant-set-buy-object = Cannot set 'Buy Object' because the object is not for sale. Please set the object for sale and try again.
notification-deed-object-to-group = Deeding this object will cause the group to: * Receive L$ paid into the object
notification-return-to-owner = Are you sure you want to return the selected objects to their owners? Transferable deeded objects will be returned to their previous owners. *WARNING* No-transfer deeded objects will be deleted!
notification-cant-modify-content-in-no-mod-task = You don't have permission to modify content of this object
notification-too-many-prims-selected = There are too many prims selected. Please select [MAX_PRIM_COUNT] or fewer prims and try again.
notification-too-many-scripts-selected = Too many scripts in the objects selected. Please select fewer objects and try again
notification-click-action-not-payable = Warning: The 'Pay object' click action has been set, but it will only work if a script is added with a money() event.
notification-confirm-unlink = Do you really want to unlink the selected object?
notification-material-missing = Material is missing from database.
notification-material-no-permissions = You don't have permission to view this material.
notification-material-images-were-scaled = One or more textures in this material were scaled to be within the allowed limits. Textures must have power of two dimensions and must not exceed [MAX_SIZE]x[MAX_SIZE] pixels.
notification-rez-item-no-permissions = Insufficient permissions to rez the object(s).
notification-unable-to-load-material = Unable to load material. Please try again.
notification-missing-material-caps = Not connected to a materials capable region.
notification-cant-select-reflection-probe = You have placed a reflection probe, but 'Select Reflection Probes' is disabled. To be able to select reflection probes, check Build > Options > Select Reflection Probes.
notification-unable-to-link-while-downloading = Unable to link while downloading object data. Please try again.
notification-owned-objects-returned = The objects you own on the selected parcel of land have been returned back to your inventory.
notification-other-objects-returned = The objects on the selected parcel of land that is owned by [NAME] have been returned to his or her inventory.
notification-other-objects-returned2 = The objects on the selected parcel of land owned by the resident '[NAME]' have been returned to their owner.
notification-group-objects-returned = The objects on the selected parcel of land shared with the group [GROUPNAME] have been returned back to their owner's inventory. Transferable deeded objects have been returned to their previous owners. Non-transferable objects that are deeded to the group have been deleted.
notification-un-owned-objects-returned = The objects on the selected parcel that are *NOT* owned by you have been returned to their owners.
notification-first-sandbox = This is a sandbox area, and is meant to help residents learn how to build. Things you build here will be deleted after you leave, so do not forget to right-click you items and choose 'Take' to move your creation into your inventory.
notification-mesh-upload-error-details = [LABEL] failed to upload: [MESSAGE] [DETAILS] See Firestorm.log for details
notification-mesh-upload-error = [LABEL] failed to upload: [MESSAGE] See Firestorm.log for details
notification-mesh-upload-perm-error = Error while requesting mesh upload permissons.
notification-cannot-upload-material = Unable to upload material file. The file may be corrupted, in an unsupported format, or contain invalid data. Please check that you're using a valid GLTF/GLB file with proper material definitions.
notification-save-material-as-default = [DESC]
notification-save-material-as = Name this material:
notification-invalid-material-name = Please enter a non-empty name
notification-usaved-material-changes = You have unsaved changes.
notification-live-preview-unavailable = We cannot display a preview of this texture because it is no-copy and/or no-transfer.
notification-live-preview-unavailable-pbr = We cannot display a preview of this material because it is no-copy, no-transfer, and/or no-modify.
notification-face-paste-failed = Paste failed. [REASON]
notification-failed-to-apply-texture-no-copy-to-multiple = Failed to apply texture. You can not apply a no-copy texture to multiple objects.
notification-failed-to-apply-gltf-no-copy-to-multiple = Failed to apply GLTF material. You can not apply a no-copy material to multiple objects.
notification-face-paste-texture-permissions = You applied a texture with limited permissions, object will inherit permissions from texture.
notification-pathfinding-linksets-warn-on-phantom = Some selected linksets will have the Phantom flag toggled. Do you wish to continue?
notification-pathfinding-linksets-mismatch-on-restricted = Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because of permission restrictions on the linkset. These linksets will be set to be '[RESTRICTED_TYPE]' instead. Do you wish to continue?
notification-pathfinding-linksets-mismatch-on-volume = Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because the shape is non-convex. Do you wish to continue?
notification-pathfinding-linksets-warn-on-phantom-mismatch-on-restricted = Some selected linksets will have the Phantom flag toggled. Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because of permission restrictions on the linkset. These linksets will be set to be '[RESTRICTED_TYPE]' instead. Do you wish to continue?
notification-pathfinding-linksets-warn-on-phantom-mismatch-on-volume = Some selected linksets will have the Phantom flag toggled. Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because the shape is non-convex. Do you wish to continue?
notification-pathfinding-linksets-mismatch-on-restricted-mismatch-on-volume = Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because of permission restrictions on the linkset. These linksets will be set to be '[RESTRICTED_TYPE]' instead. Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because the shape is non-convex. These linksets' use types will not change. Do you wish to continue?
notification-pathfinding-linksets-warn-on-phantom-mismatch-on-restricted-mismatch-on-volume = Some selected linksets will have the Phantom flag toggled. Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because of permission restrictions on the linkset. These linksets will be set to be '[RESTRICTED_TYPE]' instead. Some selected linksets cannot be set to be '[REQUESTED_TYPE]' because the shape is non-convex. These linksets' use types will not change. Do you wish to continue?
notification-pathfinding-linksets-change-to-flexible-path = The selected object affects the navmesh. Changing it to a Flexible Path will remove it from the navmesh.
notification-no-trans-no-save-to-contents = Cannot save '[OBJ_NAME]' to object contents because you do not have permission to transfer the object's ownership.
notification-pathfinding-return-multiple-items = You are returning [NUM_ITEMS] items. Are you sure you want to continue?
notification-pathfinding-delete-multiple-items = You are deleting [NUM_ITEMS] items. Are you sure you want to continue?
notification-now-own-object = You are now the owner of object [OBJECT_NAME]
notification-now-own-object-inv = You are now the owner of object [OBJECT_NAME] and it has been placed in your inventory.
notification-cant-rez-on-land = Can't rez object at [OBJECT_POS] because the owner of this land does not allow it. Use the land tool to see land ownership.
notification-rez-fail-too-many-requests = Object can not be rezzed because there are too many requests.
notification-no-new-object-region-full = Unable to create new object. The region is full.
notification-no-own-no-gardening = You can't create trees and grass on land you don't own.
notification-no-copy-perms-no-object = Copy failed because you lack permission to copy the object '[OBJ_NAME]'.
notification-no-trans-perms-no-object = Copy failed because the object '[OBJ_NAME]' cannot be transferred to you.
notification-add-to-nav-mesh-no-copy = Copy failed because the object '[OBJ_NAME]' contributes to navmesh.
notification-dupe-with-no-roots-selected = Duplicate with no root objects selected.
notification-cant-dupe-cuz-region-is-full = Can't duplicate objects because the region is full.
notification-cant-dupe-cuz-parcel-not-found = Can't duplicate objects - Can't find the parcel they are on.
notification-cant-create-cuz-parcel-full = Can't create object because the parcel is full.
notification-rez-attempt-failed = Attempt to rez an object failed.
notification-toxic-inv-rez-attempt-failed = Unable to create item that has caused problems on this region.
notification-inv-item-is-blacklisted = That inventory item has been blacklisted.
notification-no-can-rez-objects = You are not currently allowed to create objects.
notification-save-back-to-inv-disabled = Save Back To Inventory has been disabled.
notification-no-exist-no-save-to-contents = Cannot save '[OBJ_NAME]' to object contents because the object it was rezzed from no longer exists.
notification-no-mod-no-save-to-contents = Cannot save '[OBJ_NAME]' to object contents because you do not have permission to modify the object '[DEST_NAME]'.
notification-no-save-back-to-inv-disabled = Cannot save '[OBJ_NAME]' back to inventory -- this operation has been disabled.
notification-no-copy-no-sel-copy = You cannot copy your selection because you do not have permission to copy the object '[OBJ_NAME]'.
notification-no-trans-no-sel-copy = You cannot copy your selection because the object '[OBJ_NAME]' is not transferable.
notification-no-trans-no-copy = You cannot copy your selection because the object '[OBJ_NAME]' is not transferable.
notification-no-perms-no-removal = Removal of the object '[OBJ_NAME]' from the simulator is disallowed by the permissions system.
notification-no-mod-no-save-selection = Cannot save your selection because you do not have permission to modify the object '[OBJ_NAME]'.
notification-no-copy-no-save-selection = Cannot save your selection because the object '[OBJ_NAME]' is not copyable.
notification-no-mod-no-taking = You cannot take your selection because you do not have permission to modify the object '[OBJ_NAME]'.
notification-rez-dest-internal-error = Internal Error: Unknown destination type.
notification-delete-fail-obj-not-found = Delete failed because object not found
notification-cmo-parcel-full = Can't move object '[O]' to [P] in region [R] because the parcel is full.
notification-cmo-parcel-perms = Can't move object '[O]' to [P] in region [R] because your objects are not allowed on this parcel.
notification-cmo-parcel-resources = Can't move object '[O]' to [P] in region [R] because there are not enough resources for this object on this parcel.
notification-no-parcel-perms-no-object = Copy failed because you lack access to that parcel.
notification-cmo-region-version = Can't move object '[O]' to [P] in region [R] because the other region is running an older version which does not support receiving this object via region crossing.
notification-cmo-nav-mesh = Can't move object '[O]' to [P] in region [R] because you cannot modify the navmesh across region boundaries.
notification-cmowtf = Can't move object '[O]' to [P] in region [R] because of an unknown reason. ([F])
notification-no-perm-modify-object = You don't have permission to modify that object
notification-too-much-object-inventory-selected = Too many objects with large inventory are selected. Please select fewer objects and try again.
notification-cant-enable-phys-obj-contributes-to-nav = Can't enable physics for an object that contributes to the navmesh.
notification-cant-enable-phys-keyframed-obj = Can't enable physics for keyframed objects.
notification-cant-enable-phys-not-enough-land-resources = Can't enable physics for object -- insufficient land resources.
notification-cant-enable-phys-cost-too-great = Can't enable physics for object with physics resource cost greater than [MAX_OBJECTS]
notification-phantom-with-concave-piece = This object cannot have a concave piece because it is phantom and contributes to the navmesh.
notification-unable-add-item = Unable to add item!
notification-unable-edit-item = Unable to edit this!
notification-no-perm-to-edit = Not permitted to edit this.
notification-cant-save-item-doesnt-exist = Cannot save to object contents: Item no longer exists.
notification-cant-save-item-already-exists = Cannot save to object contents: Item with that name already exists in inventory
notification-cant-save-modify-attachment = Cannot save to object contents: This would modify the attachment permissions.
notification-asset-server-timeout-obj-return = Asset server didn't respond in a timely fashion. Object returned to the region.
notification-region-disable-physics-shapes = This region does not have physics shapes enabled.
notification-no-mod-navmesh-across-regions = You cannot modify the navmesh across region boundaries.
notification-no-set-physics-properties-on-object-type = Cannot set physics properties on that object type.
notification-no-set-root-prim-with-no-shape = Cannot set root prim to have no shape.
notification-no-region-support-phys-mats = This region does not have physics materials enabled.
notification-only-root-prim-phys-mats = Only root prims may have their physics materials adjusted.
notification-no-support-character-phys-mats = Setting physics materials on characters is not yet supported.
notification-invalid-phys-mat-property = One or more of the specified physics material properties was invalid.
notification-no-perms-alter-stitching-mesh-obj = You may not alter the stitching type of a mesh object.
notification-no-perms-alter-shape-mesh-obj = You may not alter the shape of a mesh object
notification-link-failed-owners-differ = Link failed -- owners differ
notification-link-failed-no-mod-navmesh-across-regions = Link failed -- cannot modify the navmesh across region boundaries.
notification-link-failed-no-perm-to-edit = Link failed because you do not have edit permission.
notification-link-failed-too-many-prims = Link failed -- too many primitives
notification-link-failed-cant-link-no-copy-no-trans = Link failed -- cannot link no-copy with no-transfer
notification-link-failed-nothing-linkable = Link failed -- nothing linkable.
notification-link-failed-too-many-pathfinding-chars = Link failed -- too many pathfinding characters
notification-link-failed-insufficient-land = Link failed -- insufficient land resources
notification-link-failed-too-much-physics = Object uses too many physics resources -- its dynamics have been disabled.
notification-cant-create-object-region-full = Unable to create requested object. The region is full.
notification-cant-create-animated-object-too-large = Unable to create requested animated object because it exceeds the rigged triangle limit.
notification-cant-create-multiple-obj-at-loc = You can't create multiple objects here.
notification-unable-to-create-obj-time-out = Unable to create requested object. Object is missing from database.
notification-unable-to-create-obj-unknown = Unable to create requested object. The request timed out. Please try again.
notification-unable-to-create-obj-missing-from-db = Unable to create requested object. Please try again.
notification-rez-failure-took-too-long = Rez failed, requested object took too long to load.
notification-failed-to-place-obj-at-loc = Failed to place object at specified location. Please try again.
notification-cant-create-plants-on-land = You cannot create plants on this land.
notification-cant-restore-object-no-world-pos = Cannot restore object. No world position found.
notification-cant-rez-object-invalid-mesh-data = Unable to rez object because its mesh data is invalid.
notification-cant-rez-object-too-many-scripts = Unable to rez object because there are already too many scripts in this region.
notification-cant-create-object-no-access = Your access privileges don't allow you to create objects there.
notification-cant-create-object = You are not currently allowed to create objects.
notification-invalid-object-params = Invalid object parameters
notification-cant-duplicate-object-no-access = Your access privileges don't allow you to duplicate objects here.
notification-cant-change-shape = You are not allowed to change this shape.
notification-no-perms-link-animated-object-too-large = Can't link these objects because the resulting animated object would exceed the rigged triangle limit.
notification-no-perms-set-flag-animated-object-too-large = Can't make this object into an animated object because it would exceed the rigged triangle limit.
notification-cant-change-animated-object-state-insufficient-land = Can't change animated object state for this object because it would cause parcel limit to be exceeded.
notification-error-no-mesh-data = Server error: cannot complete this operation because mesh data is not loaded.
notification-no-access-to-claim-objects = Your access privileges don't allow you to claim objects here.
notification-deed-failed-no-perm-to-deed-for-group = Deed failed because you do not have permission to deed objects for your group.
notification-cant-touch-object-banned-from-parcel = Can't touch/grab this object because you are banned from the land parcel.
notification-plz-narrow-delete-params = Please narrow your delete parameters.
notification-ten-objects-disabled-plz-refresh = Only the first 10 selected objects have been disabled. Refresh and make additional selections if required.
notification-cant-build-overflow-parcel = You cannot build objects here because doing so would overflow the parcel.
notification-claim-object-failed-no-permission = Claim object failed because you don't have permission
notification-cant-create-object-parcel-full = Can't create object because the parcel is full.
notification-failed-placing-object = Failed to place object at specified location. Please try again.
notification-cant-derez-inventory-error = Cannot derez object due to inventory fault.
notification-cant-find-object = Unable to find object.
notification-inventory-creation-in-world-object-failed = Inventory creation on in-world object failed.
notification-large-prim-agent-intersect = Cannot create large prims that intersect other residents. Please re-try when other residents have moved.
notification-default-object-permissions = There was a problem saving the default object permissions: [REASON]. Please try setting the default permissions later.
notification-export-finished = Export finished and saved to [FILENAME].
notification-export-failed = Export failed unexpectedly. Please see the log for details.
notification-export-collada-success = Successfully saved [OBJECT] to [FILENAME].
notification-export-collada-failure = Export of [OBJECT] to [FILENAME] failed.
notification-import-success = Successfully imported [COUNT] [OBJECT].
notification-cant-restore-to-world-no-copy = Restore to Last Position is not allowed for no copy items to prevent possible content loss.
notification-reflection-probe-applied = WARNING: You have made your object a Reflection Probe. This will implicitly change the object to mimic its influence volume and will make irreversible changes to the object. Do you want to continue?
notification-gltf-open-selection = You must select an object to act as a handle to the GLTF asset you are previewing.
notification-gltf-load-failed = Failed to load GLTF file. See log for details.
notification-gltf-save-failed = Failed to save GLTF file. See log for details.
notification-gltf-save-selection = You must select an object that has a GLTF asset associated with it.
notification-gltf-upload-selection = You must select an object that has local-only GLTF asset associated with it.
notification-gltf-upload-in-progress = Upload is currently in progress. Please try again later.
notification-water-exclusion-surfaces-warning = Checking the hide water box will overwrite the texture, bumpiness, and shininess choices.
notification-water-exclusion-no-material = Unable to apply material to the water exclusion surface.
notification-image-upload-resized = The texture you are uploading has been resized from [ORIGINAL_WIDTH]x[ORIGINAL_HEIGHT] to [NEW_WIDTH]x[NEW_HEIGHT] in order to to fit the maximum size of [MAX_WIDTH]x[MAX_HEIGHT] pixels.
notification-image-empty-alpha-layer = The image you are trying to upload contains an empty, or almost empty alpha channel (transparency information). This is almost always not desired and should be stripped off. Adding an alpha channel to an image will lead to textures flipping on top of each other at different camera angles, and it makes rendering slower. So, unless you really need this texture to have an empty / almost empty alpha channel, consider stripping it out.

## IM & chat (viewer-notification-catalogue-im-chat). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-friends-and-groups-only = Non-friends won't know that you've chosen to ignore their calls and instant messages.
notification-add-auto-replace-list = Name for the new list:
notification-rename-auto-replace-list = The name '[DUPNAME]' is in use Enter a new unique name:
notification-remove-auto-replace-list = '[LIST_NAME]' contains [MAP_SIZE] entries. Are you sure you want to delete this list?
notification-invalid-auto-replace-entry = The keyword must be a single word, and the replacement may not be empty. Allowed punctuation in keywords: ( ) . , - _
notification-invalid-auto-replace-list = That replacement list is not valid.
notification-do-not-disturb-mode-set = Unavailable mode is on. You will not be notified of incoming communications. - Other residents will receive your Unavailable mode response (set in Preferences > Privacy > Autoresponse). - Voice calls will be rejected.
notification-autorespond-mode-set = Autorespond mode is on. Incoming instant messages will now be answered with your configured autoresponse.
notification-autorespond-non-friends-mode-set = Autorespond mode for non-friends is on. Incoming instant messages from anyone who is not your friend will now be answered with your configured autoresponse.
notification-confirm-adding-chat-participants = When you add a person to an existing conversation, a new conversation will be created. All participants will receive new conversation notifications.
notification-chatter-box-session-start-error = Unable to start a new chat session with [RECIPIENT]. [REASON]
notification-chatter-box-session-event-error = [EVENT]
notification-force-close-chatter-box-session = Your chat session with [NAME] must close. [REASON]
notification-chat-system-message-tip = [MESSAGE]
notification-im-system-message-tip = [MESSAGE]
notification-im-across-parent-estates = Unable to send IM across parent estates.
notification-object-message = [NAME]: [MESSAGE]
notification-server-object-message = Message from [NAME]: [MSG]
notification-invite-ad-hoc = [NAME] is inviting you to a conference chat. Click Accept to join the chat or Decline to decline the invitation. Click mute to permanently block all messages this caller.
notification-im-toast = [MESSAGE]
notification-confirm-close-all = Are you sure you want to close all IMs?
notification-text-chat-is-muted-by-moderator = Your text chat has been muted by a moderator.
notification-chat-history-is-busy-alert = Chat history file is busy with previous operation. Please try again in a few minutes or choose chat with another person.
notification-preference-chat-clear-log = This will delete the logs of previous conversations, and any backups of that file.
notification-preference-chat-delete-transcripts = This will delete the transcripts for all previous conversations. The list of past conversations will not be affected. All files with the suffixes .txt and txt.backup in the folder [FOLDER] will be deleted.
notification-preference-chat-path-changed = Unable to move files. Restored previous path.
notification-anti-spam-blocked = AntiSpam: Blocked [SOURCE] for spamming a [QUEUE] ([COUNT]) times in [PERIOD] seconds.
notification-anti-spam-im-new-line-flood-blocked = AntiSpam: Blocked [SOURCE] for sending an instant message with more than [COUNT] lines.
notification-anti-spam-chat-new-line-flood-blocked = AntiSpam: Blocked [SOURCE] for sending a chat message with more than [COUNT] lines.
notification-snooze-duration-default = [DURATION]
notification-snooze-duration = Time in seconds to snooze group chat:
notification-snooze-duration-invalid-input = Please enter a valid number for the snooze duration!
notification-chat-history-is-missing = Chat transcript file is missing. Either there are no transcripts for this chat, or transcripts aren't being saved. You can enable saving transcripts under Preferences > Privacy > Logs & Transcripts

## Preferences (viewer-notification-catalogue-preferences). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-favorites-on-login = Note: When you turn on this option, anyone who uses this computer can see your list of favorite locations.
notification-cache-will-clear = Cache will be cleared after restarting the viewer.
notification-disable-javascript-breaks-search = If you disable Javascript, the search function will not work properly, and you will not be able to use it.
notification-cache-will-be-moved = Cache will be moved after restarting the viewer. Note: This will also clear the cache.
notification-sound-cache-will-be-moved = Sound cache will be moved after restarting the viewer.
notification-change-connection-port = Port settings take effect after restarting the viewer.
notification-change-deferred-debug-setting = This debug setting change will take effect after you restart the viewer.
notification-change-skin = The new skin will appear after restarting the viewer. Would you like to shutdown the viewer and launch it manually again in order to apply this change?
notification-change-language = The selected language or time format will be applied after restarting the viewer.
notification-auto-fps-confirm-disable = Changing this setting will disable automatic adjustment and turn off 'Automatic settings'. Are you sure you want to continue?
notification-advanced-lighting-confirm = To turn on advanced lighting, we need to increase quality to level 4.
notification-shadows-confirm = To enable shadows, we need to increase quality to level 4.
notification-resolution-switch-fail = Failed to switch resolution to [RESX] by [RESY].
notification-spelling-dict-import-required = You must specify a file, a name, and a language.
notification-spelling-dict-is-secondary = The dictionary [DIC_NAME] does not appear to have an "aff" file; this means that it is a "secondary" dictionary. It can be used as an additional dictionary, but not as your Main dictionary. See https://wiki.secondlife.com/wiki/Adding_Spelling_Dictionaries
notification-spelling-dict-import-failed = Unable to copy [FROM_NAME] to [TO_NAME]
notification-display-set-to-safe = Display settings have been set to safe levels because you have specified the -safe option.
notification-display-set-to-recommended-gpu-change = Display settings have been set to recommended levels because your graphics card changed from '[LAST_GPU]' to '[THIS_GPU]'
notification-display-set-to-recommended-feature-change = Display settings have been set to recommended levels because of a change to the rendering subsystem.
notification-region-entry-access-blocked-adults-only-content = The region you're trying to visit contains [REGIONMATURITY] content, which is accessible to adults only.
notification-region-entry-access-blocked-change = The region you're trying to visit contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content. We can change your preferences, or you can cancel. After your preferences are changed, you may attempt to enter the region again.
notification-preferred-maturity-changed = You won't receive any more notifications that you're about to visit a region with [RATING] content. You may change your content preferences in the future by using Avatar > Preferences > General from the menu bar.
notification-maturity-change-error = We were unable to change your preferences to view [PREFERRED_MATURITY] content at this time. Your preferences have been reset to view [ACTUAL_MATURITY] content. You may attempt to change your preferences again by using Avatar > Preferences > General from the menu bar.
notification-confirm-restore-toybox = This action will restore your default buttons and toolbars. You cannot undo this action.
notification-confirm-clear-all-toybox = This action will return all buttons to the toolbox and your toolbars will be empty. You cannot undo this action.
notification-confirm-clear-browser-cache = Are you sure you want to delete your travel, web, and search history?
notification-confirm-clear-cache = Are you sure you want to clear your viewer cache?
notification-confirm-clear-inventory-cache = Are you sure you want to clear your inventory cache?
notification-confirm-clear-web-browser-cache = Are you sure you want to clear your web browser cache (Requires Restart)?
notification-confirm-clear-cookies = Are you sure you want to clear your cookies?
notification-confirm-clear-media-url-list = Are you sure you want to clear your list of saved URLs?
notification-wl-save-preset-alert = Do you wish to overwrite the saved preset?
notification-wl-no-edit-default = You cannot edit or delete a default preset.
notification-wl-missing-sky = This day cycle file references a missing sky file: [SKY].
notification-wl-region-apply-fail = Sorry, the settings couldn't be applied to the region. Reason: [FAIL_REASON]
notification-wl-local-texture-day-block = A Local texture is in use on track [TRACK], frame #[FRAMENO] ([FRAME]%) in field [FIELD]. Settings may not be saved using local textures.
notification-wl-local-texture-fixed-block = A local texture is in use in field [FIELD]. Settings may not be saved using local textures.
notification-env-cannot-delete-last-day-cycle-key = Unable to delete the last key in this day cycle because you cannot have an empty day cycle. You should modify the last remaining key instead of attempting to delete it and then to create a new one.
notification-day-cycle-too-many-keyframes = You cannot add any more keyframes to this day cycle. The maximum number of keyframes for day cycles of [SCOPE] scope is [MAX].
notification-env-update-rate = You may only update region environmental settings every [WAIT] seconds. Wait at least that long and then try again.
notification-pp-save-effect-alert = PostProcess Effect exists. Do you still wish overwrite it?
notification-preset-not-saved = Error saving preset [NAME].
notification-default-preset-not-saved = Can not overwrite default preset.
notification-preset-already-exists = '[NAME]' is in use. You may replace this preset or choose another name.
notification-preset-not-deleted = Error deleting preset [NAME].
notification-bottom-tray-button-can-not-be-shown = Selected button cannot be shown right now. The button will be shown when there is enough space for it.
notification-socks-bad-creds = Invalid SOCKS 5 username or password.
notification-proxy-invalid-http-host = Invalid HTTP proxy address or port "[HOST]:[PORT]".
notification-proxy-invalid-socks-host = Invalid SOCKS proxy address or port "[HOST]:[PORT]".
notification-change-proxy-settings = Proxy settings take effect after you restart the viewer.
notification-mode-change = Changing modes requires you to quit and restart. Change mode and quit?
notification-no-classifieds = Creation and editing of Classifieds is only available in Advanced mode. Would you like to quit and change modes? The mode selector can be found on the login screen.
notification-no-group-info = Creation and editing of Groups is only available in Advanced mode. Would you like to quit and change modes? The mode selector can be found on the login screen.
notification-no-place-info = Viewing place profile is only available in Advanced mode. Would you like to quit and change modes? The mode selector can be found on the login screen.
notification-no-picks = Creation and editing of Picks is only available in Advanced mode. Would you like to quit and change modes? The mode selector can be found on the login screen.
notification-no-world-map = Viewing of the world map is only available in Advanced mode. Would you like to quit and change modes? The mode selector can be found on the login screen.
notification-no-voice-call = Voice calls are only available in Advanced mode. Would you like to logout and change modes?
notification-no-avatar-share = Sharing is only available in Advanced mode. Would you like to logout and change modes?
notification-no-avatar-pay = Paying other residents is only available in Advanced mode. Would you like to logout and change modes?
notification-no-inventory = Viewing inventory is only available in Advanced mode. Would you like to logout and change modes?
notification-no-appearance = The appearance editor is only available in Advanced mode. Would you like to logout and change modes?
notification-no-search = Search is only available in Advanced mode. Would you like to logout and change modes?
notification-confirm-hide-ui = This action will hide all menu items and buttons. To get them back, click [SHORTCUT] again.
notification-confirm-clear-debug-search-url = Are you sure you want to clear the debug search url?
notification-confirm-pick-debug-search-url = Are you sure you want to pick the current search url as debug search url?
notification-firestorm-clear-settings-prompt = Resetting all settings may be helpful if you are experiencing problems; however, you will need to redo any customizations you have made to the default configuration. Are you sure you want to reset all settings?
notification-settings-will-clear = Settings will be cleared after restarting the viewer.
notification-debug-settings-warning = Warning! The use of the Debug Settings window is unsupported! Changing debug settings can severely impact your experience and might lead to loss of data, functionality or even access to the service. Please do not change any values without knowing exactly what you are doing.
notification-control-name-copied-to-clipboard = This debug setting's name has been copied to your clipboard. You can now paste it somewhere else to use it.
notification-sanity-check = The viewer has detected a possible issue with your settings: [SANITY_MESSAGE] Reason: [SANITY_COMMENT] Current setting: [CURRENT_VALUE]
notification-cache-empty = Your viewer cache is currently empty. Please be aware that you may experience slow framerates and inventory loading for a short time while new content downloads.
notification-preference-controls-defaults = Do you want to restore default values for controls?
notification-preference-quality-with-low-memory = Your system has [TOTAL_MEM]MB of memory, which might not be enough to run viewer at higher settings and might result in issues.
notification-fsbw-too-high = We strongly recommend that you not set the bandwidth above 1500 KBPS. This is unlikely to work well and will almost certainly not improve your performance.
notification-backup-finished = Your settings have been backed up.
notification-backup-path-empty = The backup path is empty. Please provide a location to back up and restore your settings first.
notification-backup-path-does-not-exist-or-create-failed = The backup path could not be found or created.
notification-backup-path-does-not-exist = The backup path could not be found.
notification-settings-confirm-backup = Are you sure you want to save a backup to this directory? [DIRECTORY] Any existing backups in that location will be overwritten!
notification-settings-restore-needs-logout = Settings restore requires a viewer restart. Do you want to restore your settings and quit the viewer now?
notification-restore-finished = Restore complete! Please restart your viewer now.
notification-confirm-restore-quick-prefs-defaults = This action will immediately restore your quick preferences to their default settings. You cannot undo this action.
notification-quick-prefs-duplicate-control = Setting has already been added. Please select a different one.
notification-mesh-max-concurrent-req-too-high = The value you set, [VALUE], for the number of concurrent requests to load mesh objects (debug setting [DEBUGNAME]) is higher than the maximum of [MAX]. It has been reset to the default of [DEFAULT].
notification-skin-defaults-change-settings = [MESSAGE]
notification-render-volume-lod-factor-warning = WARNING: The Level of Detail (LOD) Factor is set high For everyday use, LOD Factor in the range of 1-3 suffices. Consider replacing objects that look deformed at such values. LOD Factor >3: Adds to lag. Advised only for photography. LOD Factor >4: Use in special circumstances. Reverts after relog. LOD Factor >8: Has no real effect. May cause errors.
notification-override-vram-warning = WARNING: Overriding the VRAM detection may cause instability. Most users should leave this setting disabled and let the viewer and operating system determine the correct value. This setting is intended for cases where VRAM detection is reporting incorrect values. Use with caution, seek support advice in case of doubt.
notification-enable-hi-dpi = Enabling HiDPI support may have adverse effects and may impair performance.
notification-failed-to-find-settings = Could not load the settings for [NAME] from the database.
notification-failed-to-load-settings-apply = Unable to apply those settings to the environment.
notification-failed-to-build-settings-day = Unable to apply those settings to the environment.
notification-no-environment-settings = This Region does not support environmental settings.
notification-save-setting-as-default = [DESC] (new)
notification-save-setting-as = Save current environmental settings as:
notification-wl-import-fail = Unable to import legacy Windlight settings [NAME] from [FILE]. [REASONS]
notification-wl-parcel-apply-fail = Unable to set the environment for this parcel. Please enter or select a parcel that you have rights to modify.
notification-settings-unsupported = Settings are not supported on this region. Please move to a settings enabled region and retry your action.
notification-settings-confirm-loss = You are about to lose the changes you have made to this [TYPE] named "[NAME]". Are you sure you want to continue?
notification-settings-confirm-reset = You are about to remove all applied settings. Are you sure you want to continue?
notification-personal-settings-confirm-reset = You are about to remove all applied Personal lighting settings. Are you sure you want to continue?
notification-settings-make-no-trans = You are about to import non-transferable settings into this daycycle. Continuing will cause the settings you are editing to become non-transferable also. This change can not be undone. Are you sure you want to continue?
notification-no-edit-from-library = You may not edit settings directly from the libary. Please copy to your own inventory and try again.
notification-environment-apply-failed = We have encountered an issue with these settings. They can not be saved or applied at this time.
notification-track-load-failed = Unable to load the track into [TRACK].
notification-track-load-mismatch = Unable to load the track from [TRACK1] into [TRACK2].
notification-auto-adjust-hdr-sky = You are editing a non-HDR sky that has been automatically converted to HDR. To remove HDR and tone mapping, set Reflection Probe Ambiance to zero.
notification-enable-auto-fps-warning = You are about to enable AutoFPS. All unsaved graphics settings will be lost. Would you like to save them first?
notification-no-valid-env-setting-found = No valid environment setting selected. Please note that "Shared Environment" and "Day cycle based" cannot be selected!
notification-windlight-bulk-import-finished = Bulk import of Windlights has finished.

## Friends & people (viewer-notification-catalogue-friends-people). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-grant-modify-rights = Granting modify rights to another resident allows them to change, delete or take ANY objects you may have in-world. Be VERY careful when handing out this permission. Do you want to grant modify rights for [NAME]?
notification-grant-modify-rights-multiple = Granting modify rights to another Resident allows them to change ANY objects you may have in-world. Be VERY careful when handing out this permission. Do you want to grant modify rights for the selected Residents?
notification-revoke-modify-rights = Do you want to revoke modify rights for [NAME]?
notification-revoke-modify-rights-multiple = Do you want to revoke modify rights for the selected Residents?
notification-mute-limit-reached = Unable to add new entry to block list because you reached the limit of [MUTE_LIMIT] entries.
notification-add-friend-with-message-default = Would you be my friend?
notification-add-friend-with-message = Friends can give permissions to track each other on the map and receive online status updates. Offer friendship to [NAME]?
notification-remove-multiple-from-friends = Are you sure you want to remove multiple friends from your Friends list?
notification-revoked-modify-rights = Your privilege to modify [NAME]'s objects has been revoked.
notification-mute-linden = Sorry, you cannot block a Linden.
notification-mute-by-name-failed = You already have blocked/muted this name.
notification-cant-offer-calling-card = Cannot offer a calling card at this time. Please try again in a moment.
notification-cant-offer-friendship = Cannot offer friendship at this time. Please try again in a moment.
notification-reject-friendship-requests-mode-set = Reject all incoming friendship requests mode is on. Incoming friendship requests from anyone will now be rejected with your configured autoresponse. You will not be notified because of that fact.
notification-friend-online-offline = [NAME] is [STATUS].
notification-add-self-friend = Although you're very nice, you can't add yourself as a friend.
notification-add-self-render-exceptions = You can't add yourself to the rendering exceptions list.
notification-offered-card = You have offered a calling card to [NAME].
notification-calling-card-accepted = Your calling card was accepted.
notification-calling-card-declined = Your calling card was declined.
notification-offer-friendship-no-message = [NAME_SLURL] is offering friendship. (By default, you will be able to see each other's online status.)
notification-friendship-accepted = [NAME] accepted your friendship offer.
notification-friendship-declined = [NAME] declined your friendship offer.
notification-friendship-accepted-by-me = Friendship offer accepted.
notification-friendship-declined-by-me = Friendship offer declined.
notification-auto-unmute-by-im = [NAME] was sent an instant message and has been automatically unblocked.
notification-auto-unmute-by-money = [NAME] was given money and has been automatically unblocked.
notification-auto-unmute-by-inventory = [NAME] was offered inventory and has been automatically unblocked.
notification-zoom-to-avatar-not-possible = Cannot zoom to this avatar, because it is out of reach.
notification-track-avatar-not-possible = Cannot track this avatar, because it is beyond radar range.
notification-radar-alert = [NAME] [MESSAGE]
notification-add-new-contact-set-default = New Contact Set
notification-add-new-contact-set = Create new contact set with the name:
notification-remove-contact-set = Are you sure you want to remove [SET_NAME]? You won't be able to restore it.
notification-remove-contact-from-set = Are you sure you want to remove [TARGET] from [SET_NAME]?
notification-remove-contacts-from-set = Are you sure you want to remove these [TARGET] avatars from [SET_NAME]?
notification-add-to-contact-set-single-success = [NAME] was added to [SET].
notification-add-to-contact-set-multiple-success = [COUNT] avatars were added to [SET].
notification-set-avatar-pseudonym = Enter an alias for [AVATAR]:
notification-set-avatar-pseudonym-multiple = Enter an alias for [COUNT] avatars:
notification-rename-contact-set-failure = Could not rename set '[SET]' to '[NEW_NAME]' because a set with the same name already exists or the new name is invalid.
notification-confirm-global-online-status-toggle = Are you sure you want to change your online status visibility for all friends at once? Due to server load, this mass change can take a while to become effective and may cause temporary issues for some friends seeing your online status.
notification-global-online-status-toggle = Due to server load, mass toggling online status visibility can take a while to become effective. Please be patient.

## Groups (viewer-notification-catalogue-groups). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-group-name-length-warning = A group name must be between [MIN_LEN] and [MAX_LEN] characters.
notification-unable-to-create-group = Unable to create group. [MESSAGE]
notification-panel-group-apply = [NEEDS_APPLY_MESSAGE] [WANT_APPLY_MESSAGE]
notification-must-specify-group-notice-subject = You must specify a subject to send a group notice.
notification-add-group-owner-warning = You are about to add group members to the role of [ROLE_NAME]. Members cannot be removed from that role. The members must resign from the role themselves. Are you sure you want to continue?
notification-assign-dangerous-action-warning = You are about to add the Ability '[ACTION_NAME]' to the Role '[ROLE_NAME]'. *WARNING* Any Member in a role with this ability can assign themselves -- and any other member -- to roles that have more powers than they currently have, potentially elevating themselves to near-Owner power. Be sure you know what you are doing before assigning this ability. Add this ability to '[ROLE_NAME]'?
notification-assign-dangerous-ability-warning = You are about to add the ability '[ACTION_NAME]' to the role '[ROLE_NAME]'. *WARNING* Any Member in a role with this ability can assign themselves -- and any other member -- all abilities, elevating themselves to near-Owner power. Add this ability to '[ROLE_NAME]'?
notification-assign-ban-ability-warning = You are about to add the Ability '[ACTION_NAME]' to the Role '[ROLE_NAME]'. *WARNING* Any Member in a Role with this Ability will also be granted the Abilities '[ACTION_NAME_2]' and '[ACTION_NAME_3]'
notification-remove-ban-ability-warning = You are removing the Ability '[ACTION_NAME]' to the Role '[ROLE_NAME]'. *WARNING* Removing this ability will NOT remove the Abilities '[ACTION_NAME_2]' and '[ACTION_NAME_3]'. If you no longer wish to have these abilities granted to this role, disable them immediately!
notification-eject-group-member-warning = You are about to eject [AVATAR_NAME] from the group.
notification-eject-group-members-warning = You are about to eject [COUNT] members from the group.
notification-ban-group-member-warning = You are about to ban [AVATAR_NAME] from the group.
notification-ban-group-members-warning = You are about to ban [COUNT] members from group.
notification-group-ban-user-on-banlist = Some residents have not been sent an invite due to being banned from the group.
notification-join-group-can-afford = Joining this group costs L$[COST]. Do you wish to proceed?
notification-join-group-no-cost = You are joining group [NAME]. Do you wish to proceed?
notification-join-group-cannot-afford = Joining this group costs L$[COST]. You do not have enough L$ to join this group.
notification-create-group-cost = Creating this group will cost L$[COST]. Groups need more than one member, or they are deleted forever. Please invite members within 48 hours.
notification-join-group-inaccessible = This group is not accessible to you.
notification-join-group-error = Error processing your group membership request.
notification-join-group-error-reason = Unable to join group: [reason]
notification-join-group-trial-user = Sorry, trial users can't join groups.
notification-join-group-max-groups = You cannot join '[group_name]': You are already a member of [group_count] groups, the maximum number allowed is [max_groups]
notification-join-group-closed-enrollment = You cannot join '[group_name]': The group no longer has open enrollment.
notification-join-group-insufficient-funds = Unable to transfer the required L$ [membership_fee] membership fee.
notification-select-proposal-to-view = Please select a proposal to view.
notification-select-history-item-to-view = Please select a history item to view.
notification-eject-avatar-from-group = You ejected [AVATAR_NAME] from group [GROUP_NAME].
notification-group-leave-confirm-member-no-fee = Leave the group '[GROUP]'? There is currently no fee to join this group again.
notification-owner-cannot-leave-group = Unable to leave group. You cannot leave the group because you are the last owner of the group. Please assign another member to the owner role first.
notification-group-depart-error = Unable to leave group.
notification-reject-all-group-invites-mode-set = Reject all incoming group invites mode is on. Incoming group invites from anyone will now be rejected automatically. You will not be notified because of that fact.
notification-joined-too-many-groups-member = You have reached your maximum number of groups. Please leave another group before joining this one, or decline the offer. [NAME] has invited you to join a group as a member.
notification-joined-too-many-groups = You have reached your maximum number of groups. Please leave some group before joining or creating a new one.
notification-group-limit-info = Residents with Basic memberships may join up to [MAX_BASIC] groups. Premium memberships allow up to [MAX_PREMIUM]. Learn more or upgrade
notification-group-limit-info-plus = Residents with Basic memberships may join up to [MAX_BASIC] groups. Premium memberships allow up to [MAX_PREMIUM]. Premium Plus memberships allow up to [MAX_PREMIUM_PLUS]. Learn more or upgrade
notification-set-group-mature = Does this group contain Moderate content?
notification-join-group = [MESSAGE]
notification-fs-set-title-region = Enter the exact name of a region to assign to the selected group title. The name will be verified before the assignment is saved.
notification-fs-set-title-region-not-found = The region '[REGION]' could not be found. Please check the name and try again.
notification-first-join-support-group2 = Welcome to the Phoenix/Firestorm Viewer Support Group! To make support easier, it is recommended to announce your viewer's version to the group. This information includes current viewer version, viewer skin, operating system and RLVa status. You can choose to display your viewer's version in front of any chat you send to the group. Our support members can give you more meaningful advice right away if they know the viewer version you are on. You can enable and disable this function at any time using the checkbox in the group chat floater. Do you want to enable the automatic viewer version display?
notification-cant-fetch-inventory-for-group-notice = Unable to fetch inventory details for the group notice.
notification-cant-send-group-notice-not-permitted = Unable to send group notice -- not permitted.
notification-cant-send-group-notice-cant-construct-inventory = Unable to send group notice -- could not construct inventory.
notification-cant-parce-inventory-in-notice = Unable to parse inventory in notice.

## Land & parcel (viewer-notification-catalogue-land-parcel). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-land-buy-pass = For L$[COST] you can enter this land ('[PARCEL_NAME]') for [TIME] hours. Buy a pass?
notification-sale-price-restriction = Sale price must be set to more than L$0 if selling to anyone. Please select an individual to sell to if selling for L$0.
notification-confirm-land-sale-change = The selected [LAND_SIZE] m² land is being set for sale. Your selling price will be L$[SALE_PRICE] and will be authorized for sale to [NAME].
notification-confirm-land-sale-to-anyone-change = ATTENTION: Clicking 'Sell to anyone' makes your land available to the entire [CURRENT_GRID] community, even those not in this region. The selected [LAND_SIZE] m² land is being set for sale. Your selling price will be L$[SALE_PRICE] and will be authorized for sale to [NAME].
notification-must-be-in-parcel = You must be standing inside the land parcel to set its landing point.
notification-go-to-auction-page = Go to the [CURRENT_GRID] web page to see auction details or make a bid?
notification-parcel-no-terraforming = You are not allowed to terraform parcel [PARCEL].
notification-freeze-avatar = Freeze this avatar? He or she will temporarily be unable to move, chat, or interact with the world.
notification-freeze-avatar-fullname = Freeze [AVATAR_NAME]? He or she will temporarily be unable to move, chat, or interact with the world.
notification-freeze-avatar-multiple = Freeze the following avatars? [RESIDENTS] They will temporarily be unable to move, chat, or interact with the world.
notification-eject-avatar-fullname = Eject [AVATAR_NAME] from your land?
notification-eject-avatar-no-ban = Eject this avatar from your land?
notification-eject-avatar-fullname-no-ban = Eject [AVATAR_NAME] from your land?
notification-eject-avatar-multiple = Eject the following avatars from your land? [RESIDENTS]
notification-eject-avatar-multiple-no-ban = Eject the following avatars from your land? [RESIDENTS]
notification-cannot-set-land-owner-nothing-selected = Unable to set land owner: No parcel selected.
notification-cannot-set-land-owner-multiple-regions = Unable to force land ownership because selection spans multiple regions. Please select a smaller area and try again.
notification-force-owner-auction-warning = This parcel is up for auction. Forcing ownership will cancel the auction and potentially make some residents unhappy if bidding has begun. Force ownership?
notification-cannot-contentify-nothing-selected = Unable to contentify: No parcel selected.
notification-cannot-contentify-no-region = Unable to contentify: No region selected.
notification-cannot-release-land-nothing-selected = Unable to abandon land: No parcel selected.
notification-cannot-release-land-no-region = Unable to abandon land: Cannot find region.
notification-cannot-buy-land-nothing-selected = Unable to buy land: No parcel selected.
notification-cannot-buy-land-no-region = Unable to buy land: Cannot find the region this land is in.
notification-cannot-close-floater-buy-land = You cannot close the Buy Land window until the viewer estimates the price of this transaction.
notification-cannot-deed-land-nothing-selected = Unable to deed land: No parcel selected.
notification-cannot-deed-land-no-group = Unable to deed land: No Group selected.
notification-cannot-deed-land-no-region = Unable to deed land: Cannot find the region this land is in.
notification-cannot-deed-land-multiple-selected = Unable to deed land: Multiple parcels selected. Try selecting a single parcel.
notification-cannot-deed-land-waiting-for-server = Unable to deed land: Waiting for server to report ownership. Please try again.
notification-cannot-deed-land-no-transfer = Unable to deed land: The region [REGION] does not allow transfer of land.
notification-cannot-release-land-waiting-for-server = Unable to abandon land: Waiting for server to update parcel information. Try again in a few seconds.
notification-cannot-release-land-selected = Unable to abandon land: You do not own all the parcels selected. Please select a single parcel.
notification-cannot-release-land-dont-own = Unable to abandon land: You do not have permission to release this parcel. Parcels you own appear in green.
notification-cannot-release-land-region-not-found = Unable to abandon land: Cannot find the region this land is in.
notification-cannot-release-land-no-transfer = Unable to abandon land: The region [REGION] does not allow transfer of land.
notification-cannot-release-land-partial-selection = Unable to abandon land: You must select an entire parcel to release it. Select an entire parcel, or divide your parcel first.
notification-release-land-warning = You are about to release [AREA] m² of land. Releasing this parcel will remove it from your land holdings, but will not grant any L$. Release this land?
notification-cannot-divide-land-nothing-selected = Unable to divide land: No parcels selected.
notification-cannot-divide-land-partial-selection = Unable to divide land: You have an entire parcel selected. Try selecting a part of the parcel.
notification-land-divide-warning = Dividing this land will split this parcel into two and each parcel can have its own settings. Some settings will be reset to defaults after the operation. Divide land?
notification-cannot-divide-land-no-region = Unable to divide land: Cannot find the region this land is in.
notification-cannot-join-land-no-region = Unable to join land: Cannot find the region this land is in.
notification-cannot-join-land-nothing-selected = Unable to join land: No parcels selected.
notification-cannot-join-land-entire-parcel-selected = Unable to join land: You only have one parcel selected. Select land across both parcels.
notification-cannot-join-land-selection = Unable to join land: You must select more than one parcel. Select land across both parcels.
notification-join-land-warning = Joining this land will create one large parcel out of all parcels intersecting the selected rectangle. You will need to reset the name and options of the new parcel. Join land?
notification-only-officer-can-buy-land = Unable to buy land for the group: You do not have permission to buy land for your active group.
notification-cant-buy-land-across-multiple-regions = Unable to buy land because selection spans multiple regions. Please select a smaller area and try again.
notification-deed-land-to-group = By deeding this parcel, the group will be required to have and maintain sufficient land use credits. The purchase price of the land is not refunded to the owner. If a deeded parcel is sold, the sale price will be divided evenly among group members. Deed this [AREA] m² of land to the group '[GROUP_NAME]'?
notification-deed-land-to-group-with-contribution = By deeding this parcel, the group will be required to have and maintain sufficient land use credits. The deed will include a simultaneous land contribution to the group from '[NAME]'. The purchase price of the land is not refunded to the owner. If a deeded parcel is sold, the sale price will be divided evenly among group members. Deed this [AREA] m² of land to the group '[GROUP_NAME]'?
notification-cannot-start-auction-already-for-sale = You cannot start an auction on a parcel which is already set for sale. Disable the land sale if you are sure you want to start an auction.
notification-land-claim-access-blocked-adults-only-content = Only adults can claim this land.
notification-land-claim-access-blocked-notify = The land you're trying to claim contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content.
notification-land-claim-access-blocked-notify-adults-only = The land you're trying to claim contains [REGIONMATURITY] content, which is accessible to adults only.
notification-land-claim-access-blocked-change = The land you're trying to claim contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content. We can change your preferences, then you can try claiming the land again.
notification-land-buy-access-blocked-adults-only-content = Only adults can buy this land.
notification-land-buy-access-blocked-notify = The land you're trying to buy contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content.
notification-land-buy-access-blocked-notify-adults-only = The land you're trying to buy contains [REGIONMATURITY] content, which is accessible to adults only.
notification-land-buy-access-blocked-change = The land you're trying to buy contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content. We can change your preferences, then you can try buying the land again.
notification-cant-select-land-from-multiple-regions = Selected land is not all in the same region. Try selecting a smaller piece of land.
notification-transfer-objects-highlighted = All objects on this parcel that will transfer to the purchaser of this parcel are now highlighted. * Trees and grasses that will transfer are not highlighted.
notification-not-safe = This land has damage enabled. You can be hurt here. If you die, you will be teleported to your home location.
notification-no-fly = This area has flying disabled. You cannot fly here.
notification-push-restricted = This area does not allow pushing. You can't push others here unless you own the land.
notification-no-voice = This area has voice chat disabled. You will not be able to use voice chat here.
notification-no-build = This area has building disabled. You can't build or rez objects here.
notification-see-avatars = This parcel hides avatars and text chat from another parcel. You can't see other residents outside the parcel, and those outside are not able to see you. Regular text chat on channel 0 is also blocked.
notification-claim-public-land = You can only claim public land that is in the same region as you.
notification-must-get-age-parcel = You must be age 18 or over to enter this parcel.
notification-cannot-enter-parcel-not-a-group-member = Only members of a certain group can visit this area.
notification-cannot-enter-parcel-banned = Cannot enter parcel, you have been banned.
notification-cannot-enter-parcel-not-on-access-list = Cannot enter parcel, you are not on the access list.
notification-deed-to-group-fail = Deed to group failed.
notification-release-land-throttled = The parcel [PARCEL_NAME] can not be abandoned at this time.
notification-released-land-with-reclaim = The [AREA] m² parcel '[PARCEL_NAME]' has been released. You will have [RECLAIM_PERIOD] hours to reclaim for L$0 before it is set for sale to anyone.
notification-released-land-no-reclaim = The [AREA] m² parcel '[PARCEL_NAME]' has been released. It is now available for purchase by anyone.
notification-update-viewer-buy-parcel = You need to update your viewer to buy this parcel.
notification-cant-buy-parcel-not-for-sale = Unable to buy, this parcel is not for sale.
notification-cant-buy-sale-price-or-land-area-changed = Unable to buy, the sale price or land area has changed.
notification-cant-buy-parcel-not-authorized = You are not the authorized buyer for this parcel.
notification-cant-buy-parcel-awaiting-purchase-auth = You cannot purchase this parcel because it is already awaiting purchase aut
notification-selected-multiple-owned-land = You selected land with different owners. Please select a smaller area and try again.
notification-cant-join-too-few-leased-parcels = Not enough leased parcels in selection to join.
notification-cant-divide-land-multiple-parcels-selected = Can't divide land. There is more than one parcel selected. Try selecting a smaller piece of land.
notification-cant-divide-land-cant-find-parcel = Can't divide land. Can't find the parcel. Please report with Help -> Report Problem...
notification-cant-divide-land-whole-parcel-selected = Can't divide land. Whole parcel is selected. Try selecting a smaller piece of land.
notification-land-has-been-divided = Land has been divided.
notification-pass-purchased = You purchased a pass.
notification-land-pass-expire-soon = Your pass to this land is about to expire.
notification-cant-deed-group-land = Cannot deed group-owned land.
notification-cant-buy-pass-try-again = Unable to buy pass right now. Try again later.

## Media & sound (viewer-notification-catalogue-media-sound). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-multiple-faces-selected = Multiple faces are currently selected. If you continue this action, separate instances of media will be set on multiple faces of the object. To place the media on only one face, choose Select Face and click on the desired face of that object then click 'Add'.
notification-delete-media = You have selected to delete the media associated with this face. Are you sure you want to continue?
notification-cannot-upload-sound-file = Could not open uploaded sound file for reading: [FILE]
notification-sound-file-not-riff = File does not appear to be a RIFF WAVE file: [FILE]
notification-sound-file-not-pcm = File does not appear to be a PCM WAVE audio file: [FILE]
notification-sound-file-invalid-channel-count = File has invalid number of channels (must be mono or stereo): [FILE]
notification-sound-file-invalid-sample-rate = File does not appear to be a supported sample rate (must be 44.1k): [FILE]
notification-sound-file-invalid-word-size = File does not appear to be a supported word size (must be 8 or 16 bit): [FILE]
notification-sound-file-invalid-header = Could not find 'data' chunk in WAV header: [FILE]
notification-sound-file-invalid-chunk-size = Wrong chunk size in WAV file: [FILE]
notification-sound-file-invalid-too-long = Audio file is too long ([MAX_LENGTH] second maximum): [FILE]
notification-cannot-open-temporary-sound-file = Couldn't open temporary compressed sound file for writing: [FILE]
notification-unknown-vorbis-encode-failure = Unknown Vorbis encode failure on: [FILE]
notification-parcel-can-play-media = This location provides streaming media, which may require more of your network bandwidth. Play streaming media when available? (You can change this option later under Preferences > Sound & Media.)
notification-parcel-playing-media = This location plays media: [URL] Would you like to play it?
notification-enable-media-filter = Playing media or music can expose your identity to sites outside [CURRENT_GRID]. You can enable a filter that will allow you to select which sites will receive media requests, and give you better control over your privacy. Enable the media filter? (You can change this option later under Preferences > Sound & Media.)
notification-media-alert = This parcel provides media from: Domain: [MEDIADOMAIN] URL: [MEDIAURL]
notification-media-alert2 = Do you want to remember your choice and [LCONDITION] allow media from this source? Domain: [MEDIADOMAIN] URL: [MEDIAURL]
notification-media-alert-single = This parcel provides media from: Domain: [MEDIADOMAIN] URL: [MEDIAURL]
notification-audio-alert = This parcel provides music from: Domain: [AUDIODOMAIN] URL: [AUDIOURL]
notification-audio-alert2 = Do you want to remember your choice and [LCONDITION] allow music from this source? Domain: [AUDIODOMAIN] URL: [AUDIOURL]
notification-audio-alert-single = Do you want to remember your choice and [LCONDITION] allow music from this source? Domain: [AUDIODOMAIN] URL: [AUDIOURL]
notification-no-quick-time = Apple's QuickTime software does not appear to be installed on your system. If you want to view streaming media on parcels that support it you should go to the QuickTime site and install the QuickTime Player.
notification-no-plugin = No Media Plugin was found to handle the "[MIME_TYPE]" mime type. Media of this type will be unavailable.
notification-media-plugin-failed = The following Media Plugin has failed: [PLUGIN] Please re-install the plugin or contact the vendor if you continue to experience problems.
notification-object-media-failure = Server Error: Media update or get failed. '[ERROR]'
notification-stream-list-export-success = Successfully exported stream list to XML to [FILENAME].
notification-stream-list-import-success = Successfully imported stream list from XML.
notification-stream-metadata = ♫ Now Playing: [TITLE] [ARTIST] ♫
notification-stream-metadata-no-artist = ♫ Now Playing: [TITLE] ♫
notification-add-to-media-list = Enter a domain name to be added to the [LIST]:

## Money & economy (viewer-notification-catalogue-money-economy). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-error-cannot-afford-upload = You need L$[COST] to upload this item.
notification-could-not-buy-currency = [TITLE] [MESSAGE]
notification-could-not-buy-currency-os = [TITLE] [MESSAGE]
notification-prompt-go-to-currency-page = [EXTRA] Go to [_URL] for information on purchasing L$?
notification-not-enough-currency = [NAME] L$ [PRICE] You do not have enough L$ to do that.
notification-buy-one-object-only = Unable to buy more than one object at a time. Please select only one object and try again.
notification-publish-classified = Remember: Classified ad fees are non-refundable. Publish this classified now for L$[AMOUNT]?
notification-buy-object-one-owner = Cannot buy objects from different owners at the same time. Please select only one object and try again.
notification-buy-contents-one-only = Unable to buy the contents of more than one object at a time. Please select only one object and try again.
notification-buy-contents-one-owner = Cannot buy objects from different owners at the same time. Please select only one object and try again.
notification-buy-original = Buy original object from [OWNER] for L$[PRICE]? You will become the owner of this object. You will be able to: Modify: [MODIFYPERM] Copy: [COPYPERM] Resell or Give Away: [RESELLPERM]
notification-buy-original-no-owner = Buy original object for L$[PRICE]? You will become the owner of this object. You will be able to: Modify: [MODIFYPERM] Copy: [COPYPERM] Resell or Give Away: [RESELLPERM]
notification-buy-copy = Buy a copy from [OWNER] for L$[PRICE]? The object will be copied to your inventory. You will be able to: Modify: [MODIFYPERM] Copy: [COPYPERM] Resell or Give Away: [RESELLPERM]
notification-buy-copy-no-owner = Buy a copy for L$[PRICE]? The object will be copied to your inventory. You will be able to: Modify: [MODIFYPERM] Copy: [COPYPERM] Resell or Give Away: [RESELLPERM]
notification-buy-contents = Buy contents from [OWNER] for L$[PRICE]? They will be copied to your inventory.
notification-buy-contents-no-owner = Buy contents for L$[PRICE]? They will be copied to your inventory.
notification-confirm-purchase = This transaction will: [ACTION] Are you sure you want to proceed with this purchase?
notification-confirm-purchase-password = This transaction will: [ACTION] Are you sure you want to proceed with this purchase? Please re-enter your password and click OK.
notification-pay-confirmation = Confirm that you want to pay L$[AMOUNT] to [TARGET].
notification-pay-object-failed = Payment failed: object was not found.
notification-payment-blocked-button-mismatch = Payment stopped: the price paid does not match any of the pay buttons set for this object.
notification-do-not-disturb-mode-pay = You have turned on Unavailable mode. You will not receive any items offered in exchange for this payment. Would you like to turn off Unavailable mode before completing this transaction?
notification-cannot-purchase-an-attachment = You cannot buy an object while it is attached.
notification-cannot-enter-parcel-no-payment-info-on-file = You must have payment information on file to visit this area. Do you want to go to the [CURRENT_GRID] website and set this up? [_URL]
notification-upload-payment = You paid L$[AMOUNT] to upload.
notification-unable-to-buy-while-downloading = Unable to buy while downloading object data. Please try again.
notification-cannot-buy-objects-from-different-owners = You can only buy objects from one owner at a time. Please select a single object.
notification-object-not-for-sale = This object is not for sale.
notification-payment-received = [MESSAGE]
notification-payment-sent = [MESSAGE]
notification-payment-failure = [MESSAGE]
notification-buy-linden-dollar-success = Thank you for your payment! Your L$ balance will be updated when processing completes. If processing takes more than 20 mins, your transaction may be canceled. In that case, the purchase amount will be credited to your US$ balance. The status of your payment can be checked on your Transaction History page on your Dashboard
notification-not-enough-money-for-bulk-upload = Your current balance of L$[BALANCE] is not enough to upload [COUNT] items at a total cost of L$[COST].
notification-upload-cost-confirmation = This upload will cost L$[PRICE], do you wish to continue with the upload?
notification-no-privs-to-buy-object = Your access privileges don't allow you to buy objects here.
notification-claim-object-failed-no-money = Claim object failed because you don't have enough L$.
notification-buy-object-failed-no-money = Buy object failed because you don't have enough L$.
notification-buy-inventory-failed-no-money = Buy inventory failed because you do not have enough L$
notification-buy-pass-failed-no-money = You don't have enough L$ to buy a pass to this land.
notification-add-primitive-failure = Insufficient funds to create primitve.
notification-rez-object-failure = Insufficient funds to create object.
notification-cant-transfter-money-region-disabled = Money transfers to objects are currently disabled in this region.
notification-dropped-money-transfer-request = Unable to make payment due to system load.
notification-cant-pay-no-agent = Could not figure out who to pay.
notification-cant-donate-to-public-objects = You cannot give L$ to public objects.
notification-user-balance-or-land-usage-error = An internal error prevented us from properly updating your viewer. The L$ balance or parcel holdings displayed in your viewer may not reflect your actual balance on the servers.
notification-add-payment-method = On the following page, choose a L$ amount and click a place Order button. You will be able to add a payment method at checkout.
notification-currency-uri-override-received = This region has elected to specify a third-party currency portal. Please note that currency purchases made through Firestorm Viewer are transactions between you (the user) and the provider(s) or seller(s) of the currency. Neither Firestorm Viewer, the Phoenix Firestorm Viewer Project Inc., nor its team shall be liable for any cost or damage arising either directly or indirectly from any such transaction. If you do not agree to these terms of use, then no financial transactions should be conducted using this viewer.

## Landmarks & navigation (viewer-notification-catalogue-landmarks-navigation). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-prompt-go-to-events-page = Go to the [CURRENT_GRID] events web page?
notification-landmark-already-exists = You already have a landmark for this location.
notification-landmark-location-unknown = Viewer wasn't able to get region's location. Region might be temporarily unavailable or was removed.
notification-cannot-create-landmark-not-owner = You cannot create a landmark here because the owner of the land does not allow it.
notification-create-landmark-folder = Choose a name for the folder:
notification-search-filtered-on-short-words = Your search query was modified and the words that were too short were removed. Searched for: [FINALQUERY]
notification-search-filtered-on-short-words-empty = Your search terms were too short so no search was performed.
notification-rename-landmark-default = [NAME]
notification-rename-landmark = Choose a new name for [NAME]
notification-set-classified-mature = Does this classified contain Moderate content?
notification-set-pick-location = Note: You have updated the location of this pick but the other details will retain their original values.
notification-copy-slurl = The following SLurl has been copied to your clipboard: [SLURL] Link to this from a web page to give others easy access to this location, or try it out yourself by pasting it into the address bar of any web browser.
notification-landmark-missing = Landmark is missing from the database.
notification-unable-to-load-landmark = Unable to load the landmark. Please try again.
notification-search-word-banned = Some terms in your search query were excluded due to content restrictions as clarified in the Community Standards.
notification-no-content-to-search = Please select at least one type of content to search (General, Moderate, or Adult).
notification-event-notification = Event Notification: [NAME] [DATE]
notification-land-search-blocked = Land Search Blocked. You have performed too many land searches too quickly. Please try again in a minute.
notification-region-disallows-classifieds = Region does not allow classified advertisements.
notification-cant-create-landmark-for-event = Unable to create landmark for event.
notification-cant-create-landmark = Cannot create landmark.
notification-region-tracker-add-default = [LABEL]
notification-region-tracker-add = What label would you like to use for the region "[REGION]"?

## Inventory (viewer-notification-catalogue-inventory). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-delete-notecard = Are you sure you want to delete this notecard?
notification-gesture-save-failed-too-many-steps = Gesture save failed. This gesture has too many steps. Try removing some steps, then save again.
notification-gesture-save-failed-try-again = Gesture save failed. Please try again in a minute.
notification-gesture-save-failed-object-not-found = Could not save gesture because the object or the associated object inventory could not be found. The object may be out of range or may have been deleted.
notification-gesture-save-failed-reason = There was a problem saving a gesture due to the following reason: [REASON]. Please try resaving the gesture later.
notification-save-notecard-fail-object-not-found = Could not save notecard because the object or the associated object inventory could not be found. The object may be out of range or may have been deleted.
notification-save-notecard-fail-reason = There was a problem saving a notecard due to the following reason: [REASON]. Please try re-saving the notecard later.
notification-cannot-copy-warning = You do not have permission to copy the following items: [ITEMS] and will lose it from your inventory if you give it away. Do you really want to offer these items?
notification-cannot-give-item = Unable to give inventory item.
notification-transaction-cancelled = Transaction canceled.
notification-too-many-items = Cannot give more than 42 items in a single inventory transfer.
notification-no-items = You do not have permission to transfer the selected items.
notification-cannot-copy-count-items = You do not have permission to copy [COUNT] of the selected items. You will lose these items from your inventory. Do you really want to give these items?
notification-cannot-give-category = You do not have permission to transfer the selected folder.
notification-no-inventory-host = The inventory system is currently unavailable.
notification-confirm-notecard-save = This notecard needs to be saved before the item can be copied or viewed. Save notecard?
notification-confirm-item-copy = Copy this item to your inventory?
notification-inventory-unusable = There was a problem loading your inventory. First, try logging out and logging in again. If you see this message again, contact Support to correct the problem.
notification-rename-gesture-default = [NAME]
notification-rename-gesture = New gesture name:
notification-rename-item-default = [NAME]
notification-rename-item = Choose a new name for: [NAME]
notification-confirm-item-delete-has-links = At least one of the items you have selected has link items that point to it. If you delete this item, its links will permanently stop working. It is strongly advised to delete the links first. Are you sure you want to delete these items?
notification-unable-to-load-notecard-asset = Unable to load notecard's asset at this time.
notification-not-allowed-to-view-notecard = Insufficient permissions to view notecard associated with asset ID requested.
notification-missing-notecard-asset-id = Asset ID for notecard is missing from database.
notification-apply-inventory-to-object = You are applying 'no copy' inventory item. This item will be moved to object's inventory, not copied. Move the inventory item?
notification-move-inventory-from-object = You have selected 'no copy' inventory items. These items will be moved to your inventory, not copied. Move the inventory item(s)?
notification-move-inventory-from-scripted-object = You have selected 'no copy' inventory items. These items will be moved to your inventory, not copied. Because this object is scripted, moving these items to your inventory may cause the script to malfunction. Move the inventory item(s)?
notification-open-object-cannot-copy = There are no items in this object that you are allowed to copy.
notification-delete-items = [QUESTION]
notification-delete-filtered-items = Your inventory is currently filtered and not all of the items you're about to delete are currently visible. Are you sure you want to delete them?
notification-delete-worn-items = Some item(s) you wish to delete are being worn on your avatar. Remove these items from your avatar?
notification-delete-thumbnail = Delete the image for this item? There is no undo.
notification-thumbnail-dimensions-limit = Only square images from 64 to 256 pixels per side are allowed.
notification-thumbnail-insufficient-permissions = Only copy and transfer free images can be assigned as thumbnails.
notification-thumbnail-selection-too-large = You can only modify up to 50 thumbnails at a time.
notification-cant-link-notecard = You must save the notecard before creating a link to it.
notification-cant-link-material = You must save the material before creating a link to it.
notification-confirm-delete-protected-category = The folder '[FOLDERNAME]' is a system folder. Deleting system folders can cause instability. Are you sure you want to delete it?
notification-purge-selected-items = [COUNT] item(s) will be permanently deleted. Are you sure you want to permanently delete selected item(s) from your Trash?
notification-trash-is-full = Your trash is overflowing. This may cause problems logging in.
notification-inventory-limit-reached-ais = Your inventory is experiencing issues. Please contact support of your grid.
notification-inventory-limit-reached-ais-alert = Your inventory is experiencing issues. Please contact support of your grid.
notification-confirm-empty-lost-and-found = Are you sure you want to permanently delete the contents of your Lost And Found?
notification-confirm-replace-link = You're about to replace '[TYPE]' body part link with the item which doesn't match the type. Are you sure you want to proceed?
notification-gesture-missing = Gesture [NAME] is missing from the database.
notification-unable-to-load-gesture = Unable to load gesture [NAME].
notification-notecard-missing = Notecard is missing from the database.
notification-notecard-no-permissions = You do not have permission to view this notecard.
notification-transfer-inventory-across-parent-estates = Unable to transfer inventory across parent estates.
notification-unable-to-load-notecard = Unable to load the notecard. Please try again.
notification-incomplete-inventory = Some of the contents are you trying to share cannot be given/transferred just yet. Please try offering these items again in a bit.
notification-incomplete-inventory-item = The item you are accessing is not yet locally available. Please try again in a minute.
notification-cannot-modify-protected-categories = You cannot modify protected categories.
notification-cannot-remove-protected-categories = You cannot remove protected categories.
notification-copy-failed = You do not have permission to copy this.
notification-inventory-accepted = [NAME] received your inventory offer.
notification-inventory-declined = [NAME] declined your inventory offer.
notification-own-object-give-item = Your object named [OBJECTFROMNAME] has given you this [OBJECTTYPE]: [ITEM_SLURL]
notification-user-give-item-legacy = [NAME_SLURL] has given you this [OBJECTTYPE]: [ITEM_SLURL] Do you want to keep it? "Mute" will block all future offers or messages from [NAME_SLURL].
notification-bulk-upload-no-compatible-files = Selected files can not be bulk-uploaded.
notification-bulk-upload-incompatible-files = Some of the selected files can not be bulk-uploaded.
notification-share-notification = Select residents to share with.
notification-share-items-confirmation = Are you sure you want to share the following items: [ITEMS] With the following residents: [RESIDENTS]
notification-share-folder-confirmation = Only one folder at a time can be shared. Are you sure you want to share the following items: [ITEMS] With the following Residents: [RESIDENTS]
notification-items-shared = Items successfully shared.
notification-cannot-upload-texture = Unable to upload texture: '[NAME]' [REASON]
notification-no-perm-to-copy-inventory = Not permitted to copy that inventory.
notification-unable-to-upload-asset = Unable to upload asset.
notification-cant-create-requested-inv = Cannot create requested inventory.
notification-cant-create-requested-inv-folder = Cannot create requested inventory folder.
notification-cant-create-inventory = Cannot create that inventory.
notification-inventory-not-for-sale = Inventory is not for sale.
notification-cant-find-inv-item = Unable to find inventory item.
notification-inventory-validation-failed = Corruption has been found in your inventory. Please contact [HELP] with the following list of issues. They can use http://opensimulator.org/wiki/inventory to fix the issues. [ERRORS]
notification-create-subfolder-default = [DESC]
notification-create-subfolder = Name the new folder:
notification-same-folder-required = Selected items must be in the same folder.
notification-clear-inventory-thumbnails-warning = You are about to remove thumbnail images from the inventory items in the list. This change cannot be undone. Would you like to proceed?
notification-write-inventory-thumbnails-warning = You are about to overwrite thumbnail images for some or all of the inventory items in the list. This change cannot be undone. Would you like to proceed?
notification-cant-create-inventory-name = Cannot create inventory item: [NAME]
notification-ungroup-folder = Ungroup the folder "[FOLDER_NAME]"?

## Scripts (viewer-notification-catalogue-scripts). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-script-cannot-undo = Could not undo all changes in your version of the script. Would you like to load the server's last saved version? (**Warning** This operation cannot be undone.)
notification-save-script-fail-object-not-found = Could not save the script because the object it is in could not be found. The object may be out of range or may have been deleted.
notification-could-not-start-stop-script = Could not start or stop the script because the object it is on could not be found. The object may be out of range or may have been deleted.
notification-cannot-recompile-select-objects-no-scripts = Not able to perform 'recompilation'. Select an object with a script.
notification-cannot-recompile-select-objects-no-permission = Not able to perform 'recompilation'. Select objects with scripts that you have permission to modify.
notification-cannot-reset-select-objects-no-scripts = Not able to perform 'reset'. Select objects with scripts.
notification-cannotdelete-select-objects-no-scripts = Not able to perform 'remove'. Select objects with scripts.
notification-cannot-reset-select-objects-no-permission = Not able to perform 'reset'. Select objects with scripts that you have permission to modify.
notification-cannot-open-script-object-no-mod = Unable to open script in object without modify permissions.
notification-cannot-set-running-select-objects-no-scripts = Not able to set any scripts to 'running'. Select objects with scripts.
notification-cannot-set-running-not-select-objects-no-scripts = Unable to set any scripts to 'not running'. Select objects with scripts.
notification-debit-permission-details = Granting this request gives a script ongoing permission to take Linden dollars (L$) from your account. To revoke this permission, the object owner must delete the object or reset the scripts in the object.
notification-script-missing = Script is missing from the database.
notification-script-no-permissions = Insufficient permissions to view the script.
notification-unable-to-load-script = Unable to load the script. Please try again.
notification-scripts-stopped = An administrator has temporarily stopped scripts in this region.
notification-scripts-not-running = This region is not running any scripts.
notification-no-outside-scripts = This land has outside scripts disabled. No scripts will work here except those belonging to the land owner.
notification-particle-script-find-folder-failed = Could not find a folder for the new script in inventory.
notification-particle-script-creation-failed = Could not create new script for this particle system.
notification-particle-script-not-found = Could not find the newly created script for this particle system.
notification-particle-script-create-temp-file-failed = Could not create temporary file for script upload.
notification-particle-script-injected = Particle script was injected successfully.
notification-particle-script-caps-failed = Failed to inject script into object. Request for capabilities returned an empty address.
notification-particle-script-copied-to-clipboard = The LSL script to create this particle system has been copied to your clipboard. You can now paste it into a new script to use it.
notification-default-label-missing = The behavior for switch() statements without a default case was previously incorrect and has been fixed. See FIRE-17710 for details.
notification-confirm-script-modify = Are you sure you want to modify scripts in selected objects?
notification-unable-add-script = Unable to add script!
notification-lsl-color-copied-to-clipboard = The LSL color string has been copied to your clipboard. You can now paste it into your script to use it.
notification-warn-scripted-camera = Camera reset might be inhibited by the following objects: [SOURCES]

## Web browser (viewer-notification-catalogue-web-browser). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-cannot-download-file = Unable to download file
notification-media-file-download-unsupported = You have requested a file download, which is not supported within the viewer.
notification-web-launch-external-target = Do you want to open your Web browser to view this content? Opening webpages from an unknown source may place your computer at risk. URL: [UNTRUSTED_URL]
notification-web-launch-join-now = Go to your Dashboard to manage your account?
notification-web-launch-security-issues = Visit the [CURRENT_GRID] Wiki for details of how to report a security issue.
notification-web-launch-qa-wiki = Visit the [CURRENT_GRID] QA Wiki.
notification-web-launch-public-issue = Visit the [CURRENT_GRID] Public Issue Tracker, where you can report bugs and other issues.
notification-web-launch-support-wiki = Go to the Official Linden Blog, for the latest news and information.
notification-web-launch-lsl-guide = Do you want to open the Scripting Guide for help with scripting?
notification-web-launch-lsl-wiki = Do you want to visit the LSL Portal for help with scripting?
notification-web-launch-account-history = Go to your Dashboard to see your account history?
notification-goto-url = [MESSAGE] [URL]
notification-unsupported-command-slurl = The SLurl you clicked on is not supported.

## Security (viewer-notification-catalogue-security). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-corrupted-protected-data-store = We were unable to decode the file storing your saved login credentials. At this point saving or deleting credentials will erase all those that were previously stored. This may happen when you change network setup. Restarting the viewer with previous network configuration may help recovering your saved login credentials.
notification-general-certificate-error-short = Could not connect to the server. [REASON]
notification-general-certificate-error = Could not connect to the server. [REASON] SubjectName: [SUBJECT_NAME_STRING] IssuerName: [ISSUER_NAME_STRING] Valid From: [VALID_FROM] Valid To: [VALID_TO] MD5 Fingerprint: [SHA1_DIGEST] SHA1 Fingerprint: [MD5_DIGEST] Key Usage: [KEYUSAGE] Extended Key Usage: [EXTENDEDKEYUSAGE] Subject Key Identifier: [SUBJECTKEYIDENTIFIER]
notification-trust-certificate-error = The certification authority for this server is not known. Certificate Information: SubjectName: [SUBJECT_NAME_STRING] IssuerName: [ISSUER_NAME_STRING] Valid From: [VALID_FROM] Valid To: [VALID_TO] MD5 Fingerprint: [SHA1_DIGEST] SHA1 Fingerprint: [MD5_DIGEST] Key Usage: [KEYUSAGE] Extended Key Usage: [EXTENDEDKEYUSAGE] Subject Key Identifier: [SUBJECTKEYIDENTIFIER] Would you like to trust this authority?
notification-help-report-abuse-confirm = Thank you for taking the time to inform us of this issue. We will review your report for possible violations and take the appropriate action.
notification-help-report-abuse-select-category = Please select a category for this abuse report. Selecting a category helps us file and process abuse reports.
notification-help-report-abuse-abuser-name-empty = Please enter the name of the abuser. Entering an accurate value helps us file and process abuse reports.
notification-help-report-abuse-abuser-location-empty = Please enter the location where the abuse took place. Entering an accurate value helps us file and process abuse reports.
notification-help-report-abuse-summary-empty = Please enter a summary of the abuse that took place. Entering an accurate summary helps us file and process abuse reports.
notification-help-report-abuse-details-empty = Please enter a detailed description of the abuse that took place. Be as specific as you can, including names and the details of the incident you are reporting. Entering an accurate description helps us file and process abuse reports.
notification-help-report-abuse-contains-copyright = Dear Resident, You appear to be reporting intellectual property infringement. Please make sure you are reporting it correctly: (1) The Abuse Process. You may submit an abuse report if you believe a resident is exploiting the [CURRENT_GRID] permissions system, for example, by using CopyBot or similar copying tools, to infringe intellectual property rights. The Abuse Team investigates and issues appropriate disciplinary action for behavior that violates the [CURRENT_GRID] Terms of Service or Community Standards. However, the Abuse Team does not handle and will not respond to requests to remove content from the [CURRENT_GRID] world. (2) The DMCA or Content Removal Process. To request removal of content from [CURRENT_GRID], you MUST submit a valid notification of infringement as provided in our DMCA Policy. If you still wish to continue with the abuse process, please close this window and finish submitting your report. You may need to select the specific category 'CopyBot or Permissions Exploit'. Thank you, Linden Lab
notification-not-age-verified = The location you're trying to visit is restricted to residents age 18 and over.
notification-not-age-verified-notify = Location restricted to age 18 and over.
notification-blocked-slurl = A SLurl was received from an untrusted browser and has been blocked for your security.
notification-throttled-slurl = Multiple SLurls were received from an untrusted browser within a short period. They will be blocked for a few seconds for your security.

## Teleport (viewer-notification-catalogue-teleport). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-could-not-teleport-reason = Teleport failed. [REASON]
notification-invalid-tport = Teleport attempts are limited to 6 per minute. If you are having trouble, wait one minute and try teleporting again. If the problem persists, log out and log in again.
notification-invalid-region-handoff = Problem encountered processing your region crossing. You may need to log back in before you can cross regions. If you continue to get this message, please check the [SUPPORT_SITE].
notification-blocked-tport = Sorry, teleport is currently blocked. Try again in a moment. If you still cannot teleport, please log out and log back in to resolve the problem.
notification-nolandmark-tport = Sorry, but system was unable to locate landmark destination.
notification-timeout-tport = Sorry, but system was unable to complete the teleport connection. Try again in a moment.
notification-noaccess-tport = Sorry, you do not have access to that teleport destination.
notification-missing-attach-tport = Your attachments have not arrived yet. Try waiting for a few more seconds or log out and back in again before attempting to teleport.
notification-too-many-uploads-tport = The asset queue in this region is currently clogged so your teleport request will not be able to succeed in a timely manner. Please try again in a few minutes or go to a less busy area.
notification-expired-tport = Sorry, but the system was unable to complete your teleport request in a timely fashion. Please try again in a few minutes.
notification-expired-region-handoff = Sorry, but the system was unable to complete your region crossing in a timely fashion. Please try again in a few minutes.
notification-preexisting-tport = Sorry, but the system was unable to start your teleport. Please try again in a few minutes.
notification-no-host = Unable to find teleport destination. The destination may be temporarily unavailable or no longer exists. Please try again in a few minutes.
notification-avatar-moved-desired = Your desired location is not currently available. You have been moved into a nearby region.
notification-avatar-moved-last = Your requested location is not currently available. You have been moved into a nearby region.
notification-avatar-moved-home = Your home location is not currently available. You have been moved into a nearby region. You may want to set a new home location.
notification-cant-teleport-to-grid = Could not teleport to [SLURL] as it's on a different grid ([GRID]) than the current grid ([CURRENT_GRID]). Please close your viewer and try again.
notification-reject-teleport-offers-mode-set = Reject all incoming teleport offers and requests mode is on. Incoming teleport offers and requests from anyone will now be rejected with your configured autoresponse. You will not be notified because of that fact.
notification-reject-teleport-offers-mode-warning = You cannot send a teleport request at the moment, because 'reject all incoming teleport offers and requests' mode is on. Go to the 'Comm' > 'Online Status' menu if you wish to disable it.
notification-offer-teleport-default = Join me in [REGION]
notification-offer-teleport = Offer a teleport to your location with the following message?
notification-teleport-request-prompt = Request a teleport to [NAME] with the following message
notification-too-many-teleport-offers = You attempted to make [OFFERS] teleport offers which exceeds the limit of [LIMIT].
notification-offer-teleport-from-god-default = Join me in [REGION]
notification-offer-teleport-from-god = God summon this resident to your location?
notification-teleport-from-landmark = Are you sure you want to teleport to [LOCATION]?
notification-teleport-via-slapp = Are you sure you want to teleport to [LOCATION]?
notification-teleport-to-pick = Teleport to [PICK]?
notification-teleport-to-classified = Teleport to [CLASSIFIED]?
notification-teleport-to-history-entry = Teleport to [HISTORY_ENTRY]?
notification-teleport-entry-access-blocked-adults-only-content = The region you're trying to visit contains [REGIONMATURITY] content, which is accessible to adults only.
notification-teleport-entry-access-blocked-notify = The region you're trying to visit contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content.
notification-teleport-entry-access-blocked-notify-adults-only = The region you're trying to visit contains [REGIONMATURITY] content, which is accessible to adults only.
notification-teleport-entry-access-blocked-change-and-re-teleport = The region you're trying to visit contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content. We can change your preferences and continue with the teleport, or you can cancel this teleport.
notification-teleport-entry-access-blocked-change = The region you're trying to visit contains [REGIONMATURITY] content, but your current preferences are set to exclude [REGIONMATURITY] content. We can change your preferences, or you can cancel the teleport. After your preferences are changed, you will need to attempt the teleport again.
notification-teleport-entry-access-blocked-preferences-out-of-sync = We are having technical difficulties with your teleport because your preferences are out of sync with the server.
notification-region-tp-special-usage-blocked = Unable to enter region. '[REGION_NAME]' is a Skill Gaming Region, and you must meet certain criteria in order to enter. For details, please review the Skill Gaming FAQ.
notification-region-tp-access-blocked = The region you’re trying to visit has a maturity rating exceeding your maximum maturity preference. Change this preference using Avatar > Preferences > General. Complete information on maturity ratings can be found here.
notification-no-dest-region = No destination region found.
notification-not-allowed-in-dest = You are not allowed into the destination.
notification-region-parcel-ban = Cannot region cross into banned parcel. Try another way.
notification-telehub-redirect = You have been redirected to a telehub.
notification-couldnt-tp-closer = Could not teleport closer to destination.
notification-tp-cancelled = Teleport canceled.
notification-full-region-try-again = The region you are attempting to enter is currently full. Please try again in a few moments.
notification-general-failure = General failure.
notification-routed-wrong-region = Routed to wrong region. Please try again.
notification-no-valid-agent-id = No valid agent id.
notification-no-valid-session = No valid session id.
notification-no-valid-circuit = No valid circuit code.
notification-no-pending-connection = Unable to create pending connection.
notification-internal-usher-error = Internal error attempting to connect agent usher.
notification-no-good-tp-destination = Unable to find a good teleport destination in this region.
notification-internal-error-region-resolver = Internal error attempting to activate region resolver.
notification-no-valid-landing = A valid landing point could not be found.
notification-no-valid-parcel = No valid parcel could be found.
notification-teleport-offered-sl-url = [NAME_SLURL] has offered to teleport you to their location ([POS_SLURL]): [MESSAGE] [MATURITY_ICON] - [MATURITY_STR]
notification-teleport-offered-maturity-exceeded-sl-url = [NAME_SLURL] has offered to teleport you to their location ([POS_SLURL]): [MESSAGE] [MATURITY_ICON] - [MATURITY_STR] This region contains [REGION_CONTENT_MATURITY] content, but your current preferences are set to exclude [REGION_CONTENT_MATURITY] content. We can change your preferences and continue with the teleport, or you can cancel this teleport.
notification-teleport-offered-maturity-blocked-sl-url = [NAME_SLURL] has offered to teleport you to their location ([POS_SLURL]): [MESSAGE] [MATURITY_ICON] - [MATURITY_STR] However, this region contains content accessible to adults only.
notification-teleport-offer-sent = Teleport offer sent to [TO_NAME]
notification-teleport-request = [NAME_SLURL] is requesting to be teleported to your location. [MESSAGE] Offer a teleport?
notification-confirm-clear-teleport-history = This will delete the entire list of places you have visited, and cannot be undone. Continue?
notification-teleport-to-avatar-not-possible = Teleport to this avatar not possible, because the exact position is unknown.
notification-you-died-and-got-tp-home = You died and have been teleported to your home location
notification-region-sez-not-a-home = This region does not allow you to set your home location here.
notification-home-location-limits = You can only set your 'Home Location' on your land or at a mainland Infohub.
notification-teleported-home-by-object-on-parcel = You have been teleported home by the object '[OBJECT_NAME]' on the parcel '[PARCEL_NAME]'
notification-teleported-home-by-object = You have been teleported home by the object '[OBJECT_NAME]'
notification-teleported-by-attachment = You have been teleported by an attachment on [ITEM_ID]
notification-teleported-by-object-on-parcel = You have been teleported by the object '[OBJECT_NAME]' on the parcel '[PARCEL_NAME]'
notification-teleported-by-object-owned-by = You have been teleported by the object '[OBJECT_NAME]' owned by [OWNER_ID]
notification-teleported-by-object-unknown-user = You have been teleported by the object '[OBJECT_NAME]' owned by an unknown user.
notification-reset-home-position-not-legal = Reset Home position since Home wasn't legal.
notification-cant-invite-region-full = You cannot currently invite anyone to your location because the region is full. Try again later.
notification-cant-set-home-at-region = This region does not allow you to set your home location here.
notification-list-valid-home-locations = You can only set your 'Home Location' on your land or at a mainland Infohub.
notification-set-home-position = Home position set.

## Premium account (viewer-notification-catalogue-premium-account). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-set-display-name-success = Hi [DISPLAY_NAME]! Just like in real life, it takes a while for everyone to learn about a new name. Please allow several days for your name to update in objects, scripts, search, etc.
notification-set-display-name-blocked = Sorry, you cannot change your display name. If you feel this is in error, please contact the grid support.
notification-set-display-name-failed-length = Sorry, that name is too long. Display names can have a maximum of [LENGTH] characters. Please try a shorter name.
notification-set-display-name-failed-generic = Sorry, we could not set your display name. Please try again later.
notification-set-display-name-mismatch = The display names you entered do not match. Please re-enter.
notification-agent-display-name-update-threshold-exceeded = Sorry, you have to wait longer before you can change your display name. See http://wiki.secondlife.com/wiki/Setting_your_display_name Please try again later.
notification-agent-display-name-set-blocked = Sorry, we could not set your requested name because it contains a banned word. Please try a different name.
notification-agent-display-name-set-invalid-unicode = The display name you wish to set contains invalid characters.
notification-agent-display-name-set-only-punctuation = Your display name must contain letters other than punctuation.
notification-display-name-update = [OLD_NAME] ([SLID]) is now known as [NEW_NAME].
notification-display-name-update-remove-alias = [OLD_NAME] ([SLID]) is now known as [NEW_NAME]. This agent has a set alias that will replace [NEW_NAME] Would you like to remove it?

## Voice (viewer-notification-catalogue-voice). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-voice-version-mismatch = This version of the viewer is not compatible with the Voice Chat feature in this region. In order for Voice Chat to function correctly you will need to update the viewer.
notification-voice-invite-p2-p = [NAME] is inviting you to a Voice Chat call. Click Accept to join the call or Decline to decline the invitation. Click mute to permanently block all messages this caller.
notification-voice-invite-group = [NAME] has joined a Voice Chat call with the group [GROUP]. Click Accept to join the call or Decline to decline the invitation. Click mute to permanently block all messages from this caller.
notification-voice-invite-ad-hoc = [NAME] has joined a voice chat call with a conference chat. Click Accept to join the call or Decline to decline the invitation. Click mute to permanently block all message from this caller.
notification-voice-channel-full = The voice call you are trying to join, [VOICE_CHANNEL_NAME], has reached maximum capacity. Please try again later.
notification-proximal-voice-channel-full = This area has reached maximum capacity for voice conversations. Please try to use voice in a different area.
notification-voice-channel-disconnected = You have been disconnected from [VOICE_CHANNEL_NAME]. You will now be reconnected to Nearby Voice Chat.
notification-voice-channel-disconnected-p2-p = [VOICE_CHANNEL_NAME] has ended the call. You will now be reconnected to Nearby Voice Chat.
notification-p2-p-call-declined = [VOICE_CHANNEL_NAME] has declined your call. You will now be reconnected to Nearby Voice Chat.
notification-p2-p-call-no-answer = [VOICE_CHANNEL_NAME] is not available to take your call. You will now be reconnected to Nearby Voice Chat.
notification-voice-channel-join-failed = Failed to connect to [VOICE_CHANNEL_NAME], please try again later. You will now be reconnected to Nearby Voice Chat.
notification-voice-effects-expired = One or more of your subscribed Voice Morphs has expired. Click here to renew your subscription. If you are a Premium Member, click here to receive your voice morphing perk.
notification-voice-effects-expired-in-use = The active Voice Morph has expired, your normal voice settings have been applied. Click here to renew your subscription. If you are a Premium Member, click here to receive your voice morphing perk.
notification-voice-effects-will-expire = One or more of your Voice Morphs will expire in less than [INTERVAL] days. Click here to renew your subscription. If you are a Premium Member, click here to receive your voice morphing perk.
notification-voice-effects-new = New Voice Morphs are available!
notification-voice-effects-not-supported = Voice Morphs are not supported by this viewer. For more information about other voice morph tools, see this article.
notification-voice-not-allowed = You do not have permission to connect to voice chat for [VOICE_CHANNEL_NAME].
notification-voice-call-generic-error = An error has occurred while trying to connect to voice chat for [VOICE_CHANNEL_NAME]. Please try again later.
notification-voice-is-muted-by-moderator = Your voice has been muted by a moderator.
notification-no-voice-connect = We are unable to connect to the voice server: [HOSTID] Ports that must be allowed for voice are: :TCP: 80, 443 :UDP: 3478, 3479, 5060, 5062, 6250, 12000-32000 Please check your network and firewall setup. Disable any SIP ALG feature in your router. Voice communications will not be available. https://wiki.firestormviewer.org/fs_voice
notification-no-voice-connect-giab = We're having trouble connecting to your voice server. Voice communications will not be available. Please check your network and firewall setup.
notification-confirm-leave-call = Are you sure you want to leave this call?
notification-confirm-mute-all = You have selected to mute all participants in a group call. This will also cause all residents that later join the call to be muted, even after you have left the call. Mute everyone?

## Experiences (viewer-notification-catalogue-experiences). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-experience-acquire-failed = Unable to acquire a new experience: [ERROR_MESSAGE]
notification-not-in-group-experience-profile-message = A change to the experience group was ignored because the owner is not a member of the selected group.
notification-uneditable-experience-profile-message = The uneditable field '[field]' was ignored when updating the experience profile.
notification-restricted-to-owner-experience-profile-message = Ignored changes to the field '[field]' which can only be set by the experience owner.
notification-maturity-rating-exceeds-owner-experience-profile-message = You may not set the maturity rating of an experience higher than that of the owner.
notification-restricted-term-experience-profile-message = The following terms prevented the update of the experience profile name and/or description: [extra_info]
notification-teleported-home-experience-removed = You have been teleported from the region [region_name] for removing the experience secondlife:///app/experience/[public_id]/profile and are no longer permitted in the region.
notification-trusted-experience-entry = You have been allowed into the region [region_name] by participating in the key experience secondlife:///app/experience/[public_id]/profile removing this experience may kick you from the region.
notification-experience-event = An object was allowed to [EventType] by the secondlife:///app/experience/[public_id]/profile experience. Owner: secondlife:///app/agent/[OwnerID]/inspect Object Name: [ObjectName] Parcel Name: [ParcelName]
notification-experience-event-attachment = An attachment was allowed to [EventType] by the secondlife:///app/experience/[public_id]/profile experience. Owner: secondlife:///app/agent/[OwnerID]/inspect

## RLV (viewer-notification-catalogue-rlv). Bodies follow
## the reference notifications.xml with the standard trims (KB URLs,
## [APP_NAME]/[SECOND_LIFE] self-references, <nolink> markup).

notification-rl-va-change-strings = Changes won't take effect until after you restart the viewer.
notification-rl-va-list-requested = [NAME_SLURL] has requested to be sent a list of your currently active RLV restrictions.

## The reference `ignoretext` lines — one per suppressible / checkbox-only
## notification, keyed notification-ignoretext-<kebab-of-name>. The alerts
## tab's row labels; for a CheckboxOnly template, the checkbox label.
notification-ignoretext-add-group-owner-warning = Confirm before I add a new group Owner
notification-ignoretext-alert-merchant-listing-activate-required = Alert about version folder activation when I create a listing with several version folders
notification-ignoretext-alert-merchant-stock-folder-empty = Alert when a listing is unlisted because stock folder is empty
notification-ignoretext-alert-merchant-stock-folder-split = Alert when stock folder is being split before being listed
notification-ignoretext-alert-merchant-version-folder-empty = Alert when a listing is unlisted because version folder is empty
notification-ignoretext-apply-inventory-to-object = Warn me before I apply 'no-copy' items to an object
notification-ignoretext-attachment-drop = Confirm before dropping attachments
notification-ignoretext-auto-adjust-hdr-sky = HDR Sky adjustment warning
notification-ignoretext-auto-wear-new-clothing = Wear the clothing I create while editing My Appearance
notification-ignoretext-autorespond-mode-set = I change my status to autorespond mode
notification-ignoretext-autorespond-non-friends-mode-set = I change my status to autorespond mode for non-friends
notification-ignoretext-ban-group-member-warning = Confirm banning a participant from group
notification-ignoretext-ban-group-members-warning = Confirm banning multiple members from group
notification-ignoretext-can-not-remove-connected-grid = Warn that the grid connected to can not be removed.
notification-ignoretext-cannot-enter-parcel-no-payment-info-on-file = I lack payment information on file
notification-ignoretext-cant-select-reflection-probe = Warn if Reflection Probes selection is disabled.
notification-ignoretext-change-object-bonus-factor = Confirm changing object bonus factor
notification-ignoretext-click-action-not-payable = I set the action 'Pay object' when building an object without a money() script
notification-ignoretext-clothing-loading = Clothing is taking a long time to download
notification-ignoretext-confirm-adding-chat-participants = Confirm adding chat paticipants
notification-ignoretext-confirm-clear-debug-search-url = Confirm clearing debug search url
notification-ignoretext-confirm-close-all = Confirm before I close all IMs
notification-ignoretext-confirm-copy-to-marketplace = Confirm before I try to copy a selection containing no copy items to the marketplace
notification-ignoretext-confirm-delete-protected-category = Confirm before I delete a system folder
notification-ignoretext-confirm-empty-lost-and-found = Confirm before I empty the inventory Lost And Found folder
notification-ignoretext-confirm-hide-ui = Confirm before hiding UI
notification-ignoretext-confirm-leave-call = Confirm before I leave call
notification-ignoretext-confirm-listing-cut-or-delete = Confirm before I move or delete a listing from the marketplace
notification-ignoretext-confirm-merchant-active-change = Confirm before I change an active listing on the marketplace
notification-ignoretext-confirm-merchant-clear-version = Confirm before I deactivate the version folder of a listing on the marketplace
notification-ignoretext-confirm-merchant-move-inventory = Confirm before I move an item from the inventory to the marketplace
notification-ignoretext-confirm-merchant-unlist = Confirm before I unlist an active listing on the marketplace
notification-ignoretext-confirm-mute-all = Confirm before I mute all participants in a group call
notification-ignoretext-confirm-overwrite-outfit = Confirm before overwriting outfit
notification-ignoretext-confirm-pick-debug-search-url = Confirm picking debug search url
notification-ignoretext-confirm-quit = Confirm before I quit
notification-ignoretext-confirm-remove-grid = Confirm removing grids
notification-ignoretext-confirm-replace-link = Confirm before I replace link
notification-ignoretext-confirm-restore-quick-prefs-defaults = Confirm restore quick prefs defaults
notification-ignoretext-confirm-script-modify = Confirm before I modify scripts in selection
notification-ignoretext-control-name-copied-to-clipboard = A debug setting's name was copied to my clipboard
notification-ignoretext-copy-slurl = SLurl is copied to my clipboard
notification-ignoretext-currency-uri-override-received = When the region sets a new currency helper.
notification-ignoretext-debug-settings-warning = Debug Settings warning message
notification-ignoretext-deed-object-to-group = Confirm before I deed an object to a group
notification-ignoretext-default-label-missing = A LSL script has switch statement without a default label
notification-ignoretext-delete-filtered-items = Confirm before deleting filtered items
notification-ignoretext-delete-items = Confirm before deleting items
notification-ignoretext-delete-media = Confirm before I delete media from an object
notification-ignoretext-delete-notecard = Confirm notecard deletion
notification-ignoretext-delete-thumbnail = Warn me that thumbnail will be permanently removed
notification-ignoretext-do-not-disturb-mode-pay = I am about to pay a person or object while I am in Unavailable mode
notification-ignoretext-do-not-disturb-mode-set = I change my status to unavailable
notification-ignoretext-eject-group-member-warning = Confirm ejecting a participant from group
notification-ignoretext-eject-group-members-warning = Confirm ejecting multiple members from group
notification-ignoretext-face-paste-texture-permissions = Paste: You applied a texture with limited permissions
notification-ignoretext-first-join-support-group2 = The Phoenix/Firestorm Support Group was joined
notification-ignoretext-fs-large-outfits-warning-in-this-session = Outfit count warning
notification-ignoretext-global-online-status-toggle = Inform me that toggling online status visibility may take a while
notification-ignoretext-image-upload-resized = Image Upload Resized
notification-ignoretext-inventory-validation-failed = Warn if inventory validation errors have been detected.
notification-ignoretext-land-buy-access-blocked-adults-only-content = Only adults can buy this land.
notification-ignoretext-land-buy-access-blocked-change = The land you're trying to buy contains content excluded by your preferences.
notification-ignoretext-land-claim-access-blocked-adults-only-content = Only adults can claim this land.
notification-ignoretext-land-claim-access-blocked-change = The land you're trying to claim contains content excluded by your preferences.
notification-ignoretext-live-preview-unavailable = Warn me that Live Preview mode is not available for no-copy and/or no-transfer textures
notification-ignoretext-live-preview-unavailable-pbr = Warn me that Live Preview mode is not available for no-copy, no-transfer, and/or no-modify materials
notification-ignoretext-lsl-color-copied-to-clipboard = An LSL color string was copied to my clipboard
notification-ignoretext-material-images-were-scaled = Warn if textures will be scaled during upload.
notification-ignoretext-media-file-download-unsupported = Warn about unsupported file downloads
notification-ignoretext-media-plugin-failed = A Media Plugin fails to run
notification-ignoretext-merchant-force-validate-listing = Warn me that creating a listing fixes the hierarchy of the content
notification-ignoretext-move-inventory-from-object = Warn me before I move 'no-copy' items from an object
notification-ignoretext-move-inventory-from-scripted-object = Warn me before I move 'no-copy' items which might break a scripted object
notification-ignoretext-multiple-faces-selected = Media will be set on multiple selected faces
notification-ignoretext-no-havok = No Havok Warning
notification-ignoretext-no-voice-connect = Warn me when the viewer can't connect to the voice server
notification-ignoretext-not-age-verified = I am not old enough to visit age restricted areas.
notification-ignoretext-old-gpu-driver = My graphics driver is out of date
notification-ignoretext-outbox-folder-created = A new folder was created in the Merchant Outbox
notification-ignoretext-outbox-import-complete = All folders sent to the Marketplace
notification-ignoretext-parcel-playing-media = Always choose this option for this land.
notification-ignoretext-particle-script-copied-to-clipboard = A particle script was copied to my clipboard
notification-ignoretext-particle-script-injected = A particle script was injected to an object.
notification-ignoretext-pathfinding-delete-multiple-items = Are you sure you want to delete multiple items?
notification-ignoretext-pathfinding-linksets-change-to-flexible-path = The selected object affects the navmesh. Changing it to a Flexible Path will remove it from the navmesh.
notification-ignoretext-pathfinding-linksets-mismatch-on-restricted = Some selected linksets cannot be set because of permission restrictions on the linkset.
notification-ignoretext-pathfinding-linksets-mismatch-on-restricted-mismatch-on-volume = Some selected linksets cannot be set because of permission restrictions on the linkset and because the shape is non-convex.
notification-ignoretext-pathfinding-linksets-mismatch-on-volume = Some selected linksets cannot be set because the shape is non-convex
notification-ignoretext-pathfinding-linksets-warn-on-phantom = Some selected linksets phantom flag will be toggled.
notification-ignoretext-pathfinding-linksets-warn-on-phantom-mismatch-on-restricted = Some selected linksets phantom flag will be toggled and others cannot be set because of permission restrictions on the linkset.
notification-ignoretext-pathfinding-linksets-warn-on-phantom-mismatch-on-restricted-mismatch-on-volume = Some selected linksets phantom flag will be toggled and others cannot be set because of permission restrictions on the linkset and because the shape is non-convex.
notification-ignoretext-pathfinding-linksets-warn-on-phantom-mismatch-on-volume = Some selected linksets phantom flag will be toggled and others cannot be set because the shape is non-convex
notification-ignoretext-pathfinding-return-multiple-items = Are you sure you want to return multiple items?
notification-ignoretext-preference-chat-clear-log = Confirm before I delete the log of previous conversations.
notification-ignoretext-preference-chat-delete-transcripts = Confirm before I delete transcripts.
notification-ignoretext-preference-chat-path-changed = Unable to move files. Restored previous path.
notification-ignoretext-prompt-mfa-token-with-save = Remember this computer for 30 days.
notification-ignoretext-reflection-probe-applied = Reflection Probe tips
notification-ignoretext-region-entry-access-blocked-adults-only-content = Region crossing: The region you're trying to visit contains content which is accessible to adults only.
notification-ignoretext-region-entry-access-blocked-change = Region crossing: The region you're trying to visit contains content excluded by your preferences.
notification-ignoretext-reject-all-group-invites-mode-set = I change my status to reject all group invites mode
notification-ignoretext-reject-friendship-requests-mode-set = I change my status to reject all friendship requests mode
notification-ignoretext-reject-teleport-offers-mode-set = I change my status to reject all teleport offers and requests mode
notification-ignoretext-remove-contact-from-set = Confirm before removing someone from a contact set
notification-ignoretext-remove-contact-set = Confirm before removing a contact set
notification-ignoretext-remove-contacts-from-set = Confirm before removing multiple avatars from a contact set
notification-ignoretext-replace-attachment = Replace an existing attachment with the selected item
notification-ignoretext-return-to-owner = Confirm before I return objects to their owners
notification-ignoretext-rigged-mesh-attached-to-hud = Warn me when rigged mesh is attached to HUD point.
notification-ignoretext-rl-va-list-requested = Confirm before sending anyone a list of my current RLV restrictions.
notification-ignoretext-sanity-check = A settings control has failed the sanity check.
notification-ignoretext-settings-confirm-loss = Are you sure you want to lose changes?
notification-ignoretext-settings-make-no-trans = Are you sure you want to make settings non-transferable?
notification-ignoretext-share-items-confirmation = Confirm before I share an item
notification-ignoretext-skin-defaults-change-settings = A preferences setting was changed to the skin's default value.
notification-ignoretext-teleport-entry-access-blocked-adults-only-content = Teleport: The region you're trying to visit contains content which is accessible to adults only.
notification-ignoretext-teleport-entry-access-blocked-change = Teleport (non-restartable): The region you're trying to visit contains content excluded by your preferences.
notification-ignoretext-teleport-entry-access-blocked-change-and-re-teleport = Teleport (restartable): The region you're trying to visit contains content excluded by your preferences.
notification-ignoretext-teleport-from-landmark = Confirm that I want to teleport to a landmark
notification-ignoretext-teleport-to-classified = Confirm that I want to teleport to a location in Classifieds
notification-ignoretext-teleport-to-history-entry = Confirm that I want to teleport to a history location
notification-ignoretext-teleport-to-pick = Confirm that I want to teleport to a location in Picks
notification-ignoretext-teleport-via-slapp = Confirm that I want to teleport via SLAPP
notification-ignoretext-teleported-by-attachment = Teleport: You have been teleported by an attachment
notification-ignoretext-teleported-by-object-on-parcel = Teleport: You have been teleported by an object on a parcel
notification-ignoretext-teleported-home-experience-removed = Kicked from region for removing an experience
notification-ignoretext-trusted-experience-entry = Allowed into a region by an experience
notification-ignoretext-unknown-gpu = My graphics card could not be identified
notification-ignoretext-unsupported-hardware = My computer hardware is not supported
notification-ignoretext-usaved-wearable-changes = Confirm before I discard unsaved wearable changes
notification-ignoretext-voice-effects-not-supported = Warn me about voice morph not being supported
notification-ignoretext-voice-effects-will-expire = Warn me about voice morph expiring
notification-ignoretext-web-launch-account-history = Launch my browser to see my account history
notification-ignoretext-web-launch-external-target = Launch my browser to view a web page
notification-ignoretext-web-launch-join-now = Launch my browser to manage my account
notification-ignoretext-web-launch-lsl-guide = Launch my browser to view the Scripting Guide
notification-ignoretext-web-launch-lsl-wiki = Launch my browser to view the LSL Portal
notification-ignoretext-web-launch-public-issue = Launch my browser to use the Public Issue Tracker
notification-ignoretext-web-launch-qa-wiki = Launch my browser to view the QA Wiki
notification-ignoretext-web-launch-security-issues = Launch my browser to learn how to report a Security Issue
notification-ignoretext-web-launch-support-wiki = Launch my browser to view the blog

notification-overflow =
    { $count ->
        [one] { $count } more
       *[other] { $count } more
    }

# The `SL_VIEWER_NOTIFICATION_DEMO` sample toasts (a debug affordance).
notification-demo-tip = A transient tip — it fades away after ten seconds unless you hover it.
notification-demo-notify = An informational notice — it stays for thirty seconds, then fades.
notification-demo-alert = A sticky alert — it waits for you to acknowledge it rather than fading.

# The group-notice toast (viewer-group-notice-display): the card a received
# group notice pops.
group-notice-header = Group Notice
group-notice-sent-by = Sent by { $sender }, { $group }
group-notice-loading = (loading)
group-notice-timestamp = { $when } SLT
group-notice-button-ok = OK
group-notice-button-notices = Group Notices
group-notice-button-chat = Group Chat

# The script-dialog toast (viewer-dialog-lldialog): the card a scripted object's
# llDialog / llTextBox pops. The title reads "Owner Name's 'Object Name'".
script-dialog-from = { $owner }'s '{ $object }'
script-dialog-button-block = Block
script-dialog-button-ignore = Ignore
script-dialog-button-submit = Submit

# The script web-page request toast (viewer-dialog-script-load-url): the card a
# scripted object's llLoadURL pops, showing the object, owner and target URL so
# the user can vet the link before opening it in the embedded browser.
load-url-heading = Open a web page?
load-url-from = '{ $object }' owned by { $owner }
load-url-owner-loading = (loading…)
load-url-button-load = Load
load-url-button-block = Block
load-url-button-ignore = Ignore

# The shared URL-linkification widget (viewer-url-linkification): the placeholder
# an agent / group link shows until its name resolves, and the hover-tooltip
# category lines (shown above the link's actual destination URL). Mirror the
# reference Tooltip* strings.
link-loading = (loading…)
link-tooltip-http = Web page
link-tooltip-slurl = Location in Second Life
link-tooltip-slapp = Second Life command
link-tooltip-parcel = Parcel information
link-tooltip-agent = Resident profile
link-tooltip-group = Group information

# The avatar / object inspector mini-popups (viewer-inspector-popups): the small
# self-dismissing card a clicked resident / object name opens. Mirrors the
# reference LLInspectAvatar / LLInspectObject buttons and labels.
inspector-loading = (loading…)
inspector-no-bio = (no profile text)
inspector-owner = Owner:
inspector-owner-unknown = (unknown)
inspector-object-unnamed = (unnamed object)
inspector-button-profile = View Profile
inspector-button-im = IM
inspector-button-add-friend = Add Friend
inspector-button-offer-teleport = Offer Teleport
inspector-button-show-on-map = Show on Map
inspector-button-block = Block

# The script permission-request toast (viewer-permission-request-dialog): the card
# a scripted object's llRequestPermissions pops (the ScriptQuestion message),
# naming the object / owner and the requested permission bits.
script-permission-intro = '{ $object }', an object owned by { $owner }, would like to:
script-permission-confirm = Is this OK?
script-permission-button-yes = Yes
script-permission-button-no = No
script-permission-button-block = Block
# The caution (money-access) variant (ScriptQuestionCaution), shown when a script
# asks to debit the agent's L$ account.
script-permission-caution-warning = The object '{ $object }' wants access to take money from your Linden Dollar account. If you allow this, it can take any or all of your money from you at any time, with no further warning or request.
script-permission-caution-advice = Before allowing this access, make sure you know what the object is and why it is making this request, as well as whether you trust the creator. If you're not certain, click Deny.
script-permission-caution-additional = If you allow access to your account, you will also be allowing the object to:
script-permission-button-allow = Allow access
script-permission-button-deny = Deny
# The requested-permission lines (the reference ScriptQuestion [QUESTIONS] strings).
script-permission-q-debit = Take Linden dollars (L$) from you
script-permission-q-controls = Act on your control inputs
script-permission-q-animation = Animate your avatar
script-permission-q-attach = Attach to your avatar
script-permission-q-links = Link and delink from other objects
script-permission-q-track-camera = Track your camera
script-permission-q-control-camera = Control your camera
script-permission-q-teleport = Teleport you
script-permission-q-experience = Participate in an experience
script-permission-q-estate = Suppress alerts when managing estate access lists
script-permission-q-override-anim = Replace your default animations
script-permission-q-return-objects = Return objects on your behalf

# The experience-acceptance toast (viewer-experience-permission-dialog): the
# reference ScriptQuestionExperience card a scripted object pops to run under an
# experience. The object / owner / scope come from the ScriptQuestion; the name and
# scope from the fetched experience metadata; the permission lines reuse the
# script-permission-q-* strings above.
experience-permission-intro = '{ $object }', an object owned by { $owner }, requests your participation in the { $scope } experience:
experience-permission-once = Once permission is granted you will not see this message again for this experience unless it is revoked from the experience profile.
experience-permission-scripts = Scripts associated with this experience will be able to do the following on regions where the experience is active:
experience-permission-confirm = Is this OK?
experience-permission-button-yes = Yes
experience-permission-button-no = No
experience-permission-button-block-experience = Block Experience
experience-permission-button-block-object = Block Object
# Shown in place of the name when the grid cannot resolve the experience id.
experience-permission-unknown-name = (unknown experience)
# The notification-well history line summarising an experience prompt.
experience-permission-history = Experience: { $experience }
# The experience scope word, from the metadata's grid-wide vs land property
# (the reference Grid-Scope / Land-Scope substitution).
experience-scope-grid = grid-wide
experience-scope-land = land-scoped

# The Experiences floater (viewer-experience-permission-dialog): the manage surface
# listing the agent's allowed / blocked experiences with a per-row Forget.
experiences-title = Experiences
experiences-refresh = Refresh
experiences-allowed-heading = Allowed experiences
experiences-blocked-heading = Blocked experiences
experiences-empty = No experiences.
experiences-forget = Forget

# The offers & invites toasts (viewer-dialog-offers-invites): the accept /
# decline cards the grid throws at the user over IM — an inventory offer, a
# teleport offer / lure, a friendship offer, and a group-membership invitation.
offer-inventory-heading = Inventory Offer
offer-inventory-from = { $name } has given you an item:
offer-teleport-heading = Teleport Offer
offer-teleport-from = { $name } has offered to teleport you to their location:
offer-friendship-heading = Friendship Offer
offer-friendship-from = { $name } is offering to be your friend.
offer-group-heading = Group Invitation
offer-group-from = { $name } has invited you to join a group:
offer-group-fee = There is a fee of L$ { $fee } to join this group.
offer-button-accept = Accept
offer-button-decline = Decline
offer-button-teleport = Teleport
offer-button-join = Join
offer-button-block = Block

## The snapshot floater (viewer-snapshot-floater) — a framed preview of the world
## (refreshed on demand), include-UI / include-HUD toggles, a format picker and a
## save-to-disk destination laid out as tabs. The postcard / e-mail, profile-feed
## and inventory destinations are placeholder tabs with their own follow-up tasks.
snapshot-title = Snapshot
snapshot-refresh = Refresh
snapshot-preview-empty = Click Refresh to update the preview.
snapshot-include-ui = Show interface in snapshot
snapshot-include-hud = Show HUD objects in snapshot
snapshot-hide-balance = Hide L$ balance in snapshot
snapshot-format-label = Format:
snapshot-save-disk = Save to Disk
snapshot-hint = Saves at the window's resolution to your Pictures folder; the path is echoed to nearby chat.
snapshot-status-ready = Ready
snapshot-status-saving = Working…
snapshot-saved = Saved snapshot to { $path }
snapshot-save-failed = Could not save the snapshot: { $error }
snapshot-no-dir = No snapshot folder is available on this system.
# The destination tabs.
snapshot-tab-disk = Disk
snapshot-tab-postcard = Postcard
snapshot-tab-profile = Profile
snapshot-tab-inventory = Inventory
snapshot-postcard-todo = Sending a snapshot as a postcard / e-mail is coming in its own task.
snapshot-profile-todo = Posting a snapshot to your profile feed is coming in its own task.
snapshot-inventory-todo = Saving a snapshot to inventory as a texture is coming in its own task.

## The Preferences floater shell (viewer-preferences-floater).

preferences-title = Preferences
preferences-search-placeholder = Search settings
preferences-ok = OK
preferences-cancel = Cancel
# The tab strip.
preferences-tab-general = General
preferences-tab-graphics = Graphics
preferences-tab-world-ui = UI & world display
preferences-tab-alerts = Alerts
# The general tab (viewer-preferences-general-tab): language, maturity, start
# location, UI scale, name-tag basics, away timeout, busy responses.
preferences-section-language = Language
preferences-row-language = Interface language
preferences-locale-default = System default
preferences-locale-english = English
preferences-locale-japanese = 日本語 (Japanese)
preferences-locale-arabic = العربية (Arabic)
preferences-locale-polish = Polski (Polish)
preferences-locale-pseudo = Pseudolocale (testing)
preferences-section-content-rating = Content rating
preferences-row-maturity = I want to access content rated
preferences-maturity-general = General
preferences-maturity-moderate = Moderate
preferences-maturity-adult = Adult
preferences-section-start-location = Start location
preferences-row-start-location = Log in to
preferences-start-last = My last location
preferences-start-home = My home
preferences-section-interface = Interface
preferences-row-ui-scale = UI size
preferences-ui-scale-reset = Reset
preferences-section-name-tags = Name tags
preferences-row-name-tags = Show name tags
preferences-row-own-name-tag = Show my own name tag
preferences-row-name-tag-display-names = Show display names
preferences-row-name-tag-usernames = Show usernames under display names
preferences-row-name-tag-group-titles = Show group titles
preferences-row-name-tag-typing = Show a Typing line while an avatar types
preferences-row-name-tag-distance = Show the distance line
preferences-row-name-tag-friend-color = Colour friends' name tags
preferences-row-name-tag-color-by-distance = Tint whole tags by chat range
preferences-row-name-tag-fade-start = Fade name tags starting at (m)
preferences-row-name-tag-fade-range = Name-tag fade range (m)
preferences-row-name-tag-bubble-opacity = Name-tag bubble opacity
preferences-section-away = Away
preferences-row-afk-timeout = Mark me as away after
preferences-afk-never = Never
preferences-afk-2-min = 2 minutes
preferences-afk-5-min = 5 minutes
preferences-afk-10-min = 10 minutes
preferences-afk-30-min = 30 minutes
preferences-afk-60-min = 60 minutes
preferences-section-busy-response = Automatic replies
preferences-row-busy-response = Do Not Disturb reply
preferences-row-autorespond-response = Autorespond reply
preferences-row-autorespond-non-friends-response = Autorespond reply to non-friends
# The UI & world display tab's section headings and rows.
preferences-section-world = In-world display
preferences-section-maps = Mini-map & world map
preferences-row-property-lines = Show property lines
preferences-row-status-coordinates = Show coordinates in the status bar
preferences-row-hover-text = Show floating text over objects
preferences-row-minimap-rotate = Rotate mini-map with the camera
preferences-row-minimap-auto-center = Auto-center the mini-map
preferences-row-minimap-objects = Show objects on the mini-map
preferences-row-minimap-property-lines = Show property lines on the mini-map
preferences-row-minimap-for-sale = Show for-sale parcels on the mini-map
preferences-row-minimap-chat-ring = Show chat range rings on the mini-map
preferences-row-minimap-scale = Mini-map zoom
preferences-row-minimap-opacity = Mini-map opacity
preferences-row-worldmap-people = Show people on the world map
preferences-row-worldmap-infohubs = Show infohubs on the world map
preferences-row-worldmap-land-sale = Show land for sale on the world map
preferences-row-worldmap-events = Show events on the world map
preferences-row-worldmap-region-names = Show region names on the world map
# The graphics tab (viewer-preferences-graphics-tab): quality tier, draw
# distance, LOD factor, shadows, reflections / mirrors, glow, tone mapping
# and the vsync / frame-rate cap.
preferences-section-render-quality = Quality & speed
preferences-row-render-quality = Quality preset
preferences-quality-low = Low
preferences-quality-medium-low = Medium-Low
preferences-quality-medium = Medium
preferences-quality-medium-high = Medium-High
preferences-quality-high = High
preferences-quality-high-ultra = High-Ultra
preferences-quality-ultra = Ultra
preferences-row-draw-distance = Draw distance (m)
preferences-row-lod-factor = Mesh detail (LOD factor)
preferences-row-max-particles = Maximum particle count
preferences-section-shadows = Shadows
preferences-row-shadow-detail = Shadows
preferences-shadows-none = None
preferences-shadows-sun-moon = Sun and moon
preferences-row-shadow-map-size = Shadow map resolution
preferences-shadow-map-1024 = 1024 (fastest)
preferences-shadow-map-2048 = 2048
preferences-shadow-map-4096 = 4096 (default)
preferences-shadow-map-8192 = 8192 (sharpest)
preferences-row-shadow-cascades = Shadow cascades
preferences-section-reflections = Reflections & mirrors
preferences-row-probe-dynamic = Show avatars in reflections
preferences-row-mirrors = Realtime mirrors
preferences-row-mirror-resolution = Mirror resolution (takes effect after restart)
preferences-mirror-res-256 = 256
preferences-mirror-res-512 = 512 (default)
preferences-mirror-res-1024 = 1024
preferences-mirror-res-2048 = 2048
preferences-row-mirror-update-rate = Mirror update rate
preferences-mirror-rate-1 = Every frame
preferences-mirror-rate-2 = Every 2nd frame
preferences-mirror-rate-4 = Every 4th frame
preferences-mirror-rate-8 = Every 8th frame
preferences-section-glow = Glow
preferences-row-glow = Render glow
preferences-row-glow-strength = Glow strength
preferences-row-glow-width = Glow width
preferences-row-glow-iterations = Glow quality (blur iterations)
preferences-section-tonemap = Tone mapping & exposure
preferences-row-tonemap-type = Tone curve
preferences-tonemap-khronos = Khronos PBR Neutral
preferences-tonemap-aces = ACES
preferences-tonemap-none = None
preferences-row-tonemap-mix = Tone curve mix
preferences-row-exposure = Exposure
preferences-row-dynamic-exposure = Dynamic exposure (eye adaptation)
preferences-row-auto-adjust-legacy = Auto-adjust legacy skies
preferences-section-display = Display & frame rate
preferences-row-vsync = Vertical sync (VSync)
preferences-row-limit-framerate = Limit frame rate
preferences-row-fps-limit = Maximum frames per second
# The alerts tab (viewer-preferences-alerts-tab): headline toggles, then the
# per-notification popup list.
preferences-section-alert-headlines = Notices
preferences-row-friend-online-toasts = Notify me when my friends log in or out
preferences-row-group-notice-toasts = Show a toast when a group notice arrives
preferences-row-auto-accept-inventory = Automatically accept incoming inventory offers
preferences-section-alert-popups = Viewer alerts that can be shown or hidden
preferences-alerts-col-show = Show
preferences-alerts-col-label = Alert

## The Quick Preferences panel (viewer-quick-preferences): the small
## bottom-right floater of the settings reached-for hourly.

quick-prefs-title = Quick Preferences
quick-prefs-environment = Environment
quick-prefs-env-preset = Preset
quick-prefs-env-time = Time of day
# The environment preset groups.
quick-prefs-env-shared = Shared (region)
quick-prefs-env-daycycle = Region day cycle
quick-prefs-env-legacy = Legacy WindLight
quick-prefs-env-modern = Modern (EEP)
# The times of day.
quick-prefs-time-sunrise = Sunrise
quick-prefs-time-midday = Midday
quick-prefs-time-sunset = Sunset
quick-prefs-time-midnight = Midnight
# The curated default setting rows.
quick-prefs-draw-distance = Draw distance
quick-prefs-max-particles = Max particles
quick-prefs-master-volume = Master volume
quick-prefs-probe-dynamic = Avatars in reflections

# In-world hover tooltips (viewer-hover-tooltips): the static labels the tip
# box adds around the object / owner data it fetches.
hovertip-loading = Loading…
hovertip-owner = Owner:
hovertip-flag-script = Script
hovertip-flag-physics = Physics
hovertip-flag-touch = Touch
hovertip-flag-money = L$
hovertip-flag-drop-inventory = Drop Inventory
hovertip-flag-phantom = Phantom
hovertip-flag-temporary = Temporary
hovertip-prims = Prims:
hovertip-land-impact = , Land Impact:
hovertip-position = Position:
hovertip-distance = Distance:
