// probe: memory/sizes
// script: Globals integer
// order: 2
// drive: none
//
// 32 integer globals holding the type's default and never touched: what
// they cost, against Globals none. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "integer";
integer g00;
integer g01;
integer g02;
integer g03;
integer g04;
integer g05;
integer g06;
integer g07;
integer g08;
integer g09;
integer g10;
integer g11;
integer g12;
integer g13;
integer g14;
integer g15;
integer g16;
integer g17;
integer g18;
integer g19;
integer g20;
integer g21;
integer g22;
integer g23;
integer g24;
integer g25;
integer g26;
integer g27;
integer g28;
integer g29;
integer g30;
integer g31;

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
