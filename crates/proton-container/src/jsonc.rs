//! Minimal JSONC (JSON with comments and trailing commas) pre-processor for
//! `devcontainer.json`.

/// Strip `//` line comments, `/* ... */` block comments, and trailing commas
/// before `}` or `]` while preserving string literals and escape sequences.
pub fn strip(input: &str) -> String {
    let no_comments = strip_comments(input);
    strip_trailing_commas(&no_comments)
}

fn strip_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;

    while i < bytes.len() {
        let ch = input[i..].chars().next().unwrap();
        let ch_len = ch.len_utf8();

        if in_string {
            out.push(ch);
            if ch == '\\' {
                // Keep the escaped character verbatim so `\"` or `\\` cannot
                // confuse string termination.
                if i + ch_len < bytes.len() {
                    let next = input[i + ch_len..].chars().next().unwrap();
                    out.push(next);
                    i += ch_len + next.len_utf8();
                    continue;
                }
            } else if ch == '"' {
                in_string = false;
            }
            i += ch_len;
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push('"');
            i += 1;
            continue;
        }

        if input[i..].starts_with("//") {
            i += 2;
            while i < bytes.len() {
                let c = input[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }

        if input[i..].starts_with("/*") {
            i += 2;
            let mut saw_newline = false;
            while i < bytes.len() {
                if input[i..].starts_with("*/") {
                    i += 2;
                    break;
                }
                let c = input[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == '\n' {
                    out.push('\n');
                    saw_newline = true;
                }
            }
            if !saw_newline {
                out.push(' ');
            }
            continue;
        }

        out.push(ch);
        i += ch_len;
    }

    out
}

fn strip_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;

    while i < bytes.len() {
        let ch = input[i..].chars().next().unwrap();
        let ch_len = ch.len_utf8();

        if in_string {
            out.push(ch);
            if ch == '\\' {
                if i + ch_len < bytes.len() {
                    let next = input[i + ch_len..].chars().next().unwrap();
                    out.push(next);
                    i += ch_len + next.len_utf8();
                    continue;
                }
            } else if ch == '"' {
                in_string = false;
            }
            i += ch_len;
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push('"');
            i += 1;
            continue;
        }

        if ch == ',' {
            let mut look = i + 1;
            while look < bytes.len() && bytes[look].is_ascii_whitespace() {
                look += 1;
            }
            if look < bytes.len() && matches!(bytes[look], b'}' | b']') {
                // Skip the trailing comma.
                i += 1;
                continue;
            }
        }

        out.push(ch);
        i += ch_len;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::strip;
    use serde_json::Value;

    #[test]
    fn preserves_urls_and_escaped_quotes_inside_strings() {
        let src = r#"{
            // comment before url
            "url": "https://example.com/a//b/*not_comment*/",
            "quote": "hello \"//world\" \\", // trailing line comment
            "list": [1, 2, /* inline */ 3,],
        }"#;
        let cleaned = strip(src);
        let val: Value = serde_json::from_str(&cleaned).expect("valid json after strip");
        assert_eq!(val["url"], "https://example.com/a//b/*not_comment*/");
        assert_eq!(val["quote"], "hello \"//world\" \\");
        assert_eq!(val["list"], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn strips_multiline_block_comments_and_nested_trailing_commas() {
        let src = r#"{
            /* multi-line
               comment with } and , inside */
            "a": {
                "b": [
                    { "x": 1, },
                    { "y": 2, },
                ],
            },
        }"#;
        let cleaned = strip(src);
        let val: Value = serde_json::from_str(&cleaned).expect("valid json after strip");
        assert_eq!(val["a"]["b"][0]["x"], 1);
        assert_eq!(val["a"]["b"][1]["y"], 2);
    }

    #[test]
    fn handles_trailing_line_comment_without_newline() {
        let src = r#"{ "ok": true } // eof"#;
        let val: Value = serde_json::from_str(&strip(src)).unwrap();
        assert_eq!(val["ok"], true);
    }
}
