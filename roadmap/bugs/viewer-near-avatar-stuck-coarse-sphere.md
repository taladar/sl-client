---
id: viewer-near-avatar-stuck-coarse-sphere
title: A nearby avatar stays a coarse sphere even as the camera closes in
topic: viewer
status: bugs
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
---

Context: [context/viewer.md](../context/viewer.md).

On aditi an avatar (reported at ~622 m away, on the same parcel, just above the
camera) rendered only as the coarse placeholder **sphere**, and stayed a sphere
even as the camera moved close to it — the full avatar never resolved.

Related to the R22a far/late static-T-pose routing and the R22b interest-camera
work: a coarse-only avatar placeholder that never upgrades to the full body once
it is near/interesting. The avatar was
initially far (its ObjectUpdate may have arrived only as a coarse-location dot,
or its body was requested at coarse LOD and never upgraded). Check the
placeholder→full-body promotion path (`MeshManager::upgrade_to_finest` /
`apply_rigged_attachments` and the coarse-dot reconcile) for the case where an
avatar starts far and only later comes into interest range — it should promote
to the full mesh body once close, and currently does not for this avatar.

## Investigation (2026-08-11) — confirmed NOT a viewer render drop

Traced the promotion path in code. The **only** path that turns a coarse avatar
into a body is a full `pcode==47` `ObjectUpdate`: `AvatarState::apply_coarse`
(`avatars.rs`) only ever spawns / moves a placeholder **sphere** and never
re-evaluates a coarse entry by camera distance; `AvatarState::apply_object` is
the sole promotion (it removes the sphere and spawns a body whenever the
`AvatarBody` resource is present, which it is under `--viewer-assets`). So a
**persistently-stuck sphere means the simulator never sent a full ObjectUpdate**
for that agent — the avatar existed only as a `CoarseLocationUpdate` dot. This
is a "never streamed" case, not a decode / bind / LOD drop in the viewer.

Leading cause: the announced **draw distance gates the sim's interest list**.
`DEFAULT_DRAW_DISTANCE_METRES = 512.0` (`session.rs`), announced via
`Command::SetDrawDistance` (`apply_draw_distance`). The reported avatar was
**~622 m away — beyond the 512 m interest radius**, so the sim builds its
interest list around 512 m and never streams that avatar's full object; only the
region-wide coarse dot arrives.

Why flying closer did not resolve it (needs live confirmation): the
`report_camera_interest` path (`session.rs`) reports the camera eye as
`CameraCenter` **only when the camera actually moves** — a static third-person
view following the avatar never re-points at the distant avatar — and even when
adjacent, the announced `RenderFarClip = 512` still caps the interest radius.
The R22 progress note independently records that the interest camera "does not
resolve the spheres live." Also note coarse dots carry a region offset
(`apply_coarse`); if the avatar were actually in a **neighbour** region its full
object needs that region's object stream — another "never streamed" route (the
"same parcel" report argues same-region).

### Candidate fixes (verify live on aditi, do not blind-fix)

- Raise the default draw distance / `RenderFarClip` (the direct lever for the
  622 m case) and confirm the sim then streams the full object.
- Fire `report_camera_interest` without requiring camera movement (or on a
  periodic tick) so a static follow-cam still points interest at a distant
  avatar, and verify the sim honours camera-only proximity within the far clip.

### Live diagnostic

`SL_VIEWER_LOG_AVATAR_INTEREST=1` enables `log_avatar_interest_census`
(`avatars.rs`) plus the per-arrival `"R22b full avatar object"` logs — this
distinguishes "never streamed" (no full ObjectUpdate ever logged for the agent)
from "streamed-but-unrendered" (logged, but no body). Confirm which before
touching code.

## From gridspec-avatar-presence (2026-10-08) — the draw distance is not it

`avatar-presence` put one of our avatars 1,123 m above another on aditi
(Ahern), the watcher's draw distance at 64 m: the far avatar was never
killed, and when the watcher left for a neighbouring region and came back
it was sent the far avatar's full object 1.2 s after arriving
(`book/src/gridspec/avatars.md`, *How far an avatar is sent*). So a
same-region avatar is streamed whatever the draw distance and however high,
and the "leading cause" above is wrong; raising `RenderFarClip` would not
have helped.

What is left to check live, with `SL_VIEWER_LOG_AVATAR_INTEREST=1`:

- whether the resident was in the agent's own region at all — a region the
  viewer holds no child circuit to sends only what its neighbour relays,
  and a coarse height at the 1,020 m ceiling is how the feed states "above
  what I can say";
- whether the object did arrive and the viewer dropped it (an update for an
  avatar seated on something not yet streamed, say).
