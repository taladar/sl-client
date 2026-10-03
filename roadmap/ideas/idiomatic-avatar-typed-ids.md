---
id: idiomatic-avatar-typed-ids
title: Typed avatar ids: global colours, visual params, texture slots, built-in animations
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

- Bake global colours by name: `LayerTint::Global(&'static str)` and
  `global_color(name) -> Option`, consumed with `.unwrap_or(WHITE)` — a typo
  renders untinted skin / hair / eyes. A `GlobalColor { Skin, Hair, Eye }`.
- Visual param ids as bare `i32` across sl-avatar / sl-bake
  (`BTreeMap<i32, f32>`, `LayerTint::Params(&[i32])`) — `VisualParamId`.
- Avatar texture slots as `usize` / `u32` in sl-avatar, sl-bake and the
  viewer (`HashMap<usize, TextureKey>`, `BODY_BAKE_SLOTS: [usize; 6]`), the
  reference's `ETextureIndex` vs `EBakedTextureIndex` mix-up waiting to happen
  — an `AvatarTextureIndex` (and a separate bake index) in sl-proto.
- Built-in animations looked up by name returning `Option`
  (`builtin_animation_by_name("away")` cached as `LazyLock<Option<Uuid>>` in
  name tags; `motion_stops.rs` releasing the movement hold on `"standup"` /
  `"land"`) — generated `ANIM_AGENT_*: AnimationKey` constants or a
  `BuiltinAnim` enum.
- Hand pose crossing as a bare index (`hand_pose_morph_param(pose: usize)` vs
  `HandPose(u32)`), animation priority decaying back to `i32` beside
  `JointPriority`, parallel morph-name lists.
