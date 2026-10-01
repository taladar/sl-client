//! The command line: the global options, the three ways to reach a viewer
//! (`launch`, `stage`, `attach`) and the verbs run against one.

use std::path::PathBuf;
use std::time::Duration;

use clap::{ArgGroup, Args, Parser, Subcommand};
use sl_automation_proto::{Locator, LogStream, NameMatcher, WaitCondition, WorldLocator};

/// Drive a running Second Life viewer from the shell, through its
/// automation socket.
///
/// Start one with `launch` (a headless viewer logged into a grid) or `stage`
/// (a fake grid and several viewers from a TOML file); both print each
/// viewer's socket and hold it until Ctrl-C, then log it out. Any other
/// command drives the viewer at `--socket` (or `SL_VIEWER_SOCKET`, or the one
/// automation socket that answers under `$XDG_RUNTIME_DIR`); `attach` runs
/// those commands one per line from standard input over one connection.
///
/// A UI selector names nodes by role and attributes, scoped with `>>`:
/// `window[test_id=floater:build] >> button[name_key=build-apply]`. A world
/// selector names one kind of thing: `object[name=Door][near=own_avatar]`.
/// Either may also be given as the locator's JSON.
#[derive(Debug, Parser)]
#[command(name = "sl-viewer-ctl", version)]
pub struct Cli {
    /// The options every command takes.
    #[command(flatten)]
    pub global: Global,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The options every command takes.
#[derive(Debug, Clone, Args)]
pub struct Global {
    /// The viewer's automation socket.
    #[arg(long, env = "SL_VIEWER_SOCKET", global = true, value_name = "PATH")]
    pub socket: Option<PathBuf>,
    /// Print one JSON document per result instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    /// How long an action or a wait waits, in seconds.
    #[arg(long, global = true, default_value = "10", value_parser = seconds, value_name = "SECONDS")]
    pub timeout: Duration,
    /// Save a failure's screenshot, tree excerpt and event tail under this
    /// directory.
    #[arg(long, global = true, value_name = "DIR")]
    pub artifacts: Option<PathBuf>,
}

/// What to do.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Launch a headless viewer with an automation socket, logged into a
    /// grid; print its socket once it has settled, and log it out on Ctrl-C.
    Launch(LaunchArgs),
    /// Start a fake grid and the viewers a TOML file names; print their
    /// sockets once all have settled, and take everything down on Ctrl-C.
    Stage {
        /// The stage file.
        file: PathBuf,
    },
    /// Connect to a viewer and run commands from standard input, one per
    /// line, over the one connection (`quit` or end of input to stop).
    Attach {
        /// The viewer's automation socket; `--socket`'s default when absent.
        socket: Option<PathBuf>,
    },
    /// A command run against a viewer.
    #[command(flatten)]
    Verb(Verb),
}

/// How `launch` starts its viewer.
#[derive(Debug, Clone, Args)]
pub struct LaunchArgs {
    /// The viewer executable; the `sl-client-bevy-viewer` beside this one
    /// when absent.
    #[arg(long, value_name = "PATH")]
    pub viewer: Option<PathBuf>,
    /// The run directory: the viewer's private state and its log. A fresh
    /// one under `$XDG_STATE_HOME/sl-viewer-ctl/runs` when absent.
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// The credentials file the viewer logs in with.
    #[arg(long, value_name = "PATH")]
    pub credentials: PathBuf,
    /// The avatar's key in the credentials file; its default avatar when
    /// absent.
    #[arg(long, value_name = "KEY")]
    pub avatar: Option<String>,
    /// A grid nickname (`localhost`, `aditi`, `agni`).
    #[arg(long, value_name = "NICK")]
    pub grid: Option<String>,
    /// An explicit login URI, overriding `--grid` and the avatar's own.
    #[arg(long, value_name = "URI")]
    pub login_uri: Option<String>,
    /// Where to start: `last`, `home`, or a region location.
    #[arg(long, value_name = "LOCATION")]
    pub start: Option<String>,
    /// Also open a window showing what the headless viewer renders (it
    /// still takes no input from the desktop).
    #[arg(long)]
    pub watch: bool,
    /// Run the embedded web-media engine (off by default: it starts
    /// Chromium, which a UI check rarely needs).
    #[arg(long)]
    pub web_media: bool,
    /// The rendered frame's size, `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1280x720", value_name = "WxH")]
    pub capture_size: String,
    /// Print the socket as soon as the viewer answers, without waiting for
    /// it to log in and settle.
    #[arg(long)]
    pub no_wait: bool,
    /// More arguments for the viewer, after `--`.
    #[arg(last = true, value_name = "VIEWER ARGS")]
    pub viewer_args: Vec<String>,
}

/// A command run against a viewer.
#[derive(Debug, Clone, Subcommand)]
pub enum Verb {
    /// Print the semantic UI tree, or the subtree of the one node a selector
    /// names.
    Tree {
        /// The subtree's root; the whole tree when absent.
        #[arg(value_parser = ui_selector)]
        selector: Option<Locator>,
        /// How many levels below the root to print.
        #[arg(long)]
        depth: Option<usize>,
    },
    /// List the nodes a selector names now, without waiting.
    Find {
        /// Which nodes.
        #[arg(value_parser = ui_selector)]
        selector: Locator,
    },
    /// Click the one node a selector names, once it is actionable.
    Click {
        /// Which node.
        #[arg(value_parser = ui_selector)]
        selector: Locator,
        /// With the right button.
        #[arg(long)]
        right: bool,
        /// Twice.
        #[arg(long, conflicts_with = "right")]
        double: bool,
    },
    /// Drag the one node a selector names with the left button: drop it onto
    /// another node (`--onto`), or move it by an offset (`--by`) — a window
    /// by its `floater-title-bar`, or resized by its `floater-resize`.
    #[command(group(ArgGroup::new("destination").required(true).args(["onto", "by"])))]
    Drag {
        /// Which node to press on.
        #[arg(value_parser = ui_selector)]
        selector: Locator,
        /// Release over the one node this selector names.
        #[arg(long, value_parser = ui_selector, value_name = "SELECTOR")]
        onto: Option<Locator>,
        /// Move by `X,Y` logical pixels, `X` rightwards and `Y` downwards
        /// (`--by -40,0` moves left).
        #[arg(long, value_parser = drag_offset, allow_hyphen_values = true, value_name = "X,Y")]
        by: Option<[f32; 2]>,
    },
    /// Replace the text of the one text field a selector names by typing.
    Fill {
        /// Which field.
        #[arg(value_parser = ui_selector)]
        selector: Locator,
        /// The text.
        text: String,
    },
    /// Press a key or a chord (`Enter`, `Ctrl+Shift+S`) on whatever has the
    /// focus, or on a field clicked into first.
    Press {
        /// The key or chord.
        keys: String,
        /// Click into this node first.
        #[arg(long, value_parser = ui_selector, value_name = "SELECTOR")]
        on: Option<Locator>,
    },
    /// Wait for the nodes a selector names to be in a state, and print them.
    Wait {
        /// Which nodes.
        #[arg(value_parser = ui_selector)]
        selector: Locator,
        /// The state: attached, detached, visible, hidden, enabled, disabled,
        /// `text=<exact>` or `text~=<part>`.
        #[arg(long = "for", default_value = "visible", value_parser = wait_condition, value_name = "STATE")]
        condition: WaitCondition,
    },
    /// Open the window of a floater by its id (`build`, `inventory`,
    /// `preferences`).
    Open {
        /// The floater id.
        floater: String,
    },
    /// Walk a menu path from the menu bar by the entries' Fluent keys, and
    /// click the last entry.
    Menu {
        /// The entries' keys, the bar menu's first.
        #[arg(required = true)]
        path: Vec<String>,
    },
    /// Find or act on things in the world.
    #[command(subcommand)]
    World(WorldVerb),
    /// Print every open conversation's transcript, local chat first.
    Chat,
    /// Print every notification the viewer raised, oldest first.
    Notifications,
    /// Print the own agent: region, position, seat, teleport, camera, heading.
    Agent,
    /// Print the environment being drawn: the sky's name and ambient colour,
    /// and whether the viewer's own local sky stands in for the shared one.
    Environment,
    /// Save the viewer's frame to a PNG.
    Screenshot {
        /// Where.
        path: PathBuf,
        /// Outline the boxes of the nodes this selector names.
        #[arg(long, value_parser = ui_selector, value_name = "SELECTOR")]
        outline: Option<Locator>,
    },
    /// Print the event log: events, commands and UI actions.
    Events {
        /// Keep printing entries as they are recorded, until Ctrl-C.
        #[arg(long)]
        follow: bool,
        /// Only these streams: event, command, ui_action.
        #[arg(long, value_delimiter = ',', value_parser = log_stream)]
        stream: Vec<LogStream>,
        /// The first sequence number to print; everything the viewer keeps
        /// when absent, or only what comes next with `--follow`.
        #[arg(long)]
        from: Option<u64>,
    },
}

/// A command about the world.
#[derive(Debug, Clone, Subcommand)]
pub enum WorldVerb {
    /// List the things a world selector names, once their names arrived.
    Find {
        /// Which things.
        #[arg(value_parser = world_selector)]
        selector: WorldLocator,
    },
    /// Touch the one thing a world selector names: a left click on it,
    /// framing it with the camera first when no point of it takes one.
    Touch {
        /// Which thing.
        #[arg(value_parser = world_selector)]
        selector: WorldLocator,
        /// Never move the camera: fail instead.
        #[arg(long)]
        no_reveal: bool,
    },
}

/// A command for the attached viewer, one per line; `quit` or the end of
/// the input stops.
#[derive(Debug, Parser)]
#[command(no_binary_name = true, name = "")]
pub struct Line {
    /// The verb.
    #[command(subcommand)]
    pub verb: Verb,
}

/// A duration given in (possibly fractional) seconds.
fn seconds(text: &str) -> Result<Duration, String> {
    let seconds: f64 = text
        .parse()
        .map_err(|error| format!("{text:?} is not a number of seconds: {error}"))?;
    Duration::try_from_secs_f64(seconds).map_err(|error| format!("{text:?}: {error}"))
}

/// A UI selector, in the grammar or as the locator's JSON.
///
/// # Errors
///
/// The parse error, naming the column.
pub fn ui_selector(text: &str) -> Result<Locator, String> {
    if text.trim_start().starts_with('{') {
        serde_json::from_str(text).map_err(|error| format!("the locator JSON: {error}"))
    } else {
        text.parse()
            .map_err(|error: sl_automation_proto::SelectorError| error.to_string())
    }
}

/// A world selector, in the grammar or as the locator's JSON.
///
/// # Errors
///
/// The parse error, naming the column.
pub fn world_selector(text: &str) -> Result<WorldLocator, String> {
    if text.trim_start().starts_with('{') {
        serde_json::from_str(text).map_err(|error| format!("the world locator JSON: {error}"))
    } else {
        text.parse()
            .map_err(|error: sl_automation_proto::SelectorError| error.to_string())
    }
}

/// A drag's offset, `X,Y` in logical pixels.
fn drag_offset(text: &str) -> Result<[f32; 2], String> {
    let (x, y) = text
        .split_once(',')
        .ok_or_else(|| format!("{text:?} is not an offset `X,Y` (`--by -40,0`)"))?;
    let axis = |part: &str, name: &str| -> Result<f32, String> {
        part.trim()
            .parse::<f32>()
            .ok()
            .filter(|amount| amount.is_finite())
            .ok_or_else(|| format!("{text:?}: {name} {part:?} is not a number of pixels"))
    };
    Ok([axis(x, "X")?, axis(y, "Y")?])
}

/// A state `wait` waits for.
fn wait_condition(text: &str) -> Result<WaitCondition, String> {
    if let Some(part) = text.strip_prefix("text~=") {
        return Ok(WaitCondition::Text(NameMatcher::Contains(part.to_owned())));
    }
    if let Some(exact) = text.strip_prefix("text=") {
        return Ok(WaitCondition::Text(NameMatcher::Exact(exact.to_owned())));
    }
    match text {
        "attached" => Ok(WaitCondition::Attached),
        "detached" => Ok(WaitCondition::Detached),
        "visible" => Ok(WaitCondition::Visible),
        "hidden" => Ok(WaitCondition::Hidden),
        "enabled" => Ok(WaitCondition::Enabled),
        "disabled" => Ok(WaitCondition::Disabled),
        other => Err(format!(
            "unknown state {other:?} (attached, detached, visible, hidden, enabled, disabled, \
             text=<exact>, text~=<part>)"
        )),
    }
}

/// An event log stream by its name.
fn log_stream(text: &str) -> Result<LogStream, String> {
    [LogStream::Event, LogStream::Command, LogStream::UiAction]
        .into_iter()
        .find(|stream| stream.as_str() == text)
        .ok_or_else(|| format!("unknown stream {text:?} (event, command, ui_action)"))
}
