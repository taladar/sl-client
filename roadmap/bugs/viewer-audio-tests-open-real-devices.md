---
id: viewer-audio-tests-open-real-devices
title: Unit tests open real audio devices on the developer's machine
topic: viewer
status: bugs
origin: noticed in pavucontrol during a pre-commit run (2026-08-19) — five
  `sl_client_bevy_viewer` playback streams appeared while the hook's nextest
  run was in flight
refs: [viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

Running the test suite opens **real playback streams on the machine's audio
server**. Under `cargo nextest` — which runs each test in its own process —
several appear at once: five `PipeWire ALSA [sl_client_bevy_viewer-<hash>]`
sink-inputs were live simultaneously during one pre-commit hook run, visible in
`pavucontrol` and indistinguishable at a glance from a viewer that failed to
shut down (which is exactly how this was noticed).

Two causes, the second much the bigger:

- `Mixer::new` alone is enough — it builds a `FirewheelContext`
  (`sl-audio/src/mixer.rs`), which opens an ALSA client without any `start()`.
  Several tests call it directly and their own comments say they are running
  *"without a device"* (`world_sounds::stale_oneshot_is_dropped`,
  `volume_panel::focus_mute_never_writes_the_store`), so the intent is already
  right and only the implementation disagrees.
- **`AudioPlugin::build` opens *and starts* the default output device**
  (`audio.rs`: `Mixer::new` then `mixer.start(&DeviceSelection::Default)`), so
  *any* test that builds an app including it holds a started playback stream
  for that test's whole lifetime — and the app-building tests (render, gallery,
  UI) are among the slowest. That is why the streams look persistent rather
  than blinking: under nextest's process-per-test model several are alive at
  once, and the set churns but is rarely empty for the length of a run.

Why it matters, in rough order:

- **A test suite must not touch the machine it runs on.** Audio hardware is
  shared, mutable, global state; a test that grabs it can make noise, can fail
  because something else holds the device, and behaves differently on a machine
  with no audio server at all (a container, CI, a headless build box).
- It makes a real leak **unrecognisable**: the whole point of noticing stray
  sinks is to catch a viewer that did not clean up, and that signal is buried
  under test noise.
- Under nextest's process-per-test model the count scales with parallelism, so
  it gets worse as the suite grows.

Fix directions, cheapest first:

- Have `AudioPlugin` skip the device when the app has no window / is headless,
  or take the device selection from a config the tests can set to "none". The
  plugin already survives a device that cannot be opened (every audio system
  guards on the mixer being present), so the not-started path is known-good.
- Give the mixer a **headless / null backend** for tests — firewheel can be
  configured with no output device, so the graph can be built and exercised
  without an ALSA client at all. The tests here are about the bus graph, the
  listener maths and the not-started guards; none of them needs a device.
- Failing that, gate the device-opening tests behind `#[ignore]` and run them
  only in the deliberate live-audio pass.

Check afterwards that a full `cargo nextest run` leaves `pactl list
sink-inputs` unchanged.
