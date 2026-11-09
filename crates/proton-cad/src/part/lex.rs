//! A line of a `.part` file in tokens. The language is a statement a line,
//! so lines are read one at a time; a `#` starts a comment to the line's end.

use super::ast::Diag;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    /// A number without a sign: digits, and a fraction after a point.
    Num(f64),
    Str(String),
    Sym(char),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    /// The column it starts at, from 1, in characters.
    pub col: usize,
    /// The byte it starts at, for taking the rest of the line as written.
    pub at: usize,
}

/// A line's tokens and its comment, the text after `#` (outside a string).
pub fn line(text: &str, lineno: usize) -> Result<(Vec<Token>, Option<String>), Diag> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        let col = i + 1;
        if c.is_whitespace() {
            i += 1;
        } else if c == '#' {
            return Ok((toks, Some(text[at + 1..].to_string())));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].1.is_ascii_alphanumeric() || chars[i].1 == '_') {
                i += 1;
            }
            toks.push(Token { tok: Tok::Ident(chars[start..i].iter().map(|x| x.1).collect()), col, at });
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].1.is_ascii_digit() {
                i += 1;
            }
            if i + 1 < chars.len() && chars[i].1 == '.' && chars[i + 1].1.is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].1.is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().map(|x| x.1).collect();
            let n = s.parse::<f64>().map_err(|_| Diag::new(lineno, col, format!("`{s}` is not a number")))?;
            toks.push(Token { tok: Tok::Num(n), col, at });
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                let Some(&(_, d)) = chars.get(i) else { return Err(Diag::new(lineno, col, "a string with no closing `\"`")) };
                i += 1;
                match d {
                    '"' => break,
                    '\\' => {
                        let Some(&(_, e)) = chars.get(i) else { return Err(Diag::new(lineno, col, "a string with no closing `\"`")) };
                        s.push(e);
                        i += 1;
                    }
                    _ => s.push(d),
                }
            }
            toks.push(Token { tok: Tok::Str(s), col, at });
        } else if "()=,.+-*/&".contains(c) {
            toks.push(Token { tok: Tok::Sym(c), col, at });
            i += 1;
        } else {
            return Err(Diag::new(lineno, col, format!("`{c}` means nothing here")));
        }
    }
    Ok((toks, None))
}
