// probe: persistence/rerez
// script: Stopped
// order: 1
// drive: none
//
// The script the probe stops before its object is taken: it answers the
// probe's question (link message 99) with link message 98 while it runs,
// and prints whenever it starts or its object is rezzed, which a stopped
// script does not.
string ID = "persistence/rerez";
integer ASK_STOPPED = 99;
integer STOPPED_SAYS = 98;

default
{
    state_entry()
    {
        llOwnerSay("PROBE " + ID + " rec=stopped_entry");
    }

    on_rez(integer param)
    {
        llOwnerSay("PROBE " + ID + " rec=stopped_on_rez");
    }

    link_message(integer sender, integer number, string text, key id)
    {
        if (number == ASK_STOPPED)
        {
            llMessageLinked(LINK_THIS, STOPPED_SAYS, "running", NULL_KEY);
        }
    }
}
