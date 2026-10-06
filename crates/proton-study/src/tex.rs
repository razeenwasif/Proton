//! TeX that went into JSON without its backslashes doubled, mended.
//!
//! A model asked for JSON writes `"$\frac{1}{2}$"`, and JSON reads `\f` as a
//! form feed: what arrives is a form feed, then `rac{1}{2}`. The same happens
//! to `\b` (`\beta`, `\binom`), `\t` (`\theta`, `\times`), `\r` (`\rho`,
//! `\right`) and `\n` (`\nabla`, `\neq`). Each is put back where what follows
//! the control character is a TeX command it could have begun — and `\n`,
//! which is also a real line break, only inside `$…$` mathematics, where a
//! line break never is.

use serde_json::Value;

/// Commands a tab, a carriage return or a line break could have begun, by
/// the letters after it. A form feed or a backspace is never anything but a
/// `\f` or a `\b` that lost its backslash.
const AFTER_T: &[&str] = &["theta", "tau", "times", "text", "textbf", "textit", "tilde", "to", "top", "triangle", "tfrac", "tan", "tanh", "therefore", "tiny", "texttt", "textrm", "textsf"];
const AFTER_R: &[&str] = &["rho", "right", "rangle", "rightarrow", "Rightarrow", "rm", "rceil", "rfloor", "rbrace", "real"];
const AFTER_N: &[&str] = &["nabla", "neq", "ne", "nu", "not", "notin", "newline", "neg", "nexists", "ni", "nmid", "nleq", "ngeq", "nolimits", "nonumber", "noindent"];

/// Mend one string.
pub fn mend(text: &str) -> String {
    if !text.contains(['\u{c}', '\u{8}', '\t', '\r', '\n']) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 8);
    let mut dollars = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        if c == '$' && (i == 0 || chars[i - 1] != '\\') {
            dollars += 1;
        }
        // JSON took the command's first letter with its backslash: `\times`
        // arrives as a tab and `imes`, so the letter is put back to look.
        let word: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
        let begins = |letter: char, list: &[&str]| {
            let whole = format!("{letter}{word}");
            list.iter().any(|cmd| whole.starts_with(cmd))
        };
        let fixed = match c {
            '\u{c}' => Some('f'),
            '\u{8}' => Some('b'),
            '\t' if begins('t', AFTER_T) => Some('t'),
            '\r' if begins('r', AFTER_R) => Some('r'),
            // A line break inside mathematics was never one.
            '\n' if dollars % 2 == 1 && begins('n', AFTER_N) => Some('n'),
            _ => None,
        };
        match fixed {
            Some(letter) => {
                out.push('\\');
                out.push(letter);
            }
            None => out.push(c),
        }
    }
    out
}

/// Mend every string in a JSON value.
pub fn mend_json(value: &mut Value) {
    match value {
        Value::String(text) => *text = mend(text),
        Value::Array(items) => items.iter_mut().for_each(mend_json),
        Value::Object(fields) => fields.values_mut().for_each(mend_json),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backslash_json_ate_is_put_back() {
        // What JSON makes of `"$\frac{1}{2} \beta \theta \rho$"` written unescaped.
        let eaten: String = serde_json::from_str(r#""$\frac{1}{2} + \beta \times \theta - \rho$""#).unwrap();
        assert_ne!(eaten, "$\\frac{1}{2} + \\beta \\times \\theta - \\rho$", "JSON did eat them");
        assert_eq!(mend(&eaten), "$\\frac{1}{2} + \\beta \\times \\theta - \\rho$");
    }

    #[test]
    fn a_real_line_break_or_tab_is_left_alone() {
        assert_eq!(mend("First line\nnot a command, just prose"), "First line\nnot a command, just prose");
        assert_eq!(mend("a\ttable\tcell"), "a\ttable\tcell", "`table` begins nothing a tab could have");
        // Inside mathematics, `\n` before `abla` was `\nabla`.
        let eaten: String = serde_json::from_str(r#""the gradient $\nabla f$ and $a \neq b$""#).unwrap();
        assert_eq!(mend(&eaten), "the gradient $\\nabla f$ and $a \\neq b$");
    }

    #[test]
    fn every_string_in_an_answer_is_mended() {
        let mut value: Value = serde_json::from_str(r#"{"cards": [{"front": "What is $\frac{d}{dx} x^2$?", "back": "$2x$"}]}"#).unwrap();
        mend_json(&mut value);
        assert_eq!(value["cards"][0]["front"], "What is $\\frac{d}{dx} x^2$?");
    }
}
