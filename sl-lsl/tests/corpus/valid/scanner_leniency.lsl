// The grid's scanner is more lenient than an editor's: a character that starts
// no token is skipped, a `"` with no closing quote is one such character, and
// a string may carry the legacy `L` prefix and span a line break.
default
{ $
    state_entry()
    {
        llOwnerSay(L"Hello
world");
    }"
}
