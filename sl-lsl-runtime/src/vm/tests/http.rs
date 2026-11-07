//! Outbound HTTP and in-world URLs held to the grids: the `http/*` probes run
//! in the VM under each flavour, their requests routed between the region's
//! scripts by the test region's URL surface ([`super::comms::Region::route_http`],
//! built on `vm::http`, as the fake grid's is), and every `PROBE` and
//! `DEBUG_CHANNEL` line set against the transcript's but for the fields the
//! probe's header names as varying and the lines named as deviations.

use pretty_assertions::assert_eq;

use super::clocks::run_for;
use super::comms::Region;
use super::prim_state::debug_notes;
use super::strings::probe_lines;
use super::*;

/// The transcripts of the probe `http/<name>`, per flavour.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    macro_rules! three {
        ($probe:literal) => {
            [
                include_str!(concat!(
                    "../../../probes/results/aditi/http/",
                    $probe,
                    ".txt"
                )),
                include_str!(concat!(
                    "../../../probes/results/opensim-yengine/http/",
                    $probe,
                    ".txt"
                )),
                include_str!(concat!(
                    "../../../probes/results/opensim-xengine/http/",
                    $probe,
                    ".txt"
                )),
            ]
        };
    }
    let [aditi, yengine, xengine] = match name {
        "outbound" => three!("outbound"),
        "throttle" => three!("throttle"),
        "options" => three!("options"),
        _ => three!("url"),
    };
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// A region of `flavour` as the probes met it, the scripts in prim 1.
pub(super) fn http_stage(
    flavour: EngineFlavour,
    scripts: &[(CallerId, &str, &str)],
) -> Result<Region, String> {
    let mut region = super::clocks::stage(flavour, &[], 1_793_000_000_000_000)?;
    if flavour.is_opensim() {
        region.host.simulator = Simulator {
            version: "OpenSim 0.9.2.1 Yeti Dev   36f6d16 (Unix/Mono)".to_owned(),
            grid_name: "the lost continent of hippo".to_owned(),
            grid_nick: "hippogrid".to_owned(),
            physics_engine_type: "ubODE".to_owned(),
            physics_engine_name: "ubODE 1.0-0.13.2".to_owned(),
        };
        region.host.region.name = "Default Region".to_owned();
        region.host.region.channel = "OpenSim".to_owned();
        region.host.region.corner = (256_000.0, 256_000.0);
    } else {
        region.host.simulator = Simulator {
            version: "2026-07-24.30110090421".to_owned(),
            grid_name: "Second Life Beta".to_owned(),
            grid_nick: "aditi".to_owned(),
            physics_engine_type: String::new(),
            physics_engine_name: String::new(),
        };
        region.host.region.name = "Mauve".to_owned();
        region.host.region.channel = "Luau".to_owned();
        region.host.region.corner = (255_744.0, 257_792.0);
    }
    for (id, name, source) in scripts {
        region.add_unrigged(*id, name, 1, source)?;
    }
    Ok(region)
}

/// `line` without the fields named in `vary`.
pub(super) fn without(line: &str, vary: &[&str]) -> String {
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        let end = field_end(rest);
        let (field, tail) = rest.split_at(end);
        let name = field.split('=').next().unwrap_or_default();
        if !vary.contains(&name) {
            out.push(field);
        }
        rest = tail.trim_start();
    }
    out.join(" ")
}

/// Where the field `rest` starts with ends: at the next space outside quotes.
fn field_end(rest: &str) -> usize {
    let mut quoted = false;
    let mut escaped = false;
    for (at, character) in rest.char_indices() {
        match character {
            '\\' if quoted && !escaped => {
                escaped = true;
                continue;
            }
            '"' if !escaped => quoted = !quoted,
            ' ' if !quoted => return at,
            _ => {}
        }
        escaped = false;
    }
    rest.len()
}

/// Run `region` for `seconds` and hold it to `transcript` but for `vary`, the
/// lines named in `deviations` (each a line of ours and the grid's that must
/// differ), and — with `unordered` — the order of the `rec=answers` lines.
pub(super) fn hold(
    region: &mut Region,
    flavour: EngineFlavour,
    transcript: &str,
    (seconds, vary): (u32, &[&str]),
    deviations: &[&str],
) -> Result<(), String> {
    run_for(region, flavour, seconds)?;
    let deviating = |line: &String| deviations.iter().any(|mark| line.contains(mark));
    let shape = |lines: Vec<String>| -> Vec<String> {
        let mut shaped: Vec<String> = lines
            .iter()
            .map(|line| without(line, vary))
            .filter(|line| !deviating(line))
            .collect();
        shaped.sort_by_key(|line| line.contains("rec=answers").then(|| line.clone()));
        shaped
    };
    assert_eq!(
        shape(region.lines()),
        shape(probe_lines(transcript)),
        "{flavour:?}"
    );
    let ours_debug: Vec<String> = region
        .debug_lines()
        .into_iter()
        .map(|line| line.split('\n').next().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(ours_debug, debug_notes(transcript), "{flavour:?}");
    Ok(())
}

#[test]
fn outbound_requests_go_out_and_come_back_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/http/outbound.lsl");
    for (flavour, transcript) in transcripts("outbound") {
        let mut region = http_stage(flavour, &[(A, "Probe", source)])?;
        // Second Life's answer to a HEAD is the raw response text with its
        // date, and its proxy's page for a host that does not resolve; the
        // test region answers neither, and sends nothing off it (499).
        let deviations: &[&str] = if flavour.is_opensim() {
            &["rec=response step=31"]
        } else {
            &["rec=response step=7", "rec=response step=31"]
        };
        hold(
            &mut region,
            flavour,
            transcript,
            (
                300,
                &[
                    "region",
                    "position",
                    "here",
                    "rotation",
                    "turn",
                    "velocity",
                    "owner_name",
                ],
            ),
            deviations,
        )?;
    }
    Ok(())
}

#[test]
fn the_throttle_holds_each_grids_burst() -> Result<(), String> {
    let source = include_str!("../../../probes/http/throttle.lsl");
    for (flavour, transcript) in transcripts("throttle") {
        let mut region = http_stage(flavour, &[(A, "Probe", source)])?;
        hold(&mut region, flavour, transcript, (90, &[]), &[])?;
    }
    Ok(())
}

#[test]
fn the_newer_options_answer_as_second_life_did() -> Result<(), String> {
    let probe = include_str!("../../../probes/http/options/probe.lsl");
    let space = include_str!("../../../probes/http/options/agent-space.lsl");
    let extended = include_str!("../../../probes/http/options/agent-space-extended.lsl");
    for (flavour, transcript) in transcripts("options") {
        let scripts = [
            (A, "Agent space", space),
            (B, "Agent space extended", extended),
            (C, "Probe", probe),
        ];
        if flavour.is_opensim() {
            // OpenSim knows none of the three options: the first script does
            // not compile.
            assert!(transcript.contains("compile error in \"Agent space\""));
            let refused = http_stage(flavour, &scripts[..1]).err();
            assert!(refused.is_some(), "{flavour:?}");
            continue;
        }
        // The runner saved the scripts in their order, the Probe after its
        // neighbours had sent theirs and heard back.
        let mut region = http_stage(flavour, &scripts[..2])?;
        run_for(&mut region, flavour, 2)?;
        region.add_unrigged(C, "Probe", 1, probe)?;
        // The host that does not resolve: Second Life's proxy page; the test
        // region sends nothing off it.
        hold(
            &mut region,
            flavour,
            transcript,
            (300, &["region", "channel", "version"]),
            &["rec=response step=17"],
        )?;
    }
    Ok(())
}

/// `line` with every key written `<key>`.
fn keyless(line: &str) -> String {
    let characters: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut at = 0_usize;
    while let Some(character) = characters.get(at) {
        let candidate: String = characters
            .get(at..at.saturating_add(36))
            .map(|run| run.iter().collect())
            .unwrap_or_default();
        if crate::library::dotnet::is_uuid(&candidate) {
            out.push_str("<key>");
            at = at.saturating_add(36);
        } else {
            out.push(*character);
            at = at.saturating_add(1);
        }
    }
    out
}

/// The value of the field `name` in a probe line, unquoted.
fn field(line: &str, name: &str) -> Option<String> {
    let mut rest = line;
    while !rest.is_empty() {
        let end = field_end(rest);
        let (one, tail) = rest.split_at(end);
        if let Some(value) = one.strip_prefix(&format!("{name}=")) {
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            return Some(value.replace("\\\"", "\"").replace("\\\\", "\\"));
        }
        rest = tail.trim_start();
    }
    None
}

#[test]
fn urls_take_requests_from_outside_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/http/url.lsl");
    for (flavour, transcript) in transcripts("url") {
        let mut region = http_stage(flavour, &[(A, "Probe", source)])?;
        let ticks_a_second: u64 = if flavour.is_opensim() { 55 } else { 45 };
        let mut seen = 0_usize;
        let mut url = String::new();
        for _ in 0..ticks_a_second.saturating_mul(240) {
            region.tick()?;
            let lines = region.lines();
            for line in lines.get(seen..).unwrap_or_default() {
                if line.contains("rec=granted kind=plain") {
                    let path = field(line, "path").unwrap_or_default();
                    url = format!("http://{}{}", region.host.http.authority, path);
                }
                if !line.contains("rec=waiting drive=fetch") {
                    continue;
                }
                // The runner's request: its method, path, header and body.
                let method = field(line, "method").unwrap_or_default();
                let path = field(line, "path").unwrap_or_default();
                let mut headers =
                    vec![("User-Agent".to_owned(), "sl-conformance-probe".to_owned())];
                if let Some(header) = field(line, "header")
                    && let Some((name, value)) = header.split_once(": ")
                {
                    headers.push((name.to_owned(), value.to_owned()));
                }
                if let Some(kind) = field(line, "type") {
                    headers.push(("Content-Type".to_owned(), kind));
                }
                let body = match (field(line, "body"), field(line, "repeat")) {
                    (Some(body), Some(repeat)) => body.repeat(repeat.parse().unwrap_or(0)),
                    (Some(body), None) => body,
                    _ => String::new(),
                };
                // The runner appends the path to the URL as granted.
                let to = format!("{url}{path}");
                region.fetch(&method, &to, &headers, &body)?;
            }
            seen = lines.len();
            if region.ended() {
                break;
            }
        }
        let vary = ["region", "channel", "version", "free", "length", "url"];
        let shape = |lines: Vec<String>| -> Vec<String> {
            lines
                .iter()
                .map(|line| keyless(&without(line, &vary)))
                .collect()
        };
        assert_eq!(
            shape(region.lines()),
            shape(probe_lines(transcript)),
            "{flavour:?}"
        );
        // What came back outside, where the transcript notes it.
        let statuses: Vec<i32> = region
            .host
            .http
            .fetched
            .iter()
            .map(|(_, answered)| answered.status)
            .collect();
        let noted: Vec<i32> = transcript
            .lines()
            .filter_map(|line| line.strip_prefix("# fetched status="))
            .filter_map(|rest| rest.split(' ').next()?.parse().ok())
            .collect();
        assert_eq!(statuses, noted, "{flavour:?}");
    }
    Ok(())
}
