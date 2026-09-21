//! The run plan: everything both viewers are told, and the one environment
//! block that tells them.
//!
//! Two frames are only comparable when the things that decide what a frame
//! *is* — its pixel grid, which layers of the composited image are in it, where
//! the camera stands, where the sun stands, how long the viewer waited before
//! pressing the shutter — were the same on both sides. Keeping those in one
//! value that both launches are built from is what makes "the same scene" true
//! rather than intended.
//!
//! The environment variables are the ones both viewers' harnesses already read
//! ([`CaptureSpec::env`]); the camera is passed as command-line flags, because
//! this workspace's viewer takes it only that way. Nothing here reads the
//! process environment: a run's settings come from its plan, so a stray
//! `SL_VIEWER_*` in the operator's shell cannot silently desynchronise the pair.

use core::fmt;

/// A point in Second Life region coordinates: metres, Z up, region-local.
///
/// The units are the contract between the two viewers' camera flags, which both
/// parse `x,y,z` in exactly this frame — so a plan never has to know which
/// viewer stores its camera which way round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionPoint {
    /// Metres east of the region's south-west corner.
    pub x: f32,
    /// Metres north of the region's south-west corner.
    pub y: f32,
    /// Metres above sea level.
    pub z: f32,
}

impl RegionPoint {
    /// A point at `x, y, z`.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

impl fmt::Display for RegionPoint {
    /// The `x,y,z` form both viewers' `--camera-position` / `--camera-look-at`
    /// parse. No spaces: a shell would split them into three arguments.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", self.x, self.y, self.z)
    }
}

/// Parse an `x,y,z` argument into a [`RegionPoint`], the same shape both
/// viewers accept.
///
/// # Errors
///
/// Returns a message naming what was wrong, for `clap` to print.
pub fn parse_region_point(text: &str) -> Result<RegionPoint, String> {
    let parts: Vec<&str> = text.split(',').collect();
    let [x, y, z] = parts.as_slice() else {
        return Err(format!(
            "expected three comma-separated numbers `x,y,z`, got {text:?}"
        ));
    };
    let coordinate = |raw: &str| raw.trim().parse::<f32>().map_err(|error| error.to_string());
    Ok(RegionPoint::new(
        coordinate(x)?,
        coordinate(y)?,
        coordinate(z)?,
    ))
}

/// Where the camera stands and what it looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSpec {
    /// Where the camera stands, in region metres.
    pub position: RegionPoint,
    /// What it aims at, in region metres. `None` leaves each viewer its own
    /// default aim, which is only useful when the two are not being compared.
    pub look_at: Option<RegionPoint>,
}

impl CameraSpec {
    /// A camera standing `distance` metres south of `subject` and `height`
    /// metres above it, looking at it.
    ///
    /// South-and-above rather than any other bearing because it is the one that
    /// needs no knowledge of the scene: the fake grid's fixture rows run
    /// west-to-east, so a camera to the south sees the row rather than the end
    /// of it, and nothing in a scenario stands south of its own landmarks.
    #[must_use]
    pub fn facing(subject: RegionPoint, distance: f32, height: f32) -> Self {
        Self {
            position: RegionPoint::new(subject.x, subject.y - distance, subject.z + height),
            look_at: Some(subject),
        }
    }
}

/// How much slack a derived deadline carries over the timings it is derived
/// from: process start-up, the login round trip, the logout, and the difference
/// between a frame interval and the frame it actually takes.
const SLACK_SECS: f32 = 60.0;

/// Whether a capture run may make sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAudio {
    /// Both viewers muted for the run — the default.
    Muted,
    /// Both viewers play sound as an ordinary session would.
    Audible,
}

/// The environment a skin choice reaches its viewer by.
///
/// One helper rather than two identical bodies, because the *variable names*
/// are the one thing the two viewers genuinely share — both harnesses read
/// `SL_VIEWER_SKIN` and `SL_VIEWER_THEME`. What may go in them does not, which
/// is why the callers are separate types.
///
/// An unnamed skin is **absent** rather than stated as empty, unlike the
/// capture block's layer switches: no value means "your own default" to either
/// viewer, and an empty string names no skin in either, so leaving the variable
/// out is the only way to say it.
fn skin_env(skin: Option<&str>, theme: Option<&str>) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(skin) = skin {
        env.push(("SL_VIEWER_SKIN".to_owned(), skin.to_owned()));
    }
    if let Some(theme) = theme {
        env.push(("SL_VIEWER_THEME".to_owned(), theme.to_owned()));
    }
    env
}

/// Which skin **this workspace's viewer** wears.
///
/// A distinct type from [`FirestormSkin`], not one shared "skin spec" used
/// twice, because the two namespaces are unrelated and a value valid in one is
/// generally invalid in the other: `vintage` names a skin in the reference and
/// (so far) nothing here, `graphite` the reverse. A single type would let a
/// run hand one viewer the other's skin, and the mistake would surface as a
/// capture of the wrong interface rather than as an error.
///
/// They are also expected to diverge further. Firestorm's skin list is a
/// catalogue file with display names beside folders; ours is a directory
/// listing. Whatever either grows — a version, a search path, a validity rule —
/// it grows without dragging the other along.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlClientSkin {
    /// A directory under the viewer's `assets/skins/`, or `None` for its
    /// default.
    pub skin: Option<String>,
    /// A theme overlay id — a file under that skin's `themes/` — or `None` for
    /// the skin's own base.
    pub theme: Option<String>,
}

impl SlClientSkin {
    /// The environment this choice reaches the viewer by.
    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        skin_env(self.skin.as_deref(), self.theme.as_deref())
    }

    /// Whether the viewer was left in whatever skin it comes up in.
    #[must_use]
    pub const fn is_unset(&self) -> bool {
        self.skin.is_none() && self.theme.is_none()
    }
}

/// Which skin **Firestorm** wears.
///
/// The counterpart of [`SlClientSkin`], and deliberately not the same type —
/// see there for why. Both fields take either the folder or the display name
/// from the reference's `skins.xml`, matched case-insensitively by its harness,
/// which refuses a name it cannot resolve and says which it has.
///
/// The theme is where the two viewers are furthest apart, permanently: every
/// reference skin's first theme has an **empty folder**, so it can only be
/// asked for by name (`Classic`, `Grey`). That wart is not being reproduced on
/// our side, so there is no spelling of "the base theme" the two share.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FirestormSkin {
    /// A skin folder or display name from its `skins.xml`, or `None` for
    /// whatever the run directory comes up in.
    pub skin: Option<String>,
    /// A theme folder or display name within that skin, or `None` to let the
    /// harness take the skin's first.
    pub theme: Option<String>,
}

impl FirestormSkin {
    /// The environment this choice reaches the viewer by.
    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        skin_env(self.skin.as_deref(), self.theme.as_deref())
    }

    /// Whether the viewer was left in whatever skin it comes up in.
    #[must_use]
    pub const fn is_unset(&self) -> bool {
        self.skin.is_none() && self.theme.is_none()
    }
}

/// The pixel grid, the layers, the shutter and the sun: everything that decides
/// what a captured frame holds, and the only thing both viewers are configured
/// with by **one** environment block.
///
/// The skin is deliberately not here — see [`SlClientSkin`] and
/// [`FirestormSkin`], which are per viewer and not even the same type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureSpec {
    /// The frame width in pixels.
    pub width: u32,
    /// The frame height in pixels.
    pub height: u32,
    /// Whether the viewer's own interface is in the frame.
    pub ui: bool,
    /// Whether the HUD-attachment layer is in the frame.
    pub hud: bool,
    /// Whether the edit-tool gizmo overlay is in the frame.
    pub gizmos: bool,
    /// Whether the viewers may make sound. Not a layer of the frame, but it
    /// travels with them: muted by default, so a scene's looping sound sources
    /// do not play through the machine's speakers for the length of a run.
    pub audio: CaptureAudio,
    /// How many frames to capture.
    pub frames: usize,
    /// Seconds between successive frames.
    pub interval: f32,
    /// Seconds to wait for the scene to stop loading before capturing anyway.
    pub settle_timeout: f32,
    /// Seconds to wait to get in world before giving up on the run.
    pub login_timeout: f32,
    /// Where the sun stands, as a day position in `[0, 1]`. `None` leaves each
    /// viewer its own default, which is *not* the same default — pin it for any
    /// comparison that involves lighting, which is all of them.
    pub day_position: Option<f32>,
    /// The camera's vertical field of view, in degrees. `None` leaves each
    /// viewer its own default.
    ///
    /// Both default to the reference's 60° since
    /// [[viewer-camera-fov-parity]] — ours was 45°, and a run put both cameras
    /// at one pose and framed five prims of a row against three. Pinning it
    /// states the premise instead of resting on two defaults agreeing, which is
    /// what let that go unnoticed.
    pub fov_degrees: Option<f32>,
}

impl Default for CaptureSpec {
    /// 1080p, world only, thirty frames half a second apart, with the same
    /// settle and login timeouts both viewers already default to.
    ///
    /// The day position defaults to *unset* rather than to a number: picking one
    /// here would quietly override whatever the scenario's own environment says,
    /// and a scene whose sky is part of the fixture is one the runner should not
    /// be second-guessing.
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            ui: false,
            hud: false,
            gizmos: false,
            audio: CaptureAudio::Muted,
            frames: 30,
            interval: 0.5,
            settle_timeout: 25.0,
            login_timeout: 180.0,
            day_position: None,
            fov_degrees: None,
        }
    }
}

impl CaptureSpec {
    /// The environment block that configures **both** viewers.
    ///
    /// Every name here is read by this workspace's viewer and by the Firestorm
    /// harness, with the same meaning; that parity is the whole reason the
    /// capture settings travel by environment while everything else travels by
    /// flag. The three layer switches and the audio switch are always
    /// emitted, including as `0`, because both viewers treat `0` as off and an *absent* variable would let
    /// a leftover one from the operator's shell reach one viewer and not the
    /// other.
    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        let flag = |on: bool| if on { "1" } else { "0" }.to_owned();
        let mut env = vec![
            (
                "SL_VIEWER_CAPTURE_SIZE".to_owned(),
                format!("{}x{}", self.width, self.height),
            ),
            ("SL_VIEWER_CAPTURE_UI".to_owned(), flag(self.ui)),
            ("SL_VIEWER_CAPTURE_HUD".to_owned(), flag(self.hud)),
            ("SL_VIEWER_CAPTURE_GIZMOS".to_owned(), flag(self.gizmos)),
            (
                "SL_VIEWER_CAPTURE_AUDIO".to_owned(),
                flag(self.audio == CaptureAudio::Audible),
            ),
            (
                "SL_VIEWER_SCREENSHOT_FRAMES".to_owned(),
                self.frames.to_string(),
            ),
            (
                "SL_VIEWER_SCREENSHOT_INTERVAL".to_owned(),
                self.interval.to_string(),
            ),
            (
                "SL_VIEWER_SCREENSHOT_DELAY".to_owned(),
                self.settle_timeout.to_string(),
            ),
            (
                "SL_VIEWER_LOGIN_TIMEOUT".to_owned(),
                self.login_timeout.to_string(),
            ),
        ];
        if let Some(position) = self.day_position {
            env.push((
                "SL_VIEWER_SKY_DAY_POSITION".to_owned(),
                position.to_string(),
            ));
        }
        if let Some(degrees) = self.fov_degrees {
            env.push(("SL_VIEWER_CAPTURE_FOV".to_owned(), degrees.to_string()));
        }
        env
    }

    /// How long a run can reasonably take: the login timeout, the settle
    /// timeout, every frame's interval and a logout grace, plus slack.
    ///
    /// Derived rather than a flat number because the three parts a run spends
    /// its time in are all configurable, and a fixed deadline that was generous
    /// for thirty frames silently kills a run of three hundred.
    #[must_use]
    pub fn suggested_deadline_secs(&self) -> f32 {
        // Firestorm counts its settle timeout from *after* login and this viewer
        // counts it from startup, so summing them is the safe reading for both.
        //
        // The capture time is computed as a `Duration` rather than as a float
        // product: it saturates instead of overflowing, and a negative or
        // not-a-number interval — which a command line can carry — becomes zero
        // here instead of a panic or a deadline in the past.
        let frames = u32::try_from(self.frames).unwrap_or(u32::MAX);
        let capture = core::time::Duration::from_secs_f32(self.interval.max(0.0))
            .saturating_mul(frames)
            .as_secs_f32();
        self.login_timeout + self.settle_timeout + capture + SLACK_SECS
    }
}

/// A whole run: the scene, where the grid is, who logs in, what is captured and
/// from where.
#[expect(
    clippy::module_name_repetitions,
    reason = "the plan for a run is `RunPlan` everywhere it is used, including in the two \
              launches and the summary; the module it is filed under should not rename it"
)]
#[derive(Debug, Clone, PartialEq)]
pub struct RunPlan {
    /// The named scenario every region of the grid is dressed with.
    pub scenario: String,
    /// The login URI both viewers are pointed at, as an IPv4 literal — never
    /// `localhost`, which resolves to `::1` first while the fake grid listens on
    /// IPv4 only, and fails to connect for a reason that looks nothing like the
    /// cause.
    pub login_uri: url::Url,
    /// The avatar's first name.
    pub first_name: String,
    /// The avatar's last name.
    pub last_name: String,
    /// The avatar's password on the fake grid.
    pub password: String,
    /// What each frame holds.
    pub capture: CaptureSpec,
    /// The skin this workspace's viewer wears.
    pub sl_client_skin: SlClientSkin,
    /// The skin Firestorm wears. A different type, not a second copy of the
    /// same one: see [`SlClientSkin`] for why one name cannot dress both.
    pub firestorm_skin: FirestormSkin,
    /// Where the camera stands. `None` leaves both viewers wherever they put
    /// their camera on arrival, which is not the same place — useful for a
    /// smoke test, useless for a comparison.
    pub camera: Option<CameraSpec>,
}

impl RunPlan {
    /// The `host:port` Firestorm takes as a grid name and resolves through
    /// `GET /get_grid_info`.
    ///
    /// `--grid` rather than `--loginuri`: `CmdLineLoginURI` is dead code in the
    /// OpenSim build of Firestorm — declared, mapped, and read by nothing — so a
    /// run configured with it logs into whatever grid the viewer used last.
    ///
    /// # Errors
    ///
    /// Returns a message when the login URI carries no host, which no fake grid
    /// URI does.
    pub fn firestorm_grid_name(&self) -> Result<String, String> {
        let host = self
            .login_uri
            .host_str()
            .ok_or_else(|| format!("the login URI {} has no host", self.login_uri))?;
        Ok(match self.login_uri.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        CameraSpec, CaptureSpec, FirestormSkin, RegionPoint, RunPlan, SlClientSkin,
        parse_region_point,
    };

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A plan pointed at a grid on the loopback address.
    fn plan() -> Result<RunPlan, TestError> {
        Ok(RunPlan {
            scenario: "catalogue".to_owned(),
            login_uri: "http://127.0.0.1:9100/".parse()?,
            first_name: "Test".to_owned(),
            last_name: "User".to_owned(),
            password: "password".to_owned(),
            capture: CaptureSpec::default(),
            sl_client_skin: SlClientSkin::default(),
            firestorm_skin: FirestormSkin::default(),
            camera: None,
        })
    }

    /// The layer switches are emitted even when off. An absent variable is not
    /// the same as `0`: it lets one left over in the operator's shell reach one
    /// viewer, and the pair is then not comparable in the one way the run was
    /// meant to control.
    #[test]
    fn every_layer_switch_is_stated_either_way() {
        let env = CaptureSpec::default().env();
        for key in [
            "SL_VIEWER_CAPTURE_UI",
            "SL_VIEWER_CAPTURE_HUD",
            "SL_VIEWER_CAPTURE_GIZMOS",
            "SL_VIEWER_CAPTURE_AUDIO",
        ] {
            let value = env
                .iter()
                .find(|(name, _value)| name == key)
                .map(|(_name, value)| value.as_str());
            assert_eq!(value, Some("0"), "{key} should be stated as off");
        }
    }

    /// A pinned lens reaches both viewers, and an unpinned one stays unpinned:
    /// they agree on 60° by default, and a run that wants to *state* that says
    /// so rather than resting on two defaults agreeing.
    #[test]
    fn a_pinned_lens_travels_with_the_capture_block() {
        let mut capture = CaptureSpec::default();
        assert!(
            !capture
                .env()
                .iter()
                .any(|(name, _value)| name == "SL_VIEWER_CAPTURE_FOV")
        );
        capture.fov_degrees = Some(60.0);
        assert!(
            capture
                .env()
                .contains(&("SL_VIEWER_CAPTURE_FOV".to_owned(), "60".to_owned()))
        );
    }

    /// An unnamed skin is left out of the environment rather than stated as
    /// empty — the one setting where "unset" has no spelling, because no
    /// string names a viewer's own default.
    #[test]
    fn an_unnamed_skin_says_nothing() {
        assert!(SlClientSkin::default().env().is_empty());
        assert!(SlClientSkin::default().is_unset());
        assert!(FirestormSkin::default().env().is_empty());
        assert!(FirestormSkin::default().is_unset());
    }

    /// A skin and its theme travel as the two variables both viewers read, and
    /// a skin without a theme names only itself: the reference's base theme is
    /// its empty folder and ours is the absence of an overlay, so there is
    /// nothing to say.
    #[test]
    fn a_named_skin_travels_as_two_variables() {
        let skin_only = SlClientSkin {
            skin: Some("graphite".to_owned()),
            theme: None,
        };
        assert_eq!(
            skin_only.env(),
            vec![("SL_VIEWER_SKIN".to_owned(), "graphite".to_owned())]
        );
        assert!(!skin_only.is_unset());

        let themed = FirestormSkin {
            skin: Some("firestorm".to_owned()),
            theme: Some("Dark".to_owned()),
        };
        assert_eq!(
            themed.env(),
            vec![
                ("SL_VIEWER_SKIN".to_owned(), "firestorm".to_owned()),
                ("SL_VIEWER_THEME".to_owned(), "Dark".to_owned()),
            ]
        );
    }

    /// The two viewers' skins are independent: dressing one says nothing about
    /// the other, which is the whole reason they are separate fields.
    #[test]
    fn each_viewer_is_dressed_on_its_own() -> Result<(), TestError> {
        let mut plan = plan()?;
        plan.firestorm_skin = FirestormSkin {
            skin: Some("vintage".to_owned()),
            theme: Some("Classic".to_owned()),
        };
        assert!(plan.sl_client_skin.is_unset());
        assert_eq!(plan.sl_client_skin.env(), Vec::new());
        assert_eq!(plan.firestorm_skin.env().len(), 2);
        Ok(())
    }

    /// The size goes out in the `WIDTHxHEIGHT` form both viewers parse, and an
    /// unpinned sun stays unpinned rather than becoming a number of ours.
    #[test]
    fn the_capture_size_and_the_sun_travel_by_environment() {
        let mut capture = CaptureSpec::default();
        let env = capture.env();
        assert!(env.contains(&("SL_VIEWER_CAPTURE_SIZE".to_owned(), "1920x1080".to_owned())));
        assert!(
            !env.iter()
                .any(|(name, _value)| name == "SL_VIEWER_SKY_DAY_POSITION")
        );
        capture.day_position = Some(0.25);
        assert!(
            capture
                .env()
                .contains(&("SL_VIEWER_SKY_DAY_POSITION".to_owned(), "0.25".to_owned()))
        );
    }

    /// A deadline follows the timings it is a deadline for: a run of three
    /// hundred frames must not inherit one sized for thirty.
    #[test]
    fn the_deadline_grows_with_the_run() {
        let short = CaptureSpec::default();
        let long = CaptureSpec {
            frames: 300,
            ..short
        };
        assert!(long.suggested_deadline_secs() > short.suggested_deadline_secs() + 100.0);
    }

    /// The camera flag's text form is what both viewers parse: three numbers,
    /// commas, no spaces.
    #[test]
    fn a_camera_point_prints_as_both_viewers_read_it() -> Result<(), TestError> {
        let point = RegionPoint::new(128.0, 120.5, 26.0);
        assert_eq!(point.to_string(), "128,120.5,26");
        assert_eq!(parse_region_point("128,120.5,26")?, point);
        let Err(_too_few) = parse_region_point("128,120.5") else {
            return Err("two numbers are not a point".into());
        };
        let Err(_not_numbers) = parse_region_point("a,b,c") else {
            return Err("three words are not a point".into());
        };
        Ok(())
    }

    /// Aiming at a landmark puts the camera south of it and above it, looking
    /// back — so a west-to-east fixture row is seen along its length rather than
    /// end on.
    #[test]
    fn a_camera_aimed_at_a_landmark_stands_south_of_it() {
        let subject = RegionPoint::new(128.0, 140.0, 25.0);
        let camera = CameraSpec::facing(subject, 8.0, 2.0);
        assert_eq!(camera.position, RegionPoint::new(128.0, 132.0, 27.0));
        assert_eq!(camera.look_at, Some(subject));
    }

    /// Firestorm takes the grid as `host:port` and resolves it through
    /// `get_grid_info`; the port must survive, or the run logs into port 80.
    #[test]
    fn the_firestorm_grid_name_keeps_the_port() -> Result<(), TestError> {
        assert_eq!(plan()?.firestorm_grid_name()?, "127.0.0.1:9100");
        Ok(())
    }
}
