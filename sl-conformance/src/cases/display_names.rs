//! A census of display names: what `GetDisplayNames` answers for two avatars
//! and an id nobody has, what our client makes of the answer, and whether the
//! grid offers `SetDisplayName`.
//!
//! A *display name* is the name an avatar chooses, layered over the legacy
//! `First Last` it logged in with. Both live grids serve `GetDisplayNames`;
//! only Second Life lets an avatar set one. **The census never sets a name**:
//! Second Life lets an avatar change it once a week, and the test avatars are
//! not ours to rename on a whim. It records:
//!
//! - the raw answer for A, B and an unknown id: its status and keys, how many
//!   agents it names, and what it does with the unknown id;
//! - the shape of one record, and each avatar's record as Second Life's rules
//!   describe it — the user name as the SLID (a Resident's first name, or
//!   `first.last`, in lower case) or as the legacy name, the display name as
//!   Second Life's default (a Resident's first name, anyone else's legacy
//!   name) or as the legacy name — so the measurement is the grid's, not
//!   whatever the test accounts happen to be called;
//! - what [`Command::RequestDisplayNames`] brings the client;
//! - whether `SetDisplayName` is granted.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/profiles.md`; every run is held to it.
//!
//! `2av`, `[both, fake]`. The fake grid answers as each flavour's people
//! service (`ImitatedGrid::display_name_style`).

use std::time::{Duration, Instant};

use sl_client_tokio::{AgentKey, Command, Event, Uuid};
use sl_wire::{Llsd, display_names_query, parse_llsd_xml};

use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{hold_to_measured, http_client, shape_of};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT};

/// The capability that reads display names.
const GET_DISPLAY_NAMES: &str = "GetDisplayNames";

/// The capability that sets one's display name.
const SET_DISPLAY_NAME: &str = "SetDisplayName";

/// What a metric reads where the grid has no such capability.
const NO_CAPABILITY: &str = "no capability";

/// How long the capability map may take to arrive after a login.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/profiles.md (display-names, 2026-10-15)";

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer, by the metric the census records it
/// under: every run is held to it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "client_reads",
        each(
            Answer::Text("an unknown id missing, avatar A named, avatar B named"),
            Answer::Text("avatar A named, avatar B named"),
        ),
    ),
    (
        "display_names_answer",
        each(
            Answer::Text("200, keys agents,bad_ids, 2 agent(s), bad ids [an unknown id]"),
            Answer::Text("200, keys agents, 2 agent(s), bad ids absent"),
        ),
    ),
    (
        "display_names_other",
        each(
            Answer::Text(
                "username the SLID, display name Second Life's default, legacy name as logged in, default true, expires absent, next update a date",
            ),
            Answer::Text(
                "username the legacy name, display name Second Life's default, legacy name as logged in, default true, expires a date, next update a date",
            ),
        ),
    ),
    (
        "display_names_own",
        each(
            Answer::Text(
                "username the SLID, display name Second Life's default, legacy name as logged in, default true, expires absent, next update a date",
            ),
            Answer::Text(
                "username the legacy name, display name Second Life's default, legacy name as logged in, default true, expires a date, next update a date",
            ),
        ),
    ),
    (
        "display_names_shape",
        each(
            Answer::Text(
                "{display_name:string,display_name_next_update:date,id:uuid,is_display_name_default:boolean,legacy_first_name:string,legacy_last_name:string,username:string}",
            ),
            Answer::Text(
                "{display_name:string,display_name_expires:date,display_name_next_update:date,id:uuid,is_display_name_default:boolean,legacy_first_name:string,legacy_last_name:string,username:string}",
            ),
        ),
    ),
    (
        "set_display_name",
        each(Answer::Text("granted"), Answer::Text("no capability")),
    ),
];

/// A census of display names: the raw `GetDisplayNames` answer, what the
/// client makes of it, and whether `SetDisplayName` is granted.
#[derive(Debug)]
pub struct DisplayNames;

impl GridTest for DisplayNames {
    fn name(&self) -> &'static str {
        "display-names"
    }

    fn description(&self) -> &'static str {
        "What GetDisplayNames answers for two avatars and an unknown id, and whether SetDisplayName is granted"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The ids a metric names by what they are.
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// The id nobody has.
    stranger: Uuid,
}

impl Scene {
    /// What `id` is the id of.
    fn id_text(&self, id: Uuid) -> &'static str {
        if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else if id == self.stranger {
            "an unknown id"
        } else {
            "another id"
        }
    }
}

/// The census.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "display-names needs a secondary avatar".to_owned(),
        ));
    };
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let scene = Scene {
        a: a_id.uuid(),
        b: b_id.uuid(),
        stranger: Uuid::new_v4(),
    };
    let ids = [scene.a, scene.b, scene.stranger];
    metrics.set(
        "set_display_name",
        if cap(a, SET_DISPLAY_NAME).await.is_some() {
            "granted"
        } else {
            NO_CAPABILITY
        },
    );

    match cap(a, GET_DISPLAY_NAMES).await {
        Some(url) => {
            let response = http_client()?
                .get(format!("{url}{}", display_names_query(&ids)))
                .header("Accept", "application/llsd+xml")
                .send()
                .await
                .map_err(|error| {
                    TestFailure::Assertion(format!("a capability request failed: {error}"))
                })?;
            let status = response.status().as_u16();
            let llsd = parse_llsd_xml(&response.text().await.unwrap_or_default()).ok();
            record_answer(metrics, &scene, status, llsd.as_ref(), (a, b));
        }
        None => {
            for key in [
                "display_names_answer",
                "display_names_shape",
                "display_names_own",
                "display_names_other",
            ] {
                metrics.set(key, NO_CAPABILITY);
            }
        }
    }

    a.send(Command::RequestDisplayNames(
        ids.iter().copied().map(AgentKey::from).collect(),
    ))
    .await?;
    let read = a
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::DisplayNames(names) => Some(names.clone()),
            _ => None,
        })
        .await;
    metrics.set(
        "client_reads",
        match read {
            Ok(names) => {
                let mut read: Vec<String> = names
                    .iter()
                    .map(|name| {
                        format!(
                            "{} {}",
                            scene.id_text(name.id.uuid()),
                            if name.missing { "missing" } else { "named" }
                        )
                    })
                    .collect();
                read.sort();
                read.join(", ")
            }
            Err(TestFailure::Timeout(_)) => "nothing".to_owned(),
            Err(other) => return Err(other),
        },
    );
    Ok(())
}

/// Records the raw `GetDisplayNames` answer.
fn record_answer(
    metrics: &mut Metrics,
    scene: &Scene,
    status: u16,
    llsd: Option<&Llsd>,
    (a, b): (&Session, &Session),
) {
    let agents: Vec<Llsd> = match llsd.and_then(|llsd| llsd.get("agents")) {
        Some(Llsd::Array(agents)) => agents.clone(),
        _ => Vec::new(),
    };
    let bad_ids = llsd.and_then(|llsd| llsd.get("bad_ids")).map_or_else(
        || "absent".to_owned(),
        |ids| match ids {
            Llsd::Array(ids) => {
                let ids: Vec<&str> = ids
                    .iter()
                    .map(|id| match id {
                        Llsd::Uuid(id) => scene.id_text(*id),
                        _ => "something else",
                    })
                    .collect();
                format!("[{}]", ids.join(", "))
            }
            other => shape_of(other),
        },
    );
    metrics.set(
        "display_names_answer",
        format!(
            "{status}, keys {}, {} agent(s), bad ids {bad_ids}",
            llsd.map_or_else(|| "none".to_owned(), keys_text),
            agents.len(),
        ),
    );
    metrics.set(
        "display_names_shape",
        agents
            .first()
            .map_or_else(|| "no agent".to_owned(), shape_of),
    );
    for (key, who, session) in [
        ("display_names_own", scene.a, a),
        ("display_names_other", scene.b, b),
    ] {
        let entry = agents
            .iter()
            .find(|agent| matches!(agent.get("id"), Some(Llsd::Uuid(id)) if *id == who));
        metrics.set(
            key,
            entry.map_or_else(
                || "absent".to_owned(),
                |entry| display_name_text(entry, &session.avatar_legacy_name()),
            ),
        );
    }
}

/// A capability's URL on `session`'s region, waited for as long as the
/// capability map may take; `None` where the region does not grant it.
async fn cap(session: &Session, name: &str) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Some(url) = session.cap(name) {
            return Some(url);
        }
        if session.capability_names().is_empty() && started.elapsed() < CAP_WAIT {
            tokio::time::sleep(Duration::from_millis(250)).await;
        } else {
            return None;
        }
    }
}

/// The keys of an LLSD map, `,`-joined and in order.
fn keys_text(value: &Llsd) -> String {
    match value {
        Llsd::Map(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            keys.join(",")
        }
        _ => "-".to_owned(),
    }
}

/// The SLID Second Life derives from a legacy name: a Resident's first name,
/// anyone else's `first.last`, in lower case.
fn slid(legacy: &str) -> String {
    match legacy.split_once(' ') {
        Some((first, last)) if last.eq_ignore_ascii_case("Resident") => first.to_lowercase(),
        Some((first, last)) => format!("{first}.{last}").to_lowercase(),
        None => legacy.to_lowercase(),
    }
}

/// The display name Second Life gives an avatar that never chose one: a
/// Resident's first name, anyone else's legacy name.
fn default_display_name(legacy: &str) -> String {
    match legacy.split_once(' ') {
        Some((first, last)) if last.eq_ignore_ascii_case("Resident") => first.to_owned(),
        _ => legacy.to_owned(),
    }
}

/// One `GetDisplayNames` record as a metric states it, against `legacy`, the
/// name the avatar logged in with: each name by the rule it follows, never by
/// what it is.
fn display_name_text(entry: &Llsd, legacy: &str) -> String {
    let text = |key: &str| match entry.get(key) {
        Some(Llsd::String(text)) => text.clone(),
        _ => String::new(),
    };
    let username = text("username");
    let display = text("display_name");
    let username_form = if username == slid(legacy) {
        "the SLID"
    } else if username == legacy {
        "the legacy name"
    } else if username.is_empty() {
        "empty"
    } else {
        "another name"
    };
    let display_form = if display == default_display_name(legacy) {
        "Second Life's default"
    } else if display == legacy {
        "the legacy name"
    } else if display.is_empty() {
        "empty"
    } else {
        "a name of its own"
    };
    let legacy_parts = format!("{} {}", text("legacy_first_name"), text("legacy_last_name"));
    format!(
        "username {username_form}, display name {display_form}, legacy name {}, default {}, \
         expires {}, next update {}",
        if legacy_parts == legacy {
            "as logged in"
        } else {
            "otherwise"
        },
        entry.get("is_display_name_default").map_or_else(
            || "absent".to_owned(),
            |value| match value {
                Llsd::Boolean(flag) => flag.to_string(),
                other => shape_of(other),
            }
        ),
        entry
            .get("display_name_expires")
            .map_or("absent", |_date| "a date"),
        entry
            .get("display_name_next_update")
            .map_or("absent", |_date| "a date"),
    )
}

#[cfg(test)]
mod tests {
    use super::{default_display_name, slid};
    use pretty_assertions::assert_eq;

    /// Second Life's rules for the names an avatar has before it chooses one.
    #[test]
    fn second_life_derives_the_names_from_the_legacy_name() {
        assert_eq!(slid("Example Resident"), "example");
        assert_eq!(slid("Avatar Tester"), "avatar.tester");
        assert_eq!(default_display_name("Example Resident"), "Example");
        assert_eq!(default_display_name("Avatar Tester"), "Avatar Tester");
    }
}
