// probe: inventory/rez
// script: Probe
// order: 2
// drive: none
// vary: delay, seconds
// unordered: burst-rez
//
// Rezzing from prim A's inventory. Prim A holds an object, Child, whose
// script reports on a channel and dies (see there), and a notecard, Notes.
// One rez at a time, four seconds apart: llRezObject a metre above with
// start parameter 42; llRezAtRoot with -7; a rotation that is not unit
// length; a velocity of 5 m/s upward on a prim that is not physical; 11 m
// up, past the 10 m a script may rez at; an item that is not there; the
// notecard. For each, every object_rez and every report the child makes is
// printed in the order they come, with whether the child's on_rez ran before
// the rezzer's object_rez by the clock (llGetTimestamp on both sides).
// Last, twenty llRezObject calls in a row, and how many object_rez events
// and reports come within ten seconds. What a grid says about a call is its
// debug-channel chat, which the runner notes beside the `try` record printed
// before it. `delay` is how long the call itself took.
string ID = "inventory/rez";

integer CHANNEL = -7341;
integer step;
integer order;
string label;
key rezzed;
key child;
string rez_stamp;
string child_stamp;
integer burst_rez;
integer burst_reports;
list burst_keys;

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

string cm(vector v)
{
    return "<" + (string)(llRound(v.x * 100.0) / 100.0) + ","
        + (string)(llRound(v.y * 100.0) / 100.0) + ","
        + (string)(llRound(v.z * 100.0) / 100.0) + ">";
}

string r3(rotation r)
{
    return "<" + (string)(llRound(r.x * 1000.0) / 1000.0) + ","
        + (string)(llRound(r.y * 1000.0) / 1000.0) + ","
        + (string)(llRound(r.z * 1000.0) / 1000.0) + ","
        + (string)(llRound(r.s * 1000.0) / 1000.0) + ">";
}

// A timestamp's minutes and seconds, in seconds: fine enough in a float to
// order two events a frame apart (an hour boundary between them would
// misorder them).
float secs(string stamp)
{
    return (float)llGetSubString(stamp, 14, 15) * 60.0 + (float)llGetSubString(stamp, 17, -2);
}

rez(string name, string item, vector offset, vector velocity, rotation turn, integer param, integer root)
{
    label = name;
    order = 0;
    rezzed = NULL_KEY;
    child = NULL_KEY;
    rez_stamp = "";
    child_stamp = "";
    emit("try", " label=" + name);
    float before = llGetTime();
    if (root)
    {
        llRezAtRoot(item, llGetPos() + offset, velocity, turn, param);
    }
    else
    {
        llRezObject(item, llGetPos() + offset, velocity, turn, param);
    }
    emit("called", " case=" + name + " delay=" + (string)(llGetTime() - before));
}

default
{
    state_entry()
    {
        begin();
        llListen(CHANNEL, "", NULL_KEY, "");
        llSetTimerEvent(4.0);
        rez("plain", "Child", <0.0, 0.0, 1.0>, ZERO_VECTOR, ZERO_ROTATION, 42, FALSE);
    }

    timer()
    {
        if (step > 7) return;
        emit("done", " case=" + label + " events=" + (string)order);
        ++step;
        if (step == 1)
        {
            rez("at-root", "Child", <0.0, 0.0, 1.0>, ZERO_VECTOR, ZERO_ROTATION, -7, TRUE);
        }
        else if (step == 2)
        {
            rez("rotation", "Child", <0.0, 0.0, 1.0>, ZERO_VECTOR, <0.0, 0.0, 1.0, 1.0>, 0, FALSE);
        }
        else if (step == 3)
        {
            rez("velocity", "Child", <0.0, 0.0, 1.0>, <0.0, 0.0, 5.0>, ZERO_ROTATION, 1, FALSE);
        }
        else if (step == 4)
        {
            rez("far", "Child", <0.0, 0.0, 11.0>, ZERO_VECTOR, ZERO_ROTATION, 2, FALSE);
        }
        else if (step == 5)
        {
            rez("missing", "Nope", <0.0, 0.0, 1.0>, ZERO_VECTOR, ZERO_ROTATION, 3, FALSE);
        }
        else if (step == 6)
        {
            rez("notecard", "Notes", <0.0, 0.0, 1.0>, ZERO_VECTOR, ZERO_ROTATION, 4, FALSE);
        }
        else if (step == 7)
        {
            label = "burst";
            order = 0;
            emit("try", " label=burst");
            float before = llGetTime();
            integer i;
            for (i = 0; i < 20; ++i)
            {
                llRezObject("Child", llGetPos() + <0.0, 0.0, 1.0>, ZERO_VECTOR, ZERO_ROTATION, 100 + i);
            }
            emit("called", " case=burst calls=20 seconds=" + (string)(llGetTime() - before));
            llSetTimerEvent(10.0);
        }
        else
        {
            llSetTimerEvent(0.0);
            emit("burst", " object_rez=" + (string)burst_rez + " reports=" + (string)burst_reports
                + " distinct=" + (string)llGetListLength(burst_keys));
            emit("end", "");
        }
    }

    object_rez(key id)
    {
        if (label == "burst")
        {
            ++burst_rez;
            if (llListFindList(burst_keys, [id]) == -1) burst_keys += id;
            return;
        }
        ++order;
        rez_stamp = llGetTimestamp();
        rezzed = id;
        string first = "";
        if (child_stamp != "")
        {
            first = " is_child=" + (string)(id == child)
                + " child_first=" + (string)(secs(child_stamp) < secs(rez_stamp));
        }
        emit("object-rez", " case=" + label + " order=" + (string)order + first);
    }

    listen(integer channel, string name, key id, string text)
    {
        list f = llParseStringKeepNulls(text, ["|"], []);
        string kind = llList2String(f, 0);
        if (label == "burst")
        {
            if (kind == "rez")
            {
                ++burst_reports;
                emit("burst-rez", " param=" + llList2String(f, 1));
            }
            return;
        }
        ++order;
        if (kind == "entry")
        {
            emit("entry", " case=" + label + " order=" + (string)order + " start=" + llList2String(f, 1));
            return;
        }
        child_stamp = llList2String(f, 5);
        child = (key)llList2String(f, 3);
        string first = "";
        if (rez_stamp != "")
        {
            // The child's clock against the rezzer's, whichever event came
            // first here.
            first = " is_object_rez=" + (string)(child == rezzed)
                + " child_first=" + (string)(secs(child_stamp) < secs(rez_stamp));
        }
        emit("on-rez", " case=" + label + " order=" + (string)order
            + " param=" + llList2String(f, 1) + " start=" + llList2String(f, 2)
            + " rezzer_is_me=" + (string)((key)llList2String(f, 4) == llGetKey())
            + " offset=" + cm((vector)llList2String(f, 6) - llGetPos())
            + " rot=" + r3((rotation)llList2String(f, 7))
            + " vel=" + cm((vector)llList2String(f, 8))
            + " die_at_edge=" + llList2String(f, 9) + first);
    }
}
