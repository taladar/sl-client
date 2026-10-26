// probe: land/ground
// drive: none
// vary: wind, cloud
//
// What the ground and weather functions answer on each grid. The heights
// are the land's, so they differ between grids for the land's sake; what a
// comparison should read is the relations printed beside them. For each
// offset — none, a few metres each way, one that leaves the region on each
// side, one far below and one far above — it prints llGround, llWater and
// the height of the ground at the region edge the offset is clamped to (so a
// clamp shows as `same_as_clamped=1`), and how the height 1, 30 and 400 m
// west of the region compares with the height at its west edge. Then llGroundNormal, llGroundSlope and
// llGroundContour at the prim: their lengths and the dot products between
// them, and whether the normal's slope agrees with the heights a metre
// apart. llWind and llCloud are printed as measurements, with whether the
// wind is level (z of zero) and the cloud density between 0 and 2.
string ID = "land/ground";

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

// A float to three decimals, rounded, so that the last digit of a single
// precision dot product does not read as a difference.
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

float clamp(float v)
{
    if (v < 0.0)
    {
        return 0.0;
    }
    if (v > 255.999)
    {
        return 255.999;
    }
    return v;
}

ground(string label, vector offset)
{
    vector here = llGetPos();
    vector at = here + offset;
    // XEngine's parser takes `<…> - here` for a syntax error.
    vector clamped = <clamp(at.x), clamp(at.y), at.z>;
    clamped = clamped - here;
    float g = llGround(offset);
    float c = llGround(clamped);
    emit("ground", " offset=" + label + " same_as_clamped="
        + (string)(llFabs(g - c) < 0.001)
        + " above_prim=" + r3(g - here.z)
        + " water=" + r3(llWater(offset))
        + " water_same_here=" + (string)(llWater(offset) == llWater(ZERO_VECTOR)));
}

default
{
    state_entry()
    {
        begin();
        ground("none", ZERO_VECTOR);
        ground("east-3", <3.0, 0.0, 0.0>);
        ground("north-3", <0.0, 3.0, 0.0>);
        ground("west-3", <-3.0, 0.0, 0.0>);
        ground("up-10", <0.0, 0.0, 10.0>);
        ground("down-100", <0.0, 0.0, -100.0>);
        ground("west-out", <-400.0, 0.0, 0.0>);
        ground("east-out", <400.0, 0.0, 0.0>);
        ground("south-out", <0.0, -400.0, 0.0>);
        ground("north-out", <0.0, 400.0, 0.0>);
        ground("fraction", <0.5, 0.25, 0.0>);
        vector here = llGetPos();
        float edge = llGround(<-here.x, 0.0, 0.0>);
        float near = llGround(<-here.x - 1.0, 0.0, 0.0>);
        float mid = llGround(<-here.x - 30.0, 0.0, 0.0>);
        float far = llGround(<-here.x - 400.0, 0.0, 0.0>);
        emit("ground-west", " near_minus_edge=" + r3(near - edge)
            + " mid_minus_edge=" + r3(mid - edge)
            + " far_minus_edge=" + r3(far - edge)
            + " far_minus_mid=" + r3(far - mid));

        vector n = llGroundNormal(ZERO_VECTOR);
        vector s = llGroundSlope(ZERO_VECTOR);
        vector c = llGroundContour(ZERO_VECTOR);
        emit("surface", " normal_length=" + r3(llVecMag(n))
            + " slope_length=" + r3(llVecMag(s))
            + " contour_length=" + r3(llVecMag(c))
            + " normal_up=" + (string)(n.z > 0.0)
            + " normal_dot_slope=" + r3(n * s)
            + " normal_dot_contour=" + r3(n * c)
            + " slope_dot_contour=" + r3(s * c)
            + " contour_z=" + r3(c.z));
        float east = llGround(<1.0, 0.0, 0.0>) - llGround(<-1.0, 0.0, 0.0>);
        float north = llGround(<0.0, 1.0, 0.0>) - llGround(<0.0, -1.0, 0.0>);
        vector across = <-east / 2.0, -north / 2.0, 1.0>;
        across = llVecNorm(across);
        emit("surface-agrees", " normal_dot_heights=" + r3(n * across)
            + " slope_points_down=" + (string)(s * <east, north, 0.0> <= 0.0));
        emit("surface-flat", " normal=" + q((string)n) + " slope=" + q((string)s)
            + " contour=" + q((string)c));

        vector w = llWind(ZERO_VECTOR);
        float cloud = llCloud(ZERO_VECTOR);
        emit("weather", " wind=" + q((string)w) + " wind_level=" + (string)(w.z == 0.0)
            + " cloud=" + (string)cloud
            + " cloud_in_range=" + (string)(cloud >= 0.0 && cloud <= 2.0)
            + " wind_far_same=" + (string)(llWind(<100.0, 0.0, 0.0>) == w));
        emit("end", "");
    }
}
