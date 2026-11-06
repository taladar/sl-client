// corpus: llGetSubString counts negative indices from the end and wraps
// round when start is after end
// Oracle: the SL wiki's llGetSubString page (negative indices; "if start is
// greater than end the sub string is the exclusion of the entries").
default
{
    state_entry()
    {
        string s = "abcdefgh";
        print(llGetSubString(s, 0, 2));
        print(llGetSubString(s, -3, -1));
        print(llGetSubString(s, 6, 1));
        print(llGetSubString(s, -1, 0));
        print(llGetSubString(s, 3, 100));
        print(llGetSubString(s, 100, 200));
        print(llDeleteSubString(s, 6, 1));
    }
}
