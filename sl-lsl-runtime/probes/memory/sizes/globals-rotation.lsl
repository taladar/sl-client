// probe: memory/sizes
// script: Globals rotation
// order: 5
// drive: none
//
// 32 rotation globals holding the type's default and never touched: what
// they cost, against Globals none. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "rotation";
rotation g00;
rotation g01;
rotation g02;
rotation g03;
rotation g04;
rotation g05;
rotation g06;
rotation g07;
rotation g08;
rotation g09;
rotation g10;
rotation g11;
rotation g12;
rotation g13;
rotation g14;
rotation g15;
rotation g16;
rotation g17;
rotation g18;
rotation g19;
rotation g20;
rotation g21;
rotation g22;
rotation g23;
rotation g24;
rotation g25;
rotation g26;
rotation g27;
rotation g28;
rotation g29;
rotation g30;
rotation g31;

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num != 1) return;
        integer a = llGetUsedMemory();
        integer b = llGetUsedMemory();
        llOwnerSay("PROBE " + ID + " rec=globals kind=" + KIND + " count=32 used="
            + (string)a + "," + (string)b);
        llMessageLinked(LINK_THIS, 2, KIND, NULL_KEY);
    }
}
