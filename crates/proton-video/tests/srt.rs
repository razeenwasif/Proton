//! What reading the caption sidecar has to get right.
//!
//! The fixture has the things a transcription tool actually emits: a cue whose
//! words break over two lines on purpose, and a timing line carrying the
//! position hints some tools append and nothing else understands.

use proton_video::Captions;

const WORDS: &str = "1
00:00:02,000 --> 00:00:05,500
...and that is the burn complete.

2
00:00:06,120 --> 00:00:09,000
Orbital insertion confirmed.
Telemetry nominal.

3
00:00:12,000 --> 00:00:14,400  X1:100 X2:600 Y1:400 Y2:480
Stage two separation.
";

fn words() -> Captions {
    Captions::parse(WORDS).unwrap()
}

#[test]
fn a_file_that_was_not_edited_comes_back_exactly() {
    assert_eq!(words().to_text(), WORDS);
}

#[test]
fn a_cue_is_a_number_two_times_and_the_words() {
    let words = words();
    let cues = words.captions();
    assert_eq!(cues.len(), 3);
    assert_eq!(cues[0].index, "1");
    assert_eq!(cues[0].start, "00:00:02,000");
    assert_eq!(cues[0].end, "00:00:05,500");
    assert_eq!(cues[0].text, "...and that is the burn complete.");
    assert_eq!(cues[0].line, 2, "the line to open the file at");
}

#[test]
fn a_caption_that_breaks_over_two_lines_keeps_its_break() {
    // Where a caption breaks is a decision somebody made about how it reads on
    // screen. Joining the lines would quietly undo it.
    assert_eq!(
        words().captions()[1].text,
        "Orbital insertion confirmed.\nTelemetry nominal."
    );
}

#[test]
fn times_are_read_in_thousandths() {
    let words = words();
    let cues = words.captions();
    assert_eq!(cues[0].start_ms, 2_000);
    assert_eq!(cues[0].end_ms, 5_500);
    assert_eq!(cues[1].start_ms, 6_120);
}

#[test]
fn a_full_stop_is_read_the_same_as_a_comma() {
    // The format says comma. Tools written by people who use a full stop for a
    // decimal point emit one, and players accept it.
    let text = WORDS.replace("00:00:02,000", "00:00:02.000");
    let cues = Captions::parse(&text).unwrap();
    assert_eq!(cues.captions()[0].start_ms, 2_000);
}

#[test]
fn a_position_hint_after_the_times_survives_being_read() {
    let words = words();
    let third = &words.captions()[2];
    assert_eq!(third.start, "00:00:12,000");
    assert_eq!(third.end, "00:00:14,400");
    assert_eq!(third.text, "Stage two separation.");
}

#[test]
fn editing_a_time_leaves_the_position_hint_where_it_was() {
    // The hint sits on the same line as the times. A writer that re-printed
    // the line would drop it, and nothing would say so until a player put the
    // caption in the wrong place.
    let mut words = words();
    assert_eq!(words.set_field(2, "end", "00:00:15,000"), Ok(true));
    assert_eq!(
        words.to_text(),
        WORDS.replace(
            "00:00:12,000 --> 00:00:14,400  X1:100",
            "00:00:12,000 --> 00:00:15,000  X1:100"
        )
    );
}

#[test]
fn editing_the_words_replaces_only_the_words() {
    let mut words = words();
    assert_eq!(
        words.set_field(0, "text", "…and that is the burn complete."),
        Ok(true)
    );
    assert_eq!(
        words.to_text(),
        WORDS.replace(
            "...and that is the burn complete.",
            "…and that is the burn complete."
        )
    );
}

#[test]
fn a_two_line_caption_can_be_rewritten_as_two_lines() {
    let mut words = words();
    assert_eq!(
        words.set_field(
            1,
            "text",
            "Orbital insertion confirmed.\nAll telemetry nominal."
        ),
        Ok(true)
    );
    assert_eq!(
        words.to_text(),
        WORDS.replace("\nTelemetry nominal.", "\nAll telemetry nominal.")
    );
    assert_eq!(words.captions().len(), 3, "the cue split in two");
}

#[test]
fn a_blank_line_inside_a_caption_is_refused() {
    // It is what ends a cue. Accepting one would silently split this caption
    // in two and renumber nothing, which is a broken file.
    let mut words = words();
    let said = words
        .set_field(0, "text", "First half\n\nSecond half")
        .unwrap_err();
    assert!(said.contains("blank line"), "{said}");
    assert_eq!(words.to_text(), WORDS, "a refused edit still wrote");
}

#[test]
fn setting_what_is_already_there_changes_nothing() {
    let mut words = words();
    assert_eq!(
        words.set_field(0, "text", "...and that is the burn complete."),
        Ok(false)
    );
    assert_eq!(words.to_text(), WORDS);
}

#[test]
fn a_value_that_is_not_a_caption_time_is_refused() {
    let mut words = words();
    assert!(words.set_field(0, "start", "00:00:02").is_err());
    assert!(words.set_field(0, "start", "00:00:02,00").is_err());
    assert!(words.set_field(0, "start", "two seconds in").is_err());
    assert!(words.set_field(0, "start", "00:99:02,000").is_err());
    assert_eq!(words.to_text(), WORDS);
}

#[test]
fn a_file_that_omits_the_cue_numbers_is_still_read() {
    let bare = "00:00:01,000 --> 00:00:02,000\nOne.\n\n00:00:03,000 --> 00:00:04,000\nTwo.\n";
    let cues = Captions::parse(bare).unwrap();
    let cues = cues.captions();
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].index, "");
    assert_eq!(cues[1].text, "Two.");
}

#[test]
fn words_can_be_given_to_a_cue_that_has_none() {
    let empty = "1\n00:00:01,000 --> 00:00:02,000\n\n2\n00:00:03,000 --> 00:00:04,000\nTwo.\n";
    let mut cues = Captions::parse(empty).unwrap();
    assert_eq!(cues.captions()[0].text, "");
    assert_eq!(cues.set_field(0, "text", "One."), Ok(true));
    assert_eq!(
        cues.to_text(),
        "1\n00:00:01,000 --> 00:00:02,000\nOne.\n\n2\n00:00:03,000 --> 00:00:04,000\nTwo.\n"
    );
    assert_eq!(cues.captions().len(), 2);
}

#[test]
fn a_file_with_no_captions_in_it_is_refused() {
    let said = Captions::parse("TITLE: LAUNCH SEQUENCE\n001 AX V C\n").unwrap_err();
    assert!(said.contains("no captions"), "{said}");
}

#[test]
fn an_empty_file_is_a_file_with_no_captions_in_it() {
    // Which is what a transcription that found no speech looks like, and not
    // an error worth stopping the mode for.
    let cues = Captions::parse("").unwrap();
    assert!(cues.captions().is_empty());
    assert_eq!(cues.to_text(), "");
}

#[test]
fn a_file_a_windows_tool_wrote_survives_being_read() {
    let text = WORDS.replace('\n', "\r\n");
    let mut cues = Captions::parse(&text).unwrap();
    assert_eq!(cues.to_text(), text);
    assert_eq!(cues.captions().len(), 3);
    assert_eq!(cues.captions()[0].end, "00:00:05,500");
    assert_eq!(
        cues.captions()[1].text,
        "Orbital insertion confirmed.\r\nTelemetry nominal.",
        "the caption's own line break is the file's, carriage return and all"
    );
    assert_eq!(cues.set_field(0, "text", "Burn complete."), Ok(true));
    assert_eq!(
        cues.to_text(),
        WORDS
            .replace("...and that is the burn complete.", "Burn complete.")
            .replace('\n', "\r\n")
    );
}

#[test]
fn there_is_no_caption_at_a_row_that_is_not_there() {
    let mut words = words();
    assert!(words.set_field(99, "text", "Nothing").is_err());
}

#[test]
fn a_field_this_does_not_edit_is_refused_by_name() {
    let mut words = words();
    let said = words.set_field(0, "index", "7").unwrap_err();
    assert!(said.contains("index"), "{said}");
}
