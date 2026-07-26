---
id: viewer-perf-gpu-jpeg2000-decode
title: GPU (wgpu compute) JPEG2000 texture decoding
topic: viewer
status: ideas
origin: user request (2026-07-26), planning-worktree research
refs: [viewer-profiling, viewer-perf-texture-decode-cache,
  viewer-texture-vram-budget, viewer-perf-gpu-particles]
---

Context: [context/viewer.md](../context/viewer.md).

Decode SL's classic JPEG2000 (Part-1 J2C) textures on the GPU via
vendor-agnostic wgpu/WGSL compute. Must work on all GPU vendors
(AMD/Intel/NVIDIA); CUDA-only solutions (nvJPEG2000, Fastvideo,
Comprimato) are excluded by requirement and serve only as existence
proofs and performance ceilings. Researched 2026-07-26; the analysis
below is the plan-of-record when this is picked up.

## Motivation: CPU offload, not (only) latency

Even at decode-latency parity, GPU decode frees CPU cores: decode
shares the global rayon pool with tessellation and bake compositing,
and decode load is burstiest during login/teleport scene loads —
exactly when the GPU tends to have render headroom because little of
the scene is loaded yet. Counterpoints to keep honest: in steady state
the GPU is the renderer's contended resource; on iGPUs/low-end GPUs
the GPU is the scarcer side, so offloading to it can hurt; today's
decode threads run on otherwise-idle cores on many-core desktops.
**Evaluation criterion: end-to-end frame pacing + scene-load wall
time, not per-image decode latency.**

## Two workload regimes

- **Burst (teleport/login storm)** — dozens to hundreds of textures
  queued at once. Batching across the queue per dispatch provides
  ample parallelism (100 textures × ~800 codeblocks ≈ 80k work items).
  Published batch numbers: Fastvideo reaches 17–31× CPU throughput at
  2K in batch mode; nvJPEG2000 decodes 1024×1024 tiles at ~0.9 ms/tile
  (~1100/s) inside a 121-tile batched workload. A batched GPU decoder
  plausibly beats the multithreaded CPU pipeline on throughput here
  *and* frees all CPU cores.
- **Trickle (steady-state exploring)** — single textures arrive
  continuously; batches are small, single-texture dispatches
  underutilize the GPU (see codeblock math below), and dispatches
  compete with rendering. Tuned CPU decode wins here; a GPU path needs
  a graceful fallback (e.g. keep the CPU path for small batches).

## Parallelizability breakdown (per stage)

- **Tier-2 packet/header parsing** (~few % of decode time): inherently
  sequential — variable-length packet headers must be read in stream
  order just to locate each codeblock's bytes. Stays on the CPU in
  every GPU decoder, including nvJPEG2000.
- **Tier-1 EBCOT block decoding** (**~50–70% of decode time**, the
  dominant cost): **bit-serial inside a codeblock**. The MQ arithmetic
  coder is context-adaptive — each decoded bit updates probability
  state the next bit depends on — and each bitplane takes three
  sequential coding passes; SL's ~5 quality layers append further
  sequential pass segments. The only standard-compatible parallelism
  is **across codeblocks** (64×64): one thread or small subgroup per
  codeblock, serial MQ inside. Finer-grained schemes exist (BPC-PaCo,
  ~52.7× vs Kakadu; fine-granular EBCOT) but redesign the bitstream —
  useless for existing SL assets. Best published standard-compatible
  GPU Tier-1: ~17× on large satellite images.
- **Dequantization** (small %): embarrassingly parallel per
  coefficient.
- **Inverse DWT** (bulk of the remaining ~30–50%): data-parallel
  within each subband (well studied on GPU, e.g. PDWT), mildly serial
  across the ~5 resolution levels (each consumes the previous level's
  output).
- **Inverse color transform + level shift + RGBA pack** (small %):
  embarrassingly parallel per pixel.

Net: a third to a half of the work is ideal GPU material; the dominant
half-to-two-thirds (Tier-1) parallelizes only at codeblock
granularity, so a viable design **must batch across the decode queue**
— which the storm workload naturally provides. Codeblock math at SL
sizes: a 1024×1024 RGB texture has ~780–1040 codeblocks (~25–32
warps); a 256×256 texture ~60–80 (~2 warps); even a 4K frame
underutilizes a GTX 1080 (Naman & Taubman, ICIP 2019). Per-codeblock
work is highly variable (bitplane count, layer truncation), so a real
implementation also needs load-balancing/divergence engineering
(codeblock sorting, persistent threads) — the part the CUDA decoders
spent years tuning. Fastvideo's tuned decoder manages only ~3×
single-image at 2K vs a multithreaded CPU.

## Feasibility & cost

Portability is *not* the blocker: wgpu/WGSL subgroup operations
(shipped in wgpu and in the WebGPU spec) cover everything the CUDA
implementations actually use — thread-per-codeblock,
subgroup-per-codeblock with shared-memory LUTs; no dynamic
parallelism. nvJPEG2000 is the existence proof for the CPU/GPU split
(Tier-2 on CPU, everything else on GPU).

What makes it expensive rather than infeasible:

- No portable implementation exists to port — all production GPU J2K
  decoders are CUDA; the sole OpenCL attempt (ThousandthChicken) never
  decoded an image; no Vulkan/Metal/WGSL decoder exists anywhere.
- ~6–12 person-months: Tier-2 parsing, MQ/EBCOT decoder (three coding
  passes, context modeling, layer truncation), IDWT and ICT in WGSL,
  plus the load-balancing tuning above, plus SL legacy-stream quirks
  (5-component server bakes, 16-bit narrowing — see
  `sl-texture/src/decode.rs`).
- Batching plumbing through the existing priority/discard model: the
  16-slot `PriorityGate` + num_cpus decode semaphore
  (`sl-texture/src/store.rs`) and truncated-prefix fetches would need
  a batched GPU queue that still respects priority order.
- The hybrid option (Tier-1 on CPU, dequant+IDWT+ICT on GPU) is
  Amdahl-capped at ~1.5–2× (Tier-1 is 50–70% of the cost) and uploads
  16/32-bit wavelet coefficients — more bytes than the RGBA it
  replaces. Wrong 30% of the problem; only interesting as a later
  add-on.
- Greenfield GPU plumbing: the workspace has no compute shaders or
  render-graph nodes yet (nearest neighbour:
  [[viewer-perf-gpu-particles]] option 2). Decoded output staying
  GPU-resident would bypass `to_bevy_image`
  (`sl-client-bevy/src/textures.rs`) and save the 4 MB-per-1024²
  RGBA upload (the compressed J2C is typically 50–300 KB).

## When it becomes most attractive

(a) Linden Lab adopts HTJ2K server-side — HTJ2K's block coder is
designed for parallel decode (~10× on CPU; the ICIP 2019 Kakadu/UNSW
GPU kernels map cleanly onto WGSL subgroups), but SL assets today are
classic Part-1 and no adoption signal exists; (b) profiling
([[viewer-profiling]]) shows decode-induced CPU contention hurting
frame pacing or scene-load time; (c) a portable open-source GPU
J2K/HTJ2K decoder appears that is worth porting. A local
transcode-to-HTJ2K cache captures much of the HTJ2K benefit without
LL's involvement — see [[viewer-perf-texture-decode-cache]], which
should land first either way.

First concrete steps when fleshing out to ready:

1. Capture a representative teleport-storm decode trace (texture
   count, sizes, discard levels, arrival timing) as the benchmark
   input, so the batch-vs-trickle mix is measured, not assumed.
2. A WGSL IDWT + dequantization + ICT prototype (the well-understood
   20–30% of the problem), evaluated against the frame-pacing /
   scene-load criterion, before committing to Tier-1-in-WGSL.

## Sources

- nvJPEG2000 docs: <https://docs.nvidia.com/cuda/nvjpeg2000/index.html>
  and the NVIDIA benchmark blog post "Accelerating JPEG 2000 decoding
  for digital pathology and satellite images using the nvJPEG2000
  library" (developer.nvidia.com/blog).
- Fastvideo J2K decoder benchmarks:
  <https://fastcompression.com/benchmarks/decoder-benchmarks-j2k.htm>
- Naman & Taubman, "Decoding HTJ2K on a GPU" (ICIP 2019):
  <https://kakadusoftware.com/wp-content/uploads/ICIP2019_GPU.pdf>
- BPC-PaCo, bitstream-incompatible fine-grained Tier-1 (IEEE TPDS
  2017): deic.uab.cat/~francesc/research/bpc_paco/
- HTJ2K white paper:
  <https://ds.jpeg.org/whitepapers/jpeg-htj2k-whitepaper.pdf>
- wgpu subgroup ops: <https://github.com/gfx-rs/wgpu/issues/5555>
- SL wiki on Kakadu vs OpenJPEG: <https://wiki.secondlife.com/wiki/Kakadu>
- The same bottleneck in another Rust SL client:
  <https://github.com/rust-gamedev/wg/issues/124>
- The OpenCL attempt that never worked ("ThousandthChicken"):
  encode.su thread "Open-source OpenCL jpeg 2000 library".
