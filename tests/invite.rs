//! Friends inviting each other, with no code: iroh already knows who is who.

use std::time::Duration;

use chess_p2p::net::CHESS;
use chess_p2p::session::{self, Game, Incoming, Invite, Link, Listener};
use iroh::{Endpoint, SecretKey};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;

async fn next<T>(rx: &mut UnboundedReceiver<T>, what: &str) -> T {
    tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
        .unwrap_or_else(|| panic!("channel closed waiting for {what}"))
}

struct Player {
    endpoint: Endpoint,
    listener: Listener,
    heard: UnboundedReceiver<Incoming>,
}

async fn player() -> Player {
    let endpoint = session::bind(SecretKey::generate()).await.unwrap();
    endpoint.online().await;
    let (tx, heard) = unbounded_channel();
    let listener = Listener::start(endpoint.clone(), &[CHESS], tx);
    Player {
        endpoint,
        listener,
        heard,
    }
}

/// `from` invites `to`, dialling its address directly so the test does not
/// depend on discovery.
fn invite(from: &Player, to: &Player, game: Game) -> JoinHandle<anyhow::Result<Link>> {
    let endpoint = from.endpoint.clone();
    let addr = to.endpoint.addr();
    tokio::spawn(async move { session::invite(&endpoint, addr, game, "bob").await })
}

async fn invited(player: &mut Player) -> Invite {
    match next(&mut player.heard, "the invite").await {
        Incoming::Invite(invite) => invite,
        other => panic!("expected Invite, got {other:?}"),
    }
}

async fn outcome(task: JoinHandle<anyhow::Result<Link>>) -> anyhow::Result<Link> {
    tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .expect("the invite settles")
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_invite_pairs_both_sides() {
    let mut alice = player().await;
    alice.listener.idle();
    let bob = player().await;

    let asking = invite(&bob, &alice, CHESS);
    let invite = invited(&mut alice).await;
    assert_eq!(invite.peer, bob.endpoint.id());
    assert_eq!(invite.name, "bob");
    assert_eq!(invite.game, CHESS);

    alice.listener.busy();
    invite.accept("alice");
    let bobs = outcome(asking).await.expect("alice said yes");
    assert_eq!(bobs.peer, alice.endpoint.id());
    assert_eq!(bobs.peer_name, "alice");
    match next(&mut alice.heard, "alice's link").await {
        Incoming::Paired(link) => assert_eq!(link.peer, bob.endpoint.id()),
        other => panic!("expected Paired, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_invite_says_why() {
    let mut alice = player().await;
    alice.listener.idle();
    let bob = player().await;

    let asking = invite(&bob, &alice, CHESS);
    invited(&mut alice).await.refuse("declined");
    let err = outcome(asking).await.expect_err("alice said no");
    assert_eq!(session::explain(&err, "alice"), "alice said no");

    // What a player does with an invite from someone not in their friends.
    let asking = invite(&bob, &alice, CHESS);
    invited(&mut alice).await.refuse("unknown");
    let err = outcome(asking).await.expect_err("alice does not know bob");
    assert_eq!(
        session::explain(&err, "alice"),
        "alice no longer has you as a friend"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_in_a_game_is_busy() {
    let alice = player().await;
    alice.listener.busy();
    let bob = player().await;

    let err = outcome(invite(&bob, &alice, CHESS))
        .await
        .expect_err("alice is playing");
    assert_eq!(session::explain(&err, "alice"), "alice is busy right now");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invite_for_another_game_is_turned_away() {
    let alice = player().await;
    alice.listener.idle();
    let bob = player().await;
    let go = Game {
        name: "go",
        version: 1,
    };

    let err = outcome(invite(&bob, &alice, go))
        .await
        .expect_err("alice has no go");
    assert!(format!("{err:#}").contains("cannot play"), "{err:#}");
}

/// Giving up on an invite takes it off the other player's screen.
#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_invite_goes_away() {
    let mut alice = player().await;
    alice.listener.idle();
    let bob = player().await;

    let asking = invite(&bob, &alice, CHESS);
    let invite = invited(&mut alice).await;
    asking.abort();
    bob.endpoint.close().await;

    match next(&mut alice.heard, "the invite going").await {
        Incoming::InviteGone(peer) => assert_eq!(peer, bob.endpoint.id()),
        other => panic!("expected InviteGone, got {other:?}"),
    }
    drop(invite);
}
