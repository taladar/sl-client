//! The script recorder: writes a replayable `.repl` transcript of the
//! interactively-entered REPL lines so a live session can be re-run verbatim
//! with `--script`.
//!
//! [`ScriptRecorder`] runs in parallel with the always-on log file. For each
//! interactive line it emits a `sleep <seconds>` directive capturing the wall
//! gap since the previous entry (so replay reproduces the session's pacing) and
//! then the raw line **verbatim** — `$placeholder` tokens are preserved exactly
//! as typed, so a `touch $lastobj` replays as `touch $lastobj` and rebinds to
//! whatever object is current on the next run rather than freezing a stale id.
//!
//! Lines that fail to [parse](crate::parse::parse_line) are written as a
//! `# ERROR: …` comment so the transcript stays honest yet remains a valid
//! script (comments are ignored on replay). The output is exactly the grammar
//! [`parse_line`] accepts, so it round-trips, and every line is flushed
//! immediately so the script survives a crash or Ctrl-C.
//!
//! Only **interactive** input should be fed to the recorder — auto-generated
//! `--smoke` requests are not recorded, and a script being replayed is already
//! the input so it is not re-recorded (interactive lines entered after a script
//! finishes are appended).

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use crate::parse::parse_line;

/// Records interactively-entered REPL lines to a replayable `.repl` script.
///
/// Construct one with [`ScriptRecorder::create`] (file-backed) or
/// [`ScriptRecorder::with_writer`] (any sink), optionally add header comments
/// with [`comment`](ScriptRecorder::comment), then call
/// [`record`](ScriptRecorder::record) once per interactive line.
pub struct ScriptRecorder {
    /// The transcript sink (a buffered file, or a custom writer for tests).
    writer: Box<dyn Write + Send>,
    /// The monotonic instant of the previous recorded line, used to compute the
    /// `sleep` delta before the next one. `None` until the first line.
    last: Option<Instant>,
}

impl fmt::Debug for ScriptRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptRecorder")
            .field("last", &self.last)
            .finish_non_exhaustive()
    }
}

impl ScriptRecorder {
    /// Create a recorder writing a fresh `.repl` transcript at `path`
    /// (truncating any existing file), with a leading header comment.
    ///
    /// # Errors
    ///
    /// Returns any [`io::Error`] from creating the file or writing the header.
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = File::create(path)?;
        Self::with_writer(Box::new(BufWriter::new(file)))
    }

    /// Create a recorder writing to an arbitrary `writer` (used by tests and any
    /// caller that wants a non-file sink), with a leading header comment.
    ///
    /// # Errors
    ///
    /// Returns any [`io::Error`] from writing the header.
    pub fn with_writer(writer: Box<dyn Write + Send>) -> io::Result<Self> {
        let mut recorder = Self { writer, last: None };
        recorder.write_line("# sl-repl session transcript")?;
        Ok(recorder)
    }

    /// Write a free-standing `# `-prefixed comment line (for the grid/login
    /// header the binary records at startup, secrets excluded).
    ///
    /// # Errors
    ///
    /// Returns any [`io::Error`] from writing the line.
    pub fn comment(&mut self, text: &str) -> io::Result<()> {
        self.write_line(&format!("# {text}"))
    }

    /// Record one interactive line, entered at monotonic instant `now`.
    ///
    /// Emits a `sleep <seconds>` directive first (except before the very first
    /// line) capturing the gap since the previous entry, then the line verbatim
    /// — or, if the line fails to parse, a `# ERROR: …` comment. Blank lines are
    /// ignored. The transcript is flushed before returning.
    ///
    /// # Errors
    ///
    /// Returns any [`io::Error`] from writing the line(s).
    pub fn record(&mut self, raw_line: &str, now: Instant) -> io::Result<()> {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            return Ok(());
        }
        if let Some(previous) = self.last {
            let delta = now.saturating_duration_since(previous);
            let millis = delta.as_millis();
            if millis > 0 {
                let seconds = millis / 1000;
                let frac = millis % 1000;
                self.write_line(&format!("sleep {seconds}.{frac:03}"))?;
            }
        }
        self.last = Some(now);
        match parse_line(line) {
            Ok(_) => self.write_line(line),
            Err(error) => self.write_line(&format!("# ERROR: {error}: {line}")),
        }
    }

    /// Write a single line followed by a newline and flush immediately, so the
    /// transcript survives a crash mid-session.
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        writeln!(self.writer, "{line}")?;
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;

    use super::ScriptRecorder;
    use crate::parse::{ReplAction, parse_line};

    /// A `Write` sink backed by a shared buffer so a test can read back exactly
    /// what the recorder produced without touching the filesystem.
    #[derive(Clone)]
    struct SharedSink(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedSink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_poisoned| io::Error::other("poisoned recorder buffer"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Record `lines` (each paired with its monotonic instant) and return the
    /// resulting transcript text.
    fn transcript(lines: &[(&str, Instant)]) -> Result<String, String> {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let mut recorder = ScriptRecorder::with_writer(Box::new(SharedSink(Arc::clone(&buffer))))
            .map_err(|error| format!("{error:?}"))?;
        for (line, now) in lines {
            recorder
                .record(line, *now)
                .map_err(|error| format!("{error:?}"))?;
        }
        drop(recorder);
        let bytes = buffer
            .lock()
            .map_err(|_poisoned| "poisoned recorder buffer".to_owned())?
            .clone();
        String::from_utf8(bytes).map_err(|error| format!("{error:?}"))
    }

    #[test]
    fn records_replayable_command_sequence() -> Result<(), String> {
        let start = Instant::now();
        let text = transcript(&[
            ("chat hello", start),
            ("im $self hi", start + Duration::from_millis(1500)),
            (r#"chat "unterminated"#, start + Duration::from_secs(2)),
        ])?;

        // Replay the transcript: the command names that come back must match the
        // interactive commands, with sleeps and the error comment filtered out.
        let mut names = Vec::new();
        for line in text.lines() {
            if let Some(ReplAction::Command(pending)) =
                parse_line(line).map_err(|error| format!("{error:?}"))?
            {
                names.push(pending.name);
            }
        }
        assert_eq!(
            names,
            vec!["chat".to_owned(), "im".to_owned()],
            "the parse-failing line is dropped, leaving only the two real commands"
        );
        Ok(())
    }

    #[test]
    fn emits_sleep_delta_between_entries() -> Result<(), String> {
        let start = Instant::now();
        let text = transcript(&[
            ("chat first", start),
            ("chat second", start + Duration::from_millis(2500)),
        ])?;
        assert!(
            text.contains("sleep 2.500"),
            "a 2.5s gap should be recorded as `sleep 2.500`, got:\n{text}"
        );
        assert!(
            !text.contains("sleep 0"),
            "no sleep directive should precede the first entry, got:\n{text}"
        );
        Ok(())
    }

    #[test]
    fn parse_failure_is_recorded_as_error_comment() -> Result<(), String> {
        let text = transcript(&[(r#"chat "oops"#, Instant::now())])?;
        assert!(
            text.contains("# ERROR:"),
            "an unterminated quote should be recorded as a `# ERROR` comment, got:\n{text}"
        );
        // The honest-but-valid comment must itself parse as a comment.
        let comment_parses = text
            .lines()
            .filter(|line| line.starts_with("# ERROR:"))
            .all(|line| matches!(parse_line(line), Ok(Some(ReplAction::Meta(_)))));
        assert!(comment_parses, "the error line must replay as a comment");
        Ok(())
    }

    #[test]
    fn blank_lines_are_not_recorded() -> Result<(), String> {
        let start = Instant::now();
        let text = transcript(&[
            ("   ", start),
            ("chat hi", start + Duration::from_millis(500)),
        ])?;
        // The blank line is skipped, so it does not become the `last` entry and
        // therefore no sleep precedes the first real command.
        assert!(
            !text.contains("sleep"),
            "a leading blank line should not produce a sleep, got:\n{text}"
        );
        Ok(())
    }
}
