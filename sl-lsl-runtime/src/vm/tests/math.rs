//! The maths and rotation library held to the grids: the probes' own
//! scripts run in the VM under each flavour and every line they print set
//! against the line the grid printed — `math/functions` and
//! `math/rotations` whole, and each `errors/math` case as a script of its
//! own, with what OpenSim's engines say when `llModPow` throws.
//!
//! As in `strings.rs`, a probe is changed only where it reads the world, and
//! every other difference is a named deviation with its reason. On OpenSim
//! most are one reason: the value model is Second Life's, a float of 24 bits,
//! where OpenSim computes and prints in double — so a result past the float
//! range, a digit in the sixth decimal and a negative zero's sign come out
//! differently though the function's formula is OpenSim's.

use pretty_assertions::assert_eq;

use super::strings::{hold, offline, probe_lines, replay};
use super::*;

/// A `frand` line without its `min=` and `max=`, which are each run's own
/// draws; what is left — the count, which side of zero, how many zeros — is
/// the grid's answer.
fn without_draws(line: &str) -> String {
    line.find(" min=").map_or_else(
        || line.to_owned(),
        |at| line.get(..at).unwrap_or(line).to_owned(),
    )
}

/// The `PROBE` lines, `frand` lines without their draws.
fn comparable(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            if line.contains(" rec=frand ") {
                without_draws(line)
            } else {
                line.clone()
            }
        })
        .collect()
}

/// `math/functions` under `flavour` against `transcript`, but for the
/// `(rec, case)` pairs `deviations` names.
fn functions_hold(
    flavour: EngineFlavour,
    transcript: &str,
    deviations: &[(&str, &str)],
) -> Result<(), String> {
    let source = offline(include_str!("../../../probes/math/functions.lsl"), "");
    let (host, ours) = replay(&source, flavour, 400)?;
    assert!(host.told.is_empty() && host.heard(A).is_empty());
    hold(
        &comparable(&ours),
        &comparable(&probe_lines(transcript)),
        deviations,
    );
    Ok(())
}

#[test]
fn math_functions_print_what_aditi_printed() -> Result<(), String> {
    functions_hold(
        EngineFlavour::SecondLife,
        include_str!("../../../probes/results/aditi/math/functions.txt"),
        &[],
    )
}

/// `math/functions` lines OpenSim prints in double: `llPow` past the float
/// range (`1e60`, `1e42` there, `Infinity` in a float), `llTan(PI_BY_TWO)`
/// with all its digits (−22877332.428857; the float −22877332 prints
/// `-22877330.000000`), `llTan(1e30)` of the double 1e30 — the literal is a
/// float here, `1.0000000150474662e30`, whose tangent is another number —
/// and `llSqrt(-0.0)`, which is `-0.0` on both but OpenSim prints without the
/// sign.
const OPENSIM_FUNCTIONS_IN_DOUBLE: [(&str, &str); 5] = [
    ("pow", "1e30^2"),
    ("pow", "1e-6^-7"),
    ("tan", "pi/2"),
    ("tan", "1e30"),
    ("sqrt", "minus-zero"),
];

#[test]
fn math_functions_print_what_opensim_printed() -> Result<(), String> {
    functions_hold(
        EngineFlavour::YEngine,
        include_str!("../../../probes/results/opensim-yengine/math/functions.txt"),
        &OPENSIM_FUNCTIONS_IN_DOUBLE,
    )?;
    functions_hold(
        EngineFlavour::XEngine,
        include_str!("../../../probes/results/opensim-xengine/math/functions.txt"),
        &OPENSIM_FUNCTIONS_IN_DOUBLE,
    )
}

/// `math/rotations` under `flavour` against `transcript`, but for the
/// `(rec, case)` pairs `deviations` names.
fn rotations_hold(
    flavour: EngineFlavour,
    transcript: &str,
    deviations: &[(&str, &str)],
) -> Result<(), String> {
    let source = offline(include_str!("../../../probes/math/rotations.lsl"), "");
    let (host, ours) = replay(&source, flavour, 400)?;
    assert!(host.told.is_empty() && host.heard(A).is_empty());
    hold(&ours, &probe_lines(transcript), deviations);
    Ok(())
}

/// `math/rotations` round trips that print a digit apart on Second Life:
/// `llRot2Euler(llEuler2Rot(…))` of `<30°, 45°, 60°>` (`<30.00001, 45.00001,
/// 60>` here, `<30, 45.00001, 60.00001>` there) and of `<10°, −90°, 20°>`
/// (`9.999999` against `10`). Both rest on `llEuler2Rot`'s last bits, which
/// the runtime takes in PyOptimizer's double form, within three units in the
/// last place of aditi's, since no arrangement of Linden's code tried
/// matches them (`library::rotations`); the five other round trips agree.
const SL_EULER_ROUND_TRIPS: [(&str, &str); 2] = [
    ("euler2rot-back", "xyz30-45-60"),
    ("euler2rot-back", "gimbal-down"),
];

#[test]
fn math_rotations_print_what_aditi_printed() -> Result<(), String> {
    rotations_hold(
        EngineFlavour::SecondLife,
        include_str!("../../../probes/results/aditi/math/rotations.txt"),
        &SL_EULER_ROUND_TRIPS,
    )
}

/// `math/rotations` lines that part on OpenSim for the value model, not the
/// library: the operators `vector * rotation` and `vector / rotation` are
/// Second Life's on every flavour (OpenSim normalises first and uses another
/// formula — book `gridspec/lsl.md` § Rotations), unary minus keeps a zero's
/// sign here, a negative zero prints without its sign there, and a double
/// prints a sixth decimal a float rounds away (`90.000001`, `-140.000004`;
/// `llRot2Axis(<1, 2, 3, 4>)`'s `y` is 0.53452248 in either, which a float
/// prints `0.534523` and a double `0.534522`).
const OPENSIM_ROTATIONS_IN_DOUBLE: [(&str, &str); 16] = [
    ("euler2rot-back", "y90"),
    ("euler2rot-back", "gimbal-up"),
    ("euler2rot-back", "gimbal-down"),
    ("euler2rot-back", "past-a-turn"),
    ("rotate", "x-by-twice-identity"),
    ("rotate", "x-by-1234"),
    ("rotate", "x-by-zero"),
    ("unrotate", "x-by-1234"),
    ("axis-angle", "z-450"),
    ("axis-angle", "z-minus-90"),
    ("rot2euler", "negative-identity"),
    ("rot2euler", "z-negative-s"),
    ("rot2axis", "1234"),
    ("rot2axis", "z-negative-s"),
    ("axes2rot", "left-handed"),
    ("negate", "x-axis"),
];

#[test]
fn math_rotations_print_what_opensim_printed() -> Result<(), String> {
    rotations_hold(
        EngineFlavour::YEngine,
        include_str!("../../../probes/results/opensim-yengine/math/rotations.txt"),
        &OPENSIM_ROTATIONS_IN_DOUBLE,
    )?;
    rotations_hold(
        EngineFlavour::XEngine,
        include_str!("../../../probes/results/opensim-xengine/math/rotations.txt"),
        &OPENSIM_ROTATIONS_IN_DOUBLE,
    )
}

/// The `errors/math` cases, by number.
const MATH_ERRORS: [(u32, &str); 15] = [
    (1, include_str!("../../../probes/errors/math/math-01.lsl")),
    (2, include_str!("../../../probes/errors/math/math-02.lsl")),
    (3, include_str!("../../../probes/errors/math/math-03.lsl")),
    (4, include_str!("../../../probes/errors/math/math-04.lsl")),
    (5, include_str!("../../../probes/errors/math/math-05.lsl")),
    (6, include_str!("../../../probes/errors/math/math-06.lsl")),
    (7, include_str!("../../../probes/errors/math/math-07.lsl")),
    (8, include_str!("../../../probes/errors/math/math-08.lsl")),
    (9, include_str!("../../../probes/errors/math/math-09.lsl")),
    (10, include_str!("../../../probes/errors/math/math-10.lsl")),
    (11, include_str!("../../../probes/errors/math/math-11.lsl")),
    (12, include_str!("../../../probes/errors/math/math-12.lsl")),
    (13, include_str!("../../../probes/errors/math/math-13.lsl")),
    (14, include_str!("../../../probes/errors/math/math-14.lsl")),
    (15, include_str!("../../../probes/errors/math/math-15.lsl")),
];

/// What an `errors/math` case did: its number, the `case` lines it printed,
/// the lines told the owner and the lines said on `DEBUG_CHANNEL`.
type MathReport = (u32, Vec<String>, Vec<String>, Vec<String>);

/// Every `errors/math` case under `flavour`, each running its case with the
/// text `"0"` as it enters `default` rather than when its controller sends
/// it.
fn math_errors(flavour: EngineFlavour) -> Result<Vec<MathReport>, String> {
    let mut reports = Vec::new();
    for (number, source) in MATH_ERRORS {
        let script = offline(source, &format!("Math {number}")).replacen(
            "default\n{\n",
            "default\n{\n    state_entry() { run(\"0\"); }\n",
            1,
        );
        let (host, lines) = replay(&script, flavour, 50)?;
        let lines = lines
            .into_iter()
            .filter(|line| line.contains(" rec=case "))
            .collect();
        let told = host.told.iter().map(|(_, _, text)| text.clone()).collect();
        let said = host.heard(A).into_iter().map(str::to_owned).collect();
        reports.push((number, lines, told, said));
    }
    Ok(reports)
}

/// The `case` lines a transcript holds for `Math <number>`.
fn math_case_lines(transcript: &str, number: u32) -> Vec<String> {
    let script = format!("script=\"Math {number}\" case=");
    probe_lines(transcript)
        .into_iter()
        .filter(|line| line.contains(" rec=case ") && line.contains(&script))
        .collect()
}

/// The `errors/math` cases whose answer is the library's, by number: 8
/// `llSqrt`, 10 `llModPow`, 11 `llLog` and `llLog10`, 12 `llAcos` and
/// `llAsin`, 13 `llPow`, 15 `llVecNorm`, `llRot2Euler` and `llAxes2Rot`. The
/// others are the operators' and casts', held by the run-time error tests.
const LIBRARY_CASES: [u32; 6] = [8, 10, 11, 12, 13, 15];

#[test]
fn the_library_cases_of_errors_math_answer_as_on_aditi() -> Result<(), String> {
    let aditi = include_str!("../../../probes/results/aditi/errors/math.txt");
    for (number, lines, told, said) in math_errors(EngineFlavour::SecondLife)? {
        if LIBRARY_CASES.contains(&number) {
            assert_eq!(lines, math_case_lines(aditi, number), "Math {number}");
            assert!(told.is_empty() && said.is_empty(), "Math {number}");
        }
    }
    Ok(())
}

/// The library cases OpenSim prints otherwise for the value model: case 15
/// prints vectors and a rotation with `(string)`, five decimals here and six
/// there (its answers, `ZERO_VECTOR` twice and `<0, 0, 0, 0.5>`, agree).
const OPENSIM_MATH_CASTS: [u32; 1] = [15];

/// YEngine's report of `llModPow(2, 3, 0)`, as the transcript has it after
/// the item's key: the exception and that a reset is needed.
const YENGINE_MOD_POW: [&str; 2] = [
    "DivideByZeroException: Attempted to divide by zero.",
    "Script must be Reset to re-enable.",
];

#[test]
fn the_library_cases_of_errors_math_answer_as_on_opensim() -> Result<(), String> {
    for (flavour, transcript) in [
        (
            EngineFlavour::YEngine,
            include_str!("../../../probes/results/opensim-yengine/errors/math.txt"),
        ),
        (
            EngineFlavour::XEngine,
            include_str!("../../../probes/results/opensim-xengine/errors/math.txt"),
        ),
    ] {
        for (number, lines, told, said) in math_errors(flavour)? {
            if !LIBRARY_CASES.contains(&number) {
                continue;
            }
            let theirs = math_case_lines(transcript, number);
            assert_eq!(
                lines == theirs,
                !OPENSIM_MATH_CASTS.contains(&number),
                "{flavour:?} Math {number}:\n  ours {lines:?}\n  grid {theirs:?}"
            );
            match (number, flavour) {
                (10, EngineFlavour::YEngine) => {
                    let [first, exception, _where, reset] = told.as_slice() else {
                        return Err(format!("Math 10 told {told:?}"));
                    };
                    assert!(first.starts_with("[YEngine]: Exception while running "));
                    assert_eq!([exception.as_str(), reset.as_str()], YENGINE_MOD_POW);
                    for line in YENGINE_MOD_POW {
                        assert!(transcript.contains(&format!(
                            "# owner-say: from=\"Faulty prim\" text=\"{line}\""
                        )));
                    }
                    assert!(said.is_empty());
                }
                (10, _) => {
                    let [line] = said.as_slice() else {
                        return Err(format!("Math 10 said {said:?}"));
                    };
                    assert!(
                        transcript.contains(&format!("text=\"{line}\\n  at ")),
                        "{line}"
                    );
                    assert!(told.is_empty());
                }
                _ => assert!(told.is_empty() && said.is_empty(), "Math {number}"),
            }
        }
    }
    Ok(())
}

#[test]
fn llfrand_draws_the_same_from_the_same_seed_and_stays_in_range() -> Result<(), String> {
    let source = r#"
default
{
    state_entry()
    {
        integer i;
        for (i = 0; i < 50; ++i)
        {
            print((string)llFrand(10.0) + " " + (string)llFrand(-0.5));
        }
        print("done");
    }
}"#;
    let draws = || -> Result<Vec<String>, String> {
        let mut engine = engine();
        let mut host = Recorder::default();
        let _none = engine.add(A, program(source)?);
        let _reports = run(&mut engine, &mut host, 20);
        Ok(host.texts(A).into_iter().map(str::to_owned).collect())
    };
    let first = draws()?;
    assert_eq!(first.len(), 51);
    assert_eq!(first, draws()?);
    for line in first.iter().take(50) {
        let (up, down) = line.split_once(' ').ok_or("two draws a line")?;
        let up: f32 = up.parse().map_err(|error| format!("{line}: {error}"))?;
        let down: f32 = down.parse().map_err(|error| format!("{line}: {error}"))?;
        assert!((0.0..10.0).contains(&up), "{line}");
        assert!((-0.5..=0.0).contains(&down), "{line}");
    }
    // Not all one draw.
    assert!(
        first
            .iter()
            .take(50)
            .any(|line| Some(line) != first.first())
    );
    Ok(())
}
