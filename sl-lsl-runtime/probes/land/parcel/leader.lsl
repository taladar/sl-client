// probe: land/parcel
// script: Leader
// order: 7
// drive: none
// vary: count, used
//
// What a script learns of the parcel it stands on, on each grid. The codes
// are written as numbers, not as PARCEL_DETAILS_* names, because the two
// grids give 7, 8 and 10 different names (Second Life: PRIM_CAPACITY,
// PRIM_USED, LANDING_LOOKAT; OpenSim: ANY_AVATAR_SOUNDS, GROUP_SOUNDS,
// CLAIMDATE) and OpenSim adds 64 (DWELL), so a name one grid lacks would stop
// the probe compiling there.
//
// It prints llGetParcelFlags bit by bit; llGetParcelDetails for each code
// from -1 to 15, 64 and 99 on its own (the length of the answer, its type and
// its value, a key as a role: nil, owner, group — the parcel's group — or
// set), then for all of 0 to 13 in one call (how many come back and in which
// order) and with the list empty — above and below the region last, and
// every position beside it in a neighbour script of its own (saved first,
// since OpenSim throws on some of them, which stops the script); the prim
// limits and counts (the counts are measurements: other residents build on
// a sandbox); llGetParcelPrimOwners; who llGetLandOwnerAt names, what
// llOverMyLand says of the owner and of NULL_KEY, and llScriptDanger;
// llParcelMediaQuery for each query code and for all at once; and the music
// URL, which it then tries to set to a URL of its own, reads back and sets
// back to what it was.
string ID = "land/parcel";

key group;

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

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

string role(key id)
{
    if (id == NULL_KEY)
    {
        return "nil";
    }
    if ((string)id == "")
    {
        return "empty";
    }
    if (id == llGetOwner())
    {
        return "owner";
    }
    if (id == group && group != NULL_KEY)
    {
        return "group";
    }
    return "set";
}

string type_name(integer t)
{
    if (t == TYPE_INTEGER) return "integer";
    if (t == TYPE_FLOAT) return "float";
    if (t == TYPE_STRING) return "string";
    if (t == TYPE_KEY) return "key";
    if (t == TYPE_VECTOR) return "vector";
    if (t == TYPE_ROTATION) return "rotation";
    return "invalid";
}

integer is_key(string s)
{
    if ((key)s)
    {
        return TRUE;
    }
    return FALSE;
}

// One element of a list: its type and its value, a key (or a string that is
// a key) as a role, a long string cut to its first 60 characters.
string element(list l, integer i)
{
    integer t = llGetListEntryType(l, i);
    string s = llList2String(l, i);
    string value;
    if (t == TYPE_KEY || (t == TYPE_STRING && llStringLength(s) == 36
        && is_key(s)))
    {
        value = role((key)s);
    }
    else if (llStringLength(s) > 60)
    {
        value = q(llGetSubString(s, 0, 59)) + " length=" + (string)llStringLength(s);
    }
    else
    {
        value = q(s);
    }
    return type_name(t) + ":" + value;
}

string elements(list l)
{
    integer n = llGetListLength(l);
    string out = "";
    integer i;
    for (i = 0; i < n; ++i)
    {
        if (i > 0)
        {
            out += ",";
        }
        out += element(l, i);
    }
    return out;
}

detail(string at, vector pos, list codes)
{
    list got = llGetParcelDetails(pos, codes);
    emit("details", " at=" + at + " codes=" + q(llList2CSV(codes))
        + " length=" + (string)llGetListLength(got)
        + " got=" + q(elements(got)));
}

flags()
{
    integer f = llGetParcelFlags(llGetPos());
    list names = [
        "ALLOW_FLY", 0x1, "ALLOW_SCRIPTS", 0x2, "ALLOW_LANDMARK", 0x8,
        "ALLOW_TERRAFORM", 0x10, "ALLOW_DAMAGE", 0x20,
        "ALLOW_CREATE_OBJECTS", 0x40, "USE_ACCESS_GROUP", 0x100,
        "USE_ACCESS_LIST", 0x200, "USE_BAN_LIST", 0x400,
        "USE_LAND_PASS_LIST", 0x800, "LOCAL_SOUND_ONLY", 0x8000,
        "RESTRICT_PUSHOBJECT", 0x200000, "ALLOW_GROUP_SCRIPTS", 0x2000000,
        "ALLOW_CREATE_GROUP_OBJECTS", 0x4000000,
        "ALLOW_ALL_OBJECT_ENTRY", 0x8000000,
        "ALLOW_GROUP_OBJECT_ENTRY", 0x10000000
    ];
    string set = "";
    integer known = 0;
    integer n = llGetListLength(names);
    integer i;
    for (i = 0; i < n; i += 2)
    {
        integer bit = llList2Integer(names, i + 1);
        known = known | bit;
        if (f & bit)
        {
            if (set != "")
            {
                set += ",";
            }
            set += llList2String(names, i);
        }
    }
    emit("flags", " value=" + (string)f + " set=" + q(set)
        + " other_bits=" + (string)(f & ~known));
}

media()
{
    list codes = [5, 4, 10, 11, 12, 13, 9, 3, 6, 99];
    integer n = llGetListLength(codes);
    integer i;
    for (i = 0; i < n; ++i)
    {
        integer code = llList2Integer(codes, i);
        list got = llParcelMediaQuery([code]);
        emit("media-query", " code=" + (string)code
            + " length=" + (string)llGetListLength(got)
            + " got=" + q(elements(got)));
    }
    list all = llParcelMediaQuery([5, 4, 10, 11, 12]);
    emit("media-query", " code=all length=" + (string)llGetListLength(all)
        + " got=" + q(elements(all)));
}

music()
{
    string was = llGetParcelMusicURL();
    emit("music", " step=read url=" + q(was));
    llSetParcelMusicURL("http://example.com/sl-client-probe.mp3");
    string now = llGetParcelMusicURL();
    emit("music", " step=set url=" + q(now) + " changed=" + (string)(now != was));
    if (now != was)
    {
        llSetParcelMusicURL(was);
        emit("music", " step=restore url=" + q(llGetParcelMusicURL())
            + " restored=" + (string)(llGetParcelMusicURL() == was));
    }
}

default
{
    state_entry()
    {
        begin();
        vector here = llGetPos();
        group = llList2Key(llGetParcelDetails(here, [3]), 0);
        flags();
        integer code;
        for (code = -1; code <= 15; ++code)
        {
            detail("here", here, [code]);
        }
        detail("here", here, [64]);
        detail("here", here, [99]);
        detail("here", here, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
        detail("here", here, [5, 99, 2, 2]);
        detail("here", here, []);
        key id = llList2Key(llGetParcelDetails(here, [5]), 0);
        key under = llList2Key(llGetParcelDetails(<here.x, here.y, 0.0>, [5]), 0);
        emit("same-parcel", " ground=" + (string)(id == under));

        emit("max-prims", " parcel=" + (string)llGetParcelMaxPrims(here, FALSE)
            + " sim_wide=" + (string)llGetParcelMaxPrims(here, TRUE));
        integer category;
        for (category = -1; category <= 6; ++category)
        {
            emit("prim-count", " category=" + (string)category
                + " count=" + q((string)llGetParcelPrimCount(here, category, FALSE)
                    + "/" + (string)llGetParcelPrimCount(here, category, TRUE)));
        }
        list owners = llGetParcelPrimOwners(here);
        integer at = llListFindList(owners, [llGetOwner()]);
        string owner_count = "absent";
        if (at >= 0)
        {
            owner_count = llList2String(owners, at + 1);
        }
        emit("prim-owners", " empty=" + (string)(owners == [])
            + " owner_counted=" + owner_count
            + " used=" + (string)(llGetListLength(owners) / 2));

        emit("land-owner", " at=here owner=" + role(llGetLandOwnerAt(here))
            + " parcel_owner=" + role(llList2Key(llGetParcelDetails(here, [2]), 0)));
        emit("over-my-land", " of=owner answer=" + (string)llOverMyLand(llGetOwner())
            + " of_nil=" + (string)llOverMyLand(NULL_KEY));
        emit("script-danger", " here=" + (string)llScriptDanger(here)
            + " west_edge=" + (string)llScriptDanger(<0.0, here.y, here.z>));
        media();
        music();
        detail("high", <here.x, here.y, 5000.0>, [4, 5]);
        detail("low", <here.x, here.y, -100.0>, [4, 5]);
        emit("end", "");
    }
}
