//! What a command prints: a line or a few for a person, or one JSON
//! document per result for a script (`--json`).

use std::io::{self, Write};
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use sl_automation_proto::{
    AgentReadout, ConversationReadout, ConversationRef, EnvironmentReadout, GroundPoint,
    InventoryFolderReadout, LogEntry, NodeState, NodeValue, NodeVisibility, NotificationReadout,
    UiNode, ViewerIdentity, WorldMapReadout, WorldNode,
};
use sl_viewer_driver::{AnsweredFileDialog, Screenshot};

/// One result of a command.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A semantic tree, or the subtrees of some roots, cut `depth` levels
    /// below them.
    Tree {
        /// The roots.
        roots: Vec<UiNode>,
        /// How many levels below a root to print; all when `None`.
        depth: Option<usize>,
    },
    /// Nodes, without their children.
    Nodes(Vec<UiNode>),
    /// The node an action was carried out on.
    Acted {
        /// What was done: `clicked`, `filled`, `dropped onto` (the node is
        /// the drop target), `dragged` (the node as it was when pressed).
        verb: &'static str,
        /// The node.
        node: UiNode,
    },
    /// A key or chord was pressed.
    Pressed(String),
    /// In-world things.
    WorldNodes(Vec<WorldNode>),
    /// The thing a world action was carried out on.
    WorldActed {
        /// What was done: `touched`.
        verb: &'static str,
        /// The thing.
        node: WorldNode,
    },
    /// A ground action's result.
    GroundActed {
        /// What was done: `double-clicked`.
        verb: &'static str,
        /// The ground.
        ground: GroundPoint,
        /// Where the click landed, in the region's metres.
        hit_point: [f32; 3],
    },
    /// The conversations.
    Conversations(Vec<ConversationReadout>),
    /// The notifications.
    Notifications(Vec<NotificationReadout>),
    /// The own agent.
    Agent(AgentReadout),
    /// An inventory tree, the root folder first.
    InventoryTree(Vec<InventoryFolderReadout>),
    /// The environment being drawn.
    Environment(EnvironmentReadout),
    /// What the world map knows.
    WorldMap(WorldMapReadout),
    /// A file dialog answered.
    FileDialog {
        /// The dialog.
        answered: AnsweredFileDialog,
        /// What it was answered with; `None` for Cancel.
        picked: Option<PathBuf>,
    },
    /// A screenshot the viewer saved.
    Screenshot(Screenshot),
    /// Event log entries.
    Log(Vec<LogEntry>),
    /// One event log entry, as `events --follow` streams them.
    Entry(LogEntry),
    /// Entries the log lost before they could be read.
    Dropped(u64),
    /// Who a viewer is.
    Identity(ViewerIdentity),
    /// A viewer `launch` or `stage` brought up, ready to be driven.
    Ready {
        /// Its label.
        label: String,
        /// Its automation socket.
        socket: String,
        /// Its log.
        log: String,
        /// Its process id.
        pid: u32,
        /// Who it is.
        identity: ViewerIdentity,
    },
    /// A viewer `launch` or `stage` stopped, and how.
    Stopped {
        /// Its label.
        label: String,
        /// How it ended.
        ending: String,
    },
    /// A fake grid `stage` started.
    Grid {
        /// Its login URI.
        login_uri: String,
        /// Its scenario.
        scenario: String,
    },
}

/// Where results are printed, and how.
#[derive(Debug)]
pub struct Printer<W> {
    /// JSON (`true`) or text.
    json: bool,
    /// Where to.
    out: W,
}

impl<W: Write> Printer<W> {
    /// A printer writing to `out`, JSON when `json` is set.
    pub const fn new(out: W, json: bool) -> Self {
        Self { json, out }
    }

    /// Whether it prints JSON.
    pub const fn is_json(&self) -> bool {
        self.json
    }

    /// The writer, to take back what was printed.
    pub fn into_inner(self) -> W {
        self.out
    }

    /// Print `outcome`, and flush — a script reading line by line sees each
    /// result as it comes.
    ///
    /// # Errors
    ///
    /// The write's.
    pub fn print(&mut self, outcome: &Outcome) -> io::Result<()> {
        if self.json {
            let value = json_of(outcome)?;
            serde_json::to_writer(&mut self.out, &value)?;
            writeln!(self.out)?;
        } else {
            text_of(&mut self.out, outcome)?;
        }
        self.out.flush()
    }

    /// Print a note for a person — a help text: as it is, or `{"note": …}`
    /// under JSON.
    ///
    /// # Errors
    ///
    /// The write's.
    pub fn note(&mut self, text: &str) -> io::Result<()> {
        if self.json {
            serde_json::to_writer(&mut self.out, &json!({ "note": text }))?;
            writeln!(self.out)?;
        } else {
            writeln!(self.out, "{}", text.trim_end())?;
        }
        self.out.flush()
    }

    /// Print an error: text, or `{"error": …}` under JSON.
    ///
    /// # Errors
    ///
    /// The write's.
    pub fn error(&mut self, message: &str) -> io::Result<()> {
        if self.json {
            serde_json::to_writer(&mut self.out, &json!({ "error": message }))?;
            writeln!(self.out)?;
        } else {
            writeln!(self.out, "error: {message}")?;
        }
        self.out.flush()
    }
}

/// `value` as JSON.
fn to_json(value: &impl Serialize) -> io::Result<JsonValue> {
    serde_json::to_value(value).map_err(io::Error::other)
}

/// The JSON document an outcome prints as.
fn json_of(outcome: &Outcome) -> io::Result<JsonValue> {
    Ok(match outcome {
        Outcome::Tree { roots, depth } => {
            let roots: Vec<UiNode> = roots.iter().map(|root| pruned(root, *depth)).collect();
            to_json(&roots)?
        }
        Outcome::Nodes(nodes) => to_json(nodes)?,
        Outcome::Acted { verb, node } => json!({ "done": verb, "node": to_json(node)? }),
        Outcome::Pressed(keys) => json!({ "done": "pressed", "keys": keys }),
        Outcome::WorldNodes(nodes) => to_json(nodes)?,
        Outcome::WorldActed { verb, node } => json!({ "done": verb, "node": to_json(node)? }),
        Outcome::GroundActed {
            verb,
            ground,
            hit_point,
        } => json!({ "done": verb, "ground": to_json(ground)?, "hit_point": hit_point }),
        Outcome::Conversations(conversations) => to_json(conversations)?,
        Outcome::Notifications(notifications) => to_json(notifications)?,
        Outcome::Agent(agent) => to_json(agent)?,
        Outcome::InventoryTree(folders) => to_json(folders)?,
        Outcome::Environment(environment) => to_json(environment)?,
        Outcome::WorldMap(map) => to_json(map)?,
        Outcome::FileDialog { answered, picked } => json!({
            "done": if picked.is_some() { "answered" } else { "cancelled" },
            "purpose": answered.purpose,
            "title": answered.title,
            "folder": answered.folder,
            "path": picked.as_ref().map(|path| path.display().to_string()),
        }),
        Outcome::Screenshot(shot) => json!({
            "path": shot.path.display().to_string(),
            "width": shot.width,
            "height": shot.height,
            "outlined": to_json(&shot.outlined)?,
        }),
        Outcome::Log(entries) => to_json(entries)?,
        Outcome::Entry(entry) => to_json(entry)?,
        Outcome::Dropped(count) => json!({ "dropped": count }),
        Outcome::Identity(identity) => to_json(identity)?,
        Outcome::Ready {
            label,
            socket,
            log,
            pid,
            identity,
        } => json!({
            "ready": label,
            "socket": socket,
            "log": log,
            "pid": pid,
            "viewer": to_json(identity)?,
        }),
        Outcome::Stopped { label, ending } => json!({ "stopped": label, "ending": ending }),
        Outcome::Grid {
            login_uri,
            scenario,
        } => json!({ "grid": login_uri, "scenario": scenario }),
    })
}

/// `node` with its descendants cut `depth` levels below it.
fn pruned(node: &UiNode, depth: Option<usize>) -> UiNode {
    let children = match depth {
        Some(0) => Vec::new(),
        Some(depth) => node
            .children
            .iter()
            .map(|child| pruned(child, Some(depth.saturating_sub(1))))
            .collect(),
        None => node.children.clone(),
    };
    UiNode {
        children,
        ..node.clone()
    }
}

/// Print an outcome as text.
fn text_of(out: &mut impl Write, outcome: &Outcome) -> io::Result<()> {
    match outcome {
        Outcome::Tree { roots, depth } => {
            for root in roots {
                tree_lines(out, root, 0, *depth)?;
            }
        }
        Outcome::Nodes(nodes) => {
            if nodes.is_empty() {
                writeln!(out, "no match")?;
            }
            for node in nodes {
                writeln!(out, "{}", node_line(node))?;
            }
        }
        Outcome::Acted { verb, node } => writeln!(out, "{verb} {}", node_line(node))?,
        Outcome::Pressed(keys) => writeln!(out, "pressed {keys}")?,
        Outcome::WorldNodes(nodes) => {
            if nodes.is_empty() {
                writeln!(out, "no match")?;
            }
            for node in nodes {
                writeln!(out, "{node}")?;
            }
        }
        Outcome::WorldActed { verb, node } => writeln!(out, "{verb} {node}")?,
        Outcome::GroundActed {
            verb,
            ground,
            hit_point: [x, y, z],
        } => writeln!(out, "{verb} {ground}, landing at <{x},{y},{z}>")?,
        Outcome::Conversations(conversations) => conversation_lines(out, conversations)?,
        Outcome::Notifications(notifications) => notification_lines(out, notifications)?,
        Outcome::Agent(agent) => agent_lines(out, agent)?,
        Outcome::InventoryTree(folders) => inventory_lines(out, folders)?,
        Outcome::Environment(environment) => environment_lines(out, environment)?,
        Outcome::WorldMap(map) => world_map_lines(out, map)?,
        Outcome::FileDialog { answered, picked } => match picked {
            Some(path) => writeln!(
                out,
                "answered {} ({:?}) with {}",
                answered.purpose,
                answered.title,
                path.display()
            )?,
            None => writeln!(out, "cancelled {} ({:?})", answered.purpose, answered.title)?,
        },
        Outcome::Screenshot(shot) => {
            writeln!(
                out,
                "saved {} ({}x{})",
                shot.path.display(),
                shot.width,
                shot.height
            )?;
            for node in &shot.outlined {
                writeln!(out, "  outlined {}", node_line(node))?;
            }
        }
        Outcome::Log(entries) => {
            for entry in entries {
                writeln!(out, "{}", entry_line(entry))?;
            }
        }
        Outcome::Entry(entry) => writeln!(out, "{}", entry_line(entry))?,
        Outcome::Dropped(count) => writeln!(out, "({count} entries lost before they were read)")?,
        Outcome::Identity(identity) => writeln!(out, "{}", identity_line(identity))?,
        Outcome::Ready {
            label,
            socket,
            log,
            pid,
            identity,
        } => {
            writeln!(out, "{label} ready: {}", identity_line(identity))?;
            writeln!(out, "  socket {socket}")?;
            writeln!(out, "  log    {log} (pid {pid})")?;
            writeln!(out, "  export SL_VIEWER_SOCKET={}", shell_word(socket))?;
        }
        Outcome::Stopped { label, ending } => writeln!(out, "{label} stopped: {ending}")?,
        Outcome::Grid {
            login_uri,
            scenario,
        } => writeln!(out, "fake grid at {login_uri}, scenario {scenario}")?,
    }
    Ok(())
}

/// `node` and its descendants, indented by depth, down to `limit` levels.
fn tree_lines(
    out: &mut impl Write,
    node: &UiNode,
    level: usize,
    limit: Option<usize>,
) -> io::Result<()> {
    writeln!(
        out,
        "{:indent$}{}",
        "",
        node_line(node),
        indent = level.saturating_mul(2)
    )?;
    if limit.is_some_and(|limit| level >= limit) {
        if !node.children.is_empty() {
            writeln!(
                out,
                "{:indent$}… {} more",
                "",
                node.children.len(),
                indent = level.saturating_add(1).saturating_mul(2)
            )?;
        }
        return Ok(());
    }
    for child in &node.children {
        tree_lines(out, child, level.saturating_add(1), limit)?;
    }
    Ok(())
}

/// One line for a node: what it is, its box, its states and value, and why
/// it cannot be seen when it cannot.
fn node_line(node: &UiNode) -> String {
    let mut parts = vec![node.to_string()];
    if !node.states.is_empty() {
        let states: Vec<&str> = node.states.iter().copied().map(state_name).collect();
        parts.push(format!("[{}]", states.join(", ")));
    }
    match &node.value {
        Some(NodeValue::Text(text)) => parts.push(format!("= {text:?}")),
        Some(NodeValue::Number(number)) => parts.push(format!("= {number}")),
        Some(NodeValue::Color(color)) => parts.push(format!("= {color}")),
        None => {}
    }
    if let Some(accelerator) = &node.accelerator {
        parts.push(format!("({accelerator})"));
    }
    let hidden = match node.visibility {
        NodeVisibility::Visible => None,
        NodeVisibility::Hidden => Some("(hidden)"),
        NodeVisibility::Clipped => Some("(clipped)"),
        NodeVisibility::OffScreen => Some("(off screen)"),
        NodeVisibility::Covered => Some("(covered)"),
    };
    parts.extend(hidden.map(str::to_owned));
    parts.join(" ")
}

/// One line for an event log entry.
fn entry_line(entry: &LogEntry) -> String {
    format!(
        "#{} {} {}: {}",
        entry.seq, entry.stream, entry.kind, entry.detail
    )
}

/// A state's name.
const fn state_name(state: NodeState) -> &'static str {
    match state {
        NodeState::Disabled => "disabled",
        NodeState::ReadOnly => "read-only",
        NodeState::Checked => "checked",
        NodeState::Selected => "selected",
        NodeState::Expanded => "expanded",
        NodeState::Focused => "focused",
        NodeState::Hovered => "hovered",
    }
}

/// One line naming a viewer and who is logged into it.
fn identity_line(identity: &ViewerIdentity) -> String {
    let mut parts = vec![format!(
        "{} {} (pid {})",
        identity.viewer, identity.version, identity.pid
    )];
    if let Some(name) = &identity.agent_name {
        parts.push(format!("as {name}"));
    }
    if let Some(grid) = &identity.grid {
        parts.push(format!("on {grid}"));
    }
    parts.join(" ")
}

/// Each conversation's heading and its lines.
fn conversation_lines(
    out: &mut impl Write,
    conversations: &[ConversationReadout],
) -> io::Result<()> {
    if conversations.is_empty() {
        writeln!(out, "no conversations")?;
    }
    for conversation in conversations {
        let which = match conversation.conversation {
            ConversationRef::Nearby => "nearby chat".to_owned(),
            ConversationRef::Direct(id) => format!("instant messages with {id}"),
            ConversationRef::Group(id) => format!("group chat {id}"),
            ConversationRef::Conference(id) => format!("conference {id}"),
        };
        let mut heading = vec![format!("== {which}")];
        if conversation.unread > 0 {
            heading.push(format!("({} unread)", conversation.unread));
        }
        if conversation.pending_invite {
            heading.push("(invitation pending)".to_owned());
        }
        writeln!(out, "{}", heading.join(" "))?;
        for line in &conversation.lines {
            let own = if line.own { "* " } else { "  " };
            writeln!(out, "{own}{}: {}", line.speaker, line.text)?;
        }
    }
    Ok(())
}

/// Each notification on a line, its buttons on the next.
fn notification_lines(
    out: &mut impl Write,
    notifications: &[NotificationReadout],
) -> io::Result<()> {
    if notifications.is_empty() {
        writeln!(out, "no notifications")?;
    }
    for notification in notifications {
        let live = if notification.live { " (shown)" } else { "" };
        writeln!(
            out,
            "#{} {}{live}: {}",
            notification.id, notification.template, notification.text
        )?;
        if !notification.buttons.is_empty() {
            let buttons: Vec<String> = notification
                .buttons
                .iter()
                .map(|button| {
                    let default = if button.default { "*" } else { "" };
                    format!("{}{default} ({})", button.label, button.name)
                })
                .collect();
            writeln!(out, "  buttons: {}", buttons.join(", "))?;
        }
    }
    Ok(())
}

/// The agent's readout, a field a line.
fn agent_lines(out: &mut impl Write, agent: &AgentReadout) -> io::Result<()> {
    match agent.agent_id {
        Some(id) => writeln!(out, "agent    {id}")?,
        None => writeln!(out, "agent    (not logged in)")?,
    }
    if let Some(region) = &agent.region {
        let name = region.name.as_deref().unwrap_or("(unnamed)");
        match region.id {
            Some(id) => writeln!(out, "region   {name} ({id})")?,
            None => writeln!(out, "region   {name}")?,
        }
    }
    if let Some([x, y, z]) = agent.position {
        writeln!(out, "position {x:.2}, {y:.2}, {z:.2}")?;
    }
    if let Some(seat) = agent.seated_on {
        writeln!(out, "sits on  {seat}")?;
    }
    if let Some(teleport) = &agent.teleport {
        writeln!(out, "teleport {}", to_json(teleport)?)?;
    }
    if let Some(camera) = &agent.camera {
        writeln!(out, "camera   {}", to_json(camera)?)?;
    }
    if let Some(heading) = agent.heading {
        writeln!(out, "heading  {heading:.3} rad")?;
    }
    Ok(())
}

/// One block per folder: its id, name and whether it was fetched, then its
/// items, indented; a last line counts what is there and what is not loaded.
fn inventory_lines(out: &mut impl Write, folders: &[InventoryFolderReadout]) -> io::Result<()> {
    for folder in folders {
        let state = if folder.loaded {
            "loaded"
        } else {
            "NOT LOADED"
        };
        writeln!(out, "{} {:?} ({state})", folder.id, folder.name)?;
        for item in &folder.items {
            writeln!(out, "  {} {:?} {}", item.id, item.name, item.kind)?;
        }
    }
    let items: usize = folders.iter().map(|folder| folder.items.len()).sum();
    let unloaded = folders.iter().filter(|folder| !folder.loaded).count();
    writeln!(
        out,
        "{} folders, {items} items, {unloaded} not loaded",
        folders.len()
    )
}

/// The world map's readout: the tiles, then a region a line, then an item
/// layer a line.
fn world_map_lines(out: &mut impl Write, map: &WorldMapReadout) -> io::Result<()> {
    writeln!(
        out,
        "tiles    {} ready, {} pending, {} absent, from {}",
        map.tiles_ready,
        map.tiles_pending,
        map.tiles_absent,
        map.tile_server.as_deref().unwrap_or("(no tile server)")
    )?;
    for region in &map.regions {
        let [x, y] = region.grid;
        writeln!(out, "region   ({x}, {y}) {}", region.name)?;
    }
    for layer in &map.items {
        let [x, y] = layer.grid;
        let drawn = layer.items.iter().filter(|item| item.drawn).count();
        writeln!(
            out,
            "items    ({x}, {y}) type {}: {} items, {drawn} drawn",
            layer.kind,
            layer.items.len()
        )?;
    }
    Ok(())
}

/// The environment's readout, a field a line.
fn environment_lines(out: &mut impl Write, environment: &EnvironmentReadout) -> io::Result<()> {
    match &environment.sky {
        Some(sky) => {
            let [red, green, blue] = sky.ambient;
            let [azimuth, elevation] = sky.sun;
            writeln!(out, "sky      {}", sky.name)?;
            writeln!(out, "ambient  {red:.6}, {green:.6}, {blue:.6}")?;
            writeln!(out, "haze     {:.6}", sky.haze_density)?;
            writeln!(
                out,
                "sun      azimuth {:.1}°, elevation {:.1}°",
                azimuth.to_degrees(),
                elevation.to_degrees()
            )?;
        }
        None => writeln!(out, "sky      (none drawn yet)")?,
    }
    if let Some(water) = &environment.water {
        writeln!(
            out,
            "water    {}, fog density {:.6}",
            water.name, water.fog_density
        )?;
    }
    let layer = if environment.local_sky {
        "local"
    } else {
        "shared"
    };
    writeln!(out, "layer    {layer}")?;
    if let Some(fraction) = environment.transition {
        writeln!(out, "fading   {:.0}%", fraction * 100.0)?;
    }
    if !environment.previewing.is_empty() {
        writeln!(out, "preview  {}", environment.previewing.join(", "))?;
    }
    Ok(())
}

/// `text` quoted for a POSIX shell when it needs to be.
fn shell_word(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|next| next.is_ascii_alphanumeric() || matches!(next, '/' | '.' | '_' | '-' | ':'))
    {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}
