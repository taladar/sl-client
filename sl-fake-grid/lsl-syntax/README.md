# Vendored OpenSim script syntax

`ScriptSyntax.xml` is OpenSim's `bin/ScriptSyntax.xml`, copied verbatim
from the OpenSim tree the local test grid runs (commit `4dbfdbc022`,
"update ScriptSyntax.xml", 2021-12-04; SHA-256
`5df7f5c72478579619bb227a29411ef2a6498d256384f623f7cd5bcc7b188edd`). Its
first line is the `LSLSyntaxId` OpenSim advertises
(`4b833b57-d52b-5503-85a4-76754ac3b8ff`); the rest is the `LSLSyntax`
document, which a region serves with every line trimmed and joined and
comment lines dropped (`SimulatorFeaturesModule.ReadScriptSyntax`). The fake
grid's OpenSim flavour does the same at run time
(`sl-fake-grid/src/script_syntax.rs`), so a refresh is a copy of the file.

OpenSim is distributed under the BSD licence in `LICENSE-OpenSim.txt`.

The Second Life flavour's document is not here: it is
`sl-lsl-runtime/keywords_lsl_default.xml`, the copy the script engine's
library table is built from (`sl-lsl-runtime/README.md`).
