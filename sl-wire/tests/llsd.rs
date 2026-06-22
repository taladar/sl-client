//! Tests for the LLSD-XML parser and the capability request/response helpers.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_types::key::InventoryFolderKey;
    use sl_wire::{
        EventQueueEvent, Llsd, build_event_queue_request, build_event_queue_response,
        build_seed_request, parse_event_queue_response, parse_llsd_xml, parse_seed_response,
    };

    type TestError = Box<dyn std::error::Error>;

    #[test]
    fn event_queue_response_round_trips() -> Result<(), TestError> {
        let events = vec![
            EventQueueEvent {
                message: "EnableSimulator".to_owned(),
                body: Llsd::Map(std::collections::HashMap::from([(
                    "Handle".to_owned(),
                    Llsd::Integer(7),
                )])),
            },
            EventQueueEvent {
                message: "TeleportFinish".to_owned(),
                body: Llsd::Array(vec![Llsd::String("x".to_owned())]),
            },
        ];
        let xml = build_event_queue_response(42, &events);
        let parsed = parse_event_queue_response(&xml)?;
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.events, events);
        Ok(())
    }

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
    fn parses_seed_response_map() -> Result<(), TestError> {
        let xml = "<llsd><map>\
            <key>EventQueueGet</key><string>http://127.0.0.1:9000/CAPS/EQG</string>\
            <key>SeedCapability</key><string>http://127.0.0.1:9000/CAPS/SEED</string>\
            </map></llsd>";
        let caps = parse_seed_response(xml)?;
        assert_eq!(
            caps.get("EventQueueGet").map(String::as_str),
            Some("http://127.0.0.1:9000/CAPS/EQG")
        );
        Ok(())
    }

    #[test]
    fn parses_event_queue_parcel_properties() -> Result<(), TestError> {
        let xml = "<llsd><map><key>events</key><array><map>\
            <key>message</key><string>ParcelProperties</string>\
            <key>body</key><map><key>ParcelData</key><array><map>\
              <key>LocalID</key><integer>7</integer>\
              <key>Area</key><integer>4096</integer>\
              <key>ParcelFlags</key><integer>1088</integer>\
              <key>MaxPrims</key><integer>1000</integer>\
              <key>SimWideMaxPrims</key><integer>5000</integer>\
              <key>AABBMin</key><array><real>0</real><real>0</real><real>0</real></array>\
              <key>AABBMax</key><array><real>64</real><real>48</real><real>0</real></array>\
              <key>Bitmap</key><binary>AQID</binary>\
            </map></array></map>\
            </map></array><key>id</key><integer>5</integer></map></llsd>";
        let response = parse_event_queue_response(xml)?;
        assert_eq!(response.id, 5);
        assert_eq!(response.events.len(), 1);
        let event = response.events.first().ok_or("one event")?;
        assert_eq!(event.message, "ParcelProperties");

        let parcel = event
            .body
            .get("ParcelData")
            .and_then(|data| data.index(0))
            .ok_or("ParcelData[0]")?;
        assert_eq!(parcel.get("LocalID").and_then(Llsd::as_i32), Some(7));
        assert_eq!(parcel.get("Area").and_then(Llsd::as_i32), Some(4096));
        assert_eq!(parcel.get("ParcelFlags").and_then(Llsd::as_i32), Some(1088));
        let aabb_max = parcel.get("AABBMax").ok_or("AABBMax")?;
        assert_eq!(
            aabb_max.index(0).and_then(Llsd::as_f64).map(f64::to_bits),
            Some(64.0_f64.to_bits())
        );
        assert_eq!(
            parcel.get("Bitmap").and_then(Llsd::as_binary),
            Some(&[1u8, 2, 3][..])
        );
        Ok(())
    }

    #[test]
    fn builds_request_bodies() {
        let seed = build_seed_request(&["EventQueueGet", "ParcelProperties"]);
        assert!(seed.contains("<string>EventQueueGet</string>"));
        assert!(seed.starts_with("<llsd><array>"));

        assert!(build_event_queue_request(None, false).contains("<undef />"));
        let poll = build_event_queue_request(Some(5), false);
        assert!(poll.contains("<key>ack</key><integer>5</integer>"));
        assert!(poll.contains("<key>done</key><boolean>0</boolean>"));
    }

    #[test]
    fn builds_fetch_inventory_request() -> Result<(), TestError> {
        let owner = "11111111-1111-1111-1111-111111111111".parse::<uuid::Uuid>()?;
        let folder = "22222222-2222-2222-2222-222222222222".parse::<uuid::Uuid>()?;
        let body =
            sl_wire::build_fetch_inventory_request(owner, &[InventoryFolderKey::from(folder)]);
        assert!(body.starts_with("<llsd><map><key>folders</key><array>"));
        assert!(
            body.contains("<key>folder_id</key><uuid>22222222-2222-2222-2222-222222222222</uuid>")
        );
        assert!(
            body.contains("<key>owner_id</key><uuid>11111111-1111-1111-1111-111111111111</uuid>")
        );
        assert!(body.contains("<key>fetch_items</key><boolean>1</boolean>"));
        // The parsed round-trip is well-formed LLSD.
        assert!(parse_llsd_xml(&body)?.get("folders").is_some());
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

    #[test]
    fn serialized_event_queue_response_re_parses() -> Result<(), TestError> {
        // The serializer must reproduce a structure the existing parsers read.
        let body = Llsd::Map(
            [("LocalID".to_owned(), Llsd::Integer(7))]
                .into_iter()
                .collect(),
        );
        let event = Llsd::Map(
            [
                ("message".to_owned(), Llsd::String("TestEvent".to_owned())),
                ("body".to_owned(), body),
            ]
            .into_iter()
            .collect(),
        );
        let response = Llsd::Map(
            [
                ("id".to_owned(), Llsd::Integer(99)),
                ("events".to_owned(), Llsd::Array(vec![event])),
            ]
            .into_iter()
            .collect(),
        );
        let parsed = parse_event_queue_response(&response.to_llsd_xml())?;
        assert_eq!(parsed.id, 99);
        let event = parsed.events.first().ok_or("expected one event")?;
        assert_eq!(event.message, "TestEvent");
        assert_eq!(event.body.get("LocalID").and_then(Llsd::as_i32), Some(7));
        Ok(())
    }
}
