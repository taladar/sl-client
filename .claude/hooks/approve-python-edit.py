#!/usr/bin/env python3
"""PreToolUse hook: approve a `python3 - <<'PY'` heredoc that is only an edit.

Editing a file through a Python heredoc is a normal move here, and every one of
them costs a permission prompt: 855 in the recorded transcripts, the largest
single category left after the allow-rules. They are not all alike, though --
some import subprocess, some loop, some write outside the tree -- so this does
not pattern-match the text. It parses the body with `ast` and approves only a
body that reads files, does string replacements, and writes them back.

Anything else stays silent, which Claude Code reads as "no opinion" and turns
into the usual prompt. Silence is the safe direction and every unhandled shape
takes it; the only output this ever produces is an approval it can justify.

The shell around the body matters as much as the body:

  * A hook approves the WHOLE command, so `<heredoc> && cargo build` cannot be
    approved on the strength of the heredoc alone. Every trailing segment is
    judged on its own: read-only, or a formatter naming a file this very body
    just wrote.
  * The heredoc delimiter must be quoted. With a bare `<<PY` the shell expands
    `$(...)` and backticks in the body *before* Python sees it, so the text
    that was parsed is not the text that runs.
"""

import ast
import json
import os
import re
import sys

# Calls the body may make. String methods that build or search text, plus the
# handful of builtins these scripts use to report what they did.
ALLOWED_METHODS = {
    "read",
    "write",
    "replace",
    "read_text",
    "write_text",
    "strip",
    "rstrip",
    "lstrip",
    "splitlines",
    "join",
    "split",
    "count",
    "format",
    "startswith",
    "endswith",
    "index",
    "find",
    "upper",
    "lower",
    "sub",
    "escape",
    "group",
    "search",
    "match",
    "Path",
}
ALLOWED_FUNCS = {
    "open",
    "print",
    "len",
    "str",
    "int",
    "repr",
    "sorted",
    "list",
    "set",
    "Path",
    "abs",
    "min",
    "max",
}
ALLOWED_IMPORTS = {"pathlib", "re", "Path"}
BANNED_NAMES = {
    "eval",
    "exec",
    "compile",
    "__import__",
    "getattr",
    "setattr",
    "globals",
    "locals",
    "vars",
    "input",
    "breakpoint",
    "subprocess",
    "os",
    "sys",
    "shutil",
    "socket",
    "urllib",
    "requests",
}
# Control flow is not refused because a loop is dangerous, but because it makes
# the path analysis below unsound: what a name holds at the point of an open()
# stops being decidable from the syntax alone.
BANNED_NODES = (
    ast.For,
    ast.While,
    ast.If,
    ast.FunctionDef,
    ast.AsyncFunctionDef,
    ast.ClassDef,
    ast.With,
    ast.Try,
    ast.Lambda,
    ast.ListComp,
    ast.DictComp,
    ast.SetComp,
    ast.GeneratorExp,
    ast.IfExp,
    ast.Delete,
    ast.Raise,
    ast.Global,
    ast.Nonlocal,
    ast.NamedExpr,
    ast.Starred,
)

# Commands allowed to follow the heredoc on the strength of the head alone.
# Read-only -- though read-only *by default* is not the same as read-only, which
# is what TAIL_WRITE_FLAGS below is for. Anything that writes as a matter of
# course belongs in FORMATTER_HEADS instead, where it has to name its file.
SAFE_TAIL_HEADS = {
    "grep",
    "rg",
    "tail",
    "head",
    "cat",
    "wc",
    "sort",
    "uniq",
    "cut",
    "echo",
    "printf",
    "true",
    "ls",
    "diff",
    "sed",
    "awk",
    "find",
}
# Formatters and linters, allowed on one condition: every file they name must
# be a file the heredoc just wrote. Re-formatting what you have this moment
# edited is the common idiom here -- 40 of the 72 recorded `rumdl` tails do
# exactly that -- and it is bounded in a way a bare head name is not: the
# formatter can only reach what the edit already reached, so approving it adds
# no file to the blast radius. A formatter naming no file (`cargo fmt --all`,
# `rumdl fmt book/src`) is refused: whole-tree is not what was just edited.
FORMATTER_HEADS = {
    "rumdl",
    "typos",
    "rustfmt",
    "shfmt",
    "tombi",
    "prettier",
    "ruff",
    "black",
    "yamllint",
    "shellcheck",
}
# Tools exempt from the "name the edited file" rule, for two distinct reasons.
#
#   * `cargo fmt` and `cargo sort`: the commit hook already holds the whole tree
#     to their output on every commit, so running one tree-wide can only ever be
#     a no-op or a fix the next commit would have demanded anyway.
#   * `roadmap/index.py`: it regenerates roadmap/INDEX.md, a file nothing else
#     writes, from the roadmap items -- and the same hook runs it with --check.
#     Its whole output is one file it exclusively owns.
#
# Both reasons are properties of THIS repo, and an adopter without those hooks
# should empty the set. Matched on (head, first word), with the first word
# normalised, so `./roadmap/index.py` and `roadmap/index.py` are the same entry.
WHOLE_TREE_TOOLS = {
    ("cargo", "fmt"),
    ("cargo", "sort"),
    ("python3", "roadmap/index.py"),
}
# Read-only subcommands of a tool whose bare head is far too broad to admit.
# Kept apart from WHOLE_TREE_TOOLS deliberately: that set admits things whose
# whole-tree effect is already accounted for, this one admits things that plainly
# do not write, and merging them would leave one comment explaining both badly.
# `git status` and `git diff` are already allowed outright by this repo's rules,
# so these grant nothing new -- they close the gap where each half of
# `<edit> && git status` is permitted but the whole is not. The rest of git stays
# out on purpose: `git add -A` stages whatever else is in the tree, `git mv`
# renames, and `git checkout <file>` discards uncommitted work outright.
SAFE_TAIL_SUBCOMMANDS = {
    ("git", "status"),
    ("git", "diff"),
    ("git", "log"),
    ("git", "show"),
}
# Several of the above read by default and write when asked. Naming the head is
# therefore not enough: `sed -i` rewrites in place, an awk program can redirect
# to a file from inside its own script, `typos -w` fixes what it finds, and
# `find -delete`/`-exec` does whatever it likes. Each is refused by the flag
# that turns it into a writer, which keeps the ordinary reading use.
TAIL_WRITE_FLAGS = {
    "sed": re.compile(r"(^|\s)-[a-zA-Z]*i"),
    "awk": re.compile(r"print[^;}]*>"),
    "find": re.compile(r"(^|\s)-(delete|exec|execdir|fprint|fls)(\s|$)"),
}

HEREDOC_RE = re.compile(
    r"^python3?\s+-\s*<<\s*'([A-Za-z_][A-Za-z0-9_]*)'\s*\n(.*?)\n\1(?:\s|$)(.*)$",
    re.DOTALL,
)
CD_PREFIX_RE = re.compile(r"^cd\s+([^\s&|;<>]+)\s*&&\s*(.*)$", re.DOTALL)
OPERATOR_RE = re.compile(r"&&|\|\||[|;]")


def _literal_env(tree):
    """Map each name to its string literal, dropping any name bound otherwise.

    The idiom is `p='file.rs'` followed by `open(p)`, so the path check needs
    this to see anything at all. A name rebound by `+=`, by tuple unpacking, or
    to a non-literal is removed rather than trusted -- `p='ok'; p+='/../etc/x'`
    would otherwise be checked as "ok".
    """
    env, poisoned = {}, set()
    for node in ast.walk(tree):
        if isinstance(node, ast.AugAssign) and isinstance(node.target, ast.Name):
            poisoned.add(node.target.id)
        elif isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name):
                    is_literal = (
                        len(node.targets) == 1
                        and isinstance(node.value, ast.Constant)
                        and isinstance(node.value.value, str)
                    )
                    if is_literal:
                        if target.id in env and env[target.id] != node.value.value:
                            poisoned.add(target.id)
                        env[target.id] = node.value.value
                    else:
                        poisoned.add(target.id)
                else:
                    for sub in ast.walk(target):
                        if isinstance(sub, ast.Name):
                            poisoned.add(sub.id)
    for name in poisoned:
        env.pop(name, None)
    return env


def _has_substitution(text):
    """True if the shell would run something inside this fragment."""
    return "`" in text or "$(" in text


def _outside_quotes(text):
    """The text with single- and double-quoted spans blanked out.

    An operator only operates where the shell can see it, so scanning for
    redirects has to ignore quoted text. Spans are replaced by spaces rather
    than deleted so that nothing either side of a quote is accidentally joined.
    """
    out, quote = [], ""
    for char in text:
        if quote:
            out.append(" ")
            if char == quote:
                quote = ""
        elif char in "'\"":
            quote = char
            out.append(" ")
        else:
            out.append(char)
    return "".join(out)


def _call_name(node):
    func = node.func
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return None


def _path_is_inside(path, cwd, project_dir):
    """True if a path the body opens stays in the project (or the scratchpad)."""
    resolved = os.path.normpath(os.path.join(cwd, os.path.expanduser(path)))
    if resolved.startswith("/tmp/"):
        return True
    project = os.path.normpath(project_dir)
    return resolved == project or resolved.startswith(project + os.sep)


def check_body(body, cwd, project_dir):
    """None if the Python body is an approvable edit, else a short reason."""
    try:
        tree = ast.parse(body)
    except SyntaxError:
        return "syntax"

    for node in ast.walk(tree):
        if isinstance(node, BANNED_NODES):
            return "control-flow/" + type(node).__name__
        if isinstance(node, ast.Attribute) and node.attr.startswith("__"):
            return "dunder"
        if isinstance(node, ast.Name) and node.id in BANNED_NAMES:
            return "banned/" + node.id
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            module = node.module if isinstance(node, ast.ImportFrom) else None
            names = [a.name.split(".")[0] for a in node.names]
            for name in ([module] if module else []) + names:
                if name and name not in ALLOWED_IMPORTS:
                    return "import/" + name
        if isinstance(node, ast.Call):
            func = node.func
            if isinstance(func, ast.Name):
                if func.id not in ALLOWED_FUNCS:
                    return "func/" + func.id
            elif isinstance(func, ast.Attribute):
                if func.attr not in ALLOWED_METHODS:
                    return "method/." + func.attr
            else:
                return "call/computed"

    env = _literal_env(tree)
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or _call_name(node) not in ("open", "Path"):
            continue
        # The path has to arrive positionally. `open(file=...)` or `open(**d)`
        # would otherwise hand over a path nothing here ever looks at.
        if not node.args or any(
            kw.arg in (None, "file", "path") for kw in node.keywords
        ):
            return "path/not-positional"
        first = node.args[0]
        if isinstance(first, ast.Constant) and isinstance(first.value, str):
            path = first.value
        elif isinstance(first, ast.Name) and first.id in env:
            path = env[first.id]
        else:
            return "path/computed"
        if not _path_is_inside(path, cwd, project_dir):
            return "path/outside-project"
    return None


def written_paths(tree, cwd):
    """Absolute paths the body opens for writing, as far as syntax can tell."""
    env = _literal_env(tree)

    def resolve(node):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            return node.value
        if isinstance(node, ast.Name) and node.id in env:
            return env[node.id]
        return None

    out = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        func, name, arg = node.func, _call_name(node), None
        if name == "open" and len(node.args) >= 2:
            mode = node.args[1]
            if isinstance(mode, ast.Constant) and "w" in str(mode.value):
                arg = node.args[0]
        elif name == "write_text" and isinstance(func, ast.Attribute):
            owner = func.value
            if isinstance(owner, ast.Call) and owner.args:
                arg = owner.args[0]
            elif isinstance(owner, ast.Name):
                arg = owner
        path = resolve(arg) if arg is not None else None
        if path:
            out.add(os.path.normpath(os.path.join(cwd, os.path.expanduser(path))))
    return out


def check_tail(rest, edited, cwd):
    """None if what follows the heredoc is safe, else a short reason.

    `edited` is the set of absolute paths the body wrote; a formatter is
    admitted only for those.
    """
    if not rest:
        return None
    if "<<" in rest:
        return "tail/second-heredoc"
    for segment in (s.strip() for s in OPERATOR_RE.split(rest)):
        if not segment:
            continue
        # `2>&1` and `>/dev/null` are fine; a redirect to a real file is a write
        # this has not checked, whatever the command in front of it is. Only the
        # unquoted part counts -- a `>` the shell never sees is not a redirect,
        # and `awk 'length > 80 {print NR}'` is a comparison, not a write. An
        # awk program that really does redirect is caught by TAIL_WRITE_FLAGS,
        # which reads the quoted text this deliberately drops.
        for match in re.finditer(r"(?<!\d)>+\s*([^\s|;&]+)", _outside_quotes(segment)):
            if match.group(1) not in ("/dev/null", "&1", "&2"):
                return "tail/redirect"
        words = segment.split()
        head = os.path.basename(words[0]) if words else ""
        subcommand = os.path.normpath(words[1]) if len(words) > 1 else ""

        if (head, subcommand) in WHOLE_TREE_TOOLS:
            continue
        if (head, subcommand) in SAFE_TAIL_SUBCOMMANDS:
            continue
        if head in FORMATTER_HEADS:
            targets = _file_arguments(words[1:])
            if not targets:
                return f"tail/{head}-whole-tree"
            for target in targets:
                if (
                    os.path.normpath(os.path.join(cwd, os.path.expanduser(target)))
                    not in edited
                ):
                    return f"tail/{head}-other-file"
            continue
        if head not in SAFE_TAIL_HEADS:
            return "tail/" + head
        writer = TAIL_WRITE_FLAGS.get(head)
        if writer and writer.search(segment):
            return f"tail/{head}-writes"
    return None


# A leading verb like `rumdl check` is a subcommand, not a path to compare.
SUBCOMMAND_WORDS = {"check", "fmt", "format", "lint", "sort", "run", "fix"}


def _file_arguments(words):
    """The words that name a file: not flags, not subcommands, not values."""
    out = []
    skip = False
    if words and words[0] in SUBCOMMAND_WORDS:
        words = words[1:]
    for word in words:
        if skip:
            skip = False
            continue
        if word.startswith("-"):
            # A flag that takes a value would otherwise swallow it as a path.
            skip = word in ("--config", "--config-path", "-c", "--stdin-filename")
            continue
        if ">" in word or "<" in word or word in ("&1", "&2"):
            continue
        out.append(word)
    return out


def verdict(command, cwd, project_dir):
    """None if the whole Bash command may be approved, else a short reason."""
    command = command.strip()

    match = CD_PREFIX_RE.match(command)
    if match:
        if _has_substitution(match.group(1)):
            return "command-substitution"
        target = os.path.expanduser(match.group(1))
        cwd = os.path.normpath(os.path.join(cwd, target))
        if not _path_is_inside(".", cwd, project_dir):
            return "cd/outside-project"
        command = match.group(2).strip()

    match = HEREDOC_RE.match(command)
    if not match:
        return "not-a-quoted-heredoc"
    body, rest = match.group(2), match.group(3).strip()

    # Command substitution is only checked where the shell would act on it: the
    # cd target above and the tail below. Inside the body it is inert -- the
    # delimiter is quoted (HEREDOC_RE insists), so the shell passes the body
    # through untouched and a `$(...)` in a Python string is just text. Testing
    # the whole command for it instead refuses 185 of the 576 recorded calls
    # for a substitution that never runs.
    if _has_substitution(rest):
        return "command-substitution"

    # The body is judged first, because the tail rule depends on it: a
    # formatter is admitted only for the files the body itself wrote.
    reason = check_body(body, cwd, project_dir)
    if reason:
        return reason
    return check_tail(rest, written_paths(ast.parse(body), cwd), cwd)


def main():
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        return 0
    if payload.get("tool_name") != "Bash":
        return 0
    command = (payload.get("tool_input") or {}).get("command")
    if not isinstance(command, str):
        return 0

    project_dir = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    cwd = payload.get("cwd") or os.getcwd()
    if verdict(command, cwd, project_dir) is None:
        json.dump(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "permissionDecisionReason": "A Python heredoc that only reads, replaces and writes "
                    "back files inside the project.",
                }
            },
            sys.stdout,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
