// probe: money/lapse
// drive: answer, revoke, away, rerez, attach, drop
//
// When a grant lapses. The script asks the owner for TAKE_CONTROLS,
// TRIGGER_ANIMATION, TRACK_CAMERA and DEBIT (1046), which the runner grants,
// and prints what it holds after each of: a state change and back; the
// viewer revoking TRIGGER_ANIMATION of this object; the viewer revoking
// TRIGGER_ANIMATION and OVERRIDE_ANIMATIONS of every object in the region
// (what a viewer sends when its avatar stops all animations); the viewer
// revoking every permission of this object (what a viewer sends when its
// avatar stands up from it); llTakeControls then llReleaseControls; the
// owner away in another region for twenty seconds (the script keeps what it
// holds each two seconds meanwhile and prints it once the owner is back,
// since an owner away may not hear it); llResetScript; the owner taking the
// prim and rezzing it again; the owner attaching it from the ground (and,
// worn, the script asking again: whether a grid puts the question to the
// wearer at all) and dropping it. Before each the script asks again and the
// runner grants again. Any run_time_permissions that no question of the
// script's explains is printed as `unasked`. The prim's description carries
// the step across a reset.
string ID = "money/lapse";
string MARK = "money/lapse step ";

integer MASK;
integer step;
integer awaiting;
integer polls;
list kept;

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

string granter()
{
    key id = llGetPermissionsKey();
    if (id == NULL_KEY)
    {
        return "none";
    }
    if (id == llGetOwner())
    {
        return "owner";
    }
    return "someone";
}

string holding()
{
    return " perms=" + (string)llGetPermissions() + " granter=" + granter()
        + " attached=" + (string)llGetAttached();
}

held(string why)
{
    emit("held", " step=" + (string)step + " why=" + why + holding());
}

// Asks the owner for MASK, the runner granting it; the step's action comes
// with the answer.
ask()
{
    awaiting = TRUE;
    emit("waiting", " drive=answer grant=" + (string)MASK);
    llRequestPermissions(llGetOwner(), MASK);
    // An answer comes within a second; the patience is for a runner still
    // settling the owner back in after its trip away.
    llSetTimerEvent(45.0);
}

// After an action, what is held once three seconds have passed.
settle()
{
    llSetTimerEvent(3.0);
}

act()
{
    if (step == 1)
    {
        // The state change is the timer handler's.
        llSetTimerEvent(0.5);
    }
    else if (step == 2)
    {
        emit("waiting", " drive=revoke perms=" + (string)PERMISSION_TRIGGER_ANIMATION);
        llSetTimerEvent(6.0);
    }
    else if (step == 3)
    {
        emit("waiting", " drive=revoke target=region perms="
            + (string)(PERMISSION_TRIGGER_ANIMATION | PERMISSION_OVERRIDE_ANIMATIONS));
        llSetTimerEvent(6.0);
    }
    else if (step == 4)
    {
        emit("waiting", " drive=revoke perms=4294967295");
        llSetTimerEvent(6.0);
    }
    else if (step == 5)
    {
        llTakeControls(CONTROL_FWD, TRUE, TRUE);
        held("took");
        llReleaseControls();
        settle();
    }
    else if (step == 6)
    {
        polls = 0;
        kept = [];
        emit("waiting", " drive=away seconds=20");
        llSetTimerEvent(2.0);
    }
    else if (step == 7)
    {
        llSetObjectDesc(MARK + (string)step);
        llResetScript();
    }
    else if (step == 8)
    {
        llSetObjectDesc(MARK + (string)step);
        emit("waiting", " drive=rerez");
        llSetTimerEvent(30.0);
    }
    else if (step == 9)
    {
        emit("waiting", " drive=attach point=" + (string)ATTACH_LHAND);
        llSetTimerEvent(30.0);
    }
    else if (step == 10)
    {
        emit("waiting", " drive=drop");
        llSetTimerEvent(30.0);
    }
}

next()
{
    step = step + 1;
    if (step <= 10)
    {
        ask();
    }
    else
    {
        llSetTimerEvent(0.0);
        llSetObjectDesc("");
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        MASK = PERMISSION_TAKE_CONTROLS | PERMISSION_TRIGGER_ANIMATION
            | PERMISSION_TRACK_CAMERA | PERMISSION_DEBIT;
        string desc = llGetObjectDesc();
        if (llSubStringIndex(desc, MARK) == 0)
        {
            step = (integer)llGetSubString(desc, llStringLength(MARK), -1);
            if (step == 7)
            {
                held("after-reset");
                next();
            }
            else
            {
                // A grid that resets the script on a re-rez starts it here.
                held("restarted");
            }
            return;
        }
        if (step == 0)
        {
            begin();
            held("fresh");
            next();
            return;
        }
        held("state-default");
        next();
    }

    run_time_permissions(integer answer)
    {
        if (!awaiting)
        {
            emit("unasked", " step=" + (string)step + " answer=" + (string)answer + holding());
            return;
        }
        awaiting = FALSE;
        llSetTimerEvent(0.0);
        emit("answer", " step=" + (string)step + " answer=" + (string)answer);
        if (step == 9 && llGetAttached())
        {
            held("worn-asked");
            settle();
            return;
        }
        act();
    }

    on_rez(integer param)
    {
        if (step == 8)
        {
            llSetObjectDesc("");
            held("on-rez");
            next();
        }
    }

    attach(key id)
    {
        if (step == 9 && id != NULL_KEY)
        {
            llSetTimerEvent(0.0);
            held("attached");
            // Worn now, asked again: the runner notes whether a question
            // came at all.
            ask();
        }
        else if (step == 10 && id == NULL_KEY)
        {
            llSetTimerEvent(0.0);
            held("dropped");
            settle();
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (awaiting)
        {
            awaiting = FALSE;
            held("no-answer");
            next();
            return;
        }
        if (step == 1)
        {
            state other;
        }
        else if (step == 6)
        {
            polls = polls + 1;
            integer gone = llGetAgentSize(llGetOwner()) == ZERO_VECTOR;
            kept += [polls, gone, llGetPermissions(), granter()];
            if (polls < 40 && (gone || polls < 10))
            {
                llSetTimerEvent(2.0);
                return;
            }
            integer i;
            for (i = 0; i < llGetListLength(kept); i += 4)
            {
                emit("away", " step=6 poll=" + llList2String(kept, i)
                    + " owner_gone=" + llList2String(kept, i + 1)
                    + " perms=" + llList2String(kept, i + 2)
                    + " granter=" + llList2String(kept, i + 3));
            }
            held("back");
            next();
        }
        else
        {
            held("after");
            next();
        }
    }
}

state other
{
    state_entry()
    {
        held("state-other");
        state default;
    }
}
