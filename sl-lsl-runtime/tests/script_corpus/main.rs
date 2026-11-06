//! The executable LSL corpus: scripts under `corpus/`, each run in the VM
//! against a recording host for a fixed number of ticks, and the transcript of
//! everything it did set against the one checked in beside it.
//!
//! This is where the language's semantics are pinned as *behaviour* — what a
//! script prints, says and ends up holding — rather than as unit tests of the
//! helpers that implement them, so the cases survive a rewrite of the VM. No
//! grid, no network, no runtime: source in, transcript out.
//!
//! A case is `corpus/<area>/<name>.lsl` (its header is described in
//! [`case`]) and its expectation `<name>.expected`. A case runs under every
//! flavour its header names; when the flavours do not agree, each keeps its
//! own `<name>.<flavour>.expected` instead.
//!
//! **Adding or changing a case:** write the script, run
//! `UPDATE_EXPECT=1 cargo test -p sl-lsl-runtime --test script_corpus`, and
//! read the transcripts it wrote before committing them — an expectation is a
//! claim about LSL, and the test only holds the runtime to what you accept.
//!
//! A transcript is:
//!
//! - one line per stimulus (`@3 > touch`), stamped with the tick before which
//!   it was posted;
//! - one line per host call that does something (`@3 say channel=0 …`),
//!   stamped with the tick it was made in;
//! - an `end` line: the state the script is in, whether it is running and how
//!   many events are left queued;
//! - every global, in declaration order, with its final value — floats in the
//!   shortest form that reads back as the same `f32`.

mod case;
mod host;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use sl_lsl_runtime::compile_for;
    use sl_lsl_runtime::library::event;
    use sl_lsl_runtime::value::Value;
    use sl_lsl_runtime::vm::comms::Heard;
    use sl_lsl_runtime::vm::{CallerId, Detected, Engine, EngineConfig, EngineFlavour};

    use super::case::{Case, Stimulus, dialect};
    use super::host::{AT, CorpusHost, OWNER, OWNER_NAME, value, vector_text};

    /// The corpus's step: a tenth of a second, so `llSleep(1.0)` is ten ticks.
    const STEP: Duration = Duration::from_millis(100);

    /// The one script a case runs.
    const SCRIPT: CallerId = CallerId(1);

    /// The environment variable that rewrites the expectations instead of
    /// checking them.
    const UPDATE: &str = "UPDATE_EXPECT";

    /// The corpus root.
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
    }

    /// Every file below `dir`, sorted.
    fn files(dir: &Path) -> Result<Vec<PathBuf>, String> {
        let mut found = Vec::new();
        let mut entries = fs_err::read_dir(dir)
            .map_err(|error| error.to_string())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort();
        for path in entries {
            if path.is_dir() {
                found.extend(files(&path)?);
            } else {
                found.push(path);
            }
        }
        Ok(found)
    }

    /// A path below the corpus root, without the extension, `/`-separated.
    fn id_of(path: &Path) -> String {
        let below = path.strip_prefix(root()).unwrap_or(path);
        let text = below
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        // A flavour's expectation has a second extension to drop.
        text.split('.').next().unwrap_or_default().to_owned()
    }

    /// Every case in the corpus.
    fn cases() -> Result<Vec<Case>, String> {
        files(&root())?
            .iter()
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("lsl"))
            .map(|path| Case::read(id_of(path), path))
            .collect()
    }

    /// Posts `stimulus` to the script.
    fn post(engine: &mut Engine, stimulus: &Stimulus) -> Result<(), String> {
        let named = |name: &str| event(name).ok_or_else(|| format!("no event {name}"));
        let posted = match stimulus {
            Stimulus::Touch => {
                let toucher = Detected {
                    key: OWNER.to_owned(),
                    name: OWNER_NAME.to_owned(),
                    owner: OWNER.to_owned(),
                    // `AGENT`.
                    kind: 1,
                    position: AT,
                    ..Detected::blank()
                };
                let (start, end) = (named("touch_start")?, named("touch_end")?);
                engine
                    .post_detected(SCRIPT, start, vec![toucher.clone()])
                    .and_then(|_| engine.post_detected(SCRIPT, end, vec![toucher]))
                    .map(drop)
            }
            Stimulus::Chat { channel, message } => engine
                .hear(
                    SCRIPT,
                    &Heard {
                        channel: *channel,
                        name: OWNER_NAME.to_owned(),
                        speaker: OWNER.to_owned(),
                        message: message.clone(),
                    },
                )
                .map(drop),
            Stimulus::Link { num, text, id } => engine
                .post(
                    SCRIPT,
                    named("link_message")?,
                    vec![
                        Value::Integer(1),
                        Value::Integer(*num),
                        Value::String(text.clone()),
                        Value::Key(id.clone()),
                    ],
                )
                .map(drop),
            Stimulus::Changed(bits) => engine.changed(SCRIPT, *bits).map(drop),
            Stimulus::Rez(param) => engine.rez(SCRIPT, *param).map(drop),
        };
        posted.map_err(|error| format!("posting {stimulus:?}: {error:?}"))
    }

    /// Runs `case` under `flavour` and returns its transcript.
    fn transcript(case: &Case, flavour: EngineFlavour) -> Result<String, String> {
        let program = compile_for(&case.source, dialect(flavour)).map_err(|errors| {
            let shown: Vec<String> = errors.iter().map(ToString::to_string).collect();
            format!("does not compile:\n{}", shown.join("\n"))
        })?;
        let program = Arc::new(program);
        let mut config = EngineConfig::for_step(STEP);
        config.flavour = flavour;
        let mut engine = Engine::new(config);
        let _none = engine.add(SCRIPT, Arc::clone(&program));
        let mut host = CorpusHost::new(SCRIPT);
        for tick in 1..=case.ticks {
            host.now = tick;
            for (_at, stimulus) in case.stimuli.iter().filter(|(at, _)| *at == tick) {
                host.lines.push(format!("@{tick} > {}", describe(stimulus)));
                post(&mut engine, stimulus)?;
            }
            let _report = engine.tick(&mut host);
            for bits in core::mem::take(&mut host.owed_changes) {
                engine
                    .changed(SCRIPT, bits)
                    .map_err(|error| format!("raising changed({bits}): {error:?}"))?;
            }
            if host.died {
                let _gone = engine.remove(SCRIPT);
                host.lines.push(format!("@{tick} gone"));
                break;
            }
        }
        let mut lines = host.lines;
        if let Some(instance) = engine.instance(SCRIPT) {
            let state = program
                .states
                .get(usize::try_from(instance.state().0).unwrap_or(usize::MAX))
                .map_or("?", |state| state.name.as_str());
            lines.push(format!(
                "end state={state} running={} queued={}",
                instance.is_running(),
                instance.queued()
            ));
            for (global, held) in program.globals.iter().zip(instance.globals()) {
                lines.push(format!("global {} = {}", global.name, value(held)));
            }
        }
        lines.push(String::new());
        Ok(lines.join("\n"))
    }

    /// A stimulus as its transcript line names it.
    fn describe(stimulus: &Stimulus) -> String {
        match stimulus {
            Stimulus::Touch => format!("touch by {OWNER} at {}", vector_text(&AT)),
            Stimulus::Chat { channel, message } => format!("chat channel={channel} {message:?}"),
            Stimulus::Link { num, text, id } => format!("link {num} {text:?} {id:?}"),
            Stimulus::Changed(bits) => format!("changed {bits}"),
            Stimulus::Rez(param) => format!("rez {param}"),
        }
    }

    /// What a flavour's run is held to: its own expectation when it has one,
    /// the shared one otherwise; [`None`] when there is neither.
    fn expected(case: &Case, flavour: &str) -> Result<Option<String>, String> {
        let own = case.flavour_expectation(flavour);
        let path = if own.exists() {
            own
        } else {
            case.shared_expectation()
        };
        if !path.exists() {
            return Ok(None);
        }
        fs_err::read_to_string(&path)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    /// Writes `case`'s expectations from its runs: one shared file when every
    /// flavour agrees, one per flavour when they do not, and none left over
    /// from the other arrangement.
    fn update(case: &Case, runs: &[(&str, String)]) -> Result<(), String> {
        let write = |path: PathBuf, text: &str| {
            fs_err::write(path, text).map_err(|error| error.to_string())
        };
        let remove = |path: PathBuf| {
            if path.exists() {
                fs_err::remove_file(path).map_err(|error| error.to_string())
            } else {
                Ok(())
            }
        };
        let agree = runs.windows(2).all(|pair| match pair {
            [(_, first), (_, second)] => first == second,
            _ => true,
        });
        if let (true, Some((_, text))) = (agree, runs.first()) {
            write(case.shared_expectation(), text)?;
            for (flavour, _) in super::case::FLAVOURS {
                remove(case.flavour_expectation(flavour))?;
            }
        } else {
            remove(case.shared_expectation())?;
            for (flavour, text) in runs {
                write(case.flavour_expectation(flavour), text)?;
            }
        }
        Ok(())
    }

    /// Every expectation file must belong to a case: one left behind by a
    /// renamed or deleted script would otherwise go on looking like coverage.
    fn orphans(cases: &[Case]) -> Result<Vec<String>, String> {
        Ok(files(&root())?
            .iter()
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("expected"))
            .filter(|path| !cases.iter().any(|case| case.id == id_of(path)))
            .map(|path| path.display().to_string())
            .collect())
    }

    /// **The corpus**: every case under every flavour it names, held to its
    /// expected transcript — or, with `UPDATE_EXPECT` set, rewriting it.
    #[test]
    fn every_case_does_what_its_transcript_says() -> Result<(), String> {
        let cases = cases()?;
        assert!(!cases.is_empty(), "the corpus has no case");
        let updating = std::env::var_os(UPDATE).is_some();
        let mut failures = Vec::new();
        for case in &cases {
            let mut runs = Vec::new();
            for (name, flavour) in &case.flavours {
                match transcript(case, *flavour) {
                    Ok(text) => runs.push((*name, text)),
                    Err(error) => failures.push(format!("{} on {name}: {error}", case.id)),
                }
            }
            if updating {
                update(case, &runs)?;
                continue;
            }
            // Flavours that miss the same expectation the same way are one
            // failure, named once for all of them.
            let mut missed: Vec<(Vec<&str>, Option<String>, &str)> = Vec::new();
            for (name, actual) in &runs {
                let want = expected(case, name)?;
                if want.as_deref() == Some(actual.as_str()) {
                    continue;
                }
                match missed.iter_mut().find(|(_, other_want, other_actual)| {
                    *other_want == want && other_actual == actual
                }) {
                    Some((names, _, _)) => names.push(name),
                    None => missed.push((vec![name], want, actual)),
                }
            }
            for (names, want, actual) in missed {
                let on = names.join(", ");
                failures.push(want.map_or_else(
                    || {
                        format!(
                            "{} on {on}: no expected transcript; write one with {UPDATE}=1",
                            case.id
                        )
                    },
                    |want| {
                        format!(
                            "{} on {on} did not do what its transcript says \
                             (< expected, > actual):\n{}",
                            case.id,
                            pretty_assertions::StrComparison::new(&want, actual)
                        )
                    },
                ));
            }
        }
        for orphan in orphans(&cases)? {
            failures.push(format!("{orphan} belongs to no case"));
        }
        // Printed with `Display`, so the diffs keep their lines.
        assert!(failures.is_empty(), "{}", failures.join("\n\n"));
        Ok(())
    }
}
