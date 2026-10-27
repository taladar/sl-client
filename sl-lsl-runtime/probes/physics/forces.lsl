// probe: physics/forces
// drive: none
// vary: vel, vel_z
//
// Forces, torques, velocities and impulses, on each grid. First on a prim
// that is not physical: what llGetForce and llGetTorque read after
// llSetForce and llSetTorque, globally, locally on a prim turned a quarter
// round Z, and at a huge value; llSetForceAndTorque; what llGetVel,
// llGetOmega and llGetAccel read after llSetVelocity, llSetAngularVelocity,
// llApplyImpulse and llApplyRotationalImpulse; and whether llSetBuoyancy,
// llSetHoverHeight, llStopHover, llGroundRepel and llPushObject of itself
// move it within a second. Then a short physical leg, the prim weightless
// (llSetBuoyancy(1.0)): whether it stays where it is with nothing else, the
// first time (after the impulses and the push it was given while not
// physical) and again,
// whether a force set before physics is still there and how fast it then
// goes, llSetVelocity read back at once and after half a second, a huge
// llSetVelocity read back at once and stopped in the same event, and an
// upward llApplyImpulse. The prim is made non-physical and put back after.
// Every velocity is a measurement; whether the prim moved is the reading.
string ID = "physics/forces";

vector home;
rotation home_rot;
integer step;

// How far the prim stands above where it started.
float rise()
{
    vector p = llGetPos();
    return p.z - home.z;
}

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

string v(vector x)
{
    return q((string)x);
}

restore()
{
    llSetStatus(STATUS_PHYSICS, FALSE);
    llSetForceAndTorque(ZERO_VECTOR, ZERO_VECTOR, FALSE);
    llSetBuoyancy(0.0);
    llStopHover();
    llSetRegionPos(home);
    llSetRot(home_rot);
}

string moved_from(vector at)
{
    return " moved=" + (string)(llVecDist(llGetPos(), at) > 0.01)
        + " turned=" + (string)(llAngleBetween(llGetRot(), home_rot) > 0.01);
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();

        emit("fresh", " force=" + v(llGetForce()) + " torque=" + v(llGetTorque())
            + " vel=" + v(llGetVel()) + " omega=" + v(llGetOmega())
            + " accel=" + v(llGetAccel()));

        llSetForce(<1.0, 2.0, 3.0>, FALSE);
        emit("force", " set=global read=" + v(llGetForce()));
        llSetRot(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        llSetForce(<1.0, 0.0, 0.0>, TRUE);
        emit("force", " set=local-turned read=" + v(llGetForce()));
        llSetRot(home_rot);
        llSetForce(<1000000.0, 0.0, 0.0>, FALSE);
        emit("force", " set=huge read=" + v(llGetForce()));
        llSetForce(ZERO_VECTOR, FALSE);
        emit("force", " set=zero read=" + v(llGetForce()));

        llSetTorque(<1.0, 2.0, 3.0>, FALSE);
        emit("torque", " set=global read=" + v(llGetTorque()));
        llSetRot(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        llSetTorque(<1.0, 0.0, 0.0>, TRUE);
        emit("torque", " set=local-turned read=" + v(llGetTorque()));
        llSetRot(home_rot);
        llSetTorque(<1000000.0, 0.0, 0.0>, FALSE);
        emit("torque", " set=huge read=" + v(llGetTorque()));
        llSetForceAndTorque(<4.0, 5.0, 6.0>, <7.0, 8.0, 9.0>, FALSE);
        emit("force-and-torque", " force=" + v(llGetForce())
            + " torque=" + v(llGetTorque()));
        llSetForceAndTorque(ZERO_VECTOR, ZERO_VECTOR, FALSE);

        llSetVelocity(<1.0, 0.0, 0.0>, FALSE);
        emit("velocity", " set=velocity read=" + v(llGetVel()));
        llSetAngularVelocity(<0.0, 0.0, 1.0>, FALSE);
        emit("velocity", " set=angular read=" + v(llGetOmega()));
        llApplyImpulse(<0.0, 0.0, 1.0>, FALSE);
        emit("velocity", " set=impulse read=" + v(llGetVel()));
        llApplyRotationalImpulse(<0.0, 0.0, 1.0>, FALSE);
        emit("velocity", " set=rotational-impulse read=" + v(llGetOmega())
            + " accel=" + v(llGetAccel()));

        llSetBuoyancy(2.0);
        llSetHoverHeight(5.0, FALSE, 0.5);
        llGroundRepel(3.0, FALSE, 0.5);
        llPushObject(llGetKey(), <0.0, 0.0, 5.0>, <0.0, 0.0, 5.0>, FALSE);
        step = 1;
        llSetTimerEvent(1.0);
    }

    timer()
    {
        if (step == 1)
        {
            emit("non-physical-movers", moved_from(home)
                + " vel=" + v(llGetVel()));
            restore();

            // Weightless, nothing else.
            llSetBuoyancy(1.0);
            llSetStatus(STATUS_PHYSICS, TRUE);
            step = 2;
            llSetTimerEvent(0.5);
        }
        else if (step == 2)
        {
            emit("physical", " step=weightless vel=" + v(llGetVel())
                + " rising=" + (string)(rise() > 0.01)
                + " falling=" + (string)(rise() < -0.01));
            restore();
            llSetBuoyancy(1.0);
            llSetStatus(STATUS_PHYSICS, TRUE);
            step = 21;
            llSetTimerEvent(0.5);
        }
        else if (step == 21)
        {
            emit("physical", " step=weightless-again vel=" + v(llGetVel())
                + " rising=" + (string)(rise() > 0.01)
                + " falling=" + (string)(rise() < -0.01));
            restore();

            // A force set before physics, then physics, weightless.
            llSetBuoyancy(1.0);
            llSetForce(<0.0, 0.0, 0.25>, FALSE);
            llSetStatus(STATUS_PHYSICS, TRUE);
            emit("physical", " step=on force=" + v(llGetForce()));
            step = 25;
            llSetTimerEvent(0.5);
        }
        else if (step == 25)
        {
            emit("physical", " step=pushed vel=" + v(llGetVel()));
            llSetForce(ZERO_VECTOR, FALSE);
            restore();
            llSetBuoyancy(1.0);
            llSetStatus(STATUS_PHYSICS, TRUE);
            llSetVelocity(<0.0, 0.0, 0.5>, FALSE);
            vector at_once = llGetVel();
            emit("physical", " step=set-velocity vel_z=" + (string)at_once.z
                + " read_at_once=" + (string)(llFabs(at_once.z - 0.5) < 0.05));
            step = 3;
            llSetTimerEvent(0.5);
        }
        else if (step == 3)
        {
            vector later = llGetVel();
            emit("physical", " step=velocity-later vel_z=" + (string)later.z
                + " rising=" + (string)(rise() > 0.0));
            llSetVelocity(<0.0, 0.0, 200.0>, FALSE);
            vector huge = llGetVel();
            llSetVelocity(ZERO_VECTOR, FALSE);
            emit("physical", " step=huge-velocity vel_z=" + (string)huge.z
                + " read_above_100=" + (string)(huge.z > 100.0));
            llSetRegionPos(home);
            step = 4;
            llSetTimerEvent(0.5);
        }
        else if (step == 4)
        {
            llSetVelocity(ZERO_VECTOR, FALSE);
            float m = llGetMass();
            llApplyImpulse(<0.0, 0.0, m>, FALSE);
            vector at_once = llGetVel();
            emit("physical", " step=impulse mass=" + (string)m
                + " vel_z=" + (string)at_once.z);
            step = 5;
            llSetTimerEvent(0.5);
        }
        else
        {
            llSetTimerEvent(0.0);
            vector later = llGetVel();
            emit("physical", " step=impulse-later vel_z=" + (string)later.z
                + " rising=" + (string)(rise() > 0.0));
            restore();
            emit("after", " physics=" + (string)llGetStatus(STATUS_PHYSICS)
                + " force=" + v(llGetForce()) + " torque=" + v(llGetTorque())
                + moved_from(home));
            emit("end", "");
        }
    }
}
