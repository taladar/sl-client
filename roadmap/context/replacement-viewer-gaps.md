# Replacement-viewer gaps (drafted 2026-09-21, updated 2026-09-27)

What is still **missing as a feature** — a whole floater, a whole
subsystem, a thing a resident does every day — before this viewer can be
somebody's only viewer, in the order it is worth building. Drafted over the
548 open roadmap tasks of 2026-09-21; as of 2026-09-26 there are 550 (302
ready, 118 ideas, 62 blocked, 30 deferred, 25 in-progress, 13 bugs). The
five days between were almost entirely skin work, which closed the skin
system and shipped Vintage (see *The skin* below); no feature gap on this
page closed or opened in them, and every task status quoted here was
re-checked on the update.

## Where we are (updated 2026-09-27)

Progress through the list below, section by section. The next section to
work on is first. Update this table whenever a gap closes or opens.

| Section | Closed | Open |
| --- | --- | --- |
| Partly overtaken | `viewer-object-rezzing`, `viewer-menu-touch-object` | `viewer-sit-stand-actions` residue: **next** |
| Tier 0, session boundary | nothing yet | `viewer-login-screen` (then `viewer-login-tos`), `viewer-disconnect-screen`: **next tier** |
| 1.1 Money | nothing yet | all |
| 1.2 Gestures, AO | nothing yet | all |
| 1.3 Outfits | the worn state (`viewer-inventory-worn-before-attach-confirmed`) | the outfit editor, favourites, hover height |
| 1.4 Landmarks | nothing yet | all |
| 1.5 Reading and hearing | notecards, streaming audio, media on a prim | `viewer-video-playback` (in progress), `viewer-media-playback-policies` |
| 1.6 Files in and out | the open-file dialog | upload, save dialogs, the other portals |
| Tier 2, creators | nothing yet | all |
| Tier 3, parity comfort | nothing yet | six in progress; see the families |
| Tier 4, after parity | nothing yet | all |

Found on the way, not yet placed in a tier (2026-09-27, all while testing
the worn-object Touch):

- `parcel-properties-update-via-udp-poisons-opensim` (**bug**, under *Land*
  in tier 3). Parcel edits go over UDP. On OpenSim one edit breaks every
  later save in the region, rezzing and attaching included, so it goes
  before any further live testing that rezzes on OpenSim.
- `viewer-rlv-send-side-consumers` (under *RLV* in tier 3). Nothing in the
  viewer asks the RLV façade.
- `viewer-object-moved-region-reuse`. An object that changes region is
  rebuilt rather than moved.
- `fake-grid-own-attachments-and-region-moves`. The offline grid models none
  of the attachment, COF or region-move behaviour behind these fixes.

## The baseline this measures against

The viewer is already far past a tech demo, and the gaps below only mean
something against what works today: log in from a credentials file and
render the region (terrain, prims, mesh, sculpt, flexi, animesh, particles,
trees, local lights, shadows, reflection probes, sky / water / EEP), walk,
fly, sit, teleport within and across regions, world map + minimap + radar,
the inventory tree / gallery / worn tab with wear and COF maintenance, the
appearance editor for body parts and clothing, client-side bake, nearby chat
plus IM / group / conference conversations, the full ported notification
catalogue, people / friends / groups / profiles, search, About Land and
About Region, the build floater's create + transform + texture tabs, pie
menus on every pick target, preferences, the snapshot floater, the audio
backend with in-world sounds, the RLV command engine, and three shipped
skins (Graphite, Azure and the reference's classic Vintage) chosen by
`--skin` or the preferences Colors & Skins tab.

So the question is not "does it work" but "is anything a resident reaches
for on an ordinary day simply *absent*".

## What is deliberately not on this list

Of the 548 open tasks, roughly 180 are not features in this sense and are
excluded on purpose:

- **Audits and refactors** (`*-audit-*`) — internal shape, no user-visible
  behaviour.
- **Performance** (`viewer-perf-*`, ~45 tasks) — these make an existing
  feature usable, not a missing feature present. They rank on their own
  axis, against measurements, not against the reference's feature list.
- **Skin / theming** (`viewer-skin-*`, `viewer-vintage-*`) — parity of
  *look*, measured in [`vintage-skin.md`](vintage-skin.md). Largely done
  now; the residue and why it still stays off this list are under *The
  skin* below.
- **Test and harness tiers** (`test-*`, `viewer-render-*` matrices,
  `viewer-ui-baseline-*`) — how we know something is right.
- **The grid side** (`server-*`, the LSL engine tranches) — `sl-fake-grid`
  is a test target; nobody logs into it to live there.
- **Upstream work** (the four `viewer-ui-text-parley-*` items) — filed
  against `linebender/parley`; nothing of ours waits on them.

## The skin

When this list was drafted the skin was a set of colour tokens over widgets
that still painted most of their own state from Rust, and Vintage was a
blocked capstone. Between 2026-09-21 and 2026-09-26 that whole programme
landed — eighteen `viewer-skin-*` tasks are done:

- **State lives in the cascade.** Hover, press, focus, selection, toggle and
  refusal are CSS pseudo-classes and `.sk-*` classes the skin sheet styles
  (`:active` is a press, `.sk-selected` a selection, `:checked` a toggle); no
  panel repaints its own greying or selection any more, and the last
  per-frame colour writes left Rust.
- **Every text node names a role**, including the ~430 hand-built labels
  and table cells that used to take a literal colour, with a test holding
  the role-from-colour map injective.
- **One widget per control.** Button, checkbox, radio, tab strip,
  scrollbar and tooltip are each a single skinned widget used everywhere,
  and the floaters' controls are those widgets, so a skin reaches all of
  them.
- **A skin can change shape, not only colour**: nine-sliced art per widget
  state (`sl-viewer-skin-art` draws it — the reference's art is not ours to
  copy), light data surfaces distinct from the chrome, bevels, text shadow,
  row striping, focus-ring geometry, and glyph slots for every decorative
  mark.
- **Vintage shipped** (`viewer-vintage-skin`, done 2026-09-26): the merged
  reference palette, stadium push buttons with the gold press, sage fields
  and lists with black text, steel-blue shadowed labels, and switching back
  to Graphite leaves nothing behind (tested).

What is left, and why it still is not a gap on this list — none of it stops
anyone from using the viewer, it only makes Vintage more exact:

| Task | Status | Residue |
| --- | --- | --- |
| `viewer-skin-icon-set` | ready | Inventory icons are emoji chosen in Rust; the legacy icon set is half of what makes Vintage recognisable, and no skin can reach it yet. |
| `viewer-vintage-ui-chrome-crosscheck` | ready | Record a chrome-capture pair per skin. The harness side is done (both viewers wear the chosen skin, same size or a stated failure) and the pair was waiting only on Vintage existing, which it now does. Floaters stay out of a capture by design. |
| `viewer-vintage-radar-range-colours` | bug | The radar's range column uses the name-tag colours, unreadable on a light list. |
| `viewer-parcel-audio-bar-backing-unskinned` | bug | A hard-coded dark strip behind the parcel audio cluster. |
| `viewer-vintage-bottom-bar` | blocked | Vintage's one *layout* difference. Blocked on `viewer-animation-overrider` (1.2) and `viewer-toolbar-customization` (tier 3), so it rides on those rather than being scheduled on its own. |

One skin item does sit on the feature axis, under 1.6: reading the desktop's
light / dark preference needs the Settings portal of
`viewer-os-portals-linux`.

## Three tasks the code had partly overtaken

Found while compiling this list, and verified 2026-09-24 (still so on
2026-09-26): each is partly done and now `in-progress`, its file rewritten
down to the residue, so none of them should be scheduled as a gap:

- `viewer-object-rezzing` — closed 2026-09-27. The drag-rez was already in
  place (`inventory_drag.rs`, plus the separately fixed link case). The
  residue was Restore to Last Position, a rez coming back selected while the
  build tools are open, and refusing to rez a worn attachment. The parcel
  pre-check was dropped: the reference has none, and lets the simulator
  refuse.
- `viewer-menu-touch-object` — closed 2026-09-27. The object and attachment
  pies already sent `Command::TouchObject`. The residue was the inventory's
  worn-object Touch, which now touches the attachment root in the
  object-centre form. The reference's RLV gate on it went to the new
  `viewer-rlv-send-side-consumers` (see *RLV* in tier 3).
- `viewer-sit-stand-actions` (ready) — its own body says it was "largely
  completed" by `viewer-sit-target-and-stand-button`; audit and close or
  re-scope to the residue.

## Tier 0 — the session boundary

Three tasks, and they are first because they are what a *new user* hits
before anything else on this list can matter.

| Task | Status | Why first |
| --- | --- | --- |
| `viewer-login-screen` | ready | There is no front door. Login is `--credentials <toml> --grid <name> --avatar <key>`, the password sits in plain text on disk, the grid is a CLI flag and MFA is a prompt on stdin. Nobody who is not us can log this viewer in. |
| `viewer-login-tos` | blocked on the above | Second Life hands a returning account a TOS or critical-message interstitial and the login **cannot be completed** without answering it. Today that login simply fails. |
| `viewer-disconnect-screen` | ready | On any `LoggedOut` / `Disconnected` the process **exits**. Every dropped connection is indistinguishable from a crash, and the reason the grid gave is lost with the window. |

`viewer-crash-reporter` (ideas) belongs to the same family — there is no
crash handling at all — but it is a tier below: it costs a user a session,
where the three above cost them the viewer.

## Tier 1 — ordinary residency

The things a resident does on an ordinary evening that this viewer cannot
do at all. Ranked within the tier.

### 1.1 Money — you cannot spend the balance the status bar shows

`viewer-money-economy-ui` (ready since 2026-09-27, when
`viewer-media-prim-browser` closed) is the whole economy surface: pay a resident
or object, buy an object, buy land, buy L$, transaction history. The wire side
is done (`api-g6` object purchase / pay, `protocol-43` balance). Shopping is the
single most common thing residents do, and today none of it is reachable.

Its satellites, each small once the core exists:
`viewer-object-pie-buy-take-chain` (the pie's two `Buy` slices are greyed
because buying is unwired), `viewer-land-transactions` (sell / deed /
abandon / reclaim / buy pass), `viewer-inventory-marketplace-operations`,
`viewer-marketplace-shop-link`.

### 1.2 Gestures and the AO — the avatar cannot express anything

`viewer-gesture-runtime` (ready) sequences a gesture's animation / chat /
wait steps and fires it from its `/`-trigger; it unblocks
`viewer-gesture-management-ui` and `viewer-input-gesture-bindings`. Beside
it, `viewer-animation-overrider` — the client-side AO, described in its own
file as one of Firestorm's most-used features, and cheap here because the
locomotion state machine that picks the default animations is already ours.
The AO is also one of the two blockers of `viewer-vintage-bottom-bar`, whose
utility cluster carries its button.

The recovery verbs sit in the same place: `viewer-stop-all-animations`
("I'm stuck in a pose"), `viewer-resync-animations`, and the Rebake half of
`viewer-avatar-debug-tools` — the universal "I am a cloud" fix, which every
resident uses and we have no path to.

### 1.3 Outfits — you can dress, but you cannot keep what you put on

Wearing works; *composing* does not. `viewer-outfit-editor` is the Edit
Outfit surface — the current outfit by category, add / take off per row,
Save and Save As into My Outfits — and it unblocks
`viewer-outfit-layer-reorder`. `viewer-wearable-favorites` is the quick
jewellery box over the same machinery, and `viewer-hover-height` (plus its
ingest half `viewer-agent-hover-height-ingest`) is the slider every mesh-body
user reaches for within a minute of arriving.

What those build on became trustworthy on 2026-09-27
(`viewer-inventory-worn-before-attach-confirmed`). An attachment is worn now
exactly when its object is on the avatar, as in the reference. Before, it
was worn when a wear was requested, whether or not the simulator attached
anything, and only the inventory's own Detach un-wore it. Attachment COF
links follow the objects too: written on arrival, dropped on a real detach,
and kept across a teleport. The same testing found and fixed a world-layer
bug that affected every neighbour teleport on OpenSim. The old region never
killed our own avatar and attachments, so their stale copies lingered on the
HUD and in the worn state. The session now treats an object arriving from
another region under a known full id as having moved there, which is the
reference's full-id-keyed object list.

### 1.4 Landmarks and navigation

`viewer-places-landmarks` is the navigation hub: My Landmarks with
teleport-on-double-click, "Landmark This Place", and teleport history. A
viewer where the map works but landmarks do not makes every trip a manual
search. `viewer-world-map-tracking-teleport` hands the map's tracking to the
in-world beam and its double-click to the teleport flow;
`viewer-os-slurl-handler-linux` makes a SLURL clicked in a browser or chat
app actually land in the viewer; `viewer-autopilot-click-to-walk` is
click-to-go. `viewer-navigation-favorites-bars` (deferred) is the location
bar and favourites bar the default skin has and Vintage does without.

### 1.5 Reading and hearing what the world hands you

`viewer-notecard-editor` (notecards are how SL ships instructions, landmark
packs and freebies) closed on 2026-09-27: an item dropped into the body goes
where it was dropped, and a click on an embedded sound, landmark or material
does what the reference's does. `viewer-streaming-audio` (parcel music — a
club with no stream is not a club) closed on 2026-09-27 with the Nearby Media
window: every media source around the agent, each with its own controls. The
third,
`viewer-media-prim-browser`, closed on 2026-09-27 with the per-face media
editor (the Texture tab's Media mode and the Media Settings window), the
page's cursor and IME composition; its zero-copy headroom is the deferred
`viewer-media-zero-copy-frames`.
`viewer-media-playback-policies` is the domain allow/deny and first-click
policy layer around them, and `viewer-video-playback` (in progress) is the
second media engine for `video/*`.

### 1.6 Getting files in and out

`viewer-image-upload` leads here: texture / sound / animation / bulk upload
with the L$ cost, which in turn unblocks `viewer-snapshot-to-inventory` —
the destination that makes the snapshot floater useful — and pairs with
`viewer-default-creation-permissions`, which decides what every upload and
every new prim is born with.

It is **not** gated on a file dialog, contrary to how this cluster is easy
to read. `viewer-os-portals-linux`'s **open** half already landed
(2026-09-11): `sl_viewer_platform::file_dialog` is an `OpenFileDialog` →
`FileDialogClosed` message pair over `rfd`, whose `xdg-portal` backend
talks `org.freedesktop.portal.FileChooser` over a `dlopen`ed `libdbus`
(falling back to `zenity`), driven on the `IoTaskPool`, single-flight, with
a last-used directory per purpose. `viewer-environment-import-legacy-presets`
built it rather than a private hack, and that choice also settles the
`rfd`-versus-`ashpd` evaluation the portals task asked for.

What is still that task's, and what each piece gates:

- **Save dialogs, folder pickers, multi-select** — only `pick_file` is
  wired, so snapshot save-as, script / notecard save-to-file, object export,
  settings backup and access-list import are blocked; *opening* a file (and
  therefore upload) is not.
- **A parent window** — `rfd::set_parent` wants a `HasWindowHandle` and Bevy
  exposes one only through `RawHandleWrapper::get_handle`, an `unsafe fn`
  this workspace's `unsafe_code = "forbid"` rules out. Fixing it properly
  means a safe accessor upstream, so the dialog is parentless until then.
- **OpenURI, Notification, Settings, Inhibit** — untouched. A URL clicked in
  chat still shells out via `system_browser`; there are no desktop
  notifications for an IM while the window is unfocused, and the skin cannot
  read the OS colour-scheme preference.

## Tier 2 — creators

Nobody builds or scripts in this viewer today. Each of these is a
self-contained subsystem with a clear spine.

### 2.1 Scripting

The spine is `viewer-lsl-editor-widget` — and its file is worth reading
before planning the tier, because stock Bevy 0.19 **physically cannot**
render two colours in one editable text field, so the widget is a parley
`PlainEditor` fork, not an afternoon. It unblocks
`viewer-lsl-editor-highlight`, `viewer-lsl-editor-save-compile` (which in
turn unblocks `viewer-task-inventory-open-and-save-back`) and
`viewer-script-recovery`.

Independent of the widget, and worth doing first because they need no UI
framework: `viewer-script-mirror-download` (mirror grid scripts to a real
directory so nvim / ripgrep / git just work — for a power user probably
worth more than the in-viewer editor) and its `-upload-watch` half, and the
two error surfaces `viewer-script-warning-window` /
`viewer-script-error-window` — today a script that errors at run time talks
to `DEBUG_CHANNEL` and **no viewer crate mentions that channel at all**.
Then `viewer-script-queue` (mass reset / recompile) and
`viewer-script-limits`.

### 2.2 Mesh upload

`viewer-mesh-gltf-import` is the spine; `viewer-mesh-lod-decimation`
(meshoptimizer), `viewer-mesh-physics-vhacd` (parry3d's V-HACD, which we
already ship) and `viewer-mesh-upload-sequence` hang off it, and
`viewer-mesh-preview-floater` / `viewer-upload-model` tie them into the
wizard the reference demands before it will send anything.
`viewer-mesh-cost-estimate` is unblocked and standalone. `viewer-local-mesh`
and `viewer-local-textures` are the iterate-without-paying loop, and
`viewer-material-asset-authoring` closes the GLTF-material inventory verbs.

### 2.3 The build floater's missing half

Sixteen tasks, each small, collectively the difference between "can move a
prim" and "can build": `viewer-build-physics-params`,
`viewer-build-sculpt-controls`, `viewer-build-general-sale-clickaction`,
`viewer-build-probe-animesh-controls`, `viewer-build-create-tool-options`,
`viewer-build-align-tool`, `viewer-build-copy-paste-params`,
`viewer-build-grid-options`, `viewer-build-selection-filters`,
`viewer-build-display-options`, `viewer-build-stretch-textures`,
`viewer-build-texture-tab-fs-extras`, `viewer-build-tool-row-parity`,
`viewer-build-numeric-field-spinners`, `viewer-build-creation-defaults`,
`viewer-build-widget-parity-upgrades`.

Around them: `viewer-edit-permission-gating` (grey out what perms forbid,
rather than letting the sim refuse), `viewer-edit-attachment-behavior`,
`viewer-texture-drag-drop`, `viewer-object-weights` and
`viewer-object-inspect` (the numbers builders watch constantly),
`viewer-particle-editor`, `viewer-area-search` ("where is the thing named
X"), and the posing helpers `viewer-pose-stand`, `viewer-poser`,
`viewer-attachment-align`, `viewer-avatar-alignment-tools`.

## Tier 3 — parity comfort, safety and the rest of the floaters

Individually small, collectively the difference between "works" and "is
pleasant". Grouped by family, not ranked within a family.

**Text and chat.** `viewer-text-field-context-menu` (there is no
right-click Cut / Copy / Paste on any text input — the most basic omission
on this page), `viewer-chat-timestamps`, `viewer-chat-spellcheck`,
`viewer-chat-mention-autocomplete`, `viewer-chat-keyword-alerts`,
`viewer-chat-autoreplace`, `viewer-chat-bubbles`, `viewer-chat-bar-commands`,
`viewer-chat-input-behavior-options`, `viewer-chat-input-world-autostart`,
`viewer-chat-clickable-sender-names`, `viewer-chat-transcript-style-options`,
`viewer-conversation-log`, `viewer-conversation-pane-toolbar` (a
conversation does not show who is in it), `viewer-url-context-menus`,
`viewer-announce-incoming-im`, `viewer-window-title-unread-count`,
`viewer-window-attention-flash`, `viewer-chat-omnifilter`.

**Widgets and floaters** (filed 2026-09-24/25, while the skin work walked
every floater). `viewer-sliders-show-no-value` (a bug, and the worst of
these: no slider in the viewer shows the number it holds, nor its bounds or
step), `viewer-multiline-editor-scrollbar` (multi-line fields scroll with no
scrollbar, and no table scrolls sideways), `viewer-floater-font-size-threading`
(parts of several floaters ignore the font-size setting),
`viewer-i18n-floater-literal-english` (literal English left in floaters) and
`viewer-minimized-floaters-move-to-a-shelf` (ideas).

**Safety and moderation.** `viewer-report-abuse` (the protocol is done; the
form is not — a viewer with no abuse report is not one to hand a stranger),
`viewer-bumps-floater` (the harassment-evidence log), `viewer-anti-spam-filter`,
`viewer-avatar-moderation-actions` (freeze / eject / ban, shared across
every avatar surface), `viewer-group-session-moderation`,
`viewer-parcel-ban-duration`, `viewer-region-entry-maturity-gate` (eight
notifications are ported and nothing raises them),
`viewer-search-maturity-filter` (search asks for every rating whatever the
account may see), `viewer-particle-pick-mute`, `viewer-sound-explorer`,
`viewer-animation-explorer`.

**Diagnostics the user is expected to read.** `viewer-statistics-floater`,
`viewer-performance-floater`, `viewer-graphics-presets` ("crank it down for
this club"), `viewer-debug-consoles`, `viewer-notification-history`,
`viewer-region-debug-console`, `viewer-network-debug-tools`.

**Camera, movement, input, menus.** `viewer-camera-controls-window`,
`viewer-camera-presets`, `viewer-camera-keyboard-controls`,
`viewer-camera-flycam-floater`, `viewer-camera-constraint-plane`,
`viewer-camera-script-control` / `viewer-scripted-followcam-llsetcameraparams`
(scripted vehicles cannot drive the view), `viewer-movement-controls-floater`,
`viewer-input-locomotion-actions`, `viewer-input-modifier-chords`,
`viewer-input-mouse-button-bindings`, `viewer-input-script-control-capture`
(`llTakeControls`), `viewer-input-rebinding-persistence` →
`viewer-input-rebinding-ui`, `viewer-flycam-avatar-movement-keys`,
`viewer-qol-toggles`, `viewer-menu-advanced-shortcuts`,
`viewer-menu-bar-fill-implemented-entries` and
`viewer-pie-wire-ready-placeholders` (both are pure wiring of features that
already exist — the cheapest visible parity on this page),
`viewer-toolbar-customization` (with the AO, the other blocker of
`viewer-vintage-bottom-bar`), `viewer-fullscreen-mode`.

**Land, estate, admin.** `viewer-region-options-estate` and
`viewer-region-options-terrain` (both in progress, terrain's write path
unverified on a live grid), `viewer-land-holdings`,
`viewer-about-land-objects-return`, `viewer-region-estate-object-return`,
`viewer-region-restart-schedule`, `viewer-god-tools`,
`viewer-neighbor-region-parcels` (About Land cannot act on a neighbour
region at all), `viewer-land-access-list-export-import`.
`parcel-properties-update-via-udp-poisons-opensim` (a bug, filed
2026-09-27) comes before any of the parcel-writing ones. Parcel edits go
over UDP rather than the `ParcelPropertiesUpdate` capability. On OpenSim
one edit left the region unable to save anything, rezzing and attaching
included, until a restart. The capability's body is also what
`viewer-parcel-config-missing-writes` is waiting on.

**People and social.** `viewer-recent-people`,
`viewer-people-lists-multi-select`, `viewer-display-name-set`,
`viewer-profile-image-editing`, `viewer-group-insignia-editing`,
`viewer-group-titles-quick-switch`, `viewer-group-notice-attachments`,
`viewer-group-chat-snooze`, `viewer-social-group-extras`,
`viewer-give-calling-card`, `viewer-conference-start-ui` (in progress),
`chat-group-history-server-side` (in progress).

**Voice.** `viewer-voice-audio` (ready) is what turns the per-session
voice-channel state we already model into WebRTC media, mic capture and
speaking indicators; it alone unblocks `viewer-voice-controls` (talk
button, PTT, participants, per-speaker volume), `viewer-voice-call-dialogs`
(incoming / outgoing calls, channel switching), `viewer-p31-10` (lip-sync)
and the two parked conformance cases `test-voice-account` /
`test-voice-signaling`. It is here rather than in tier 1 deliberately: the
signalling half is done but **inert** — nothing in the viewer can hear or
speak, and nothing else in the tree depends on it — so it is a whole
self-contained subsystem to schedule as a block, not a gap that holds up
anything above it. For a voice-centric resident it is of course tier 1; for
everyone else, text chat already works.

**RLV.** The engine is largely ours; the user-facing half is not.
`viewer-rlv-send-side-consumers` (filed 2026-09-27) comes first: the
`RlvActions` façade has no caller in the viewer, so no send-side restriction
(touch, edit, chat, teleport, …) is honoured yet. After it:
`viewer-rlva-floaters-toggles` (in progress), `viewer-rlv-enforce-camera`,
`viewer-rlv-enforce-forced-actions`, `viewer-rlv-enforce-info-hiding`,
`viewer-rlv-vision-render`, `viewer-rlv-blocked-objects`,
`viewer-rlv-console-lines-wrap` (the console cuts a long line behind an
ellipsis where the reference wraps it). For the segment
that uses RLV this is a tier-1 subsystem; for everyone else it is invisible.

**Render features a resident notices.** One of these reads as a defect
rather than an absence: `viewer-antialiasing-post` — MSAA does not work with
a deferred renderer, so without a post-AA resolve every high-contrast edge
in SL shimmers, and the viewer looks worse than the reference on the same
scene. `viewer-antialiasing-sharpen-aniso` pairs with it. Then
`viewer-depth-of-field`, `viewer-screen-space-reflections`,
`viewer-realtime-mirrors` (in progress), `viewer-pbr-terrain` (increasingly
what modern regions use), `viewer-projector-lights-textured`,
`viewer-occlusion-culling`, `viewer-viewer-effect-render` (other avatars'
edit beams are invisible), `viewer-highlight-transparent`,
`viewer-render-type-toggles`, `viewer-debug-render-beacons`,
`viewer-beacons-control`, `viewer-local-light-count-setting`,
`viewer-texture-vram-budget`, `viewer-draw-distance-stepping`,
`viewer-texture-mip-chain-missing`.

**i18n.** `viewer-i18n-locale-selection` — the machinery ships four
bundles chosen for typographic coverage, and the user cannot pick one.

## Tier 4 — after parity

Real features, none of them a reason anyone stays on another viewer:
`viewer-fs-bridge-lifecycle` / `-protocol` (a Firestorm extension, opt-in),
the USB route family, `viewer-photo-hosting-upload`,
`viewer-snapshot-postcard` / `-profile-feed` / `-composition-guides` /
`-highres-quiet`, `viewer-video-recording` (no SL viewer has this),
`viewer-i18n-chat-translation`, `viewer-onboarding-tutorial`,
`viewer-a11y-screen-reader` (the reference viewer has none either),
`viewer-avatar-welcome-pack`, `viewer-destination-guide`,
`viewer-linden-home`, `viewer-grid-status-feed`, `viewer-region-tracker`,
`viewer-support-group-version-tags`, `viewer-combat-health-indicator`,
`viewer-collision-messages-chat`, `viewer-region-script-count-monitor`,
`viewer-settings-backup`, `viewer-stream-favorites`,
`viewer-manual-music-stream-url`, `viewer-object-export-import`,
`viewer-pathfinding-floaters`, `viewer-quick-preferences-editor`,
`viewer-inventory-*` power verbs and QoL toggles.

## The dependency spine

Eight tasks unblock most of the rest. If a round has to choose, choose from
here — but read the tier a task sits in too: unblocking many things and
being urgent are different properties, and `viewer-voice-audio` has the
first without the second.

| Task | Tier | Unblocks |
| --- | --- | --- |
| `viewer-login-screen` | 0 | `viewer-login-tos`, and the viewer being installable at all |
| ~~`viewer-media-prim-browser`~~ (done 2026-09-27) | 1 | `viewer-money-economy-ui`, `viewer-destination-guide`, the L$ purchase and marketplace flows |
| `viewer-image-upload` | 1 | `viewer-snapshot-to-inventory`, `viewer-profile-image-editing`, `viewer-group-insignia-editing`, `viewer-inventory-thumbnails` |
| `viewer-gesture-runtime` | 1 | `viewer-gesture-management-ui`, `viewer-input-gesture-bindings` |
| `viewer-os-portals-linux` | 1 | the **save** half of every disk-touching flow: snapshot save-as, script / notecard save-to-file, export, settings backup, access-list import — plus OpenURI and desktop notifications. Opening a file already works, so upload is not behind it |
| `viewer-lsl-editor-widget` | 2 | the whole scripting tier |
| `viewer-mesh-gltf-import` | 2 | the whole mesh-upload tier |
| `viewer-voice-audio` | 3 | `viewer-voice-controls`, `viewer-voice-call-dialogs`, `viewer-p31-10`, two conformance cases — a self-contained block nothing above it waits on |

## How this maps onto the parallel split

The three-agent ownership split in
[`parallel-work-plan.md`](parallel-work-plan.md) holds unchanged; the tiers
here cut across it, so a round can take one theme per agent:

- **A (world & render)** — 1.2's animation recovery verbs, tier 3's render
  family (lead with `viewer-antialiasing-post`), `viewer-pbr-terrain`,
  `viewer-realtime-mirrors`, `viewer-viewer-effect-render`, the land /
  parcel / region halves of the estate work.
- **B (UI shell & social)** — tier 0 in full, 1.1 money, 1.3 outfits, 1.4
  places, 1.6's `viewer-image-upload`, tier 3's text / chat / people /
  safety / diagnostics / widgets-and-floaters families, and the skin residue
  (`viewer-skin-icon-set`, the two Vintage bugs, the chrome capture pair).
- **C (tools, protocol & server)** — 1.2's gesture runtime, all of tier 2
  (scripting, mesh, build floater), `viewer-os-portals-linux`, the media /
  streaming-audio tasks already in flight, and tier 3's voice block when it
  comes up.

One caution: the two in-progress tier-1 items (`viewer-streaming-audio`,
`viewer-video-playback`) are worth finishing before either opens a new front,
because they are tier-1 entries themselves. The other two,
`viewer-media-prim-browser` and `viewer-notecard-editor`, are done, and the
money UI the first held back is ready.
