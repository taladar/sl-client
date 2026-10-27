// probe: physics/vehicles
// drive: none
// vary: dx, dz, speed, spin
//
// The vehicle functions' errors and clamps, on each grid. No function reads
// a vehicle parameter back, so a setter's answer is its debug-channel chat
// (which the runner notes beside each `try`) and what the prim does. On a
// prim that is not physical: a parameter set before any vehicle type, each
// type and the status flags after it, an unknown type, an unknown parameter
// code for each setter, values outside the documented ranges, and the
// flags with unknown bits. Then physical: a sled whose linear motor pushes
// it east at 1 m/s (how far it went in a second and a half), how fast it
// still goes half a second after VEHICLE_TYPE_NONE, a linear motor of
// 100 m/s straight up for half a second (the documented clamp is 30 m/s on
// Second Life), and an angular motor of 100 rad/s about Z (the documented
// clamp is 4π). Distances and speeds are measurements; the readings are
// which way the prim went and whether a speed is above a clamp. The prim is
// made non-physical, VEHICLE_TYPE_NONE and put back after.
string ID = "physics/vehicles";

vector home;
rotation home_rot;
integer step;

list STATUS = ["PHYSICS", 1, "ROTATE_X", 2, "ROTATE_Y", 4, "ROTATE_Z", 8, "PHANTOM", 16];

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

string status()
{
    string out = "";
    integer n = llGetListLength(STATUS);
    integer i;
    for (i = 0; i < n; i += 2)
    {
        out += " " + llToLower(llList2String(STATUS, i)) + "="
            + (string)llGetStatus(llList2Integer(STATUS, i + 1));
    }
    return out;
}

try(string label)
{
    emit("try", " label=" + label);
}

restore()
{
    llSetVehicleType(VEHICLE_TYPE_NONE);
    llSetStatus(STATUS_PHYSICS, FALSE);
    llSetRegionPos(home);
    llSetRot(home_rot);
}

next(integer s, float seconds)
{
    step = s;
    llSetTimerEvent(seconds);
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();
        emit("fresh", status());

        try("param-before-type");
        llSetVehicleFloatParam(VEHICLE_LINEAR_MOTOR_TIMESCALE, 1.0);
        llSetVehicleVectorParam(VEHICLE_LINEAR_MOTOR_DIRECTION, <1.0, 0.0, 0.0>);
        llSetVehicleRotationParam(VEHICLE_REFERENCE_FRAME, ZERO_ROTATION);
        llSetVehicleFlags(VEHICLE_FLAG_NO_DEFLECTION_UP);
        llRemoveVehicleFlags(VEHICLE_FLAG_NO_DEFLECTION_UP);
        emit("after", " label=param-before-type" + status());

        list types = [VEHICLE_TYPE_SLED, VEHICLE_TYPE_CAR, VEHICLE_TYPE_BOAT,
            VEHICLE_TYPE_AIRPLANE, VEHICLE_TYPE_BALLOON, VEHICLE_TYPE_NONE, 99, -1];
        integer i;
        for (i = 0; i < llGetListLength(types); ++i)
        {
            integer type = llList2Integer(types, i);
            try("type-" + (string)type);
            llSetVehicleType(type);
            emit("after", " label=type-" + (string)type + status());
        }

        llSetVehicleType(VEHICLE_TYPE_CAR);
        try("unknown-float-param");
        llSetVehicleFloatParam(999, 1.0);
        try("unknown-vector-param");
        llSetVehicleVectorParam(999, <1.0, 0.0, 0.0>);
        try("unknown-rotation-param");
        llSetVehicleRotationParam(999, ZERO_ROTATION);
        try("float-param-as-vector");
        llSetVehicleVectorParam(VEHICLE_HOVER_HEIGHT, <1.0, 0.0, 0.0>);
        try("negative-timescale");
        llSetVehicleFloatParam(VEHICLE_LINEAR_MOTOR_TIMESCALE, -1.0);
        try("huge-timescale");
        llSetVehicleFloatParam(VEHICLE_LINEAR_MOTOR_DECAY_TIMESCALE, 1000.0);
        try("efficiency-above-one");
        llSetVehicleFloatParam(VEHICLE_ANGULAR_DEFLECTION_EFFICIENCY, 5.0);
        try("efficiency-below-zero");
        llSetVehicleFloatParam(VEHICLE_BANKING_EFFICIENCY, -5.0);
        try("unknown-flags");
        llSetVehicleFlags(0x40000000);
        llRemoveVehicleFlags(0x40000000);
        emit("after", " label=params" + status());
        llSetVehicleType(VEHICLE_TYPE_NONE);

        // Physical: a sled pushed east at 1 m/s.
        try("sled-motor");
        llSetVehicleType(VEHICLE_TYPE_SLED);
        llSetVehicleVectorParam(VEHICLE_LINEAR_MOTOR_DIRECTION, <1.0, 0.0, 0.0>);
        llSetVehicleFloatParam(VEHICLE_LINEAR_MOTOR_TIMESCALE, 0.2);
        llSetVehicleFloatParam(VEHICLE_LINEAR_MOTOR_DECAY_TIMESCALE, 10.0);
        llSetStatus(STATUS_PHYSICS, TRUE);
        emit("after", " label=sled-motor" + status());
        next(1, 1.5);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        vector p = llGetPos();
        if (step == 1)
        {
            emit("sled-motor", " dx=" + (string)(p.x - home.x)
                + " east=" + (string)(p.x - home.x > 0.3));
            llSetVehicleType(VEHICLE_TYPE_NONE);
            next(2, 0.5);
        }
        else if (step == 2)
        {
            emit("type-none", " speed=" + (string)llVecMag(llGetVel())
                + " physics=" + (string)llGetStatus(STATUS_PHYSICS));
            restore();
            try("linear-clamp");
            llSetVehicleType(VEHICLE_TYPE_BALLOON);
            llSetVehicleFloatParam(VEHICLE_BUOYANCY, 1.0);
            llSetVehicleFloatParam(VEHICLE_HOVER_EFFICIENCY, 0.0);
            llSetVehicleFloatParam(VEHICLE_HOVER_TIMESCALE, 1000.0);
            llSetVehicleVectorParam(VEHICLE_LINEAR_MOTOR_DIRECTION, <0.0, 0.0, 100.0>);
            llSetVehicleFloatParam(VEHICLE_LINEAR_MOTOR_TIMESCALE, 0.1);
            llSetStatus(STATUS_PHYSICS, TRUE);
            next(3, 0.5);
        }
        else if (step == 3)
        {
            vector vel = llGetVel();
            llSetVehicleType(VEHICLE_TYPE_NONE);
            llSetStatus(STATUS_PHYSICS, FALSE);
            emit("linear-clamp", " speed=" + (string)llVecMag(vel)
                + " dz=" + (string)(p.z - home.z)
                + " up=" + (string)(vel.z > 1.0)
                + " above_30=" + (string)(llVecMag(vel) > 31.0));
            restore();
            try("angular-clamp");
            llSetVehicleType(VEHICLE_TYPE_BALLOON);
            llSetVehicleFloatParam(VEHICLE_BUOYANCY, 1.0);
            llSetVehicleVectorParam(VEHICLE_ANGULAR_MOTOR_DIRECTION, <0.0, 0.0, 100.0>);
            llSetVehicleFloatParam(VEHICLE_ANGULAR_MOTOR_TIMESCALE, 0.1);
            llSetStatus(STATUS_PHYSICS, TRUE);
            next(4, 1.0);
        }
        else
        {
            vector omega = llGetOmega();
            emit("angular-clamp", " spin=" + (string)omega.z
                + " turning=" + (string)(llFabs(omega.z) > 1.0)
                + " above_4pi=" + (string)(llFabs(omega.z) > 4.0 * PI + 0.5));
            restore();
            emit("after", " label=restored" + status());
            emit("end", "");
        }
    }
}
