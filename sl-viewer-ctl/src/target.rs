//! Which viewer a command drives: the socket given, or the one viewer whose
//! automation socket answers in the default place.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::error::CtlError;

/// The directory under `$XDG_RUNTIME_DIR` the viewer puts its automation
/// socket in when `--automation-socket` names none, and where `launch`
/// puts the ones it asks for.
pub(crate) const SOCKET_DIR: &str = "sl-client-bevy-viewer";

/// The default socket directory, or `None` without `XDG_RUNTIME_DIR`.
#[must_use]
pub(crate) fn socket_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|runtime| PathBuf::from(runtime).join(SOCKET_DIR))
}

/// The socket to drive: `explicit` when given, else the one socket in the
/// default directory that accepts a connection.
///
/// # Errors
///
/// [`CtlError::NoSocket`] when none is given and no socket, or more than one,
/// answers.
pub(crate) fn resolve(explicit: Option<&Path>) -> Result<PathBuf, CtlError> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let dir = socket_dir().ok_or_else(|| {
        CtlError::NoSocket(
            "no --socket given, and XDG_RUNTIME_DIR is not set to look for one in".to_owned(),
        )
    })?;
    let answering = answering_sockets(&dir);
    let mut found = answering.iter();
    match (found.next(), found.next()) {
        (Some(only), None) => Ok(only.clone()),
        (None, _) => Err(CtlError::NoSocket(format!(
            "no --socket given, and no viewer's automation socket answers in {}; launch one \
             (`sl-viewer-ctl launch`) or start the viewer with --automation-socket",
            dir.display()
        ))),
        (Some(_), Some(_)) => {
            let listed: Vec<String> = answering
                .iter()
                .map(|path| format!("  {}", path.display()))
                .collect();
            Err(CtlError::NoSocket(format!(
                "no --socket given, and several viewers answer; pick one:\n{}",
                listed.join("\n")
            )))
        }
    }
}

/// The `*.sock` files in `dir` that accept a connection, in name order —
/// a viewer that exited without removing its socket leaves one that does
/// not.
fn answering_sockets(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs_err::read_dir(dir) else {
        return Vec::new();
    };
    let mut sockets: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "sock")
        })
        .filter(|path| UnixStream::connect(path).is_ok())
        .collect();
    sockets.sort();
    sockets
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use pretty_assertions::assert_eq;

    use super::answering_sockets;

    /// Only a socket something listens on counts: a stale file left by a
    /// viewer that died, or any other file, does not.
    #[test]
    fn only_answering_sockets_are_found() -> Result<(), Box<dyn core::error::Error>> {
        let dir = std::env::temp_dir().join(format!("sl-viewer-ctl-target-{}", std::process::id()));
        fs_err::create_dir_all(&dir)?;
        let live = dir.join("b.sock");
        let _listener = UnixListener::bind(&live)?;
        let stale = dir.join("a.sock");
        drop(UnixListener::bind(&stale)?);
        fs_err::write(dir.join("c.txt"), "not a socket")?;
        assert_eq!(answering_sockets(&dir), vec![live]);
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }
}
