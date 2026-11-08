// probe: events/latency
// script: Talker
// prim: B
// order: 1
// distance: 3
// drive: none
//
// The latency probe's chat echo: a separate object, since a prim does not
// hear its own chat. `hello` is answered with `here`, so the probe learns
// this prim's key; `ping <n>` is answered with `pong <n>` at once; `mark
// <n>` writes `<n>` into this prim's description, which the probe polls
// while it is still busy in the handler that said it. Prints nothing
// itself.
integer ASK = -730201;
integer ANSWER = -730202;

default
{
    state_entry()
    {
        llListen(ASK, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "hello")
        {
            llSay(ANSWER, "here");
        }
        else if (llGetSubString(message, 0, 4) == "ping ")
        {
            llSay(ANSWER, "pong " + llGetSubString(message, 5, -1));
        }
        else if (llGetSubString(message, 0, 4) == "mark ")
        {
            llSetObjectDesc(llGetSubString(message, 5, -1));
        }
    }
}
