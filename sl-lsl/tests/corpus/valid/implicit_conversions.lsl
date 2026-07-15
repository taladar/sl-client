// LSL's implicit conversions the pass must NOT flag: integer where a float is
// wanted, and string<->key both ways. A false error on any of these is exactly
// the kind of over-eager typing the differential oracle guards against.
default
{
    state_entry()
    {
        // integer literal fills a float parameter.
        llSetTimerEvent(5);

        // key -> string (llKey2Name wants a key; llGetOwner yields one).
        string owner = llKey2Name(llGetOwner());

        // string -> key: a string literal is accepted where a key is wanted.
        key target = "00000000-0000-0000-0000-000000000000";
        llSay(0, owner + (string)target);
    }
}
