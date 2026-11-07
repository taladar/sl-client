//! The link-number family and a script's own links (`server-world-link-sets`):
//! names by link number, the `LINK_*` constants resolved in one place, and
//! `llCreateLink`, `llBreakLink` and `llBreakAllLinks` behind
//! `PERMISSION_CHANGE_LINKS` — each held to the rows of
//! `book/src/gridspec/lsl.md` the `prim/identity` and `prim/link` probes
//! measured.

use pretty_assertions::assert_eq;

use super::*;
use crate::value::NULL_KEY;
use crate::vm::permissions::PERMISSION_CHANGE_LINKS;

/// The three flavours.
const FLAVOURS: [EngineFlavour; 3] = [
    EngineFlavour::SecondLife,
    EngineFlavour::YEngine,
    EngineFlavour::XEngine,
];

/// The root's key in [`three`].
const ROOT: &str = "10000000-0000-4000-8000-000000000001";
/// The second prim's key in [`three`].
const SECOND: &str = "10000000-0000-4000-8000-000000000002";
/// The third prim's key in [`three`].
const THIRD: &str = "10000000-0000-4000-8000-000000000003";
/// A prim of another object the owner may link.
const OTHER_PRIM: &str = "20000000-0000-4000-8000-000000000001";

/// A linkset of three prims, `Root`, `Second` and `Third`, the calling
/// script (`A`) in the prim numbered `this`.
fn three(this: i32) -> Links {
    let prims = [(ROOT, "Root"), (SECOND, "Second"), (THIRD, "Third")];
    Links {
        this,
        prims: prims
            .into_iter()
            .zip(1..)
            .map(|((key, name), link)| LinkedPrim {
                link,
                key: key.to_owned(),
                name: name.to_owned(),
                scripts: if link == this { vec![A] } else { Vec::new() },
            })
            .collect(),
        sitters: Vec::new(),
    }
}

/// Runs `source` in a fresh `flavour` engine against `host` for `ticks`,
/// returning the engine for more.
fn started(
    source: &str,
    flavour: EngineFlavour,
    host: &mut Recorder,
    ticks: u64,
) -> Result<Engine, String> {
    let mut engine = engine_as(flavour);
    let _none = engine.add(A, program_for(source, flavour)?);
    let _reports = run(&mut engine, host, ticks);
    Ok(engine)
}

/// What the scripts said on `DEBUG_CHANNEL`.
fn debug_lines(host: &Recorder) -> Vec<String> {
    host.said
        .iter()
        .filter(|(_, _, speech)| speech.channel == crate::vm::DEBUG_CHANNEL)
        .map(|(_, _, speech)| speech.text.clone())
        .collect()
}

/// `llGetLinkName` and `llGetObjectName` on a prim on its own
/// (`prim/identity`): link 0 is the prim and link 1 nothing, `NULL_KEY` as
/// text, on both grids.
#[test]
fn a_lone_prims_names_are_link_zero_alone() -> Result<(), String> {
    let source = "default { state_entry() {
        print(llGetObjectName() + \"|\" + llGetLinkName(0) + \"|\" + llGetLinkName(1)
            + \"|\" + llGetLinkName(LINK_THIS) + \"|\" + llGetLinkName(LINK_ROOT)
            + \"|\" + llGetLinkName(2));
    } }";
    for flavour in FLAVOURS {
        let mut host = Recorder::default();
        let _engine = started(source, flavour, &mut host, 2)?;
        assert_eq!(
            host.texts(A),
            vec![format!(
                "Object|Object|{NULL_KEY}|Object|{NULL_KEY}|{NULL_KEY}"
            )],
            "{flavour:?}"
        );
    }
    Ok(())
}

/// In a linkset the numbers name the prims in link order and the sitters
/// after them; 0 names nothing, and `llGetObjectName` is the caller's own
/// prim's, not the root's.
#[test]
fn a_linksets_names_follow_its_link_numbers() -> Result<(), String> {
    let source = "default { state_entry() {
        print(llGetObjectName() + \"|\" + llGetLinkName(0) + \"|\" + llGetLinkName(1)
            + \"|\" + llGetLinkName(2) + \"|\" + llGetLinkName(3) + \"|\" + llGetLinkName(4)
            + \"|\" + llGetLinkName(5) + \"|\" + (string)llGetNumberOfPrims()
            + \"|\" + (string)llGetLinkNumber());
    } }";
    for flavour in FLAVOURS {
        let mut linked = three(2);
        linked.sitters.push(LinkedAvatar {
            key: OTHER_PRIM.to_owned(),
            name: "Somebody Resident".to_owned(),
        });
        let mut host = Recorder {
            linked: Some(linked),
            ..Recorder::default()
        };
        let _engine = started(source, flavour, &mut host, 2)?;
        assert_eq!(
            host.texts(A),
            vec![format!(
                "Second|{NULL_KEY}|Root|Second|Third|Somebody Resident|{NULL_KEY}|4|2"
            )],
            "{flavour:?}"
        );
    }
    Ok(())
}

/// The `LINK_*` constants name prims in one place: from the root of three,
/// `LINK_ALL_OTHERS` names exactly the other two, `LINK_ALL_CHILDREN` the
/// same, `LINK_SET` all three and `LINK_THIS` the root.
#[test]
fn link_constants_name_the_prims_of_a_set_of_three() {
    fn keys(prims: Vec<&LinkedPrim>) -> Vec<&str> {
        prims.into_iter().map(|prim| prim.key.as_str()).collect()
    }
    let from_root = three(1);
    for flavour in FLAVOURS {
        assert_eq!(
            keys(from_root.named(-2, flavour)),
            vec![SECOND, THIRD],
            "{flavour:?}"
        );
        assert_eq!(keys(from_root.named(-3, flavour)), vec![SECOND, THIRD]);
        assert_eq!(
            keys(from_root.named(-1, flavour)),
            vec![ROOT, SECOND, THIRD]
        );
        assert_eq!(keys(from_root.named(-4, flavour)), vec![ROOT]);
        assert_eq!(keys(from_root.named(1, flavour)), vec![ROOT]);
        assert_eq!(keys(from_root.named(0, flavour)), Vec::<&str>::new());
        assert_eq!(keys(from_root.named(4, flavour)), Vec::<&str>::new());
    }
    let from_child = three(3);
    assert_eq!(
        keys(from_child.named(-2, EngineFlavour::SecondLife)),
        vec![ROOT, SECOND]
    );
}

/// `llCreateLink` without `PERMISSION_CHANGE_LINKS` is refused in each
/// grid's words and links nothing (`prim/link`); OpenSim first drops a target
/// that is no key without a word.
#[test]
fn a_link_without_the_permission_is_refused_in_each_grids_words() -> Result<(), String> {
    let source = format!(
        "default {{ state_entry() {{
            llCreateLink(\"{OTHER_PRIM}\", TRUE);
            llCreateLink(\"not a key\", TRUE);
        }} }}"
    );
    for (flavour, refused) in [
        (
            EngineFlavour::SecondLife,
            vec![
                "Script trying to link but PERMISSION_CHANGE_LINKS permission not set!".to_owned();
                2
            ],
        ),
        (
            EngineFlavour::XEngine,
            vec!["llCreateLink: PERMISSION_CHANGE_LINKS permission not set".to_owned()],
        ),
        (
            EngineFlavour::YEngine,
            vec!["llCreateLink: PERMISSION_CHANGE_LINKS permission not set".to_owned()],
        ),
    ] {
        let mut host = Recorder {
            linkable: [OTHER_PRIM.to_owned()].into_iter().collect(),
            ..Recorder::default()
        };
        let _engine = started(&source, flavour, &mut host, 2)?;
        assert_eq!(debug_lines(&host), refused, "{flavour:?}");
        assert_eq!(host.created_links, Vec::new(), "{flavour:?}");
    }
    Ok(())
}

/// Granted, `llCreateLink` hands a target the owner may link to the host,
/// root or child as asked; a target that is not one is refused on Second Life
/// with `target is invalid` and dropped silently on OpenSim (`prim/link`:
/// `NULL_KEY`, a key nothing has and the owner's own).
#[test]
fn a_granted_link_names_its_target_and_refuses_an_invalid_one() -> Result<(), String> {
    let source = format!(
        "default {{
            state_entry() {{ llRequestPermissions(llGetOwner(), PERMISSION_CHANGE_LINKS); }}
            run_time_permissions(integer granted) {{
                llCreateLink(NULL_KEY, TRUE);
                llCreateLink(llGetOwner(), TRUE);
                llCreateLink(\"{OTHER_PRIM}\", TRUE);
                llCreateLink(\"{OTHER_PRIM}\", FALSE);
            }}
        }}"
    );
    for flavour in FLAVOURS {
        let mut host = Recorder {
            linkable: [OTHER_PRIM.to_owned()].into_iter().collect(),
            ..Recorder::default()
        };
        let mut engine = started(&source, flavour, &mut host, 1)?;
        let _posted = engine
            .answer(A, OWNER, PERMISSION_CHANGE_LINKS)
            .map_err(|error| error.to_string())?;
        // Each link sleeps a second: four of them, and a tick to spare.
        let _reports = run(&mut engine, &mut host, 45);
        let invalid = if flavour.is_opensim() {
            Vec::new()
        } else {
            vec!["Script trying to link but target is invalid.".to_owned(); 2]
        };
        assert_eq!(debug_lines(&host), invalid, "{flavour:?}");
        assert_eq!(
            host.created_links,
            vec![
                (A, OTHER_PRIM.to_owned(), true),
                (A, OTHER_PRIM.to_owned(), false)
            ],
            "{flavour:?}"
        );
    }
    Ok(())
}

/// `llBreakLink` hands the host the prim its number names — `LINK_THIS` the
/// caller's own — and nothing for 0, a number past the last prim, another
/// constant, or a prim on its own; `llBreakAllLinks` hands over the set, and
/// nothing for a prim on its own (`prim/link`: `llBreakLink(3)` of a set of
/// two is nothing).
#[test]
fn breaking_a_link_names_the_prim_its_number_names() -> Result<(), String> {
    let source = "default {
        state_entry() { llRequestPermissions(llGetOwner(), PERMISSION_CHANGE_LINKS); }
        run_time_permissions(integer granted) {
            llBreakLink(3); llBreakLink(0); llBreakLink(4); llBreakLink(LINK_SET);
            llBreakLink(LINK_THIS); llBreakLink(1); llBreakAllLinks();
        }
    }";
    for flavour in FLAVOURS {
        let mut host = Recorder {
            linked: Some(three(2)),
            ..Recorder::default()
        };
        let mut engine = started(source, flavour, &mut host, 1)?;
        let _posted = engine
            .answer(A, OWNER, PERMISSION_CHANGE_LINKS)
            .map_err(|error| error.to_string())?;
        let _reports = run(&mut engine, &mut host, 2);
        assert_eq!(
            host.broken_links,
            vec![
                (A, THIRD.to_owned()),
                (A, SECOND.to_owned()),
                (A, ROOT.to_owned())
            ],
            "{flavour:?}"
        );
        assert_eq!(host.broken_all, vec![A], "{flavour:?}");

        let mut lone = Recorder::default();
        let mut engine = started(source, flavour, &mut lone, 1)?;
        let _posted = engine
            .answer(A, OWNER, PERMISSION_CHANGE_LINKS)
            .map_err(|error| error.to_string())?;
        let _reports = run(&mut engine, &mut lone, 2);
        assert_eq!(lone.broken_links, Vec::new(), "{flavour:?}");
        assert_eq!(lone.broken_all, Vec::new(), "{flavour:?}");
    }
    Ok(())
}

/// A delink without `PERMISSION_CHANGE_LINKS` is refused in each grid's
/// words and takes nothing out.
#[test]
fn a_delink_without_the_permission_is_refused() -> Result<(), String> {
    let source = "default { state_entry() { llBreakLink(2); llBreakAllLinks(); } }";
    for (flavour, refused) in [
        (
            EngineFlavour::SecondLife,
            vec!["Script trying to break links but PERMISSION_CHANGE_LINKS permission not set!"; 2],
        ),
        (
            EngineFlavour::XEngine,
            vec![
                "llBreakLink: PERMISSION_CHANGE_LINKS permission not set",
                "llBreakAllLinks: Script trying to link but PERMISSION_CHANGE_LINKS permission \
                 not set!",
            ],
        ),
    ] {
        let mut host = Recorder {
            linked: Some(three(1)),
            ..Recorder::default()
        };
        // OpenSim's first refusal costs the script a second.
        let _engine = started(source, flavour, &mut host, 12)?;
        assert_eq!(debug_lines(&host), refused, "{flavour:?}");
        assert_eq!(host.broken_links, Vec::new());
        assert_eq!(host.broken_all, Vec::new());
    }
    Ok(())
}
