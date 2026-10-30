// probe: memory/sizes
// script: Globals float
// order: 3
// drive: none
//
// 32 float globals holding the type's default and never touched: what
// they cost, against Globals none. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "float";
float g00;
float g01;
float g02;
float g03;
float g04;
float g05;
float g06;
float g07;
float g08;
float g09;
float g10;
float g11;
float g12;
float g13;
float g14;
float g15;
float g16;
float g17;
float g18;
float g19;
float g20;
float g21;
float g22;
float g23;
float g24;
float g25;
float g26;
float g27;
float g28;
float g29;
float g30;
float g31;

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
