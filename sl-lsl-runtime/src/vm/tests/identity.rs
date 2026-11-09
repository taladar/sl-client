//! A list as each grid holds it, held to the grids: `vm/list-identity` run
//! in the VM under each flavour, every line it prints set against the line
//! the grid printed. Second Life's list is a value; OpenSim's is a shared
//! object that some of its library and XEngine's `+` change in place, so a
//! change shows through every variable holding it (book `gridspec/lsl.md`,
//! § Strings, lists, maths and rotations).
//!
//! `llGetOwner`, which the probe hands its dialogs, is the host's owner
//! written in, as [`offline`] writes in the other things a probe reads of the
//! world.

use pretty_assertions::assert_eq;

use super::strings::{hold, offline, probe_lines, replay};
use super::*;

#[test]
fn list_identity_prints_what_each_grid_printed() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/vm/list-identity.lsl"), "")
        .replace("llGetOwner()", &format!("(key)\"{OWNER}\""));
    for (flavour, transcript) in [
        (
            EngineFlavour::SecondLife,
            include_str!("../../../probes/results/aditi/vm/list-identity.txt"),
        ),
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/vm/list-identity.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/vm/list-identity.txt"),
        ),
    ] {
        // Three dialogs of a second each, at ten ticks a second.
        let (_, ours) = replay(&source, flavour, 400)?;
        hold(&ours, &probe_lines(transcript), &[]);
    }
    Ok(())
}

/// `persistence/lists` without what this VM does not run yet (the prim's
/// description, `llGetAgentSize`), and with its take before its restart,
/// which the probe runs the other way round only because an object rezzed
/// just before a restart did not come back on OpenSim. On link message 100
/// two globals take one list and a third an equal one; on 101 it pads the
/// first and prints the three; on 102 two more globals take one list, a local
/// takes a third global's list, and the handler sleeps eight seconds before
/// padding the local and printing the global; a region start pads the first
/// of the pair and prints both.
const PERSISTED: &str = r#"list A;
list B;
list C;
list D;
list E;
list F;

default
{
    link_message(integer sender, integer number, string text, key id)
    {
        if (number == 100)
        {
            A = [1];
            B = A;
            C = [1];
        }
        else if (number == 101)
        {
            llSetPayPrice(PAY_DEFAULT, A);
            print("rerez A=" + llList2CSV(A) + " B=" + llList2CSV(B) + " C=" + llList2CSV(C));
        }
        else if (number == 102)
        {
            D = [2];
            E = D;
            F = [3];
            list held = F;
            llSleep(8.0);
            llSetPayPrice(PAY_DEFAULT, held);
            print("woke F=" + llList2CSV(F));
        }
    }

    changed(integer change)
    {
        if (change & CHANGED_REGION_START)
        {
            llSetPayPrice(PAY_DEFAULT, D);
            print("after D=" + llList2CSV(D) + " E=" + llList2CSV(E));
        }
    }
}
"#;

/// A link message to the script from its own prim.
fn message(engine: &mut Engine, number: i32) -> Result<(), String> {
    let _posted = link(engine, A, number, "")?;
    Ok(())
}

#[test]
fn a_take_and_a_restart_keep_or_part_the_lists_as_each_grid_saves_them() -> Result<(), String> {
    const PADDED: &str = "1, -1, -1, -1, -1";
    for (flavour, rerez, woke, after) in [
        (
            EngineFlavour::SecondLife,
            "rerez A=1 B=1 C=1".to_owned(),
            Some("woke F=3".to_owned()),
            "after D=2 E=2".to_owned(),
        ),
        (
            EngineFlavour::YEngine,
            format!("rerez A={PADDED} B={PADDED} C=1"),
            Some("woke F=3, -1, -1, -1, -1".to_owned()),
            "after D=2, -1, -1, -1, -1 E=2, -1, -1, -1, -1".to_owned(),
        ),
        (
            // XEngine resets a script on a rez, and saves each global apart
            // at a shutdown, dropping the handler it was in.
            EngineFlavour::XEngine,
            "rerez A=-1, -1, -1, -1, -1 B= C=".to_owned(),
            None,
            "after D=2, -1, -1, -1, -1 E=2".to_owned(),
        ),
    ] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let program = program_for(PERSISTED, flavour)?;
        let _none = engine.add(A, Arc::clone(&program));
        message(&mut engine, 100)?;
        let _reports = run(&mut engine, &mut host, 5);
        let taken = engine.take(A).ok_or("nothing to take")?;
        let taken = SavedScript::decode(&taken.encode()).map_err(|error| error.to_string())?;
        let _posted = engine
            .rez_saved(A, Arc::clone(&program), &taken, Duration::from_secs(5), 0)
            .map_err(|error| error.to_string())?;
        let _reports = run(&mut engine, &mut host, 5);
        message(&mut engine, 101)?;
        let _reports = run(&mut engine, &mut host, 5);
        message(&mut engine, 102)?;
        let _reports = run(&mut engine, &mut host, 20);
        let saved = engine
            .shut_down()
            .into_iter()
            .map(|(id, saved)| {
                SavedScript::decode(&saved.encode())
                    .map(|saved| (id, Arc::clone(&program), saved))
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        engine
            .start_up(saved, Duration::from_secs(20))
            .map_err(|(id, error)| format!("{id:?}: {error}"))?;
        let _reports = run(&mut engine, &mut host, 100);
        let mut expected = vec![rerez];
        expected.extend(woke);
        expected.push(after);
        assert_eq!(host.texts(A), expected, "{flavour:?}");
    }
    Ok(())
}

/// `prim/owner-key` under each flavour, with the region's agents written in
/// as the owner and one stranger: `llGetOwnerKey` answers alike on every grid.
#[test]
fn owner_key_prints_what_each_grid_printed() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/prim/owner-key.lsl"), "")
        .replace(
            "llGetAgentList(AGENT_LIST_REGION, [])",
            &format!("[(key)\"{OWNER}\", (key)\"0a0a0a0a-0000-4000-8000-0000000000aa\"]"),
        )
        .replace("llGetOwner()", &format!("(key)\"{OWNER}\""));
    for (flavour, transcript) in [
        (
            EngineFlavour::SecondLife,
            include_str!("../../../probes/results/aditi/prim/owner-key.txt"),
        ),
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/prim/owner-key.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/prim/owner-key.txt"),
        ),
    ] {
        let (_, ours) = replay(&source, flavour, 50)?;
        hold(&ours, &probe_lines(transcript), &[]);
    }
    Ok(())
}

/// `vm/eval-order` under each flavour: the order each engine evaluates an
/// expression's parts in. Second Life evaluates a binary operator's operands
/// right to left and a list's elements left to right; YEngine the same but a
/// list literal's elements right to left; XEngine, C#, everything left to
/// right.
#[test]
fn expressions_evaluate_in_each_engines_order() -> Result<(), String> {
    let source = offline(include_str!("../../../probes/vm/eval-order.lsl"), "");
    for (flavour, transcript, deviations) in [
        (
            EngineFlavour::SecondLife,
            include_str!("../../../probes/results/aditi/vm/eval-order.txt"),
            &[][..],
        ),
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/vm/eval-order.txt"),
            &[][..],
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/vm/eval-order.txt"),
            &XENGINE_ORDER[..],
        ),
    ] {
        let (_, ours) = replay(&source, flavour, 50)?;
        hold(&ours, &probe_lines(transcript), deviations);
    }
    Ok(())
}

/// The `vm/eval-order` cases XEngine evaluates left to right where the
/// runtime keeps YEngine's order: the compiler has one OpenSim dialect, and
/// it is YEngine's, the engine a fake grid that imitates OpenSim runs. The
/// values agree; only the order of the side effects differs.
const XENGINE_ORDER: [(&str, &str); 12] = [
    ("order", "minus"),
    ("order", "chain"),
    ("order", "products"),
    ("order", "less"),
    ("order", "and"),
    ("order", "or"),
    ("order", "list"),
    ("order", "list-plus"),
    ("order", "library-list-arg"),
    ("order", "concat"),
    ("order", "compound"),
    ("order", "index-inline"),
];

/// `events/state-self` under each flavour: a `state` in a global function
/// lets the caller carry on on Second Life, and ends the whole handler on
/// both OpenSim engines, which throw out of it.
#[test]
fn a_state_in_a_function_ends_the_handler_on_opensim_alone() -> Result<(), String> {
    let source = offline(
        include_str!("../../../probes/events/state-self.lsl"),
        "Probe",
    );
    for (flavour, transcript) in [
        (
            EngineFlavour::SecondLife,
            include_str!("../../../probes/results/aditi/events/state-self.txt"),
        ),
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/events/state-self.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/events/state-self.txt"),
        ),
    ] {
        let (_, ours) = replay(&source, flavour, 50)?;
        hold(&ours, &probe_lines(transcript), &[]);
    }
    Ok(())
}
