# Context — known issues observed on the SL Beta grid (aditi)

Minor, non-blocking observations from the first live `sl-repl-tokio`
login/hold/logout smoke test against the Second Life Beta grid (aditi) on
2026-06-25. The session itself succeeded end-to-end (login, ~2-minute hold with
three `request_region_info` liveness probes, clean `logout`, exit 0). None of
the items broke the run; they warrant further investigation while we work on
real-SL support.

The full trace from that run is saved (uncommitted) for analysis at:
`~/.claude-personal/projects/-home-taladar-devel-new-sl-client/analysis/aditi-smoke-run-2026-06-25.log`

These are SL-specific findings: OpenSim never exercised these paths, so they
only show up against a real Linden Lab simulator.

Tasks split out from this file carry the `aditi` topic.
