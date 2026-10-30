// probe: vm/region-budget
// script: Probe
// order: 2
// drive: none
// deadline: 300
// stats: active-scripts, scripts-run, script-time, time-dilation
// vary: rezzed, reported, slowest, fastest, mean, total, real_slowest, real_mean, dilation
//
// How many busy scripts a region runs at full speed before each slows down.
// Prim A holds an object, Worker, whose script runs an empty loop for five
// seconds when told to and reports its rate (see there). For 1, 16, 64, 128
// and 256 workers in turn, the probe rezzes them a metre and a half
// above prim A in batches of 32 (so no more object_rez events queue than the
// 64 a queue holds), waits two seconds, says "go" to all of them at once,
// and collects their reports: per stage, how many were rezzed and reported,
// and the slowest, fastest and mean per-script rate in iterations a second
// of llGetTime, and their total; then the slowest and mean rate per second
// of real time, and the mean ratio of the two clocks (1 unless the region's
// time dilated). The stage where the mean falls below the one-worker rate is
// the region's budget in scripts' shares. The runner notes the region's
// active-script count, scripts-run percentage, script time and time dilation
// meanwhile.
string ID = "vm/region-budget";

integer GO = -73105;
integer REPORT = -73106;
float SECONDS = 5.0;
integer BATCH = 32;
list STAGES = [1, 16, 64, 128, 256];

integer stage;
integer count;
integer asked;
integer rezzed;
integer reported;
float slowest;
float fastest;
float total;
float real_slowest;
float real_total;
float dilation;
// "rez", "settle" or "run": what the next timer event does.
string phase;

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

string rate(float per_second)
{
    return (string)llRound(per_second);
}

start_stage()
{
    if (stage == llGetListLength(STAGES))
    {
        emit("end", "");
        return;
    }
    count = llList2Integer(STAGES, stage);
    asked = 0;
    rezzed = 0;
    reported = 0;
    slowest = 0.0;
    fastest = 0.0;
    total = 0.0;
    real_slowest = 0.0;
    real_total = 0.0;
    dilation = 0.0;
    phase = "rez";
    llSetTimerEvent(0.1);
}

// Rez the next batch; the object_rez events it raises are handled before
// the timer comes back.
rez_batch()
{
    integer i;
    for (i = 0; i < BATCH && asked < count; ++i)
    {
        ++asked;
        llRezObject("Worker", llGetPos() + <0.0, 0.0, 1.5>, ZERO_VECTOR, ZERO_ROTATION, asked);
    }
    if (asked < count)
    {
        llSetTimerEvent(0.1);
    }
    else
    {
        phase = "settle";
        llSetTimerEvent(2.0);
    }
}

end_stage()
{
    llSetTimerEvent(0.0);
    llRegionSay(GO, "die");
    string mean = "none";
    string real_mean = "none";
    string ratio = "none";
    if (reported > 0)
    {
        mean = rate(total / reported);
        real_mean = rate(real_total / reported);
        ratio = (string)(llRound(dilation / reported * 1000.0) / 1000.0);
    }
    emit("stage", " scripts=" + (string)count + " rezzed=" + (string)rezzed
        + " reported=" + (string)reported + " slowest=" + rate(slowest)
        + " fastest=" + rate(fastest) + " mean=" + mean
        + " total=" + rate(total) + " real_slowest=" + rate(real_slowest)
        + " real_mean=" + real_mean + " dilation=" + ratio);
    ++stage;
    start_stage();
}

default
{
    state_entry()
    {
        begin();
        llListen(REPORT, "", NULL_KEY, "");
        stage = 0;
        start_stage();
    }

    object_rez(key id)
    {
        ++rezzed;
    }

    timer()
    {
        if (phase == "rez")
        {
            llSetTimerEvent(0.0);
            rez_batch();
        }
        else if (phase == "settle")
        {
            phase = "run";
            llRegionSay(GO, "go");
            // The run, every worker's staggered report, and ten seconds over.
            llSetTimerEvent(SECONDS + 0.05 * count + 10.0);
        }
        else
        {
            end_stage();
        }
    }

    listen(integer channel, string name, key id, string msg)
    {
        if (phase != "run")
        {
            return;
        }
        list fields = llParseString2List(msg, [" "], []);
        float iterations = llList2Float(fields, 1);
        float seconds = llList2Float(fields, 2);
        float elapsed = llList2Float(fields, 3);
        float per_second = iterations / seconds;
        float per_real_second = iterations / elapsed;
        if (reported == 0 || per_real_second < real_slowest)
        {
            real_slowest = per_real_second;
        }
        real_total += per_real_second;
        dilation += seconds / elapsed;
        if (reported == 0 || per_second < slowest)
        {
            slowest = per_second;
        }
        if (per_second > fastest)
        {
            fastest = per_second;
        }
        total += per_second;
        ++reported;
        if (reported == count)
        {
            end_stage();
        }
    }
}
