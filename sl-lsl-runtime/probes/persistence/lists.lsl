// probe: persistence/lists
// drive: rerez, restart
// deadline: 300
//
// Whether two variables that hold one list still hold one list after a
// restart of the region, and after the object is taken and rezzed again: on
// OpenSim a list is an object both variables hold (`vm/list-identity`), and
// what a grid saves of a script may keep that or copy each variable apart.
// A change in place shows which: llSetPayPrice pads a short button list in
// the very object it is given on OpenSim, and changes nothing on Second Life.
//
// The script sets D to [2], E to D and F to [3], marks the object's
// description, asks for the region to be restarted in 15 seconds, holds F in
// a local and sleeps a minute, inside which the region goes down; once the
// sleep ends it pads the local and prints F (`woke`). On
// CHANGED_REGION_START (or once its patience for one is out, on a grid that
// does not restart the region — the runner notes why) it pads D and prints D
// and E (`after`). Then it sets A to [1], B to A and C to [1], marks the
// description again and asks to be taken and rezzed again; on `on_rez` it
// pads A and prints A, B and C (`rerez`). The restart comes first because an
// object rezzed from inventory just before a restart did not come back with
// the region on OpenSim (2026-11-01).
//
// A `state_entry` that finds a mark is a reset the grid did, printed as
// `restarted` with what the globals hold. The owner is logged out while the
// region is down, and an owner-say to an owner who is not there is lost, so
// each line waits until the owner is in the region again; a one-second timer
// sends the waiting lines and counts the patience.
string ID = "persistence/lists";
string REREZ_MARK = "persistence/lists rerez";
integer PATIENCE = 30;
integer COMEBACK = 90;
float SLEEP = 60.0;
integer SECONDS = 15;

list A;
list B;
list C;
list D;
list E;
list F;

string awaiting;
integer waited;
list waiting_lines;

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

integer owner_here()
{
    return llGetAgentSize(llGetOwner()) != ZERO_VECTOR;
}

flush()
{
    if (!owner_here())
    {
        return;
    }
    integer count = llGetListLength(waiting_lines);
    integer i;
    for (i = 0; i < count; ++i)
    {
        llOwnerSay(llList2String(waiting_lines, i));
    }
    waiting_lines = [];
}

emit(string rec, string fields)
{
    waiting_lines += ["PROBE " + ID + " rec=" + rec + fields];
    flush();
}

show(string rec, string name, list l)
{
    emit(rec, " name=" + name + " value=" + q(llList2CSV(l)));
}

string globals()
{
    return " A=" + q(llList2CSV(A)) + " B=" + q(llList2CSV(B))
        + " D=" + q(llList2CSV(D)) + " E=" + q(llList2CSV(E));
}

pad(list p)
{
    llSetPayPrice(PAY_DEFAULT, p);
}

finish()
{
    awaiting = "";
    llSetTimerEvent(0.0);
    emit("end", "");
}

rerez_leg()
{
    A = [1];
    B = A;
    C = [1];
    llSetObjectDesc(REREZ_MARK);
    awaiting = "on_rez";
    waited = 0;
    emit("waiting", " drive=rerez");
}

after()
{
    pad(D);
    show("after", "D", D);
    show("after", "E", E);
    rerez_leg();
}

default
{
    state_entry()
    {
        llSetTimerEvent(1.0);
        string mark = llGetObjectDesc();
        if (mark == ID)
        {
            emit("restarted", " step=restart" + globals());
            awaiting = "region_start";
            return;
        }
        if (mark == REREZ_MARK)
        {
            emit("restarted", " step=rerez" + globals());
            awaiting = "on_rez";
            waited = 0;
            return;
        }
        emit("start", " region=" + q(llGetRegionName())
            + " channel=" + q(llGetEnv("sim_channel"))
            + " version=" + q(llGetEnv("sim_version")));
        llSetObjectDesc(ID);
        D = [2];
        E = D;
        F = [3];
        awaiting = "region_start";
        waited = 0;
        emit("waiting", " drive=restart seconds=" + (string)SECONDS);
        list held = F;
        llSleep(SLEEP);
        pad(held);
        show("woke", "F", F);
    }

    on_rez(integer param)
    {
        if (llGetObjectDesc() != REREZ_MARK)
        {
            return;
        }
        pad(A);
        show("rerez", "A", A);
        show("rerez", "B", B);
        show("rerez", "C", C);
        finish();
    }

    changed(integer change)
    {
        if ((change & CHANGED_REGION_START) && awaiting == "region_start")
        {
            after();
        }
    }

    timer()
    {
        flush();
        if (awaiting == "" || !owner_here())
        {
            return;
        }
        ++waited;
        if (awaiting == "region_start" && waited >= COMEBACK)
        {
            emit("timeout", " awaited=" + awaiting);
            after();
        }
        else if (awaiting == "on_rez" && waited >= PATIENCE)
        {
            emit("timeout", " awaited=" + awaiting);
            finish();
        }
    }
}
