#!/usr/bin/env python3
"""Regression suite for approve-python-edit.py.

The hook's only output is an approval, which runs a command with nobody
watching, so the cost of a wrong answer is asymmetric: an over-broad approval
is a silent file write, a missed one is a single prompt. Every case here that
is not plainly an in-project edit therefore expects a refusal.

  python3 .claude/hooks/approve-python-edit-test.py
"""

import importlib.util
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
# The hook is named with hyphens, so it cannot simply be imported.
_spec = importlib.util.spec_from_file_location(
    "approve_python_edit", os.path.join(HERE, "approve-python-edit.py")
)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)
verdict = _mod.verdict

PROJECT = "/home/taladar/devel/new/sl-client"


def heredoc(body, tail="", cd=None):
    cmd = "python3 - <<'PY'\n" + body + "\nPY"
    if tail:
        cmd += " " + tail
    if cd:
        cmd = "cd " + cd + " && " + cmd
    return cmd


MD_EDIT = "p='notes.md'\ns=open(p).read()\nopen(p,'w').write(s.replace('a','b'))"

EDIT = "p='sl-wire/src/lib.rs'\ns=open(p).read()\nassert s.count('a')==1\ns=s.replace('a','b')\nopen(p,'w').write(s)"

APPROVE = [
    ("canonical search/replace", heredoc(EDIT)),
    (
        "several replacements",
        heredoc(
            "p='a.rs'\ns=open(p).read()\ns=s.replace('a','b')\ns=s.replace('c','d')\nopen(p,'w').write(s)"
        ),
    ),
    (
        "pathlib form",
        heredoc(
            "from pathlib import Path\np=Path('a.rs')\np.write_text(p.read_text().replace('a','b'))"
        ),
    ),
    (
        "re.sub form",
        heredoc(
            "import re\np='a.rs'\ns=open(p).read()\nopen(p,'w').write(re.sub('a','b',s))"
        ),
    ),
    (
        "encoding keyword",
        heredoc(
            "p='a.rs'\ns=open(p,encoding='utf-8').read()\nopen(p,'w',encoding='utf-8').write(s.replace('a','b'))"
        ),
    ),
    (
        "assert then edit, printing a count",
        heredoc(
            "p='a.rs'\ns=open(p).read()\nprint(s.count('x'))\nopen(p,'w').write(s.replace('x','y'))"
        ),
    ),
    ("scratchpad file", heredoc("open('/tmp/notes.txt','w').write('hello')")),
    ("read-only tail", heredoc(EDIT, "&& grep -n 'b' sl-wire/src/lib.rs")),
    ("two read-only tails", heredoc(EDIT, "&& head -5 a.rs | wc -l")),
    ("tail redirecting to /dev/null", heredoc(EDIT, "&& grep -c b a.rs >/dev/null")),
    ("sed reading, not editing", heredoc(EDIT, "&& sed -n '1,20p' a.rs")),
    ("awk that only prints", heredoc(EDIT, "&& awk '{print $1}' a.rs")),
    ("find that only lists", heredoc(EDIT, "&& find src -name '*.rs'")),
    # Formatting the file you just edited: bounded by the edit itself.
    ("rumdl check on the edited file", heredoc(MD_EDIT, "&& rumdl check notes.md")),
    ("rumdl fmt on the edited file", heredoc(MD_EDIT, "&& rumdl fmt notes.md")),
    ("typos on the edited file", heredoc(MD_EDIT, "&& typos notes.md")),
    (
        "formatter with a flag and the edited file",
        heredoc(MD_EDIT, "&& rumdl check --no-color notes.md"),
    ),
    (
        "formatter on the edited file, output piped",
        heredoc(MD_EDIT, "&& rumdl check notes.md 2>&1 | tail -5"),
    ),
    # `>` inside a quoted awk program is a comparison, not a shell redirect.
    (
        "awk comparing a length",
        heredoc(EDIT, "&& awk 'length > 80 {print NR\": \"length}' a.rs"),
    ),
    (
        "awk comparison, piped",
        heredoc(EDIT, "&& awk 'length > 80 {print FILENAME\":\"FNR}' a.rs | head"),
    ),
    ("grep for a literal angle bracket", heredoc(EDIT, "&& grep -n 'a > b' a.rs")),
    # Regenerating the index it exclusively owns.
    ("roadmap index regenerated", heredoc(EDIT, "&& python3 roadmap/index.py")),
    (
        "roadmap index checked",
        heredoc(EDIT, "&& python3 roadmap/index.py --check 2>&1 | tail -3"),
    ),
    ("roadmap index by ./ path", heredoc(EDIT, "&& python3 ./roadmap/index.py")),
    # Whole-tree, but the commit hook holds the tree to it anyway.
    ("cargo fmt over the workspace", heredoc(EDIT, "&& cargo fmt --all")),
    ("cargo sort over the workspace", heredoc(EDIT, "&& cargo sort --workspace")),
    ("cd into the project first", heredoc(EDIT, cd=PROJECT)),
    # Inert inside a quoted heredoc: the shell hands the body over untouched,
    # so this writes the six characters `$(id)` and runs nothing.
    (
        "substitution-looking text in the body",
        heredoc("open('/tmp/x.txt','w').write('cost is $(id) dollars')"),
    ),
    (
        "backtick in replacement text",
        heredoc("p='a.rs'\ns=open(p).read()\nopen(p,'w').write(s.replace('x','`y`'))"),
    ),
]

REFUSE = [
    # --- the body is not an edit ---
    ("imports subprocess", heredoc("import subprocess\nsubprocess.run(['id'])")),
    ("imports os", heredoc("import os\nprint(os.listdir('.'))")),
    ("loops", heredoc("for f in ['a','b']:\n    open(f,'w').write('x')")),
    ("defines a function", heredoc("def f():\n    pass\nf()")),
    ("reaches through a dunder", heredoc("print(().__class__.__bases__)")),
    ("calls eval", heredoc("eval('1+1')")),
    # --- the path is not one we checked ---
    (
        "absolute path outside the project",
        heredoc("open('/etc/passwd','w').write('x')"),
    ),
    ("parent-directory escape", heredoc("open('../../etc/passwd','w').write('x')")),
    ("home-relative path", heredoc("open('~/.ssh/authorized_keys','w').write('x')")),
    (
        "augassign moves the path",
        heredoc("p='a.rs'\np+='/../../../../etc/passwd'\nopen(p,'w').write('x')"),
    ),
    (
        "rebound after its literal",
        heredoc("p='a.rs'\np='/etc/passwd'\nopen(p,'w').write('x')"),
    ),
    ("path by keyword", heredoc("open(file='/etc/passwd',mode='w').write('x')")),
    ("path by kwargs splat", heredoc("d={'file':'/etc/passwd'}\nopen(**d).write('x')")),
    ("computed path", heredoc("p='/etc/'+'passwd'\nopen(p,'w').write('x')")),
    ("f-string path", heredoc("n='passwd'\nopen(f'/etc/{n}','w').write('x')")),
    ("walrus path", heredoc("open((p:='/etc/passwd'),'w').write('x')")),
    ("star-args", heredoc("a=['/etc/passwd','w']\nopen(*a).write('x')")),
    (
        "print redirected into a file",
        heredoc("print('x',file=open('/etc/passwd','w'))"),
    ),
    # --- the shell around it is not safe ---
    ("unquoted heredoc delimiter", "python3 - <<PY\n" + EDIT + "\nPY"),
    ("command substitution in the tail", heredoc(EDIT, "&& echo `id`")),
    ("command substitution in the cd target", heredoc(EDIT, cd="$(echo /etc)")),
    ("build chained after the edit", heredoc(EDIT, "&& cargo build")),
    ("commit chained after the edit", heredoc(EDIT, "&& git commit -m x")),
    ("rm chained after the edit", heredoc(EDIT, "&& rm -rf /home/taladar/devel")),
    (
        "tail redirecting into a file",
        heredoc(EDIT, "&& grep -c b a.rs > /home/taladar/out"),
    ),
    # A formatter is bounded by the edit; these reach past it.
    (
        "formatter on a file that was not edited",
        heredoc(MD_EDIT, "&& rumdl fmt other.md"),
    ),
    ("formatter over a whole directory", heredoc(MD_EDIT, "&& rumdl fmt book/src")),
    ("formatter with no file argument", heredoc(MD_EDIT, "&& rumdl fmt")),
    (
        "formatter on a file only read, not written",
        heredoc(
            "p='notes.md'\nq='other.md'\ns=open(p).read()\nopen(q,'w').write(s)",
            "&& rumdl fmt notes.md",
        ),
    ),
    ("some other python script", heredoc(EDIT, "&& python3 tools/release.py")),
    ("another script in roadmap/", heredoc(EDIT, "&& python3 roadmap/other.py")),
    (
        "a real redirect outside quotes",
        heredoc(EDIT, "&& awk '{print $1}' a.rs > out.txt"),
    ),
    ("cargo build is not a formatter", heredoc(EDIT, "&& cargo build")),
    ("cargo test is not a formatter", heredoc(EDIT, "&& cargo test -p x")),
    # Read-by-default commands, asked to write. The head alone would pass.
    ("sed -i rewrites in place", heredoc(EDIT, "&& sed -i 's/a/b/' a.rs")),
    ("sed with bundled -i flag", heredoc(EDIT, "&& sed -ne 1p -i a.rs")),
    (
        "awk redirecting from inside its program",
        heredoc(EDIT, "&& awk '{print > \"out.txt\"}' a.rs"),
    ),
    ("find -delete", heredoc(EDIT, "&& find . -name '*.bak' -delete")),
    ("find -exec", heredoc(EDIT, "&& find . -name '*.rs' -exec rm {} +")),
    (
        "a second heredoc in the tail",
        heredoc(EDIT, "&& python3 - <<'P2'\nprint(1)\nP2"),
    ),
    ("cd out of the project first", heredoc(EDIT, cd="/etc")),
    ("not a heredoc at all", "python3 script.py"),
    ("python -c", "python3 -c 'import os; os.system(\"id\")'"),
    ("an ordinary command", "ls -la"),
]


def run():
    passed = failed = 0
    for want_ok, cases in ((True, APPROVE), (False, REFUSE)):
        print(
            "-- expected to be approved --"
            if want_ok
            else "\n-- expected to be refused --"
        )
        for name, command in cases:
            reason = verdict(command, PROJECT, PROJECT)
            ok = (reason is None) if want_ok else (reason is not None)
            if ok:
                passed += 1
                print("  ok    {}{}".format(name, "" if want_ok else f"  ({reason})"))
            else:
                failed += 1
                print(
                    "FAIL    {}: {}".format(
                        name, f"refused: {reason}" if want_ok else "approved"
                    )
                )

    # The hook must also behave as a hook: right shape on stdout, and silent
    # rather than crashing on input it was not built for.
    print("\n-- as a process --")
    hook = os.path.join(HERE, "approve-python-edit.py")
    for name, payload, expect_allow in (
        (
            "approves via stdin",
            {
                "tool_name": "Bash",
                "tool_input": {"command": heredoc(EDIT)},
                "cwd": PROJECT,
            },
            True,
        ),
        (
            "silent on a refusal",
            {"tool_name": "Bash", "tool_input": {"command": "ls -la"}},
            False,
        ),
        (
            "silent on another tool",
            {"tool_name": "Edit", "tool_input": {"file_path": "/etc/passwd"}},
            False,
        ),
        ("silent on junk input", {"nonsense": True}, False),
    ):
        proc = subprocess.run(
            [sys.executable, hook],
            input=json.dumps(payload),
            capture_output=True,
            text=True,
            env={**os.environ, "CLAUDE_PROJECT_DIR": PROJECT},
            check=False,  # a non-zero exit is itself one of the things under test
        )
        got_allow = (
            bool(proc.stdout.strip())
            and json.loads(proc.stdout)["hookSpecificOutput"]["permissionDecision"]
            == "allow"
        )
        if proc.returncode == 0 and got_allow == expect_allow:
            passed += 1
            print(f"  ok    {name}")
        else:
            failed += 1
            print(f"FAIL    {name}: rc={proc.returncode} stdout={proc.stdout[:80]!r}")

    print(f"\n{passed} passed, {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(run())
