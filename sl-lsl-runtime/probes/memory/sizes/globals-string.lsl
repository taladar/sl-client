// probe: memory/sizes
// script: Globals string
// order: 6
// drive: none
//
// 32 string globals holding the type's default and never touched: what
// they cost, against Globals none. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "string";
string g00;
string g01;
string g02;
string g03;
string g04;
string g05;
string g06;
string g07;
string g08;
string g09;
string g10;
string g11;
string g12;
string g13;
string g14;
string g15;
string g16;
string g17;
string g18;
string g19;
string g20;
string g21;
string g22;
string g23;
string g24;
string g25;
string g26;
string g27;
string g28;
string g29;
string g30;
string g31;

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
