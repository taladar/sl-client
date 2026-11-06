// corpus: integer % 0 is a Math Error
// Oracle: PyOptimizer expr.suite/math-error.
default
{
    state_entry()
    {
        integer zero = 0;
        print("before");
        print((string)(7 % zero));
        print("after");
    }
}
