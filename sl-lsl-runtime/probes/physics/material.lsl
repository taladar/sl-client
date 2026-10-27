// probe: physics/material
// drive: none
//
// llSetPhysicsMaterial's clamps and the mass functions, on each grid, all on
// a prim that is not physical. A fresh prim's llGetPhysicsMaterial (gravity
// multiplier, restitution, friction, density), its llGetMass, llGetMassMKS
// and llGetObjectMass, its centre of mass against its position and its
// scale and physics shape type. Then each material term set on its own to
// values inside and outside the documented range, read back with the mass;
// a call with no bits, which should change nothing, and one with all four.
// Last the mass of the prim at other scales and under each physics shape
// type, put back as it was.
string ID = "physics/material";

vector home_scale;

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

string material()
{
    list m = llGetPhysicsMaterial();
    return " length=" + (string)llGetListLength(m)
        + " gravity=" + llList2String(m, 0)
        + " restitution=" + llList2String(m, 1)
        + " friction=" + llList2String(m, 2)
        + " density=" + llList2String(m, 3);
}

string mass()
{
    return " mass=" + (string)llGetMass()
        + " mass_mks=" + (string)llGetMassMKS()
        + " object_mass=" + (string)llGetObjectMass(llGetKey());
}

reset_material()
{
    llSetPhysicsMaterial(GRAVITY_MULTIPLIER | RESTITUTION | FRICTION | DENSITY,
        1.0, 0.5, 0.6, 1000.0);
}

try(string term, integer bit, float value)
{
    reset_material();
    float g = 1.0;
    float r = 0.5;
    float f = 0.6;
    float d = 1000.0;
    if (bit == GRAVITY_MULTIPLIER)
    {
        g = value;
    }
    else if (bit == RESTITUTION)
    {
        r = value;
    }
    else if (bit == FRICTION)
    {
        f = value;
    }
    else
    {
        d = value;
    }
    llSetPhysicsMaterial(bit, g, r, f, d);
    emit("set", " term=" + term + " value=" + (string)value + material() + mass());
}

default
{
    state_entry()
    {
        begin();
        home_scale = llGetScale();
        list shape = llGetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE]);
        emit("fresh", material() + mass()
            + " scale=" + q((string)home_scale)
            + " shape_type=" + llList2String(shape, 0)
            + " centre_offset=" + q((string)(llGetCenterOfMass() - llGetPos())));

        list g = [-2.0, -1.0, 0.0, 0.5, 28.0, 40.0];
        list r = [-1.0, 0.0, 1.0, 2.0];
        list f = [-1.0, 0.0, 255.0, 300.0];
        list d = [-5.0, 0.0, 1.0, 22587.0, 30000.0];
        integer i;
        for (i = 0; i < llGetListLength(g); ++i)
        {
            try("gravity", GRAVITY_MULTIPLIER, llList2Float(g, i));
        }
        for (i = 0; i < llGetListLength(r); ++i)
        {
            try("restitution", RESTITUTION, llList2Float(r, i));
        }
        for (i = 0; i < llGetListLength(f); ++i)
        {
            try("friction", FRICTION, llList2Float(f, i));
        }
        for (i = 0; i < llGetListLength(d); ++i)
        {
            try("density", DENSITY, llList2Float(d, i));
        }

        reset_material();
        llSetPhysicsMaterial(0, 5.0, 0.9, 9.0, 5000.0);
        emit("set", " term=none" + material() + mass());
        llSetPhysicsMaterial(GRAVITY_MULTIPLIER | RESTITUTION | FRICTION | DENSITY,
            0.25, 0.75, 0.125, 500.0);
        emit("set", " term=all" + material() + mass());
        reset_material();

        list scales = [<1.0, 1.0, 1.0>, <2.0, 2.0, 2.0>, <4.0, 0.5, 0.5>];
        for (i = 0; i < llGetListLength(scales); ++i)
        {
            llSetScale(llList2Vector(scales, i));
            emit("scaled", " scale=" + q((string)llGetScale()) + mass());
        }
        llSetScale(home_scale);

        list shapes = [PRIM_PHYSICS_SHAPE_CONVEX, PRIM_PHYSICS_SHAPE_PRIM,
            PRIM_PHYSICS_SHAPE_NONE];
        for (i = 0; i < llGetListLength(shapes); ++i)
        {
            llSetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE, llList2Integer(shapes, i)]);
            shape = llGetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE]);
            emit("shape", " asked=" + (string)llList2Integer(shapes, i)
                + " shape_type=" + llList2String(shape, 0) + mass());
        }
        llSetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE, PRIM_PHYSICS_SHAPE_PRIM]);
        emit("end", "");
    }
}
