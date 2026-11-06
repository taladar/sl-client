// corpus: a cast out of a string reads what it can and never fails
// Oracle: PyOptimizer unit_tests/expr.suite/casts; the value model's
// "(integer)\"0x1A\" reads hex, (integer)\"  12abc\" reads 12, (integer)\"abc\"
// is 0".
default
{
    state_entry()
    {
        print((string)(integer)"0x1A");
        print((string)(integer)"  12abc");
        print((string)(integer)"abc");
        print((string)(integer)"-7.9");
        print((string)(float)"1e3");
        print((string)(float)"  -2.5xyz");
        print((string)(float)"nonsense");
        print((string)(vector)"<1, 2, 3>");
        print((string)(vector)"<1, 2>");
        print((string)(rotation)"<0, 0, 0, 1>");
    }
}
