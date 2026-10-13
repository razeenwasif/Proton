//! A new game: the same bytes every time, art included — and the UI tests'
//! fixture is exactly it.
//!
//! When the template changes on purpose, write the fixture again with
//!
//! ```bash
//! PROTON_BLESS=1 cargo test -p proton-game --test template
//! ```

use std::path::PathBuf;

use proton_game::template;

/// The fixture the browser tests open, at the repository's root.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/ui/fixtures/game")
}

#[test]
fn the_template_is_the_same_bytes_every_time() {
    let a = template::files("metroidvania", "Crossroads").unwrap();
    let b = template::files("metroidvania", "Crossroads").unwrap();
    assert_eq!(a, b);
    assert!(template::files("nonsense", "x").is_err());
}

#[test]
fn every_image_is_a_png_of_the_size_its_planes_expect() {
    let files = template::files("metroidvania", "Crossroads").unwrap();
    let size = |rel: &str| {
        let file = files.iter().find(|f| f.rel == rel).unwrap_or_else(|| panic!("no {rel}"));
        let decoder = png::Decoder::new(file.bytes.as_slice());
        let reader = decoder.read_info().unwrap();
        let info = reader.info();
        (info.width, info.height)
    };
    assert_eq!(size("art/sky.png"), (480, 270));
    assert_eq!(size("art/ground.png"), (512, 128));
    assert_eq!(size("art/player.png"), (48, 80));
    // And every image a room names is one of the game's.
    for file in files.iter().filter(|f| f.rel.ends_with(".room.json")) {
        let text = String::from_utf8(file.bytes.clone()).unwrap();
        for image in text.split("\"image\": \"").skip(1).map(|rest| &rest[..rest.find('"').unwrap()]) {
            assert!(files.iter().any(|f| f.rel == image), "{} names {image}, which the template does not make", file.rel);
        }
    }
}

#[test]
fn a_new_game_is_not_written_over_an_old_one() {
    let dir = std::env::temp_dir().join(format!("proton-game-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = template::files("metroidvania", "Crossroads").unwrap();
    template::write(&dir, &files).unwrap();
    let why = template::write(&dir, &files).unwrap_err();
    assert!(why.contains("already holds a game"), "{why}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_ui_fixture_is_the_template() {
    let files = template::files("metroidvania", "Crossroads").unwrap();
    let root = fixture();
    if std::env::var_os("PROTON_BLESS").is_some() {
        let _ = std::fs::remove_dir_all(&root);
        template::write(&root, &files).unwrap();
    }
    for file in &files {
        let held = std::fs::read(root.join(&file.rel)).unwrap_or_else(|e| panic!("{}: {e} — write it with PROTON_BLESS=1", file.rel));
        assert!(held == file.bytes, "{} differs from the template — write it again with PROTON_BLESS=1", file.rel);
    }
}
