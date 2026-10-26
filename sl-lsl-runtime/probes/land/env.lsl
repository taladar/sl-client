// probe: land/env
// drive: none
// vary: reading, frames_per_second
//
// What llGetEnv answers on each grid. It asks for every key either grid is
// known to answer and a few no grid documents, and prints each answer as it
// is, with its shape (empty, integer, float or text). The keys whose answer
// moves on its own — the frame number, the idle flag, the CPU ratio, the
// up-time, the agent counts and the host the region runs on — are printed as
// a `reading`, a measurement, beside a shape that is compared. It also asks
// for a key in capitals and for the empty key, and reads the frame number
// twice a second apart for how fast the simulator counts frames.
string ID = "land/env";

list STEADY = [
    "sim_channel", "sim_version", "estate_id", "estate_name",
    "region_product_name", "region_product_sku", "region_max_prims",
    "region_object_bonus", "region_rating", "dynamic_pathfinding",
    "agent_limit", "whisper_range", "chat_range", "shout_range",
    "region_start_time", "grid", "allow_damage_adjust", "restrict_pushobject",
    "damage_limit", "region_cpu_ratio", "region_idle_timeout", "estate_owner",
    "region_name", "region_corner", "agent_avatar_limit", "SIM_CHANNEL",
    "Sim_Version", "", "no_such_key"
];

list MOVING = [
    "frame_number", "region_idle", "region_up_time", "agent_reserved",
    "agent_unused", "simulator_hostname"
];

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

// Whether every character of `s` is in `allowed`.
integer only(string s, string allowed)
{
    integer n = llStringLength(s);
    integer i;
    for (i = 0; i < n; ++i)
    {
        if (llSubStringIndex(allowed, llGetSubString(s, i, i)) < 0)
        {
            return FALSE;
        }
    }
    return TRUE;
}

// The shape of an answer: empty, integer, float or text.
string shape(string s)
{
    if (s == "")
    {
        return "empty";
    }
    string digits = s;
    if (llGetSubString(s, 0, 0) == "-")
    {
        digits = llGetSubString(s, 1, -1);
    }
    if (digits != "" && only(digits, "0123456789"))
    {
        return "integer";
    }
    if (digits != "" && only(digits, "0123456789.")
        && llGetListLength(llParseStringKeepNulls(digits, ["."], [])) == 2)
    {
        return "float";
    }
    return "text";
}

default
{
    state_entry()
    {
        begin();
        integer n = llGetListLength(STEADY);
        integer i;
        for (i = 0; i < n; ++i)
        {
            string name = llList2String(STEADY, i);
            string value = llGetEnv(name);
            emit("env", " name=" + q(name) + " shape=" + shape(value)
                + " value=" + q(value));
        }
        n = llGetListLength(MOVING);
        for (i = 0; i < n; ++i)
        {
            string name = llList2String(MOVING, i);
            string value = llGetEnv(name);
            emit("env-moving", " name=" + q(name) + " shape=" + shape(value)
                + " reading=" + q(value));
        }
        emit("hostname", " same_as_function="
            + (string)(llGetEnv("simulator_hostname") == llGetSimulatorHostname()));
        integer first = (integer)llGetEnv("frame_number");
        float started = llGetTime();
        llSleep(1.0);
        integer second = (integer)llGetEnv("frame_number");
        float took = llGetTime() - started;
        emit("frames", " counts_up=" + (string)(second > first)
            + " frames_per_second=" + (string)((float)(second - first) / took));
        emit("end", "");
    }
}
