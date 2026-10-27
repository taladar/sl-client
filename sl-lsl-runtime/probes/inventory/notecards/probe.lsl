// probe: inventory/notecards
// script: Probe
// order: 1
// drive: none
// vary: delay, reply
//
// Reading notecards over the dataserver. Prim A holds four: Lines (ten
// lines: a word, an empty line, one with spaces at both ends, one with a
// tab, one with an accent and an emoji, 300 ASCII bytes, 1100 ASCII bytes,
// 400 and 600 two-byte characters, and a last word, then a newline), Empty
// (no text at all), NoNewline (one line with no newline after it) and Many
// (a thousand short lines). One request at a time: llGetNumberOfNotecardLines
// of each; every line of Lines and two past its end, and line -1; the last
// line of Many and the one after; line 0 and 1 of Empty and NoNewline;
// Lines read by its asset key; a notecard that is not there, a script read
// as a notecard and an empty name. Each answer prints its length in
// characters, its head and the Base64 of its last two characters (where a
// cut through a two-byte character shows), or EOF; a request no answer comes
// to within five seconds prints `timeout`. Then ten lines of Lines asked for
// at once, printed in the order the answers come. `delay` is how long the
// call itself took (its forced sleep), `reply` how long its answer took.
string ID = "inventory/notecards";

list plan;
key pending;
string label;
float asked;
integer burst;
list burst_keys;
integer burst_got;

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

string shown(string data)
{
    if (data == EOF) return " eof=1";
    integer n = llStringLength(data);
    return " eof=0 length=" + (string)n + " head=" + q(llGetSubString(data, 0, 11))
        + " tail64=" + llStringToBase64(llGetSubString(data, -2, -1));
}

next()
{
    if (plan == [])
    {
        llSetTimerEvent(0.0);
        burst = 1;
        integer i;
        for (i = 0; i < 10; ++i)
        {
            burst_keys += llGetNotecardLine("Lines", i);
        }
        llSetTimerEvent(10.0);
        return;
    }
    list step = llParseStringKeepNulls(llList2String(plan, 0), ["|"], []);
    plan = llDeleteSubList(plan, 0, 0);
    string kind = llList2String(step, 0);
    string card = llList2String(step, 1);
    integer line = llList2Integer(step, 2);
    float before = llGetTime();
    if (kind == "count")
    {
        label = "count " + card;
        emit("try", " label=" + q(label));
        pending = llGetNumberOfNotecardLines(card);
    }
    else if (kind == "asset")
    {
        label = "asset " + card + " " + (string)line;
        emit("try", " label=" + q(label));
        pending = llGetNotecardLine(llGetInventoryKey(card), line);
    }
    else
    {
        label = card + " " + (string)line;
        emit("try", " label=" + q(label));
        pending = llGetNotecardLine(card, line);
    }
    asked = llGetTime();
    emit("asked", " label=" + q(label) + " null=" + (string)(pending == NULL_KEY)
        + " delay=" + (string)(asked - before));
    llSetTimerEvent(5.0);
}

default
{
    state_entry()
    {
        begin();
        plan = ["count|Lines", "count|Empty", "count|NoNewline", "count|Many"];
        integer i;
        for (i = -1; i < 12; ++i)
        {
            plan += "line|Lines|" + (string)i;
        }
        plan += ["line|Many|999", "line|Many|1000", "line|Empty|0", "line|Empty|1",
            "line|NoNewline|0", "line|NoNewline|1", "asset|Lines|0",
            "line|Nope|0", "count|Nope", "line|Probe|0", "count|Probe", "line||0"];
        next();
    }

    dataserver(key id, string data)
    {
        if (burst == 2) return;
        if (burst)
        {
            integer at = llListFindList(burst_keys, [id]);
            ++burst_got;
            emit("burst", " request=" + (string)at + " arrival=" + (string)burst_got
                + shown(data));
            if (burst_got == 10)
            {
                burst = 2;
                llSetTimerEvent(0.0);
                emit("end", "");
            }
            return;
        }
        if (id != pending)
        {
            emit("stray", " data=" + q(llGetSubString(data, 0, 11)));
            return;
        }
        emit("answer", " label=" + q(label) + shown(data) + " reply=" + (string)(llGetTime() - asked));
        pending = NULL_KEY;
        next();
    }

    timer()
    {
        if (burst == 2) return;
        if (burst)
        {
            llSetTimerEvent(0.0);
            burst = 2;
            emit("burst-timeout", " got=" + (string)burst_got);
            emit("end", "");
            return;
        }
        emit("timeout", " label=" + q(label));
        pending = NULL_KEY;
        next();
    }
}
