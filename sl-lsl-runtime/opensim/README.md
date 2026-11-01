# Vendored OpenSim script syntax and OSSL configuration

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

The script engine's build (`../build.rs`) reads the same file for OpenSim's
part of the library table: every function and constant Second Life's
document has not got. The document lists each overload of a function under
the same key (`osApproxEquals` six times); the build numbers the later ones so
each is an entry of its own.

`ossl-unserved.xml` holds the functions OpenSim's engines compile and the
document leaves out, in its format with only a `functions` map: `osListenRegex`,
`osReplaceRegionEnvironment`, four of the `osSetDynamicTexture*` family,
`osTriggerSoundLimited` (signatures from `IOSSL_Api.cs`) and `modSendCommand`
(`IMOD_Api.cs`). The variadic `modInvoke*` functions are not there: no LSL
signature states them.

`ossl-checks.txt` is the permission check each OSSL function makes before it
runs, derived from `OSSL_Api.cs` by `extract_ossl_checks.py` (run it on an
OpenSim checkout and redirect its output there): the threat level a
`CheckThreatLevel(level, name)` names, `enabled` for a bare
`CheckThreatLevel()`, `none` for no check, and a deprecated function's
replacement.

`osslDefaultEnable.ini` is OpenSim's `bin/config-include/osslDefaultEnable.ini`,
copied verbatim from the same tree (commit `36f6d16a61`; SHA-256
`505681602920fc1b25b8f866ba5b2bb1f26092f76c8f942f2e5779c88f819438`): the
`[OSSL]` configuration the local grid runs, and the one the engine's
OpenSim flavours gate the OSSL functions by unless a region sets another
(`vm::ossl::Settings::stock`).

OpenSim is distributed under the BSD licence in `LICENSE-OpenSim.txt`.

The Second Life flavour's document is not here: it is
`../keywords_lsl_default.xml` (`../README.md`).
