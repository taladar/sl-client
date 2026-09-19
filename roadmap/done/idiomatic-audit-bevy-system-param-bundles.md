---
id: idiomatic-audit-bevy-system-param-bundles
title: 128+ too_many_arguments suppressions are Bevy systems that want a SystemParam bundle
topic: idiomatic
status: done
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/idiomatic.md](../context/idiomatic.md).

The workspace carried **338** `#[expect(clippy::too_many_arguments)]` attributes
when this sweep started (293 when the audit was written; the binary extraction
redistributed them). Every one carries an honest, bespoke reason about Bevy
system parameters, so these are **not** defects — the lint genuinely cannot tell
a system signature from a bad API. But the idiomatic fix is already in the
codebase's vocabulary: `#[derive(SystemParam)]`.

Scope: sweep the largest clusters into `SystemParam` bundles, in a codebase
whose stated convention is no `#[expect]`. Start with `menu.rs`, then
`sl-viewer-edit`.

## Swept so far (2026-09-19): 338 → 13, all thirteen deliberate

Every crate is now at **zero** suppressions except the two that keep theirs on
purpose: `sl-proto` (11) and `sl-viewer-world-objects` (2), both explained at
the end. The largest clusters:

- **`sl-viewer-ui-widgets` (16)** — `menu.rs`'s thirteen collapse into one
  `MenuNav` (the ancestry / children / conditions / slots / direction / filter /
  hosts / branches / free-menu / commands the open-and-close paths all wanted)
  plus a plain `MenuBuildCtx` for the popup builder chain; `toggle_host`,
  `open_host`, `open_submenu_popup`, `commit_row` and `switch_bar_menu` become
  methods on it. `ui_color_picker.rs` gains `PaletteStore` and `PickerVisuals`,
  `floater.rs` `FloaterStack` + `FloaterParents`. Net −190 lines in `menu.rs`.
- **`sl-viewer-edit` (46)** — `edit_material.rs` (15) around
  `RenderFaceLookup` / `MatShownFacts` / `LegacyFaceEdit` / `PbrFaceEdit` /
  `FacePaint`, with `allowed()` / `preview()` / `commit()` / `apply_override()`
  as methods; `edit_texture.rs` (8) around `TexFaceEdit` + `PreviewPaint`;
  `edit_selection.rs` (6) around `HighlightWorld` (the two outline reconcilers
  were the same reconcile twice), `FaceCursorWorld`, `SelectTargets`,
  `RubberBand`; `gizmos.rs` (6) around `GizmoRig`, `DragTargets`,
  `GizmoCameras`, `SelectionPose`, `ScreenView`, `GizmoReadout` and the plain
  `AxisRuler` / `DragStart` records; `edit_contents.rs` (5) around
  `ContentsStore` / `ContentsModel` / `ContentsSurfaces` / `ContentsOutbox` /
  `RenameWidgets`; `edit_tool.rs`, `edit_params.rs`, `edit_create.rs` (2 each).
- **`sl-viewer-world-objects` (31 → 2)** — the key primitive is
  `textures.rs`'s `FaceStores<'a>`: the materials, the texture manager, the
  decoded store and the per-prim textures, which are the four stores every path
  that composes a face material writes through, with `FaceStoresParam` as its
  `SystemParam` wrapper and a
  `TexturePatch` bundle for the three drape/patch systems. `objects.rs`'s
  eighteen fold into `GeometryStores` and the `FaceBuild<'a, 'w, 's>` borrow
  struct it hands out (commands + meshes + `FaceStores` + mesh manager +
  geometry/material caches + `FaceReuse` + `MaterialInternContext`), plus
  `SculptSource` and the `PickedObject` / `PickedFace` records. The two left in
  `name_tag_billboard.rs` stay: both are trimmed verbatim ports of stock Bevy
  text systems whose reason says the argument list mirrors the original's so the
  file stays diffable against upstream.
- **`sl-viewer-world-avatar` (30)** — `avatars.rs` around `AvatarRig`,
  `BakePaint`, `AppearanceFold`, `AppearanceAssets` and `AppearanceBody`;
  `animations.rs` around `AnimationState`, `AvatarSockets` and the plain
  `SocketTarget`; `rigged_attachments.rs` around `RiggedStores`,
  `RiggedMirrors` and `RiggedBind`; `gpu_avatars/` around `ComputeContext`
  (shared with the spike),
  `StageTargets`, `AnimeshCensus`, `WatchedGpuState` / `WatchedView` /
  `OwnWatchFacts`, and the plain `BlendParams`; plus `LookAtFrom`, `ReachPick`,
  `OwnAvatar`, `FirstPersonGate` / `FirstPersonTargets`, `JellydollTargets`,
  `ComplexitySources`, `NameTagFacts`, `RowCells`.
- **`sl-viewer-world-view` (28)** — `camera.rs` around `CameraInput` (context +
  modifiers + buttons + motion + wheel + actions, with `alt()` / `ctrl()` /
  `scroll()` / `moving()` as methods), `UiHover`, `FlycamAxes`, and for
  `position_camera` the four bundles `CameraState` / `OwnAvatar` / `SeatCamera`
  / `CameraColliders` plus `CameraOut` — which also replaces the two tuple
  params the system had been squeezed into; `apply_pose` takes a plain
  `PoseTarget`. `media_prim.rs` gains `MediaStores` and the disjoint-borrow
  `MediaPaint` it splits into, `MediaWorld`, `MediaInput`, `MediaPickIo`,
  `MediaHoverState`; `hud_pick.rs` `TouchPointer` / `TouchTargets` / `TouchOut`;
  `media_controls.rs` `MediaBarState` / `BarRuntime` / `BarProjection` /
  `BarPointer`; `scene_dump.rs` one `DumpSources` for the whole photograph plus
  a plain `AvatarEntry`; `physics.rs` `ColliderSources`; `gpu_pick/render.rs`
  `PickRenderWorld`; `movement.rs` `MovementInput` / `MovementWorld` /
  `CameraGaze`; `arrival.rs` `ArrivalCamera`; `hover_tooltip.rs` `HoverGesture`
  / `HoverOut`; `screenshot.rs` `CaptureRig`.
- **`sl-viewer-people` (28)** — the panel clusters. `contact_sets_panel.rs`
  around `ContactSetsList` / `ContactSetsPanelState` / `ContactSetsIntents` /
  `AddToSetFloater` / `ConfigWidgets`; `radar.rs` around `RadarSweepWorld` /
  `RadarSweepOut` / `RadarAlertChannels` / `RadarViewState` / `RadarWidgets` /
  `RadarClick` / `RadarNames` / `RadarMenuOut` / `RadarActionModel` /
  `RadarActionOut`; `avatar_profile.rs` around `ProfileSources` / `ProfileHost`
  / `ProfileActionFacts` / `ProfileActionOut` / `WebStatusSources`;
  `conversations.rs` around `ConversationFacts` / `ConversationChrome` /
  `RefreshMemo`; plus `BlockedSelection` / `BlockedOut`, `GroupRowClick`,
  `GroupProfileHost`, `NoticeSinks` / `NoticeOut`, `OfferSinks` / `OfferFacts`,
  `ActivityInput` / `AutoRespondFacts`, `PeopleChrome` — and the one non-Bevy
  case, an eight-scalar `triangle` rasteriser that now takes its three vertices
  as one array.
- **`sl-viewer-inventory` (22)** — the crate that had already hit **Bevy's own**
  16-parameter cap and answered it with anonymous tuple params. Those become
  named, documented bundles with the same field names, so the bodies did not
  move: `InventoryStashes` / `InventoryMenuOut` / `MenuActionContext` in
  `inventory_actions.rs`, and in `inventory_drag.rs` the six of
  `on_row_drag_end` — `DragSession` / `DragGeometry` / `DropTargets` /
  `WorldDrop` / `DragOutputs`, plus `DragStartFacts` / `DragHover` / `DragModel`
  / `PointerOcclusion`. Also `InventoryMenuFacts` / `InventoryMenuPick` (shared
  with the gallery), `CreateSinks` / `CreateStores` / `AddActionFacts`,
  `HotkeyGate`, `GearWidgets`, `RevealTargets`, `RebuildSources`,
  `RowPressGeometry` / `RowPressState`, `RenameWidgets`, `GallerySources`,
  `PropertiesSources` / `PropertiesHost`, the two recursive tree emitters'
  `EmitFiltered` / `EmitMembers`, and `SampleRow`.
- **`sl-viewer-places` (21)** — `about_land.rs` around `LandWorld` /
  `LandOpenState` / `LandOpenSinks` / `LandFacts` / `EnableGates` /
  `LandActionFacts` / `LandPickers`; `about_region.rs` `RegionEnableGates` /
  `AccessNames` / `RegionActionOut`; `about_landmark.rs` `LandmarkNames` /
  `LandmarkContent` / `LandmarkRefs` / `DetailRefs`; `telehub.rs`
  `TelehubFacts`; `top_objects.rs` `TopObjectsButtons` / `TopObjectsFacts` /
  `TopObjectsActionOut`; and `teleport_progress.rs`'s six per-part overlay
  queries as one `OverlayParts`.
- **`sl-viewer-world-scene` (15)** — one or two per renderer, nearly all of them
  the same shape: the asset stores a pass writes through. `BeaconTarget` /
  `BeaconStores` / `BeaconWidgets` / `BeaconOverlayState`, `DebugBeaconStores`,
  `BorderStores`, `ProbeBooks`, `WaterStores`, `SkyStores` / `DiscStores` /
  `DiscLog`, `TerrainStores` / `TerrainWork`, `SplitSources`,
  `ParticleStores` / `ParticleTuning`, and the three plain records `HazePass`,
  `PrimSpec` and `EmitInto`.
- **`sl-viewer-map` (13)** — `minimap.rs` around `MinimapWorld` /
  `MinimapPointer` / `MinimapImages` / `LayerSources` / `DotFacts` /
  `HoverLookup` / `HoverLabels` / `ClickWorld` / `ClickOut` / `MinimapNames` /
  `MinimapMenuOut` / `MinimapActionOut`; `world_map.rs` around `WorldMapWorld` /
  `WorldMapPointer` / `LocationWidgets` / `WorldMapData` / `MapFetchAs` /
  `MapLabels` / `WorldMapSearchWidgets`.
- **`sl-viewer-environment` (11)** — `my_environments.rs`'s three anonymous
  tuple params become `EnvActionInputs` / `EnvActionStashes` / `EnvActionOut`,
  plus `EnvListWidgets` / `EnvRowPick` / `EnvActionContext` / `EnvSelectState`;
  `settings_editor.rs` `EditorOpenOut` / `EditorButtonGate` /
  `EditorButtonStashes` / `EditorButtonOut` / `EditorFacts`;
  `day_cycle_editor.rs` `DayFacts` / `DayChromeText` / `DayChromeOut` /
  `DayButtonGate` / `DayButtonOut`; `land_environment.rs` `LandControls` /
  `LandChrome` / `LandActionOut`; `bulk_import.rs` `BulkOut`.
- **`sl-viewer-notices` (10)** — `notification_host.rs`'s `RaiseTargets` /
  `RaiseGate` / `RaiseOut` / `ResolveWidgets`, and `adopt_toast`'s five
  identity arguments as one `ToastSpec` — which is what its own suppression
  reason already called them ("the toast's identity is genuinely this many
  independent facts"), and which makes its six call sites name the `None` and
  the priority they were passing positionally. Plus `PickerOut`,
  `ProfileWhere` / `ProfileWidgets`, `ExperienceNames` /
  `ExperienceListWidgets` / `ExperiencesPick` / `ExperiencesButtonState` /
  `ExperiencesOut`, and `InspectorHost`.
- **`sl-viewer-ui-context-menus` (8)** — `MenuConditionFacts` (identity,
  parcel, ground-sit, friends: the four facts every one of the four menus greys
  its entries from) in a new `menu_params.rs`, plus `AttachmentActionOut`,
  `ObjectMenuModel` / `ObjectMenuOut` / `SeatPoses`, `PickOcclusion` /
  `RightClickOut` / `AvatarMenuOut`. Three of the four menus reach the Build
  tools the same way, so `edit_tool.rs` grew the sweep's second shared bundle,
  `BuildToolsSurfaces`.
- **`sl-client-bevy-viewer` (7)** — `SupportFacts`, `TeleportGate`,
  `ShotWidgets`, `StatusFacts`, and the menu bar's `TopMenuFacts` /
  `TopMenuState` / `TopMenuOut`. The odd one out is `run_session`, which is not
  a Bevy system at all: the last loose startup knobs become `SessionContent`
  beside the `CameraStartup` / `CaptureStartup` / `SkinRuntime` / `MediaRuntime`
  the file had already grown for the same reason.
- **`sl-viewer-search` (5)** — the Search button and the `Enter` shortcut are
  one action behind two triggers, so `run_new_search`'s nine arguments become
  `SearchRun` with `run()` / `search_field()` on it and both systems shrink to
  two or three parameters. Plus `SearchRowWidgets` and `DetailActionOut`.
- **`sl-viewer-preferences` (5)** — `DebugViewWidgets`, `DebugDetailWidgets`,
  `DebugFieldCommit`, `BindingWalk` / `PreferencesOkOut`, `PrefFilterRows` /
  `PrefTabTree`.
- **`sl-viewer-asset-editors` (5)** — Revert and a close-without-saving put the
  *same* four calls' worth of original asset back on the avatar, so both now go
  through `WearPreview::restore`; beside it `WearSaveOut`, `WearOpenWidgets`,
  and two positional-argument records: `EditorShape` (whose call sites read
  `true, source, true, font, false`) and `ParamSliderSpec`, which replaces a
  `(i32, bool, f32, f32, f32, String)` tuple carried through three functions.
- **`sl-viewer-audio` (3)** — the grid's sound events and the viewer's own
  collision layer start a sound identically, so both take one `SoundStage`
  (clock, clip cache, sound state, object mirror, mute list, parcel
  audibility); plus `ParcelAudioChrome`.
- **`sl-conformance` (3)** — `LoginSpec` for the two login entry points, whose
  call sites ended `&state_dir, args.force, None`; `RunResult` for what a
  finished case leaves behind, as against the four arguments saying where the
  record goes.
- **the singles and pairs** — `RlvRun` shared by the RLV console and the
  owner-say intake; `GroupPickOut` / `AvatarPickOut` (and both pickers now use
  the shared `FloaterHost`); `LayoutTriggers` / `LayoutGateWalk` /
  `DirtyCategories` for the UI layout gate; `ParcelRegionSpec` (a call that
  read `0.25, -128.0, -128.0, 256.0, colour, true, false, 64`) and
  `ParticlePipelines`; `GalleryStage`; `WaitChannels` and `WorldStores` /
  `WorldPolicies` in the fake grid — the last two named by the suppression's own
  reason, which already split the list that way in prose; `WebChrome`;
  `ChatOverlayGates`; `SlSinks`; `ReplStreams` / `ReplOut`; `SymbolEntry`; and
  `sl-tree`'s `gen_branch`, where the reference's recursive parameter list
  splits cleanly into what never changes (`BranchTemplates`) and the per-level
  state (`BranchLevel`).

The sweep produced two **shared** bundles. The first is `FloaterHost` in
`sl-viewer-ui-widgets::floater`, the injected form of `host_floater`'s
`parents` + `floaters` pair. That pair has 51 call sites across the workspace
and three crates had each grown a private copy of it during this sweep, so it
now lives beside the function it wraps. The second is `BuildToolsSurfaces` in
`sl-viewer-edit::edit_tool`, the injected form of `open_build_tools_with`'s
floaters + panels + tool state, which the land, attachment and object menus had
each been spelling out.

Patterns worth reusing:

- A widget-query bundle absorbs the `FontCx` / `LayoutCx` / `InputFocus` a
  programmatic `EditableText` rewrite needs — they belong to the rewrite, not to
  the system.
- A bundle holding `Commands` alongside the queries it writes through works
  because Rust borrows struct fields disjointly: a method can hold
  `self.hosts.get_mut(..)` and `&mut self.commands` at once.
- When a helper needs a *live* borrow out of one field while writing through the
  others (one media face's `ActiveMedia` while its material is repainted), give
  the bundle a `split()` returning `(&mut State, Borrows<'_, ..>)` rather than a
  `&mut self` method — `MediaStores::split` / `MediaPaint`.
- `missing_debug_implementations` only fires for **publicly reachable** types,
  so a `pub(crate)` or private bundle holding `Commands` / `Assets<T>` /
  `MessageWriter` needs no `#[expect]`; a `pub` one does.
- A `Local<'s, T>` is a `SystemParam` and belongs inside the bundle whose state
  it carries (`MediaPickIo`'s throttle, `SeatCamera`'s was-engaged flag).
- A plain borrow struct must not put two **invariant** Bevy types under one
  lifetime: `MessageWriter<'w, T>`, `Query<'w, 's, ..>` and `Commands<'w, 's>`
  are invariant, so `&'a mut MessageWriter<'w, A>` beside
  `&'a mut MessageWriter<'w, B>` forces the two `'w`s equal and the call site
  cannot satisfy it. Give each its own lifetime (`CreateSinks<'a, 'wire, 'ui>`)
  — or bundle the **shared references** instead, which are covariant and just
  work (`LandmarkRefs`, `AccessNames`).
- Destructure a bundle **by value** (`let Facts { a, b } = facts;`), not by
  reference: `= &facts` leaves each binding a `&Res<T>`, and every `&a` at a
  call site then trips `needless_borrow`.

`sl-proto` went 13 → 11, and the eleven that stay are a **decision, not a
backlog**. Every one of them says the same thing — "mirrors the wire block
one-to-one" — and they mean it: the parameter list *is* the protocol record, in
the order the message template declares it, so the builder reads against the
template. Putting a hand-written struct beside the generated block type would
add a second name for the same record. The two that did go were not wire
mirrors: `decompress_patch` / `encode_patch` share three precomputed codec
tables, now one `PatchTables`.

## What stays (13, all deliberate)

The eleven `sl-proto` wire-block mirrors above and the two stock-Bevy
text-system ports in `name_tag_billboard.rs`. Nothing else is left to sweep.

Not in scope, and worth recording so it is not re-litigated: the remaining cast
suppressions (152 `as_conversions`, 105 `cast_possible_truncation`, 63
`cast_sign_loss`) are overwhelmingly load-bearing numeric conversions with
checkable reasons, and the 134 `module_name_repetitions` ones all say
"re-exported at the crate root" — if that repetition is the house style, turning
the lint off workspace-wide beats 134 local suppressions.
