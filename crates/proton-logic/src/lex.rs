//! Text to tokens, for either language. Symbols are read in every form a
//! textbook or another calculator writes them, and each is given one name:
//! `¬`, `!` and `~` are all `not`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Num(i64),
    /// An operator or punctuation, in its one name: `not and or xor -> <->
    /// = != < <= > >= + - * ( ) , : ; . .. ? |= == { } |` and the like.
    Sym(&'static str),
    /// The end of a line, where it can end a statement.
    Newline,
    /// A comment, without its `#` or `%`.
    Comment(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
}

/// Which language: the comment character, and how `|` and `^` read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Proton,
    L4f,
}

/// Symbols, longest first so `<->` is not read as `<` `-` `>`.
const SYMBOLS: &[(&str, &str)] = &[
    ("<=>", "<->"),
    ("<->", "<->"),
    ("->", "->"),
    ("=>", "->"),
    ("|=", "|="),
    ("|-", "|="),
    ("==", "=="),
    ("!=", "!="),
    ("<>", "!="),
    ("<=", "<="),
    (">=", ">="),
    ("&&", "and"),
    ("||", "or"),
    ("..", ".."),
    ("!<", ">="),
    ("!>", "<="),
    ("¬", "not"),
    ("∧", "and"),
    ("∨", "or"),
    ("⊕", "xor"),
    ("→", "->"),
    ("⊃", "->"),
    ("↔", "<->"),
    ("≡", "=="),
    ("⊨", "|="),
    ("⊢", "|="),
    ("≠", "!="),
    ("≤", "<="),
    ("≥", ">="),
    ("⊤", "true"),
    ("⊥", "false"),
    ("∀", "all"),
    ("∃", "some"),
    ("–", "-"),
    ("!", "not"),
    ("~", "not"),
    ("&", "and"),
    ("|", "|"),
    ("^", "and"),
    ("=", "="),
    ("<", "<"),
    (">", ">"),
    ("+", "+"),
    ("-", "-"),
    ("*", "*"),
    ("(", "("),
    (")", ")"),
    ("{", "{"),
    ("}", "}"),
    (",", ","),
    (":", ":"),
    (";", ";"),
    (".", "."),
    ("?", "?"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

pub fn lex(text: &str, dialect: Dialect) -> Result<Vec<Token>, LexError> {
    let comment = if dialect == Dialect::Proton { '#' } else { '%' };
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let chars: Vec<char> = line.chars().collect();
        let mut c = 0;
        while c < chars.len() {
            let ch = chars[c];
            if ch.is_whitespace() {
                c += 1;
                continue;
            }
            if ch == comment {
                let rest: String = chars[c + 1..].iter().collect();
                out.push(Token { tok: Tok::Comment(rest.trim().to_string()), line: n, col: c + 1 });
                break;
            }
            if ch.is_ascii_digit() {
                let start = c;
                while c < chars.len() && chars[c].is_ascii_digit() {
                    c += 1;
                }
                let s: String = chars[start..c].iter().collect();
                let v = s.parse::<i64>().map_err(|_| LexError { line: n, col: start + 1, message: format!("{s} is too big a number") })?;
                out.push(Token { tok: Tok::Num(v), line: n, col: start + 1 });
                continue;
            }
            if ch.is_alphabetic() || ch == '_' {
                let start = c;
                while c < chars.len() && (chars[c].is_alphanumeric() || chars[c] == '_' || chars[c] == '\'') {
                    c += 1;
                }
                // Logic4Fun subscripts a built-in with its sort: `+_int` reads as
                // `+`, the sort being typing's to find.
                let s: String = chars[start..c].iter().collect();
                out.push(Token { tok: Tok::Ident(s), line: n, col: start + 1 });
                continue;
            }
            let rest: String = chars[c..].iter().collect();
            let Some((text, name)) = SYMBOLS.iter().find(|(t, _)| rest.starts_with(t)) else {
                return Err(LexError { line: n, col: c + 1, message: format!("“{ch}” is not part of the language") });
            };
            c += text.chars().count();
            // `+_int`, `-_nat`: a subscript naming the sort, which is dropped.
            if dialect == Dialect::L4f && matches!(*name, "+" | "-") && chars.get(c) == Some(&'_') {
                c += 1;
                while c < chars.len() && (chars[c].is_alphanumeric() || chars[c] == '_') {
                    c += 1;
                }
            }
            out.push(Token { tok: Tok::Sym(name), line: n, col: c + 1 - text.chars().count() });
        }
        out.push(Token { tok: Tok::Newline, line: n, col: chars.len() + 1 });
    }
    Ok(out)
}
