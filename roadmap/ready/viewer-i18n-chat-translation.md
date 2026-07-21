---
id: viewer-i18n-chat-translation
title: Machine translation of chat / IM
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-i18n-localization
blocked_by: [viewer-i18n-fluent-scaffold, viewer-chat-history-panel]
---

Context: [context/viewer.md](../context/viewer.md).

Machine-translate incoming chat / IM: send text to a translation backend, show
the original + translation together, with per-conversation toggles.

## Backends surveyed (2026-07)

| Backend | Kind | Rust path | Data leaves machine? | Verdict |
| --- | --- | --- | --- | --- |
| **Bergamot / Firefox-Translations models** | on-device NMT | FFI to `bergamot-translator` (C++) | no | **default** |
| Opus-MT / NLLB-200 via CTranslate2 | on-device NMT | `ct2rs` (MIT) | no | local alt — widest coverage |
| LibreTranslate (Argos Translate) | self-hosted server | `libretranslate` crate / `reqwest` | user's own server | opt-in self-host |
| DeepL | cloud MT | `reqwest` direct | yes | opt-in — best EU quality (reference parity) |
| Google Cloud Translation | cloud MT | `reqwest` direct | yes | opt-in — max coverage (189+) |
| Azure Translator | cloud MT | `reqwest` direct | yes | opt-in — 2M chars/mo free (reference parity) |
| Claude / GPT (LLM) | cloud LLM | `reqwest` (see `claude-api` skill) | yes | opt-in — best on SL slang/emotes/honorifics |

## Recommendation — local-first, everything else behind a trait

Default to **on-device** translation; keep every other backend as an opt-in
through a `Translator` trait (below). This is net-new versus every SL viewer —
the reference is cloud-only (see the end) — and it keeps sensitive chat on the
machine. Two viable local engines, both behind the same trait:

- **Bergamot / Firefox-Translations (recommended local default).** Mozilla's
  forked Marian NMT with int8-quantised "tiny" per-language-pair models; roughly
  10 ms/sentence on CPU. No Rust crate exists yet — the Rust path is a
  **thin FFI to `bergamot-translator`** (the C++ project `kotki` demonstrates
  the integration end-to-end; there is also the option to vendor its
  `marian-lite` build). This is the same "one crate owns `unsafe` FFI" shape the
  workspace already accepts for `sl-j2c-encode` / `openjpeg-sys`. Per-pair
  models are ~15–40 MB.
- **Opus-MT / NLLB-200 via `ct2rs` (local, widest coverage).** `ct2rs` (MIT)
  binds OpenNMT's CTranslate2 and runs **Opus-MT** (Helsinki-NLP, 1000+ pairs,
  one small model per pair) or **NLLB-200-distilled-600M** (Meta, *200 languages
  in one model*, ~1–1.2 GB). Lower integration friction than hand-binding
  Bergamot (a maintained Rust crate), at larger model size. Use it for the
  long-tail languages Bergamot's curated pair set misses.

Split by trade-off: **Bergamot for the common pairs** (smallest / fastest),
**`ct2rs` + NLLB for the long tail** (one big download, near-universal
coverage). Spike both; ship whichever integrates cleanly first, the other slots
in behind the trait.

## Models are downloaded, not bundled

Fetch a model **on first use of a language pair**, cache it under the viewer
data dir (kotki's `~/.config/<app>/models/` posture), verify a checksum, and
show download progress; surface a "manage downloaded languages" list to delete
them. **Do not ship the language-pair matrix in the installer** — most users
won't translate, and Bergamot's per-pair models are ~15–40 MB *each* while NLLB
is ~1 GB, so bundling every pair would balloon the download for a minority
feature. Download per pair on demand; if anything is pre-installed at all it is
at most a single common pair, never the full matrix. Cloud / self-host backends
download nothing.

## Language detection

Chat carries no language tag, so the source language must be detected before a
local translation. SL chat is **short text** — the hard case (accuracy collapses
under ~20 characters). Options:

- **`lingua`** — most accurate for short / mixed text (the right fit for chat),
  at the cost of bundled per-language statistical models and more memory.
- **`whichlang`** — fast and light, fewer languages, ~6% less accurate.
- **`whatlang`** — middle ground, slower.

Use **`lingua`** for the local backends (short-text accuracy dominates here).
Cloud and LibreTranslate **auto-detect server-side** — skip the client detector
entirely when one of those is the active backend. Always **skip translation when
detected == target**, and offer a manual source-language override for the cases
short-text detection gets wrong.

**Constrain the candidate set per avatar (the big short-text lever).** Both
`lingua` and `whatlang` let you restrict detection to a language *subset*, which
sharply improves both accuracy and speed on short text — the docs' own example
is literally "only decide between English and German":

- `lingua`: `LanguageDetectorBuilder::from_languages(&[English, German])` (also
  `from_iso_codes_639_1/3`, `from_all_languages_without(...)`).
- `whatlang`: `Detector::with_allowlist(vec![Lang::Eng, Lang::Deu])`, and it
  detects **script first** (Latin / Cyrillic / CJK) which narrows the set for
  free.
- `whichlang` has **no** subset API — another reason to prefer `lingua` /
  `whatlang`.

So **track the languages seen from each avatar** (accumulate the confident
detections / translated-from languages per agent id) and build the detector for
that avatar from *its* history ∪ the viewer's own locale, collapsing "which of
dozens?" into "which of the two or three this avatar actually uses?". Cold start
(no history) falls back to script-narrowing plus a small common-set, or the full
set. Practical note: constructing a `lingua` detector loads per-language models,
so **cache detectors keyed by the sorted language set** — per-avatar sets
collapse to a handful of shared combos in practice, so this is cheap.

**Let the user pin an avatar's languages directly.** Add a small per-avatar
language control (an entry on the avatar context menu / in the chat surface, and
in the mute-list-style people UI) where the local user sets exactly which
languages a given individual uses — a manual pin that *seeds or overrides* the
auto-tracked history. When the user knows "this friend speaks French and
English", that becomes an authoritative two-language candidate set (or a fixed
source language, skipping detection entirely), which is the most reliable
outcome of all on short lines. Auto-history is the default; the pin is the
escape hatch and the power-user path.

**Persist the two tiers in different stores, matching their durability.**
Auto-detected per-avatar history is derived, rebuildable state → the **cache**
(may be evicted / expired, and re-learned from chat; never authoritative). A
user pin is deliberate, authoritative intent → the **settings store** (durable,
never silently dropped, and it *wins* over auto-history when both exist). Key
both by agent id.

## The provider trait (keeps the roster from rotting)

Mirror [[viewer-photo-hosting-upload]]'s `ShareTarget` pattern — one small impl
per backend so adding / dropping one is a leaf change:

```text
trait Translator {
    fn id(&self) -> &str;             // "bergamot", "deepl", "claude", …
    fn kind(&self) -> BackendKind;    // LocalNmt|SelfHost|CloudMt|CloudLlm
    fn needs(&self) -> Needs;         // ApiKey|ServerUrl|ModelDownload|None
    async fn ensure_ready(&self, store: &dyn SecretStore) -> Result<()>;
    fn supported(&self) -> LanguageSet;
    async fn translate(&self, text: &str, from: Option<Lang>, to: Lang)
        -> Result<Translated>;
}
```

`ensure_ready` fetches the model / validates the key. Target language comes from
[[viewer-i18n-locale-selection]]. `supported()` lets the UI grey out unavailable
target languages per backend.

## Secrets, cost and plumbing

- **Secrets.** Cloud MT and LLM API keys are secrets — store via **`keyring`**
  (OS keychain), reusing the exact pattern (and the headless / locked-keyring
  encrypted-file fallback) that [[viewer-photo-hosting-upload]] establishes. A
  self-host URL is not a secret. Local backends need no credential at all.
- **Render.** Original + translation shown together, into the chat surface
  [[viewer-chat-history-panel]] builds, with per-conversation toggles.
- **Cache by `(text-hash, from, to)`** so repeated phrases and emotes never
  re-translate — this matters for cloud cost and LLM latency.
- **Policy.** Never translate your own outgoing messages; skip system / object
  messages; translate a `/me` emote's *body* but keep the `/me`; pass URLs,
  SLURLs and object names through untranslated. Translate **on demand only**
  (the per-conversation opt-in), never the whole backlog automatically — that is
  what keeps cloud spend and LLM latency bounded.

## The LLM option (the "LLM models" angle)

LLMs (Claude / GPT) beat NMT on exactly the text SL chat is full of — slang,
emotes, honorifics, and conversation-level context — but at higher latency
(~1–3 s vs. sub-200 ms for dedicated MT) and cost (~$30–45 per million
characters vs. $10–20 cloud MT, free locally). Rough cloud MT free tiers for
reference: Azure 2M
chars/mo, DeepL 500K, Google 500K; paid ≈ Azure $10 / Amazon $15 / Google
$20 / DeepL $25 (+ base) per million. The LLM is a first-class
**opt-in "high quality" backend** under the same trait (`reqwest` to the
provider; model ids / pricing via the `claude-api` skill) — not the default,
because of latency, per-char cost, and that it sends chat to a third party.

## Costs / risks to accept up front

- **No Bergamot Rust crate** — either FFI-bind `bergamot-translator` (C++
  toolchain, `marian-lite`) or start with `ct2rs` (also C++ CTranslate2 under
  the hood, but a maintained Rust crate). `ct2rs` is the lower-friction start;
  Bergamot the smaller / faster models. Spike both before committing.
- **Model licensing varies** (Firefox models, Opus-MT, NLLB each differ) —
  verify each is redistributable before wiring up its download.
- **Short-text detection is unreliable** — the manual source override is not
  optional polish, it is the escape hatch for the common failure.

## Reference and deps

Reference (Firestorm, read-only): `lltranslate`, `llfloatertranslationsettings`
— **cloud-only** (Azure / Google / DeepL, each needing a user-supplied API key),
so the local path here is net-new; no SL viewer offers on-device translation.

Builds on / deps: [[viewer-i18n-fluent-scaffold]] (i18n foundation),
[[viewer-chat-history-panel]] (the surface the dual text renders into),
[[viewer-i18n-locale-selection]] (target language),
[[viewer-photo-hosting-upload]] (the `keyring` secret-store pattern reused for
API keys).
