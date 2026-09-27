---
id: viewer-media-zero-copy-frames
title: Zero-copy media frames (DMA-BUF / shared-texture import)
topic: viewer
status: deferred
origin: split from viewer-media-prim-browser (2026-09-27)
refs: [viewer-media-prim-browser, viewer-video-playback]
---

Context: [context/viewer.md](../context/viewer.md).

Every media surface — a CEF page and a GStreamer video alike — reaches the GPU
through the **CPU path**: the engine's BGRA frame is copied into a Bevy `Image`
and uploaded with `write_texture`. [[viewer-media-prim-browser]] designed that
as the promise and zero-copy as **headroom**, never a requirement, and it was
split out here when that task closed so the headroom is not lost.

What the headroom is, per platform, behind the one `Frame` boundary both
engines share:

- **Linux, CEF**: the OSR `on_accelerated_paint` DMA-BUF (fds, modifier,
  format) imported as a `wgpu::Texture`. The gaps are Rust-side: cef-rs's import
  omits `VkExternalMemoryImageCreateInfo` and handles one plane only, and it
  needs `VK_EXT_image_drm_format_modifier`, which wgpu does not enable on the
  device Bevy creates (escape hatch: `RenderCreation::Manual`). CEF also wants
  `--use-angle=gl-egl`. Never enable the `cef` crate's `accelerated_osr`
  feature before Bevy is on wgpu 30.
- **Linux, GStreamer**: VA-API → DMA-BUF → the same import (`lumina-video` is
  prior art to read, not to depend on).
- **Windows / macOS**: the D3D11 shared handle and IOSurface equivalents.

Cautions already known: the accelerated path reports **no damage rects**
(whole-surface re-imports), and Linux OSR shared textures are reported broken
on NVIDIA even with the right ANGLE flags. Spike before believing any of it,
and only after a measurement says the copy is what a scene is paying for —
eight 512² surfaces at 30 fps is ~240 MiB/s, about 1 % of PCIe.
