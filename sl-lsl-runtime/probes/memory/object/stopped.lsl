// probe: memory/object
// script: Stopped
// order: 4
// drive: none
//
// A Mono script that stops itself as it starts: in the object's total
// script count, and on the grid's word not among the running ones.

default
{
    state_entry()
    {
        llSetScriptState(llGetScriptName(), FALSE);
    }
}
