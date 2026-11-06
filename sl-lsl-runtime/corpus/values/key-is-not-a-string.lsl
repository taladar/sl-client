// corpus: a key compares as text but tests true only when it is a valid key
// Oracle: the value model: (key)"" == NULL_KEY is false; NULL_KEY and a key
// that is not a UUID at all are false in a condition.
default
{
    state_entry()
    {
        print((string)((key)"" == NULL_KEY));
        key empty = "";
        key null = NULL_KEY;
        key garbage = "not a uuid";
        key real = "0a0a0a0a-0000-4000-8000-000000000001";
        if (empty) print("empty is true"); else print("empty is false");
        if (null) print("NULL_KEY is true"); else print("NULL_KEY is false");
        if (garbage) print("garbage is true"); else print("garbage is false");
        if (real) print("a uuid is true"); else print("a uuid is false");
    }
}
