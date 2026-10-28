// probe: throttle/chat
// script: Tally
// prim: B
// distance: 5
// order: 5
// drive: none
//
// Prim B's spokesman (see the Flood). It answers the Flood's hello on
// channel -7420 with prim B's key, and when the Flood asks for a kind's
// count, it adds up what the four Ears hand it and prints, for each wave of
// that kind, how many of the Flood's messages arrived; then it tells the
// Flood it is done.
string ID = "throttle/chat";

integer CHANNEL = -7420;
key flood;
integer kind = -1;
integer reports;
list sums;

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    state_entry()
    {
        llListen(CHANNEL, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string text)
    {
        if (text == "hello")
        {
            flood = id;
            llRegionSayTo(id, CHANNEL, "here|" + (string)llGetKey());
        }
    }

    link_message(integer sender, integer num, string text, key id)
    {
        if (num < 1000)
        {
            return;
        }
        if (num - 1000 != kind || reports >= 4)
        {
            kind = num - 1000;
            reports = 0;
            sums = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        }
        list counts = llCSV2List(text);
        integer i;
        for (i = 0; i < 11; ++i)
        {
            sums = llListReplaceList(sums,
                [llList2Integer(sums, i) + llList2Integer(counts, i)], i, i);
        }
        ++reports;
        if (reports < 4)
        {
            return;
        }
        integer waves = 11;
        if (kind == 0)
        {
            waves = 1;
        }
        for (i = 0; i < waves; ++i)
        {
            emit("heard", " kind=" + (string)kind + " wave=" + (string)i
                + " count=" + (string)llList2Integer(sums, i));
        }
        llRegionSayTo(flood, CHANNEL, "done|" + (string)kind);
    }
}
