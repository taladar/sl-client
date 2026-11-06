// corpus: vector * vector is the dot product and vector % vector the cross
// Oracle: the value model's arithmetic list; PyOptimizer expr.suite/operators
// (<3.,4.,5.>*<3.,4.,5.> is 50.).
default
{
    state_entry()
    {
        print((string)(<3, 4, 5> * <3, 4, 5>));
        print((string)(<1, 0, 0> % <0, 1, 0>));
        print((string)(<1, 2, 3> * 2));
        print((string)(<2, 4, 6> / 2));
        print((string)(<1, 2, 3> + <1, 1, 1>));
        print((string)(-<1, -2, 3>));
    }
}
