//! In-memory loopback tests of the server-side CAPS core: the client's own
//! seed/event-queue builders and parsers driven against [`SimCaps::dispatch`]
//! and a [`SimSession`]'s event buffer — the CAPS mirror of the UDP loopback
//! in `tests/sim_session.rs`, with no HTTP transport involved.

#[cfg(test)]
mod test {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_proto::{
        CapsDispatch, CapsRequest, LLSD_XML_CONTENT_TYPE, REQUESTED_CAPABILITIES, RegionHandle,
        SimCaps, SimSession, build_event_queue_request, build_seed_request,
        enable_simulator_to_caps_llsd, parse_event_queue_response, parse_seed_response,
    };

    /// A boxed test error.
    type TestError = Box<dyn Error>;

    /// The region handle the simulator serves throughout these tests.
    const REGION_HANDLE: u64 = 0x0000_03e8_0000_03e8;

    /// The simulator's UDP address (for event bodies that carry one).
    fn sim_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000)
    }

    /// A fresh simulator session (the event-queue buffer needs no circuit).
    fn new_sim() -> SimSession {
        SimSession::new(RegionHandle(REGION_HANDLE), Instant::now())
    }

    /// A [`SimCaps`] with a deterministic token mint.
    fn new_caps() -> Result<SimCaps, TestError> {
        let base: url::Url = "http://127.0.0.1:9001/".parse()?;
        let mut next: u128 = 0;
        let mint = move || {
            next = next.wrapping_add(1);
            uuid::Uuid::from_u128(next)
        };
        Ok(SimCaps::new(base, uuid::Uuid::from_u128(0x5eed), mint))
    }

    /// A `POST` [`CapsRequest`] carrying an LLSD-XML body.
    fn post<'a>(path: &'a str, body: &'a str) -> CapsRequest<'a> {
        CapsRequest {
            method: "POST",
            path,
            query: None,
            body: body.as_bytes(),
        }
    }

    /// Dispatches and unwraps an immediate response, failing on would-block.
    fn respond(
        caps: &mut SimCaps,
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> Result<(u16, String), TestError> {
        match caps.dispatch(sim, request) {
            CapsDispatch::Response(response) => {
                Ok((response.status, String::from_utf8(response.body.clone())?))
            }
            CapsDispatch::EventQueueWouldBlock => Err("unexpected would-block".into()),
        }
    }

    #[test]
    fn seed_round_trips_against_the_client_builders() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();

        // The exact request the client runtime POSTs to the seed URL.
        let request_body = build_seed_request(REQUESTED_CAPABILITIES);
        let seed_path = caps.seed_url().path().to_owned();
        let (status, body) = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(status, 200);

        // The client's own parser reads the grant; only the served
        // capability comes back, with the URL `grant` mints.
        let granted = parse_seed_response(&body)?;
        let expected = caps.grant(&["EventQueueGet".to_owned()]);
        assert_eq!(granted, expected);
        assert_eq!(granted.len(), 1);
        Ok(())
    }

    #[test]
    fn seed_grant_is_idempotent_across_retries() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let request_body = build_seed_request(REQUESTED_CAPABILITIES);
        let seed_path = caps.seed_url().path().to_owned();
        let first = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        // The reference viewer retries the seed POST up to 30 times; every
        // retry must receive a byte-identical grant.
        let second = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn unsupported_caps_are_omitted_from_the_grant() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let request_body = build_seed_request(&["EventQueueGet", "GetTexture", "NoSuchCapability"]);
        let seed_path = caps.seed_url().path().to_owned();
        let (status, body) = respond(&mut caps, &mut sim, &post(&seed_path, &request_body))?;
        assert_eq!(status, 200);
        let granted = parse_seed_response(&body)?;
        assert!(granted.contains_key("EventQueueGet"));
        assert!(!granted.contains_key("GetTexture"));
        assert!(!granted.contains_key("NoSuchCapability"));
        Ok(())
    }

    /// Grants the event queue and returns its URL path.
    fn granted_event_queue_path(caps: &SimCaps) -> Result<String, TestError> {
        let granted = caps.grant(&["EventQueueGet".to_owned()]);
        let url: url::Url = granted
            .get("EventQueueGet")
            .ok_or("EventQueueGet not granted")?
            .parse()?;
        Ok(url.path().to_owned())
    }

    #[test]
    fn event_queue_full_poll_cycle() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let eq_path = granted_event_queue_path(&caps)?;

        // A first poll (ack undef) with one queued event delivers batch 1.
        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr()),
        );
        let poll = build_event_queue_request(None, false);
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 200);
        let batch = parse_event_queue_response(&body)?;
        assert_eq!(batch.id, 1);
        assert_eq!(
            batch.events.first().map(|event| event.message.as_str()),
            Some("EnableSimulator")
        );

        // The client re-polls acking batch 1; nothing is queued, so the
        // long-poll would block (the runtime holds it open).
        let ack_poll = build_event_queue_request(Some(batch.id), false);
        assert_eq!(
            caps.dispatch(&mut sim, &post(&eq_path, &ack_poll)),
            CapsDispatch::EventQueueWouldBlock
        );

        // A later event releases the next poll as batch 2.
        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr()),
        );
        let (status, body) = respond(&mut caps, &mut sim, &post(&eq_path, &ack_poll))?;
        assert_eq!(status, 200);
        assert_eq!(parse_event_queue_response(&body)?.id, 2);
        Ok(())
    }

    #[test]
    fn empty_poll_would_block_and_times_out_as_502() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        assert_eq!(
            caps.dispatch(&mut sim, &post(&eq_path, &poll)),
            CapsDispatch::EventQueueWouldBlock
        );
        // The runtime's hold expires: the 502 is what the reference viewer
        // treats as "nothing yet, re-poll".
        assert_eq!(caps.event_queue_timeout().status, 502);
        Ok(())
    }

    #[test]
    fn done_poll_tears_the_queue_down() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let eq_path = granted_event_queue_path(&caps)?;

        let teardown = build_event_queue_request(Some(1), true);
        let (status, _) = respond(&mut caps, &mut sim, &post(&eq_path, &teardown))?;
        assert_eq!(status, 200);

        // Every later poll answers 404 — the client's "stop polling" signal
        // — even with events queued.
        sim.enqueue_caps_event(
            "EnableSimulator",
            enable_simulator_to_caps_llsd(REGION_HANDLE, sim_addr()),
        );
        let poll = build_event_queue_request(None, false);
        let (status, _) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 404);
        Ok(())
    }

    #[test]
    fn closed_session_polls_are_gone() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let now = Instant::now();
        let mut sim = SimSession::new(RegionHandle(REGION_HANDLE), now);
        // Let the inactivity timeout close the session (45 s in SimSession).
        let later = now
            .checked_add(Duration::from_secs(60))
            .ok_or("clock overflow")?;
        sim.handle_timeout(later);
        assert!(sim.is_closed());

        let eq_path = granted_event_queue_path(&caps)?;
        let poll = build_event_queue_request(None, false);
        let (status, _) = respond(&mut caps, &mut sim, &post(&eq_path, &poll))?;
        assert_eq!(status, 404);
        Ok(())
    }

    #[test]
    fn wrong_method_and_unknown_paths_are_rejected() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let seed_path = caps.seed_url().path().to_owned();

        // GET on the seed: known URL, wrong method.
        let get = CapsRequest {
            method: "GET",
            path: &seed_path,
            query: None,
            body: b"",
        };
        let (status, _) = respond(&mut caps, &mut sim, &get)?;
        assert_eq!(status, 405);

        // An unminted token and a non-capability path: not found.
        let unknown = post("/cap/00000000-0000-0000-0000-0000000000ff", "");
        let (status, _) = respond(&mut caps, &mut sim, &unknown)?;
        assert_eq!(status, 404);
        let elsewhere = post("/somewhere/else", "");
        let (status, _) = respond(&mut caps, &mut sim, &elsewhere)?;
        assert_eq!(status, 404);

        // A seed body that is not LLSD-XML: bad request.
        let (status, _) = respond(&mut caps, &mut sim, &post(&seed_path, "not xml <"))?;
        assert_eq!(status, 400);
        Ok(())
    }

    #[test]
    fn responses_carry_the_llsd_content_type() -> Result<(), TestError> {
        let mut caps = new_caps()?;
        let mut sim = new_sim();
        let request_body = build_seed_request(REQUESTED_CAPABILITIES);
        let seed_path = caps.seed_url().path().to_owned();
        match caps.dispatch(&mut sim, &post(&seed_path, &request_body)) {
            CapsDispatch::Response(response) => {
                assert_eq!(response.content_type, LLSD_XML_CONTENT_TYPE);
            }
            CapsDispatch::EventQueueWouldBlock => return Err("unexpected would-block".into()),
        }
        Ok(())
    }
}
