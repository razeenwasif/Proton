//! Canvas mode through the seam the window uses: the home page, a canvas
//! made and opened, edits answered with a hash and held until saved, undo
//! and redo pushed as tiles, saves written on a worker, and the quit prompt.

use std::path::PathBuf;

use proton_core::bus::{Event, NoticeLevel};
use proton_core::raster::RasterView;
use proton_core::{Command, ModeId, Workspace};
use proton_raster::{Canvas, Op, Preset};

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-core-raster-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn view(events: &[Event]) -> Option<Option<RasterView>> {
    events.iter().rev().find_map(|e| match e {
        Event::RasterCanvas { canvas } => Some(canvas.clone()),
        _ => None,
    })
}

fn home(events: &[Event]) -> Option<Vec<String>> {
    events.iter().find_map(|e| match e {
        Event::RasterHome { canvases, .. } => Some(canvases.iter().map(|c| format!("{} {:?} {}×{}", c.path, c.preset, c.size[0], c.size[1])).collect()),
        _ => None,
    })
}

fn warned(events: &[Event]) -> Option<String> {
    events.iter().find_map(|e| match e {
        Event::Notice { text, level: NoticeLevel::Warn | NoticeLevel::Error } => Some(text.clone()),
        _ => None,
    })
}

fn applied(events: &[Event]) -> (u32, String, Option<String>, bool) {
    events
        .iter()
        .find_map(|e| match e {
            Event::RasterApplied { seq, hash, refused, dirty, .. } => Some((*seq, hash.clone(), refused.clone(), *dirty)),
            _ => None,
        })
        .expect("an answer")
}

/// A project in Canvas mode with a 64 × 64 pixel canvas made and open.
fn with_canvas(tag: &str) -> (Workspace, PathBuf, RasterView) {
    let root = project(tag);
    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode { mode: ModeId::Raster });
    let events = space.apply(Command::RasterNew { name: "Hero Sprite".into(), preset: Preset::Pixel, width: 64, height: 64, folder: None });
    assert!(warned(&events).is_none(), "{events:?}");
    let shown = view(&events).unwrap().expect("the canvas made is open");
    (space, root, shown)
}

fn fill(generation: u32, seq: u32, layer: u32, rect: [i32; 4], color: [u8; 4]) -> Command {
    Command::RasterEdit { generation, seq, op: Op::FillRect { layer, rect, color }, label: "fill".into() }
}

#[test]
fn a_new_canvas_is_written_and_opened_and_the_home_page_lists_it() {
    let (mut space, root, shown) = with_canvas("new");
    assert_eq!(shown.path, "hero-sprite.canvas");
    assert_eq!((shown.preset, shown.width, shown.height), (Preset::Pixel, 64, 64));
    assert_eq!(shown.layers.len(), 1);
    assert!(!shown.dirty && shown.tiles.iter().all(|(_, t)| t.is_empty()));
    assert!(root.join("hero-sprite.canvas/canvas.json").exists());
    let events = space.apply(Command::Sync);
    assert_eq!(home(&events).unwrap(), ["hero-sprite.canvas Pixel 64×64"]);
    // Not over one already there, not outside the project, not too large.
    let again = space.apply(Command::RasterClose { discard: false });
    assert!(view(&again).unwrap().is_none());
    assert!(warned(&space.apply(Command::RasterNew { name: "Hero Sprite".into(), preset: Preset::Pixel, width: 8, height: 8, folder: None })).unwrap().contains("already there"));
    assert!(warned(&space.apply(Command::RasterNew { name: "x".into(), preset: Preset::Pixel, width: 8, height: 8, folder: Some("../x.canvas".into()) })).is_some());
    assert!(warned(&space.apply(Command::RasterNew { name: "big".into(), preset: Preset::Pixel, width: 2048, height: 8, folder: None })).unwrap().contains("at most 1024"));
}

#[test]
fn an_edit_is_answered_with_the_hash_the_window_made_and_held_until_saved() {
    let (mut space, root, shown) = with_canvas("edit");
    let layer = shown.layers[0].id;
    let manifest = std::fs::read(root.join("hero-sprite.canvas/layers/1.png")).unwrap();
    let events = space.apply(fill(shown.generation, 1, layer, [4, 4, 10, 10], [255, 0, 77, 255]));
    let (seq, hash, refused, dirty) = applied(&events);
    // The window's copy, made by the same engine, hashes the same.
    let mut copy = Canvas::new(64, 64, Preset::Pixel).unwrap();
    let (done, _) = copy.apply(&Op::FillRect { layer, rect: [4, 4, 10, 10], color: [255, 0, 77, 255] }).unwrap();
    assert_eq!((seq, refused, dirty), (1, None, true));
    assert_eq!(hash, format!("{:016x}", copy.hash_touched(&done.touched)));
    assert_eq!(std::fs::read(root.join("hero-sprite.canvas/layers/1.png")).unwrap(), manifest, "written before the person saved");
    assert_eq!(space.unsaved(), ["hero-sprite.canvas"], "the quit prompt does not name the canvas");
    // A layer added is a change to the layers: the canvas is sent again.
    let added = space.apply(Command::RasterEdit { generation: shown.generation, seq: 2, op: Op::AddLayer { name: "Ink".into(), at: None }, label: "add".into() });
    assert_eq!(view(&added).unwrap().unwrap().layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Sprite", "Ink"]);
    // Refused: said, answered, nothing changed.
    let refused = space.apply(Command::RasterEdit { generation: shown.generation, seq: 3, op: Op::RemoveLayer { layer: 99 }, label: "remove".into() });
    assert!(applied(&refused).2.is_some() && warned(&refused).is_some());
    // For a canvas since closed: answered, and not applied.
    let stale = space.apply(fill(shown.generation + 7, 4, layer, [0, 0, 64, 64], [0, 0, 0, 255]));
    assert!(applied(&stale).2.unwrap().contains("no longer open"));
}

#[test]
fn undo_and_redo_send_the_tiles_they_changed() {
    let (mut space, _root, shown) = with_canvas("undo");
    let layer = shown.layers[0].id;
    space.apply(fill(shown.generation, 1, layer, [0, 0, 10, 10], [1, 2, 3, 255]));
    let undone = space.apply(Command::RasterUndo);
    let changed = undone
        .iter()
        .find_map(|e| match e {
            Event::RasterChanged { tiles, meta, .. } => Some((tiles.clone(), *meta)),
            _ => None,
        })
        .expect("the tiles undo changed");
    assert_eq!(changed, (vec![(layer, vec![0])], false));
    let after = view(&undone).unwrap().unwrap();
    assert!(!after.dirty && !after.can_undo && after.can_redo && after.redo.as_deref() == Some("fill"));
    let redone = space.apply(Command::RasterRedo);
    let back = view(&redone).unwrap().unwrap();
    assert!(back.dirty && back.tiles == vec![(layer, vec![0])]);
    // The tiles themselves, as the shell reads them for the window.
    // A 64 × 64 canvas is one tile: one past it is not a tile at all.
    let tiles = space.raster_tiles(shown.generation, layer, &[0, 1]).unwrap();
    assert_eq!(tiles.tiles.len(), 1);
    assert_eq!(tiles.tiles[0].1.as_ref().unwrap().get(9, 9), [1, 2, 3, 255]);
    space.apply(Command::RasterUndo);
    assert!(space.raster_tiles(shown.generation, layer, &[0]).unwrap().tiles[0].1.is_none(), "a tile cleared by undo is sent as clear");
    assert!(space.raster_tiles(shown.generation + 1, layer, &[0]).is_none(), "tiles of a canvas since closed");
}

#[test]
fn a_save_is_written_on_a_worker_and_said_when_it_is_done() {
    let (mut space, root, shown) = with_canvas("save");
    let layer = shown.layers[0].id;
    space.apply(fill(shown.generation, 1, layer, [0, 0, 64, 32], [9, 99, 199, 255]));
    let started = space.apply(Command::RasterSave);
    assert!(view(&started).unwrap().unwrap().saving);
    let mut said = None;
    for _ in 0..200 {
        let events = space.apply(Command::Poll);
        if let Some(v) = view(&events) {
            said = Some((v.unwrap(), events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == "saved hero-sprite"))));
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (after, noticed) = said.expect("the save never finished");
    assert!(!after.dirty && !after.saving && noticed);
    assert!(space.unsaved().is_empty());
    let read = proton_raster::file::load(&root.join("hero-sprite.canvas")).unwrap();
    assert_eq!(read.layers[0].grid.pixel(63, 31), [9, 99, 199, 255]);
}

#[test]
fn quitting_saves_what_is_unsaved_at_once() {
    let (mut space, root, shown) = with_canvas("quit");
    let layer = shown.layers[0].id;
    space.apply(fill(shown.generation, 1, layer, [1, 1, 1, 1], [200, 10, 10, 255]));
    // A save already writing is waited for, then the newest state written.
    space.apply(Command::RasterSave);
    space.apply(fill(shown.generation, 2, layer, [2, 2, 1, 1], [10, 200, 10, 255]));
    assert!(space.save_unsaved().is_empty());
    assert!(space.unsaved().is_empty());
    let read = proton_raster::file::load(&root.join("hero-sprite.canvas")).unwrap();
    assert_eq!((read.layers[0].grid.pixel(1, 1), read.layers[0].grid.pixel(2, 2)), ([200, 10, 10, 255], [10, 200, 10, 255]));
}

#[test]
fn a_canvas_with_changes_is_not_closed_or_left_for_another() {
    let (mut space, _root, shown) = with_canvas("guard");
    space.apply(fill(shown.generation, 1, shown.layers[0].id, [0, 0, 2, 2], [1, 1, 1, 255]));
    assert!(warned(&space.apply(Command::RasterClose { discard: false })).unwrap().contains("unsaved"));
    assert!(warned(&space.apply(Command::RasterNew { name: "Other".into(), preset: Preset::Paint, width: 64, height: 64, folder: None })).unwrap().contains("save or discard"));
    let gone = space.apply(Command::RasterClose { discard: true });
    assert!(view(&gone).unwrap().is_none());
    // Opened again: as last saved, with a new generation.
    let again = space.apply(Command::RasterOpen { path: "hero-sprite.canvas".into() });
    let reopened = view(&again).unwrap().unwrap();
    assert!(reopened.generation > shown.generation && !reopened.dirty && reopened.tiles.iter().all(|(_, t)| t.is_empty()));
    assert!(warned(&space.apply(Command::RasterOpen { path: "notes".into() })).unwrap().contains("not a canvas"));
}

fn said(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn images(events: &[Event]) -> Option<Vec<String>> {
    events.iter().find_map(|e| match e {
        Event::RasterHome { images, .. } => Some(images.clone()),
        _ => None,
    })
}

#[test]
fn a_canvas_is_written_out_as_a_png_and_a_psd_beside_its_folder() {
    let (mut space, root, shown) = with_canvas("export");
    let id = shown.layers[0].id;
    space.apply(fill(shown.generation, 1, id, [2, 2, 4, 4], [200, 30, 30, 255]));
    let events = space.apply(Command::RasterExport { format: proton_core::raster::RasterFormat::Png, scale: Some(4) });
    assert!(warned(&events).is_none(), "{events:?}");
    let (w, h, rgba) = proton_raster::file::decode_png(&std::fs::read(root.join("hero-sprite@4x.png")).unwrap()).unwrap();
    assert_eq!((w, h, &rgba[((10 * 256 + 10) * 4) as usize..((10 * 256 + 10) * 4 + 4) as usize]), (256, 256, &[200u8, 30, 30, 255][..]));
    assert!(images(&events).unwrap().contains(&"hero-sprite@4x.png".to_string()), "the home page does not offer it back");
    space.apply(Command::RasterExport { format: proton_core::raster::RasterFormat::Psd, scale: None });
    let picture = proton_raster::port::read(&std::fs::read(root.join("hero-sprite.psd")).unwrap(), "x").unwrap();
    assert_eq!((picture.width, picture.parts.len(), picture.parts[0].name.as_str()), (64, 1, "Sprite"));
}

#[test]
fn a_picture_comes_in_as_a_canvas_or_as_layers_one_undo_away() {
    let root = project("import");
    // A small PNG in the project: a canvas of its own, its few colours the palette.
    let mut rgba = vec![0u8; 20 * 10 * 4];
    for i in 0..20 * 10 {
        rgba[i * 4..i * 4 + 4].copy_from_slice(if i % 3 == 0 { &[10, 200, 30, 255] } else { &[250, 250, 250, 255] });
    }
    std::fs::create_dir_all(root.join("art")).unwrap();
    std::fs::write(root.join("art/leaf.png"), proton_raster::file::encode_png(20, 10, &rgba).unwrap()).unwrap();
    let mut space = Workspace::rooted(&root);
    let events = space.apply(Command::SetMode { mode: ModeId::Raster });
    assert_eq!(images(&events), Some(vec!["art/leaf.png".to_string()]));
    let events = space.apply(Command::RasterImport { path: Some("art/leaf.png".into()), blob: None, layer: false, above: None, name: None });
    assert!(warned(&events).is_none(), "{events:?}");
    let shown = view(&events).unwrap().expect("the picture is open as a canvas");
    assert_eq!((shown.path.as_str(), shown.preset, shown.width, shown.palette.len()), ("art/leaf.canvas", Preset::Pixel, 20, 2));
    // Bytes handed over — a paste — come in as a layer in the middle.
    let blob = space.raster_put_blob(proton_raster::file::encode_png(4, 2, &[255u8; 32]).unwrap()).unwrap();
    let events = space.apply(Command::RasterImport { path: None, blob: Some(blob), layer: true, above: None, name: None });
    let at_canvas = events.iter().position(|e| matches!(e, Event::RasterCanvas { .. })).expect("the layers are said");
    let at_changed = events.iter().position(|e| matches!(e, Event::RasterChanged { .. })).expect("the tiles are pushed");
    assert!(at_canvas < at_changed, "the layer must reach the window before its tiles");
    let shown = view(&events).unwrap().unwrap();
    assert_eq!((shown.layers.len(), shown.layers[1].name.as_str(), shown.can_undo), (2, "Pasted", true));
    let undone = space.apply(Command::RasterUndo);
    assert_eq!(view(&undone).unwrap().unwrap().layers.len(), 1, "one undo takes the layer back");
    // What cannot be read is said.
    let gif = space.raster_put_blob(b"GIF89a....".to_vec()).unwrap();
    assert!(warned(&space.apply(Command::RasterImport { path: None, blob: Some(gif), layer: true, above: None, name: None })).unwrap().contains("only PNG and Photoshop"));
    assert!(warned(&space.apply(Command::RasterImport { path: None, blob: Some(999), layer: true, above: None, name: None })).unwrap().contains("gone"));
    assert!(warned(&space.apply(Command::RasterImport { path: Some("../elsewhere.png".into()), blob: None, layer: true, above: None, name: None })).is_some());
}

#[test]
fn a_canvas_sent_to_the_game_open_is_written_into_its_art_and_imported() {
    let root = project("send");
    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode { mode: ModeId::Game });
    let events = space.apply(Command::GameNew { name: "Crossroads".into(), template: "metroidvania".into(), folder: None });
    assert!(warned(&events).is_none(), "{events:?}");
    let game = events
        .iter()
        .find_map(|e| match e {
            Event::GameHome { games, .. } => games.first().map(|g| g.path.clone()),
            _ => None,
        })
        .expect("the game is listed");
    space.apply(Command::SetMode { mode: ModeId::Raster });
    let events = space.apply(Command::RasterNew { name: "Far Hills [800]".into(), preset: Preset::Paint, width: 128, height: 64, folder: None });
    let shown = view(&events).unwrap().unwrap();
    space.apply(fill(shown.generation, 1, shown.layers[0].id, [0, 30, 128, 34], [60, 90, 70, 255]));
    // A layer named as Game reads planes: its depth in square brackets.
    let rename = Op::SetLayer { layer: shown.layers[0].id, name: Some("Far hills [800]".into()), opacity: None, blend: None, hidden: None, locked: None, alpha_lock: None };
    space.apply(Command::RasterEdit { generation: shown.generation, seq: 2, op: rename, label: "rename".into() });
    let events = space.apply(Command::RasterSend { game: game.clone(), layers: true });
    assert!(warned(&events).is_none(), "{events:?}");
    let dir = if game.is_empty() { root.clone() } else { root.join(&game) };
    assert!(dir.join("art/far-hills-800.psd").exists(), "{:?}", said(&events));
    assert!(said(&events).iter().any(|s| s.contains("import")), "the game open did not import it: {:?}", said(&events));
    assert!(warned(&space.apply(Command::RasterSend { game: "nowhere".into(), layers: false })).is_some());
}
