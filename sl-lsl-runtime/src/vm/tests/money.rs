//! Permissions, money and payment held to the grids: `avatar/permissions`,
//! `money/debit`, `money/pay-price` and `money/pay` run in the VM under each
//! flavour, in the routing region of `comms.rs`, with a runner of this test's
//! own that touches, answers, pays and asks for pay buttons as each probe's
//! `waiting` records ask and as the grid's runner did, and checks every
//! `PROBE` line, the `DEBUG_CHANNEL` lines and the questions, payments and
//! pay-button answers the grid's runner noted.
//!
//! The economy is the test's: on Second Life the owner and the other avatar
//! have accounts of L$ 100 and a payment to a prim is taken by
//! [`PayPrice::takes`]; on OpenSim the stock money module refuses no transfer
//! to an account and takes no payment to an object. `money/lapse` is not
//! replayed — it takes controls, attaches, drops and teleports, which are
//! later tranches' — and its rows are unit tests below.

use std::collections::BTreeMap;

use pretty_assertions::assert_eq;

use super::comms::{OTHER, Region, canonical, name_of, note_field, noted_debug};
use super::strings::probe_lines;
use super::*;
use crate::value::NULL_KEY;
use crate::vm::permissions::{
    PERMISSION_CONTROL_CAMERA, PERMISSION_DEBIT, PERMISSION_TAKE_CONTROLS,
    PERMISSION_TRIGGER_ANIMATION,
};

/// How many ticks the runner takes to act on a `waiting` record: half a
/// second, so a script's own work in the same handler and the link messages
/// it sends land first, as they did on the grid.
const RUNNER_TICKS: u64 = 5;

/// What the runner does for one `waiting` record.
#[derive(Debug, Clone)]
enum Drive {
    /// The other avatar touches the probe's script.
    Touch,
    /// `agent` answers whatever question is waiting for it with `grant`.
    Answer { agent: String, grant: i32 },
    /// `agent` pays the prim `prim` `amount` L$, as a viewer's pay dialog.
    Pay {
        agent: String,
        prim: u64,
        amount: i32,
    },
    /// `agent` asks the prim `prim` for its pay buttons.
    PayPrice { agent: String, prim: u64 },
}

/// The prim a `prim=` field names, A when it names none.
fn prim_named(name: Option<&str>) -> u64 {
    match name {
        Some("B") => 2,
        Some("C") => 3,
        _ => 1,
    }
}

/// A probe region and the runner driving it.
struct Runner {
    /// The region.
    region: Region,
    /// The flavour imitated.
    flavour: EngineFlavour,
    /// The script a touch goes to.
    lead: CallerId,
    /// How many `PROBE` lines have been read.
    read: usize,
    /// Drives due, by the tick they are due at.
    due: Vec<(u64, Drive)>,
    /// The ticks run.
    now: u64,
    /// What the runner noted, in its transcript's words: each question, each
    /// pay-button answer, each alert.
    notes: Vec<String>,
    /// How many of the host's questions have been noted.
    asked: usize,
    /// A `PROBE` record that sets a script's start parameter when printed,
    /// and the parameter: how a rigged probe carries a mark across a reset.
    mark: Option<(&'static str, CallerId, i32)>,
}

impl Runner {
    /// An empty region imitating `flavour`, with the test's economy.
    fn new(flavour: EngineFlavour) -> Self {
        let mut region = Region::new(flavour);
        let balance = if flavour.is_opensim() {
            i64::MAX / 4
        } else {
            100
        };
        for agent in [super::OWNER, OTHER] {
            let _none = region.host.accounts.insert(agent.to_owned(), balance);
        }
        Self {
            region,
            flavour,
            lead: CallerId(0),
            read: 0,
            due: Vec::new(),
            now: 0,
            notes: Vec::new(),
            asked: 0,
            mark: None,
        }
    }

    /// Add `source` as `name`, the script `id` in prim `prim`; the first
    /// added is the one touched.
    fn add(&mut self, id: CallerId, name: &str, prim: u64, source: &str) -> Result<(), String> {
        if self.lead == CallerId(0) {
            self.lead = id;
        }
        self.region.add(id, name, prim, source)
    }

    /// Run until a script prints `end`, or `ticks` ticks.
    fn run(&mut self, ticks: u64) -> Result<(), String> {
        for _ in 0..ticks {
            self.now = self.now.saturating_add(1);
            self.region.host.now = self.now;
            self.region.tick()?;
            self.note_questions();
            self.schedule()?;
            self.act()?;
            if self.region.ended() {
                break;
            }
        }
        assert!(
            self.region.host.stubbed.is_empty(),
            "{:?}",
            self.region.host.stubbed
        );
        Ok(())
    }

    /// Note each question put since the last tick, as the grid's runner did.
    fn note_questions(&mut self) {
        let questions = self
            .region
            .host
            .questions
            .get(self.asked..)
            .unwrap_or_default();
        for (_, _, question) in questions {
            let by = if question.agent == OTHER {
                " by=other"
            } else {
                ""
            };
            self.notes.push(format!(
                "# question{by} permissions={} object=\"Object\" owner=\"<first> <last>\" \
                 experience=none",
                question.mask
            ));
        }
        self.asked = self.region.host.questions.len();
    }

    /// Read the `PROBE` lines printed since the last tick and schedule what
    /// each `waiting` record asks of the runner.
    fn schedule(&mut self) -> Result<(), String> {
        let lines = self.region.lines();
        for line in lines.get(self.read..).unwrap_or_default() {
            let parsed = crate::probe::Line::parse(line).map_err(|error| error.to_string())?;
            if let Some((rec, id, parameter)) = self.mark
                && parsed.rec == "set"
                && parsed.field("what") == Some(rec)
                && let Some(instance) = self.region.engine.instance_mut(id)
            {
                instance.set_start_parameter(parameter);
            }
            if parsed.rec == "removed"
                && let Some(id) = self.region.host.scripts.get("Second till").copied()
            {
                // `llRemoveInventory(llGetScriptName())`, rigged out of the
                // script: the region removes it.
                let _removed = self.region.engine.remove(id);
                let _gone = self.region.host.prims.remove(&id);
            }
            if parsed.rec != "waiting" {
                continue;
            }
            let agent = if parsed.field("by") == Some("other") {
                OTHER
            } else {
                super::OWNER
            }
            .to_owned();
            let number = |name: &str| -> Result<i32, String> {
                parsed
                    .field(name)
                    .unwrap_or_default()
                    .parse::<i32>()
                    .map_err(|error| format!("{line}: {error}"))
            };
            let drive = match parsed.field("drive") {
                Some("touch") => Drive::Touch,
                Some("answer") => Drive::Answer {
                    agent,
                    grant: number("grant")?,
                },
                Some("pay") => Drive::Pay {
                    agent,
                    prim: prim_named(parsed.field("prim")),
                    amount: number("amount")?,
                },
                Some("pay-price") => Drive::PayPrice {
                    agent,
                    prim: prim_named(parsed.field("prim")),
                },
                other => return Err(format!("no drive {other:?}: {line}")),
            };
            self.due
                .push((self.now.saturating_add(RUNNER_TICKS), drive));
        }
        self.read = lines.len();
        Ok(())
    }

    /// Carry out every drive due now.
    fn act(&mut self) -> Result<(), String> {
        let now = self.now;
        let (ready, waiting): (Vec<_>, Vec<_>) =
            self.due.drain(..).partition(|(tick, _)| *tick <= now);
        self.due = waiting;
        for (_, drive) in ready {
            match drive {
                Drive::Touch => {
                    let touch = Detected {
                        key: OTHER.to_owned(),
                        name: name_of(OTHER).to_owned(),
                        owner: OTHER.to_owned(),
                        kind: 1,
                        ..Detected::blank()
                    };
                    let _posted = self
                        .region
                        .engine
                        .post_detected(self.lead, named("touch_start")?, vec![touch])
                        .map_err(|error| error.to_string())?;
                }
                Drive::Answer { agent, grant } => {
                    for id in self.scripts() {
                        let _posted = self
                            .region
                            .engine
                            .answer(id, &agent, grant)
                            .map_err(|error| error.to_string())?;
                    }
                }
                Drive::Pay {
                    agent,
                    prim,
                    amount,
                } => self.pay(&agent, prim, amount)?,
                Drive::PayPrice { agent, prim } => {
                    let by = if agent == OTHER { " by=other" } else { "" };
                    let letter = match prim {
                        2 => "B",
                        3 => "C",
                        _ => "A",
                    };
                    let layout = self
                        .region
                        .host
                        .pay_prices
                        .get(&prim)
                        .copied()
                        .unwrap_or_default();
                    self.notes.push(match layout.reply(self.flavour) {
                        Some(reply) => format!(
                            "# pay-price{by} prim={letter} default={} buttons=\"{}\"",
                            reply.price,
                            reply
                                .buttons
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        None => format!("# pay-price{by} prim={letter} response=none"),
                    });
                }
            }
        }
        Ok(())
    }

    /// `agent` pays the prim `prim` `amount`: on Second Life every script
    /// still in it hears a payment its layout takes, and the payer is alerted
    /// of one it refuses; OpenSim's stock module takes nothing.
    fn pay(&mut self, agent: &str, prim: u64, amount: i32) -> Result<(), String> {
        if self.flavour.is_opensim() {
            return Ok(());
        }
        let layout = self
            .region
            .host
            .pay_prices
            .get(&prim)
            .copied()
            .unwrap_or_default();
        match layout.takes(amount) {
            PaymentAdmission::Taken => {
                let in_prim: Vec<CallerId> = self
                    .region
                    .host
                    .prims
                    .iter()
                    .filter(|(_, at)| **at == prim)
                    .map(|(id, _)| *id)
                    .collect();
                for id in in_prim {
                    let _posted = self
                        .region
                        .engine
                        .paid(id, agent, amount)
                        .map_err(|error| error.to_string())?;
                }
            }
            PaymentAdmission::Refused => {
                let by = if agent == OTHER { " by=other" } else { "" };
                self.notes
                    .push(format!("# alert{by} text=\"{PAYMENT_STOPPED}\""));
            }
            PaymentAdmission::Dropped => {}
        }
        Ok(())
    }

    /// Every script still in the region.
    fn scripts(&self) -> Vec<CallerId> {
        self.region.host.prims.keys().copied().collect()
    }
}

/// The notes of `transcript` this runner makes too: each question, each
/// pay-button answer and each alert, in order.
fn noted(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter(|line| {
            line.starts_with("# question")
                || line.starts_with("# pay-price")
                || line.starts_with("# alert")
        })
        .map(str::to_owned)
        .collect()
}

/// The three grids' transcripts of the probe `name` (`avatar/permissions` or
/// one of `money/*`).
fn transcripts(name: &str) -> [(EngineFlavour, &'static str); 3] {
    let files = match name {
        "permissions" => [
            include_str!("../../../probes/results/aditi/avatar/permissions.txt"),
            include_str!("../../../probes/results/opensim-yengine/avatar/permissions.txt"),
            include_str!("../../../probes/results/opensim-xengine/avatar/permissions.txt"),
        ],
        "debit" => [
            include_str!("../../../probes/results/aditi/money/debit.txt"),
            include_str!("../../../probes/results/opensim-yengine/money/debit.txt"),
            include_str!("../../../probes/results/opensim-xengine/money/debit.txt"),
        ],
        "pay-price" => [
            include_str!("../../../probes/results/aditi/money/pay-price.txt"),
            include_str!("../../../probes/results/opensim-yengine/money/pay-price.txt"),
            include_str!("../../../probes/results/opensim-xengine/money/pay-price.txt"),
        ],
        _ => [
            include_str!("../../../probes/results/aditi/money/pay.txt"),
            include_str!("../../../probes/results/opensim-yengine/money/pay.txt"),
            include_str!("../../../probes/results/opensim-xengine/money/pay.txt"),
        ],
    };
    let [aditi, yengine, xengine] = files;
    [
        (EngineFlavour::SecondLife, aditi),
        (EngineFlavour::YEngine, yengine),
        (EngineFlavour::XEngine, xengine),
    ]
}

/// What Second Life says of the avatar-control calls `avatar/permissions`
/// makes before anything is granted. Those calls are
/// `server-lsl-lib-avatar-control`'s, rigged out of the probe here, so the
/// lines are not said.
const AVATAR_CONTROL_COMPLAINTS: [&str; 3] = [
    "Script trying to take controls from owner but PERMISSION_TAKE_CONTROLS permission not set!",
    "Script trying to trigger animations but PERMISSION_TRIGGER_ANIMATION permission not set",
    "Script trying to set camera parameters but PERMISSION_CONTROL_CAMERA permission not set!",
];

#[test]
fn avatar_permissions_asks_answers_and_refuses_as_each_grid_did() -> Result<(), String> {
    let source = include_str!("../../../probes/avatar/permissions.lsl")
        .replace("llTakeControls(CONTROL_FWD, TRUE, FALSE);", "")
        .replace("llStartAnimation(\"dance1\");", "")
        .replace("llSetCameraParams([CAMERA_ACTIVE, TRUE]);", "")
        .replace("llGetCameraPos() == ZERO_VECTOR", "TRUE");
    for (flavour, transcript) in transcripts("permissions") {
        let mut runner = Runner::new(flavour);
        runner.add(A, "Probe", 1, &source)?;
        runner.run(2_000)?;
        assert_eq!(
            canonical(&runner.region.lines(), &[]),
            canonical(&probe_lines(transcript), &[]),
            "{flavour:?}"
        );
        assert_eq!(runner.notes, noted(transcript), "{flavour:?}");
        let said: Vec<String> = noted_debug(transcript)
            .into_iter()
            .filter(|line| !AVATAR_CONTROL_COMPLAINTS.contains(&line.as_str()))
            .collect();
        assert_eq!(runner.region.debug_lines(), said, "{flavour:?}");
    }
    Ok(())
}

#[test]
fn money_debit_gives_transfers_and_refuses_as_each_grid_did() -> Result<(), String> {
    for (flavour, transcript) in transcripts("debit") {
        let mut runner = Runner::new(flavour);
        runner.add(
            A,
            "Probe",
            1,
            include_str!("../../../probes/money/debit.lsl"),
        )?;
        runner.run(3_000)?;
        assert_eq!(
            canonical(&runner.region.lines(), &[]),
            canonical(&probe_lines(transcript), &[]),
            "{flavour:?}"
        );
        assert_eq!(runner.notes, noted(transcript), "{flavour:?}");
        assert_eq!(
            runner.region.debug_lines(),
            noted_debug(transcript),
            "{flavour:?}"
        );
        // What was handed to the economy: Second Life's refusals of
        // `llGiveMoney` never reach it, OpenSim's module is given every
        // amount; each transfer that moved is one the grid's runner saw
        // succeed.
        let moved = |payments: &[(u64, CallerId, Payment)]| -> Vec<(String, i32)> {
            payments
                .iter()
                .map(|(_, _, payment)| {
                    let to = if payment.to == OTHER {
                        "other"
                    } else {
                        "owner"
                    };
                    (to.to_owned(), payment.amount)
                })
                .collect()
        };
        let (given, transferred) = if flavour.is_opensim() {
            (
                vec![("other", 1), ("other", 0), ("other", -1)],
                vec![("other", 1), ("owner", 1), ("other", 1_000_000_000)],
            )
        } else {
            (vec![("other", 1)], vec![("other", 1), ("owner", 1)])
        };
        let owned = |list: Vec<(&str, i32)>| -> Vec<(String, i32)> {
            list.into_iter()
                .map(|(to, amount)| (to.to_owned(), amount))
                .collect()
        };
        assert_eq!(
            moved(&runner.region.host.given),
            owned(given),
            "{flavour:?}"
        );
        assert_eq!(
            moved(&runner.region.host.transferred),
            owned(transferred),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn money_pay_price_answers_the_buttons_each_grid_answered() -> Result<(), String> {
    let leader = include_str!("../../../probes/money/pay-price/leader.lsl")
        .replace(
            "llSetObjectDesc(MARK);\n        llResetScript();",
            "llSleep(1.0);\n        llResetScript();",
        )
        .replace("llGetObjectDesc() == MARK", "llGetStartParameter() == 13")
        .replace("llSetObjectDesc(\"\");", "");
    for (flavour, transcript) in transcripts("pay-price") {
        let mut runner = Runner::new(flavour);
        let leader_id = CallerId(3);
        runner.add(leader_id, "Leader", 1, &leader)?;
        runner.add(
            CallerId(1),
            "No money",
            2,
            include_str!("../../../probes/money/pay-price/no-money.lsl"),
        )?;
        runner.add(
            CallerId(2),
            "Mixed",
            3,
            include_str!("../../../probes/money/pay-price/mixed.lsl"),
        )?;
        // The prim's description carries the mark across the reset on the
        // grid; here the start parameter does, which a reset keeps.
        runner.mark = Some(("before-reset", leader_id, 13));
        runner.run(3_000)?;
        assert_eq!(
            canonical(&runner.region.lines(), &[]),
            canonical(&probe_lines(transcript), &[]),
            "{flavour:?}"
        );
        assert_eq!(runner.notes, noted(transcript), "{flavour:?}");
        match flavour {
            EngineFlavour::SecondLife => assert!(runner.region.debug_lines().is_empty()),
            EngineFlavour::XEngine => {
                let [line] = runner
                    .region
                    .debug_lines()
                    .try_into()
                    .map_err(|lines| format!("{lines:?}"))?;
                assert!(
                    transcript.contains(&format!("text=\"{line}\\n  at ")),
                    "{line}"
                );
            }
            EngineFlavour::YEngine => {
                assert!(runner.region.debug_lines().is_empty());
                let told: Vec<String> = runner
                    .region
                    .host
                    .told
                    .iter()
                    .map(|(_, _, text)| text.clone())
                    .collect();
                let noted_told: Vec<String> = transcript
                    .lines()
                    .filter(|line| line.starts_with("# owner-say:"))
                    .filter_map(|line| note_field(line, "text"))
                    .filter(|text| !text.starts_with("  at "))
                    .collect();
                // The item's key and the prim's place are the host's.
                let shape = |lines: &[String]| -> Vec<String> {
                    lines
                        .iter()
                        .map(|line| {
                            line.split(" running ")
                                .next()
                                .unwrap_or_default()
                                .to_owned()
                        })
                        .map(|line| {
                            line.split(", Location:")
                                .next()
                                .unwrap_or_default()
                                .to_owned()
                        })
                        .collect()
                };
                assert_eq!(
                    shape(&told),
                    shape(&noted_told)
                        .into_iter()
                        .map(|line| line.replace("Prim: <Object>", "Prim: <Faulty prim>"))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn money_pay_raises_money_for_the_payments_each_grid_took() -> Result<(), String> {
    for (flavour, transcript) in transcripts("pay") {
        let mut runner = Runner::new(flavour);
        runner.add(
            A,
            "Till",
            1,
            include_str!("../../../probes/money/pay/till.lsl"),
        )?;
        runner.add(
            B,
            "Second till",
            1,
            &include_str!("../../../probes/money/pay/second-till.lsl")
                .replace("llRemoveInventory(llGetScriptName());", ""),
        )?;
        runner.run(4_000)?;
        // Both scripts hear one payment; on aditi in either order.
        assert_eq!(
            canonical(&runner.region.lines(), &["money"]),
            canonical(&probe_lines(transcript), &["money"]),
            "{flavour:?}"
        );
        assert_eq!(runner.notes, noted(transcript), "{flavour:?}");
        assert!(runner.region.debug_lines().is_empty(), "{flavour:?}");
        let refund: Vec<i32> = runner
            .region
            .host
            .given
            .iter()
            .map(|(_, _, payment)| payment.amount)
            .collect();
        let expected = if flavour.is_opensim() {
            Vec::new()
        } else {
            vec![23]
        };
        assert_eq!(refund, expected, "{flavour:?}");
    }
    Ok(())
}

/// A script that asks the owner for `mask` when it starts (and after a
/// reset, not after a state change), and prints what it holds whenever it
/// is told to, and each `run_time_permissions`.
fn holder(mask: i32) -> String {
    format!(
        "integer asked; default {{ state_entry() {{ if (!asked) {{ asked = TRUE; \
         llRequestPermissions(\"{OWNER}\", {mask}); }} }} \
         run_time_permissions(integer p) {{ print(\"answer \" + (string)p); }} \
         link_message(integer s, integer n, string m, key k) {{ \
         if (m == \"show\") {{ print((string)llGetPermissions() + \" \" + (string)llGetPermissionsKey()); }} \
         else if (m == \"away\") {{ state other; }} else if (m == \"reset\") {{ llResetScript(); }} }} }} \
         state other {{ state_entry() {{ state default; }} }}"
    )
}

/// Send the instance `id` the link message `text`, and run a second.
fn tell(engine: &mut Engine, host: &mut Recorder, id: CallerId, text: &str) -> Result<(), String> {
    let _posted = engine.post(
        id,
        named("link_message")?,
        vec![
            Value::Integer(0),
            Value::Integer(0),
            Value::String(text.to_owned()),
            Value::Key(String::new()),
        ],
    );
    let _reports = run(engine, host, 10);
    Ok(())
}

/// `money/lapse`'s rows: a state change keeps the grant; a viewer's
/// revocation drops what it names, of the object or the region, and every
/// bit drops the granter; a reset drops everything — and no lapse raises
/// `run_time_permissions`. After the reset the script asks again: OpenSim
/// names the owner the granter at once, holding nothing until the answer.
#[test]
fn a_grant_lapses_only_as_measured() -> Result<(), String> {
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::XEngine] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program_for(&holder(1046), flavour)?);
        let _reports = run(&mut engine, &mut host, 2);
        assert_eq!(engine.answer(A, OWNER, 1046), Ok(Some(Posted::Queued)));
        let _reports = run(&mut engine, &mut host, 2);
        tell(&mut engine, &mut host, A, "show")?;
        tell(&mut engine, &mut host, A, "away")?;
        tell(&mut engine, &mut host, A, "show")?;
        engine.revoke(A, OTHER, PERMISSION_TRIGGER_ANIMATION);
        engine.revoke(A, OWNER, PERMISSION_TRIGGER_ANIMATION);
        tell(&mut engine, &mut host, A, "show")?;
        engine.revoke(A, OWNER, -1);
        tell(&mut engine, &mut host, A, "show")?;
        // Nothing is asked now, so an answer is nothing.
        assert_eq!(engine.answer(A, OWNER, 1046), Ok(None));
        tell(&mut engine, &mut host, A, "reset")?;
        tell(&mut engine, &mut host, A, "show")?;
        let held = format!("1046 {OWNER}");
        let after_reset = if flavour.is_opensim() {
            format!("0 {OWNER}")
        } else {
            format!("0 {NULL_KEY}")
        };
        assert_eq!(
            host.texts(A),
            vec![
                "answer 1046".to_owned(),
                held.clone(),
                held,
                format!("1030 {OWNER}"),
                format!("0 {NULL_KEY}"),
                after_reset,
            ],
            "{flavour:?}"
        );
    }
    Ok(())
}

/// What the object is granted unasked: the avatar-control permissions of an
/// avatar sitting on it or wearing it, at once and with `run_time_permissions`;
/// `PERMISSION_DEBIT` is asked even of the owner wearing it.
#[test]
fn a_seated_or_wearing_avatar_grants_avatar_control_unasked() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let ask = |agent: &str, mask: i32| {
        format!(
            "default {{ state_entry() {{ llRequestPermissions(\"{agent}\", {mask}); }} \
             run_time_permissions(integer p) {{ \
             print((string)p + \" \" + (string)llGetPermissionsKey()); }} }}"
        )
    };
    let seated = PERMISSION_TRIGGER_ANIMATION | PERMISSION_CONTROL_CAMERA;
    let worn = PERMISSION_TAKE_CONTROLS | PERMISSION_DEBIT;
    let _none = engine.add(A, program(&ask(OTHER, seated))?);
    let _none = engine.add(B, program(&ask(OWNER, worn))?);
    let _old = host.seated.insert(A, OTHER.to_owned());
    let _old = host.worn.insert(B, OWNER.to_owned());
    let _reports = run(&mut engine, &mut host, 5);
    assert_eq!(host.texts(A), vec![format!("{seated} {OTHER}")]);
    assert!(host.texts(B).is_empty());
    assert_eq!(
        host.questions
            .iter()
            .map(|(_, id, question)| (*id, question.agent.as_str(), question.mask))
            .collect::<Vec<_>>(),
        vec![(B, OWNER, worn)]
    );
    Ok(())
}

/// OpenSim refuses a question for an avatar not in the region at once with
/// `run_time_permissions(0)`; Second Life puts nothing and raises nothing.
#[test]
fn a_question_for_an_absent_avatar_is_refused_on_opensim_only() -> Result<(), String> {
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _new = host.away.insert(OTHER.to_owned());
        let _none = engine.add(
            A,
            program_for(
                &format!(
                    "default {{ state_entry() {{ llRequestPermissions(\"{OTHER}\", 16); }} \
                     run_time_permissions(integer p) {{ print((string)p); }} }}"
                ),
                flavour,
            )?,
        );
        let _reports = run(&mut engine, &mut host, 5);
        let expected: Vec<&str> = if flavour.is_opensim() {
            vec!["0"]
        } else {
            Vec::new()
        };
        assert_eq!(host.texts(A), expected, "{flavour:?}");
        assert!(host.questions.is_empty());
    }
    Ok(())
}

/// A transfer's handle is a fresh key each call, and its `transaction_result`
/// comes after the call's handler — unless the handler resets, which drops
/// it with the rest of the queue.
#[test]
fn a_transfer_answers_with_its_own_handle_after_the_handler() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "key one; default { state_entry() { one = llTransferLindenDollars(NULL_KEY, 1); \
             key two = llTransferLindenDollars(NULL_KEY, 1); print((string)(one != two)); \
             print((string)(llStringLength((string)one) == 36)); } \
             transaction_result(key id, integer ok, string data) { \
             print((string)(id == one) + \" \" + data); } }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 5);
    assert_eq!(
        host.texts(A),
        vec![
            "1",
            "1",
            "1 MISSING_PERMISSION_DEBIT",
            "0 MISSING_PERMISSION_DEBIT"
        ]
    );
    Ok(())
}

/// The acceptance's vendor: a prim that hides the price field and offers
/// two buttons hears what it is paid; a script calling `llGiveMoney` without
/// `PERMISSION_DEBIT` fails and says so on `DEBUG_CHANNEL`.
#[test]
fn a_vendor_hears_a_button_payment_and_cannot_give_unpermitted() -> Result<(), String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(
        A,
        program(
            "default { state_entry() { llSetPayPrice(PAY_HIDE, [10, 20]); } \
             money(key id, integer amount) { print((string)amount); \
             print((string)llGiveMoney(id, 5)); } }",
        )?,
    );
    let _reports = run(&mut engine, &mut host, 2);
    let layout = host.pay_prices.get(&A.0).copied().unwrap_or_default();
    assert_eq!(
        layout,
        PayPrice {
            price: PAY_HIDE,
            buttons: [10, 20, PAY_HIDE, PAY_HIDE],
        }
    );
    for amount in [20, 15] {
        if layout.takes(amount) == PaymentAdmission::Taken {
            let _posted = engine.paid(A, OTHER, amount);
        }
    }
    let _reports = run(&mut engine, &mut host, 5);
    assert_eq!(host.texts(A), vec!["20", "0"]);
    assert!(host.given.is_empty());
    assert_eq!(
        host.heard(A),
        vec!["Script trying to debit L$ from owner but PERMISSION_DEBIT permission not set!"]
    );
    Ok(())
}

/// OpenSim pads the button list a script passes in place, so the script's
/// own variable — a local or a global — reads the padding after the call;
/// Second Life leaves it.
#[test]
fn opensim_pads_the_scripts_own_button_list() -> Result<(), String> {
    let source = "list g = [1]; default { state_entry() { list l = [1, 2]; \
                  llSetPayPrice(7, l); llSetPayPrice(7, g); \
                  print(llList2CSV(l) + \"|\" + llList2CSV(g)); } }";
    let mut layouts = BTreeMap::new();
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let mut engine = engine_as(flavour);
        let mut host = Recorder::default();
        let _none = engine.add(A, program_for(source, flavour)?);
        let _reports = run(&mut engine, &mut host, 2);
        let _old = layouts.insert(format!("{flavour:?}"), host.pay_prices.get(&A.0).copied());
        let expected = if flavour.is_opensim() {
            "1, 2, -1, -1, -1|1, -1, -1, -1, -1"
        } else {
            "1, 2|1"
        };
        assert_eq!(host.texts(A), vec![expected], "{flavour:?}");
    }
    assert_eq!(
        layouts,
        BTreeMap::from([
            (
                "SecondLife".to_owned(),
                Some(PayPrice {
                    price: 7,
                    buttons: [1, PAY_HIDE, PAY_HIDE, PAY_HIDE],
                })
            ),
            (
                "YEngine".to_owned(),
                Some(PayPrice {
                    price: 7,
                    buttons: [1, PAY_HIDE, PAY_HIDE, PAY_HIDE],
                })
            ),
        ])
    );
    Ok(())
}
