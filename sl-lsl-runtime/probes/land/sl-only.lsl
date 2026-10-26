// probe: land/sl-only
// drive: none
// vary: value, time_of_day
//
// The parcel, region and environment functions Second Life has and OpenSim
// does not (llGetSimStats, llGetEnvironment, llGetRegionTimeOfDay,
// llGetClosestNavPoint), in a probe of their own so that the others compile
// on OpenSim; there this one does not, and its compile errors are its
// measurement. It prints llGetSimStats for every code from -1 to 27 (a
// measurement, with whether the answer is zero, negative or whole — what a
// comparison can hold); llGetEnvironment at the prim for the day info, for
// each sky and water rule, for an unknown rule and outside the region (the
// length and the values); llGetRegionTimeOfDay beside llGetTimeOfDay; and
// what llGetClosestNavPoint answers for the prim's position.
string ID = "land/sl-only";

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

environment(string at, vector pos, integer rule)
{
    list got = llGetEnvironment(pos, [rule]);
    string text = llList2CSV(got);
    string shown = q(text);
    if (llStringLength(text) > 300)
    {
        shown = q(llGetSubString(text, 0, 299)) + " text_length="
            + (string)llStringLength(text);
    }
    emit("environment", " at=" + at + " rule=" + (string)rule
        + " length=" + (string)llGetListLength(got) + " got=" + shown);
}

default
{
    state_entry()
    {
        begin();
        integer code;
        for (code = -1; code <= 27; ++code)
        {
            float v = llGetSimStats(code);
            emit("sim-stat", " code=" + (string)code + " value=" + (string)v
                + " zero=" + (string)(v == 0.0)
                + " negative=" + (string)(v < 0.0)
                + " whole=" + (string)(v == (float)((integer)v)));
        }
        vector here = llGetPos();
        environment("here", here, ENVIRONMENT_DAYINFO);
        list rules = [
            SKY_AMBIENT, SKY_TEXTURE_DEFAULTS, SKY_CLOUDS, SKY_DOME, SKY_GAMMA,
            SKY_GLOW, SKY_LIGHT, SKY_MOON, SKY_PLANET, SKY_REFRACTION,
            SKY_STAR_BRIGHTNESS, SKY_SUN, SKY_TRACKS, SKY_CLOUD_TEXTURE,
            SKY_MOON_TEXTURE, SKY_SUN_TEXTURE, SKY_BLUE, SKY_HAZE,
            SKY_REFLECTION_PROBE_AMBIANCE, WATER_BLUR_MULTIPLIER, WATER_FOG,
            WATER_FRESNEL, WATER_TEXTURE_DEFAULTS, WATER_NORMAL_SCALE,
            WATER_REFRACTION, WATER_WAVE_DIRECTION, WATER_NORMAL_TEXTURE, 999
        ];
        integer n = llGetListLength(rules);
        integer i;
        for (i = 0; i < n; ++i)
        {
            environment("here", here, llList2Integer(rules, i));
        }
        environment("high", <here.x, here.y, 3000.0>, SKY_AMBIENT);
        environment("west", <-10.0, here.y, here.z>, ENVIRONMENT_DAYINFO);
        float region_time = llGetRegionTimeOfDay();
        float parcel_time = llGetTimeOfDay();
        emit("time", " time_of_day=" + (string)region_time
            + " same_as_parcel=" + (string)(llFabs(region_time - parcel_time) < 1.0));
        list nav = llGetClosestNavPoint(here, []);
        string dist = "none";
        if (nav != [])
        {
            dist = (string)(llVecDist(llList2Vector(nav, 0), here) < 50.0);
        }
        emit("nav-point", " length=" + (string)llGetListLength(nav)
            + " within_50m=" + dist + " got=" + q(llList2CSV(nav)));
        emit("end", "");
    }
}
