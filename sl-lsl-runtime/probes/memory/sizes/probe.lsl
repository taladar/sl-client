// probe: memory/sizes
// script: Sizes
// order: 9
// drive: none
// unordered: globals
//
// The exact bytes llGetUsedMemory charges for each kind of value in each
// place a script holds one: an element of a global list (by type, a string
// by length and character class, a fresh string against one literal shared
// by every element), the list itself by length, a global string or key by
// its content, a local, a function argument and a call frame; and, from the
// eight Globals scripts beside it, a global of each type that holds its
// default. Every measurement is made three times and printed as the three
// deltas, so the allocation granularity shows through the few hundred bytes
// llGetUsedMemory wanders between two reads of nothing (memory/budget).
//
// A delta is two reads into locals around the one thing measured, in
// statements of their own (Second Life evaluates an expression right to
// left, and building a line allocates). Every value is fresh — an integer
// counted up, a key from llGenerateKey, a string cut from a longer one —
// unless the record says it is shared, since a list of one literal holds
// the literal once. The two- and three-byte characters are U+00E9 and
// U+20AC, made with llUnescapeURL so the source stays ASCII.
string ID = "memory/sizes";

list held;
string text;
key named;
string ascii;
string two;
string three;
integer rounds;
integer replies;

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

// A string of `n` copies of `c`, by doubling.
string repeat(string c, integer n)
{
    string s = c;
    while (llStringLength(s) < n) s += s;
    return llGetSubString(s, 0, n - 1);
}

// The first `length` characters of the base string of `class`: a new
// string each call, never the base itself (which is longer).
string cut(string class, integer length)
{
    if (length == 0) return "";
    if (class == "two-byte") return llGetSubString(two, 0, length - 1);
    if (class == "three-byte") return llGetSubString(three, 0, length - 1);
    return llGetSubString(ascii, 0, length - 1);
}

// Appends `count` fresh values of `kind` to the global list; a string
// `length` characters long of `class`.
fill(string kind, string class, integer length, integer count)
{
    integer n;
    for (n = 0; n < count; ++n)
    {
        if (kind == "integer") held += [n];
        else if (kind == "float") held += [(float)n];
        else if (kind == "vector") held += [<(float)n, 0.0, 0.0>];
        else if (kind == "rotation") held += [<(float)n, 0.0, 0.0, 1.0>];
        else if (kind == "key") held += [llGenerateKey()];
        else if (kind == "shared-string") held += ["abcdefgh"];
        else held += [cut(class, length)];
    }
}

// Three deltas of the global list going from empty to `count` elements.
list_deltas(string kind, string class, integer length, integer count)
{
    list deltas;
    integer r;
    for (r = 0; r < 3; ++r)
    {
        held = [];
        integer before = llGetUsedMemory();
        fill(kind, class, length, count);
        integer after = llGetUsedMemory();
        deltas += [after - before];
    }
    held = [];
    string fields = " kind=" + kind;
    if (kind == "string") fields += " class=" + class + " length=" + (string)length;
    emit("list", fields + " count=" + (string)count + " deltas="
        + llDumpList2String(deltas, ","));
}

list_counts(string kind, string class, integer length, list counts)
{
    integer i;
    for (i = 0; i < llGetListLength(counts); ++i)
        list_deltas(kind, class, length, llList2Integer(counts, i));
}

// Three deltas of the global string going from empty to `length`
// characters of `class`.
text_deltas(string class, integer length)
{
    list deltas;
    integer r;
    for (r = 0; r < 3; ++r)
    {
        text = "";
        integer before = llGetUsedMemory();
        text = cut(class, length);
        integer after = llGetUsedMemory();
        deltas += [after - before];
    }
    text = "";
    emit("text", " class=" + class + " length=" + (string)length + " deltas="
        + llDumpList2String(deltas, ","));
}

// Three deltas of the global key going from empty to a fresh key, to one
// cast from a fresh string, and to NULL_KEY.
key_deltas(string from)
{
    list deltas;
    integer r;
    for (r = 0; r < 3; ++r)
    {
        named = "";
        integer before = llGetUsedMemory();
        if (from == "generated") named = llGenerateKey();
        else if (from == "cast") named = (key)cut("ascii", 36);
        else named = NULL_KEY;
        integer after = llGetUsedMemory();
        deltas += [after - before];
    }
    named = "";
    emit("global-key", " from=" + from + " deltas=" + llDumpList2String(deltas, ","));
}

// Each local_* function holds four locals of a type and returns what
// llGetUsedMemory read while they were live; local_none holds none.
integer local_none(integer seed)
{
    integer u = llGetUsedMemory();
    return u + 0 * seed;
}

integer local_integer(integer seed)
{
    integer a = seed;
    integer b = seed + 1;
    integer c = seed + 2;
    integer d = seed + 3;
    integer u = llGetUsedMemory();
    return u + 0 * (a + b + c + d);
}

integer local_float(integer seed)
{
    float a = (float)seed;
    float b = (float)(seed + 1);
    float c = (float)(seed + 2);
    float d = (float)(seed + 3);
    integer u = llGetUsedMemory();
    return u + 0 * (integer)(a + b + c + d);
}

integer local_vector(integer seed)
{
    vector a = <(float)seed, 0.0, 0.0>;
    vector b = <(float)seed, 1.0, 0.0>;
    vector c = <(float)seed, 2.0, 0.0>;
    vector d = <(float)seed, 3.0, 0.0>;
    integer u = llGetUsedMemory();
    return u + 0 * (integer)(a.x + b.x + c.x + d.x);
}

integer local_rotation(integer seed)
{
    rotation a = <(float)seed, 0.0, 0.0, 1.0>;
    rotation b = <(float)seed, 1.0, 0.0, 1.0>;
    rotation c = <(float)seed, 2.0, 0.0, 1.0>;
    rotation d = <(float)seed, 3.0, 0.0, 1.0>;
    integer u = llGetUsedMemory();
    return u + 0 * (integer)(a.x + b.x + c.x + d.x);
}

integer local_string(integer seed)
{
    string a = cut("ascii", 16);
    string b = cut("ascii", 16);
    string c = cut("ascii", 16);
    string d = cut("ascii", 16);
    integer u = llGetUsedMemory();
    return u + 0 * (seed + llStringLength(a + b + c + d));
}

integer local_key(integer seed)
{
    key a = llGenerateKey();
    key b = llGenerateKey();
    key c = llGenerateKey();
    key d = llGenerateKey();
    integer u = llGetUsedMemory();
    return u + 0 * (seed + llStringLength((string)a + (string)b + (string)c + (string)d));
}

integer local_list(integer seed)
{
    list a = [seed];
    list b = [seed + 1];
    list c = [seed + 2];
    list d = [seed + 3];
    integer u = llGetUsedMemory();
    return u + 0 * llGetListLength(a + b + c + d);
}

integer local_read(string kind, integer seed)
{
    if (kind == "integer") return local_integer(seed);
    if (kind == "float") return local_float(seed);
    if (kind == "vector") return local_vector(seed);
    if (kind == "rotation") return local_rotation(seed);
    if (kind == "string") return local_string(seed);
    if (kind == "key") return local_key(seed);
    if (kind == "list") return local_list(seed);
    return local_none(seed);
}

// Three deltas from before the call to inside a function holding four
// locals of `kind`.
local_deltas(string kind)
{
    list deltas;
    integer r;
    for (r = 0; r < 3; ++r)
    {
        integer before = llGetUsedMemory();
        integer inside = local_read(kind, r);
        deltas += [inside - before];
    }
    emit("local", " kind=" + kind + " count=4 deltas=" + llDumpList2String(deltas, ","));
}

// Each argument_* function takes four arguments of a type and returns what
// llGetUsedMemory read inside it; argument_none takes one integer, as
// local_none does, which the others' deltas are read against.
integer argument_integer(integer a, integer b, integer c, integer d)
{
    integer u = llGetUsedMemory();
    return u + 0 * (a + b + c + d);
}

integer argument_float(float a, float b, float c, float d)
{
    integer u = llGetUsedMemory();
    return u + 0 * (integer)(a + b + c + d);
}

integer argument_vector(vector a, vector b, vector c, vector d)
{
    integer u = llGetUsedMemory();
    return u + 0 * (integer)(a.x + b.x + c.x + d.x);
}

integer argument_rotation(rotation a, rotation b, rotation c, rotation d)
{
    integer u = llGetUsedMemory();
    return u + 0 * (integer)(a.x + b.x + c.x + d.x);
}

integer argument_string(string a, string b, string c, string d)
{
    integer u = llGetUsedMemory();
    return u + 0 * llStringLength(a + b + c + d);
}

integer argument_key(key a, key b, key c, key d)
{
    integer u = llGetUsedMemory();
    return u + 0 * llStringLength((string)a + (string)b + (string)c + (string)d);
}

integer argument_list(list a, list b, list c, list d)
{
    integer u = llGetUsedMemory();
    return u + 0 * llGetListLength(a + b + c + d);
}

// Three deltas from before the call to inside a function taking four
// fresh arguments of `kind`; the arguments are made before the first read.
argument_deltas(string kind)
{
    list deltas;
    integer r;
    for (r = 0; r < 3; ++r)
    {
        integer before;
        integer inside;
        if (kind == "integer")
        {
            integer i1 = r;
            integer i2 = r + 1;
            integer i3 = r + 2;
            integer i4 = r + 3;
            before = llGetUsedMemory();
            inside = argument_integer(i1, i2, i3, i4);
        }
        else if (kind == "float")
        {
            float f1 = (float)r;
            float f2 = (float)(r + 1);
            float f3 = (float)(r + 2);
            float f4 = (float)(r + 3);
            before = llGetUsedMemory();
            inside = argument_float(f1, f2, f3, f4);
        }
        else if (kind == "vector")
        {
            vector v1 = <(float)r, 0.0, 0.0>;
            vector v2 = <(float)r, 1.0, 0.0>;
            vector v3 = <(float)r, 2.0, 0.0>;
            vector v4 = <(float)r, 3.0, 0.0>;
            before = llGetUsedMemory();
            inside = argument_vector(v1, v2, v3, v4);
        }
        else if (kind == "rotation")
        {
            rotation r1 = <(float)r, 0.0, 0.0, 1.0>;
            rotation r2 = <(float)r, 1.0, 0.0, 1.0>;
            rotation r3 = <(float)r, 2.0, 0.0, 1.0>;
            rotation r4 = <(float)r, 3.0, 0.0, 1.0>;
            before = llGetUsedMemory();
            inside = argument_rotation(r1, r2, r3, r4);
        }
        else if (kind == "string")
        {
            string s1 = cut("ascii", 16);
            string s2 = cut("ascii", 16);
            string s3 = cut("ascii", 16);
            string s4 = cut("ascii", 16);
            before = llGetUsedMemory();
            inside = argument_string(s1, s2, s3, s4);
        }
        else if (kind == "key")
        {
            key k1 = llGenerateKey();
            key k2 = llGenerateKey();
            key k3 = llGenerateKey();
            key k4 = llGenerateKey();
            before = llGetUsedMemory();
            inside = argument_key(k1, k2, k3, k4);
        }
        else if (kind == "list")
        {
            list l1 = [r];
            list l2 = [r + 1];
            list l3 = [r + 2];
            list l4 = [r + 3];
            before = llGetUsedMemory();
            inside = argument_list(l1, l2, l3, l4);
        }
        else
        {
            before = llGetUsedMemory();
            inside = local_none(r);
        }
        deltas += [inside - before];
    }
    emit("argument", " kind=" + kind + " count=4 deltas=" + llDumpList2String(deltas, ","));
}

// What llGetUsedMemory reads `depth` calls down, one integer argument each.
integer descend(integer depth)
{
    if (depth == 0) return llGetUsedMemory();
    return descend(depth - 1);
}

frame_deltas(integer depth)
{
    list deltas;
    integer r;
    for (r = 0; r < 3; ++r)
    {
        integer before = llGetUsedMemory();
        integer inside = descend(depth);
        deltas += [inside - before];
    }
    emit("frames", " depth=" + (string)depth + " deltas=" + llDumpList2String(deltas, ","));
}

default
{
    state_entry()
    {
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        integer limit = llGetMemoryLimit();
        emit("limit", " value=" + (string)limit);
        ascii = repeat("abcdefghijklmnopqrstuvwxyz0123456789", 300);
        two = repeat(llUnescapeURL("%C3%A9"), 300);
        three = repeat(llUnescapeURL("%E2%82%AC"), 300);

        list noise;
        integer r;
        for (r = 0; r < 5; ++r)
        {
            integer a = llGetUsedMemory();
            integer b = llGetUsedMemory();
            noise += [b - a];
        }
        emit("noise", " deltas=" + llDumpList2String(noise, ","));

        list counts = [0, 1, 2, 3, 4, 8, 16, 32, 64, 128, 256];
        list_counts("integer", "", 0, counts);
        list_counts("float", "", 0, counts);
        list_counts("vector", "", 0, counts);
        list_counts("rotation", "", 0, counts);
        list_counts("key", "", 0, [1, 2, 4, 16, 64, 128]);
        list_counts("shared-string", "", 0, [1, 2, 4, 16, 64, 256]);
        list_counts("string", "ascii", 1, [1, 2, 4, 16, 64, 256]);
        list_counts("string", "ascii", 4, [1, 16, 64, 256]);
        list_counts("string", "ascii", 16, [1, 16, 64, 256]);
        list_counts("string", "two-byte", 4, [1, 16, 64, 256]);
        list_counts("string", "three-byte", 4, [1, 16, 64, 256]);

        list lengths = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
            32, 64, 128, 256];
        integer i;
        for (i = 0; i < llGetListLength(lengths); ++i)
            text_deltas("ascii", llList2Integer(lengths, i));
        lengths = [1, 2, 3, 4, 16, 64, 256];
        for (i = 0; i < llGetListLength(lengths); ++i)
        {
            text_deltas("two-byte", llList2Integer(lengths, i));
            text_deltas("three-byte", llList2Integer(lengths, i));
        }

        key_deltas("generated");
        key_deltas("cast");
        key_deltas("null");

        list kinds = ["none", "integer", "float", "vector", "rotation", "string", "key", "list"];
        for (i = 0; i < llGetListLength(kinds); ++i)
            local_deltas(llList2String(kinds, i));
        for (i = 0; i < llGetListLength(kinds); ++i)
            argument_deltas(llList2String(kinds, i));

        list depths = [0, 1, 2, 4, 8, 16, 32];
        for (i = 0; i < llGetListLength(depths); ++i)
            frame_deltas(llList2Integer(depths, i));

        // The Globals scripts read themselves three times, a second apart,
        // each when asked.
        llMessageLinked(LINK_THIS, 1, "", NULL_KEY);
        llSetTimerEvent(1.0);
    }

    timer()
    {
        ++rounds;
        if (rounds < 3) llMessageLinked(LINK_THIS, 1, "", NULL_KEY);
        else llSetTimerEvent(0.0);
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num != 2) return;
        ++replies;
        if (replies == 24) emit("end", "");
    }
}
