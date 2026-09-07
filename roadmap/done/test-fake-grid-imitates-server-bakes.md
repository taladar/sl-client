---
id: test-fake-grid-imitates-server-bakes
title: The fake grid runs a bake service on a grid that says it is OpenSim
topic: test
status: done
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 3
refs: [test-fake-grid-object-asset-id-divergence]
---

Done 2026-09-07. Four advertisements, not one, and they now move together.
See "What landed" below.

Context: [context/testing.md](../context/testing.md).

Server-side avatar baking is a Second Life thing. The login response names an
`agent_appearance_service`, the viewer decides the avatar is server-baked, and
every baked slot is fetched from that service by URL rather than by asset id
(`LLVOAvatar::getImageURL`). A stock OpenSim region does not run one; the
viewer bakes locally and uploads.

The fake grid always named the service — `runtime.rs` set
`success.agent_appearance_service` unconditionally, and `http_service.rs`
mounted the route per session — so an OpenSim-flavoured grid was still Second
Life about appearance.

**The trap, which is why this is not a one-line derivation.** Dropping the
service alone is worse than leaving it: the comment already in `runtime.rs`
records what happens, and it was paid for once. A viewer that has decided an
avatar is server-baked and finds no service URL leaves every avatar — the
agent's own included — permanently a cloud, *silently*, because `getImageURL`
returns an empty string rather than a request that fails. So the decision the
viewer makes has to flip with the service: what tells it "this avatar is
server-baked" (the bake texture ids the grid hands out, and what
`SimulatorFeatures` says) has to say the other thing too.

## What landed

`bakes::BakePolicy` — `ServerSide` / `ClientSide` — is the sixth knob
`ImitatedGrid` decides, and it is a type rather than a boolean because
**four** advertisements have to move together:

| what | `ServerSide` (SL) | `ClientSide` (OpenSim) |
| --- | --- | --- |
| login `agent_appearance_service` | the per-session route | absent; route 404s |
| `RegionProtocols` bit 0 | set | clear |
| the appearance's `AppearanceData` block | present, v1 | no block |
| the `UpdateAvatarAppearance` capability | granted | withheld |

The two middle rows are what a "just drop the URL" change would have missed,
and neither is guessed. `RegionProtocols` bit 0 is what
`LLViewerRegion::unpackRegionHandshake` turns into `getCentralBakeVersion()`,
which gates the **agent's own** half — a viewer in a region claiming to
central-bake never sends `AgentSetAppearance` — and OpenSim's
`LLClientView.SendRegionHandshake` writes `1 << 63`, bit 0 clear. The
`AppearanceData` block is `setIsUsingServerBakes(appearance_version > 0)` in
`LLVOAvatar::processAvatarAppearance`, and OpenSim's `SendAppearance` writes a
literal zero block count with the comment `// no AppearanceData` — so the
client-baked side is the block *absent*, not zeroed, which is what
`BakePolicy::apply` produces.

Bit 63 is *not* the bake policy's. It is the unrelated "more than 6 baked
textures" (Bakes on Mesh) extension riding in the same field, so
`ImitatedGrid::region_protocol_bits` contributes it and the two halves are
OR'd — an explicit `bakes(...)` override moves only its own bit. Second Life
claims nothing there rather than claiming bit 63 too: the reference viewer
reads that bit as an OpenSim extension (`// OS sets bit 63 when BOM
supported`) and decides the question from the grid's identity on Second Life,
so whether the Second Life simulator sets it is unmeasured and is not claimed.

`SimulatorFeatures` turned out **not** to be an input after all — it carries
`BakesOnMeshEnabled`, which is about what a baked slot may reference, not
about who composites it. The task text guessed it was; it is not, and the
module doc says so rather than leaving the guess standing.

New in `sl-proto`: `SimCaps::withhold(name)`, which removes a capability from
both halves of the contract (the seed grant and the URL). `SERVED_CAPABILITIES`
is what a simulator *can* serve; capability negotiation is how a grid says
which of two protocols it speaks, and this is the first grid-wide policy that
needed to say it.

Nothing changed in this workspace's own viewer, and that is a result rather
than an omission: `ingest_avatar_bakes` already keys on the service URL being
present and falls back to a by-UUID fetch when it is not, which is the road a
client-baked grid leaves open. What had never been exercised was that road —
against the fake grid every bake went through the appearance service, on both
flavours, because the service was always there.

Pinned by:

- `bakes.rs` unit tests, including one that the service and the trigger
  capability are never advertised apart — the coupling whose breakage is
  silent.
- `imitates.rs`: the "every derived knob separates the two grids" test grew
  two rows, plus a test that the OpenSim flavour bakes nothing of its own and
  one that the two `RegionProtocols` halves compose independently.
- `sim_caps.rs`: a withheld capability is neither granted nor resolvable.
- `http_glue.rs`: only a baking grid names an appearance service — on the raw
  login response, because the field's *absence* is not something the client
  re-exposes as anything but a `None` it also produces on its own.
- `client_end_to_end.rs`: a client-baking grid withholds every bake signal —
  the protocol bit, the appearance block and the capability, both flavours.
- `avatar-appearance-npc` (conformance) now declares **both** fake flavours
  and asserts the block follows the flavour; its existing `GetTexture` leg is
  what proves the ids are fetchable by id, which is the only road left on the
  client-baked one. `server-appearance-bake` declares both too and *asserts*
  the capability's absence on `FakeOpensim` rather than recording `partial` —
  offline the answer is a setting this workspace made, not an unknown.
- `full_stack_test.rs`:
  `an_npc_wears_its_bakes_on_a_grid_that_does_not_bake` — the same NPC as the
  Second-Life-flavoured bake test, rendered against a grid that names no
  service, asserted by pixels. It is a picture test because the failure it
  guards has no event and no warning to wait for.

Acceptance: `Grid::FakeOpensim` advertises no appearance service and its
avatars still render, `Grid::FakeSl` keeps the service, and the pair is pinned
by something that would have caught the silent-cloud failure.
