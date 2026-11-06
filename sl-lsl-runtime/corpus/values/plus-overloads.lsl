// corpus: + appends to a list, prepends to one, and joins strings
// Oracle: the value model's "+ overloads"; PyOptimizer expr.suite/operators:
// ["1"] + ["2"], "1" + ["2"], ["1"] + "2".
list built;

default
{
    state_entry()
    {
        built = [1] + 2;
        built = 0 + built;
        built = built + [3, 4];
        built = "start" + built;
        print(llList2CSV(built));
        print(llList2CSV(["1"] + ["2"]));
        print(llList2CSV("1" + ["2"]));
        print("1" + "2");
    }
}
