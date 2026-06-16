//! Tests for the LLSD-XML parser and the capability request/response helpers.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_wire::{
        Llsd, build_event_queue_request, build_seed_request, parse_event_queue_response,
        parse_llsd_xml, parse_seed_response,
    };

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
    fn malformed_xml_is_an_error() {
        assert!(
            parse_llsd_xml("<llsd><map>").err().is_some(),
            "expected a parse error"
        );
    }
}
