//! OpenSim's NPC and avatar functions (`server-lsl-lib-ossl-avatars`) run in
//! the VM under YEngine against the test host, with every OSSL function
//! allowed: what each one asks of the host, what it reads back, and the
//! errors it throws, as `OSSL_Api.cs` and `NPCModule.cs` state them.

use pretty_assertions::assert_eq;

use super::comms::{OTHER, prim_key};
use super::*;
use sl_types::lsl::Vector;

use crate::library::ThreatLevel;
use crate::value::NULL_KEY;
use crate::vm::avatars::Destination;
use crate::vm::npcs::{Appearance, DEFAULT_VISUAL_PARAMS, Npc, NpcAction, Vitals};
use crate::vm::ossl::Settings;

/// The flavour every test here runs under.
const FLAVOUR: EngineFlavour = EngineFlavour::YEngine;

/// An NPC the tests' owner did not create.
const THEIRS: &str = "0d0d0d0d-0000-4000-8000-000000000001";

/// An NPC nobody owns.
const ANYBODY: &str = "0d0d0d0d-0000-4000-8000-000000000002";

/// An NPC standing at `(x, 10, 20)`, owned by `owner`.
fn npc(key: &str, name: &str, owner: Option<&str>, x: f32) -> Npc {
    Npc {
        key: key.to_owned(),
        name: name.to_owned(),
        owner: owner.map(str::to_owned),
        position: Vector {
            x,
            y: 10.0,
            z: 20.0,
        },
        rotation: crate::value::ZERO_ROTATION,
    }
}

/// The default shape, `male` set to `male`.
fn shape(male: u8) -> Appearance {
    let mut visual_params = DEFAULT_VISUAL_PARAMS.to_vec();
    if let Some(byte) = visual_params.get_mut(31) {
        *byte = male;
    }
    Appearance {
        serial: 1,
        height: 1.8,
        hover: 0.0,
        textures: Vec::new(),
        extra_bakes: None,
        visual_params,
        attachments: Vec::new(),
    }
}

/// What `llGetObjectDetails` reads of the avatar `agent` standing at
/// `position`: the script's own prim's, made an avatar's.
fn avatar_details(
    host: &Recorder,
    position: Vector,
) -> Result<crate::vm::prim::ObjectDetails, String> {
    let own = host.prim_key(host.prim_of(A));
    let mut details = host
        .object_details(A, &own)
        .ok_or("no details of the script's prim")?;
    details.avatar = true;
    details.position = position;
    Ok(details)
}

/// Runs `source` as the script `A` for `ticks` ticks under YEngine, every
/// OSSL function allowed; what it printed.
fn run_os(source: &str, host: &mut Recorder, ticks: u64) -> Result<Vec<String>, String> {
    let mut engine = engine_as(FLAVOUR);
    engine.set_ossl(Settings {
        threat_level: ThreatLevel::Severe,
        ..Settings::default()
    });
    let _none = engine.add(A, program_for(source, FLAVOUR)?);
    let _reports = run(&mut engine, host, ticks);
    Ok(host.texts(A).into_iter().map(str::to_owned).collect())
}

/// The text of every fault `A` hit.
fn faults(host: &Recorder) -> Vec<String> {
    host.faults
        .iter()
        .filter(|(caller, _)| *caller == A)
        .map(|(_, fault)| fault.error.to_string())
        .collect()
}

#[test]
fn an_npc_is_unowned_unless_its_options_say_otherwise() -> Result<(), String> {
    let mut host = Recorder::default();
    let printed = run_os(
        "default { state_entry() {
            print(osNpcCreate(\"Ann\", \"Bot\", <1, 2, 3>, \"\"));
            print(osNpcCreate(\"Bob\", \"Bot\", <4, 5, 6>, \"\", OS_NPC_SENSE_AS_AGENT));
            print(osNpcCreate(\"Cy\", \"Bot\", <7, 8, 9>, \"\", OS_NPC_NOT_OWNED));
            print((string)osIsNpc(llList2Key(osGetNPCList(), 0)));
            print(llList2CSV(osGetNPCList()));
        } }",
        &mut host,
        2,
    )?;
    let owners: Vec<(Option<String>, bool)> = host
        .npc_specs
        .iter()
        .map(|spec| (spec.owner.clone(), spec.sense_as_agent))
        .collect();
    assert_eq!(
        owners,
        vec![
            (None, false),
            (Some(super::OWNER.to_owned()), true),
            (None, false),
        ]
    );
    let first = "0c0c0c0c-0000-4000-8000-000000000001";
    assert_eq!(printed.first().map(String::as_str), Some(first));
    assert_eq!(
        printed.get(4).map(String::as_str),
        Some(
            "0c0c0c0c-0000-4000-8000-000000000001, <1.000000, 2.000000, 3.000000>, Ann Bot, \
             0c0c0c0c-0000-4000-8000-000000000002, <4.000000, 5.000000, 6.000000>, Bob Bot, \
             0c0c0c0c-0000-4000-8000-000000000003, <7.000000, 8.000000, 9.000000>, Cy Bot"
        )
    );
    Ok(())
}

#[test]
fn a_full_region_answers_the_null_key_and_a_missing_notecard_throws() -> Result<(), String> {
    let mut host = Recorder {
        npc_refusal: Some(crate::vm::npcs::NpcRefusal::Full),
        ..Recorder::default()
    };
    let printed = run_os(
        "default { state_entry() {
            print(osNpcCreate(\"Ann\", \"Bot\", <1, 2, 3>, \"\"));
            print(osNpcCreate(\"Ann\", \"Bot\", <1, 2, 3>, \"nope\"));
            print(\"not reached\");
        } }",
        &mut host,
        2,
    )?;
    assert_eq!(printed, vec![NULL_KEY.to_owned()]);
    assert_eq!(
        faults(&host),
        vec!["OSSL Runtime Error: osNpcCreate: Notecard 'nope' not found.".to_owned()]
    );
    Ok(())
}

#[test]
fn only_an_npc_the_owner_may_drive_answers() -> Result<(), String> {
    let mut host = Recorder {
        npcs: vec![
            npc(THEIRS, "Their Bot", Some(OTHER), 5.0),
            npc(ANYBODY, "Free Bot", None, 6.0),
        ],
        ..Recorder::default()
    };
    let printed = run_os(
        &format!(
            "default {{ state_entry() {{
                print((string)osNpcGetPos(\"{THEIRS}\"));
                print((string)osNpcGetOwner(\"{THEIRS}\"));
                print((string)osNpcGetPos(\"{ANYBODY}\"));
                print((string)osNpcGetOwner(\"{ANYBODY}\"));
                print((string)osNpcGetOwner(\"nonsense\"));
                osNpcSay(\"{THEIRS}\", \"theirs\");
                osNpcWhisper(\"{ANYBODY}\", 5, \"free\");
                osNpcMoveToTarget(\"{ANYBODY}\", <1, 2, 3>, OS_NPC_NO_FLY | OS_NPC_RUNNING);
                osNpcRemove(\"{THEIRS}\");
                osNpcRemove(\"nonsense\");
                osNpcSay(\"nonsense\", \"x\");
                print(\"not reached\");
            }} }}"
        ),
        &mut host,
        2,
    )?;
    assert_eq!(
        printed,
        vec![
            "<0.000000, 0.000000, 0.000000>".to_owned(),
            OTHER.to_owned(),
            "<6.000000, 10.000000, 20.000000>".to_owned(),
            ANYBODY.to_owned(),
            NULL_KEY.to_owned(),
        ]
    );
    assert_eq!(
        host.npc_lines
            .iter()
            .map(|(npc, speech)| (npc.as_str(), speech.channel, speech.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(ANYBODY, 5, "free")]
    );
    assert_eq!(
        host.npc_actions,
        vec![(
            ANYBODY.to_owned(),
            NpcAction::MoveTo {
                target: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                },
                no_fly: true,
                land_at_target: false,
                running: true,
            }
        )]
    );
    assert_eq!(host.npcs.len(), 2, "nobody may remove another's NPC");
    assert_eq!(
        faults(&host),
        vec!["FormatException: Invalid UUID".to_owned()]
    );
    Ok(())
}

#[test]
fn names_kinds_and_genders_read_avatars_and_npcs() -> Result<(), String> {
    let mut host = Recorder {
        npcs: vec![npc(ANYBODY, "Free Bot", None, 6.0)],
        ..Recorder::default()
    };
    let _other = host
        .avatars
        .insert(OTHER.to_owned(), TestAvatar::standing("Other Person"));
    let _shape = host.appearances.insert(OTHER.to_owned(), shape(255));
    let _shape = host.appearances.insert(ANYBODY.to_owned(), shape(0));
    let printed = run_os(
        &format!(
            "default {{ state_entry() {{
                print((string)osAvatarType(\"{OTHER}\") + (string)osAvatarType(\"{ANYBODY}\")
                    + (string)osAvatarType(\"{THEIRS}\") + (string)osAvatarType(\"x\"));
                print((string)osAvatarType(\"Free\", \"Bot\") + (string)osAvatarType(\"Other\", \"Person\")
                    + (string)osAvatarType(\"other\", \"person\"));
                print(osKey2Name(\"{ANYBODY}\") + \"|\" + osKey2Name(\"{OTHER}\") + \"|\" + osKey2Name(\"x\"));
                print(osAvatarName2Key(\"Free\", \"Bot\") + \"|\" + osAvatarName2Key(\"No\", \"Body\"));
                print((string)osGetGender(\"{OTHER}\") + (string)osGetGender(\"{ANYBODY}\")
                    + (string)osGetGender(\"{THEIRS}\"));
            }} }}"
        ),
        &mut host,
        2,
    )?;
    assert_eq!(
        printed,
        vec![
            "120-1".to_owned(),
            "210".to_owned(),
            "Free Bot|Other Person|".to_owned(),
            format!("{ANYBODY}|{NULL_KEY}"),
            "malefemaleunknown".to_owned(),
        ]
    );
    Ok(())
}

#[test]
fn damage_needs_damage_land_and_kills_at_zero() -> Result<(), String> {
    let mut host = Recorder {
        npcs: vec![npc(ANYBODY, "Free Bot", None, 6.0)],
        ..Recorder::default()
    };
    let _other = host
        .avatars
        .insert(OTHER.to_owned(), TestAvatar::standing("Other Person"));
    let printed = run_os(
        &format!(
            "default {{ state_entry() {{
                osCauseDamage(\"{OTHER}\", 30.0);
                print((string)osGetHealth(\"{OTHER}\") + \" \" + (string)osGetHealth(\"{THEIRS}\"));
            }} }}"
        ),
        &mut host,
        2,
    )?;
    assert_eq!(printed, vec!["100.000000 -1.000000".to_owned()]);
    host.parcel = Some(super::land::parcel(0x20, super::OWNER));
    host.printed.clear();
    let printed = run_os(
        &format!(
            "default {{ state_entry() {{
                osCauseDamage(\"{OTHER}\", 30.0);
                print((string)osGetHealth(\"{OTHER}\"));
                osCauseDamage(\"{ANYBODY}\", 200.0);
                osCauseDamage(\"{OTHER}\", 80.0);
                print((string)osGetHealth(\"{OTHER}\") + \" \" + (string)llGetListLength(osGetNPCList()));
                osCauseHealing(\"{OTHER}\", -50.0);
                osCauseHealing(\"{OTHER}\", 10.0);
                print((string)osGetHealth(\"{OTHER}\"));
                osSetHealth(\"{OTHER}\", -4.0);
                osSetHealRate(\"{OTHER}\", 300.0);
                print((string)osGetHealth(\"{OTHER}\") + \" \" + (string)osGetHealRate(\"{OTHER}\")
                    + \" \" + (string)osGetHealRate(\"{THEIRS}\"));
            }} }}"
        ),
        &mut host,
        2,
    )?;
    assert_eq!(
        printed,
        vec![
            "70.000000",
            "100.000000 0",
            "60.000000",
            "1.000000 100.000000 0.000000",
        ]
    );
    assert_eq!(
        host.alerts,
        vec![(OTHER.to_owned(), "You died!".to_owned(), true)]
    );
    assert_eq!(host.teleports, vec![(OTHER.to_owned(), Destination::Home)]);
    assert_eq!(
        host.vitals.get(OTHER).copied(),
        Some(Vitals {
            health: 1.0,
            heal_rate: 100.0,
        })
    );
    Ok(())
}

#[test]
fn stand_targets_and_sit_ranges_are_the_prims_own() -> Result<(), String> {
    let mut host = Recorder::default();
    let printed = run_os(
        "default { state_entry() {
            osSetSitActiveRange(200.0);
            osSetStandTarget(<1, 2, 3>);
            print((string)osGetSitActiveRange() + \" \" + (string)osGetStandTarget());
            print((string)osGetLinkSitActiveRange(LINK_THIS) + \" \" + (string)osGetLinkStandTarget(1));
            print((string)((integer)osGetLinkSitActiveRange(-2)) + \" \" + (string)osGetLinkStandTarget(7));
            osSetLinkSitActiveRange(LINK_SET, -1.0);
            print((string)osGetSitActiveRange());
        } }",
        &mut host,
        2,
    )?;
    assert_eq!(
        printed,
        vec![
            "128.000000 <1.000000, 2.000000, 3.000000>",
            "128.000000 <1.000000, 2.000000, 3.000000>",
            "-2147483648 <0.000000, 0.000000, 0.000000>",
            "-1.000000",
        ]
    );
    Ok(())
}

#[test]
fn teleports_go_where_each_overload_says() -> Result<(), String> {
    let mut host = Recorder::default();
    let _other = host
        .avatars
        .insert(OTHER.to_owned(), TestAvatar::standing("Other Person"));
    let details = avatar_details(
        &host,
        Vector {
            x: 100.0,
            y: 100.0,
            z: 25.0,
        },
    )?;
    let _details = host.details.insert(OTHER.to_owned(), details);
    host.region.name = "Home Region".to_owned();
    host.region.corner = (256_000.0, 256_000.0);
    let _printed = run_os(
        &format!(
            "default {{ state_entry() {{
                osTeleportAgent(\"{OTHER}\", <10, 20, 30>, <1, 0, 0>);
                osTeleportAgent(\"{OTHER}\", \"home region\", <11, 20, 30>, <1, 0, 0>);
                osTeleportAgent(\"{OTHER}\", \"Elsewhere\", <12, 20, 30>, <1, 0, 0>);
                osTeleportAgent(\"{OTHER}\", 1001, 1002, <13, 20, 30>, <1, 0, 0>);
                osTeleportAgent(\"{OTHER}\", <300, 20, 30>, <1, 0, 0>);
                osTeleportAgent(\"{THEIRS}\", <10, 20, 30>, <1, 0, 0>);
                osLocalTeleportAgent(\"{OTHER}\", <-5, 300, 0>, <-1, 2, 0>, <0, 1, 0>,
                    OS_LTPAG_USEVEL | OS_LTPAG_USELOOKAT);
                osLocalTeleportAgent(\"{OTHER}\", <5, 5, 50>, <0, 0, 0>, <0, 0, 0>, 0);
            }} }}"
        ),
        &mut host,
        200,
    )?;
    let look = Vector {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    let at = |x: f32| Vector {
        x,
        y: 20.0,
        z: 30.0,
    };
    assert_eq!(
        host.teleports,
        vec![
            (
                OTHER.to_owned(),
                Destination::Local {
                    position: at(10.0),
                    look_at: look.clone()
                }
            ),
            (
                OTHER.to_owned(),
                Destination::Local {
                    position: at(11.0),
                    look_at: look.clone()
                }
            ),
            (
                OTHER.to_owned(),
                Destination::Region {
                    name: "Elsewhere".to_owned(),
                    position: at(12.0),
                    look_at: look.clone(),
                }
            ),
            (
                OTHER.to_owned(),
                Destination::Global {
                    corner: (256_256.0, 256_512.0),
                    position: at(13.0),
                    look_at: look.clone(),
                }
            ),
            (
                OTHER.to_owned(),
                Destination::Global {
                    corner: (256_256.0, 256_000.0),
                    position: at(44.0),
                    look_at: look,
                }
            ),
            (
                OTHER.to_owned(),
                Destination::Moved {
                    position: Vector {
                        x: 0.1,
                        y: 255.9,
                        z: 1.05
                    },
                    velocity: Some(Vector {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    }),
                    announced: Some((
                        Some(Vector {
                            x: 0.0,
                            y: 1.0,
                            z: 0.0
                        }),
                        None
                    )),
                }
            ),
            (
                OTHER.to_owned(),
                Destination::Moved {
                    position: Vector {
                        x: 5.0,
                        y: 5.0,
                        z: 50.0
                    },
                    velocity: None,
                    announced: None,
                }
            ),
        ]
    );
    Ok(())
}

#[test]
fn an_appearance_saved_to_a_notecard_dresses_an_npc() -> Result<(), String> {
    let mut host = Recorder::default();
    let _other = host
        .avatars
        .insert(OTHER.to_owned(), TestAvatar::standing("Other Person"));
    let _shape = host.appearances.insert(OTHER.to_owned(), shape(255));
    let printed = run_os(
        &format!(
            "default {{ state_entry() {{
                key saved = osAgentSaveAppearance(\"{OTHER}\", \"look\");
                print((string)(saved != NULL_KEY));
                osAgentSaveAppearance(\"{OTHER}\", \"look\");
                print((string)llGetInventoryNumber(INVENTORY_NOTECARD));
                print(osNpcCreate(\"Twin\", \"Bot\", <1, 2, 3>, \"look\"));
                print((string)osAgentSaveAppearance(\"{THEIRS}\", \"none\"));
            }} }}"
        ),
        &mut host,
        2,
    )?;
    assert_eq!(
        printed,
        vec![
            "1".to_owned(),
            "1".to_owned(),
            "0c0c0c0c-0000-4000-8000-000000000001".to_owned(),
            NULL_KEY.to_owned(),
        ]
    );
    let dressed = host
        .npc_specs
        .first()
        .and_then(|spec| spec.appearance.clone())
        .map(|appearance| (appearance.is_male(), appearance.serial));
    assert_eq!(dressed, Some((Some(true), 1)));
    Ok(())
}

#[test]
fn attachments_are_counted_and_messaged_by_point() -> Result<(), String> {
    let mut host = Recorder::default();
    let _other = host
        .avatars
        .insert(OTHER.to_owned(), TestAvatar::standing("Other Person"));
    let _worn = host.worn_points.insert(
        OTHER.to_owned(),
        vec![(2, prim_key(2)), (2, prim_key(3)), (35, prim_key(4))],
    );
    for prim in 2..=4 {
        let _key = host.prim_keys.insert(prim, prim_key(prim));
    }
    let _b = host.prims.insert(B, 2);
    let _c = host.prims.insert(C, 4);
    let printed = run_os(
        &format!(
            "default {{ state_entry() {{
                print(llList2CSV(osGetNumberOfAttachments(\"{OTHER}\", [2, \"35\", 0, 1.5, 7])));
                print(llList2CSV(osGetNumberOfAttachments(\"{THEIRS}\", [2])));
                osMessageAttachments(\"{OTHER}\", \"hello\", [35], 0);
                osMessageAttachments(\"{OTHER}\", \"inverted\", [35], OS_ATTACH_MSG_INVERT_POINTS);
            }} }}"
        ),
        &mut host,
        2,
    )?;
    assert_eq!(
        printed,
        vec!["2, 2, 35, 1, 0, 0, 7, 0".to_owned(), String::new()]
    );
    Ok(())
}

#[test]
fn a_drop_needs_the_owners_attach_permission_and_a_force_does_not() -> Result<(), String> {
    let mut host = Recorder::default();
    let _owner = host
        .avatars
        .insert(super::OWNER.to_owned(), TestAvatar::standing("Probe Owner"));
    let _printed = run_os(
        "default { state_entry() {
            osDropAttachment();
            osForceDropAttachmentAt(<1, 2, 3>, ZERO_ROTATION);
        } }",
        &mut host,
        100,
    )?;
    assert_eq!(
        host.debug,
        vec!["Cannot drop attachment. Permissions not granted to owner.".to_owned()]
    );
    assert_eq!(
        host.drops,
        vec![(
            A,
            Some(crate::vm::npcs::DropAt {
                position: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                },
                rotation: crate::value::ZERO_ROTATION,
            })
        )]
    );
    Ok(())
}

#[test]
fn a_wear_from_the_contents_names_a_missing_item_and_throws() -> Result<(), String> {
    let mut host = Recorder::default();
    let _owner = host
        .avatars
        .insert(super::OWNER.to_owned(), TestAvatar::standing("Probe Owner"));
    let printed = run_os(
        "default { state_entry() {
            osForceAttachToAvatarFromInventory(\"hat\", 2);
            print(\"not reached\");
        } }",
        &mut host,
        2,
    )?;
    assert_eq!(printed, Vec::<String>::new());
    assert_eq!(host.heard(A), vec!["Could not find object 'hat'"]);
    assert_eq!(
        faults(&host),
        vec!["Exception: The inventory item 'hat' could not be found".to_owned()]
    );
    Ok(())
}

#[test]
fn group_invitations_need_a_group_and_the_owners_power() -> Result<(), String> {
    let group = "0e0e0e0e-0000-4000-8000-0000000000aa";
    let mut host = Recorder::default();
    let _other = host
        .avatars
        .insert(OTHER.to_owned(), TestAvatar::standing("Other Person"));
    let own = prim_key(1);
    let _key = host.prim_keys.insert(1, own.clone());
    let _a = host.prims.insert(A, 1);
    let mut details = host
        .object_details(A, &own)
        .ok_or("no details of the prim")?;
    details.group = group.to_owned();
    let _details = host.details.insert(own, details);
    let source = format!(
        "default {{ state_entry() {{
            print((string)osInviteToGroup(\"{OTHER}\") + (string)osEjectFromGroup(\"{OTHER}\"));
        }} }}"
    );
    let printed = run_os(&source, &mut host, 2)?;
    assert_eq!(printed, vec!["00".to_owned()]);
    let _powers = host
        .group_powers
        .insert((group.to_owned(), super::OWNER.to_owned()), 0b110);
    let printed = run_os(&source, &mut host, 2)?;
    assert_eq!(printed.last().map(String::as_str), Some("11"));
    let _member = host
        .group_powers
        .insert((group.to_owned(), OTHER.to_owned()), 0);
    let printed = run_os(&source, &mut host, 2)?;
    assert_eq!(printed.last().map(String::as_str), Some("21"));
    // LSL evaluates an operator's right operand first: the ejection goes
    // before the invitation.
    assert_eq!(
        host.group_changes,
        vec![
            (group.to_owned(), OTHER.to_owned(), false),
            (group.to_owned(), OTHER.to_owned(), true),
            (group.to_owned(), OTHER.to_owned(), false),
        ]
    );
    Ok(())
}
