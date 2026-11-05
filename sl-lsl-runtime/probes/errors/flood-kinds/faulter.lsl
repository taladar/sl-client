// probe: errors/flood-kinds
// script: Faulter
// order: 1
// drive: none
//
// A second script in prim A. Sent link message 1 by "Flooder" just after
// eight refusals of one kind, it divides by zero: whether a fault's two
// lines count against the refusals' flood or are a kind of their own.
string ID = "errors/flood-kinds";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 1)
        {
            integer zero = (integer)msg;
            emit("fault", " script=faulter step=before");
            integer r = 1 / zero;
            emit("fault", " script=faulter step=after result=" + (string)r);
        }
    }
}
