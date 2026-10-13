//! A census of local chat across a region border: whether a line said in one
//! region is heard in the next, how far, and what is said of it there.
//!
//! [`super::local_chat`] asks what a region passes on of a line to the
//! avatars standing in it. This case stands its two avatars either side of a
//! border — avatar **A** walks over it, avatar **B** stays — and asks the
//! same of a whisper, a say, a shout and the typing notices, each way, at
//! three distances: a few metres apart, past a say's reach and past a
//! shout's.
//!
//! A hearer in the next region is a *child agent* of the speaker's: the
//! speaker's region knows of it only through the child circuit the hearer's
//! viewer holds there. So besides who hears what, the case records where a
//! line says it was said — in the speaker's region's coordinates or in the
//! hearer's.
//!
//! How A gets over the border depends on the grid, as in
//! [`super::region_crossing`]: a live grid has it fly there, the fake grid is
//! asked for the crossing and for every step after it.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/chat.md` § Across a border; every run is held to it.
//!
//! `2av`, `[both]`, offline on both fake flavours.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, Camera, ChatChannel, ChatMessage, ChatSource, ChatType, Command, ControlFlags,
    Diagnostic, Distance, Event, RegionCoordinates, RegionHandle, Uuid, Vector,
};

use crate::cases::effects_sounds::vector_text;
use crate::cases::local_chat::{
    BETWEEN, Got, LINE_TAG, Typed, audible_text, text_text, type_text, watch, watch_for,
};
use crate::cases::object_rez_derez::{Answer, Pace, STEP_TIMEOUT};
use crate::cases::task_inventory::{DRAIN, FAKE_DRAIN, hold_to_measured};
use crate::context::{Session, TestContext, TestFailure};
use crate::crossing::{Edge, Steer, Walk, nearest_border, walk_over_border, watch_neighbours};
use crate::fake::FakeControl;
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{Gait, REGION_TIMEOUT, is_opensim, steer_towards};

/// Where both avatars log in on OpenSim: twelve metres from the eastern
/// border of the block's south-western region.
const OPENSIM_START: &str = "uri:Default Region&244&128&25";

/// How long to watch A's arrival for its neighbour announcements.
const NEIGHBOUR_WINDOW: Duration = Duration::from_secs(30);

/// The gap without a new announcement that ends the watch.
const NEIGHBOUR_QUIET: Duration = Duration::from_secs(6);

/// How long a flight may take to reach its border, or its spot.
const WALK_BUDGET: Duration = Duration::from_secs(120);

/// How long A keeps flying after the crossing, to get clear of the border.
const CARRY_ON: Duration = Duration::from_millis(500);

/// How long a crossing is watched after A stopped.
const SETTLE: Duration = Duration::from_secs(5);

/// How far from the border the fake grid stands each avatar for the first
/// step, in metres.
const AT_THE_BORDER_M: f32 = 3.0;

/// How far inside its region the spot is that an avatar flies at to stand by
/// the border, in metres. The flight ends about four metres short of its
/// spot and the avatar then drifts on a metre or two: a spot nearer the
/// border than this had it carried over (both grids, 2026-10-13).
const BY_THE_BORDER_M: f32 = 3.0;

/// How far a whisper carries on both grids, in metres
/// (`book/src/gridspec/chat.md`).
const WHISPER_M: f32 = 10.0;

/// How far above where A logged in both avatars hover, in metres. The ground
/// either side of a border need not be level, and a line's reach is measured
/// in all three dimensions; and whatever stands by a border stands in the
/// way of a flight just above the ground (something by Ahern's western
/// border on aditi stopped one at forty-five metres up).
const HOVER_ABOVE_M: f32 = 25.0;

/// How far from the height it is to hover at an avatar may be, in metres.
const LEVEL_SLACK_M: f32 = 1.0;

/// How long an avatar may take to reach the height it is to hover at.
const LEVEL_BUDGET: Duration = Duration::from_secs(60);

/// The speed a climb to a height is planned at, in metres a second: what
/// Second Life lifts a flying avatar at.
const LIFT_M_PER_S: f32 = 16.0;

/// How long an avatar is lifted for to have the grid say where it is.
const LIFT: Duration = Duration::from_millis(300);

/// How long an avatar hovers after a move before its position is read.
const HOVER: Duration = Duration::from_secs(3);

/// The draw distance a session states for a moment to have its region tell
/// the neighbours where its avatar stands, in metres: more than 32 m off the
/// usual one, and still reaching over the border the avatar stands at.
const NARROWER_VIEW_M: f64 = 200.0;

/// The draw distance a session states otherwise, in metres.
const USUAL_VIEW_M: f64 = 256.0;

/// How long a stated draw distance is left to take effect.
const VIEW_SETTLE: Duration = Duration::from_millis(1500);

/// How far from where its source stands a line may be stated and still be
/// stated there, in metres.
const AT_M: f32 = 2.5;

/// The width of a region, in metres.
const REGION_M: f32 = 256.0;

/// Where in the destination region the fake grid's scripted crossing lands
/// A: just over the shared edge, halfway along it, on the stock ground.
const FAKE_ARRIVAL: (f32, f32, f32) = (3.0, 128.0, 26.0);

/// One step of A's way into the next region.
struct Step {
    /// The word the step's metrics begin with.
    word: &'static str,
    /// How far inside the next region A stands, in metres; [`None`] for where
    /// its creep back to the border left it.
    inside: Option<f32>,
    /// The distances between the two avatars the step's answers hold for, in
    /// metres.
    band: (f32, f32),
}

/// The steps of A's way into the next region: within a say's reach of B,
/// past it, past a shout's. Where exactly a flight comes to rest by a border
/// is not the case's to choose, so the first step is held to a say's reach
/// and a whisper there to whether the avatars are within ten metres.
const STEPS: [Step; 3] = [
    Step {
        word: "beside",
        inside: None,
        band: (0.5, 19.5),
    },
    Step {
        word: "further",
        inside: Some(32.0),
        band: (20.5, 95.0),
    },
    Step {
        word: "far",
        inside: Some(115.0),
        band: (100.5, 250.0),
    },
];

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/chat.md § Across a border (chat-border, 2026-10-13)";

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to pass on over a border, by the metric the
/// census records it under: every run, live or fake, is held to it.
///
/// Not held on Second Life: the typing notices past a say's reach. Within a
/// region it sends them marked barely audible; over a border one direction
/// of one run in four brought them, and the rest nothing.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "beside_say_of_b_told_a",
        each(
            Answer::Text(
                "say from the agent avatar B, owner avatar B, B's name, fully audible, at its source in the hearer's region's coordinates, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar B, owner avatar B, B's name, fully audible, at its source in its own region's coordinates, text as sent",
            ),
        ),
    ),
    (
        "beside_say_told_b",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source in the hearer's region's coordinates, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source in its own region's coordinates, text as sent",
            ),
        ),
    ),
    (
        "beside_shout_of_b_told_a",
        each(
            Answer::Text(
                "shout from the agent avatar B, owner avatar B, B's name, fully audible, at its source in the hearer's region's coordinates, text as sent",
            ),
            Answer::Text(
                "shout from the agent avatar B, owner avatar B, B's name, fully audible, at its source in its own region's coordinates, text as sent",
            ),
        ),
    ),
    (
        "beside_shout_told_b",
        each(
            Answer::Text(
                "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source in the hearer's region's coordinates, text as sent",
            ),
            Answer::Text(
                "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source in its own region's coordinates, text as sent",
            ),
        ),
    ),
    (
        "beside_typing_start_of_b_told_a",
        each(
            Answer::Text(
                "start typing from avatar B, B's name, fully audible, at its source in the hearer's region's coordinates",
            ),
            Answer::Text(
                "start typing from avatar B, B's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "beside_typing_start_told_b",
        each(
            Answer::Text(
                "start typing from avatar A, A's name, fully audible, at its source in the hearer's region's coordinates",
            ),
            Answer::Text(
                "start typing from avatar A, A's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "beside_typing_stop_of_b_told_a",
        each(
            Answer::Text(
                "stop typing from avatar B, B's name, fully audible, at its source in the hearer's region's coordinates",
            ),
            Answer::Text(
                "stop typing from avatar B, B's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "beside_typing_stop_told_b",
        each(
            Answer::Text(
                "stop typing from avatar A, A's name, fully audible, at its source in the hearer's region's coordinates",
            ),
            Answer::Text(
                "stop typing from avatar A, A's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "beside_whisper_heard_just_within_its_reach",
        both(Answer::Flag(true)),
    ),
    (
        "beside_whisper_of_b_heard_just_within_its_reach",
        both(Answer::Flag(true)),
    ),
    (
        "chat_unread_on_a_child_circuit_by_a",
        both(Answer::Count(0)),
    ),
    (
        "chat_unread_on_a_child_circuit_by_b",
        both(Answer::Count(0)),
    ),
    ("far_say_of_b_told_a", both(Answer::Text("nothing"))),
    ("far_say_told_b", both(Answer::Text("nothing"))),
    ("far_shout_of_b_told_a", both(Answer::Text("nothing"))),
    ("far_shout_told_b", both(Answer::Text("nothing"))),
    (
        "far_typing_start_of_b_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "start typing from avatar B, B's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "far_typing_start_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "start typing from avatar A, A's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "far_typing_stop_of_b_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "stop typing from avatar B, B's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "far_typing_stop_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "stop typing from avatar A, A's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    ("far_whisper_of_b_told_a", both(Answer::Text("nothing"))),
    ("far_whisper_told_b", both(Answer::Text("nothing"))),
    ("further_say_of_b_told_a", both(Answer::Text("nothing"))),
    ("further_say_told_b", both(Answer::Text("nothing"))),
    (
        "further_shout_of_b_told_a",
        each(
            Answer::Text(
                "shout from the agent avatar B, owner avatar B, B's name, fully audible, at its source in the hearer's region's coordinates, text as sent",
            ),
            Answer::Text(
                "shout from the agent avatar B, owner avatar B, B's name, fully audible, at its source in its own region's coordinates, text as sent",
            ),
        ),
    ),
    (
        "further_shout_told_b",
        each(
            Answer::Text(
                "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source in the hearer's region's coordinates, text as sent",
            ),
            Answer::Text(
                "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source in its own region's coordinates, text as sent",
            ),
        ),
    ),
    (
        "further_typing_start_of_b_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "start typing from avatar B, B's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "further_typing_start_told_b",
        each(
            Answer::Any,
            Answer::Text(
                "start typing from avatar A, A's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "further_typing_stop_of_b_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "stop typing from avatar B, B's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    (
        "further_typing_stop_told_b",
        each(
            Answer::Any,
            Answer::Text(
                "stop typing from avatar A, A's name, fully audible, at its source in its own region's coordinates",
            ),
        ),
    ),
    ("further_whisper_of_b_told_a", both(Answer::Text("nothing"))),
    ("further_whisper_told_b", both(Answer::Text("nothing"))),
];

/// A point of a region.
#[derive(Debug, Clone)]
struct Spot {
    /// The region.
    region: RegionHandle,
    /// Where in it.
    at: Vector,
}

impl Spot {
    /// How many metres east and north of the region `of` this spot's region
    /// begins.
    fn offset_from(&self, of: RegionHandle) -> (f32, f32) {
        let (east, north) = crate::crossing::slot_offset(of, self.region);
        // A slot offset between neighbours is one of -1, 0 and 1.
        let metres = |slots: i64| match slots {
            0 => 0.0,
            slots if slots > 0 => REGION_M,
            _ => -REGION_M,
        };
        (metres(east), metres(north))
    }

    /// The spot in the coordinates of the region `of`.
    fn seen_from(&self, of: RegionHandle) -> Vector {
        let (east, north) = self.offset_from(of);
        Vector {
            x: self.at.x + east,
            y: self.at.y + north,
            z: self.at.z,
        }
    }
}

/// The distance between two points, height included.
fn apart(left: &Vector, right: &Vector) -> f32 {
    crate::cases::effects_sounds::distance(left, right)
}

/// The ids, names and places a metric names by what they are.
#[derive(Debug, Clone)]
struct Scene {
    /// Avatar A: crosses the border.
    a: Uuid,
    /// Avatar B: stays.
    b: Uuid,
    /// The name A logged in under.
    a_name: String,
    /// The name B logged in under.
    b_name: String,
    /// Where A stands.
    a_at: Spot,
    /// Where B stands.
    b_at: Spot,
}

impl Scene {
    /// Whose id `id` is.
    fn who(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else {
            "somebody else"
        }
    }

    /// Whose name `name` is.
    fn whose_name(&self, name: &str) -> &'static str {
        if name.is_empty() {
            "no name"
        } else if name.eq_ignore_ascii_case(&self.a_name) {
            "A's name"
        } else if name.eq_ignore_ascii_case(&self.b_name) {
            "B's name"
        } else {
            "another name"
        }
    }

    /// Where the avatar `id` stands, and where the other one does.
    fn spots(&self, id: Uuid) -> Option<(&Spot, &Spot)> {
        if id == self.a {
            Some((&self.a_at, &self.b_at))
        } else if id == self.b {
            Some((&self.b_at, &self.a_at))
        } else {
            None
        }
    }

    /// How far apart the two avatars are, in metres.
    fn distance(&self) -> f32 {
        apart(&self.a_at.at, &self.b_at.seen_from(self.a_at.region))
    }

    /// Where a line of `source` is stated to have been said: in which
    /// region's coordinates it is where its source stands.
    fn place(&self, position: &RegionCoordinates, source: Uuid) -> &'static str {
        let at = Vector {
            x: position.x(),
            y: position.y(),
            z: position.z(),
        };
        let Some((speaker, hearer)) = self.spots(source) else {
            return "elsewhere";
        };
        if apart(&at, &speaker.at) < AT_M {
            "at its source in its own region's coordinates"
        } else if apart(&at, &speaker.seen_from(hearer.region)) < AT_M {
            "at its source in the hearer's region's coordinates"
        } else {
            "elsewhere"
        }
    }

    /// What `got` holds of the line that begins with `tag`, where `sent` was
    /// said: `nothing`, or the first such line — and how many, where that is
    /// not one.
    fn told(&self, got: &Got, tag: &str, sent: &str) -> String {
        let lines: Vec<&ChatMessage> = got
            .lines
            .iter()
            .filter(|line| line.message.starts_with(tag))
            .collect();
        let Some(first) = lines.first() else {
            return "nothing".to_owned();
        };
        let source = match first.source {
            ChatSource::System => "the system".to_owned(),
            ChatSource::Agent(agent) => format!("the agent {}", self.who(agent.uuid())),
            ChatSource::Object(object) => format!("the object {}", self.who(object.uuid())),
            ChatSource::Unknown {
                source_type,
                source_id,
            } => format!("source type {source_type} {}", self.who(source_id)),
        };
        let text = format!(
            "{} from {source}, owner {}, {}, {}, {}, text {}",
            type_text(first.chat_type),
            self.who(first.owner_id.unwrap_or_else(Uuid::nil)),
            self.whose_name(&first.from_name),
            audible_text(first.audible),
            self.place(&first.position, first.source.source_id()),
            text_text(&first.message, sent),
        );
        if lines.len() == 1 {
            text
        } else {
            format!("{} times, {text}", lines.len())
        }
    }

    /// What `got` holds of `typist` starting (or stopping) to type.
    fn typed(&self, got: &Got, typist: Uuid, start: bool) -> String {
        let notices: Vec<&Typed> = got
            .typed
            .iter()
            .filter(|typed| typed.source == typist && typed.start == start)
            .collect();
        let Some(first) = notices.first() else {
            return "nothing".to_owned();
        };
        let text = format!(
            "{} typing from {}, {}, {}, {}",
            if first.start { "start" } else { "stop" },
            self.who(first.source),
            self.whose_name(&first.name),
            audible_text(first.audible),
            self.place(&first.position, first.source),
        );
        if notices.len() == 1 {
            text
        } else {
            format!("{} times, {text}", notices.len())
        }
    }
}

/// Which of the two avatars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The one that crossed the border.
    A,
    /// The one that stayed.
    B,
}

/// Everything the legs share.
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// The ids, names and places the metrics name.
    scene: Scene,
    /// How long the speaker listens after a line.
    quiet: Duration,
    /// How long the queue of the other one is then read for.
    drain: Duration,
    /// How many lines the census has said.
    said: u32,
}

impl Census<'_> {
    /// Has `who` send `commands`, a moment apart, and returns what it and
    /// then the other avatar were sent.
    async fn act(&mut self, who: Who, commands: Vec<Command>) -> Result<(Got, Got), TestFailure> {
        let several = commands.len() > 1;
        let (actor, actor_id, listener, listener_id) = match who {
            Who::A => (&mut *self.a, self.scene.a, &mut *self.b, self.scene.b),
            Who::B => (&mut *self.b, self.scene.b, &mut *self.a, self.scene.a),
        };
        for command in commands {
            actor.send(command).await?;
            if several {
                tokio::time::sleep(BETWEEN).await;
            }
        }
        let acted = watch_for(actor, actor_id, STEP_TIMEOUT, self.quiet, |_got| true).await?;
        let listened = watch(listener, listener_id, self.drain).await?;
        Ok((acted, listened))
    }

    /// Has `who` whisper, say and shout, start typing and stop, and records
    /// under `word` what the other avatar was sent of each. With `by_reach`
    /// the whisper is recorded as whether it was heard just when the avatars
    /// are within a whisper's reach, rather than as what was heard.
    async fn speak(
        &mut self,
        metrics: &mut Metrics,
        word: &str,
        who: Who,
        by_reach: bool,
    ) -> Result<(), TestFailure> {
        let (speaker, by, to) = match who {
            Who::A => (self.scene.a, "", "b"),
            Who::B => (self.scene.b, "_of_b", "a"),
        };
        let mut says = Vec::new();
        for (kind, chat_type) in [
            ("whisper", ChatType::Whisper),
            ("say", ChatType::Normal),
            ("shout", ChatType::Shout),
        ] {
            self.said = self.said.saturating_add(1);
            let tag = format!("{LINE_TAG}{}-", self.said);
            says.push((kind, chat_type, format!("{tag}{kind}"), tag));
        }
        let commands = says
            .iter()
            .map(|(_kind, chat_type, text, _tag)| Command::Chat {
                message: text.clone(),
                chat_type: *chat_type,
                channel: ChatChannel(0),
            })
            .collect();
        let (_own, heard) = self.act(who, commands).await?;
        for (kind, _chat_type, text, tag) in &says {
            let told = self.scene.told(&heard, tag, text);
            if by_reach && *kind == "whisper" {
                // Within a metre of the reach itself the two positions are
                // not known well enough to say which side the avatars are.
                let short = WHISPER_M - self.scene.distance();
                metrics.set(
                    &format!("{word}_whisper{by}_heard_just_within_its_reach"),
                    short.abs() < 1.0 || (told != "nothing") == (short > 0.0),
                );
                continue;
            }
            metrics.set(&format!("{word}_{kind}{by}_told_{to}"), told);
        }
        for (kind, start) in [("typing_start", true), ("typing_stop", false)] {
            let (_own, heard) = self.act(who, vec![Command::Typing(start)]).await?;
            metrics.set(
                &format!("{word}_{kind}{by}_told_{to}"),
                self.scene.typed(&heard, speaker, start),
            );
        }
        Ok(())
    }
}

/// Where `session`'s own avatar is said to be in the region the session is
/// rooted in, listening for `window`; `known` where nothing said.
async fn own_spot(
    session: &mut Session,
    agent: Uuid,
    window: Duration,
    known: Option<Vector>,
) -> Result<Spot, TestFailure> {
    let region = session
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("the avatar is in no region".to_owned()))?;
    let mut at = known;
    let outcome = session
        .wait_for(window, |event| {
            if let Event::ObjectAdded(object) | Event::ObjectUpdated(object) = event
                && object.full_id.uuid() == agent
                && object.region_handle == region
            {
                at = Some(object.motion.position.clone());
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => {}
        Err(other) => return Err(other),
    }
    let at = at.ok_or_else(|| {
        TestFailure::Assertion("an avatar was never sent its own position".to_owned())
    })?;
    Ok(Spot { region, at })
}

/// Where `session`'s avatar is right now. A grid says where an avatar is
/// when it moves, so the avatar is lifted a little and what is then said of
/// it is read: a position kept from before a crossing is where it was, not
/// where it came to rest.
async fn fresh_spot(session: &mut Session, agent: Uuid) -> Result<Spot, TestFailure> {
    session
        .send(Command::SetControls(
            ControlFlags::FLY | ControlFlags::UP_POS,
        ))
        .await?;
    let lifted = own_spot(session, agent, LIFT, None).await;
    session
        .send(Command::SetControls(ControlFlags::FLY))
        .await?;
    let lifted = lifted.ok();
    own_spot(session, agent, HOVER, lifted.map(|spot| spot.at)).await
}

/// Has the avatar of `session`, hovering at `from`, rise or sink to the
/// height `level` and hover there; where it then is.
async fn level_out(
    session: &mut Session,
    agent: Uuid,
    from: Spot,
    level: f32,
) -> Result<Spot, TestFailure> {
    let started = Instant::now();
    let mut spot = from;
    loop {
        let off = spot.at.z - level;
        if off.abs() <= LEVEL_SLACK_M || started.elapsed() >= LEVEL_BUDGET {
            return Ok(spot);
        }
        let key = if off > 0.0 {
            ControlFlags::UP_NEG
        } else {
            ControlFlags::UP_POS
        };
        // A push for half of what is left, then a hover to read where it
        // led: a grid says nothing of an avatar that moves steadily, so a key
        // held until the height reads right is held far too long.
        let push = Duration::from_secs_f32((off.abs() / LIFT_M_PER_S / 2.0).clamp(0.15, 2.0));
        session
            .send(Command::SetControls(ControlFlags::FLY | key))
            .await?;
        spot = own_spot(session, agent, push, Some(spot.at)).await?;
        session
            .send(Command::SetControls(ControlFlags::FLY))
            .await?;
        spot = own_spot(session, agent, HOVER, Some(spot.at)).await?;
    }
}

/// Has the avatar of `session` fly from `from` up to the border `edge` of
/// the region it stands in, abreast of `beside` and at the height `level`,
/// and hover there; where it then is.
async fn to_the_border(
    session: &mut Session,
    agent: Uuid,
    from: Vector,
    beside: &Vector,
    edge: Edge,
    level: f32,
) -> Result<Spot, TestFailure> {
    let region = session
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("the avatar is in no region".to_owned()))?;
    let mut to = inside_from(edge, BY_THE_BORDER_M, beside);
    to.z = level;
    let (stopped, arrived) = steer_towards(
        session,
        from,
        &to,
        Gait::FlyingLevel,
        WALK_BUDGET,
        |_event| {},
    )
    .await?;
    session
        .send(Command::SetControls(ControlFlags::FLY))
        .await?;
    if !arrived {
        return Err(TestFailure::Assertion(format!(
            "an avatar did not get to the {} border: it is at {}",
            edge.label(),
            vector_text(&stopped)
        )));
    }
    let spot = own_spot(session, agent, HOVER, Some(stopped)).await?;
    let spot = level_out(session, agent, spot, level).await?;
    if session.region_handle() != Some(region) {
        return Err(TestFailure::Assertion(format!(
            "an avatar meant to stop at the {} border flew over it",
            edge.label()
        )));
    }
    Ok(spot)
}

/// The point `inside` metres into a region from its border `edge`, level
/// with `beside`.
fn inside_from(edge: Edge, inside: f32, beside: &Vector) -> Vector {
    let (x, y) = match edge {
        Edge::East => (REGION_M - inside, beside.y),
        Edge::West => (inside, beside.y),
        Edge::North => (beside.x, REGION_M - inside),
        Edge::South => (beside.x, inside),
    };
    Vector { x, y, z: beside.z }
}

/// Has `session`'s viewer state another draw distance and then its own
/// again, a moment apart.
///
/// OpenSim's region tells its neighbours where an agent stands only when the
/// agent has moved ten metres since it last did (`CHILDUPDATES_MOVEMENT`) —
/// or when its draw distance changed by more than 32 m. A neighbour measures
/// a line's reach to where it was last told the hearer is, so without this
/// the answers near a range's edge turn on where an avatar stood ten metres
/// ago: a whisper 8 m over the border went unheard one way and was heard the
/// other (2026-10-13).
async fn tell_the_neighbours(session: &mut Session, agent: Uuid) -> Result<(), TestFailure> {
    for metres in [NARROWER_VIEW_M, USUAL_VIEW_M] {
        session
            .send(Command::SetDrawDistance(Distance::new(metres)))
            .await?;
        watch(session, agent, VIEW_SETTLE).await?;
    }
    Ok(())
}

/// Puts `session`'s camera a metre above where its avatar stands.
async fn camera_at(session: &Session, at: &Vector) -> Result<(), TestFailure> {
    session
        .send(Command::SetCamera(Camera::looking_at(
            Vector {
                x: at.x,
                y: at.y,
                z: at.z + 1.0,
            },
            Vector {
                x: at.x + 5.0,
                y: at.y,
                z: at.z,
            },
        )))
        .await
}

/// Teleports the avatar of `session` to `to` in the region it stands in and
/// returns where it landed.
async fn place(session: &mut Session, to: &Vector) -> Result<Spot, TestFailure> {
    let region = session
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("the avatar is in no region".to_owned()))?;
    session
        .send(Command::Teleport {
            region_handle: region,
            position: RegionCoordinates::new(to.x, to.y, to.z),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let landing = session
        .wait_for(REGION_TIMEOUT, |event| match event {
            Event::TeleportLocal { position, .. } => Some(*position),
            _ => None,
        })
        .await?;
    Ok(Spot {
        region,
        at: Vector {
            x: landing.x(),
            y: landing.y(),
            z: landing.z(),
        },
    })
}

/// How many lines of chat `session` was sent down a child circuit and did
/// not read.
fn unread_on_a_child_circuit(session: &Session) -> i64 {
    let unread = session
        .diagnostics()
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic,
                Diagnostic::UnhandledMessage {
                    name: "ChatFromSimulator",
                    child: true,
                    ..
                }
            )
        })
        .count();
    i64::try_from(unread).unwrap_or(i64::MAX)
}

/// A census of local chat across a region border.
#[derive(Debug)]
pub struct ChatBorder;

impl GridTest for ChatBorder {
    fn name(&self) -> &'static str {
        "chat-border"
    }

    fn description(&self) -> &'static str {
        "Whether a whisper, a say, a shout and the typing notices are passed on over a region \
         border, how far, and where each says it was said"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        match grid {
            Grid::Aditi => super::teleport_cross_region::ADITI_START,
            Grid::Opensim => OPENSIM_START,
            Grid::FakeSl | Grid::FakeOpensim => "last",
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(900)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let control = ctx.fake().cloned();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, control.as_ref(), &mut metrics).await;
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// Stands the two avatars either side of the nearest border, A over it, on a
/// live grid; where each then is, and the border as B sees it.
async fn stand_live(
    a: &mut Session,
    b: &mut Session,
    (a_id, b_id): (AgentKey, AgentKey),
    grid: Grid,
) -> Result<(Spot, Spot, Edge), TestFailure> {
    let home = a
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("avatar A is in no region".to_owned()))?;
    let found = watch_neighbours(a, a_id, NEIGHBOUR_WINDOW, NEIGHBOUR_QUIET).await?;
    let own = found.own.clone().ok_or_else(|| {
        TestFailure::Assertion("avatar A never appeared in the object stream".to_owned())
    })?;
    let Some((_destination, edge, _distance)) = nearest_border(&found, home, &own.position) else {
        return Err(TestFailure::State(
            "no announced neighbour shares a border with the region the avatars stand in"
                .to_owned(),
        ));
    };
    tracing::info!(edge = edge.label(), "the border the avatars stand at");
    let level = own.position.z + HOVER_ABOVE_M;
    // B to the border first, abreast of A: A then flies straight over it.
    let b_from = fresh_spot(b, b_id.uuid()).await?;
    let b_at = to_the_border(b, b_id.uuid(), b_from.at, &own.position, edge, level).await?;
    camera_at(b, &b_at.at).await?;
    // A over it, and back up to it from the other side.
    let trace = walk_over_border(
        a,
        a_id,
        Some(&own),
        Walk {
            steer: Some(Steer { edge, fly: true }),
            budget: WALK_BUDGET,
            carry_on: CARRY_ON,
            settle: SETTLE,
        },
    )
    .await?;
    if trace.arrival.is_none() {
        return Err(TestFailure::Assertion(format!(
            "avatar A never crossed the {} border on {grid}",
            edge.label()
        )));
    }
    let a_over = fresh_spot(a, a_id.uuid()).await?;
    let beside = a_over.at.clone();
    let a_at = to_the_border(a, a_id.uuid(), a_over.at, &beside, edge.opposite(), level).await?;
    camera_at(a, &a_at.at).await?;
    Ok((a_at, b_at, edge))
}

/// Stands the two avatars either side of the fake grid's eastern border, A
/// over it: the grid is asked for the crossing, since it moves no avatar.
async fn stand_fake(
    a: &mut Session,
    b: &mut Session,
    a_id: AgentKey,
    control: &FakeControl,
) -> Result<(Spot, Spot, Edge), TestFailure> {
    let found = watch_neighbours(a, a_id, NEIGHBOUR_WINDOW, NEIGHBOUR_QUIET).await?;
    let b_at = place(
        b,
        &Vector {
            x: REGION_M - AT_THE_BORDER_M,
            y: FAKE_ARRIVAL.1,
            z: FAKE_ARRIVAL.2,
        },
    )
    .await?;
    let crossing = control.grid().cross_agent(
        control.agent(),
        crate::fake::EAST_REGION,
        RegionCoordinates::new(FAKE_ARRIVAL.0, FAKE_ARRIVAL.1, FAKE_ARRIVAL.2),
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    );
    // The client's `CompleteAgentMovement` completes the crossing, and it
    // sends one only while its run loop is draining: the two run together.
    let (crossed, trace) = tokio::join!(
        crossing,
        walk_over_border(
            a,
            a_id,
            found.own.as_ref(),
            Walk {
                steer: None,
                budget: WALK_BUDGET,
                carry_on: Duration::ZERO,
                settle: Duration::from_secs(1),
            },
        )
    );
    crossed
        .map_err(|error| TestFailure::State(format!("the scripted crossing failed: {error}")))?;
    let arrival = trace?.arrival.ok_or_else(|| {
        TestFailure::Assertion("avatar A never arrived across the border".to_owned())
    })?;
    let a_at = Spot {
        region: arrival.region_handle,
        at: Vector {
            x: FAKE_ARRIVAL.0,
            y: FAKE_ARRIVAL.1,
            z: FAKE_ARRIVAL.2,
        },
    };
    Ok((a_at, b_at, Edge::East))
}

/// The legs, in order.
async fn census(
    ctx: &mut TestContext,
    control: Option<&FakeControl>,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let pace = Pace::of(grid);
    let drain = if grid.is_fake() { FAKE_DRAIN } else { DRAIN };
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "chat-border needs a secondary avatar".to_owned(),
        ));
    };
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    if a.region_handle() != b.region_handle() {
        return Err(TestFailure::Assertion(
            "the two avatars did not log in to the same region".to_owned(),
        ));
    }
    let (a_at, b_at, edge) = match control {
        Some(control) => stand_fake(a, b, a_id, control).await?,
        None => stand_live(a, b, (a_id, b_id), grid).await?,
    };
    metrics.set("border", edge.label());
    let scene = Scene {
        a: a_id.uuid(),
        b: b_id.uuid(),
        a_name: a.avatar_legacy_name(),
        b_name: b.avatar_legacy_name(),
        a_at,
        b_at,
    };
    let mut census = Census {
        a,
        b,
        scene,
        quiet: pace.quiet,
        drain,
        said: 0,
    };
    for step in &STEPS {
        if let Some(inside) = step.inside {
            // Deeper into the region A crossed into, straight away from the
            // border B stands at.
            let to = inside_from(edge.opposite(), inside, &census.scene.a_at.at);
            census.scene.a_at = if control.is_some() {
                place(census.a, &to).await?
            } else {
                let from = census.scene.a_at.at.clone();
                let (stopped, _arrived) = steer_towards(
                    census.a,
                    from,
                    &to,
                    Gait::FlyingLevel,
                    WALK_BUDGET,
                    |_event| {},
                )
                .await?;
                census
                    .a
                    .send(Command::SetControls(ControlFlags::FLY))
                    .await?;
                own_spot(census.a, census.scene.a, HOVER, Some(stopped)).await?
            };
            camera_at(census.a, &census.scene.a_at.at).await?;
            watch(census.b, census.scene.b, census.drain).await?;
        }
        if control.is_none() {
            tell_the_neighbours(census.a, census.scene.a).await?;
            tell_the_neighbours(census.b, census.scene.b).await?;
        }
        let distance = census.scene.distance();
        let word = step.word;
        metrics.set(&format!("{word}_distance_m"), f64::from(distance));
        if distance <= step.band.0 || distance >= step.band.1 {
            return Err(TestFailure::Assertion(format!(
                "the `{word}` step wants the avatars between {} m and {} m apart, and they are \
                 {distance:.1} m (A at {}, B at {})",
                step.band.0,
                step.band.1,
                vector_text(&census.scene.a_at.at),
                vector_text(&census.scene.b_at.at),
            )));
        }
        let by_reach = step.inside.is_none();
        census.speak(metrics, word, Who::A, by_reach).await?;
        census.speak(metrics, word, Who::B, by_reach).await?;
    }
    metrics.set(
        "chat_unread_on_a_child_circuit_by_a",
        unread_on_a_child_circuit(census.a),
    );
    metrics.set(
        "chat_unread_on_a_child_circuit_by_b",
        unread_on_a_child_circuit(census.b),
    );
    if !is_opensim(grid) && control.is_none() {
        // Second Life holds an avatar that came down from a flight in its
        // landing animation until a viewer says it is over.
        census.a.send(Command::FinishAnimation).await?;
        census.b.send(Command::FinishAnimation).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Edge, REGION_M, Spot, inside_from};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{RegionHandle, Vector};

    /// A region `slots` east of the one at 1000/1000.
    fn region(slots: u32) -> RegionHandle {
        RegionHandle::from_grid(1000_u32.saturating_add(slots), 1000)
    }

    #[test]
    fn a_spot_in_the_next_region_as_this_one_sees_it() {
        let over = Spot {
            region: region(1),
            at: Vector {
                x: 3.0,
                y: 128.0,
                z: 26.0,
            },
        };
        assert_eq!(over.seen_from(region(1)).x.to_bits(), 3.0_f32.to_bits());
        assert_eq!(
            over.seen_from(region(0)).x.to_bits(),
            (REGION_M + 3.0).to_bits()
        );
        let back = Spot {
            region: region(0),
            at: Vector {
                x: 253.0,
                y: 128.0,
                z: 26.0,
            },
        };
        assert_eq!(back.seen_from(region(1)).x.to_bits(), (-3.0_f32).to_bits());
    }

    #[test]
    fn a_point_inside_a_region_from_each_border() {
        let beside = Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        };
        assert_eq!(
            inside_from(Edge::East, 9.0, &beside).x.to_bits(),
            247.0_f32.to_bits()
        );
        assert_eq!(
            inside_from(Edge::West, 9.0, &beside).x.to_bits(),
            9.0_f32.to_bits()
        );
        assert_eq!(
            inside_from(Edge::North, 9.0, &beside).y.to_bits(),
            247.0_f32.to_bits()
        );
        assert_eq!(
            inside_from(Edge::South, 9.0, &beside).y.to_bits(),
            9.0_f32.to_bits()
        );
    }
}
