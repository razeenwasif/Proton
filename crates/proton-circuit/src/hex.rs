//! Memory contents as text: a ROM's, a RAM's, a look-up table's.
//!
//! Read: words in hex, split by spaces, commas or lines; `n*w` for n of the
//! same word; `#` to the end of a line is a comment; a first line
//! `v2.0 raw`, as Logisim writes it, is taken as a header. Digital keeps a
//! part's contents in its file the same way, commas between. Written: the
//! `v2.0 raw` header, eight words a line, runs of four or more as `n*w`,
//! and trailing zeros left off, so the same contents are the same bytes.

/// No more words than this are read: a 24-bit address's worth.
pub const MOST_WORDS: usize = 1 << 24;

pub fn read(text: &str) -> Result<Vec<u64>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if n == 0 && line.eq_ignore_ascii_case("v2.0 raw") {
            continue;
        }
        for tok in line.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()) {
            let (count, word) = match tok.split_once('*') {
                Some((c, w)) => (c.parse::<usize>().map_err(|_| format!("line {}: “{tok}” is not a count of words", n + 1))?, w),
                None => (1, tok),
            };
            let word = word.trim_start_matches("0x");
            let v = u64::from_str_radix(word, 16).map_err(|_| format!("line {}: “{tok}” is not a word in hex", n + 1))?;
            if out.len() + count > MOST_WORDS {
                return Err(format!("more than {MOST_WORDS} words"));
            }
            out.extend(std::iter::repeat_n(v, count));
        }
    }
    Ok(out)
}

pub fn write(words: &[u64]) -> String {
    let used = words.iter().rposition(|w| *w != 0).map_or(0, |i| i + 1);
    let mut toks = Vec::new();
    let mut i = 0;
    while i < used {
        let w = words[i];
        let run = words[i..used].iter().take_while(|x| **x == w).count();
        if run >= 4 {
            toks.push(format!("{run}*{w:x}"));
            i += run;
        } else {
            toks.push(format!("{w:x}"));
            i += 1;
        }
    }
    let mut s = String::from("v2.0 raw\n");
    for line in toks.chunks(8) {
        s.push_str(&line.join(" "));
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contents_read_as_logisim_and_digital_write_them_and_round_trip() {
        assert_eq!(read("v2.0 raw\n1 2 ff # a comment\n3*0 a\n").unwrap(), vec![1, 2, 0xff, 0, 0, 0, 0xa]);
        assert_eq!(read("1,2,0x10").unwrap(), vec![1, 2, 0x10]);
        assert!(read("1 zz").unwrap_err().contains("zz"));
        let words = vec![1, 2, 3, 0, 0, 0, 0, 0, 9, 9, 9, 9, 9, 7, 0, 0];
        let text = write(&words);
        assert_eq!(text, "v2.0 raw\n1 2 3 5*0 5*9 7\n");
        assert_eq!(read(&text).unwrap(), words[..14].to_vec());
        assert_eq!(write(&[0, 0]), "v2.0 raw\n");
    }
}
