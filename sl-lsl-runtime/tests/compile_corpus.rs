//! The compiler against a corpus, with **tailslide** as the oracle for which
//! scripts a grid accepts.
//!
//! The property: a script tailslide compiles must compile here, and a script
//! it rejects must produce at least one compile error, on a line tailslide
//! also reports. tailslide reproduces Linden's compiler, so a script it
//! accepts that we reject is a save that would fail here and succeed on the
//! grid — the failure that matters most.
//!
//! The committed corpus is `sl-lsl`'s (`sl-lsl/tests/corpus/`), shared with
//! the semantic pass's own differential test. Without tailslide, its
//! `valid/` and `error/` folders stand in for the oracle's verdict (they
//! were sorted by it) and an error's line is only required to exist.
//!
//! - `SL_LSL_TAILSLIDE_BIN` — a built `tailslide`, to run the oracle.
//! - `SL_LSL_DIFFTEST_CORPUS` — a directory of `.lsl` files to use instead
//!   of the committed corpus, e.g. tailslide's own `tests/scripts/`. Needs
//!   the oracle, since its files are not sorted. At that scale only a false
//!   rejection fails the test; a script tailslide rejects and we compile is
//!   reported, as is an error on a line tailslide did not name.
//!
//! Two false rejections are known and reported apart rather than failed:
//!
//! - **The parser's depth ceiling** — `sl-lsl` stops at 128 nested levels
//!   where tailslide's heap-stacked parser goes on (its
//!   `parserstackdepth2.lsl`); `sl-lsl`'s differential test sets it apart
//!   the same way.
//! - **Functions missing from the library table.** The table is Linden's
//!   own viewer `LSLSyntax` document, which leaves out a few functions the
//!   grid still compiles and tailslide's `builtins.txt` lists
//!   (`llPointAt`, `llRemoteLoadScript`). A call to one is undefined here.
//!
//! And one error sits on a different line by design: "Not all code paths
//! return a value" is reported at the function's closing brace, where Second
//! Life reports it (measured on aditi), not at its name, where tailslide
//! does — so the scale run lists tailslide's `check_all_return.lsl` and
//! `retval.lsl` under "on another line".

#![expect(
    clippy::print_stderr,
    reason = "a corpus test reports its skip reason and its findings to the operator"
)]

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use sl_lsl_runtime::{CompileError, CompileErrorKind, compile};

    /// What the oracle (or the folder, without one) says of a script.
    enum Verdict {
        /// It compiles.
        Accepted,
        /// It does not; the lines of the errors, when the oracle ran.
        Rejected(Option<BTreeSet<u32>>),
    }

    /// The committed corpus.
    fn committed_corpus() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../sl-lsl/tests/corpus")
    }

    /// Every `.lsl` file under `dir`, sorted.
    fn lsl_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = fs_err::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("lsl") {
                    files.push(path);
                }
            }
        }
        files.sort();
        files
    }

    /// tailslide's verdict on one script: the lines of its `ERROR::`
    /// findings (`ERROR:: ( line, col): …`), empty when it compiles.
    fn tailslide(bin: &Path, script: &Path) -> Result<Verdict, String> {
        let output = Command::new(bin)
            .arg("--lint")
            .arg(script)
            .output()
            .map_err(|error| format!("running tailslide: {error}"))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: BTreeSet<u32> = stderr
            .lines()
            .filter(|line| line.trim_start().starts_with("ERROR::"))
            .filter_map(|line| {
                let (_, after) = line.split_once('(')?;
                after.split(',').next()?.trim().parse().ok()
            })
            .collect();
        let rejected = stderr
            .lines()
            .any(|line| line.trim_start().starts_with("ERROR::"));
        Ok(if rejected {
            Verdict::Rejected(Some(lines))
        } else {
            Verdict::Accepted
        })
    }

    /// The folder's verdict, for the committed corpus without the oracle.
    fn folder(script: &Path) -> Verdict {
        if script
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "error")
        {
            Verdict::Rejected(None)
        } else {
            Verdict::Accepted
        }
    }

    /// The functions tailslide's `builtins.txt` declares — beside the
    /// binary's build directory — or none when it cannot be read.
    fn oracle_functions(bin: &Path) -> BTreeSet<String> {
        let Some(builtins) = bin
            .parent()
            .and_then(Path::parent)
            .map(|repo| repo.join("builtins.txt"))
        else {
            return BTreeSet::new();
        };
        let Ok(text) = fs_err::read_to_string(builtins) else {
            return BTreeSet::new();
        };
        text.lines()
            .map(str::trim)
            .filter(|line| {
                !line.starts_with("//")
                    && !line.starts_with("const ")
                    && !line.starts_with("event ")
            })
            .filter_map(|line| {
                line.split_once('(')?
                    .0
                    .split_whitespace()
                    .nth(1)
                    .map(str::to_owned)
            })
            .collect()
    }

    /// Why a script tailslide accepts was rejected, when it is one of the
    /// known divergences.
    fn known_divergence(
        source: &str,
        errors: &[CompileError],
        oracle_functions: &BTreeSet<String>,
    ) -> Option<&'static str> {
        let parsed = sl_lsl::parse(source);
        if parsed
            .errors
            .iter()
            .any(|error| error.message.ends_with("nests too deeply"))
        {
            return Some("depth ceiling");
        }
        let missing_function = |error: &CompileError| {
            error.kind == CompileErrorKind::Undefined
                && source.get(error.span.clone()).is_some_and(|name| {
                    oracle_functions.contains(name)
                        && sl_lsl_runtime::library::builtin(name).is_none()
                })
        };
        errors
            .iter()
            .all(missing_function)
            .then_some("not in the library table")
    }

    /// A short rendering of compile errors for a report.
    fn describe(errors: &[CompileError]) -> String {
        errors
            .iter()
            .map(|error| {
                format!(
                    "{}:{} {}",
                    error.position.line,
                    error.position.column,
                    error.kind.message()
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    #[test]
    fn the_compiler_agrees_with_the_oracle() -> Result<(), String> {
        let oracle = std::env::var_os("SL_LSL_TAILSLIDE_BIN")
            .map(PathBuf::from)
            .filter(|bin| bin.exists());
        let external = std::env::var_os("SL_LSL_DIFFTEST_CORPUS").map(PathBuf::from);
        if external.is_some() && oracle.is_none() {
            eprintln!("SL_LSL_DIFFTEST_CORPUS needs SL_LSL_TAILSLIDE_BIN; skipped");
            return Ok(());
        }
        let strict = external.is_none();
        let root = external.unwrap_or_else(committed_corpus);
        let files = lsl_files(&root);
        if files.is_empty() {
            return Err(format!("no scripts under {}", root.display()));
        }

        let oracle_functions = oracle.as_deref().map(oracle_functions).unwrap_or_default();
        let mut false_rejections = Vec::new();
        let mut known = Vec::new();
        let mut misses = Vec::new();
        let mut elsewhere = Vec::new();
        for script in &files {
            let verdict = match &oracle {
                Some(bin) => tailslide(bin, script)?,
                None => folder(script),
            };
            let source =
                String::from_utf8_lossy(&fs_err::read(script).map_err(|error| error.to_string())?)
                    .into_owned();
            let name = script
                .strip_prefix(&root)
                .unwrap_or(script)
                .display()
                .to_string();
            match (verdict, compile(&source)) {
                (Verdict::Accepted, Ok(_)) => {}
                (Verdict::Accepted, Err(errors)) => {
                    let report = format!("{name}: {}", describe(&errors));
                    match known_divergence(&source, &errors, &oracle_functions) {
                        Some(why) => known.push(format!("{why}: {report}")),
                        None => false_rejections.push(report),
                    }
                }
                (Verdict::Rejected(_), Ok(_)) => misses.push(name),
                (Verdict::Rejected(lines), Err(errors)) => {
                    let line_count =
                        u32::try_from(source.lines().count().max(1)).unwrap_or(u32::MAX);
                    let plausible = errors.iter().any(|error| match &lines {
                        Some(lines) => lines.contains(&error.position.line),
                        None => (1..=line_count).contains(&error.position.line),
                    });
                    if !plausible {
                        elsewhere
                            .push(format!("{name}: {} (oracle: {lines:?})", describe(&errors)));
                    }
                }
            }
        }

        eprintln!(
            "compile corpus: {} scripts, {} false rejections ({} known), {} misses, {} on another line",
            files.len(),
            false_rejections.len(),
            known.len(),
            misses.len(),
            elsewhere.len()
        );
        for (label, entries) in [
            ("FALSE REJECTION", &false_rejections),
            ("known", &known),
            ("miss", &misses),
            ("other line", &elsewhere),
        ] {
            for entry in entries {
                eprintln!("  {label}: {entry}");
            }
        }
        let failed = !false_rejections.is_empty()
            || (strict && (!misses.is_empty() || !elsewhere.is_empty()));
        if failed {
            Err("the compiler disagrees with the oracle (see above)".to_owned())
        } else {
            Ok(())
        }
    }
}
