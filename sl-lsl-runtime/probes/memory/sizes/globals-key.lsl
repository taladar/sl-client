// probe: memory/sizes
// script: Globals key
// order: 7
// drive: none
//
// 32 key globals holding the type's default and never touched: what
// they cost, against Globals none. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "key";
key g00;
key g01;
key g02;
key g03;
key g04;
key g05;
key g06;
key g07;
key g08;
key g09;
key g10;
key g11;
key g12;
key g13;
key g14;
key g15;
key g16;
key g17;
key g18;
key g19;
key g20;
key g21;
key g22;
key g23;
key g24;
key g25;
key g26;
key g27;
key g28;
key g29;
key g30;
key g31;

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
