---
id: viewer-audio-tests-open-real-devices
title: Unit tests open real audio devices on the developer's machine
topic: viewer
status: done
origin: noticed in pavucontrol during a pre-commit run (2026-08-19) — five
  `sl_client_bevy_viewer` playback streams appeared while the hook's nextest
  run was in flight
refs: [viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

Running the test suite reaches for **the machine's audio hardware**. Under
`cargo nextest` — which runs each test in its own process — several do it at
once: five `PipeWire ALSA [sl_client_bevy_viewer-<hash>]` clients were live
simultaneously during one pre-commit hook run, visible in `pavucontrol` and
indistinguishable at a glance from a viewer that failed to shut down (which is
exactly how this was noticed).

Why it matters, in rough order:

- **A test suite must not touch the machine it runs on.** Audio hardware is
  shared, mutable, global state; a test that grabs it can fail because
  something else holds the device, and behaves differently on a machine with no
  audio server at all (a container, CI, a headless build box).
- It makes a real leak **unrecognisable**: the whole point of noticing stray
  audio clients is to catch a viewer that did not clean up, and that signal is
  buried under test noise.
- Under nextest's process-per-test model the count scales with parallelism, so
  it gets worse as the suite grows.

## What it actually was (measured 2026-09-14)

The original report named two causes. **Both were wrong**, and the real one was
in neither: it is not the mixer or the plugin, it is **device enumeration**.

Measured by `strace -f -e trace=openat` on each release test binary, counting
opens under `/dev/snd`:

| test binary | `/dev/snd` opens | why |
| --- | --- | --- |
| `sl_audio` | 1503 | one test, `a_named_device_that_is_not_there_is_an_error` |
| `sl_viewer_preferences` | 1503 | `refresh_output_device_options`, floater open |
| `sl_viewer_audio` | 0 | — |

- **`Mixer::new` does not open a device.** `FirewheelContext::new` builds ring
  buffers and a graph and nothing else; `sl_viewer_audio`'s tests, which call
  `Mixer::new` several times, open no device at all. The report's first bullet
  was a guess at a mechanism, not an observation of one.
- **`AudioPlugin::build` never runs in a test.** It does start the default
  device — but the only `add_plugins(AudioPlugin)` in the workspace is in
  `run_session`, the real viewer's app. No harness builds an app containing it.
- What *does* happen in a test is a **host enumeration**: cpal's
  `default_host_enumerator().output_devices()` opens every ALSA control device
  on the machine to ask what it supports. 1503 opens of `/dev/snd/control*` per
  offending binary, and a `PipeWire ALSA [<binary>]` client for as long as it
  takes — which is what pavucontrol was showing. Two callers reached it:
  `Mixer::start`'s name→id lookup (through the sl-audio test above) and
  `Mixer::output_devices()` in the preferences audio tab (both at tab-build
  time and in the open-floater poll, which one test drives).

## The fix

Enumeration is hardware access, so it is **carried, not called statically** —
the same shape `sl-viewer-spacenav`'s `DeviceRead` gives the 6-DOF puck, for
the same reason.

- `sl-viewer-audio` gains an `OutputDeviceEnumerator` resource (a
  `fn() -> Vec<String>`) that `AudioPlugin` inserts. An app that did not ask
  for audio has no resource, and every reader does nothing rather than falling
  back to the host.
- `preferences_audio` reads that resource instead of calling
  `Mixer::output_devices()`. The tab build hook — which has no resource access
  — spawns the combo holding only the system-default entry; the open-floater
  poll, which already ran on the frame the floater opens, fills in the rest.
  Its test injects a synthetic device list, and a new test pins that **no**
  enumerator means no enumeration even with the floater open.
- `sl-audio` splits the selection rule out of `Mixer::start` as
  `select_output_device`, which takes the enumeration as a lazily-called
  closure. The "a missing name is refused, never silently the default" test now
  runs against a device list it wrote itself, and a second test pins that
  `DeviceSelection::Default` does not enumerate at all (its closure is
  `unreachable!`).

`Mixer::start` still opens a device when called — that is its job — and only
`AudioPlugin` calls it.

## Verified

Every test binary in the workspace that links `sl-audio`, re-measured the same
way, now opens **zero** files under `/dev/snd`: `sl_audio`, `sl_viewer_audio`,
`sl_viewer_preferences`, `sl_viewer_platform`, `sl_viewer_ui_core`,
`sl_viewer_media`, and the viewer's own 316-test binary
(`sl_client_bevy_viewer`, all 316 passing under `strace`). Full workspace run:
5809 passed, 0 failed.

## How to measure this again

`pactl` is the wrong instrument, twice over. A poll misses a client that lives
a millisecond, and — the trap that cost a measurement here — **each `pactl`
call is itself a new client**, so a 50 ms poll around a several-minute run
buries the thing being measured under nine thousand of its own samples.

Ask the kernel instead. Per test binary, with no sound server in the loop:

```sh
strace -f -qq -e trace=openat -o out.strace ./target/release/deps/<binary>
grep -c /dev/snd out.strace
```

An ALSA host enumeration is unmistakable — ~1500 opens of `/dev/snd/control*`
and no `pcm*` at all, since it asks every card what it supports and opens no
stream. A started playback stream shows as `/dev/snd/pcmC*D*p`.
