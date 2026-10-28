// probe: throttle/cast-ray
// drive: none
// vary: rays, per_second, count, first_error_at
// unordered: status
//
// Whether a grid throttles llCastRay. `detection/cast-ray` cast 500 rays in
// a row on each grid and none was refused; this one keeps casting for ten
// seconds, twice: first a cheap ray, a metre straight up from two metres
// above prim A, where nothing is; then, after a two-second pause, an
// expensive one, across the whole region from corner to corner at prim A's
// height, asking for 256 hits and phantoms too. For each it prints how many
// rays it cast and how fast, every status the rays ended with (the hit
// count, or an RCERR_* code) and how many of each, and how many seconds in
// the first error came, if one did.
string ID = "throttle/cast-ray";

integer phase;
list statuses;
list counts;

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

note(integer status)
{
    integer at = llListFindList(statuses, [status]);
    if (at < 0)
    {
        statuses += [status];
        counts += [1];
    }
    else
    {
        counts = llListReplaceList(counts, [llList2Integer(counts, at) + 1], at, at);
    }
}

cast(string kind, vector from, vector to, list options)
{
    statuses = [];
    counts = [];
    integer rays = 0;
    float first_error = -1.0;
    float start = llGetTime();
    float now = start;
    while (now - start < 10.0)
    {
        list hits = llCastRay(from, to, options);
        integer status = llList2Integer(hits, -1);
        now = llGetTime();
        if (llGetListLength(hits) == 0)
        {
            status = 999;
        }
        if (status < 0 && first_error < 0.0)
        {
            first_error = now - start;
        }
        note(status);
        ++rays;
    }
    float seconds = now - start;
    emit("cast", " kind=" + kind + " rays=" + (string)rays
        + " per_second=" + (string)llRound(rays / seconds));
    integer i;
    integer n = llGetListLength(statuses);
    for (i = 0; i < n; ++i)
    {
        emit("status", " kind=" + kind + " status=" + (string)llList2Integer(statuses, i)
            + " count=" + (string)llList2Integer(counts, i));
    }
    string at = "none";
    if (first_error >= 0.0)
    {
        at = (string)(llRound(first_error * 10.0) / 10.0);
    }
    emit("errors", " kind=" + kind + " first_error_at=" + at);
}

default
{
    state_entry()
    {
        begin();
        llSetTimerEvent(1.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        vector here = llGetPos();
        if (phase == 0)
        {
            cast("cheap", here + <0.0, 0.0, 2.0>, here + <0.0, 0.0, 3.0>, []);
            phase = 1;
            llSetTimerEvent(2.0);
            return;
        }
        cast("expensive", <0.5, 0.5, here.z>, <255.5, 255.5, here.z>,
            [RC_MAX_HITS, 256, RC_DETECT_PHANTOM, TRUE, RC_DATA_FLAGS, RC_GET_NORMAL]);
        emit("end", "");
    }
}
