---
id: viewer-secret-store
title: Secret store for the viewer (OS keychain, keyed secrets)
topic: viewer
status: done
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

## Done (2026-11-17)

**`sl-secret-store`** is a new crate with no Bevy in it. It holds:

- `SecretStore`, a trait with `load`, `save` and `forget`, each of which may
  block;
- `SecretKey`, a `SecretKind` (the keychain service: `sl-client login
  password`) plus an account. `SecretKey::login_password(grid, avatar)` takes
  the grid and avatar as `sl-account-dirs` names their directories:
  `Avatar Tester @ 127.0.0.1:9000`;
- `Secret`, whose `Debug` is redacted;
- `SecretError::{Unavailable, Failed}`;
- two stores: `KeychainStore` (`keyring` 4.2 with its default `v1` feature:
  Secret Service through zbus, Keychain, Credential Manager) and
  `MemoryStore`.

The keychain's errors are sorted so that the user sees "cannot keep it" for
`NoDefaultStore`, with the store's own start-up error, and for
`NoStorageAccess` (a locked or refusing keychain). Anything else is a
failure.

**No keychain: refuse, never a file.** There is no encrypted-file fallback.
Where no keychain answers, the feature that asked says why, and the login
screen writes the reason beside its checkbox.

**Off the frame thread.** The viewer's `crate::secrets` holds the store as
`ViewerSecrets(Arc<dyn SecretStore>)`, from `ViewerAppOptions::secrets`
(default `KeychainStore`). Systems write `SecretRequest { key, op:
Load | Save | Forget }`. One worker thread, started with the first request,
carries the requests out in order and answers with `SecretAnswer`.

**Testable without a keychain.** The CLI has `--secret-store
keychain|memory` (no file option). `sl-e2e` gives every stage viewer an
`Arc<MemoryStore>` that lasts across its sessions in process, and passes
`--secret-store memory` to every process viewer. No stage touches the
developer's keychain, and nothing here is a Cargo feature.

**Build cost.** The viewer's normal dependency closure grew from 772 to 793
packages. zbus was already in it; the new ones are `keyring`, `keyring-core`,
`zbus-secret-service-keyring-store`, `secret-service` and the RustCrypto
stack its session encryption uses (aes, cbc, hkdf, hmac, sha2, …). Only
`sl-secret-store`, the viewer and `sl-e2e` depend on it.

The first consumer is [[viewer-login-saved-passwords]].
[[viewer-photo-hosting-upload]]'s token and
[[viewer-i18n-chat-translation]]'s API key get their own `SecretKind`
variants when they land.
