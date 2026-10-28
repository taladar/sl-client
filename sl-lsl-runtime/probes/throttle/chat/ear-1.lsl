// probe: throttle/chat
// script: Ear 1
// prim: B
// distance: 5
// order: 2
// drive: none
//
// One of four listeners in prim B that count the Flood's chat (see there)
// between them, so that no listener's queue takes more than fifty of a
// burst. Ear 1 listens on channels 11, 21 and 31, and counts each
// message by its kind and wave; asked for a kind's count on channel -7420,
// it hands its counts to the Tally by link message.
integer EAR = 1;
integer CHANNEL = -7420;
list counts;

default
{
    state_entry()
    {
        integer i;
        for (i = 0; i < 44; ++i)
        {
            counts += [0];
        }
        llListen(10 + EAR, "", NULL_KEY, "");
        llListen(20 + EAR, "", NULL_KEY, "");
        llListen(30 + EAR, "", NULL_KEY, "");
        llListen(CHANNEL, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string text)
    {
        list parts = llParseString2List(text, ["|"], []);
        string what = llList2String(parts, 0);
        if (what == "f")
        {
            integer at = llList2Integer(parts, 1) * 11 + llList2Integer(parts, 2);
            counts = llListReplaceList(counts, [llList2Integer(counts, at) + 1], at, at);
        }
        else if (what == "report")
        {
            integer kind = llList2Integer(parts, 1);
            llMessageLinked(LINK_THIS, 1000 + kind,
                llList2CSV(llList2List(counts, kind * 11, kind * 11 + 10)), (string)EAR);
        }
    }
}
