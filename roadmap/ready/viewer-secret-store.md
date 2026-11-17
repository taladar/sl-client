---
id: viewer-secret-store
title: Secret store for the viewer (OS keychain, keyed secrets)
topic: viewer
status: ready
origin: viewer-login-screen follow-up (2026-11-16)
refs: [viewer-login-screen, viewer-i18n-chat-translation]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer has nowhere to keep a secret. A saved login password
([[viewer-login-saved-passwords]]), a photo-hosting refresh token or webhook
URL ([[viewer-photo-hosting-upload]]) and a translation API key
([[viewer-i18n-chat-translation]]) all need one, and none may land in a
plaintext config. Build it once, as a small crate of its own, before any of
them.

## Scope

- **The OS keychain** through the `keyring` crate (4.x at the time of
  writing: Secret Service on Linux, Keychain on macOS, Credential Manager on
  Windows). Measure what it adds to the build (it pulls in a D-Bus stack on
  Linux) with `cargo tree` before and after, and keep it out of crates that do
  not store secrets.
- **Keyed secrets**: a service name per kind (login password, hosting token,
  API key) and an account key within it — for a login password the grid and
  the avatar, as `sl-account-dirs` names their directories, so each avatar on
  each grid remembers independently.
- **Off the frame thread**: a keychain call can block on a D-Bus round trip
  or an unlock prompt, so reads and writes run on a worker and answer through
  a message, as `login_flow`'s second-factor source does.
- **No keychain** (a headless box, a locked or absent Secret Service — the
  test machines): decide between refusing to remember, with the reason shown
  where the user asked, and an encrypted-file fallback. A file whose key lives
  on the same machine is obfuscation, not protection; if a fallback is kept,
  say so plainly where it is offered and never pretend otherwise. Never fall
  back to plaintext.
- **Testable without a keychain**: an in-memory store the e2e stage can hand
  a viewer, so the features above are tested on machines with no Secret
  Service. It is the stage's, not a Cargo feature (a feature doubles the
  powerset).

Reference (Firestorm, read-only): `llsecapi`, `llsechandler_basic` (the
protected credential store, and how it keys a saved password).
