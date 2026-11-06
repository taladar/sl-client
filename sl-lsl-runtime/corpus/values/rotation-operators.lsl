// corpus: a rotation rotates a vector and composes in Linden's order
// Oracle: the value model ("vector * rotation rotates, vector / rotation
// rotates by the inverse, rotation * rotation composes -- in Linden's
// order"); the book's simulator/lsl-engine.md, held to math/rotation-bits.
default
{
    state_entry()
    {
        rotation quarter = llEuler2Rot(<0, 0, PI_BY_TWO>);
        rotation tilt = llEuler2Rot(<PI_BY_TWO, 0, 0>);
        // East turned a quarter about Z is north.
        print((string)(<1, 0, 0> * quarter));
        // And back again.
        print((string)(<0, 1, 0> / quarter));
        // Linden's a * b applies a first, then b.
        print((string)(<0, 1, 0> * (quarter * tilt)));
        print((string)((<0, 1, 0> * quarter) * tilt));
        print((string)(<0, 1, 0> * (tilt * quarter)));
    }
}
