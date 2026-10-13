//! Paths a game's files name, held to its folder.

use std::path::PathBuf;

use proton_game::path::inside;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-game-path-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("art")).unwrap();
    dir
}

#[test]
fn a_plain_path_to_a_games_file_is_inside() {
    let root = scratch("plain");
    std::fs::write(root.join("art/sky.png"), b"png").unwrap();
    assert!(inside(&root, "art/sky.png").unwrap().ends_with("art/sky.png"));
    // Not there yet, as a file about to be written is not.
    assert!(inside(&root, "rooms/new.room.json").is_ok());
    assert!(inside(&root, "art/Hero.PSD").is_ok(), "the kind is not case-sensitive");
}

#[test]
fn a_path_that_climbs_out_or_names_something_else_is_refused() {
    let root = scratch("refused");
    for rel in ["", "../x.png", "art/../../x.png", "/etc/passwd.json", "./art/sky.png", "art\\sky.png", "art/run.sh", "game"] {
        assert!(inside(&root, rel).is_err(), "{rel:?} was let through");
    }
}

#[cfg(unix)]
#[test]
fn a_link_that_leads_out_is_refused() {
    let root = scratch("links");
    let outside = scratch("links-outside");
    std::fs::write(outside.join("art/secret.png"), b"x").unwrap();
    std::os::unix::fs::symlink(outside.join("art"), root.join("borrowed")).unwrap();
    std::os::unix::fs::symlink(outside.join("art/secret.png"), root.join("art/linked.png")).unwrap();
    assert!(inside(&root, "borrowed/secret.png").is_err(), "a folder linked from outside was let through");
    assert!(inside(&root, "borrowed/new.png").is_err(), "a file to be written through a link out was let through");
    assert!(inside(&root, "art/linked.png").is_err(), "a file linked from outside was let through");
}
