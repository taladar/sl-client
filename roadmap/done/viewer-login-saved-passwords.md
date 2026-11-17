---
id: viewer-login-saved-passwords
title: Login screen — remember the password
topic: viewer
status: done
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

## Done (2026-11-17)

Landed with [[viewer-secret-store]], as its first consumer. A **Remember
password** checkbox (`login-remember`) sits under the password field, bound to
the global `LoginRememberPassword` (default off). While it is ticked:

- the panel asks the store for the password of the grid and avatar it shows
  when they change (the name once its field is left, not at every key), and
  fills `PasswordField` through `PasswordField::fill`, unless its user has
  already typed a password;
- a login that arrives keeps the password it was sent with;
- unticking forgets the password of the account shown, and so does a refusal
  for a bad password (`reason` `key`), ticked or not.

Where the store cannot keep or read a password, the line beside the box
(`login-remember-note`) gives the keychain's reason.

The saved-usernames list per grid was not taken: `keyring` cannot list the
entries of a service, so it would need an index of its own.

`e2e_login_screen::a_remembered_password_logs_in_without_typing_it` ticks the
box, logs in, logs out, comes back to the screen, sees the field filled and
logs in with Log In alone. It runs in process only, because a process viewer's
in-memory store ends with its process.
