// probe: memory/sizes
// script: Globals vector
// order: 4
// drive: none
//
// 32 vector globals holding the type's default and never touched: what
// they cost, against Globals none. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "vector";
vector g00;
vector g01;
vector g02;
vector g03;
vector g04;
vector g05;
vector g06;
vector g07;
vector g08;
vector g09;
vector g10;
vector g11;
vector g12;
vector g13;
vector g14;
vector g15;
vector g16;
vector g17;
vector g18;
vector g19;
vector g20;
vector g21;
vector g22;
vector g23;
vector g24;
vector g25;
vector g26;
vector g27;
vector g28;
vector g29;
vector g30;
vector g31;

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
