//! The profile on disk: identity, name and friends, kept between runs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use chess_p2p::profile::{Profile, clean_name};
use iroh::SecretKey;

/// A fresh, empty profile directory for one test.
fn dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "chess-p2p-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn the_identity_survives_a_restart() {
    let dir = dir();
    let first = Profile::load_from(&dir).unwrap();
    let id = first.id();
    assert!(!first.is_guest());
    drop(first);

    let again = Profile::load_from(&dir).unwrap();
    assert_eq!(again.id(), id, "same key, so friends can find us again");
}

#[cfg(unix)]
#[test]
fn only_the_owner_can_read_the_key() {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir();
    Profile::load_from(&dir).unwrap();
    let mode = std::fs::metadata(dir.join("identity.key"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_second_copy_runs_as_a_guest() {
    let dir = dir();
    let first = Profile::load_from(&dir).unwrap();
    let second = Profile::load_from(&dir).unwrap();
    assert!(second.is_guest());
    assert_ne!(second.id(), first.id(), "two copies must not share a key");

    // Once the first lets go, the profile is free again.
    drop(second);
    let id = first.id();
    drop(first);
    assert_eq!(Profile::load_from(&dir).unwrap().id(), id);
}

#[test]
fn friends_are_remembered_newest_first() {
    let dir = dir();
    let mut profile = Profile::load_from(&dir).unwrap();
    let alice = SecretKey::generate().public();
    let bob = SecretKey::generate().public();

    profile.played(alice, "alice").unwrap();
    profile.played(bob, "bob").unwrap();
    // A rematch counts up, takes the name they go by now, and moves them up.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    profile.played(alice, "Alice B").unwrap();

    let names: Vec<_> = profile.contacts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Alice B", "bob"]);
    assert_eq!(profile.contact(alice).unwrap().games, 2);

    drop(profile);
    let mut profile = Profile::load_from(&dir).unwrap();
    assert_eq!(profile.contacts.len(), 2, "friends are saved");

    profile.forget(bob).unwrap();
    drop(profile);
    let profile = Profile::load_from(&dir).unwrap();
    assert!(profile.contact(bob).is_none());
    assert!(profile.contact(alice).is_some());
}

#[test]
fn a_new_name_is_kept() {
    let dir = dir();
    let mut profile = Profile::load_from(&dir).unwrap();
    profile.set_name("  Ace\tof   Spades ").unwrap();
    assert_eq!(profile.name, "Ace of Spades");
    assert!(profile.set_name(" - ").is_err(), "nothing to call anyone");
    drop(profile);
    assert_eq!(Profile::load_from(&dir).unwrap().name, "Ace of Spades");
}

#[test]
fn names_are_one_short_line() {
    assert_eq!(clean_name("bob\nsmith").as_deref(), Some("bob smith"));
    assert_eq!(clean_name("\u{1b}[31mred").as_deref(), Some("[31mred"));
    assert_eq!(clean_name(&"x".repeat(100)).unwrap().len(), 24);
    assert_eq!(clean_name("   "), None);
}

#[test]
fn a_guest_saves_nothing() {
    let mut guest = Profile::guest();
    guest
        .played(SecretKey::generate().public(), "alice")
        .unwrap();
    assert_eq!(guest.contacts.len(), 1, "remembered for this run");
}
