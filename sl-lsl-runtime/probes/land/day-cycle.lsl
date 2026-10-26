// probe: land/day-cycle
// drive: none
// vary: time_of_day, sun, moon, fraction
//
// What the day-cycle functions answer on each grid: the parcel's and the
// region's day length and offset; the time of day and the sun and moon
// directions and rotations, which move and are printed as measurements; and
// the relations a comparison can hold: whether each direction is a unit
// vector, whether the parcel's and the region's agree, which axis of each
// rotation the direction is (the dot product with the rotation's forward,
// left and up), how the sun and moon lie to each other, and whether the time
// of day is within the day length. It reads the time of day twice, two
// seconds apart, for whether it advances.
string ID = "land/day-cycle";

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

string r3(float f)
{
    integer sign = 1;
    if (f < 0.0)
    {
        sign = -1;
        f = -f;
    }
    integer m = (integer)(f * 1000.0 + 0.5);
    if (m == 0)
    {
        return "0.000";
    }
    string frac = (string)(m % 1000);
    while (llStringLength(frac) < 3)
    {
        frac = "0" + frac;
    }
    string out = (string)(m / 1000) + "." + frac;
    if (sign < 0)
    {
        out = "-" + out;
    }
    return out;
}

body(string which, string scope, vector dir, rotation rot)
{
    emit("body", " which=" + which + " scope=" + scope
        + " " + which + "=" + q((string)dir)
        + " length=" + r3(llVecMag(dir))
        + " rot_length=" + r3(llSqrt(rot.x * rot.x + rot.y * rot.y
            + rot.z * rot.z + rot.s * rot.s))
        + " dot_fwd=" + r3(dir * llRot2Fwd(rot))
        + " dot_left=" + r3(dir * llRot2Left(rot))
        + " dot_up=" + r3(dir * llRot2Up(rot)));
}

default
{
    state_entry()
    {
        begin();
        integer length = llGetDayLength();
        integer region_length = llGetRegionDayLength();
        emit("day", " length=" + (string)length
            + " offset=" + (string)llGetDayOffset()
            + " region_length=" + (string)region_length
            + " region_offset=" + (string)llGetRegionDayOffset());

        vector sun = llGetSunDirection();
        vector moon = llGetMoonDirection();
        body("sun", "parcel", sun, llGetSunRotation());
        body("sun", "region", llGetRegionSunDirection(), llGetRegionSunRotation());
        body("moon", "parcel", moon, llGetMoonRotation());
        body("moon", "region", llGetRegionMoonDirection(), llGetRegionMoonRotation());
        emit("agree", " sun=" + (string)(llVecDist(sun, llGetRegionSunDirection()) < 0.001)
            + " moon=" + (string)(llVecDist(moon, llGetRegionMoonDirection()) < 0.001)
            + " sun_dot_moon=" + r3(sun * moon));

        float first = llGetTimeOfDay();
        emit("time", " time_of_day=" + (string)first
            + " within_day=" + (string)(first >= 0.0 && first <= (float)length)
            + " fraction=" + r3(first / (float)length));
        llSleep(2.0);
        float second = llGetTimeOfDay();
        emit("time-later", " advanced=" + (string)(second > first)
            + " time_of_day=" + (string)second);
        emit("end", "");
    }
}
