# sl-viewer-launch

Launching a Second Life viewer process for a harness — a cross-check run, an
end-to-end stage — and getting it to stop again without leaving the grid
holding its session.

- `Launch` is a viewer ready to be spawned: program, arguments, the
  environment entries added to the inherited one, and the log file its
  standard output and error go to.
- `ViewerDir` is one viewer's run directory; `confined_env` points all four
  `XDG_*` roots inside it, so a run never rewrites the operator's settings or
  reads last run's cache.
- `RunningViewer::spawn` starts one; several may be alive at once.
  `RunningViewer::stop` asks it to quit with `SIGTERM` (which the viewer turns
  into a logout), waits out the logout grace, and only then kills it;
  `stop_all` stops several in parallel. A `RunningViewer` dropped while its
  viewer still runs is stopped the same way, so a panicking test leaves no
  process and no session behind.
- `run` is the one-viewer path: spawn, wait for it to end on its own, and
  escalate the same way past a deadline or on `Ctrl-C` (`interrupt_flag`).
