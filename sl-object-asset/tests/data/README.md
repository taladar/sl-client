# Reference object assets

Two complete inventory object assets, carried verbatim in the reference
viewer's own test sources and extracted here because nothing else in the
public record shows the whole format. They are what `tests/reference.rs`
pins the decoder against.

| File | Source | Shape |
| --- | --- | --- |
| `reference-attachment-prim.txt` | Firestorm `indra/llcommon/tests/commonmisc_test.cpp` (the blob inside its trailing `#if 0`) | one solitary prim, worn as an attachment: four `namevalue` lines, `orig_asset_id` / `orig_item_id`, no `linked` line |
| `reference-linkset.txt` | Firestorm `indra/llcommon/tests/lluri_test.cpp` (the `'asset_data':b(12100)` literal of its long round-trip test, minus that LLSD prefix) | a four-prim linkset: three `linked child` prims with `childpos` / `childrot`, then the `linked linked` root with a `description`, `namevalue`s and a `from_task_id` |

Both are byte-for-byte as the reference carries them, with one deliberate
exception: the linkset root's `name` field, a 2005 attachment whose name
reads as a resident's, is replaced with `Fixture_Linkset_Root`. Nothing in
the decoder or its tests depends on the value — it is one `|`-terminated
string among 12 kB of structure — and the repository does not carry
resident names. Every other byte, including the creator and owner keys the
reference itself publishes, is untouched.

The C++ sources hold them as escaped one-line string literals; the files
here are the unescaped bytes, which is what a grid would serve.
