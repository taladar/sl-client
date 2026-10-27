// probe: prim/identity
// drive: none
//
// The object's name, description and other identity, on each grid.
// llSetObjectName and llSetObjectDesc past their lengths, with two-byte
// characters, a pipe, a newline and leading and trailing spaces, read back
// by llGetObjectName, llGetObjectDesc and llGetObjectDetails; llGetKey,
// llGetOwner, llGetCreator and llGetLinkNumber of a single prim, and
// llGetNumberOfPrims; llSetClickAction of each value and one no grid has,
// read through PRIM_CLICK_ACTION and OBJECT_CLICK_ACTION; and what
// llGetObjectDetails answers for this prim, rule by rule (each OBJECT_*
// code, as its number), for NULL_KEY, for a key nothing has, for the owner
// and for an unknown code. Keys print as whether they are this prim's, the
// owner's or NULL_KEY.
string ID = "prim/identity";

// The llGetObjectDetails code being printed, or -1.
integer code_now = -1;

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

// Who a key is, as a role: this prim, the owner, NULL_KEY, empty, or other.
string who(string k)
{
    if (k == (string)llGetKey())
        return "self";
    if (k == (string)llGetOwner())
        return "owner";
    if (k == NULL_KEY)
        return "null";
    if (k == "")
        return "empty";
    return "other";
}

// The codes whose value is a measurement or this run's: the position,
// script time, the character time, script memory, the creation and rez
// times. Only their type is printed.
list VARY = [3, 11, 12, 17, 36, 45];

// A value of llGetObjectDetails, keys as roles, measurements as their type.
string shown(list l, integer i)
{
    integer t = llGetListEntryType(l, i);
    string s = llList2String(l, i);
    if (code_now >= 0 && llListFindList(VARY, [code_now]) != -1)
    {
        return llGetSubString("?ifskvr", t, t) + ":-";
    }
    if (t == TYPE_KEY || (t == TYPE_STRING && llStringLength(s) == 36
        && llGetSubString(s, 8, 8) == "-" && llGetSubString(s, 13, 13) == "-"))
    {
        return "key:" + who(s);
    }
    return llGetSubString("?ifskvr", t, t) + ":" + s;
}

name(string label, string n)
{
    llSetObjectName("Object");
    llSetObjectName(n);
    string got = llGetObjectName();
    string detail = llList2String(llGetObjectDetails(llGetKey(), [OBJECT_NAME]), 0);
    emit("name", " label=" + label + " asked=" + (string)llStringLength(n) + " length="
        + (string)llStringLength(got) + " bytes=" + (string)(llStringLength(
        llStringToBase64(got)) * 3 / 4) + " got=" + q(got) + " detail_same="
        + (string)(detail == got));
}

desc(string label, string d)
{
    llSetObjectDesc("");
    llSetObjectDesc(d);
    string got = llGetObjectDesc();
    string detail = llList2String(llGetObjectDetails(llGetKey(), [OBJECT_DESC]), 0);
    emit("desc", " label=" + label + " asked=" + (string)llStringLength(d) + " length="
        + (string)llStringLength(got) + " got=" + q(got) + " detail_same="
        + (string)(detail == got));
}

default
{
    state_entry()
    {
        begin();
        emit("fresh", " name=" + q(llGetObjectName()) + " desc=" + q(llGetObjectDesc())
            + " key=" + who(llGetKey()) + " owner=" + who(llGetOwner())
            + " creator=" + who(llGetCreator()) + " link=" + (string)llGetLinkNumber()
            + " prims=" + (string)llGetNumberOfPrims()
            + " link_key_0=" + who(llGetLinkKey(0)) + " link_key_1=" + who(llGetLinkKey(1))
            + " link_key_2=" + who(llGetLinkKey(2)) + " link_name_0=" + q(llGetLinkName(0))
            + " link_name_1=" + q(llGetLinkName(1)) + " link_name_2=" + q(llGetLinkName(2)));

        string s100 = "";
        integer i;
        for (i = 0; i < 10; ++i)
        {
            s100 += "0123456789";
        }
        name("short", "Probe");
        name("63", llGetSubString(s100, 0, 62));
        name("64", llGetSubString(s100, 0, 63));
        name("100", s100);
        name("two-byte-40", "éééééééééééééééééééééééééééééééééééééééé");
        name("pipe", "a|b");
        name("newline", "a\nb");
        name("spaces", "  padded  ");
        name("empty", "");
        llSetObjectName("Object");

        desc("short", "a description");
        desc("127", llGetSubString(s100 + s100, 0, 126));
        desc("128", llGetSubString(s100 + s100, 0, 127));
        desc("200", llGetSubString(s100 + s100, 0, 199));
        desc("two-byte-100", llGetSubString(
            "éééééééééééééééééééééééééééééééééééééééééééééééééé"
            + "éééééééééééééééééééééééééééééééééééééééééééééééééé", 0, 99));
        desc("pipe", "a|b");
        desc("newline", "a\nb");
        desc("spaces", "  padded  ");
        llSetObjectDesc("");

        // Click actions.
        list ACTIONS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 99, -1];
        integer n = llGetListLength(ACTIONS);
        for (i = 0; i < n; ++i)
        {
            integer a = llList2Integer(ACTIONS, i);
            llSetClickAction(CLICK_ACTION_NONE);
            llSetClickAction(a);
            emit("click-action", " asked=" + (string)a + " prim="
                + llList2String(llGetPrimitiveParams([43]), 0) + " detail="
                + llList2String(llGetObjectDetails(llGetKey(), [28]), 0));
        }
        llSetClickAction(CLICK_ACTION_NONE);

        // llGetObjectDetails of this prim, code by code.
        for (i = 1; i <= 57; ++i)
        {
            code_now = i;
            list got = llGetObjectDetails(llGetKey(), [i]);
            string out = "";
            integer m = llGetListLength(got);
            integer j;
            for (j = 0; j < m; ++j)
            {
                if (j)
                {
                    out += ";";
                }
                out += shown(got, j);
            }
            emit("detail", " code=" + (string)i + " count=" + (string)m + " got=" + q(out));
        }
        code_now = -1;
        list several = llGetObjectDetails(llGetKey(), [OBJECT_NAME, 9999, OBJECT_DESC, -1]);
        emit("details", " label=unknown-codes count=" + (string)llGetListLength(several)
            + " got=" + q(llList2CSV(several)));
        emit("details", " label=null-key count="
            + (string)llGetListLength(llGetObjectDetails(NULL_KEY, [OBJECT_NAME])));
        emit("details", " label=no-such-key count=" + (string)llGetListLength(
            llGetObjectDetails("11111111-2222-3333-4444-555555555555", [OBJECT_NAME])));
        emit("details", " label=not-a-key count=" + (string)llGetListLength(
            llGetObjectDetails("nothing", [OBJECT_NAME])));
        list owner = llGetObjectDetails(llGetOwner(), [OBJECT_OWNER, OBJECT_ROOT,
            OBJECT_CREATOR, OBJECT_PRIM_COUNT, OBJECT_GROUP]);
        emit("details", " label=owner count=" + (string)llGetListLength(owner) + " got="
            + q(shown(owner, 0) + ";" + shown(owner, 1) + ";" + shown(owner, 2) + ";"
            + shown(owner, 3) + ";" + shown(owner, 4)));
        emit("details", " label=empty-list count="
            + (string)llGetListLength(llGetObjectDetails(llGetKey(), [])));
        emit("end", "");
    }
}
