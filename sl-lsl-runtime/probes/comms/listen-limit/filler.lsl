// probe: comms/listen-limit
// script: Filler
// order: 2
// drive: none
//
// The Leader's other half. First two listens with the same filter, and
// whether they are one listen (the same handle) or two; both are removed.
// Then it opens listens on channels 1001, 1002, … up to seventy, printing
// each from the sixtieth on before and after the call, so the last line
// shows where a fault stopped it. If it is still running it removes the
// last, opens one more in its place, removes a handle it never had, and
// says how many it held. It answers the Leader's ping with how many it
// holds.
string ID = "comms/listen-limit";

integer held;

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    state_entry()
    {
        integer first = llListen(1000, "", NULL_KEY, "");
        integer second = llListen(1000, "", NULL_KEY, "");
        emit("duplicate", " same=" + (string)(first == second)
            + " first_positive=" + (string)(first > 0)
            + " second_positive=" + (string)(second > 0));
        llListenRemove(first);
        llListenRemove(second);
        integer handle;
        integer i;
        for (i = 1; i <= 70; ++i)
        {
            if (i >= 60)
            {
                emit("opening", " n=" + (string)i);
            }
            handle = llListen(1000 + i, "", NULL_KEY, "");
            held = i;
            if (i >= 60)
            {
                emit("opened", " n=" + (string)i
                    + " handle_positive=" + (string)(handle > 0));
            }
        }
        llListenRemove(handle);
        handle = llListen(2000, "", NULL_KEY, "");
        emit("reopened", " handle_positive=" + (string)(handle > 0));
        llListenRemove(-12345);
        emit("removed-unknown", "");
        emit("filled", " held=" + (string)held);
        llMessageLinked(LINK_THIS, 1, (string)held, "");
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (num == 2)
        {
            llMessageLinked(LINK_THIS, 3, "held " + (string)held, "");
        }
    }
}
