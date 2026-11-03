//! What a script says about who may change its prim's contents
//! (`server-world-changed-raisers`): `llAllowInventoryDrop` hands the host the
//! flag, on every flavour, true for any integer but zero.

use pretty_assertions::assert_eq;

use super::*;

/// `llAllowInventoryDrop` hands the host each setting as it is called.
#[test]
fn allowing_drops_tells_the_host() -> Result<(), String> {
    let source = "default { state_entry() { llAllowInventoryDrop(TRUE); \
                  llAllowInventoryDrop(FALSE); llAllowInventoryDrop(-5); } }";
    for flavour in [
        EngineFlavour::SecondLife,
        EngineFlavour::YEngine,
        EngineFlavour::XEngine,
    ] {
        let mut host = Recorder::default();
        let mut engine = engine_as(flavour);
        let _none = engine.add(A, program_for(source, flavour)?);
        let _reports = run(&mut engine, &mut host, 2);
        assert_eq!(
            host.allowed_drops,
            vec![(A, true), (A, false), (A, true)],
            "{flavour:?}"
        );
    }
    Ok(())
}
