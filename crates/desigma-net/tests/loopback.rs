//! In-process sessions over direct addresses: no relay, no discovery, no internet.
use desigma_net::{DisconnectReason, Guest, GuestEvent, GuestOptions, Host, HostEvent, HostOptions, JoinError, Network, Ticket};
use std::time::{Duration, Instant};

const T: Duration = Duration::from_secs(20);

fn host_with(f: impl FnOnce(&mut HostOptions)) -> (Host, Ticket) {
    let mut o = HostOptions { network: Network::Loopback, ..Default::default() };
    f(&mut o);
    let host = Host::start(o).unwrap();
    let ticket = host.wait_ticket(T).expect("host bound");
    (host, ticket)
}

fn gopts() -> GuestOptions {
    GuestOptions { network: Network::Loopback, ..Default::default() }
}

fn join(ticket: &Ticket, name: &str, pw: Option<&str>) -> Result<(Guest, u32), JoinError> {
    Guest::join_blocking(ticket, name, pw, gopts(), T)
}

/// Pull host events until `pick` matches, ignoring the rest.
fn host_wait<R>(host: &Host, mut pick: impl FnMut(HostEvent) -> Option<R>) -> R {
    let end = Instant::now() + T;
    loop {
        let left = end.checked_duration_since(Instant::now()).expect("host event timed out");
        if let Some(r) = host.recv_timeout(left).and_then(&mut pick) {
            return r;
        }
    }
}

fn guest_wait<R>(g: &Guest, mut pick: impl FnMut(GuestEvent) -> Option<R>) -> R {
    let end = Instant::now() + T;
    loop {
        let left = end.checked_duration_since(Instant::now()).expect("guest event timed out");
        if let Some(r) = g.recv_timeout(left).and_then(&mut pick) {
            return r;
        }
    }
}

fn host_msg(host: &Host) -> (u32, Vec<u8>) {
    host_wait(host, |e| match e {
        HostEvent::Message { peer, bytes } => Some((peer, bytes)),
        _ => None,
    })
}

fn guest_msg(g: &Guest) -> Vec<u8> {
    guest_wait(g, |e| match e {
        GuestEvent::Message(b) => Some(b),
        _ => None,
    })
}

#[test]
fn join_and_exchange_both_ways() {
    let (host, ticket) = host_with(|_| {});
    let (guest, peer) = join(&ticket, "Ann", None).unwrap();
    assert_eq!(peer, 1);
    let name = host_wait(&host, |e| match e {
        HostEvent::PeerJoined { peer: 1, name } => Some(name),
        _ => None,
    });
    assert_eq!(name, "Ann");
    assert_eq!(host.peers(), vec![1]);

    guest.send(b"hello host".to_vec()).unwrap();
    assert_eq!(host_msg(&host), (1, b"hello host".to_vec()));
    host.send(1, b"hello guest".to_vec()).unwrap();
    assert_eq!(guest_msg(&guest), b"hello guest");

    // Ordered delivery.
    for i in 0..50u8 {
        guest.send(vec![i]).unwrap();
    }
    for i in 0..50u8 {
        assert_eq!(host_msg(&host).1, vec![i]);
    }
    assert!(host.send(99, vec![]).is_err());
}

#[test]
fn three_guests_broadcast_except_sender() {
    let (host, ticket) = host_with(|_| {});
    let guests: Vec<Guest> = (1..=3u32)
        .map(|i| {
            let (g, peer) = join(&ticket, &format!("g{i}"), None).unwrap();
            assert_eq!(peer, i);
            g
        })
        .collect();
    assert_eq!(host.peers(), vec![1, 2, 3]);

    guests[0].send(b"from one".to_vec()).unwrap();
    let (from, bytes) = host_msg(&host);
    assert_eq!(from, 1);
    host.broadcast(bytes, Some(from));
    assert_eq!(guest_msg(&guests[1]), b"from one");
    assert_eq!(guest_msg(&guests[2]), b"from one");
    // The sender got nothing: its next message is the follow-up.
    host.send(1, b"direct".to_vec()).unwrap();
    assert_eq!(guest_msg(&guests[0]), b"direct");

    host.broadcast(b"all".to_vec(), None);
    for g in &guests {
        assert_eq!(guest_msg(g), b"all");
    }
}

#[test]
fn leaving_peers_are_reported_and_indices_not_reused() {
    let (host, ticket) = host_with(|_| {});
    let (g1, p1) = join(&ticket, "a", None).unwrap();
    assert_eq!(p1, 1);
    g1.leave();
    host_wait(&host, |e| matches!(e, HostEvent::PeerLeft { peer: 1 }).then_some(()));
    assert_eq!(
        guest_wait(&g1, |e| match e {
            GuestEvent::Disconnected(r) => Some(r),
            _ => None,
        }),
        DisconnectReason::Left
    );
    let (_g2, p2) = join(&ticket, "b", None).unwrap();
    assert_eq!(p2, 2);
}

#[test]
fn wrong_password_is_rejected_and_right_one_accepted() {
    let (host, ticket) = host_with(|o| o.password = Some("s3cret".into()));
    assert!(ticket.password_required);
    for bad in [None, Some("nope"), Some("s3cre")] {
        match join(&ticket, "Eve", bad) {
            Err(JoinError::WrongPassword) => {}
            Err(JoinError::Locked { .. }) => {}
            other => panic!("unexpected {:?}", other.map(|(_, p)| p)),
        }
    }
    let error = host_wait(&host, |e| match e {
        HostEvent::JoinRefused { error, .. } => Some(error),
        _ => None,
    });
    assert_eq!(error, JoinError::WrongPassword);
    assert!(host.peers().is_empty());
}

#[test]
fn repeated_wrong_guesses_never_get_in() {
    let (host, ticket) = host_with(|o| o.password = Some("s3cret".into()));
    // The per-endpoint lockout itself is unit-tested in auth.rs (each Guest has a fresh key).
    for _ in 0..3 {
        assert!(join(&ticket, "Eve", Some("x")).is_err());
    }
    assert!(host.peers().is_empty());
    let (_g, p) = join(&ticket, "Ann", Some("s3cret")).unwrap();
    assert_eq!(p, 1);
}

#[test]
fn version_mismatch_is_reported_to_both_sides() {
    let (host, ticket) = host_with(|_| {});
    let opts = GuestOptions { version: 999, ..gopts() };
    let err = Guest::join_blocking(&ticket, "Old", None, opts, T).err().unwrap();
    let want = JoinError::VersionMismatch { host: desigma_net::PROTOCOL_VERSION, guest: 999 };
    assert_eq!(err, want);
    assert!(err.to_string().contains("update desigma"));
    let seen = host_wait(&host, |e| match e {
        HostEvent::JoinRefused { error, .. } => Some(error),
        _ => None,
    });
    assert_eq!(seen, want);
}

#[test]
fn kick_disconnects_the_guest() {
    let (host, ticket) = host_with(|_| {});
    let (guest, peer) = join(&ticket, "Ann", None).unwrap();
    host_wait(&host, |e| matches!(e, HostEvent::PeerJoined { .. }).then_some(()));
    host.kick(peer);
    let reason = guest_wait(&guest, |e| match e {
        GuestEvent::Disconnected(r) => Some(r),
        _ => None,
    });
    assert_eq!(reason, DisconnectReason::Kicked);
    host_wait(&host, |e| matches!(e, HostEvent::PeerLeft { .. }).then_some(()));
    assert!(host.peers().is_empty());
}

#[test]
fn host_shutdown_disconnects_guests() {
    let (host, ticket) = host_with(|_| {});
    let (a, _) = join(&ticket, "a", None).unwrap();
    let (b, _) = join(&ticket, "b", None).unwrap();
    host.shutdown();
    for g in [&a, &b] {
        let reason = guest_wait(g, |e| match e {
            GuestEvent::Disconnected(r) => Some(r),
            _ => None,
        });
        assert_eq!(reason, DisconnectReason::HostEnded);
    }
}

#[test]
fn approval_accept_and_reject() {
    let (host, ticket) = host_with(|o| o.require_approval = true);

    let t2 = ticket.clone();
    let rejected = std::thread::spawn(move || join(&t2, "Mallory", None).err());
    let (request, name) = host_wait(&host, |e| match e {
        HostEvent::JoinRequest { request, name, .. } => Some((request, name)),
        _ => None,
    });
    assert_eq!(name, "Mallory");
    host.respond(request, false);
    assert_eq!(rejected.join().unwrap(), Some(JoinError::Denied));
    assert!(host.peers().is_empty());

    let t3 = ticket.clone();
    let accepted = std::thread::spawn(move || join(&t3, "Ann", None));
    let request = host_wait(&host, |e| match e {
        HostEvent::JoinRequest { request, .. } => Some(request),
        _ => None,
    });
    host.respond(request, true);
    let (guest, peer) = accepted.join().unwrap().unwrap();
    // The rejected guest never consumed an index.
    assert_eq!(peer, 1);
    host.send(peer, b"welcome".to_vec()).unwrap();
    assert_eq!(guest_msg(&guest), b"welcome");
}

#[test]
fn large_frames_both_ways() {
    let (host, ticket) = host_with(|_| {});
    let (guest, peer) = join(&ticket, "Ann", None).unwrap();
    // Mix of compressible and incompressible bytes.
    let mut big = Vec::with_capacity(20 << 20);
    let mut x = 0x9E37_79B9u32;
    while big.len() < 20 << 20 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        big.push(if big.len() % 3 == 0 { (x >> 24) as u8 } else { b'a' });
    }
    guest.send(big.clone()).unwrap();
    let (from, got) = host_msg(&host);
    assert_eq!(from, peer);
    assert!(got == big);
    host.send(peer, big.clone()).unwrap();
    assert!(guest_msg(&guest) == big);
}

#[test]
fn presence_travels_on_its_own_lane() {
    let (host, ticket) = host_with(|_| {});
    let (guest, peer) = join(&ticket, "Ann", None).unwrap();
    host_wait(&host, |e| matches!(e, HostEvent::PeerJoined { .. }).then_some(()));
    // Datagrams are unreliable: resend until one lands.
    let got = {
        let end = Instant::now() + T;
        let mut got = None;
        while got.is_none() && Instant::now() < end {
            guest.send_presence(b"cursor 1,2").unwrap();
            while let Some(e) = host.recv_timeout(Duration::from_millis(100)) {
                if let HostEvent::Presence { peer, bytes } = e {
                    got = Some((peer, bytes));
                    break;
                }
            }
        }
        got.expect("presence arrived")
    };
    assert_eq!(got, (peer, b"cursor 1,2".to_vec()));

    let end = Instant::now() + T;
    let mut back = None;
    while back.is_none() && Instant::now() < end {
        host.broadcast_presence(b"host cursor", None);
        while let Some(e) = guest.recv_timeout(Duration::from_millis(100)) {
            if let GuestEvent::Presence(b) = e {
                back = Some(b);
                break;
            }
        }
    }
    assert_eq!(back.unwrap(), b"host cursor");
    // Too big for a datagram is refused locally, not queued.
    assert!(guest.send_presence(&vec![0u8; 100_000]).is_err());
}

#[test]
fn wake_callback_fires() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let (host, ticket) = host_with(|_| {});
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    host.set_wake(Box::new(move || {
        h.fetch_add(1, Ordering::SeqCst);
    }));
    let (_g, _) = join(&ticket, "Ann", None).unwrap();
    let end = Instant::now() + T;
    while hits.load(Ordering::SeqCst) == 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(hits.load(Ordering::SeqCst) > 0);
    assert!(!host.drain().is_empty());
}

#[test]
fn ticket_survives_a_link_round_trip() {
    let (_host, ticket) = host_with(|o| o.password = Some("x".into()));
    let link = ticket.to_link();
    assert!(link.starts_with("desigma://join/"));
    let parsed = Ticket::parse(&format!("  {link}\n")).unwrap();
    assert_eq!(parsed, ticket);
    assert!(parsed.password_required);
}

#[test]
fn a_severed_guest_comes_back_with_its_secret_without_approval() {
    let (host, ticket) = host_with(|o| o.require_approval = true);
    let t2 = ticket.clone();
    let first = std::thread::spawn(move || Guest::join_resume(&t2, "Ann", None, gopts(), None).unwrap());
    let request = host_wait(&host, |e| match e {
        HostEvent::JoinRequest { request, .. } => Some(request),
        _ => None,
    });
    host.respond(request, true);
    let guest = first.join().unwrap();
    let peer = guest.wait_joined(T).unwrap();
    host_wait(&host, |e| matches!(e, HostEvent::PeerJoined { .. }).then_some(()));
    assert_eq!(host.join_token(peer), None, "a first visit shows no secret");
    host.allow_resume("sesame");

    // the network fails: the guest sees a lost connection, not a kick
    host.sever(peer);
    let reason = guest_wait(&guest, |e| match e {
        GuestEvent::Disconnected(r) => Some(r),
        _ => None,
    });
    assert!(matches!(reason, DisconnectReason::Lost(_)), "{reason:?}");
    host_wait(&host, |e| matches!(e, HostEvent::PeerLeft { .. }).then_some(()));

    // it dials again with the secret and is let in at once
    let back = Guest::join_resume(&ticket, "Ann", None, gopts(), Some("sesame".into())).unwrap();
    let again = back.wait_joined(T).unwrap();
    let new_peer = host_wait(&host, |e| match e {
        HostEvent::PeerJoined { peer, .. } => Some(peer),
        HostEvent::JoinRequest { .. } => panic!("a returning guest is not asked about again"),
        _ => None,
    });
    assert_eq!(new_peer, again);
    assert_eq!(host.join_token(new_peer).as_deref(), Some("sesame"));
    assert_eq!(host.join_token(new_peer), None, "handed out once");
    host.send(new_peer, b"hi again".to_vec()).unwrap();
    assert_eq!(guest_msg(&back), b"hi again");

    // an unknown secret still goes through approval
    let t3 = ticket.clone();
    let stranger = std::thread::spawn(move || Guest::join_resume(&t3, "Eve", None, gopts(), Some("guess".into())).unwrap());
    let request = host_wait(&host, |e| match e {
        HostEvent::JoinRequest { request, .. } => Some(request),
        _ => None,
    });
    host.respond(request, false);
    assert_eq!(stranger.join().unwrap().wait_joined(T), Err(JoinError::Denied));
}
