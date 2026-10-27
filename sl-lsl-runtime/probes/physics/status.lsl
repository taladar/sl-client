// probe: physics/status
// drive: none
//
// What llGetStatus and llSetStatus make of each STATUS_* flag, on each grid.
// A fresh prim's value of every flag, and llGetStatus of masks that are not
// one flag: none, two flags, a bit no flag has, every bit. Then each flag but
// physics is set and cleared on its own, with the flag's value after each and
// the other flags whose value moved with it, and is put back as it was; a
// value of 2 for TRUE, and two flags in one call. llGetPrimitiveParams'
// PRIM_PHYSICS, PRIM_PHANTOM and PRIM_TEMP_ON_REZ beside the status, and what
// llVolumeDetect does to the phantom flag and to a later
// llSetStatus(STATUS_PHANTOM). Last, physics: on, with the rotation flags
// while physical, together with phantom and under llVolumeDetect, each
// cleared again at once and the prim put back where it started.
string ID = "physics/status";

list NAMES = [
    "PHYSICS", 1, "ROTATE_X", 2, "ROTATE_Y", 4, "ROTATE_Z", 8,
    "PHANTOM", 16, "SANDBOX", 32, "BLOCK_GRAB", 64, "DIE_AT_EDGE", 128,
    "RETURN_AT_EDGE", 256, "CAST_SHADOWS", 512, "BLOCK_GRAB_OBJECT", 1024
];

vector home;
rotation home_rot;

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

// Every flag's value, in NAMES order.
list values()
{
    list got = [];
    integer n = llGetListLength(NAMES);
    integer i;
    for (i = 0; i < n; i += 2)
    {
        got += [llGetStatus(llList2Integer(NAMES, i + 1))];
    }
    return got;
}

// The flags whose value differs between two lists of values, as
// NAME=value, leaving out the flags in the mask `skip`.
string moved(list before, list after, integer skip)
{
    string out = "";
    integer n = llGetListLength(before);
    integer i;
    for (i = 0; i < n; ++i)
    {
        integer bit = llList2Integer(NAMES, i * 2 + 1);
        if (!(bit & skip) && llList2Integer(before, i) != llList2Integer(after, i))
        {
            if (out != "")
            {
                out += ",";
            }
            out += llList2String(NAMES, i * 2) + "=" + (string)llList2Integer(after, i);
        }
    }
    return out;
}

// One flag's value in a list of values.
integer value_of(list got, integer bit)
{
    integer at = llListFindList(NAMES, [bit]);
    return llList2Integer(got, (at - 1) / 2);
}

string prim_params()
{
    list p = llGetPrimitiveParams([PRIM_PHYSICS, PRIM_PHANTOM, PRIM_TEMP_ON_REZ]);
    return " prim_physics=" + llList2String(p, 0)
        + " prim_phantom=" + llList2String(p, 1)
        + " prim_temp=" + llList2String(p, 2);
}

// Sets the flags `mask` to `value`, prints what each flag reads after it and
// which others moved, then puts every flag in the mask back as `before` has it.
set_and_report(string label, integer mask, integer value, list before)
{
    llSetStatus(mask, value);
    list after = values();
    string got = "";
    integer n = llGetListLength(NAMES);
    integer i;
    for (i = 0; i < n; i += 2)
    {
        integer bit = llList2Integer(NAMES, i + 1);
        if (bit & mask)
        {
            got += " " + llToLower(llList2String(NAMES, i)) + "="
                + (string)value_of(after, bit);
        }
    }
    emit("set", " label=" + label + " value=" + (string)value + got
        + " others=" + q(moved(before, after, mask)) + prim_params());
}

put_back(integer mask, list before)
{
    integer n = llGetListLength(NAMES);
    integer i;
    for (i = 0; i < n; i += 2)
    {
        integer bit = llList2Integer(NAMES, i + 1);
        if (bit & mask)
        {
            llSetStatus(bit, value_of(before, bit));
        }
    }
}

restore()
{
    llSetStatus(STATUS_PHYSICS, FALSE);
    llSetRegionPos(home);
    llSetRot(home_rot);
}

default
{
    state_entry()
    {
        begin();
        home = llGetPos();
        home_rot = llGetRot();

        list fresh = values();
        string all = "";
        integer n = llGetListLength(NAMES);
        integer i;
        for (i = 0; i < n; i += 2)
        {
            all += " " + llToLower(llList2String(NAMES, i)) + "="
                + (string)llList2Integer(fresh, i / 2);
        }
        emit("fresh", all + prim_params());

        emit("mask", " mask=0 value=" + (string)llGetStatus(0));
        emit("mask", " mask=ROTATE_XYZ value="
            + (string)llGetStatus(STATUS_ROTATE_X | STATUS_ROTATE_Y | STATUS_ROTATE_Z));
        emit("mask", " mask=PHYSICS|ROTATE_X value="
            + (string)llGetStatus(STATUS_PHYSICS | STATUS_ROTATE_X));
        emit("mask", " mask=PHANTOM|ROTATE_X value="
            + (string)llGetStatus(STATUS_PHANTOM | STATUS_ROTATE_X));
        emit("mask", " mask=2048 value=" + (string)llGetStatus(2048));
        emit("mask", " mask=-1 value=" + (string)llGetStatus(-1));

        // Each flag but physics, set and cleared on its own.
        for (i = 2; i < n; i += 2)
        {
            integer bit = llList2Integer(NAMES, i + 1);
            string name = llToLower(llList2String(NAMES, i));
            set_and_report(name, bit, TRUE, fresh);
            set_and_report(name, bit, FALSE, fresh);
            put_back(bit, fresh);
        }
        set_and_report("phantom-two", STATUS_PHANTOM, 2, fresh);
        put_back(STATUS_PHANTOM, fresh);
        set_and_report("phantom-minus-one", STATUS_PHANTOM, -1, fresh);
        put_back(STATUS_PHANTOM, fresh);
        set_and_report("phantom-and-block-grab", STATUS_PHANTOM | STATUS_BLOCK_GRAB,
            TRUE, fresh);
        put_back(STATUS_PHANTOM | STATUS_BLOCK_GRAB, fresh);
        set_and_report("rotate-xyz", STATUS_ROTATE_X | STATUS_ROTATE_Y | STATUS_ROTATE_Z,
            FALSE, fresh);
        put_back(STATUS_ROTATE_X | STATUS_ROTATE_Y | STATUS_ROTATE_Z, fresh);

        // llVolumeDetect against the phantom flag.
        llVolumeDetect(TRUE);
        emit("volume-detect", " step=on phantom=" + (string)llGetStatus(STATUS_PHANTOM)
            + prim_params());
        llSetStatus(STATUS_PHANTOM, FALSE);
        emit("volume-detect", " step=phantom-cleared phantom="
            + (string)llGetStatus(STATUS_PHANTOM) + prim_params());
        llVolumeDetect(FALSE);
        emit("volume-detect", " step=off phantom=" + (string)llGetStatus(STATUS_PHANTOM)
            + prim_params());
        llSetStatus(STATUS_PHANTOM, TRUE);
        llVolumeDetect(TRUE);
        llVolumeDetect(FALSE);
        emit("volume-detect", " step=off-from-phantom phantom="
            + (string)llGetStatus(STATUS_PHANTOM) + prim_params());
        put_back(STATUS_PHANTOM, fresh);

        // Physics, each leg cleared again at once.
        set_and_report("physics", STATUS_PHYSICS, TRUE, fresh);
        set_and_report("physical-rotate-x", STATUS_ROTATE_X, FALSE, values());
        restore();
        put_back(STATUS_ROTATE_X, fresh);
        set_and_report("physics-off", STATUS_PHYSICS, FALSE, fresh);

        set_and_report("physics-and-phantom", STATUS_PHYSICS | STATUS_PHANTOM, TRUE, fresh);
        restore();
        put_back(STATUS_PHANTOM, fresh);

        llVolumeDetect(TRUE);
        set_and_report("physics-volume-detect", STATUS_PHYSICS, TRUE, values());
        restore();
        llVolumeDetect(FALSE);
        put_back(STATUS_PHANTOM, fresh);

        emit("after", " others=" + q(moved(fresh, values(), 0)) + prim_params());
        emit("end", "");
    }
}
