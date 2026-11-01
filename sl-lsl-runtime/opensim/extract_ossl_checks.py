"""Derive `ossl-checks.txt` from OpenSim's `OSSL_Api.cs`.

Every public `os*` method of the OSSL API opens with the check that decides
whether a script may call it: `CheckThreatLevel(ThreatLevel.<level>,
"<name>")`, the per-function check against the region's `[OSSL]`
configuration; a bare `CheckThreatLevel()`, which asks only that OSSL is
enabled; or nothing at all. A deprecated method then calls
`OSSLDeprecated("<old>", "<new>")`. This script reads both and writes one line
per function:

    <function> <check> [<checked name>] [<replacement>]

where `<check>` is `none`, `enabled` or a threat level, `<checked name>` is
the name the per-function check looks its `Allow_` line up under (written
only where it is not the function's own), and `<replacement>` is what a
deprecated function's warning names. A function with several overloads takes
the strictest check any of them makes, since the others delegate to it.

Usage: extract_ossl_checks.py <OpenSim checkout> > ossl-checks.txt
"""

import pathlib
import re
import sys

LEVELS = [
    "NoAccess",
    "None",
    "Nuisance",
    "VeryLow",
    "Low",
    "Moderate",
    "High",
    "VeryHigh",
    "Severe",
]

METHOD = re.compile(
    r"\n\s+public\s+(?:virtual\s+)?[\w.<>\[\]]+\s+(os\w+)\s*\([^)]*\)\s*\n\s*\{"
)
CHECK = re.compile(r'CheckThreatLevel\((?:ThreatLevel\.(\w+),\s*"(\w+)")?\)')
COMMENT = re.compile(r"/\*.*?\*/|//[^\n]*", re.DOTALL)
DEPRECATED = re.compile(r'OSSLDeprecated\("(\w+)",\s*"(\w+)"\)')


def rank(check: tuple[str, str]) -> int:
    """How strict a check is: none, then enabled, then by threat level."""
    kind, _ = check
    if kind == "none":
        return -2
    if kind == "enabled":
        return -1
    return LEVELS.index(kind)


def main() -> None:
    """Print the check of every OSSL method of the checkout named."""
    source = (
        pathlib.Path(sys.argv[1])
        / "OpenSim/Region/ScriptEngine/Shared/Api/Implementation/OSSL_Api.cs"
    ).read_text(encoding="utf-8")
    # A check that is commented out is no check (`osDie` has one).
    source = COMMENT.sub("", source)
    methods = list(METHOD.finditer(source))
    checks: dict[str, tuple[str, str]] = {}
    replacements: dict[str, str] = {}
    for index, method in enumerate(methods):
        end = methods[index + 1].start() if index + 1 < len(methods) else len(source)
        body = source[method.end() : end]
        name = method.group(1)
        found = CHECK.search(body)
        if found is None:
            check = ("none", name)
        elif found.group(1) is None:
            check = ("enabled", name)
        else:
            check = (found.group(1), found.group(2))
        if name not in checks or rank(check) > rank(checks[name]):
            checks[name] = check
        deprecated = DEPRECATED.search(body)
        if deprecated is not None:
            replacements[name] = deprecated.group(2)
    print("# Derived by extract_ossl_checks.py from OSSL_Api.cs; see README.md.")
    for name in sorted(checks):
        kind, checked = checks[name]
        fields = [name, kind]
        if checked != name or name in replacements:
            fields.append(checked)
        if name in replacements:
            fields.append(replacements[name])
        print(" ".join(fields))


if __name__ == "__main__":
    main()
