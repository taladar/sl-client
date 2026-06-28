//! Tests for the LLSD value model and the LLSD-XML codec.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_llsd::{Llsd, parse_llsd_xml};

    type TestError = Box<dyn std::error::Error>;

    #[test]
    fn parses_scalar_llsd_types() -> Result<(), TestError> {
        let xml = "<llsd><map>\
            <key>i</key><integer>42</integer>\
            <key>r</key><real>1.5</real>\
            <key>b</key><boolean>1</boolean>\
            <key>s</key><string>hi</string>\
            <key>bin</key><binary>AQID</binary>\
            </map></llsd>";
        let llsd = parse_llsd_xml(xml)?;
        assert_eq!(llsd.get("i").and_then(Llsd::as_i32), Some(42));
        assert_eq!(
            llsd.get("r").and_then(Llsd::as_f64).map(f64::to_bits),
            Some(1.5_f64.to_bits())
        );
        assert_eq!(llsd.get("b").and_then(Llsd::as_bool), Some(true));
        assert_eq!(llsd.get("s").and_then(Llsd::as_str), Some("hi"));
        assert_eq!(
            llsd.get("bin").and_then(Llsd::as_binary),
            Some(&[1u8, 2, 3][..])
        );
        Ok(())
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(
            parse_llsd_xml("<llsd><map>").err().is_some(),
            "expected a parse error"
        );
    }

    #[test]
    fn serializes_every_scalar_and_round_trips() -> Result<(), TestError> {
        let uuid = uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555")?;
        let tree = Llsd::Map(
            [
                ("undef".to_owned(), Llsd::Undef),
                ("yes".to_owned(), Llsd::Boolean(true)),
                ("no".to_owned(), Llsd::Boolean(false)),
                ("int".to_owned(), Llsd::Integer(-42)),
                ("real".to_owned(), Llsd::Real(1.5)),
                (
                    "str".to_owned(),
                    Llsd::String("a < b & c > \"d\"".to_owned()),
                ),
                ("uuid".to_owned(), Llsd::Uuid(uuid)),
                (
                    "date".to_owned(),
                    Llsd::Date("2026-06-18T00:00:00Z".to_owned()),
                ),
                (
                    "uri".to_owned(),
                    Llsd::Uri("http://example.com/x".to_owned()),
                ),
                ("bin".to_owned(), Llsd::Binary(vec![1, 2, 3, 0, 255])),
            ]
            .into_iter()
            .collect(),
        );
        let xml = tree.to_llsd_xml();
        assert!(xml.starts_with("<llsd>") && xml.ends_with("</llsd>"));
        assert_eq!(parse_llsd_xml(&xml)?, tree);
        Ok(())
    }

    #[test]
    fn serializes_nested_arrays_and_maps() -> Result<(), TestError> {
        let tree = Llsd::Array(vec![
            Llsd::Array(vec![Llsd::Integer(1), Llsd::Integer(2)]),
            Llsd::Map(
                [
                    ("k".to_owned(), Llsd::String("v".to_owned())),
                    ("inner".to_owned(), Llsd::Array(vec![Llsd::Boolean(true)])),
                ]
                .into_iter()
                .collect(),
            ),
            Llsd::Undef,
        ]);
        assert_eq!(parse_llsd_xml(&tree.to_llsd_xml())?, tree);
        Ok(())
    }

    #[test]
    fn map_keys_are_sorted_for_deterministic_output() {
        let tree = Llsd::Map(
            [
                ("zebra".to_owned(), Llsd::Integer(1)),
                ("alpha".to_owned(), Llsd::Integer(2)),
                ("mike".to_owned(), Llsd::Integer(3)),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            tree.to_llsd_xml(),
            "<llsd><map>\
             <key>alpha</key><integer>2</integer>\
             <key>mike</key><integer>3</integer>\
             <key>zebra</key><integer>1</integer>\
             </map></llsd>"
        );
    }
}
