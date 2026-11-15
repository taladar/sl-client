//! OpenSim's land, terrain, wind and environment writers
//! (`server-lsl-lib-ossl-land-writers`) run in the VM under YEngine against
//! the test host, with every OSSL function allowed: what each asks of the
//! host, the second permission check it makes itself, and what it throws, as
//! `OSSL_Api.cs` and `LS_Api.cs` state them.

use pretty_assertions::assert_eq;

use super::land::parcel;
use super::*;

use crate::library::ThreatLevel;
use crate::vm::land::{
    DayCycle, EnvironmentChange, EnvironmentScope, LandChange, ParcelDetails, RegionEdit,
};
use crate::vm::ossl::Settings;

/// The flavour every test here runs under.
const FLAVOUR: EngineFlavour = EngineFlavour::YEngine;

/// Somebody who is not the scripts' owner.
const STRANGER: &str = "0e0e0e0e-0000-4000-8000-000000000001";

/// A settings asset holding a day cycle.
const CYCLE: &str = "5e5e5e5e-0000-4000-8000-000000000001";

/// A settings asset holding a sky.
const SKY: &str = "5e5e5e5e-0000-4000-8000-000000000002";

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

/// A host whose region is one parcel of `owner`'s.
fn on_land_of(owner: &str) -> Recorder {
    Recorder {
        parcel: Some(parcel(0, owner)),
        ..Recorder::default()
    }
}

#[test]
fn the_ground_moves_where_the_owner_may_terraform_and_nowhere_else() -> Result<(), String> {
    // The deprecated name costs the script a second's pause.
    let source = "default { state_entry() {
        print((string)osSetTerrainHeight(10, 20, 33.5));
        print((string)osTerrainSetHeight(11, 20, 34.0));
    } }";
    let mut own = on_land_of(super::OWNER);
    assert_eq!(run_os(source, &mut own, 200)?, vec!["1", "1"]);
    assert_eq!(
        own.region_edits,
        vec![
            RegionEdit::TerrainHeight {
                x: 10,
                y: 20,
                height: 33.5,
            },
            RegionEdit::TerrainHeight {
                x: 11,
                y: 20,
                height: 34.0,
            },
        ]
    );
    // A stranger's land, closed to terraforming: no.
    let mut theirs = on_land_of(STRANGER);
    assert_eq!(run_os(source, &mut theirs, 200)?, vec!["0", "0"]);
    assert!(theirs.region_edits.is_empty());
    // Open to it (`PARCEL_FLAG_ALLOW_TERRAFORM`), or the owner an estate
    // manager: yes.
    let mut open = Recorder {
        parcel: Some(parcel(0x10, STRANGER)),
        ..Recorder::default()
    };
    assert_eq!(run_os(source, &mut open, 200)?, vec!["1", "1"]);
    let mut manager = on_land_of(STRANGER);
    manager.standing.roles.push(Allowed::EstateManager);
    assert_eq!(run_os(source, &mut manager, 200)?, vec!["1", "1"]);
    Ok(())
}

#[test]
fn a_height_off_the_region_is_a_runtime_error_in_the_new_name() -> Result<(), String> {
    for function in ["osSetTerrainHeight", "osTerrainSetHeight"] {
        let mut host = on_land_of(super::OWNER);
        let printed = run_os(
            &format!(
                "default {{ state_entry() {{
                    print(\"call\");
                    print((string){function}(256, 0, 1.0));
                }} }}"
            ),
            &mut host,
            3,
        )?;
        assert_eq!(printed, vec!["call"], "{function}");
        let faults = faults(&host);
        assert!(
            faults.iter().any(|fault| fault
                .contains("OSSL Runtime Error: osSetTerrainHeight: Coordinate out of bounds")),
            "{function}: {faults:?}"
        );
    }
    Ok(())
}

#[test]
fn ground_textures_change_for_a_god_alone() -> Result<(), String> {
    let source = "default { state_entry() {
        osSetTerrainTexture(1, \"0f0f0f0f-0000-4000-8000-000000000001\");
        osSetTerrainTexture(4, \"0f0f0f0f-0000-4000-8000-000000000001\");
        osSetTerrainTexture(0, NULL_KEY);
        osSetTerrainTextureHeight(3, 5.0, 40.0);
        osSetTerrainTextureHeight(-1, 5.0, 40.0);
        osTerrainFlush();
    } }";
    let mut resident = Recorder::default();
    let _printed = run_os(source, &mut resident, 2)?;
    assert_eq!(resident.region_edits, vec![RegionEdit::TerrainFlush]);
    let mut god = Recorder::default();
    god.standing.roles.push(Allowed::God);
    let _printed = run_os(source, &mut god, 2)?;
    assert_eq!(
        god.region_edits,
        vec![
            RegionEdit::TerrainTexture {
                corner: 1,
                texture: "0f0f0f0f-0000-4000-8000-000000000001".to_owned(),
            },
            RegionEdit::TerrainTextureHeights {
                corner: 3,
                low: 5.0,
                high: 40.0,
            },
            RegionEdit::TerrainFlush,
        ]
    );
    Ok(())
}

#[test]
fn the_water_and_the_sun_are_set_and_the_dead_setters_do_nothing() -> Result<(), String> {
    let mut host = Recorder::default();
    let _printed = run_os(
        "default { state_entry() {
            osSetRegionWaterHeight(21.5);
            osSetRegionSunSettings(TRUE, FALSE, -2.0);
            osSetRegionSunSettings(FALSE, TRUE, 50.0);
            osSetEstateSunSettings(TRUE, 3.0);
            osSetSunParam(\"day_length\", 3600.0);
        } }",
        &mut host,
        2,
    )?;
    assert_eq!(
        host.region_edits,
        vec![
            RegionEdit::WaterHeight(21.5),
            // -2 wraps to 22, Linden's 28; 50 to 2, Linden's 8.
            RegionEdit::Sun {
                estate_sun: true,
                fixed: false,
                hour: 28.0,
            },
            RegionEdit::Sun {
                estate_sun: false,
                fixed: true,
                hour: 8.0,
            },
        ]
    );
    Ok(())
}

#[test]
fn a_wind_parameter_is_read_and_written_and_an_unknown_one_throws() -> Result<(), String> {
    let mut host = Recorder::default();
    let _models = host.wind_plugins.insert(
        "SimpleRandomWind".to_owned(),
        std::collections::BTreeMap::from([("strength".to_owned(), 1.0)]),
    );
    let printed = run_os(
        "default { state_entry() {
            print((string)osGetWindParam(\"SimpleRandomWind\", \"strength\"));
            osSetWindParam(\"SimpleRandomWind\", \"strength\", 2.5);
            osSetWindParam(\"Nonesuch\", \"strength\", 2.5);
            print((string)osGetWindParam(\"SimpleRandomWind\", \"strength\"));
            print((string)osGetWindParam(\"SimpleRandomWind\", \"gust\"));
        } }",
        &mut host,
        2,
    )?;
    assert_eq!(printed, vec!["1.000000", "2.500000"]);
    assert!(
        faults(&host)
            .iter()
            .any(|fault| fault.contains("Exception: Unknown SimpleRandomWind parameter gust")),
        "{:?}",
        faults(&host)
    );
    let mut missing = Recorder::default();
    let _printed = run_os(
        "default { state_entry() { print((string)osGetWindParam(\"Nonesuch\", \"x\")); } }",
        &mut missing,
        2,
    )?;
    assert!(
        faults(&missing)
            .iter()
            .any(|fault| fault.contains("Exception: Could not find plugin Nonesuch")),
        "{:?}",
        faults(&missing)
    );
    Ok(())
}

#[test]
fn notices_and_restarts_need_an_estate_manager() -> Result<(), String> {
    let source = format!(
        "default {{ state_entry() {{
            osRegionNotice(\"hello\");
            osRegionNotice(\"{}\", \"you\");
            osRegionNotice(\"{STRANGER}\", \"nobody here\");
            print((string)osRegionRestart(125.0));
            print((string)osRegionRestart(60.0, \"Going down\"));
            print((string)osRegionRestart(10.0));
        }} }}",
        super::OWNER
    );
    let with_owner = || {
        let mut host = Recorder::default();
        let _owner = host.avatars.insert(
            super::OWNER.to_owned(),
            super::TestAvatar::standing("Probe Owner"),
        );
        host
    };
    let mut resident = with_owner();
    assert_eq!(run_os(&source, &mut resident, 2)?, vec!["0", "0", "0"]);
    assert!(resident.region_edits.is_empty());
    let mut manager = with_owner();
    manager.standing.roles.push(Allowed::EstateManager);
    manager.region.name = "Testing Ground".to_owned();
    assert_eq!(run_os(&source, &mut manager, 2)?, vec!["1", "1", "1"]);
    assert_eq!(
        manager.region_edits,
        vec![
            RegionEdit::Notice {
                agent: None,
                text: "hello\n".to_owned(),
            },
            RegionEdit::Notice {
                agent: Some(super::OWNER.to_owned()),
                text: "you\n".to_owned(),
            },
            RegionEdit::Restart {
                alerts: vec![125, 65, 35, 20, 5],
                notice: "Region: Testing Ground is about to restart.\n\nIf you stay here you \
                         will be logged out.\n\n\nTime remaining: 2 minutes and 5 seconds.\n"
                    .to_owned(),
            },
            RegionEdit::Restart {
                alerts: vec![60, 45, 30, 15],
                notice: "Going down\n\nTime remaining: 1 minute.\n".to_owned(),
            },
            RegionEdit::AbortRestart,
        ]
    );
    Ok(())
}

#[test]
fn parcel_details_change_as_far_as_the_owner_may() -> Result<(), String> {
    let source = format!(
        "default {{ state_entry() {{
            osSetParcelDetails(llGetPos(), [
                PARCEL_DETAILS_NAME, \"Home\",
                PARCEL_DETAILS_DESC, \"A place\",
                PARCEL_DETAILS_OWNER, \"{STRANGER}\",
                PARCEL_DETAILS_SEE_AVATARS, 1,
                PARCEL_DETAILS_GROUP_SOUNDS, \"1\"
            ]);
            print(\"done\");
        }} }}"
    );
    // The owner's own land: everything but the owner, which is refused with
    // a shout.
    let mut own = on_land_of(super::OWNER);
    assert_eq!(run_os(&source, &mut own, 200)?, vec!["done"]);
    assert_eq!(
        own.land_changes,
        vec![LandChange::Details(ParcelDetails {
            name: Some("Home".to_owned()),
            description: Some("A place".to_owned()),
            see_avatars: Some(true),
            group_sounds: Some(true),
            ..ParcelDetails::default()
        })]
    );
    assert_eq!(
        own.debug,
        vec!["script owner does not have permission to modify the parcel owner"]
    );
    // An estate manager's: the owner too, the group cleared with it.
    let mut manager = on_land_of(super::OWNER);
    manager.standing.roles.push(Allowed::EstateManager);
    let _printed = run_os(&source, &mut manager, 200)?;
    assert!(matches!(
        manager.land_changes.as_slice(),
        [LandChange::Details(ParcelDetails { owner: Some(owner), group: Some(group), .. })]
            if owner == STRANGER && group == crate::value::NULL_KEY
    ));
    // Somebody else's: refused whole.
    let mut theirs = on_land_of(STRANGER);
    let _printed = run_os(&source, &mut theirs, 200)?;
    assert!(theirs.land_changes.is_empty());
    assert_eq!(
        theirs.debug,
        vec!["script owner does not have permission to modify the parcel"]
    );
    Ok(())
}

#[test]
fn a_parcel_detail_that_is_no_integer_or_a_point_off_the_region_throws() -> Result<(), String> {
    let mut manager = on_land_of(super::OWNER);
    manager.standing.roles.push(Allowed::EstateManager);
    let printed = run_os(
        "default { state_entry() {
            osSetParcelDetails(<1, 1, 0>, [PARCEL_DETAILS_CLAIMDATE, \"soon\"]);
            print(\"after\");
        } }",
        &mut manager,
        2,
    )?;
    assert!(printed.is_empty());
    assert!(
        faults(&manager)
            .iter()
            .any(|fault| fault.contains("FormatException")),
        "{:?}",
        faults(&manager)
    );
    let mut off = on_land_of(super::OWNER);
    let _printed = run_os(
        "default { state_entry() { osParcelSubdivide(<-4, 0, 0>, <8, 8, 0>); } }",
        &mut off,
        2,
    )?;
    assert!(
        faults(&off)
            .iter()
            .any(|fault| fault.contains("Error: Parcel not found at point -4, 0")),
        "{:?}",
        faults(&off)
    );
    Ok(())
}

#[test]
fn a_cut_is_asked_for_and_a_join_of_one_parcel_is_not() -> Result<(), String> {
    let mut own = on_land_of(super::OWNER);
    let _printed = run_os(
        "default { state_entry() {
            osParcelSubdivide(<17.5, 8, 0>, <4, 32.9, 0>);
            osParcelJoin(<0, 0, 0>, <64, 64, 0>);
        } }",
        &mut own,
        2,
    )?;
    assert_eq!(
        own.land_changes,
        vec![LandChange::Subdivide((4, 8), (17, 32))]
    );
    // Not the owner's: nothing.
    let mut theirs = on_land_of(STRANGER);
    let _printed = run_os(
        "default { state_entry() { osParcelSubdivide(<4, 8, 0>, <16, 32, 0>); } }",
        &mut theirs,
        2,
    )?;
    assert!(theirs.land_changes.is_empty());
    Ok(())
}

#[test]
fn environments_are_replaced_and_reset_with_opensim_s_codes() -> Result<(), String> {
    let mut host = on_land_of(super::OWNER);
    host.region.environment_override = true;
    let _cycle = host.day_cycles.insert(CYCLE.to_owned(), DayCycle::DayCycle);
    let _sky = host
        .day_cycles
        .insert(SKY.to_owned(), DayCycle::NotDayCycle);
    let _owner = host.avatars.insert(
        super::OWNER.to_owned(),
        super::TestAvatar::standing("Probe Owner"),
    );
    let printed = run_os(
        &format!(
            "default {{ state_entry() {{
                print((string)osReplaceParcelEnvironment(2, \"{CYCLE}\"));
                print((string)osReplaceParcelEnvironment(2, \"{SKY}\"));
                print((string)osReplaceParcelEnvironment(2, \"nothing\"));
                print((string)osReplaceParcelEnvironment(2, \"\"));
                print((string)osReplaceRegionEnvironment(0, \"{CYCLE}\", 6, -2, 0, 0, 0));
                print((string)osReplaceAgentEnvironment(\"{}\", 1, \"{CYCLE}\"));
                print((string)osReplaceAgentEnvironment(\"{STRANGER}\", 1, \"{CYCLE}\"));
                print((string)osReplaceAgentEnvironment(\"{}\", 1, \"{SKY}\"));
                print((string)osResetEnvironment(1, 0));
                print((string)osResetEnvironment(0, 0));
            }} }}",
            super::OWNER,
            super::OWNER
        ),
        &mut host,
        2,
    )?;
    // The region's needs the estate's owner: refused here.
    assert_eq!(
        printed,
        vec!["1", "-5", "-4", "1", "-3", "1", "-4", "-3", "1", "-3"]
    );
    let set = |cycle: &str| EnvironmentChange::Set {
        day_cycle: Some(cycle.to_owned()),
        day_length: None,
        day_offset: None,
        altitudes: None,
    };
    let at = crate::value::ZERO_VECTOR;
    assert_eq!(
        host.environment_changes,
        vec![
            (EnvironmentScope::Parcel(at.clone()), set(CYCLE), 2),
            (
                EnvironmentScope::Agent(super::OWNER.to_owned()),
                set(CYCLE),
                1
            ),
            (EnvironmentScope::Parcel(at), EnvironmentChange::Reset, 0),
        ]
    );
    Ok(())
}

#[test]
fn the_region_s_environment_takes_its_day_and_altitudes_as_opensim_reads_them() -> Result<(), String>
{
    let mut host = Recorder::default();
    host.standing.roles.push(Allowed::EstateOwner);
    let printed = run_os(
        "default { state_entry() {
            print((string)osReplaceRegionEnvironment(3, \"\", 6, -2, 1500, 0, 900));
            print((string)osReplaceRegionEnvironment(3, \"\", 1, 20, 0, 0, 0));
            print((string)osResetEnvironment(0, 0));
        } }",
        &mut host,
        2,
    )?;
    assert_eq!(printed, vec!["1", "1", "1"]);
    assert_eq!(
        host.environment_changes,
        vec![
            (
                EnvironmentScope::Region,
                EnvironmentChange::Set {
                    day_cycle: None,
                    day_length: Some(21_600),
                    // -2 is read a day later: 22 hours.
                    day_offset: Some(79_200),
                    altitudes: Some([900.0, 1500.0, 2000.0]),
                },
                3
            ),
            (EnvironmentScope::Region, EnvironmentChange::Reset, 0),
        ]
    );
    Ok(())
}

#[test]
fn lightshare_reads_and_writes_the_scene_for_an_estate_manager() -> Result<(), String> {
    let mut host = Recorder::default();
    let printed = run_os(
        "default { state_entry() {
            print(llList2CSV(lsGetWindlightScene([WL_WATER_COLOR, 99, WL_MAX_ALTITUDE])));
            print((string)lsSetWindlightScene([WL_SCENE_GAMMA, 2.0]));
            print((string)lsSetWindlightSceneTargeted([], NULL_KEY));
        } }",
        &mut host,
        2,
    )?;
    assert_eq!(
        printed,
        vec!["0, <4.000000, 38.000000, 64.000000>, 18, 1605", "0", "0"]
    );
    assert_eq!(
        host.debug,
        vec![
            "lsSetWindlightScene can only be used by estate managers or owners.",
            "Function temporary not supported",
        ]
    );
    let mut manager = Recorder::default();
    manager.standing.roles.push(Allowed::EstateManager);
    let printed = run_os(
        "default { state_entry() {
            print((string)lsSetWindlightScene([WL_SCENE_GAMMA, 2, 77, WL_CLOUD_SCROLL_X_LOCK, 1]));
            print(llList2CSV(lsGetWindlightScene([WL_SCENE_GAMMA, WL_CLOUD_SCROLL_X_LOCK])));
            print((string)lsSetWindlightScene([WL_AMBIENT, <1, 2, 3>]));
            print((string)lsSetWindlightScene([WL_CLOUD_SCROLL_X_LOCK, 0.5]));
            lsClearWindlightScene();
            print(llList2CSV(lsGetWindlightScene([WL_SCENE_GAMMA])));
        } }",
        &mut manager,
        2,
    )?;
    assert_eq!(
        printed,
        vec!["1", "24, 2.000000, 34, 1", "0", "0", "24, 1.000000"]
    );
    assert_eq!(
        manager.debug,
        vec![
            "Error running rule WL_AMBIENT: arg #1 - parameter 1 must be rotation",
            // OpenSim names the other lock.
            "Error running rule WL_CLOUD_SCROLL_Y_LOCK: arg #1 - parameter 1 must be integer",
        ]
    );
    Ok(())
}

#[test]
fn a_sip_address_needs_the_land_and_a_voice_module() -> Result<(), String> {
    let mut theirs = on_land_of(STRANGER);
    let _printed = run_os(
        "default { state_entry() { osSetParcelSIPAddress(\"sip:a@b\"); } }",
        &mut theirs,
        2,
    )?;
    assert!(faults(&theirs).iter().any(|fault| fault.contains(
        "OSSL Runtime Error: osSetParcelSIPAddress: Sorry, you need to own the land to use this \
         function"
    )));
    let mut silent = on_land_of(super::OWNER);
    let _printed = run_os(
        "default { state_entry() { osSetParcelSIPAddress(\"sip:a@b\"); } }",
        &mut silent,
        2,
    )?;
    assert!(faults(&silent).iter().any(|fault| fault.contains(
        "OSSL Runtime Error: osSetParcelSIPAddress: No voice module enabled for this land"
    )));
    let mut voiced = on_land_of(super::OWNER);
    voiced.sip_addresses = Some(Vec::new());
    let _printed = run_os(
        "default { state_entry() { osSetParcelSIPAddress(\"sip:a@b\"); } }",
        &mut voiced,
        2,
    )?;
    assert_eq!(voiced.sip_addresses, Some(vec!["sip:a@b".to_owned()]));
    Ok(())
}

#[test]
fn the_region_s_statistics_are_a_list_of_floats() -> Result<(), String> {
    let mut host = Recorder {
        region_stats: vec![1.0, 45.0, 45.0],
        ..Recorder::default()
    };
    let printed = run_os(
        "default { state_entry() { print(llList2CSV(osGetRegionStats())); } }",
        &mut host,
        2,
    )?;
    assert_eq!(printed, vec!["1.000000, 45.000000, 45.000000"]);
    Ok(())
}
