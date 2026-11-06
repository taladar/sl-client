// corpus: list == list compares lengths and list != list is their difference
// Oracle: the value model ("list == list compares lengths, not contents") and
// the book ("list != list is the length difference, not a boolean").
default
{
    state_entry()
    {
        print((string)([1, 2, 3] == ["a", "b", "c"]));
        print((string)([1, 2, 3] == [1, 2]));
        print((string)([1, 2, 3] != [1]));
        print((string)([1] != [1, 2, 3]));
        print((string)([] == []));
    }
}
