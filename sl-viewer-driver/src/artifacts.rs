//! Failure artifacts: what a failed action or expectation leaves in the
//! viewer's artifact directory, one directory per failure —
//! `<dir>/<NNN>-<action>/`:
//!
//! - `screenshot.png`: the viewer's window, the locator's matches outlined;
//! - `tree.txt`: the semantic tree around the locator's scope, as the
//!   viewer's report had it — or, when the report has none, the top of the
//!   whole tree;
//! - `events.txt`: the event log's tail and the warnings and errors logged
//!   while the request ran.
//!
//! Each is saved on its own: one that cannot be says why in its place, and
//! never hides the failure it was saved for.

use core::fmt::Write as _;
use std::path::{Path, PathBuf};

use sl_automation_proto::{
    AutomationError, FailureReport, Locator, LogEntry, NodeVisibility, RequestBody, ResponseBody,
    UiNode,
};

use crate::error::Artifacts;
use crate::viewer::Viewer;

/// How many of the event log's last entries `events.txt` holds when the
/// viewer's report did not carry them.
const TAIL: usize = 32;

/// How many levels of the whole tree `tree.txt` holds when the report had no
/// excerpt.
const TOP_DEPTH: usize = 3;

/// The most nodes `tree.txt` holds when the report had no excerpt.
const TOP_NODES: usize = 300;

/// The longest an action's name gets in its directory's name.
const SLUG_LENGTH: usize = 48;

/// What a failure was about, which decides what its screenshot outlines.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Subject<'a> {
    /// The nodes a UI locator names.
    Ui(&'a Locator),
    /// The viewer as a whole — the world, or its state.
    Viewer,
}

/// Save the artifacts of failure number `number`, `action` failing with
/// `error`, under `dir`.
pub(crate) async fn save(
    viewer: &Viewer,
    dir: &Path,
    number: u32,
    action: &str,
    error: &AutomationError,
    report: Option<&FailureReport>,
    subject: Subject<'_>,
) -> Artifacts {
    let folder = dir.join(format!("{number:03}-{}", slug(action)));
    let folder = std::path::absolute(&folder).unwrap_or(folder);
    if let Err(error) = fs_err::create_dir_all(&folder) {
        let reason = error.to_string();
        return Artifacts {
            dir: Some(folder),
            screenshot: Some(Err(reason.clone())),
            tree: Some(Err(reason.clone())),
            events: Some(Err(reason)),
        };
    }
    let screenshot = screenshot(viewer, &folder, subject).await;
    let tree = tree(viewer, &folder, action, error, report).await;
    let events = events(viewer, &folder, report).await;
    Artifacts {
        dir: Some(folder),
        screenshot: Some(screenshot),
        tree: Some(tree),
        events: Some(events),
    }
}

/// `action` as a file name: its letters and digits, the rest dashes.
fn slug(action: &str) -> String {
    let mut slug = String::new();
    for character in action.chars() {
        if slug.chars().count() >= SLUG_LENGTH {
            break;
        }
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

/// Ask the viewer for a screenshot in `folder`, the subject's matches
/// outlined when it is a UI locator whose scope resolves.
async fn screenshot(
    viewer: &Viewer,
    folder: &Path,
    subject: Subject<'_>,
) -> Result<PathBuf, String> {
    let path = folder.join("screenshot.png");
    let outline = match subject {
        Subject::Ui(locator) => Some(locator.clone()),
        Subject::Viewer => None,
    };
    let first = raw_screenshot(viewer, &path, outline.clone()).await;
    match first {
        Ok(()) => Ok(path),
        // The outline's scope may be what is missing: the frame without it.
        Err(_reason) if outline.is_some() => {
            raw_screenshot(viewer, &path, None).await.map(|()| path)
        }
        Err(reason) => Err(reason),
    }
}

/// One screenshot request, answered or refused.
async fn raw_screenshot(
    viewer: &Viewer,
    path: &Path,
    outline: Option<Locator>,
) -> Result<(), String> {
    let body = RequestBody::Screenshot {
        path: path.display().to_string(),
        outline,
    };
    match viewer.raw(body).await {
        Ok(Ok(ResponseBody::Screenshot { .. })) => Ok(()),
        Ok(Ok(other)) => Err(format!("answered with {other:?}")),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

/// Write `tree.txt`: the report's excerpt, else the top of the whole tree.
async fn tree(
    viewer: &Viewer,
    folder: &Path,
    action: &str,
    error: &AutomationError,
    report: Option<&FailureReport>,
) -> Result<PathBuf, String> {
    let mut text = format!("{action}\n{error}\n\n");
    match report.map(|report| report.tree.as_slice()) {
        Some(excerpt) if !excerpt.is_empty() => {
            text.push_str("the tree around the locator's scope, as the viewer reported it:\n");
            render(&mut text, excerpt, 0);
        }
        _ => match viewer.raw(RequestBody::Snapshot { within: None }).await {
            Ok(Ok(ResponseBody::Snapshot { roots })) => {
                let _written = writeln!(
                    text,
                    "the top {TOP_DEPTH} levels of the whole tree (the report had no excerpt):"
                );
                let mut budget = TOP_NODES;
                render(&mut text, &prune(&roots, TOP_DEPTH, &mut budget), 0);
            }
            Ok(Ok(other)) => {
                let _written = writeln!(text, "(no tree: the snapshot answered {other:?})");
            }
            Ok(Err(error)) => {
                let _written = writeln!(text, "(no tree: {error})");
            }
            Err(error) => return Err(error.to_string()),
        },
    }
    write(&folder.join("tree.txt"), &text)
}

/// Write `events.txt`: the report's event tail and diagnostics, else the
/// event log's tail read now.
async fn events(
    viewer: &Viewer,
    folder: &Path,
    report: Option<&FailureReport>,
) -> Result<PathBuf, String> {
    let mut text = String::new();
    let tail: Vec<LogEntry> = match report {
        Some(report) => report.events.clone(),
        None => {
            match viewer
                .raw(RequestBody::ReadLog {
                    cursor: 0,
                    streams: Vec::new(),
                    limit: None,
                })
                .await
            {
                Ok(Ok(ResponseBody::Log { page })) => {
                    let skip = page.entries.len().saturating_sub(TAIL);
                    page.entries.into_iter().skip(skip).collect()
                }
                Ok(Ok(other)) => {
                    let _written = writeln!(text, "(no event log: answered with {other:?})");
                    Vec::new()
                }
                Ok(Err(error)) => {
                    let _written = writeln!(text, "(no event log: {error})");
                    Vec::new()
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    };
    text.push_str("the event log's tail, oldest first:\n");
    for entry in &tail {
        let _written = writeln!(
            text,
            "{:>8} {:?} {}: {}",
            entry.seq, entry.stream, entry.kind, entry.detail
        );
    }
    if let Some(report) = report {
        text.push_str("\nwarnings and errors logged while the request ran:\n");
        for line in &report.diagnostics {
            let _written = writeln!(
                text,
                "{:>8} {:?} {}: {}",
                line.seq, line.level, line.target, line.message
            );
        }
    }
    write(&folder.join("events.txt"), &text)
}

/// Write `text` to `path`.
fn write(path: &Path, text: &str) -> Result<PathBuf, String> {
    fs_err::write(path, text)
        .map(|()| path.to_owned())
        .map_err(|error| error.to_string())
}

/// `nodes` and their descendants down to `depth` levels, at most `budget`
/// nodes in all, in reading order.
fn prune(nodes: &[UiNode], depth: usize, budget: &mut usize) -> Vec<UiNode> {
    let mut kept = Vec::new();
    for node in nodes {
        if *budget == 0 || depth == 0 {
            break;
        }
        *budget = budget.saturating_sub(1);
        let children = prune(&node.children, depth.saturating_sub(1), budget);
        kept.push(UiNode {
            children,
            ..node.clone()
        });
    }
    kept
}

/// Append `nodes` to `text`, one line each, indented by `depth`: the node,
/// its states, its value and — when it cannot be seen — why.
pub(crate) fn render(text: &mut String, nodes: &[UiNode], depth: usize) {
    for node in nodes {
        let _written = write!(text, "{}{node}", "  ".repeat(depth));
        if !node.states.is_empty() {
            let states: Vec<String> = node
                .states
                .iter()
                .map(|state| format!("{state:?}").to_lowercase())
                .collect();
            let _written = write!(text, " [{}]", states.join(","));
        }
        if let Some(value) = &node.value {
            let _written = write!(text, " value={value:?}");
        }
        if node.visibility != NodeVisibility::Visible {
            let _written = write!(text, " ({:?})", node.visibility);
        }
        text.push('\n');
        render(text, &node.children, depth.saturating_add(1));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::slug;

    #[test]
    fn a_slug_is_a_file_name() {
        assert_eq!(
            slug("click on window #floater:build >> button key=build-apply"),
            "click-on-window-floater-build-button-key-build-a"
        );
        assert_eq!(
            slug("expect \"OK\" to be visible"),
            "expect-ok-to-be-visible"
        );
    }
}
