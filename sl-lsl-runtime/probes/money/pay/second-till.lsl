// probe: money/pay
// script: Second till
// order: 1
// drive: none
//
// A second script with a money event in prim A, beside the leader (till.lsl):
// whether one payment runs the money event of every script in the prim. It
// prints each payment it hears and tells the leader of it by link message
// (so the leader can count what the other avatar paid while it is in a state
// with no money event), and removes itself when the leader asks.
string ID = "money/pay";

string who(key id)
{
    if (id == llGetOwner())
    {
        return "owner";
    }
    if (id == NULL_KEY)
    {
        return "none";
    }
    return "other";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    money(key id, integer amount)
    {
        emit("money", " script=second payer=" + who(id) + " amount=" + (string)amount);
        llMessageLinked(LINK_THIS, amount, "paid", id);
    }

    link_message(integer sender, integer number, string text, key id)
    {
        if (text == "remove")
        {
            emit("removed", " script=second");
            llRemoveInventory(llGetScriptName());
        }
    }
}
