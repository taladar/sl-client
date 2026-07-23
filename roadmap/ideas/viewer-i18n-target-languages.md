---
id: viewer-i18n-target-languages
title: Which UI translations to ship — target-language tiers
topic: viewer
status: ideas
origin: translation-targets research (2026-07-23)
refs: [viewer-i18n-fluent-scaffold, viewer-i18n-locale-selection]
---

Context: [context/viewer.md](../context/viewer.md).

The i18n *machinery* exists ([[viewer-i18n-fluent-scaffold]] and
[[viewer-i18n-number-datetime-formats]] are done; the current en/ja/ar/pl
bundles were chosen as typographic coverage locales, not by user demand).
What no task decides yet is **which languages we should actually ship
translations for**. Two forces drive that choice: which languages SL's
user base actually speaks, and which translations existing SL users are
*used to having* from the official viewer and Firestorm — dropping a
language someone's viewer already speaks is a regression for them.

## What the reference viewers ship

Linden's official viewer ships 12 languages: en, da, de, es, fr, it, ja,
pl, pt, ru, tr, zh(-Hans). Firestorm ships those plus Azerbaijani (13);
other TPVs inherit the LL set. That set is the baseline expectation of
migrating users.

Completeness is very uneven (Firestorm tree, vs en's 797 XUI files and
2470 strings.xml entries; missing files/keys fall back to English):

| lang | XUI files | strings.xml | assessment                   |
| ---- | --------- | ----------- | ---------------------------- |
| ja   | 650       | 2337        | most complete                |
| zh   | 641       | 2451        | near-complete                |
| fr   | 635       | 2386        | near-complete                |
| de   | 614       | 2423        | near-complete                |
| pl   | 618       | 2225        | substantial                  |
| ru   | 595       | 2309        | substantial                  |
| it   | 584       | 2229        | substantial                  |
| az   | 563       | 2252        | substantial (one contributor)|
| es   | 489       | 2133        | partial                      |
| pt   | 411       | 1892        | partial — despite Brazil     |
| tr   | 395       | 1969        | partial                      |
| da   | 263       | 1498        | minimal/legacy               |

Notably the two *least* complete translations (pt, es) belong to two of
the *largest* communities — a new viewer with fresh, complete pt-BR and
es beats the incumbents where it matters most. Translations there are
hand-maintained XML mirrors (Firestorm accepts no new languages; its
Transifex effort is currently a pt-BR push), descended from LL's old
crowdsourced Community Translation Project
(wiki.secondlife.com/wiki/Community_Translation_Project). Neither viewer
supports RTL at all — no ar/he ships anywhere.

## Who actually plays SL

- Brazil is the clear #1 non-US market (SimilarWeb secondlife.com
  country shares; New World Notes' Jan 2019 SimilarWeb breakdown put
  Brazil first). Historic top blocs: Brazil, Japan, Germany, UK, France.
- Firestorm staffs in-world language support groups for de, nl, hu, pl,
  pt, ru, es (firestormviewer.org/support) — a good proxy for which
  communities are large enough to sustain volunteers. **Dutch has a
  community but no shipped translation in any viewer** — a genuine gap.
- Global internet-language rankings (en, zh, es, ar, pt, ru, de, fr, ja,
  ko, …) sanity-check the list but SL's skew is Western/Brazilian/
  Japanese/Russian, not raw internet population.
- Large markets with **no** SL evidence (checked deliberately): India,
  Indonesia, Vietnam do not appear in secondlife.com top-traffic
  countries and have no organized SL communities or translations
  anywhere. India's "localize into Hindi" guidance targets the
  smartphone-first mass market; SL needs a gaming-grade PC, so its
  Indian users self-select toward the English-proficient segment. Hindi
  additionally needs Devanagari shaping + bundled Indic fonts (parley/
  harfrust likely shapes it, but fonts and testing are real cost).

## Proposed tiers

- **Tier 1** — largest communities, immediate payoff: **pt-BR, de, fr,
  es, ja** (ja: complete the existing coverage bundle). pt-BR and es are
  where incumbent translations are weakest.
- **Tier 2** — established communities: **ru, it, pl** (complete the
  existing pl bundle), **nl** (fill the gap no other viewer serves),
  **zh-Hans**.
- **Tier 3** — parity/opportunistic: **tr, da, az** (match the reference
  set so no migrating user loses their language), **ko** (thin in SL;
  with a volunteer), **ar** (small community, but our bidi/RTL already
  works — the only SL viewer that can render it; already a coverage
  locale), **he** (same RTL argument).
- **Watch list** — accept community contributions, don't fund: **id,
  vi** (large markets, cheap Latin scripts, zero SL evidence); **hi/bn**
  explicitly not targeted (no SL community, Indic font/shaping cost);
  revisit only if a volunteer team appears.

## Mechanics

- One `assets/locales/<lang>/main.ftl` per locale (split into more .ftl
  files per bundle as it grows); Fluent supplies per-locale plural
  rules; CJK + colour-emoji fonts are already bundled.
- Completeness tracking: a small check comparing each locale's keys
  against `en/main.ftl` (the en→key fallback means gaps degrade
  gracefully, like the reference viewer's per-key English fallback).
- The shipped-locale list feeds the language dropdown of
  [[viewer-i18n-locale-selection]] (surfaced via
  [[viewer-preferences-general-tab]]) and the grid-facing
  [[viewer-i18n-agent-language]].
- Skin CSS strings resolve through [[viewer-ui-skin-l10n-functions]].

## Licensing caution

Reference-viewer translation strings are LGPL viewer source — do **not**
copy them into our .ftl bundles. Use them and the old CT Project
glossaries only as terminology references, so established SL jargon
(rez, prim, region, grid, L$, …) matches what each language community
already knows.

## Non-goals

Chat machine translation is [[viewer-i18n-chat-translation]]. Translation
workflow tooling (Weblate or similar) becomes its own task if external
contributors materialize.
