---
id: viewer-login-screen
title: Login screen — grid select, saved credentials, MFA
topic: viewer
status: ready
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

A real login screen: pick a grid, enter avatar name + password, optionally save
the password, satisfy MFA, and connect — the interactive front door the viewer
still lacks. Today login is code-only: [[viewer-p1-1]] logs in from a
`credentials.toml` file via `--credentials <path>` / `--avatar <name>`,
`Credentials::load().select()`, grid resolution from `login_uri` / `grid`, and
`Avatar::acquire_mfa()` + `LoginRequest::with_mfa`. That path is perfect for
tests and headless runs and **must stay** (see below); it is not something a
first-time user can be asked to hand-write.

## Scope

- **The panel.** Grid selector, avatar name, password, a "remember password"
  toggle, a "start location" (last / home / a named region), and a connect
  button with live status (resolving grid → authenticating → MFA →
  region handshake) and legible failure messages (bad password, MFA required /
  rejected, grid unreachable). Built on [[viewer-ui-widget-scaffold]].
- **Saved credentials.** A password the user asks to keep is a secret — store it
  via **`keyring`** (OS keychain), reusing the exact pattern (and the headless /
  locked-keyring encrypted-file fallback) that [[viewer-photo-hosting-upload]]
  establishes and [[viewer-i18n-chat-translation]] reuses for API keys. Keep it
  keyed by (grid, avatar) so multiple avatars / grids each remember
  independently. Non-secret bits (last avatar, last grid, remember-toggle state)
  live in the settings store via [[viewer-ui-settings-binding]]. Never write a
  password to a plaintext config. Match the reference's caution: store what the
  login flow needs, nothing more.
- **Grid manager.** Add / edit / remove grids (`login_uri`, display name),
  defaulting to the local OpenSim `http://127.0.0.1:9000/`, with Second Life
  (agni) and the SL beta grid (aditi) as built-ins — this is the multi-grid
  reality the `credentials.toml` `grid` field already models.
- **MFA.** Drive the existing `Avatar::acquire_mfa()` from a TOTP / token prompt
  in the panel (real SL and aditi require it; local OpenSim does not), rather
  than the config-supplied token the test path uses.
- **TOS / critical-message interstitials** are their own follow-up task,
  [[viewer-login-tos]] — the flow here must leave the retry seam it hooks
  into (login response says `tos`/`critical` → pause, show, retry).

## Keep the credentials.toml bypass

The `--credentials` / `--avatar` CLI path **skips the login screen entirely**
and logs straight in — this is load-bearing for the whole test and
live-verification story (smoke tests, the aditi TOTP wrapper, headless CI,
`sl-repl`), so it is a first-class supported entry point, not legacy. Rule: when
credentials are supplied on the CLI, do not show the panel; otherwise show it.
Both routes converge on the same `LoginParams` / `SlClientPlugin` construction
[[viewer-p1-1]] already builds — the panel just populates those fields
interactively instead of reading them from a file.

## Note on the HTML splash

Second Life's reference login screen also renders an **HTML splash / MOTD** page
(`panel_login.xml`'s `web_browser` named `login_html`) behind the native
credential widgets. That surface depends on the embedded browser
([[viewer-media-prim-browser]]) and is **optional** — the native panel is the
real credential path; treat the HTML splash as later polish, not a prerequisite,
so login never blocks on the browser being alive.

Reference (Firestorm, read-only): `llpanellogin`, `llstartup` (the login state
machine), `llviewernetwork` (the grid manager), `llsecapi` / the protected
credential store (how the reference keeps a saved password), `panel_login.xml`.

Builds on: [[viewer-p1-1]] (the `credentials.toml` login path and
`LoginParams` construction, kept as the test bypass).

Deps: [[viewer-ui-widget-scaffold]] (the panel), [[viewer-ui-settings-binding]]
(non-secret preferences), [[viewer-photo-hosting-upload]] (the `keyring`
secret-store pattern reused for the saved password).
