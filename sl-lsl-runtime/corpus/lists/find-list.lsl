// corpus: llListFindList finds a sublist by type and value, never by a cast
// Oracle: OpenSim's own LSL_ApiListTests.TestllListFindList, its cases in
// order (the last without OS_NPC_LAND_AT_TARGET, which Second Life does not
// compile).
default
{
    state_entry()
    {
        list src = [1, 2, 3];
        print((string)llListFindList(src, [4]));
        print((string)llListFindList(src, [2]));
        print((string)llListFindList(src, [AGENT]));
        print((string)llListFindList(src, [2, 3]));
        print((string)llListFindList(src, ["4"]));
        print((string)llListFindList(src, ["2"]));
        print((string)llListFindList(src, [2, 3, 4]));
        print((string)llListFindList([3, AGENT, 2], [1, 2]));
        print((string)llListFindList(src, []));
    }
}
