// probe: memory/sizes
// script: Globals list
// order: 8
// drive: none
//
// 32 list globals holding the type's default and never touched: what
// they cost, against Globals none. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "list";
list g00;
list g01;
list g02;
list g03;
list g04;
list g05;
list g06;
list g07;
list g08;
list g09;
list g10;
list g11;
list g12;
list g13;
list g14;
list g15;
list g16;
list g17;
list g18;
list g19;
list g20;
list g21;
list g22;
list g23;
list g24;
list g25;
list g26;
list g27;
list g28;
list g29;
list g30;
list g31;

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
