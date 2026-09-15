// Where a `<` or `>` inside a vector constructor compares rather than closes,
// following the grid's grammar rather than a stricter rule of thumb:
//
// - every component but the last cannot be closed, so they compare freely;
// - in the last, a `>` compares when an operand follows it — unless that
//   operand starts with `-` or `<`, which close the constructor instead (a
//   vector is never a valid `<` operand, so only the `-` case appears here).
float q = 1.0;
vector v = <1, q, 3>;

default
{
    state_entry()
    {
        integer a = 1;
        integer b = 2;
        vector first = <a > b, a < b, 0>;
        vector last = <0, 0, a > b>;
        rotation r = <0, a > b, 0, a > b>;
        vector nested = <1, 2, <1, 1, 1> * <1, 1 > 1, 1> >;
        list wrapped = [<1, 2, <1, 1, 1> * <1, 1 > 1, 1> >];
        vector difference = <1, 2, 3> - v;
        llOwnerSay((string)first + (string)last + (string)r + (string)nested
            + (string)wrapped + (string)difference);
    }
}
