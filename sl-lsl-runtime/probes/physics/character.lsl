// probe: physics/character
// drive: none
// vary: dx, dist
//
// The pathfinding character functions' errors, on each grid; OpenSim has
// none of them, so its compile error is its reading. Before any character:
// the prim's status flags and OBJECT_PATHFINDING_TYPE, llGetClosestNavPoint
// of where it stands, llGetStaticPath two metres east and llNavigateTo, which
// needs a character. Then llCreateCharacter and the same flags after it,
// llUpdateCharacter with a speed past the documented clamp, llNavigateTo
// the navmesh's point nearest two metres east with every path_update it
// raises for eight seconds and how far it went, llExecCharacterCmd with CHARACTER_CMD_STOP and with a
// command no grid has, and llDeleteCharacter twice, with the flags after.
// Each call that may be refused prints a `try` first, so the runner's note
// of its debug-channel chat comes after it. The prim is made non-physical
// and put back after.
string ID = "physics/character";

vector home;
rotation home_rot;
integer updates;

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

string flags()
{
    list p = llGetObjectDetails(llGetKey(), [OBJECT_PATHFINDING_TYPE]);
    list shape = llGetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE]);
    return " physics=" + (string)llGetStatus(STATUS_PHYSICS)
        + " phantom=" + (string)llGetStatus(STATUS_PHANTOM)
        + " rotate_x=" + (string)llGetStatus(STATUS_ROTATE_X)
        + " rotate_y=" + (string)llGetStatus(STATUS_ROTATE_Y)
        + " rotate_z=" + (string)llGetStatus(STATUS_ROTATE_Z)
        + " pathfinding_type=" + llList2String(p, 0)
        + " shape_type=" + llList2String(shape, 0);
}

try(string label)
{
    emit("try", " label=" + label);
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();
        emit("fresh", flags());

        list nav = llGetClosestNavPoint(home, []);
        string nav_dist = "none";
        if (llGetListLength(nav) > 0)
        {
            nav_dist = (string)llVecDist(llList2Vector(nav, 0), home);
        }
        emit("closest-nav-point", " length=" + (string)llGetListLength(nav)
            + " dist=" + nav_dist);

        list path = llGetStaticPath(home, home + <2.0, 0.0, 0.0>, 0.5, []);
        integer n = llGetListLength(path);
        emit("static-path", " length=" + (string)n
            + " status=" + llList2String(path, n - 1)
            + " last_type=" + (string)llGetListEntryType(path, n - 1));

        try("navigate-without-character");
        llNavigateTo(home + <2.0, 0.0, 0.0>, []);

        try("create");
        llCreateCharacter([CHARACTER_DESIRED_SPEED, 1.0, CHARACTER_TYPE, CHARACTER_TYPE_A]);
        emit("created", flags());

        try("update-speed-100");
        llUpdateCharacter([CHARACTER_DESIRED_SPEED, 100.0]);
        llUpdateCharacter([CHARACTER_DESIRED_SPEED, 1.0]);

        // The goal is the navmesh's point nearest two metres east, where
        // there is one.
        vector goal = home + <2.0, 0.0, 0.0>;
        list near = llGetClosestNavPoint(goal, []);
        if (llGetListLength(near) > 0)
        {
            goal = llList2Vector(near, 0);
        }
        emit("goal", " on_navmesh=" + (string)(llGetListLength(near) > 0)
            + " dist=" + (string)llVecDist(goal, home));
        try("navigate");
        llNavigateTo(goal, []);
        llSetTimerEvent(8.0);
    }

    path_update(integer type, list reserved)
    {
        ++updates;
        emit("path-update", " n=" + (string)updates + " type=" + (string)type
            + " reserved=" + q(llList2CSV(reserved)));
    }

    timer()
    {
        llSetTimerEvent(0.0);
        vector p = llGetPos();
        emit("navigated", " dx=" + (string)(p.x - home.x)
            + " east=" + (string)(p.x - home.x > 0.5)
            + " updates=" + (string)updates);

        try("stop");
        llExecCharacterCmd(CHARACTER_CMD_STOP, []);
        try("unknown-command");
        llExecCharacterCmd(999, []);
        try("delete");
        llDeleteCharacter();
        emit("deleted", flags());
        try("delete-again");
        llDeleteCharacter();

        llSetStatus(STATUS_PHYSICS, FALSE);
        llSetRegionPos(home);
        llSetRot(home_rot);
        emit("after", flags());
        emit("end", "");
    }
}
