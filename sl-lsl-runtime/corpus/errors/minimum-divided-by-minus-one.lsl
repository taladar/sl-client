// corpus: the one integer division that overflows wraps instead of failing
// Oracle: PyOptimizer expr.suite/operators: 0x80000000/-1 is -2147483648.
default
{
    state_entry()
    {
        integer minimum = 0x80000000;
        print((string)(minimum / -1));
        print((string)(minimum % -1));
    }
}
