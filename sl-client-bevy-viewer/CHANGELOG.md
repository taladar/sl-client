# Changelog

## 0.1.0

Initial Release

### Added

- Phase 1 viewer shell: a windowed Bevy app (`DefaultPlugins`) that logs in via
  the shared `credentials.toml` mechanism (`sl-repl::auth`), spawns a `Camera3d`
  and a directional light, and drives the `SlClientPlugin` session. `clap` args
  `--credentials` / `--avatar` / `--grid` / `--login-uri` / `--start` /
  `--channel` / `--version`, with MFA acquired via the avatar's `mfa_command`.
- Debug fly-camera: WASD translation (Shift boosts, Space / Ctrl for vertical)
  and mouse-look on a captured cursor; the camera is snapped to the agent's
  login position when its avatar object first arrives.
- Clean quit: `Esc` / `Q` requests a logout and exits on `LoggedOut` /
  `Disconnected` (or a short grace fallback). On `RegionHandshakeComplete` the
  viewer sets the draw distance so the sim streams content.
- The single Second Life (Z-up) ↔ Bevy (Y-up) coordinate conversion
  (`sl_to_bevy_vec`), applied only at the camera / entity boundary.
