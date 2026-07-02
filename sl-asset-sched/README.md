# sl-asset-sched

The generic, level-of-detail-agnostic scheduling and fetch primitives shared by
the Second Life / OpenSim asset stores (`sl-texture`, `sl-mesh`).

Everything in this crate is deliberately *domain-free* — it knows nothing about
textures, meshes, discard levels, or mesh LODs. Each higher-level store keeps
its own concrete entry, decoded representation, disk cache, and LOD flow, and
builds them on top of these shared pieces:

- **`Priority`** — an opaque `u32` urgency, plus `popularity_boost(count)` (the
  `floor(log2(count)) * SCALE` diminishing boost for popular assets) and the
  `combine` (max) helper the stores use to build an effective priority.
- **`PriorityGate`** — a bounded, priority-ordered admission gate
  (`keyed_priority_queue`-backed) with cancel-on-drop waiters, so the
  highest-priority queued request runs first and a withdrawn one is removed
  before it ever runs.
- **`AssetFetcher<K>`** — the runtime-agnostic network abstraction
  (`fetch_range(id, start, end) -> FetchChunk`), generic over the asset key `K`,
  that each frontend (async `reqwest`, blocking `reqwest`) implements.
- **`run_cpu`** — the rayon bridge that runs a CPU-bound task (decode,
  downsample) off the async caller's thread, bounded by a semaphore.

The `Requesters` set and per-store progress enums are **not** here: they carry
the store's LOD type, so each store defines its own (calling the shared
`popularity_boost`).
