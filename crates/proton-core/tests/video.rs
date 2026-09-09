//! Video mode's timeline, through the seam the shell uses.
//!
//! The two documents are `proton-video`'s and have their own tests for the
//! formats. What is checked here is the editor's half: that entering the mode
//! finds the cut and the captions beside it, that a click selects, that an
//! edit reaches the disk without disturbing the rest of a file another tool
//! will open next, that a change made elsewhere comes back, and that the frame
//! rate — the one thing an EDL does not record — behaves like the assumption
//! it is.

use std::path::{Path, PathBuf};

use proton_core::bus::Event;
use proton_core::key::Key;
use proton_core::{Command, ModeId, Workspace};

const CUT: &str = r#"TITLE: LAUNCH SEQUENCE
FCM: NON-DROP FRAME

001  AX       V     C        00:00:00:00 00:00:12:10 00:00:00:00 00:00:12:10
* FROM CLIP NAME: launch_a.mov

002  BX       V     C        00:00:04:00 00:00:09:12 00:00:12:10 00:00:17:22
* FROM CLIP NAME: ground_cam.mp4

003  AX       AA    C        00:00:00:00 00:00:17:22 00:00:00:00 00:00:17:22
* FROM CLIP NAME: vo_take3.wav
"#;

const WORDS: &str = "1
00:00:02,000 --> 00:00:05,500
...and that is the burn complete.

2
00:00:06,120 --> 00:00:09,000
Orbital insertion confirmed.
";

/// A project with a cut in it, laid out the way somebody would lay one out.
fn open(name: &str) -> (Workspace, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-video-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("edit").join("media")).unwrap();
    let path = root.join("edit").join("launch.edl");
    std::fs::write(&path, CUT).unwrap();
    std::fs::write(root.join("edit").join("launch.srt"), WORDS).unwrap();
    // One of the three sources is really there and the other two are not,
    // which is the state a cut handed over on its own always arrives in.
    std::fs::write(root.join("edit").join("media").join("launch_a.mov"), b"").unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode {
        mode: ModeId::Video,
    });
    (space, path)
}

/// The one `Event::Cut` a command answered with.
fn cut(space: &mut Workspace, command: Command) -> Event {
    space
        .apply(command)
        .into_iter()
        .find(|event| matches!(event, Event::Cut { .. }))
        .expect("a cut event")
}

fn now(space: &mut Workspace) -> Event {
    cut(space, Command::Sync)
}

fn clips(space: &mut Workspace) -> Vec<String> {
    match now(space) {
        Event::Cut { clips, .. } => clips.into_iter().map(|clip| clip.name).collect(),
        _ => unreachable!(),
    }
}

#[test]
fn entering_video_mode_finds_the_cut_and_the_captions_beside_it() {
    let (mut space, path) = open("discover");
    let Event::Cut {
        title,
        clips,
        captions,
        captions_path,
        selected,
        ..
    } = now(&mut space)
    else {
        unreachable!()
    };
    assert_eq!(title, "LAUNCH SEQUENCE");
    assert_eq!(clips.len(), 3);
    assert_eq!(clips[0].name, "launch_a.mov");
    assert_eq!(selected, usize::MAX);
    assert_eq!(captions.len(), 2, "the sidecar was not found");
    assert_eq!(captions[0].text, "...and that is the burn complete.");
    assert!(
        captions_path.ends_with("launch.srt"),
        "{captions_path:?} is not the file beside {}",
        path.display()
    );
}

#[test]
fn the_lanes_stack_picture_over_sound() {
    let (mut space, _) = open("tracks");
    let Event::Cut { tracks, .. } = now(&mut space) else {
        unreachable!()
    };
    assert_eq!(tracks, ["V1", "A1", "A2"]);
}

#[test]
fn the_media_bin_says_which_sources_are_not_there() {
    // The most useful thing this panel can say, and the only thing about the
    // media it can say without decoding any of it.
    let (mut space, _) = open("media");
    let Event::Cut { media, .. } = now(&mut space) else {
        unreachable!()
    };
    let said: Vec<(String, bool)> = media
        .into_iter()
        .map(|item| (item.name, item.found))
        .collect();
    assert_eq!(
        said,
        [
            ("launch_a.mov".to_string(), true),
            ("ground_cam.mp4".to_string(), false),
            ("vo_take3.wav".to_string(), false),
        ]
    );
}

#[test]
fn a_timecode_edited_here_reaches_the_disk_and_moves_nothing_else() {
    // The list is an interchange file and something else will open it next. A
    // re-aligned EDL is a whole-file diff in somebody's edit.
    let (mut space, path) = open("write");
    space.apply(Command::SetClipField {
        index: 0,
        field: "recordOut".into(),
        value: "00:00:12:11".into(),
    });

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        CUT.replace("00:00:00:00 00:00:12:10\n", "00:00:00:00 00:00:12:11\n")
    );
}

#[test]
fn setting_what_is_already_there_does_not_touch_the_file() {
    let (mut space, path) = open("no-op");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let events = space.apply(Command::SetClipField {
        index: 0,
        field: "recordOut".into(),
        value: "00:00:12:10".into(),
    });
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn a_caption_edited_here_reaches_the_sidecar_and_not_the_cut() {
    let (mut space, path) = open("caption");
    space.apply(Command::SetCaptionField {
        index: 0,
        field: "text".into(),
        value: "…and that is the burn complete.".into(),
    });

    let beside = path.with_file_name("launch.srt");
    assert_eq!(
        std::fs::read_to_string(&beside).unwrap(),
        WORDS.replace(
            "...and that is the burn complete.",
            "…and that is the burn complete."
        )
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        CUT,
        "editing the words moved the cut"
    );
}

#[test]
fn a_value_that_is_not_a_timecode_is_refused_and_says_so() {
    let (mut space, path) = open("refuse");
    let events = space.apply(Command::SetClipField {
        index: 0,
        field: "recordOut".into(),
        value: "twelve seconds".into(),
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice { .. })),
        "it was refused silently: {events:?}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), CUT);
}

#[test]
fn a_cut_edited_in_codex_reaches_the_timeline() {
    // The agent may be patching this file, and so may a person in the buffer
    // next door.
    let (mut space, path) = open("codex");
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    space.apply(Command::OpenBuffer {
        path: path.display().to_string(),
    });
    for c in "/ground_cam".chars() {
        space.apply(Command::Key { key: Key::char(c) });
    }
    space.apply(Command::Key {
        key: Key::named("Enter"),
    });
    for c in "ciwtower_cam".chars() {
        space.apply(Command::Key { key: Key::char(c) });
    }
    space.apply(Command::Key {
        key: Key::named("Escape"),
    });
    space.apply(Command::Save);

    let names = clips(&mut space);
    assert!(
        names.iter().any(|name| name.starts_with("tower_cam")),
        "the timeline is still showing the old cut: {names:?}"
    );
}

#[test]
fn a_file_that_is_not_a_cut_is_refused_and_the_panel_says_why() {
    // Listing a shooting script's events as a timeline would be a confident
    // wrong answer.
    let (mut space, path) = open("not-a-cut");
    let notes = path.with_file_name("notes.edl");
    std::fs::write(&notes, "Reshoot the third act.\n").unwrap();

    let said = match cut(
        &mut space,
        Command::OpenTimeline {
            path: notes.display().to_string(),
        },
    ) {
        Event::Cut { problem, .. } => problem,
        _ => unreachable!(),
    };
    assert!(said.contains("no edit events"), "{said}");
    assert!(clips(&mut space).is_empty(), "it drew something anyway");
}

#[test]
fn selecting_a_row_that_is_no_longer_there_selects_nothing() {
    let (mut space, _) = open("stale");
    space.apply(Command::SelectClip { index: 99 });
    let Event::Cut { selected, .. } = now(&mut space) else {
        unreachable!()
    };
    assert_eq!(selected, usize::MAX);
}

#[test]
fn changing_the_rate_changes_what_the_timecodes_mean_and_writes_nothing() {
    // An EDL does not record its rate, so the one on screen is an assumption.
    // Correcting it is not an edit: no byte of the file moves.
    let (mut space, path) = open("rate");
    let Event::Cut { rate, length, .. } = now(&mut space) else {
        unreachable!()
    };
    assert_eq!(rate.fps, 24, "the default rate is not the one being shown");
    assert_eq!(length, 17 * 24 + 22);

    let Event::Cut { rate, length, .. } = cut(&mut space, Command::SetTimelineRate { fps: 25 })
    else {
        unreachable!()
    };
    assert_eq!(rate.fps, 25);
    assert_eq!(length, 17 * 25 + 22);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        CUT,
        "reading the file differently rewrote it"
    );
}

#[test]
fn a_cut_with_no_captions_beside_it_is_not_a_problem() {
    // Plenty of cuts have no dialogue in them.
    let (mut space, path) = open("silent");
    std::fs::remove_file(path.with_file_name("launch.srt")).unwrap();
    space.apply(Command::OpenTimeline {
        path: path.display().to_string(),
    });

    let Event::Cut {
        captions,
        captions_path,
        problem,
        clips,
        ..
    } = now(&mut space)
    else {
        unreachable!()
    };
    assert!(captions.is_empty());
    assert_eq!(captions_path, "");
    assert_eq!(problem, "", "a missing sidecar was reported as a fault");
    assert_eq!(clips.len(), 3, "the cut itself did not open");
}

#[test]
fn a_transcription_written_after_the_cut_was_opened_still_arrives() {
    let (mut space, path) = open("late");
    std::fs::remove_file(path.with_file_name("launch.srt")).unwrap();
    space.apply(Command::OpenTimeline {
        path: path.display().to_string(),
    });
    assert!(matches!(now(&mut space), Event::Cut { ref captions, .. } if captions.is_empty()));

    // Written by something else while somebody was working elsewhere, which is
    // how transcriptions actually arrive.
    std::fs::write(path.with_file_name("launch.srt"), WORDS).unwrap();
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    space.apply(Command::SetMode {
        mode: ModeId::Video,
    });

    let Event::Cut { captions, .. } = now(&mut space) else {
        unreachable!()
    };
    assert_eq!(captions.len(), 2, "the sidecar was never looked for again");
}

#[test]
fn the_ruler_reaches_past_the_last_cut_to_the_last_word() {
    // A title card over black at the end is a caption outlasting the picture.
    // A ruler that stopped at the final cut would put it off the edge.
    let (mut space, path) = open("ruler");
    std::fs::write(
        path.with_file_name("launch.srt"),
        "1\n00:00:20,000 --> 00:00:24,000\nIn memory of the ground crew.\n",
    )
    .unwrap();
    space.apply(Command::OpenTimeline {
        path: path.display().to_string(),
    });

    let Event::Cut { length, .. } = now(&mut space) else {
        unreachable!()
    };
    assert_eq!(length, 24 * 24, "the ruler stopped at the last cut");
}

#[test]
fn a_project_with_no_cut_in_it_says_nothing_rather_than_inventing_one() {
    let root = std::env::temp_dir().join(format!("proton-video-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut space = Workspace::rooted(Path::new(&root));
    space.apply(Command::SetMode {
        mode: ModeId::Video,
    });

    let Event::Cut {
        path, clips, media, ..
    } = now(&mut space)
    else {
        unreachable!()
    };
    assert_eq!(path, "");
    assert!(clips.is_empty());
    assert!(media.is_empty());
}
