//! Runs both halves of a real connection in one process.
//!
//! These dial the host's address directly, so they do not depend on the DHT;
//! the code still has to match for the handshake to let anyone in.

use std::time::Duration;

use chess_p2p::net::{self, Net, NetEvent, Out};
use chess_p2p::session::{Code, MAX_WRONG_CODES, Progress, Target};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// Wait for the next event, failing loudly rather than hanging the suite.
async fn next(rx: &mut UnboundedReceiver<NetEvent>, what: &str) -> NetEvent {
    tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
        .unwrap_or_else(|| panic!("channel closed waiting for {what}"))
}

/// A host that is online and not on the DHT.
async fn host(code: Code) -> (Net, UnboundedReceiver<NetEvent>) {
    let (tx, rx) = unbounded_channel();
    let host = net::host(code, false, tx).await.expect("host binds");
    host.online().await;
    (host, rx)
}

async fn dial(host: &Net, code: Code) -> (Net, UnboundedReceiver<NetEvent>) {
    let (tx, rx) = unbounded_channel();
    let guest = net::join(code, Target::Addr(host.addr()), tx)
        .await
        .expect("guest binds");
    (guest, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn two_peers_exchange_a_game() {
    let code = Code::generate();
    let (host, mut host_rx) = host(code).await;
    let (guest, mut join_rx) = dial(&host, code).await;

    assert!(matches!(
        next(&mut join_rx, "guest finds host").await,
        NetEvent::Progress(Progress::Found)
    ));
    match next(&mut host_rx, "host connected").await {
        NetEvent::Connected(id) => assert_eq!(id, guest.id),
        other => panic!("expected Connected, got {other:?}"),
    }
    match next(&mut join_rx, "guest connected").await {
        NetEvent::Connected(id) => assert_eq!(id, host.id),
        other => panic!("expected Connected, got {other:?}"),
    }

    host.out.send(Out::Move("e2e4".into())).unwrap();
    match next(&mut join_rx, "white's move").await {
        NetEvent::Move(uci) => assert_eq!(uci, "e2e4"),
        other => panic!("expected Move, got {other:?}"),
    }

    guest.out.send(Out::Move("e7e5".into())).unwrap();
    match next(&mut host_rx, "black's reply").await {
        NetEvent::Move(uci) => assert_eq!(uci, "e7e5"),
        other => panic!("expected Move, got {other:?}"),
    }

    guest.out.send(Out::Resign).unwrap();
    assert!(matches!(
        next(&mut host_rx, "resignation").await,
        NetEvent::Resign
    ));

    // Dropping the guest's connection must surface on the host, not hang it.
    guest.shutdown().await;
    match next(&mut host_rx, "disconnect").await {
        NetEvent::Disconnected(_) => {}
        other => panic!("expected Disconnected, got {other:?}"),
    }
    host.shutdown().await;
}

/// A wrong code is turned away, and the host keeps waiting for the right one.
#[tokio::test(flavor = "multi_thread")]
async fn wrong_code_is_refused() {
    let code = Code::generate();
    let (host, mut host_rx) = host(code).await;

    let (intruder, mut intruder_rx) = dial(&host, Code::generate()).await;
    next(&mut intruder_rx, "intruder finds host").await;
    match next(&mut intruder_rx, "intruder refused").await {
        NetEvent::Disconnected(why) => assert!(why.contains("did not match"), "{why}"),
        other => panic!("expected Disconnected, got {other:?}"),
    }
    assert!(matches!(
        next(&mut host_rx, "host notices").await,
        NetEvent::Progress(Progress::WrongCode { attempts: 1 })
    ));
    intruder.shutdown().await;

    let (guest, mut join_rx) = dial(&host, code).await;
    next(&mut join_rx, "guest finds host").await;
    match next(&mut host_rx, "host connected").await {
        NetEvent::Connected(id) => assert_eq!(id, guest.id),
        other => panic!("expected Connected, got {other:?}"),
    }
    guest.shutdown().await;
    host.shutdown().await;
}

/// Each wrong code is a guess, so the host only allows a few.
#[tokio::test(flavor = "multi_thread")]
async fn host_stops_after_too_many_wrong_codes() {
    let (host, mut host_rx) = host(Code::generate()).await;

    for attempt in 1..=MAX_WRONG_CODES {
        let (intruder, mut rx) = dial(&host, Code::generate()).await;
        next(&mut rx, "intruder finds host").await;
        next(&mut rx, "intruder refused").await;
        match next(&mut host_rx, "host notices").await {
            NetEvent::Progress(Progress::WrongCode { attempts }) => assert_eq!(attempts, attempt),
            other => panic!("expected WrongCode, got {other:?}"),
        }
        intruder.shutdown().await;
    }
    match next(&mut host_rx, "host gives up").await {
        NetEvent::Disconnected(why) => assert!(why.contains("too many wrong codes"), "{why}"),
        other => panic!("expected Disconnected, got {other:?}"),
    }
    host.shutdown().await;
}

/// The whole path a player takes: publish, look the code up, pair. Needs the
/// internet; run it with `cargo test -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn pair_by_code_over_the_dht() {
    let code = Code::generate();
    let (host_tx, mut host_rx) = unbounded_channel();
    let host = net::host(code, true, host_tx).await.unwrap();
    match next(&mut host_rx, "code published").await {
        NetEvent::Progress(Progress::Listed) => {}
        other => panic!("expected Listed, got {other:?}"),
    }

    let (join_tx, mut join_rx) = unbounded_channel();
    let guest = net::join(code, Target::Lookup, join_tx).await.unwrap();
    assert!(matches!(
        next(&mut join_rx, "code looked up").await,
        NetEvent::Progress(Progress::Found)
    ));
    match next(&mut join_rx, "guest connected").await {
        NetEvent::Connected(id) => assert_eq!(id, host.id),
        other => panic!("expected Connected, got {other:?}"),
    }
    guest.shutdown().await;
    host.shutdown().await;
}
