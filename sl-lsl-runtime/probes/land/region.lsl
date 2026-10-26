// probe: land/region
// drive: none
// vary: fps, dilation, agents, others, hostname
// unordered: sim-data
//
// What a script learns of the region it is in, on each grid: its name and
// corner, llGetRegionFlags bit by bit, the frame rate, time dilation and
// agent count (measurements), the simulator's host name (a measurement, with
// whether llGetEnv names the same host), and llGetAgentList for each scope —
// the codes as numbers (1 parcel, 2 parcel owner's land, 4 region), with 0
// and 8 beside them — as whether the owner is in it and how many others are,
// and a lone element that is not an agent in the region as it is.
// llRequestSimulatorData asks for the position (5), status (6), rating (7)
// and release (128) of this region, of this region in lower case, of a
// region no grid has and of the empty name; each answer, or its absence
// within fifteen seconds, is printed. llEdgeOfWorld is asked from the
// prim's position towards each compass direction, with an upward direction,
// a direction of zero length, from a position by each edge of the region and
// from one outside it.
string ID = "land/region";

list pending;
list labels;

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

region_flags()
{
    integer f = llGetRegionFlags();
    list names = [
        "ALLOW_DAMAGE", 0x1, "FIXED_SUN", 0x10, "BLOCK_TERRAFORM", 0x40,
        "SANDBOX", 0x100, "DISABLE_COLLISIONS", 0x1000,
        "DISABLE_PHYSICS", 0x4000, "BLOCK_FLY", 0x80000,
        "ALLOW_DIRECT_TELEPORT", 0x100000, "RESTRICT_PUSHOBJECT", 0x400000,
        "BLOCK_FLYOVER", 0x8000000
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
    emit("region-flags", " value=" + (string)f + " set=" + q(set)
        + " other_bits=" + (string)(f & ~known));
}

agent_list(integer scope)
{
    list got = llGetAgentList(scope, []);
    integer owner = llListFindList(got, [llGetOwner()]) >= 0;
    integer others = llGetListLength(got) - owner;
    string lone = "";
    if (llGetListLength(got) == 1 && !owner)
    {
        lone = " only=" + q(llList2String(got, 0));
        if (llList2Key(got, 0) != NULL_KEY && llGetAgentSize(llList2Key(got, 0)) != ZERO_VECTOR)
        {
            lone = " only=agent";
        }
    }
    emit("agent-list", " scope=" + (string)scope + " owner_in=" + (string)owner
        + " others=" + (string)others + lone);
}

ask(string label, string region, integer data)
{
    key id = llRequestSimulatorData(region, data);
    emit("sim-data-asked", " of=" + label + " data=" + (string)data
        + " got_key=" + (string)(id != NULL_KEY));
    if (id != NULL_KEY)
    {
        pending += [id];
        labels += [label + " data=" + (string)data];
    }
}

edge(string from, vector pos, vector dir)
{
    emit("edge", " from=" + from + " dir=" + q((string)dir)
        + " answer=" + (string)llEdgeOfWorld(pos, dir));
}

default
{
    state_entry()
    {
        begin();
        vector corner = llGetRegionCorner();
        emit("region", " named=" + (string)(llGetRegionName() != "")
            + " corner=" + q((string)corner)
            + " corner_on_grid=" + (string)((integer)corner.x % 256 == 0
                && (integer)corner.y % 256 == 0));
        region_flags();
        emit("load", " fps=" + (string)llGetRegionFPS()
            + " dilation=" + (string)llGetRegionTimeDilation()
            + " agents=" + (string)llGetRegionAgentCount());
        string host = llGetSimulatorHostname();
        emit("hostname", " hostname=" + q(host)
            + " same_as_env=" + (string)(host == llGetEnv("simulator_hostname")));
        agent_list(1);
        agent_list(2);
        agent_list(4);
        agent_list(0);
        agent_list(8);

        vector here = llGetPos();
        edge("here", here, <1.0, 0.0, 0.0>);
        edge("here", here, <-1.0, 0.0, 0.0>);
        edge("here", here, <0.0, 1.0, 0.0>);
        edge("here", here, <0.0, -1.0, 0.0>);
        edge("here", here, <1.0, 1.0, 0.0>);
        edge("here", here, <0.0, 0.0, 1.0>);
        edge("here", here, <1.0, 0.0, 1.0>);
        edge("here", here, ZERO_VECTOR);
        edge("west-edge", <1.0, here.y, here.z>, <-1.0, 0.0, 0.0>);
        edge("east-edge", <255.0, here.y, here.z>, <1.0, 0.0, 0.0>);
        edge("south-edge", <here.x, 1.0, here.z>, <0.0, -1.0, 0.0>);
        edge("north-edge", <here.x, 255.0, here.z>, <0.0, 1.0, 0.0>);
        edge("outside", <-10.0, here.y, here.z>, <-1.0, 0.0, 0.0>);

        string name = llGetRegionName();
        list codes = [5, 6, 7, 128];
        integer i;
        for (i = 0; i < 4; ++i)
        {
            ask("own", name, llList2Integer(codes, i));
        }
        ask("lower", llToLower(name), 5);
        ask("lower", llToLower(name), 6);
        ask("nowhere", "No Such Region Probe", 5);
        ask("nowhere", "No Such Region Probe", 6);
        ask("empty", "", 6);
        ask("own", name, 99);
        llSetTimerEvent(15.0);
    }

    dataserver(key id, string data)
    {
        integer at = llListFindList(pending, [id]);
        if (at < 0)
        {
            emit("dataserver", " request=unknown data=" + q(data));
            return;
        }
        emit("sim-data", " of=" + llList2String(labels, at) + " data_text=" + q(data));
        pending = llDeleteSubList(pending, at, at);
        labels = llDeleteSubList(labels, at, at);
        if (pending == [])
        {
            llSetTimerEvent(0.0);
            emit("end", "");
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        integer n = llGetListLength(labels);
        integer i;
        for (i = 0; i < n; ++i)
        {
            emit("sim-data-unanswered", " of=" + llList2String(labels, i));
        }
        pending = [];
        labels = [];
        emit("end", "");
    }
}
