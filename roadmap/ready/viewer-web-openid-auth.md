---
id: viewer-web-openid-auth
title: Second Life website auto-login (OpenID cookie) in the in-viewer browser
topic: viewer
status: ready
origin: user request (2026-07-22), while shipping the CEF web-media engine
refs: [viewer-media-prim-browser, viewer-profile-web-tab-browser]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer logs the **grid account into the Second Life websites**
(my.secondlife.com web profiles, marketplace, search) automatically, so the
in-viewer browser opens them already authenticated. Our web floater and
profile Web tab currently browse anonymously.

## How the reference does it (verified in Firestorm source)

1. The **XML-RPC login response** carries two extra members: `openid_url`
   and `openid_token` (`llstartup.cpp:5170`, grid-side; OpenSim does not
   send them).
2. At login, `LLViewerMedia::openIDSetup` POSTs the raw token to
   `openid_url` (`Content-Type: application/x-www-form-urlencoded`) and
   keeps the reply's `Set-Cookie` header — the grid session cookie
   (`llviewermedia.cpp` `openIDSetupCoro`).
3. `setOpenIDCookie` then (a) injects that cookie into the media browser's
   cookie store for the OpenID host (`getOpenIDCookie` → CEF `setCookie` /
   `storeOpenIDCookie`), and (b) GETs the web-profile URL once through the
   viewer's own HTTP stack so the redirect chain mints the site session;
   embedded browsers thereafter open the sites logged in. The web-profile
   panel relies on this (Firestorm additionally re-injects before
   navigating, and has had **recent regressions** in this area when the
   websites changed their cookie/redirect behaviour — worth checking their
   tracker for the current state before porting).

## What we need

- **`sl-wire`**: extract `openid_url` / `openid_token` from the login
  response (optional fields; absent on OpenSim).
- **`sl-cef`**: a cookie-injection call on the *shared* (trusted-UI)
  request context — CEF's `CookieManager::SetCookie` for a given URL, name,
  value, domain, path, secure, http-only. Never for the isolated in-world
  contexts: a griefer's media prim must not see the session.
- **Viewer**: at login, run the token POST off-thread, parse the
  `Set-Cookie`, inject it, and prime the profile-URL redirect chain; the
  web floater / profile Web tab then get it for free. Gate on the fields
  being present so OpenSim logins are unaffected.
- Consider a `--no-web-auth` escape hatch, and make sure logout / avatar
  switch clears the shared cookie store (`sl-account-dirs` scoping: the
  shared context currently persists under one cache dir for all avatars —
  per-avatar separation likely wants a per-account `cache_path`).

Testable only against real Second Life (aditi): OpenSim sends no
`openid_url`. Reference (read-only): `llviewermedia.cpp`
(`openIDSetup[Coro]`, `openIDCookieResponse`, `setOpenIDCookie`,
`getOpenIDCookie`, `parseRawCookie`), `llstartup.cpp` (login response),
`llpanelprofile.cpp` (web tab consuming it).
