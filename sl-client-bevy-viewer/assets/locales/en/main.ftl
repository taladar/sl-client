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
notecard-readonly-note = You do not have permission to modify this notecard.
notecard-embedded-header = Embedded items:
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
