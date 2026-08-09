//! The **shared URL-linkification layer** (`viewer-url-linkification`): the
//! text-decoration engine every text-bearing panel consumes to turn runs of plain
//! text into clickable links.
//!
//! # What it recognises
//!
//! Scanning a string left-to-right, it finds the link kinds the reference
//! `LLUrlRegistry` recognises for the subset the viewer targets:
//!
//! - plain `http(s)` / `ftp` URLs ([`LinkTarget::Web`]), split into **trusted**
//!   Second Life web hosts (open in the embedded browser) and untrusted external
//!   hosts (open in the system browser) — the reference `LLUrlEntrySecondlifeURL`
//!   vs. `LLUrlEntryHTTP` distinction;
//! - the **labelled** wiki-link form `[url  visible text]`, which shows the
//!   trailing text but targets the leading URL (the reference
//!   `LLUrlEntryHTTPLabel` / `LLUrlEntrySLLabel`);
//! - **SLURLs** — a location as `secondlife://Region/x/y/z` or as a
//!   `http(s)://maps.secondlife.com/secondlife/Region/x/y/z` map link
//!   ([`LinkTarget::Location`]);
//! - the `secondlife:///app/...` **entity links** — agent, group, parcel and
//!   object ([`LinkTarget::Agent`] / [`Group`](LinkTarget::Group) /
//!   [`Parcel`](LinkTarget::Parcel) / [`Object`](LinkTarget::Object)) — plus the
//!   region / teleport / world-map location apps.
//!
//! # Cross-grid links
//!
//! Every SLURL / app link can name **another grid** — the reference's
//! `secondlife://<Grid>/app/...`, `secondlife://<Grid>/secondlife/<region>/...`,
//! `hop://<grid>/...` and `x-grid-location-info://<host>/...` forms — so a link to
//! the Second Life beta grid (Aditi) or an OpenSim grid resolves too. The grid
//! host rides on the target as [`grid`](LinkTarget::Agent::grid); an empty grid is
//! the current grid. A name label is only resolved from the local caches for a
//! current-grid link (a cross-grid name is not in our caches), so a cross-grid
//! entity link shows its URL.
//!
//! # This module's job vs. the consumer's
//!
//! This is purely the **decoration** layer: [`linkify`] turns a string into an
//! ordered run of [`TextRun::Plain`] and [`TextRun::Link`] segments, each link
//! carrying its display label, its target URL, its [`LinkTarget`] and a tooltip.
//! It is a pure function — no ECS, no async — so the whole match / precedence /
//! label logic is unit-testable here, exactly like the reference
//! `LLUrlRegistry::findUrl`.
//!
//! What a click then *does* is split: the Bevy widget in
//! [`crate::linkified_text`] renders the runs, resolves agent / group names in
//! place, shows the **actual target URL** on hover so the user can vet a link
//! before clicking, and opens `Web` links (embedded browser for trusted SL hosts,
//! the system browser for external ones). Dispatching a *SLURL* action (teleport,
//! open profile, show parcel) is the separate [[viewer-slurl-parse-dispatch]]'s
//! job — the widget emits the [`LinkTarget`] for it to route.
//!
//! Reference (Firestorm, read-only): `llui/llurlregistry` (the leftmost-match
//! scan and the terminating-punctuation trim), `llui/llurlentry` (the per-kind
//! regexes and label / tooltip rules), `llui/llurlmatch` (the match record),
//! `newview/llslurl` (the grid-qualified SLURL forms), `llui/llurlaction` (the
//! click actions).

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use sl_client_bevy::{AgentKey, ExperienceKey, GroupKey, ObjectKey, ParcelKey, Uuid};

// ---------------------------------------------------------------------------
// The public output model: a run of plain / link segments.
// ---------------------------------------------------------------------------

/// One segment of a linkified string: either a run of plain text or a recognised
/// link. [`linkify`] returns these in source order, so concatenating every
/// segment's source text (`Plain` text or a `Link`'s [`matched`](LinkMatch::matched))
/// reproduces the original string exactly.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextRun {
    /// A run of plain, non-link text (rendered verbatim, no decoration).
    Plain(String),
    /// A recognised link (rendered as a coloured, clickable span).
    Link(LinkMatch),
}

/// A recognised link. Mirrors the reference `LLUrlMatch`, which keeps both the
/// **matched source text** (for reconstructing the string) and the **canonical
/// URL** (for the action and the hover preview) — they differ for a labelled
/// `[url text]` link, where the source is the whole bracketed run but the URL is
/// only the part before the label.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinkMatch {
    /// The exact substring that matched, as it appeared in the source. Used only
    /// to reconstruct the original string; never shown.
    pub(crate) matched: String,
    /// The canonical target URL — what a click opens / dispatches and what the
    /// widget shows on hover so the user can vet the destination before clicking.
    pub(crate) url: String,
    /// What clicking the link targets — the widget / dispatcher routes this.
    pub(crate) target: LinkTarget,
    /// How the link's visible label is produced (some are fixed at match time,
    /// agent / group / parcel labels resolve against the name caches later).
    pub(crate) label: LinkLabel,
    /// The leading icon the link shows (the reference `LLUrlEntry` `mIcon`).
    pub(crate) icon: LinkIcon,
    /// The Fluent key of the link's hover-tooltip category line (shown under the
    /// literal URL).
    pub(crate) tooltip_key: &'static str,
}

/// Which leading icon a link shows, mirroring the reference `LLUrlEntry` `mIcon`:
/// a person for an agent, a group glyph for a group, a location pin for a SLURL,
/// or none (plain web / object / parcel links carry no icon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkIcon {
    /// No leading icon.
    None,
    /// A resident (`Generic_Person`).
    Agent,
    /// A group (`Generic_Group`).
    Group,
    /// A location / SLURL (the reference `Hand`).
    Location,
}

/// How a link's visible label is produced. A [`Fixed`](LinkLabel::Fixed) label is
/// known at match time; the agent / group / parcel variants carry the key the
/// rendering widget resolves against the live name caches, falling back to
/// [`LinkLabel::fallback`] until a name arrives — mirroring the reference's
/// `AvatarNameWaiting` placeholder.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LinkLabel {
    /// A label known at match time (an `http` URL, a `[url text]` label, an
    /// object name, a `Region (x,y,z)` location, a cross-grid entity URL).
    Fixed(String),
    /// An agent whose name resolves from the avatar name cache, in the given
    /// name style (the reference distinguishes complete / display / username).
    Agent(AgentKey, AgentNameStyle),
    /// A group whose name resolves from the group name cache.
    Group(GroupKey),
    /// A parcel whose name resolves from a parcel-info lookup (deferred — the
    /// fallback text stands in until that lookup lands).
    Parcel(ParcelKey),
}

impl LinkLabel {
    /// The label to show before any name cache has answered — the reference's
    /// short "(Loading...)" placeholder for the resolving kinds, and the fixed
    /// text itself for a fixed label.
    pub(crate) fn fallback(&self) -> String {
        match self {
            Self::Fixed(text) => text.clone(),
            Self::Agent(..) | Self::Group(_) | Self::Parcel(_) => LOADING_LABEL.to_owned(),
        }
    }
}

/// Which form of an agent's name a `secondlife:///app/agent/...` link shows,
/// selected by the URL's action suffix (the reference `LLUrlEntryAgent*Name`
/// entries). All the action suffixes that are not a name request
/// (`/about`, `/im`, `/mute`, …) show the complete name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentNameStyle {
    /// `Display Name (username)` — the reference `getCompleteName`.
    Complete,
    /// The chosen display name alone.
    Display,
    /// The dotted `first.last` username alone.
    Username,
}

/// The grid a cross-grid link names — the host from a `secondlife://<Grid>/…`,
/// `hop://<grid>/…` or `x-grid-location-info://<host>/…` link. `None` is the
/// current grid (the plain `secondlife:///…` / `secondlife://Region/…` forms).
pub(crate) type Grid = Option<String>;

/// What a recognised link points at. The consumer routes this: the widget opens
/// [`Web`](LinkTarget::Web) links itself (internal vs. external browser by trust);
/// [[viewer-slurl-parse-dispatch]] dispatches the SLURL / entity targets. The
/// agent / group / parcel keys also drive the visible-label resolution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LinkTarget {
    /// A plain external URL (its canonical form is [`LinkMatch::url`]).
    Web {
        /// Whether the host is a **trusted** Second Life web host
        /// (`secondlife.com` / `lindenlab.com` / `tilia-inc.com` /
        /// `secondlifegrid.net` / `secondlife.io`). Trusted links open in the
        /// embedded browser; untrusted links open in the system browser — the
        /// reference internal-vs-external distinction.
        trusted: bool,
    },
    /// A `secondlife:///app/agent/<uuid>/<action>` link.
    Agent {
        /// The agent the link addresses.
        key: AgentKey,
        /// The action suffix (`about`, `im`, `mute`, …), verbatim after the id.
        action: String,
        /// The grid the link names, or `None` for the current grid.
        grid: Grid,
    },
    /// A `secondlife:///app/group/<uuid>/<action>` link.
    Group {
        /// The group the link addresses.
        key: GroupKey,
        /// The grid the link names, or `None` for the current grid.
        grid: Grid,
    },
    /// A `secondlife:///app/parcel/<uuid>/about` link.
    Parcel {
        /// The parcel the link addresses.
        key: ParcelKey,
        /// The grid the link names, or `None` for the current grid.
        grid: Grid,
    },
    /// A `secondlife:///app/objectim/<uuid>?name=..&owner=..&slurl=..` link — an
    /// object announcing itself. The name / owner / slurl ride in the query, so
    /// they are parsed out here rather than resolved from a cache.
    Object {
        /// The object's key.
        key: ObjectKey,
        /// The object's name, from the `name` query parameter (may be empty).
        name: String,
        /// The object owner's raw id, from the `owner` query parameter, if given
        /// (agent vs. group is not encoded, so it stays a raw [`Uuid`]).
        owner: Option<Uuid>,
        /// The object's location SLURL, from the `slurl` query parameter, if any.
        slurl: Option<String>,
        /// The grid the link names, or `None` for the current grid.
        grid: Grid,
    },
    /// A `secondlife:///app/object/<uuid>/<action>` link — an in-world object
    /// addressed by its key, with no name / owner in the URL (unlike
    /// [`Object`](LinkTarget::Object), the `objectim` form). The reference
    /// `LLObjectHandler` `inspect` verb opens the object inspector, which resolves
    /// the name / owner / description from an `ObjectPropertiesFamily` reply.
    ObjectAction {
        /// The object's key.
        key: ObjectKey,
        /// The action suffix (`inspect`, `zoomin`, …), verbatim after the id.
        action: String,
        /// The grid the link names, or `None` for the current grid.
        grid: Grid,
    },
    /// A `secondlife:///app/experience/<uuid>/profile` link — an experience
    /// profile.
    Experience {
        /// The experience the link addresses.
        key: ExperienceKey,
        /// The grid the link names, or `None` for the current grid.
        grid: Grid,
    },
    /// A location SLURL — a region / place / teleport / world-map link. The
    /// region name and coordinates are parsed out here so the SLURL dispatcher
    /// ([[viewer-slurl-parse-dispatch]]) can act on the destination directly
    /// (resolve the region, teleport, centre the map) without re-parsing the URL.
    Location {
        /// Which location app / form matched.
        kind: LocationKind,
        /// The grid the link names, or `None` for the current grid.
        grid: Grid,
        /// The (URL-unescaped) destination region name.
        region: String,
        /// The region-local arrival coordinates the URL carried, each present
        /// only when the URL supplied it (`Region`, `Region/x`, `Region/x/y`,
        /// `Region/x/y/z`). The reference clamps a coordinate the URL omits to
        /// the region centre (128) / ground (0) at teleport time.
        coords: LocationCoords,
    },
}

/// The region-local coordinates a location SLURL carried — each `None` when the
/// URL did not supply that component. Mirrors the reference `LLSLURL` position,
/// which fills an omitted coordinate with the region-centre default only when the
/// destination is finally resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LocationCoords {
    /// The east (X) coordinate, if the URL supplied it.
    pub(crate) x: Option<i32>,
    /// The north (Y) coordinate, if the URL supplied it.
    pub(crate) y: Option<i32>,
    /// The up (Z) coordinate, if the URL supplied it.
    pub(crate) z: Option<i32>,
}

/// Which SLURL / location form a [`LinkTarget::Location`] matched, so the
/// dispatcher can route it (a teleport teleports; a world-map link opens the map).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocationKind {
    /// A bare or grid-qualified location SLURL (`secondlife://[Grid/]Region/x/y/z`,
    /// `hop://grid/Region/x/y/z`, `x-grid-location-info://host/region/…`).
    Slurl,
    /// `http(s)://maps.secondlife.com|slurl.com/secondlife/Region/x/y/z`.
    MapUrl,
    /// `secondlife:///app/region/Region/x/y/z`.
    Region,
    /// `secondlife:///app/teleport/Region/x/y/z`.
    Teleport,
    /// `secondlife:///app/worldmap/Region/x/y/z`.
    WorldMap,
}

/// The placeholder shown for a name-resolving link before its name has arrived —
/// the widget swaps in the localised "(Loading...)" string; this ellipsis is the
/// ECS-free fallback the pure layer can produce (matching the reference's short
/// `AvatarNameWaiting` placeholder used for layout while the cache is queried).
pub(crate) const LOADING_LABEL: &str = "\u{2026}";

// ---------------------------------------------------------------------------
// Tooltip Fluent keys (mirroring the reference `Tooltip*` LLTrans strings).
// ---------------------------------------------------------------------------

/// Tooltip category for a plain web URL (reference `TooltipHttpUrl`).
const TOOLTIP_HTTP: &str = "link-tooltip-http";
/// Tooltip category for a location SLURL (reference `TooltipSLURL`).
const TOOLTIP_SLURL: &str = "link-tooltip-slurl";
/// Tooltip category for a `secondlife:///app/...` entity link (reference
/// `TooltipSLAPP`).
const TOOLTIP_SLAPP: &str = "link-tooltip-slapp";
/// Tooltip category for a parcel link (reference `TooltipParcelUrl`).
const TOOLTIP_PARCEL: &str = "link-tooltip-parcel";
/// Tooltip category for an agent link (reference `TooltipAgentUrl`).
const TOOLTIP_AGENT: &str = "link-tooltip-agent";
/// Tooltip category for a group link (reference `TooltipGroupUrl`).
const TOOLTIP_GROUP: &str = "link-tooltip-group";

// ---------------------------------------------------------------------------
// The scan: leftmost match across the registry, then recurse on the tail.
// ---------------------------------------------------------------------------

/// Turn `text` into an ordered run of plain / link segments. Concatenating the
/// segments' source text reproduces `text` exactly.
///
/// Faithful to the reference `LLTextBase` linkification loop: repeatedly find the
/// first (leftmost) URL in the remaining text, emit the plain text before it and
/// the link itself, then continue after the link. Non-matching text between and
/// after links is emitted as [`TextRun::Plain`].
pub(crate) fn linkify(text: &str) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let rest = text.get(cursor..).unwrap_or("");
        let Some((range, link)) = find_first(rest) else {
            break;
        };
        // Emit the plain text before the link (if any).
        if range.start > 0
            && let Some(plain) = rest.get(..range.start)
            && !plain.is_empty()
        {
            runs.push(TextRun::Plain(plain.to_owned()));
        }
        runs.push(TextRun::Link(link));
        // Advance past the matched link. `find_first` guarantees `end > start`.
        cursor = cursor.saturating_add(range.end);
    }
    if let Some(tail) = text.get(cursor..)
        && !tail.is_empty()
    {
        runs.push(TextRun::Plain(tail.to_owned()));
    }
    runs
}

/// Find the first (leftmost) link in `text`, returning its byte range and the
/// parsed [`LinkMatch`]. Mirrors `LLUrlRegistry::findUrl`: every registered entry
/// is tried, the match with the smallest start wins, and on a tie the
/// earlier-registered entry wins (the registry is ordered most-specific first).
///
/// After the winner is chosen, its trailing punctuation is trimmed
/// ([`trim_trailing_punctuation`]) and an `@`-preceded match (a stray email-ish
/// run) is rejected, exactly as the reference does.
fn find_first(text: &str) -> Option<(Range<usize>, LinkMatch)> {
    let mut best: Option<(usize, usize, &UrlEntry)> = None;
    for entry in REGISTRY.iter() {
        let Some(found) = entry.regex.find(text) else {
            continue;
        };
        let start = found.start();
        // Smaller start wins; the registry order breaks ties (a later equal-start
        // match does not replace the earlier one).
        if best.is_none_or(|(best_start, _, _)| start < best_start) {
            let end = trim_trailing_punctuation(text, found.start(), found.end());
            // A zero-or-negative-length span after trimming is not a real match.
            if end > start {
                best = Some((start, end, entry));
            }
        }
    }
    let (start, end, entry) = best?;
    // Reject an `@`-preceded match (MAINT-5371: an email whose user part is
    // empty), matching the reference guard.
    if start > 0 && text.get(start.saturating_sub(1)..start) == Some("@") {
        return None;
    }
    let matched = text.get(start..end)?;
    let link = (entry.build)(matched)?;
    Some((start..end, link))
}

/// Trim the reference's non-URL terminating punctuation from a `[start, end)`
/// match: a trailing `.` or `,`, or a trailing `)` / `]` with no matching opener
/// inside the match, is excluded from the link (so `see http://x.com/.` links
/// only `http://x.com/`). A labelled `[url text]` link ends in its own `]`, so
/// the bracket trim is skipped for it. Returns the adjusted exclusive end.
fn trim_trailing_punctuation(text: &str, start: usize, end: usize) -> usize {
    // A labelled link is delimited by its own brackets; never trim those.
    if text.get(start..start.saturating_add(1)) == Some("[") {
        return end;
    }
    let mut end = end;
    while end > start {
        let Some(last) = text.get(end.saturating_sub(1)..end) else {
            break;
        };
        let trim = match last {
            "." | "," => true,
            ")" => !span_contains(text, start, end, '('),
            "]" => !span_contains(text, start, end, '['),
            _other => false,
        };
        if trim {
            end = end.saturating_sub(1);
        } else {
            break;
        }
    }
    end
}

/// Whether the `[start, end)` slice of `text` contains `needle` — the reference's
/// "does this `)` have a matching `(` inside the match" test.
fn span_contains(text: &str, start: usize, end: usize, needle: char) -> bool {
    text.get(start..end)
        .is_some_and(|slice| slice.contains(needle))
}

// ---------------------------------------------------------------------------
// The registry: one entry per recognised link kind, most-specific first.
// ---------------------------------------------------------------------------

/// One registry entry: a compiled pattern and the builder that turns a matched
/// substring into a [`LinkMatch`]. Registered most-specific first so a tie at the
/// same start position resolves to the more specific kind (the reference order).
struct UrlEntry {
    /// The compiled match pattern.
    regex: Regex,
    /// Turn a matched substring into a link record (returns `None` if the
    /// substring fails a secondary check, e.g. a malformed UUID).
    build: fn(&str) -> Option<LinkMatch>,
}

/// Compile a pattern, panicking at first use if the literal is malformed (the
/// patterns are compile-time constants, so a bad one is a build-time authoring
/// error surfaced on the first lazy access).
fn compile(pattern: &str) -> Regex {
    #[expect(
        clippy::expect_used,
        reason = "the patterns are string literals fixed at authoring time; a \
                  malformed one is a programmer error to surface loudly, and \
                  there is no runtime input that can reach this"
    )]
    Regex::new(pattern).expect("url-linkify pattern must compile")
}

/// The `/app/...` header forms the reference recognises: the current grid
/// (`secondlife:///app`), a grid-qualified maingrid form
/// (`secondlife://<Grid>/app`), and two cross-grid schemes (`hop://<grid>/app`,
/// `x-grid-location-info://<host>/app`) — so a Second Life beta-grid (Aditi) or
/// OpenSim app link resolves too. The host may be empty (the current grid).
const APP_HEADER: &str =
    r"(?:secondlife://[^/ ]*/app|hop://[-\w.:@]+/app|x-grid-location-info://[-\w.]+/app)";

/// The ordered registry, built once. Order matters: the labelled `[url text]`
/// forms and the location-app / entity entries precede the generic `secondlife://`
/// SLURL and the plain-web entry so a tie at the same start resolves to the
/// specific kind, mirroring the reference registration order.
static REGISTRY: LazyLock<Vec<UrlEntry>> = LazyLock::new(|| {
    vec![
        // Labelled links `[url  visible text]` — first, so the leading `[` wins
        // the leftmost scan over the inner URL.
        UrlEntry {
            regex: compile(r"(?i)\[(https?|ftp)://\S+[ \t]+[^\]]+\]"),
            build: build_labeled,
        },
        UrlEntry {
            regex: compile(r"(?i)\[(secondlife|hop|x-grid-location-info)://\S+[ \t]+[^\]]+\]"),
            build: build_labeled,
        },
        // Map-hosted SLURLs (http to maps.secondlife.com / slurl.com).
        UrlEntry {
            regex: compile(
                r"(?i)https?://(maps\.secondlife\.com|slurl\.com)/secondlife/[^ /]+(/\d+){0,3}/?",
            ),
            build: build_map_url,
        },
        // The `/app/...` entity apps, most specific first. Each accepts the
        // current-grid and cross-grid header forms via `APP_HEADER`.
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/agent/[0-9a-f-]+/\w+")),
            build: build_agent,
        },
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/group/[0-9a-f-]+/\w+")),
            build: build_group,
        },
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/parcel/[0-9a-f-]+/about")),
            build: build_parcel,
        },
        UrlEntry {
            regex: compile(&format!(
                r"(?i){APP_HEADER}/objectim/[0-9a-f-]+\?[^ \t\r\n]*"
            )),
            build: build_object,
        },
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/object/[0-9a-f-]+/\w+")),
            build: build_object_action,
        },
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/experience/[0-9a-f-]+/profile")),
            build: build_experience,
        },
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/region/[^/ ]+(/\d+){{0,3}}/?")),
            build: build_region,
        },
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/teleport/[^/ ]+(/\d+){{0,3}}/?")),
            build: build_teleport,
        },
        UrlEntry {
            regex: compile(&format!(r"(?i){APP_HEADER}/worldmap/[^/ ]+(/\d+){{0,3}}/?")),
            build: build_worldmap,
        },
        // Grid-qualified location SLURL `secondlife://<Grid>/secondlife/Region/x/y/z`.
        UrlEntry {
            regex: compile(r"(?i)secondlife://[^/ ]+/secondlife/[^/ ]+(/-?\d+){0,3}/?"),
            build: build_grid_slurl,
        },
        // `x-grid-location-info://<host>/region/Region/x/y/z`.
        UrlEntry {
            regex: compile(r"(?i)x-grid-location-info://[-\w.]+/region/[^/ ]+(/-?\d+){0,3}/?"),
            build: build_xgrid_location,
        },
        // `hop://<grid>/Region/x/y/z`.
        UrlEntry {
            regex: compile(r"(?i)hop://[-\w.:@]+/[^/ ]+(/-?\d+){1,3}/?"),
            build: build_hop_location,
        },
        // The bare current-grid location SLURL `secondlife://Region/x/y/z`.
        // Registered after the `/app/` apps and the grid-qualified form so those
        // win the tie (this pattern requires a non-empty host and at least one
        // coordinate, so neither `///app/...` nor `//Grid/secondlife/...` matches
        // it).
        UrlEntry {
            regex: compile(r"(?i)secondlife://[^/ ]+(/-?\d+){1,3}/?"),
            build: build_slurl,
        },
        // Web URLs — last, being the most generic. Mirrors the reference
        // `(https?|ftp)://([^\s/?\.#]+\.?)+\.\w+(:\d+)?(/[^\s]*)?`. The
        // trusted-vs-external split (reference `LLUrlEntrySecondlifeURL` vs.
        // `LLUrlEntryHTTP`) is decided in the builder from the parsed host, since
        // it needs the reference's `(?!\S)` "host ends here" check that the
        // lookahead-free `regex` crate cannot express in the pattern.
        UrlEntry {
            regex: compile(r"(?i)(https?|ftp)://([^\s/?.#]+\.?)+\.\w+(:\d+)?(/[^\s]*)?"),
            build: build_http,
        },
    ]
});

// ---------------------------------------------------------------------------
// Per-kind builders.
// ---------------------------------------------------------------------------

/// Build a web-link record. The label is the URL as it appeared (the reference
/// shows the full URL text); trust is decided from the host, so a
/// `secondlife.com` link opens in the embedded browser and everything else in the
/// system browser.
fn build_http(matched: &str) -> Option<LinkMatch> {
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::Web {
            trusted: is_trusted_web_host(matched),
        },
        label: LinkLabel::Fixed(matched.to_owned()),
        icon: LinkIcon::None,
        tooltip_key: TOOLTIP_HTTP,
    })
}

/// The Second Life web hosts the reference treats as trusted (embedded-browser
/// eligible) — the `LLUrlEntrySecondlifeURL` host set.
const TRUSTED_WEB_HOSTS: &[&str] = &[
    "secondlife.com",
    "lindenlab.com",
    "tilia-inc.com",
    "secondlifegrid.net",
    "secondlife.io",
];

/// Whether a web URL's host is a trusted Second Life host — the host itself, or
/// any subdomain of it. A look-alike like `secondlife.com.evil.example` is *not*
/// trusted (it ends in `.evil.example`, not a trusted suffix), which is the check
/// the reference's `(?!\S)` "host ends here" lookahead enforces.
fn is_trusted_web_host(url: &str) -> bool {
    let after_scheme = url.split_once("://").map_or(url, |(_scheme, rest)| rest);
    let host = after_scheme
        .split(['/', ':', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    TRUSTED_WEB_HOSTS
        .iter()
        .any(|trusted| host == *trusted || host.ends_with(&format!(".{trusted}")))
}

/// Build a labelled `[url  visible text]` link: the trailing text is the label,
/// the leading URL is re-classified through the registry so a `[secondlife://...
/// text]` link keeps its SLURL / entity target. The whole bracketed run is the
/// matched source; the inner URL is the canonical target. Mirrors the reference
/// `LLUrlEntryHTTPLabel` / `LLUrlEntrySLLabel`.
fn build_labeled(matched: &str) -> Option<LinkMatch> {
    let inner = matched.strip_prefix('[')?.strip_suffix(']')?;
    let split = inner.find([' ', '\t'])?;
    let url_part = inner.get(..split)?;
    let label_text = inner.get(split..)?.trim();
    if url_part.is_empty() || label_text.is_empty() {
        return None;
    }
    // Re-classify the inner URL to inherit its target / tooltip, but require it to
    // be a single link spanning the whole URL part.
    let (range, inner_link) = find_first(url_part)?;
    if range != (0..url_part.len()) {
        return None;
    }
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: inner_link.url,
        target: inner_link.target,
        label: LinkLabel::Fixed(label_text.to_owned()),
        icon: inner_link.icon,
        tooltip_key: inner_link.tooltip_key,
    })
}

/// Build an `/app/agent/<uuid>/<action>` record. The name style is chosen from the
/// action suffix; the visible label resolves from the avatar name cache for a
/// current-grid link, and is the raw URL for a cross-grid one (not in our cache).
/// Returns `None` if the id is not a valid UUID.
fn build_agent(matched: &str) -> Option<LinkMatch> {
    let (grid, rest) = split_app(matched, "agent")?;
    let (id_str, action) = rest.split_once('/')?;
    let key = AgentKey::from(parse_uuid(id_str)?);
    let style = match action.to_ascii_lowercase().as_str() {
        "username" => AgentNameStyle::Username,
        "displayname" => AgentNameStyle::Display,
        _complete => AgentNameStyle::Complete,
    };
    let label = if grid.is_none() {
        LinkLabel::Agent(key, style)
    } else {
        LinkLabel::Fixed(matched.to_owned())
    };
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::Agent {
            key,
            action: action.to_owned(),
            grid,
        },
        label,
        icon: LinkIcon::Agent,
        tooltip_key: TOOLTIP_AGENT,
    })
}

/// Build a `/app/group/<uuid>/<action>` record — the label resolves from the group
/// name cache for a current-grid link. Returns `None` for a malformed id.
fn build_group(matched: &str) -> Option<LinkMatch> {
    let (grid, rest) = split_app(matched, "group")?;
    let (id_str, _action) = rest.split_once('/')?;
    let key = GroupKey::from(parse_uuid(id_str)?);
    let label = if grid.is_none() {
        LinkLabel::Group(key)
    } else {
        LinkLabel::Fixed(matched.to_owned())
    };
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::Group { key, grid },
        label,
        icon: LinkIcon::Group,
        tooltip_key: TOOLTIP_GROUP,
    })
}

/// Build a `/app/parcel/<uuid>/about` record — the label resolves from a
/// parcel-info lookup (deferred), so the fallback placeholder stands in.
fn build_parcel(matched: &str) -> Option<LinkMatch> {
    let (grid, rest) = split_app(matched, "parcel")?;
    let (id_str, _about) = rest.split_once('/')?;
    let key = ParcelKey::from(parse_uuid(id_str)?);
    let label = if grid.is_none() {
        LinkLabel::Parcel(key)
    } else {
        LinkLabel::Fixed(matched.to_owned())
    };
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::Parcel { key, grid },
        label,
        icon: LinkIcon::None,
        tooltip_key: TOOLTIP_PARCEL,
    })
}

/// Build a `/app/objectim/<uuid>?name=..&owner=..&slurl=..` record. The name /
/// owner / slurl ride in the query string, so they are parsed out and the label
/// is the object name (or the raw URL when unnamed), matching the reference
/// `LLUrlEntryObjectIM::getLabel`.
fn build_object(matched: &str) -> Option<LinkMatch> {
    let (grid, rest) = split_app(matched, "objectim")?;
    let (id_str, query) = rest.split_once('?')?;
    let key = ObjectKey::from(parse_uuid(id_str)?);
    let name = query_param(query, "name").unwrap_or_default();
    let owner = query_param(query, "owner").and_then(|owner| parse_uuid(&owner));
    let slurl = query_param(query, "slurl");
    let label = if name.is_empty() {
        matched.to_owned()
    } else {
        name.clone()
    };
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::Object {
            key,
            name,
            owner,
            slurl,
            grid,
        },
        label: LinkLabel::Fixed(label),
        icon: LinkIcon::None,
        tooltip_key: TOOLTIP_SLAPP,
    })
}

/// Build a `/app/object/<uuid>/<action>` record — an in-world object addressed by
/// key. The name is not in the URL (an `inspect` opens the object inspector, which
/// resolves the name / owner from an `ObjectPropertiesFamily` reply), so the label
/// is the raw URL. Returns `None` for a malformed id.
fn build_object_action(matched: &str) -> Option<LinkMatch> {
    let (grid, rest) = split_app(matched, "object")?;
    let (id_str, action) = rest.split_once('/')?;
    let key = ObjectKey::from(parse_uuid(id_str)?);
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::ObjectAction {
            key,
            action: action.to_owned(),
            grid,
        },
        label: LinkLabel::Fixed(matched.to_owned()),
        icon: LinkIcon::None,
        tooltip_key: TOOLTIP_SLAPP,
    })
}

/// Build a `/app/experience/<uuid>/profile` record — an experience profile. The
/// experience name is not resolved here (the fallback URL stands in), matching the
/// reference `LLUrlEntryExperienceProfile`, so a caller that already knows the
/// name (the experience-permission card) supplies it via a labelled link.
fn build_experience(matched: &str) -> Option<LinkMatch> {
    let (grid, rest) = split_app(matched, "experience")?;
    let (id_str, _profile) = rest.split_once('/')?;
    let key = ExperienceKey::from(parse_uuid(id_str)?);
    Some(LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::Experience { key, grid },
        label: LinkLabel::Fixed(matched.to_owned()),
        icon: LinkIcon::None,
        tooltip_key: TOOLTIP_SLAPP,
    })
}

/// Build a `/app/region/Region/x/y/z` location record.
fn build_region(matched: &str) -> Option<LinkMatch> {
    build_app_location(matched, "region", LocationKind::Region)
}

/// Build a `/app/teleport/Region/x/y/z` location record.
fn build_teleport(matched: &str) -> Option<LinkMatch> {
    build_app_location(matched, "teleport", LocationKind::Teleport)
}

/// Build a `/app/worldmap/Region/x/y/z` location record.
fn build_worldmap(matched: &str) -> Option<LinkMatch> {
    build_app_location(matched, "worldmap", LocationKind::WorldMap)
}

/// The shared builder for the `/app/{region,teleport,worldmap}/...` location
/// apps: the path after the entity is `Region[/x[/y[/z]]]`, and the label is
/// `Region (x,y,z)`, matching the reference `LLUrlEntryRegion::getLabel`.
fn build_app_location(matched: &str, entity: &str, kind: LocationKind) -> Option<LinkMatch> {
    let (grid, rest) = split_app(matched, entity)?;
    Some(location_link(
        matched,
        kind,
        grid,
        rest.trim_end_matches('/'),
    ))
}

/// Build a bare current-grid `secondlife://Region/x/y/z` SLURL — the reference
/// `LLUrlEntryPlace`.
fn build_slurl(matched: &str) -> Option<LinkMatch> {
    let rest = matched.strip_prefix_ci("secondlife://")?;
    Some(location_link(
        matched,
        LocationKind::Slurl,
        None,
        rest.trim_end_matches('/'),
    ))
}

/// Build a grid-qualified `secondlife://<Grid>/secondlife/Region/x/y/z` SLURL: the
/// host is the grid, the region path follows `/secondlife/`.
fn build_grid_slurl(matched: &str) -> Option<LinkMatch> {
    let rest = matched.strip_prefix_ci("secondlife://")?;
    let (grid, after) = rest.split_once("/secondlife/")?;
    Some(location_link(
        matched,
        LocationKind::Slurl,
        grid_of(grid),
        after.trim_end_matches('/'),
    ))
}

/// Build a `hop://<grid>/Region/x/y/z` cross-grid location SLURL.
fn build_hop_location(matched: &str) -> Option<LinkMatch> {
    let rest = matched.strip_prefix_ci("hop://")?;
    let (grid, after) = rest.split_once('/')?;
    Some(location_link(
        matched,
        LocationKind::Slurl,
        grid_of(grid),
        after.trim_end_matches('/'),
    ))
}

/// Build an `x-grid-location-info://<host>/region/Region/x/y/z` cross-grid SLURL.
fn build_xgrid_location(matched: &str) -> Option<LinkMatch> {
    let rest = matched.strip_prefix_ci("x-grid-location-info://")?;
    let (grid, after) = rest.split_once("/region/")?;
    Some(location_link(
        matched,
        LocationKind::Slurl,
        grid_of(grid),
        after.trim_end_matches('/'),
    ))
}

/// Build a `http(s)://maps.secondlife.com/secondlife/Region/x/y/z` map-URL record
/// (always the main grid), matching the reference `LLUrlEntrySLURL::getLabel`.
fn build_map_url(matched: &str) -> Option<LinkMatch> {
    let after = matched
        .split_once("/secondlife/")
        .map(|(_host, tail)| tail)?;
    Some(location_link(
        matched,
        LocationKind::MapUrl,
        None,
        after.trim_end_matches('/'),
    ))
}

/// Assemble a [`LinkTarget::Location`] link from a `Region[/x[/y[/z]]]` path: the
/// region name and coordinates are parsed out for the target, and the visible
/// label is `Region (x,y,z)`.
fn location_link(matched: &str, kind: LocationKind, grid: Grid, path: &str) -> LinkMatch {
    let (region, coords) = parse_location_path(path);
    LinkMatch {
        matched: matched.to_owned(),
        url: matched.to_owned(),
        target: LinkTarget::Location {
            kind,
            grid,
            region: region.clone(),
            coords,
        },
        label: LinkLabel::Fixed(location_label(&region, coords)),
        icon: LinkIcon::Location,
        tooltip_key: TOOLTIP_SLURL,
    }
}

// ---------------------------------------------------------------------------
// Small parsing helpers.
// ---------------------------------------------------------------------------

/// Split an `/app/<entity>/...` URL into its grid (the host naming another grid,
/// or `None` for the current grid) and the part after `/app/<entity>/`. Works for
/// every `APP_HEADER` form (`secondlife://[Grid]/app`, `hop://grid/app`,
/// `x-grid-location-info://host/app`), since it keys off the `/app/<entity>/`
/// marker and reads the host from whatever scheme prefix preceded it.
fn split_app<'src>(matched: &'src str, entity: &str) -> Option<(Grid, &'src str)> {
    let marker = format!("/app/{entity}/");
    // ASCII lower-casing preserves byte length, so the found index is valid on the
    // original string too.
    let idx = matched.to_ascii_lowercase().find(&marker)?;
    let header = matched.get(..idx)?;
    let grid = header
        .split_once("://")
        .map(|(_scheme, host)| host)
        .and_then(grid_of);
    let rest = matched.get(idx.saturating_add(marker.len())..)?;
    Some((grid, rest))
}

/// A grid host as a [`Grid`]: `Some` when non-empty, `None` for the current grid.
fn grid_of(host: &str) -> Grid {
    (!host.is_empty()).then(|| host.to_owned())
}

/// Split a `Region[/x[/y[/z]]]` location path into its (URL-unescaped) region
/// name and up to three integer coordinates — the structured form the SLURL
/// dispatcher acts on. A non-numeric coordinate segment parses to `None` (the
/// match regex only admits digits, so this is a belt-and-braces guard).
fn parse_location_path(path: &str) -> (String, LocationCoords) {
    let mut parts = path.split('/');
    let region = parts.next().map(unescape_url).unwrap_or_default();
    let mut coords = parts
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().ok());
    (
        region,
        LocationCoords {
            x: coords.next().flatten(),
            y: coords.next().flatten(),
            z: coords.next().flatten(),
        },
    )
}

/// Format a location label from a parsed region name and coordinates: the region
/// name, then the coordinates parenthesised — `Ahern (128,128,24)`,
/// `Ahern (128,128)`, `Ahern (128)`, or just `Ahern`. Mirrors the reference
/// `getLabel` coordinate handling.
fn location_label(region: &str, coords: LocationCoords) -> String {
    let present: Vec<String> = [coords.x, coords.y, coords.z]
        .into_iter()
        .flatten()
        .map(|coord| coord.to_string())
        .collect();
    if present.is_empty() {
        region.to_owned()
    } else {
        format!("{region} ({})", present.join(","))
    }
}

/// Look a query parameter up in a `key=value&key=value` query string, returning
/// its URL-unescaped value. Absent keys give `None`; a present key with no `=`
/// gives an empty string.
fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        name.eq_ignore_ascii_case(key).then(|| unescape_url(value))
    })
}

/// Parse a hyphenated or bare UUID, returning `None` for anything malformed.
fn parse_uuid(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text).ok()
}

/// Minimal percent-decoding for a URL path / query component, plus `+` → space in
/// the query. Enough for the region names and object names SLURLs carry; a stray
/// `%` with no valid hex pair is left verbatim.
fn unescape_url(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '+' => out.push(' '),
            '%' => {
                let hi = chars.clone().next().and_then(|c| c.to_digit(16));
                let lo = chars.clone().nth(1).and_then(|c| c.to_digit(16));
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    let byte = hi.saturating_mul(16).saturating_add(lo);
                    if let Some(decoded) = char::from_u32(byte) {
                        out.push(decoded);
                    }
                    chars.next();
                    chars.next();
                } else {
                    out.push('%');
                }
            }
            _other => out.push(ch),
        }
    }
    out
}

/// A case-insensitive `strip_prefix`, for the fixed scheme prefixes (the scheme is
/// matched case-insensitively by the regex, so the matched text can carry any
/// case).
trait StripPrefixCi {
    /// Strip `prefix` (compared ASCII-case-insensitively) from the front.
    fn strip_prefix_ci(&self, prefix: &str) -> Option<&str>;
}

impl StripPrefixCi for str {
    fn strip_prefix_ci(&self, prefix: &str) -> Option<&str> {
        let head = self.get(..prefix.len())?;
        head.eq_ignore_ascii_case(prefix)
            .then(|| self.get(prefix.len()..))
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentNameStyle, LinkIcon, LinkLabel, LinkMatch, LinkTarget, LocationKind, TextRun, linkify,
        location_label, parse_location_path, query_param, split_app, trim_trailing_punctuation,
        unescape_url,
    };
    use crate::ui_test::TestError;
    use pretty_assertions::assert_eq;

    /// The single link in `text`, or an error if there is not exactly one — so a
    /// test can `?` it rather than reach for `unwrap` / `expect` (which the
    /// restriction lints forbid, tests included).
    fn only_link(text: &str) -> Result<LinkMatch, TestError> {
        let mut links = linkify(text).into_iter().filter_map(|run| match run {
            TextRun::Link(link) => Some(link),
            TextRun::Plain(_) => None,
        });
        let link = links.next().ok_or("expected a link, found none")?;
        if links.next().is_some() {
            return Err("expected exactly one link, found several".into());
        }
        Ok(link)
    }

    /// Whether `text` linkifies to at least one link.
    fn has_link(text: &str) -> bool {
        linkify(text)
            .iter()
            .any(|run| matches!(run, TextRun::Link(_)))
    }

    /// Each link kind carries the reference icon: a person for an agent, a group
    /// glyph for a group, a location pin for a SLURL, and none for a plain web /
    /// object / parcel link.
    #[test]
    fn links_carry_the_reference_icon() -> Result<(), TestError> {
        let uuid = "0e346d8b-4433-4d66-a6b0-fd37083abc4c";
        assert_eq!(
            only_link(&format!("secondlife:///app/agent/{uuid}/about"))?.icon,
            LinkIcon::Agent
        );
        assert_eq!(
            only_link(&format!("secondlife:///app/group/{uuid}/about"))?.icon,
            LinkIcon::Group
        );
        assert_eq!(
            only_link("secondlife://Ahern/128/128/24")?.icon,
            LinkIcon::Location
        );
        assert_eq!(only_link("https://example.com/")?.icon, LinkIcon::None);
        assert_eq!(
            only_link(&format!("secondlife:///app/parcel/{uuid}/about"))?.icon,
            LinkIcon::None
        );
        Ok(())
    }

    #[test]
    fn plain_text_has_no_links() {
        assert_eq!(
            linkify("hello world"),
            vec![TextRun::Plain("hello world".to_owned())]
        );
    }

    #[test]
    fn http_url_is_an_untrusted_web_link_split_from_surrounding_text() {
        let runs = linkify("see https://example.com/page now");
        assert_eq!(
            runs,
            vec![
                TextRun::Plain("see ".to_owned()),
                TextRun::Link(LinkMatch {
                    matched: "https://example.com/page".to_owned(),
                    url: "https://example.com/page".to_owned(),
                    target: LinkTarget::Web { trusted: false },
                    label: LinkLabel::Fixed("https://example.com/page".to_owned()),
                    icon: LinkIcon::None,
                    tooltip_key: super::TOOLTIP_HTTP,
                }),
                TextRun::Plain(" now".to_owned()),
            ]
        );
    }

    #[test]
    fn secondlife_web_hosts_are_trusted() -> Result<(), TestError> {
        assert_eq!(
            only_link("visit https://secondlife.com/support today")?.target,
            LinkTarget::Web { trusted: true }
        );
        assert_eq!(
            only_link("https://community.secondlife.com/")?.target,
            LinkTarget::Web { trusted: true }
        );
        assert_eq!(
            only_link("https://secondlife.com.evil.example/")?.target,
            LinkTarget::Web { trusted: false }
        );
        Ok(())
    }

    #[test]
    fn labelled_link_shows_text_but_targets_the_url() -> Result<(), TestError> {
        let link = only_link("[https://example.com/deep/page  Click here]")?;
        assert_eq!(link.label, LinkLabel::Fixed("Click here".to_owned()));
        assert_eq!(link.url, "https://example.com/deep/page");
        assert_eq!(link.target, LinkTarget::Web { trusted: false });
        assert_eq!(link.matched, "[https://example.com/deep/page  Click here]");
        Ok(())
    }

    #[test]
    fn labelled_slurl_keeps_its_location_target() -> Result<(), TestError> {
        let link = only_link("[secondlife://Ahern/128/128/24  My spot]")?;
        assert_eq!(link.label, LinkLabel::Fixed("My spot".to_owned()));
        assert!(matches!(
            link.target,
            LinkTarget::Location {
                kind: LocationKind::Slurl,
                grid: None,
                ..
            }
        ));
        assert_eq!(link.url, "secondlife://Ahern/128/128/24");
        Ok(())
    }

    #[test]
    fn labelled_agent_keeps_its_agent_target_and_a_fixed_label() -> Result<(), TestError> {
        let uuid = "0e346d8b-4433-4d66-a6b0-fd37083abc4c";
        let link = only_link(&format!("[secondlife:///app/agent/{uuid}/about  Ping me]"))?;
        assert_eq!(link.label, LinkLabel::Fixed("Ping me".to_owned()));
        assert!(matches!(link.target, LinkTarget::Agent { .. }));
        Ok(())
    }

    #[test]
    fn trailing_sentence_punctuation_is_not_part_of_the_link() -> Result<(), TestError> {
        assert_eq!(
            only_link("go to http://foo.com/.")?.matched,
            "http://foo.com/"
        );
        assert_eq!(
            only_link("http://foo.com/, then")?.matched,
            "http://foo.com/"
        );
        Ok(())
    }

    #[test]
    fn unbalanced_bracket_is_trimmed_but_balanced_is_kept() -> Result<(), TestError> {
        assert_eq!(
            only_link("(see http://foo.com/bar)")?.matched,
            "http://foo.com/bar"
        );
        assert_eq!(
            only_link("http://foo.com/bar_(baz)")?.matched,
            "http://foo.com/bar_(baz)"
        );
        Ok(())
    }

    #[test]
    fn email_like_at_prefixed_run_is_rejected() {
        assert!(!has_link("mail@http://x.com"));
    }

    #[test]
    fn agent_app_link_resolves_to_a_name_style() -> Result<(), TestError> {
        let uuid = "0e346d8b-4433-4d66-a6b0-fd37083abc4c";
        let about = only_link(&format!("secondlife:///app/agent/{uuid}/about"))?;
        assert!(matches!(
            about.label,
            LinkLabel::Agent(_, AgentNameStyle::Complete)
        ));
        assert!(matches!(about.target, LinkTarget::Agent { grid: None, .. }));

        let username = only_link(&format!("secondlife:///app/agent/{uuid}/username"))?;
        assert!(matches!(
            username.label,
            LinkLabel::Agent(_, AgentNameStyle::Username)
        ));
        let display = only_link(&format!("secondlife:///app/agent/{uuid}/displayname"))?;
        assert!(matches!(
            display.label,
            LinkLabel::Agent(_, AgentNameStyle::Display)
        ));
        Ok(())
    }

    #[test]
    fn group_and_parcel_app_links() -> Result<(), TestError> {
        let uuid = "0000060e-4b39-e00b-d0c3-d98b1934e3a8";
        let group = only_link(&format!("secondlife:///app/group/{uuid}/about"))?;
        assert!(matches!(group.target, LinkTarget::Group { grid: None, .. }));
        assert!(matches!(group.label, LinkLabel::Group(_)));

        let parcel = only_link(&format!("secondlife:///app/parcel/{uuid}/about"))?;
        assert!(matches!(
            parcel.target,
            LinkTarget::Parcel { grid: None, .. }
        ));
        assert!(matches!(parcel.label, LinkLabel::Parcel(_)));
        Ok(())
    }

    #[test]
    fn experience_profile_link() -> Result<(), TestError> {
        let uuid = "0e346d8b-4433-4d66-a6b0-fd37083abc4c";
        let link = only_link(&format!("secondlife:///app/experience/{uuid}/profile"))?;
        assert!(matches!(
            link.target,
            LinkTarget::Experience { grid: None, .. }
        ));
        // No experience-name cache, so the label is the URL (a caller that knows
        // the name supplies it via a labelled link).
        assert!(matches!(link.label, LinkLabel::Fixed(_)));
        Ok(())
    }

    #[test]
    fn object_im_link_reads_name_owner_and_slurl_from_the_query() -> Result<(), TestError> {
        let uuid = "0e346d8b-4433-4d66-a6b0-fd37083abc4c";
        let owner = "aaaa060e-4b39-e00b-d0c3-d98b1934e3a8";
        let url = format!(
            "secondlife:///app/objectim/{uuid}?name=Info%20Kiosk&owner={owner}&slurl=Ahern/128/128"
        );
        let link = only_link(&url)?;
        let LinkTarget::Object {
            name,
            owner: got,
            slurl,
            ..
        } = link.target
        else {
            return Err("expected an object target".into());
        };
        assert_eq!(name, "Info Kiosk");
        assert!(got.is_some());
        assert_eq!(slurl.as_deref(), Some("Ahern/128/128"));
        assert_eq!(link.label, LinkLabel::Fixed("Info Kiosk".to_owned()));
        Ok(())
    }

    #[test]
    fn object_app_inspect_link_carries_the_action() -> Result<(), TestError> {
        let uuid = "0e346d8b-4433-4d66-a6b0-fd37083abc4c";
        let link = only_link(&format!("secondlife:///app/object/{uuid}/inspect"))?;
        let LinkTarget::ObjectAction {
            action, grid: None, ..
        } = &link.target
        else {
            return Err("expected an object-action target".into());
        };
        assert_eq!(action, "inspect");
        Ok(())
    }

    #[test]
    fn bare_slurl_and_map_url_share_the_location_label() -> Result<(), TestError> {
        let slurl = only_link("secondlife://Ahern/128/128/24")?;
        assert_eq!(
            slurl.label,
            LinkLabel::Fixed("Ahern (128,128,24)".to_owned())
        );
        assert!(matches!(
            &slurl.target,
            LinkTarget::Location {
                kind: LocationKind::Slurl,
                grid: None,
                region,
                coords,
            } if region == "Ahern"
                && *coords == super::LocationCoords {
                    x: Some(128),
                    y: Some(128),
                    z: Some(24),
                }
        ));

        let map = only_link("http://maps.secondlife.com/secondlife/Ahern/128/128/24")?;
        assert_eq!(map.label, LinkLabel::Fixed("Ahern (128,128,24)".to_owned()));
        assert!(matches!(
            map.target,
            LinkTarget::Location {
                kind: LocationKind::MapUrl,
                grid: None,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn region_app_link_beats_the_generic_slurl_at_the_same_start() -> Result<(), TestError> {
        let link = only_link("secondlife:///app/region/Ahern/128/128/0")?;
        assert!(matches!(
            link.target,
            LinkTarget::Location {
                kind: LocationKind::Region,
                grid: None,
                ..
            }
        ));
        assert_eq!(link.label, LinkLabel::Fixed("Ahern (128,128,0)".to_owned()));
        Ok(())
    }

    #[test]
    fn cross_grid_agent_link_carries_the_grid_and_shows_the_url() -> Result<(), TestError> {
        // A grid-qualified maingrid app link: the host is the grid.
        let uuid = "0e346d8b-4433-4d66-a6b0-fd37083abc4c";
        let link = only_link(&format!("secondlife://Aditi/app/agent/{uuid}/about"))?;
        let LinkTarget::Agent { grid, .. } = &link.target else {
            return Err("expected an agent target".into());
        };
        assert_eq!(grid.as_deref(), Some("Aditi"));
        // A cross-grid name is not in our cache, so the label is fixed (the URL).
        assert!(matches!(link.label, LinkLabel::Fixed(_)));
        Ok(())
    }

    #[test]
    fn cross_grid_location_forms() -> Result<(), TestError> {
        // Grid-qualified secondlife:// location.
        let sl = only_link("secondlife://Aditi/secondlife/Morris/128/128/24")?;
        let LinkTarget::Location { grid, .. } = &sl.target else {
            return Err("expected a location".into());
        };
        assert_eq!(grid.as_deref(), Some("Aditi"));
        assert_eq!(sl.label, LinkLabel::Fixed("Morris (128,128,24)".to_owned()));

        // hop:// cross-grid location.
        let hop = only_link("hop://grid.example.org:8002/Sandbox/10/20/30")?;
        let LinkTarget::Location { grid, .. } = &hop.target else {
            return Err("expected a location".into());
        };
        assert_eq!(grid.as_deref(), Some("grid.example.org:8002"));
        assert_eq!(hop.label, LinkLabel::Fixed("Sandbox (10,20,30)".to_owned()));

        // A hop:// app link is an entity, not a location.
        let uuid = "0000060e-4b39-e00b-d0c3-d98b1934e3a8";
        let hop_group = only_link(&format!("hop://grid.example.org/app/group/{uuid}/about"))?;
        let LinkTarget::Group { grid, .. } = &hop_group.target else {
            return Err("expected a group target".into());
        };
        assert_eq!(grid.as_deref(), Some("grid.example.org"));
        Ok(())
    }

    #[test]
    fn split_app_reads_grid_and_rest_for_each_header_form() {
        assert_eq!(
            split_app("secondlife:///app/agent/abc/about", "agent"),
            Some((None, "abc/about"))
        );
        assert_eq!(
            split_app("secondlife://Aditi/app/agent/abc/about", "agent"),
            Some((Some("Aditi".to_owned()), "abc/about"))
        );
        assert_eq!(
            split_app("hop://grid.example/app/group/xyz/about", "group"),
            Some((Some("grid.example".to_owned()), "xyz/about"))
        );
    }

    #[test]
    fn location_label_handles_each_coordinate_arity() {
        let label = |path: &str| {
            let (region, coords) = parse_location_path(path);
            location_label(&region, coords)
        };
        assert_eq!(label("Ahern/1/2/3"), "Ahern (1,2,3)");
        assert_eq!(label("Ahern/1/2"), "Ahern (1,2)");
        assert_eq!(label("Ahern/1"), "Ahern (1)");
        assert_eq!(label("Ahern"), "Ahern");
        assert_eq!(label("Da%20Boom/1/2"), "Da Boom (1,2)");
    }

    #[test]
    fn several_links_and_plain_runs_interleave_in_order() -> Result<(), TestError> {
        let runs = linkify("hi http://a.com and secondlife://Ahern/1/2 bye");
        assert_eq!(runs.len(), 5);
        let mut it = runs.iter();
        assert!(matches!(it.next(), Some(TextRun::Plain(text)) if text == "hi "));
        assert!(matches!(it.next(), Some(TextRun::Link(_))));
        assert!(matches!(it.next(), Some(TextRun::Plain(text)) if text == " and "));
        assert!(matches!(it.next(), Some(TextRun::Link(_))));
        assert!(matches!(it.next(), Some(TextRun::Plain(text)) if text == " bye"));
        Ok(())
    }

    #[test]
    fn concatenating_runs_reproduces_the_source() {
        let source = "a [http://x.com/p label] b secondlife://R/1/2/3 c mail@http://y.com d";
        let rebuilt: String = linkify(source)
            .into_iter()
            .map(|run| match run {
                TextRun::Plain(text) => text,
                TextRun::Link(link) => link.matched,
            })
            .collect();
        assert_eq!(rebuilt, source);
    }

    #[test]
    fn query_param_and_unescape_helpers() {
        assert_eq!(
            query_param("name=Hello%20There&x=1", "name").as_deref(),
            Some("Hello There")
        );
        assert_eq!(query_param("a=1&b=2", "missing"), None);
        assert_eq!(unescape_url("a+b%2Fc"), "a b/c");
        assert_eq!(unescape_url("50%discount"), "50%discount");
    }

    #[test]
    fn trim_helper_leaves_a_clean_url_untouched() {
        let url = "http://foo.com/bar";
        assert_eq!(trim_trailing_punctuation(url, 0, url.len()), url.len());
    }
}
