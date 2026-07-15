---
id: viewer-photo-hosting-upload
title: Share snapshots to external photo/hosting services
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-snapshot-tools]
---

Context: [context/viewer.md](../context/viewer.md).

Share an in-world snapshot straight to the sites SL photographers use, from the
snapshot floater ([[viewer-snapshot-tools]]). The image bytes are trivial
(`reqwest` multipart over the viewer's existing async HTTP); **auth is the
feature**, and it differs per service.

## Services (surveyed 2026-07)

| Service | Auth | Rust | Verdict |
| --- | --- | --- | --- |
| **Flickr** | OAuth 1.0a | REST + `oauth1-request` | **launch** — flagship SL destination |
| **Discord** | webhook URL, no OAuth | `reqwest` direct | **launch** — trivial, very popular in SL |
| **Gyazo** | OAuth 2.0 bearer / token | `reqwest` direct | **launch** — simplest of all |
| **Bluesky** | app-password / atproto OAuth | `atrium-api` | later — rising as people leave X |
| **Mastodon** | OAuth 2.0, per-instance | `megalodon` / `mastodon-async` | later |
| **imgur** | Client-ID / OAuth 2.0 | `reqwest` direct | later — churn risk |
| **Primfeed** | — | — | **skip** — no public API |
| **Twitter/X** | OAuth 2.0, **paid** | — | **skip** — no free tier |

The two "skips" are the notable findings. **Primfeed** — the newer SL-specific
network, and the one you might most expect to want — has **no documented API**;
there is only a community feature request its owner has deprioritised, so any
integration would be reverse-engineered and possibly against ToS. Watch the
feature request (an atproto/Bluesky bridge has been floated). **Twitter/X** went
pay-per-use in early 2026 (~$0.015/post, $0.20 with a URL) — a non-starter for a
free viewer, and the reason Firestorm's `lltwitterconnect` is dead weight.
**Discord** is the pleasant surprise: a webhook URL is the whole auth story (no
OAuth at all), it is a ~20-line multipart POST, and SL communities lean on it
heavily — high value for almost no work.

## The design that keeps this from rotting

The roster churns (Twitter died, Primfeed appeared, Bluesky is rising), so build
a **share-target trait** and one small impl per service:

```text
trait ShareTarget {
    fn id(&self) -> &str;            // "flickr", "discord", …
    fn auth_kind(&self) -> AuthKind; // OAuth1a|OAuth2Pkce|Bearer|Webhook|None
    async fn authenticate(&self, store: &dyn TokenStore)
        -> Result<Credential>;
    async fn upload(&self, cred: &Credential, img: &ImagePayload,
        meta: &ShareMeta) -> Result<Url>;
    fn capabilities(&self) -> Caps;  // tags? description? max res?
}
```

`capabilities()` matters because services differ sharply (Discord has no tags;
Flickr has visibility flags; Bluesky caps image count and wants alt-text).
Adding a service becomes one impl file plus a settings entry, and dropping one
is a leaf change.

## The cross-cutting stack (the external-crate question)

- **OAuth 2.0** — `oauth2` (v5, PKCE) for Gyazo / Mastodon / imgur / Bluesky.
- **OAuth 1.0a** — `oauth1-request` + `reqwest-oauth1`, for Flickr only.
  **Budget a spike:** Flickr's signing has a trap — the `photo` binary param
  must be *excluded* from the signature base string while every other param is
  included, and the crate's happy path assumes all params are signed, so the
  multipart-plus-unsigned-file case likely needs a hand-built Authorization
  header. This is the single trickiest integration.
- **The desktop redirect** — OAuth needs a callback URL. The RFC 8252 answer is
  a **loopback listener**: bind `127.0.0.1:0`, use it as the redirect URI, catch
  `?code=…`, shut it down (~30 lines over our existing HTTP stack — no new
  crate). We *could* host the consent page in the in-viewer CEF browser
  ([[viewer-media-prim-browser]]), but RFC 8252 discourages embedded webviews
  and big providers block them — so
  **default to the system browser + loopback**, offer CEF only as a fallback,
  and let paste-a-token (Gyazo) and webhook-URL (Discord) skip the browser
  entirely.
- **Token storage** — refresh tokens and webhook URLs are secrets; use the OS
  keychain via `keyring` (Secret Service / macOS Keychain / Windows Credential
  Manager), **not** a plaintext config. Caveat worth planning for: on a headless
  or locked-keyring Linux box (exactly our test machines) `keyring` fails, so
  ship an encrypted-file fallback.

All the recommended crates are dual MIT/Apache (verify before relying).

Reference (Firestorm, read-only): `llflickrconnect`, `lltwitterconnect` (dead,
as a caution), `llfloatersocial` / the social share panel.

Deps: [[viewer-snapshot-tools]] (the snapshot it shares, and the floater's share
panel).
