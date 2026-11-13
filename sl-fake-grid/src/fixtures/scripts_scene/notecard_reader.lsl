// Touched, reads its notecard "Greetings" line by line over the dataserver
// and says each line.
string NOTECARD = "Greetings";

key gQuery;
integer gLine;

default
{
    state_entry()
    {
        llSetText("Touch to read the notecard", <1.0, 1.0, 1.0>, 1.0);
    }

    touch_start(integer count)
    {
        gLine = 0;
        gQuery = llGetNotecardLine(NOTECARD, gLine);
    }

    dataserver(key query, string data)
    {
        if (query == gQuery)
        {
            if (data == EOF)
            {
                llSay(0, "End of " + NOTECARD);
            }
            else
            {
                ++gLine;
                llSay(0, "Line " + (string)gLine + ": " + data);
                gQuery = llGetNotecardLine(NOTECARD, gLine);
            }
        }
    }
}
