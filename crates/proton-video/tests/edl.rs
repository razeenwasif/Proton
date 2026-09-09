//! What reading a cut has to get right.
//!
//! The fixture is shaped the way a real EDL comes out of an NLE, including the
//! parts this does not understand — a dissolve written as two events sharing a
//! number, an effect name, an audio remap — because those are most of a real
//! list and all of what a careless writer would destroy.

use proton_video::{Rate, Timecode, Timeline};

const SHOW: &str = r#"TITLE: LAUNCH SEQUENCE
FCM: NON-DROP FRAME

001  AX       V     C        00:00:00:00 00:00:12:10 00:00:00:00 00:00:12:10
* FROM CLIP NAME: launch_a.mov
* SOURCE FILE: /media/launch/launch_a.mov

002  BX       V     C        00:00:00:00 00:00:00:00 00:00:12:10 00:00:12:10
002  BX       V     D    024 00:00:04:00 00:00:09:12 00:00:12:10 00:00:17:22
* FROM CLIP NAME: ground_cam.mp4
* EFFECT NAME: CROSS DISSOLVE

AUD  3

003  AX       AA    C        00:00:00:00 00:00:17:22 00:00:00:00 00:00:17:22
* FROM CLIP NAME: vo_take3.wav

004  CX       AA/V  C        00:01:00:00 00:01:06:00 00:00:17:22 00:00:23:22
* FROM CLIP NAME: launch_b.mov
"#;

/// Two of the fixture's event lines, by name, so that a test can say which one
/// it expects to change and then assert on the whole file.
///
/// Whole-file equality rather than a diff of the field: `00:00:12:10` appears
/// three times in this list and `AX` twice, so an edit that hit the wrong one
/// would satisfy any assertion that only looked at the value it meant to
/// change. It would fail here.
const CUT: &str = "001  AX       V     C        00:00:00:00 00:00:12:10 00:00:00:00 00:00:12:10";
const OUTGOING: &str =
    "002  BX       V     C        00:00:00:00 00:00:00:00 00:00:12:10 00:00:12:10";

fn show() -> Timeline {
    Timeline::parse(SHOW, 24).unwrap()
}

#[test]
fn a_file_that_was_not_edited_comes_back_exactly() {
    assert_eq!(show().to_text(), SHOW);
}

#[test]
fn an_event_is_its_four_timecodes() {
    // Where the piece starts and ends in the source, and where it starts and
    // ends on the finished programme. That is the whole document.
    let show = show();
    let first = &show.clips()[0];
    assert_eq!(first.event, "001");
    assert_eq!(first.reel, "AX");
    assert_eq!(first.channel, "V");
    assert_eq!(first.transition, "C");
    assert_eq!(first.source_in, "00:00:00:00");
    assert_eq!(first.source_out, "00:00:12:10");
    assert_eq!(first.record_in, "00:00:00:00");
    assert_eq!(first.record_out, "00:00:12:10");
    assert_eq!(first.name, "launch_a.mov");
    assert_eq!(first.source, "/media/launch/launch_a.mov");
}

#[test]
fn a_dissolve_is_two_events_sharing_a_number() {
    // CMX writes the outgoing side and the incoming side as separate lines
    // with the same event number. Reading the number as an identity would
    // silently drop one of them.
    let sharing: Vec<String> = show()
        .clips()
        .into_iter()
        .filter(|clip| clip.event == "002")
        .map(|clip| clip.transition)
        .collect();
    assert_eq!(sharing, ["C", "D"]);
}

#[test]
fn the_transition_length_column_is_only_there_when_there_is_a_transition() {
    let show = show();
    let clips = show.clips();
    assert_eq!(clips[0].duration, "", "a cut has no length");
    assert_eq!(clips[2].duration, "024", "a dissolve does");
    // And the timecodes are still read from the right columns either way,
    // which is the reason they are taken from the end of the line.
    assert_eq!(clips[2].source_in, "00:00:04:00");
}

#[test]
fn one_event_can_land_in_more_than_one_lane() {
    // `AA/V` is a picture and both channels of its sound, in a single event.
    // Picking one lane for it would put the other two nowhere.
    let show = show();
    let clips = show.clips();
    assert_eq!(clips[0].tracks, ["V1"]);
    assert_eq!(clips[3].tracks, ["A1", "A2"]);
    assert_eq!(clips[4].tracks, ["A1", "A2", "V1"]);
}

#[test]
fn a_line_that_is_not_an_event_is_not_read_as_one() {
    // The title, the FCM line, an audio remap and six comments are all in this
    // file. Five lines are events.
    assert_eq!(show().clips().len(), 5);
}

#[test]
fn record_timecode_becomes_a_frame_number_at_the_stated_rate() {
    let show = show();
    let clips = show.clips();
    assert_eq!(clips[0].start, 0);
    assert_eq!(clips[0].end, 12 * 24 + 10);
    // The same file read at a different rate gives different frames, which is
    // exactly why the rate is a parameter and not a guess.
    let faster = Timeline::parse(SHOW, 30).unwrap();
    assert_eq!(faster.clips()[0].end, 12 * 30 + 10);
}

#[test]
fn non_drop_frame_is_not_read_as_drop_frame() {
    // `NON-DROP FRAME` contains the word `DROP`. Every naive reader of this
    // format gets this backwards once.
    assert!(!show().rate().drop);
    assert_eq!(show().rate().label(), "24 fps · non-drop");
}

#[test]
fn drop_frame_skips_the_numbers_it_says_it_skips() {
    let text = SHOW.replace("FCM: NON-DROP FRAME", "FCM: DROP FRAME");
    let show = Timeline::parse(&text, 30).unwrap();
    assert!(show.rate().drop);

    // The labels `01:00` and `01:01` are never used, so the frame after
    // `00:00:59:29` is `00:01:00:02` — one frame later, two numbers later.
    let rate = show.rate();
    assert_eq!(
        Timecode::parse("00:00:59:29").unwrap().to_frames(rate) + 1,
        Timecode::parse("00:01:00:02").unwrap().to_frames(rate),
    );
    // Every tenth minute skips nothing, and that is where a reader that got
    // the rule half right goes wrong.
    assert_eq!(
        Timecode::parse("00:09:59:29").unwrap().to_frames(rate) + 1,
        Timecode::parse("00:10:00:00").unwrap().to_frames(rate),
    );
}

#[test]
fn a_frame_number_and_a_timecode_are_the_same_thing_read_two_ways() {
    for drop in [false, true] {
        let rate = Rate::new(30, drop);
        for frame in [0, 1, 1799, 1800, 17981, 17982, 108_000, 215_999] {
            let there_and_back = Timecode::from_frames(frame, rate).to_frames(rate);
            assert_eq!(there_and_back, frame, "{frame} at {}", rate.label());
        }
    }
}

#[test]
fn drop_frame_is_ignored_where_it_means_nothing() {
    // It exists to correct the 1000/1001 pulldown. There is no such thing at
    // 24, and honouring it would shift every timecode in the list.
    assert!(!Rate::new(24, true).drop);
    assert!(Rate::new(30, true).drop);
}

#[test]
fn editing_a_timecode_moves_only_that_timecode() {
    let mut show = show();
    assert_eq!(
        show.set_field(0, "recordOut", "00:00:12:11"),
        Ok(true),
        "the edit did not take"
    );
    assert_eq!(
        show.to_text(),
        SHOW.replace(CUT, &format!("{}11", &CUT[..CUT.len() - 2])),
        "something other than the record out point moved"
    );
}

#[test]
fn setting_what_is_already_there_changes_nothing() {
    let mut show = show();
    assert_eq!(show.set_field(0, "recordOut", "00:00:12:10"), Ok(false));
    assert_eq!(show.to_text(), SHOW);
}

#[test]
fn a_value_that_is_not_a_timecode_is_refused() {
    let mut show = show();
    assert!(show.set_field(0, "recordOut", "twelve seconds").is_err());
    assert!(show.set_field(0, "recordOut", "00:00:12").is_err());
    assert!(show.set_field(0, "recordOut", "00:99:12:10").is_err());
    assert_eq!(show.to_text(), SHOW, "a refused edit still wrote");
}

#[test]
fn a_reel_name_with_a_space_in_it_is_refused() {
    // It would swallow the next column and turn the line into a different
    // edit — one this parser would then read back without complaining.
    let mut show = show();
    assert!(show.set_field(0, "reel", "LAUNCH A").is_err());
    assert_eq!(show.set_field(0, "reel", "LAUNCH_A"), Ok(true));
    assert_eq!(
        show.to_text(),
        SHOW.replace(CUT, &CUT.replacen("AX", "LAUNCH_A", 1))
    );
}

#[test]
fn renaming_a_clip_edits_the_comment_that_names_it() {
    let mut show = show();
    assert_eq!(show.set_field(0, "name", "launch_a_v2.mov"), Ok(true));
    assert_eq!(
        show.to_text(),
        SHOW.replace(
            "* FROM CLIP NAME: launch_a.mov",
            "* FROM CLIP NAME: launch_a_v2.mov"
        )
    );
}

#[test]
fn naming_a_clip_that_has_no_name_writes_one_line_under_its_event() {
    // The outgoing side of the dissolve has no `FROM CLIP NAME` of its own,
    // which is ordinary — most tools name only the incoming side.
    let mut show = show();
    assert_eq!(show.set_field(1, "name", "launch_a.mov"), Ok(true));
    assert_eq!(
        show.to_text(),
        SHOW.replace(
            OUTGOING,
            &format!("{OUTGOING}\n* FROM CLIP NAME: launch_a.mov")
        )
    );
    // And it is read back as that event's name, not as the next one's.
    assert_eq!(show.clips()[1].name, "launch_a.mov");
    assert_eq!(show.clips()[2].name, "ground_cam.mp4");
}

#[test]
fn a_comment_belongs_to_the_event_above_it() {
    // Every tool writes them that way. Reading one downwards would give every
    // clip the name of the one before it — wrong in a way that looks right.
    let show = show();
    let clips = show.clips();
    assert_eq!(clips[0].name, "launch_a.mov");
    assert_eq!(
        clips[1].name, "",
        "the outgoing side is unnamed in the file"
    );
    assert_eq!(clips[2].name, "ground_cam.mp4");
    assert_eq!(clips[3].name, "vo_take3.wav");
    assert_eq!(clips[4].name, "launch_b.mov");
}

#[test]
fn the_programme_ends_where_the_furthest_event_ends() {
    // Not where the last line ends. An EDL is not required to be in record
    // order, and a list assembled in several passes is not.
    let jumbled = format!(
        "{SHOW}\n005  DX       V     C        00:00:00:00 00:00:02:00 00:00:02:00 00:00:04:00\n"
    );
    let show = Timeline::parse(&jumbled, 24).unwrap();
    assert_eq!(show.length(), 23 * 24 + 22);
}

#[test]
fn the_title_comes_off_the_title_line() {
    assert_eq!(show().title(), "LAUNCH SEQUENCE");
}

#[test]
fn a_file_with_no_events_in_it_is_refused() {
    let said = Timeline::parse("TITLE: NOTHING\nFCM: NON-DROP FRAME\n", 24).unwrap_err();
    assert!(said.contains("no edit events"), "{said}");
}

#[test]
fn a_list_a_windows_tool_wrote_survives_being_read() {
    // CRLF throughout, which is what most of them emit. A reader that splits
    // on `\n` and forgets the `\r` puts it inside the last timecode.
    let text = SHOW.replace('\n', "\r\n");
    let mut show = Timeline::parse(&text, 24).unwrap();
    assert_eq!(show.to_text(), text);
    assert_eq!(show.clips().len(), 5);
    assert_eq!(show.clips()[0].record_out, "00:00:12:10");
    assert_eq!(show.clips()[0].name, "launch_a.mov");

    assert_eq!(show.set_field(0, "recordOut", "00:00:12:11"), Ok(true));
    assert_eq!(
        show.to_text(),
        SHOW.replace(CUT, &format!("{}11", &CUT[..CUT.len() - 2]))
            .replace('\n', "\r\n"),
        "the carriage returns did not survive the edit"
    );
}

#[test]
fn columns_that_were_never_aligned_are_not_aligned_now() {
    // Some tools emit single spaces. Re-aligning on the way out would be a
    // whole-file diff to change one field.
    let sloppy = "TITLE: X\n001 AX V C 00:00:00:00 00:00:01:00 00:00:00:00 00:00:01:00\n";
    let mut show = Timeline::parse(sloppy, 24).unwrap();
    assert_eq!(show.clips()[0].record_out, "00:00:01:00");
    assert_eq!(show.set_field(0, "reel", "BX"), Ok(true));
    assert_eq!(
        show.to_text(),
        "TITLE: X\n001 BX V C 00:00:00:00 00:00:01:00 00:00:00:00 00:00:01:00\n"
    );
}

#[test]
fn a_field_this_does_not_edit_is_refused_by_name() {
    let mut show = show();
    let said = show.set_field(0, "transition", "D").unwrap_err();
    assert!(said.contains("transition"), "{said}");
}

#[test]
fn there_is_no_event_at_a_row_that_is_not_there() {
    let mut show = show();
    assert!(show.set_field(99, "reel", "BX").is_err());
}
