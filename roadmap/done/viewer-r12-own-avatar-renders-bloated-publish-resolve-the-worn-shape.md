---
id: viewer-r12
title: Own avatar renders bloated — publish/resolve the worn shape
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R12. Own avatar renders bloated — publish/resolve the worn shape**
(`sl-client-bevy-viewer`). Diagnosed by a Firestorm vs local-OpenSim
side-by-side: our own avatar renders with a bloated body and vertices
spiking out of the head/hair **at rest** (no animation), while Firestorm
renders the same account as the correct slender shape. Root cause is the
client-side bake publish (P15.4, `bake_publish.rs`): it advertises a
placeholder **all-`128` "neutral" visual-parameter vector**
(`neutral_visual_params`), but `128` is the range *midpoint*, and most
body-shape morphs are **asymmetric** (default `0`, range `0..N`), so `128`
is ~50% strength on every one → permanent bloat + displaced head/hair. The
own avatar's shape is rendered from the server's
`AvatarAppearance.visual_params` (`apply_avatar_appearance`), which the sim
stores and rebroadcasts from our own `128` publish — so the bloat is
self-perpetuating **per account**. Logging the account into a reference
viewer (Firestorm) once overwrites the server appearance with the real
worn-shape params and permanently corrects our render for that account; a
never-corrected account stays bloated (confirmed: a second test avatar that
never touched Firestorm stays bloated, a Firestorm-corrected one does not).
**Fixed** — the "matching the worn shape" work `bake_publish.rs` had
deferred: `OwnBakeInputs::visual_params` builds the transmitted vector from
the worn wearables' params (a new `VisualParams::encode_appearance` +
`f32_to_u8` quantizer, the inverse of `map_appearance`; a param no wearable
sets falls back to its table default, so the vector is always the correct
neutral Ruth shape, never the `128` midpoint). It is used for **both** the
`AgentSetAppearance` publish (`drive_bake_publish`) and rendering the own
avatar (`apply_own_shape_from_wearables`, which overrides the server-echoed
appearance for our own agent and self-heals a re-outfit) — so the own avatar
is correct on any account/grid regardless of server state and other viewers
see the right shape. Verified live: a never-Firestorm'd account (Friend
Tester) that stayed bloated now renders the correct slender shape a few
seconds after login (once its wearables load). This was the *dominant*
base-body appearance bug; the animation-time skin distortion (**R11**, whose
skin-pivot premise turned out to be a proven sub-millimetre no-op) is a
separate, smaller issue to tackle next. Two viewer debug affordances were
added to make this comparison possible: `--screenshot-dir` (an offline PNG
capture harness that quits after N frames) and `--repeat-animation` (keep
re-issuing `--play-animation` so a short motion still plays once loaded).
