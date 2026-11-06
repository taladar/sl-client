// corpus: reading, slicing and searching lists
// Oracle: the SL wiki's per-function pages; PyOptimizer expr.suite/list-funcs.
default
{
    state_entry()
    {
        list l = [1, 2.5, "three", <4, 5, 6>, "0a0a0a0a-0000-4000-8000-000000000001"];
        print((string)llGetListLength(l));
        print((string)llList2Integer(l, 0) + " " + (string)llList2Float(l, 1));
        print(llList2String(l, -3));
        print((string)llList2Vector(l, 3));
        print((string)llGetListEntryType(l, 2) + " " + (string)llGetListEntryType(l, 9));
        print(llList2CSV(llList2List(l, 1, 2)));
        print(llList2CSV(llList2List(l, 3, 1)));
        print(llList2CSV(llDeleteSubList(l, 0, 2)));
        print((string)llListFindList(l, ["three"]) + " " + (string)llListFindList(l, [3]));
        print(llList2CSV(llListReplaceList(l, ["x", "y"], 1, 2)));
        print(llList2CSV(llListSort([3, 1, 2], 1, TRUE)) + " / " + llList2CSV(llListSort([3, 1, 2], 1, FALSE)));
        print(llList2CSV(llCSV2List("1, <1, 2, 3>, x")));
    }
}
