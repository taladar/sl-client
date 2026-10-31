//! Experiences held to the grids: `experience/no-experience`,
//! `experience/sit-on-link` and `experience/unserved` run in the VM under
//! each flavour with the money test's runner, which touches and answers as
//! each probe's `waiting` records ask, checking every `PROBE` line, the
//! questions the grid's runner noted and the `DEBUG_CHANNEL` lines; OpenSim
//! compiles none of them, as neither of its engines did.
//!
//! Every probe ran a script in **no** experience, the only kind our avatars
//! can save. What a script in one does is Linden Lab's documentation, not a
//! measurement, and is held here by unit tests of the model
//! (`crate::vm::experiences`) — the acceptance's two scripts of one
//! experience, the second already permitted, and a key written by one object
//! read by another of the experience and not by one outside it.
//! `experience/powers` is not replayed: its teleports and temporary
//! attachment are `server-lsl-lib-avatar-control`'s.

use pretty_assertions::assert_eq;

use super::comms::{OTHER, canonical, noted_debug};
use super::money::{Runner, noted};
use super::strings::probe_lines;
use super::*;
use crate::value::NULL_KEY;

/// The public experience `experience/no-experience` reads, Bugsly Testing.
const BUGSLY: &str = "040979ee-0aa5-11e8-8c52-00304892026a";

/// Bugsly Testing's owner and group, as the probe sees them: keys, neither
/// `NULL_KEY` nor the owner's.
const STRANGER: &str = "1c1c1c1c-0000-4000-8000-000000000003";

/// A third script.
const C: CallerId = CallerId(3);

/// The experience the unit tests' scripts are compiled into.
const QUEST: &str = "2d2d2d2d-0000-4000-8000-000000000004";

/// The three grids' transcripts of the probe `experience/<name>`.
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let files = match name {
        "no-experience" => [
            include_str!("../../../probes/results/aditi/experience/no-experience.txt"),
            include_str!("../../../probes/results/opensim-yengine/experience/no-experience.txt"),
            include_str!("../../../probes/results/opensim-xengine/experience/no-experience.txt"),
        ],
        "sit-on-link" => [
            include_str!("../../../probes/results/aditi/experience/sit-on-link.txt"),
            include_str!("../../../probes/results/opensim-yengine/experience/sit-on-link.txt"),
            include_str!("../../../probes/results/opensim-xengine/experience/sit-on-link.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/experience/unserved.txt"),
            include_str!("../../../probes/results/opensim-yengine/experience/unserved.txt"),
            include_str!("../../../probes/results/opensim-xengine/experience/unserved.txt"),
        ],
    };
    let [aditi, yengine, xengine] = files;
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// Run `source` against each grid's transcript of `name`: on Second Life
/// every `PROBE` line (a run of `unordered` records as a set), every question
/// noted and every `DEBUG_CHANNEL` line; on OpenSim, a compile that fails, as
/// the transcript's does.
fn replayed(name: &str, source: &str, unordered: &[&str]) -> Result<(), String> {
    for (flavour, transcript) in transcripts(name) {
        let mut runner = Runner::new(flavour);
        let catalogue = &mut runner.region.host.catalogue;
        let _none = catalogue.insert(
            BUGSLY.to_owned(),
            ExperienceDetails {
                name: "Bugsly Testing".to_owned(),
                owner: STRANGER.to_owned(),
                id: BUGSLY.to_owned(),
                state: ExperienceError::None,
                group: STRANGER.to_owned(),
            },
        );
        let added = runner.add(A, "Probe", 1, source);
        if flavour.is_opensim() {
            assert!(
                transcript.contains("# compile error in \"Probe\""),
                "{flavour:?}"
            );
            assert!(added.is_err(), "{flavour:?} compiled {name}");
            continue;
        }
        added?;
        runner.run(2_000)?;
        assert_eq!(
            canonical(&runner.region.lines(), unordered),
            canonical(&probe_lines(transcript), unordered),
            "{flavour:?}"
        );
        assert_eq!(runner.notes, noted(transcript), "{flavour:?}");
        assert_eq!(
            runner.region.debug_lines(),
            noted_debug(transcript),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn no_experience_is_denied_and_answered_as_each_grid_did() -> Result<(), String> {
    replayed(
        "no-experience",
        include_str!("../../../probes/experience/no-experience.lsl"),
        &["kv-answer"],
    )
}

#[test]
fn sit_on_link_refuses_a_script_in_no_experience_first() -> Result<(), String> {
    // The sit target and who sits are the avatar-control tranche's: rigged
    // here to a region where nobody sits, the two avatars are present and
    // nobody else is.
    let source = include_str!("../../../probes/experience/sit-on-link.lsl")
        .replace(
            "llGetAgentSize(agent) != ZERO_VECTOR",
            "(agent == llGetOwner() || agent == other)",
        )
        .replace("(llGetAgentInfo(agent) & AGENT_SITTING) != 0", "FALSE")
        .replace("llAvatarOnSitTarget()", "NULL_KEY")
        .replace("llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);", "")
        .replace("llSitTarget(ZERO_VECTOR, ZERO_ROTATION);", "")
        .replace("llUnSit(seated);", "");
    replayed("sit-on-link", &source, &[])
}

#[test]
fn the_unserved_two_do_nothing_and_list_nothing() -> Result<(), String> {
    replayed(
        "unserved",
        include_str!("../../../probes/experience/unserved.lsl"),
        &[],
    )
}

/// A script that asks `agent` into its experience on touch and prints what
/// follows, and reads and writes the key-value store it is told to on
/// channel-less link messages: `"read <k>"`, `"create <k> <v>"`.
const ASKER: &str = r#"
default
{
    touch_start(integer n)
    {
        llRequestExperiencePermissions(llDetectedKey(0), "");
    }

    experience_permissions(key agent)
    {
        print("granted " + (string)agent + " " + (string)llGetPermissions()
            + " " + (string)llGetPermissionsKey()
            + " in=" + (string)llAgentInExperience(agent));
    }

    experience_permissions_denied(key agent, integer reason)
    {
        print("denied " + (string)agent + " " + (string)reason + " "
            + llGetExperienceErrorMessage(reason));
    }

    link_message(integer sender, integer num, string text, key id)
    {
        list words = llParseString2List(text, [" "], []);
        string verb = llList2String(words, 0);
        if (verb == "read")
        {
            llReadKeyValue(llList2String(words, 1));
        }
        else if (verb == "create")
        {
            llCreateKeyValue(llList2String(words, 1), llList2String(words, 2));
        }
        else if (verb == "details")
        {
            print("details " + llList2CSV(llGetExperienceDetails(NULL_KEY)));
        }
        else if (verb == "sit")
        {
            print("sit " + (string)llSitOnLink((key)llList2String(words, 1),
                (integer)llList2String(words, 2)));
        }
    }

    dataserver(key id, string data)
    {
        print("data " + data);
    }
}
"#;

/// An engine with `ASKER` in each of `scripts`, each in an object of its
/// own, and the experiences each is compiled into.
fn askers(scripts: &[(CallerId, Option<&str>)]) -> Result<(Engine, Recorder), String> {
    let mut engine = engine_as(EngineFlavour::SecondLife);
    let mut host = Recorder::default();
    for (id, experience) in scripts {
        let _none = engine.add(*id, program(ASKER)?);
        if let Some(experience) = experience {
            let _none = host.experiences.insert(
                *id,
                ScriptExperience {
                    key: (*experience).to_owned(),
                    standing: Ok(()),
                },
            );
        }
    }
    Ok((engine, host))
}

/// `agent` touches the script `id`.
fn touch(engine: &mut Engine, id: CallerId, agent: &str) -> Result<(), String> {
    let touch = Detected {
        key: agent.to_owned(),
        kind: 1,
        ..Detected::blank()
    };
    let _posted = engine
        .post_detected(id, named("touch_start")?, vec![touch])
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Tell the script `id` `text`, as a link message.
fn tell(engine: &mut Engine, id: CallerId, text: &str) -> Result<(), String> {
    let _posted = engine
        .post(
            id,
            named("link_message")?,
            vec![
                Value::Integer(1),
                Value::Integer(0),
                Value::String(text.to_owned()),
                Value::Key(NULL_KEY.to_owned()),
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn a_grant_once_for_the_experience_permits_its_next_script_unasked() -> Result<(), String> {
    let (mut engine, mut host) = askers(&[(A, Some(QUEST)), (B, Some(QUEST))])?;
    touch(&mut engine, A, OWNER)?;
    let _reports = run(&mut engine, &mut host, 5);
    assert_eq!(
        host.experience_questions
            .iter()
            .map(|(_, caller, question)| (*caller, question.clone()))
            .collect::<Vec<_>>(),
        vec![(
            A,
            ExperienceQuestion {
                agent: OWNER.to_owned(),
                experience: QUEST.to_owned(),
                mask: EXPERIENCE_PERMISSIONS,
            },
        )]
    );
    assert_eq!(host.texts(A), Vec::<&str>::new(), "nothing until answered");
    // The viewer answers Allow: the grid records it for the experience and
    // tells the script that asked.
    let _none = host
        .admissions
        .insert((OWNER.to_owned(), QUEST.to_owned()), true);
    let posted = engine
        .answer_experience(A, OWNER, Ok(()))
        .map_err(|error| error.to_string())?;
    assert!(posted.is_some());
    assert_eq!(
        engine.answer_experience(A, OWNER, Ok(())),
        Ok(None),
        "an answer nobody waits for is nothing"
    );
    touch(&mut engine, B, OWNER)?;
    let _reports = run(&mut engine, &mut host, 5);
    let granted = format!("granted {OWNER} 7220 {OWNER} in=1");
    assert_eq!(host.texts(A), vec![granted.as_str()]);
    assert_eq!(host.texts(B), vec![granted.as_str()]);
    assert_eq!(host.experience_questions.len(), 1, "B asked nobody");
    Ok(())
}

#[test]
fn a_block_a_refusal_and_a_closed_parcel_deny_with_their_reasons() -> Result<(), String> {
    let (mut engine, mut host) = askers(&[(A, Some(QUEST)), (B, Some(QUEST)), (C, None)])?;
    let _none = host
        .admissions
        .insert((OTHER.to_owned(), QUEST.to_owned()), false);
    touch(&mut engine, A, OTHER)?;
    touch(&mut engine, B, OWNER)?;
    touch(&mut engine, C, OWNER)?;
    let _reports = run(&mut engine, &mut host, 5);
    let _posted = engine
        .answer_experience(B, OWNER, Err(ExperienceError::RequestTimedOut))
        .map_err(|error| error.to_string())?;
    if let Some(experience) = host.experiences.get_mut(&A) {
        experience.standing = Err(ExperienceError::NotPermittedLand);
    }
    touch(&mut engine, A, OWNER)?;
    let _reports = run(&mut engine, &mut host, 5);
    assert_eq!(
        host.texts(A),
        vec![
            format!("denied {OTHER} 4 operation not permitted").as_str(),
            format!("denied {OWNER} 17 not allowed to run in current location").as_str(),
        ]
    );
    assert_eq!(
        host.texts(B),
        vec![format!("denied {OWNER} 18 experience permissions request timed out").as_str()]
    );
    assert_eq!(
        host.texts(C),
        vec![format!("denied {OWNER} 5 script not associated with an experience").as_str()]
    );
    Ok(())
}

#[test]
fn a_key_is_the_experiences_not_the_objects() -> Result<(), String> {
    let (mut engine, mut host) = askers(&[(A, Some(QUEST)), (B, Some(QUEST)), (C, None)])?;
    tell(&mut engine, A, "create colour blue")?;
    let _reports = run(&mut engine, &mut host, 3);
    tell(&mut engine, B, "read colour")?;
    tell(&mut engine, C, "read colour")?;
    tell(&mut engine, B, "create colour red")?;
    let _reports = run(&mut engine, &mut host, 3);
    assert_eq!(host.texts(A), vec!["data 1,blue"]);
    assert_eq!(host.texts(B), vec!["data 1,blue", "data 0,13"]);
    assert_eq!(host.texts(C), vec!["data 0,5"]);
    host.store_disabled = true;
    tell(&mut engine, A, "read colour")?;
    let _reports = run(&mut engine, &mut host, 3);
    assert_eq!(host.texts(A), vec!["data 1,blue", "data 0,12"]);
    Ok(())
}

#[test]
fn own_details_and_a_sit_are_the_experiences() -> Result<(), String> {
    let (mut engine, mut host) = askers(&[(A, Some(QUEST))])?;
    let _none = host.catalogue.insert(
        QUEST.to_owned(),
        ExperienceDetails {
            name: "Quest".to_owned(),
            owner: OWNER.to_owned(),
            id: QUEST.to_owned(),
            state: ExperienceError::None,
            group: NULL_KEY.to_owned(),
        },
    );
    let _none = host
        .admissions
        .insert((OWNER.to_owned(), QUEST.to_owned()), true);
    let _none = host.away.insert(OTHER.to_owned());
    tell(&mut engine, A, "details")?;
    tell(&mut engine, A, &format!("sit {OWNER} 0"))?;
    tell(&mut engine, A, &format!("sit {OWNER} -1"))?;
    tell(&mut engine, A, &format!("sit {OWNER} 2"))?;
    tell(&mut engine, A, &format!("sit {OTHER} 0"))?;
    let _none = host
        .admissions
        .insert((OTHER.to_owned(), QUEST.to_owned()), true);
    let _reports = run(&mut engine, &mut host, 3);
    tell(&mut engine, A, &format!("sit {OTHER} 0"))?;
    let _reports = run(&mut engine, &mut host, 3);
    assert_eq!(
        host.texts(A),
        vec![
            format!("details Quest, {OWNER}, {QUEST}, 0, no error, {NULL_KEY}").as_str(),
            "sit 1",
            "sit -5",
            "sit -5",
            "sit -4",
            "sit -4",
        ]
    );
    assert_eq!(host.sat, vec![(A, OWNER.to_owned(), 0)]);
    Ok(())
}

#[test]
fn a_reset_loses_the_question_and_the_grant() -> Result<(), String> {
    let (mut engine, mut host) = askers(&[(A, Some(QUEST))])?;
    touch(&mut engine, A, OWNER)?;
    let _reports = run(&mut engine, &mut host, 3);
    if let Some(instance) = engine.instance_mut(A) {
        instance.reset();
    }
    assert_eq!(engine.answer_experience(A, OWNER, Ok(())), Ok(None));
    assert_eq!(
        engine
            .instance(A)
            .map(|instance| instance.permissions().mask()),
        Some(0)
    );
    Ok(())
}
