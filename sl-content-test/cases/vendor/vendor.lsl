// Gadget Vendor: reads its price, its colours and its thanks from the
// notecard "config", shows a toucher a menu of the colours, and on payment
// gives the buyer the gadget of the colour chosen and thanks them.

string CONFIG = "config";
integer MENU_CHANNEL = -48151;

integer price = 0;
list colours = [];
string thanks = "Thank you";
key reading = NULL_KEY;
integer line = 0;
string chosen = "";

// One "name = value" line of the configuration.
apply(string entry)
{
    integer split = llSubStringIndex(entry, "=");
    if (split < 1)
    {
        return;
    }
    string name = llToLower(llStringTrim(llGetSubString(entry, 0, split - 1), STRING_TRIM));
    string value = llStringTrim(llGetSubString(entry, split + 1, -1), STRING_TRIM);
    if (name == "price")
    {
        price = (integer)value;
    }
    else if (name == "colours")
    {
        colours = llCSV2List(value);
    }
    else if (name == "thanks")
    {
        thanks = value;
    }
}

default
{
    state_entry()
    {
        llSetText("Reading configuration...", <1.0, 1.0, 1.0>, 1.0);
        line = 0;
        reading = llGetNotecardLine(CONFIG, line);
    }

    dataserver(key query, string data)
    {
        if (query != reading)
        {
            return;
        }
        if (data == EOF)
        {
            state selling;
        }
        else
        {
            if (llGetSubString(data, 0, 0) != "#")
            {
                apply(data);
            }
            line += 1;
            reading = llGetNotecardLine(CONFIG, line);
        }
    }
}

state selling
{
    state_entry()
    {
        llSetPayPrice(price, [price, PAY_HIDE, PAY_HIDE, PAY_HIDE]);
        llListen(MENU_CHANNEL, "", NULL_KEY, "");
        llSetText("Gadgets: L$" + (string)price, <1.0, 1.0, 1.0>, 1.0);
        llSay(0, "Ready: " + (string)llGetListLength(colours) + " colours at L$" + (string)price);
    }

    touch_start(integer count)
    {
        llDialog(llDetectedKey(0), "Choose a colour", colours, MENU_CHANNEL);
    }

    listen(integer channel, string name, key id, string message)
    {
        if (llListFindList(colours, [message]) == -1)
        {
            return;
        }
        chosen = message;
        llMessageLinked(LINK_ALL_OTHERS, 0, chosen, id);
    }

    money(key payer, integer amount)
    {
        if (chosen == "")
        {
            llSay(0, "Please choose a colour first, " + llKey2Name(payer) + ".");
            return;
        }
        integer gadgets = amount / price;
        if (gadgets < 1)
        {
            llSay(0, "L$" + (string)amount + " is not enough for a gadget.");
            return;
        }
        string gadget = chosen + " gadget";
        llSay(0, thanks + ", " + llKey2Name(payer) + "! Enjoy your " + gadget + ".");
        llGiveInventory(payer, gadget);
    }

    changed(integer change)
    {
        if (change & CHANGED_INVENTORY)
        {
            llResetScript();
        }
    }
}
