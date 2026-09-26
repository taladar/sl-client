// Every arithmetic result below reaches a parameter of the type the operator
// table says it produces, so a wrong row in `sl_lsl::types` would surface as a
// false "argument type" error here.
default
{
    state_entry()
    {
        vector v = <1.0, 2.0, 3.0>;
        rotation r = <0.0, 0.0, 0.0, 1.0>;
        list l = [1];
        llSetPos(v * 2);
        llSetPos(2.0 * v);
        llSetPos(v * r);
        llSetPos(v / r);
        llSetPos(v % v);
        llSetPos(v / 2.0);
        llSetRot(r * r);
        llSetRot(r / r);
        llSetRot(r + r - r);
        llSetAlpha(v * v, ALL_SIDES);
        llSetAlpha(1 + 2.0, ALL_SIDES);
        llSetAlpha(7 / 2, ALL_SIDES);
        llOwnerSay("a" + "b");
        llOwnerSay((string)(l + 2 + l));
        llOwnerSay(llList2CSV(3 + l));
        llSleep((l == l) + (v != v) + ("a" != (key)"b"));
        llSleep(5 % 3 << 1 >> 1 & 7 | 8 ^ 1);
        integer i = 2;
        i *= 1.5;
        llSleep(i);
    }
}
