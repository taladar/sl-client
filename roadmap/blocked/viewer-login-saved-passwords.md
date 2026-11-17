---
id: viewer-login-saved-passwords
title: Login screen — remember the password
topic: viewer
status: blocked
origin: viewer-login-screen follow-up (2026-11-16)
refs: [viewer-login-screen]
blocked_by: [viewer-secret-store]
---

Context: [context/viewer.md](../context/viewer.md).

The login screen ([[viewer-login-screen]]) remembers the last grid and avatar
name but not the password. Add the reference's "Remember password" checkbox:
when it is ticked, a successful login keeps the password in the secret store
([[viewer-secret-store]]) keyed by grid and avatar; choosing that grid and
avatar again fills the password field (`PasswordField`, set without the text
ever entering the editor); unticking it, or a refusal for a bad password,
forgets it. The checkbox's own state lives in the global settings
(`LoginRememberPassword`). Where the store cannot keep a secret, the checkbox
says why rather than pretending to remember.

Firestorm also offers a list of saved usernames per grid in the name field;
take that with it if the store makes listing cheap.

Test it in `e2e_login_screen` with the stage's in-memory store: log in with
the box ticked, relog, and log in without typing the password.

Reference (Firestorm, read-only): `llpanellogin.cpp` (`remember_password`),
`llsecapi`.
