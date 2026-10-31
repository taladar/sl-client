//! Where a float expression is rounded, held to Second Life bit for bit:
//! the probes' own expressions run in the VM, printing each result's
//! mantissa and exponent the way the probe does, against the aditi
//! transcripts of `math/rotation-bits` and `math/float-rounding`.

use pretty_assertions::assert_eq;

use super::*;

/// The probes' helpers: `emit` (with `print` for `llOwnerSay`) and `bits`,
/// which prints a float as `<sign><hi>:<lo>p<exp>`. Each starts with `ID`
/// declared by the script.
const HELPERS: &str = r#"
emit(string rec, string fields)
{
    print("PROBE " + ID + " rec=" + rec + fields);
}

string bits(float f)
{
    if (f != f) return "nan";
    if (f == 0.0) return "0";
    string sign = "+";
    float a = f;
    if (a < 0.0)
    {
        sign = "-";
        a = -a;
    }
    if (a == a * 2.0) return sign + "inf";
    integer e = 0;
    while (a >= 16777216.0)
    {
        a = a / 2.0;
        ++e;
    }
    while (a < 8388608.0)
    {
        a = a * 2.0;
        --e;
    }
    integer hi = (integer)a;
    float rest = a - (float)hi;
    integer lo = (integer)(rest * 536870912.0);
    return sign + (string)hi + ":" + (string)lo + "p" + (string)e;
}
"#;

/// Run `source` until it has printed its `end` record, returning every
/// `PROBE` line it printed.
fn probe_lines(source: &str) -> Result<Vec<String>, String> {
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(A, program(source)?);
    for _ in 0..2000 {
        let _reports = run(&mut engine, &mut host, 1);
        if host.texts(A).iter().any(|line| line.contains(" rec=end")) {
            assert!(host.stubbed.is_empty(), "{:?}", host.stubbed);
            return Ok(host.texts(A).into_iter().map(str::to_owned).collect());
        }
    }
    Err(format!(
        "the script never ended; it printed {:?} and said {:?}",
        host.texts(A),
        host.heard(A)
    ))
}

/// The lines of an aditi transcript whose record is one of `recs`.
fn transcript_lines(transcript: &str, recs: &[&str]) -> Vec<String> {
    transcript
        .lines()
        .filter_map(|line| {
            crate::probe::Line::parse(line)
                .ok()
                .map(|parsed| (line, parsed))
        })
        .filter(|(_, parsed)| recs.contains(&parsed.rec.as_str()))
        .map(|(line, _)| line.to_owned())
        .collect()
}

/// A float printed as `bits` prints it, read back.
fn read_bits(text: &str) -> Result<f32, String> {
    if text == "0" {
        return Ok(0.0);
    }
    let (sign, rest) = text.split_at(1);
    let (mantissa, exponent) = rest
        .split_once('p')
        .ok_or_else(|| format!("no exponent in {text}"))?;
    let (hi, lo) = mantissa
        .split_once(':')
        .ok_or_else(|| format!("no `:` in {text}"))?;
    if lo != "0" {
        return Err(format!("{text} is not a float"));
    }
    let hi: i32 = hi.parse().map_err(|error| format!("{text}: {error}"))?;
    let exponent: i32 = exponent
        .parse()
        .map_err(|error| format!("{text}: {error}"))?;
    let magnitude = f64::from(hi) * 2.0_f64.powi(exponent);
    let value = if sign == "-" { -magnitude } else { magnitude };
    Ok(crate::num::to_f32(value))
}

/// A float as an LSL literal that reads back as exactly that float.
fn literal(value: f32) -> String {
    let text = format!("{value:?}");
    if text.contains(['.', 'e']) {
        text
    } else {
        format!("{text}.0")
    }
}

#[test]
fn a_sum_of_products_is_rounded_once_where_it_is_stored_as_on_aditi() -> Result<(), String> {
    let transcript = include_str!("../../../probes/results/aditi/math/rotation-bits.txt");
    let inputs = transcript_lines(transcript, &["input-bits"]);
    let mut pairs = Vec::new();
    for pair in inputs.chunks(2) {
        let mut rotations = Vec::new();
        for line in pair {
            let parsed = crate::probe::Line::parse(line).map_err(|error| error.to_string())?;
            let parts = ["x", "y", "z", "s"]
                .iter()
                .map(|key| {
                    parsed
                        .field(key)
                        .ok_or_else(|| format!("no {key} in {line}"))
                        .and_then(read_bits)
                        .map(literal)
                })
                .collect::<Result<Vec<_>, _>>()?;
            rotations.push(format!("<{}>", parts.join(", ")));
        }
        pairs.push(rotations.join(", "));
    }
    assert_eq!(pairs.len(), 7);
    let calls = pairs
        .iter()
        .enumerate()
        .map(|(case, pair)| format!("pair({case}, {pair});\n"))
        .collect::<Vec<_>>()
        .concat();
    let source = format!(
        r#"string ID = "math/rotation-bits";
{HELPERS}
string rbits(rotation r)
{{
    return " x=" + bits(r.x) + " y=" + bits(r.y) + " z=" + bits(r.z) + " s=" + bits(r.s);
}}

pair(integer n, rotation a, rotation b)
{{
    string c = " case=" + (string)n;
    emit("input-bits", c + " operand=a" + rbits(a));
    emit("input-bits", c + " operand=b" + rbits(b));
    float s = a.x * b.x + a.y * b.y;
    emit("two-products", c + " form=one-expression value=" + bits(s));
    float p = a.x * b.x;
    float t = a.y * b.y;
    s = p + t;
    emit("two-products", c + " form=two-statements value=" + bits(s));
    s = a.x * b.x * a.y;
    emit("three-factors", c + " form=one-expression value=" + bits(s));
    s = p * a.y;
    emit("three-factors", c + " form=two-statements value=" + bits(s));
}}

default {{ state_entry() {{
{calls}emit("end", "");
}} }}"#
    );
    let ours = probe_lines(&source)?;
    let recs = ["input-bits", "two-products", "three-factors"];
    let aditi = transcript_lines(transcript, &recs);
    assert_eq!(aditi.len(), 7 * 6);
    let ours: Vec<String> = transcript_lines(&ours.join("\n"), &recs);
    assert_eq!(ours, aditi);
    Ok(())
}

#[test]
fn every_rounding_point_of_float_rounding_is_where_aditi_has_it() -> Result<(), String> {
    // The probe with each `llList2Float(xs, i)` replaced by the literal it
    // reads (a float in a local is the same float).
    let source = format!(
        r#"string ID = "math/float-rounding";
{HELPERS}
point(string name, float residual)
{{
    emit("point", " name=" + name + " value=" + bits(residual));
}}

flag(string name, integer value)
{{
    emit("point", " name=" + name + " value=" + (string)value);
}}

float A;
float B;
float R;

float minus_r(float x)
{{
    float d = x - R;
    return d;
}}

float product(float x, float y)
{{
    return x * y;
}}

default
{{
    state_entry()
    {{
        A = 1.1;
        B = 1.3;
        R = A * B;
        emit("input", " a=" + bits(A) + " b=" + bits(B) + " r=" + bits(R));
        float s = A * B - R;
        point("expression", s);
        float t = A * B;
        s = t - R;
        point("local", s);
        s = A / B * B - A;
        point("quotient", s);
        point("user-argument", minus_r(A * B));
        s = product(A, B) - R;
        point("user-return", s);
        s = llFabs(A * B) - R;
        point("library-argument", s);
        float two = 2.0;
        float root = llSqrt(two);
        s = llSqrt(two) - root;
        point("library-return", s);
        s = -(A * B) + R;
        point("negation", s);
        s = llList2Float([A * B], 0) - R;
        point("list-element", s);
        list l = [];
        l += A * B;
        s = llList2Float(l, 0) - R;
        point("list-append", s);
        vector v = <A * B, 0.0, 0.0>;
        s = v.x - R;
        point("vector-component", s);
        rotation r = <A * B, 0.0, 0.0, 1.0>;
        s = r.x - R;
        point("rotation-component", s);
        v.y = A * B;
        s = v.y - R;
        point("member-store", s);
        s = (float)(A * B) - R;
        point("cast-float", s);
        float under = 1.4999996;
        float over = 2.0000005;
        integer i = (integer)(under * over);
        flag("cast-integer", i);
        float e = 1.05;
        float f = 3.86;
        emit("point", " name=cast-string value=" + (string)(e * f));
        flag("equal", A * B == R);
        flag("not-equal", A * B != R);
        flag("greater", A * B > R);
        float tiny = 1.0e-30;
        float tinier = 1.0e-20;
        integer taken = 0;
        if (tiny * tinier) taken = 1;
        flag("condition", taken);
        s = (t = A * B) - R;
        point("assignment-value", s);
        float c = 0.4;
        s = c;
        s += A * B;
        point("compound", s);
        s = c + A * B;
        point("compound-one-expression", s);
        s = c + R;
        point("compound-two-statements", s);
        s = c;
        t = (s += A * B) - (c + R);
        point("compound-value", t);
        float big = 16777216.0;
        s = big;
        t = (++s) - big;
        point("increment-value", t);
        t = s - big;
        point("increment-stored", t);
        s = big;
        t = (s += 1.0) - big;
        point("add-assign-value", t);
        integer n = 16777217;
        s = (n * 1.0) - big;
        point("integer-operand", s);
        s = (float)n - big;
        point("integer-cast", s);
        s = n;
        s = s - big;
        point("integer-stored", s);
        float k = 0.1;
        v = <k, 0.0, 0.0> * (A * B);
        emit("point", " name=vector-scale value=" + bits(v.x)
            + " rounded=" + bits(k * R));
        v = <A, B, 0.0>;
        vector w = <B, A, 0.0>;
        float d = v * w;
        s = v * w - d;
        point("dot-in-expression", s);
        float huge = 1.0e30;
        s = huge * huge / huge;
        point("overflow-and-back", s);
        float unit = 1.0;
        float half = 5.9604645e-08;
        float least = 8.6736174e-19;
        s = unit + half + least;
        point("extended", s);
        emit("end", "");
    }}
}}"#
    );
    let ours = probe_lines(&source)?;
    let transcript = include_str!("../../../probes/results/aditi/math/float-rounding.txt");
    let aditi = transcript_lines(transcript, &["input", "point"]);
    assert_eq!(aditi.len(), 36);
    let ours = transcript_lines(&ours.join("\n"), &["input", "point"]);
    assert_eq!(ours, aditi);
    Ok(())
}
