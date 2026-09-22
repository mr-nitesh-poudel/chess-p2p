//! Runs both halves of a real connection in one process.

use std::time::Duration;

use chess_p2p::net::{self, NetEvent, Out};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// Wait for the next event, failing loudly rather than hanging the suite.
async fn next(rx: &mut UnboundedReceiver<NetEvent>, what: &str) -> NetEvent {
    tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
        .unwrap_or_else(|| panic!("channel closed waiting for {what}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn two_peers_exchange_a_game() {
    let (host_tx, mut host_rx) = unbounded_channel();
    let (join_tx, mut join_rx) = unbounded_channel();

    let host = net::host(host_tx).await.expect("host binds");
    assert!(matches!(
        next(&mut host_rx, "host online").await,
        NetEvent::Online
    ));

    // Dial the full address so the test does not depend on DNS discovery.
    let guest = net::join(host.addr(), join_tx).await.expect("guest binds");
    assert!(matches!(
        next(&mut join_rx, "guest online").await,
        NetEvent::Online
    ));

    match next(&mut host_rx, "host connected").await {
        NetEvent::Connected(id) => assert_eq!(id, guest.addr().id),
        other => panic!("expected Connected, got {other:?}"),
    }
    match next(&mut join_rx, "guest connected").await {
        NetEvent::Connected(id) => assert_eq!(id, host.addr().id),
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
