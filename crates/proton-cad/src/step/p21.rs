//! ISO 10303-21, the text STEP files are written in: a header, then
//! numbered instances `#12=CARTESIAN_POINT('',(0.,1.,2.));`, each a type and
//! its parameters — or several types at once, `#7=(LENGTH_UNIT() NAMED_UNIT(*)
//! SI_UNIT(.MILLI.,.METRE.));`, a complex instance.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Int(i64),
    Real(f64),
    Str(String),
    /// `.T.`, `.MILLI.`: the word between the dots.
    Enum(String),
    Ref(u64),
    List(Vec<Param>),
    /// `$`: not given.
    Unset,
    /// `*`: worked out by a supertype.
    Derived,
    /// `LENGTH_MEASURE(1.E-06)`: a value of a named type.
    Typed(String, Box<Param>),
}

impl Param {
    pub fn real(&self) -> Option<f64> {
        match self {
            Param::Real(x) => Some(*x),
            Param::Int(i) => Some(*i as f64),
            Param::Typed(_, p) => p.real(),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Param::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn reference(&self) -> Option<u64> {
        match self {
            Param::Ref(r) => Some(*r),
            _ => None,
        }
    }

    pub fn list(&self) -> Option<&[Param]> {
        match self {
            Param::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Param::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn word(&self) -> Option<&str> {
        match self {
            Param::Enum(s) => Some(s),
            _ => None,
        }
    }

    pub fn flag(&self) -> Option<bool> {
        match self.word()? {
            "T" => Some(true),
            "F" => Some(false),
            _ => None,
        }
    }
}

/// An instance: one type and its parameters, or several.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub parts: Vec<(String, Vec<Param>)>,
}

impl Instance {
    /// The parameters of its part of type `ty`, if it has one.
    pub fn part(&self, ty: &str) -> Option<&[Param]> {
        self.parts.iter().find(|(t, _)| t == ty).map(|(_, p)| p.as_slice())
    }

    /// Its type, when it has one only.
    pub fn simple(&self) -> Option<(&str, &[Param])> {
        match self.parts.as_slice() {
            [(t, p)] => Some((t, p)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct File {
    pub schema: Vec<String>,
    pub instances: HashMap<u64, Instance>,
    /// Their numbers in the order the file has them.
    pub order: Vec<u64>,
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
    line: usize,
}

impl Reader<'_> {
    fn err(&self, what: impl Into<String>) -> String {
        format!("line {}: {}", self.line, what.into())
    }

    /// Past whitespace and `/* comments */`.
    fn skip(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                if self.s[self.i] == b'\n' {
                    self.line += 1;
                }
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"/*") {
                match self.s[self.i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(k) => {
                        self.line += self.s[self.i..self.i + 2 + k].iter().filter(|&&c| c == b'\n').count();
                        self.i += k + 4;
                    }
                    None => self.i = self.s.len(),
                }
                continue;
            }
            return;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(format!("expected `{}`", c as char)))
        }
    }

    fn keyword(&mut self) -> Result<String, String> {
        self.skip();
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_' || self.s[self.i] == b'-') {
            self.i += 1;
        }
        if start == self.i {
            return Err(self.err("expected a name"));
        }
        Ok(String::from_utf8_lossy(&self.s[start..self.i]).to_ascii_uppercase())
    }

    fn number(&mut self) -> Result<u64, String> {
        self.skip();
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|x| x.parse().ok()).ok_or_else(|| self.err("expected a number"))
    }

    fn string(&mut self) -> Result<String, String> {
        // `'it''s'`: a quote doubled is one; `\X2\…\X0\` is UTF-16 in hex.
        self.i += 1;
        let mut raw = Vec::new();
        loop {
            match self.s.get(self.i) {
                None => return Err(self.err("a string with no end")),
                Some(b'\'') if self.s.get(self.i + 1) == Some(&b'\'') => {
                    raw.push(b'\'');
                    self.i += 2;
                }
                Some(b'\'') => {
                    self.i += 1;
                    break;
                }
                Some(&c) => {
                    if c == b'\n' {
                        self.line += 1;
                    }
                    raw.push(c);
                    self.i += 1;
                }
            }
        }
        Ok(decode(&String::from_utf8_lossy(&raw)))
    }

    fn param(&mut self) -> Result<Param, String> {
        match self.peek() {
            Some(b'$') => {
                self.i += 1;
                Ok(Param::Unset)
            }
            Some(b'*') => {
                self.i += 1;
                Ok(Param::Derived)
            }
            Some(b'#') => {
                self.i += 1;
                Ok(Param::Ref(self.number()?))
            }
            Some(b'\'') => Ok(Param::Str(self.string()?)),
            Some(b'.') => {
                self.i += 1;
                let w = self.keyword()?;
                self.expect(b'.')?;
                Ok(Param::Enum(w))
            }
            Some(b'(') => {
                self.i += 1;
                let mut items = Vec::new();
                if self.peek() == Some(b')') {
                    self.i += 1;
                    return Ok(Param::List(items));
                }
                loop {
                    items.push(self.param()?);
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b')') => {
                            self.i += 1;
                            return Ok(Param::List(items));
                        }
                        _ => return Err(self.err("expected `,` or `)` in a list")),
                    }
                }
            }
            Some(b'"') => {
                // Binary: kept as the hex it is written in.
                self.i += 1;
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i] != b'"' {
                    self.i += 1;
                }
                let hex = String::from_utf8_lossy(&self.s[start..self.i]).to_string();
                self.i += 1;
                Ok(Param::Str(hex))
            }
            Some(c) if c == b'-' || c == b'+' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || b".eE+-".contains(&self.s[self.i])) {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                if text.contains(['.', 'e', 'E']) {
                    // `1.E-06`: Rust wants a digit after the point, or none.
                    let fixed = text.replace(".E", ".0E").replace(".e", ".0e");
                    let fixed = if fixed.ends_with('.') { format!("{fixed}0") } else { fixed };
                    fixed.parse().map(Param::Real).map_err(|_| self.err(format!("`{text}` is not a number")))
                } else {
                    text.parse().map(Param::Int).map_err(|_| self.err(format!("`{text}` is not a number")))
                }
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let name = self.keyword()?;
                self.expect(b'(')?;
                let inner = self.param()?;
                self.expect(b')')?;
                Ok(Param::Typed(name, Box::new(inner)))
            }
            _ => Err(self.err("expected a parameter")),
        }
    }

    /// `NAME(params)`.
    fn part(&mut self) -> Result<(String, Vec<Param>), String> {
        let name = self.keyword()?;
        self.expect(b'(')?;
        let mut params = Vec::new();
        if self.peek() == Some(b')') {
            self.i += 1;
            return Ok((name, params));
        }
        loop {
            params.push(self.param()?);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    return Ok((name, params));
                }
                _ => return Err(self.err(format!("expected `,` or `)` in `{name}`"))),
            }
        }
    }
}

/// `\X2\00E9\X0\` and `\X\E9` decoded; the rest as it is.
fn decode(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(k) = rest.find('\\') {
        out.push_str(&rest[..k]);
        rest = &rest[k..];
        if let Some(body) = rest.strip_prefix("\\X2\\") {
            let end = body.find("\\X0\\").unwrap_or(body.len());
            let units: Vec<u16> = body.as_bytes()[..end].chunks(4).filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect();
            out.push_str(&String::from_utf16_lossy(&units));
            rest = body.get(end + 4..).unwrap_or("");
        } else if let Some(body) = rest.strip_prefix("\\X\\") {
            if let Some(b) = body.get(..2).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b as char);
            }
            rest = body.get(2..).unwrap_or("");
        } else if let Some(body) = rest.strip_prefix("\\\\") {
            out.push('\\');
            rest = body;
        } else {
            out.push('\\');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// A string as the file writes it: quotes doubled, what is not ASCII as
/// UTF-16 in hex.
pub fn quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                out.push_str("\\X2\\");
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("{u:04X}"));
                }
                out.push_str("\\X0\\");
            }
        }
    }
    out.push('\'');
    out
}

/// A real as the file writes it: every digit it needs to read back the
/// same, and a point always.
pub fn real(x: f64) -> String {
    let x = if x == 0.0 { 0.0 } else { x };
    let s = format!("{x:?}");
    match s.split_once('e') {
        Some((m, e)) => {
            let m = if m.contains('.') { m.to_string() } else { format!("{m}.") };
            format!("{m}E{e}")
        }
        None if s.contains('.') => s,
        None => format!("{s}."),
    }
}

/// A STEP file read: its schema and its instances.
pub fn parse(text: &str) -> Result<File, String> {
    let mut r = Reader { s: text.as_bytes(), i: 0, line: 1 };
    let magic = r.keyword()?;
    if magic != "ISO-10303-21" {
        return Err("not a STEP file: it does not start ISO-10303-21".into());
    }
    r.expect(b';')?;
    let mut file = File::default();
    loop {
        let section = r.keyword()?;
        r.expect(b';')?;
        match section.as_str() {
            "HEADER" => loop {
                let name = r.keyword()?;
                if name == "ENDSEC" {
                    r.expect(b';')?;
                    break;
                }
                r.i -= name.len();
                let (name, params) = r.part()?;
                r.expect(b';')?;
                if name == "FILE_SCHEMA" {
                    if let Some(Param::List(l)) = params.first() {
                        file.schema = l.iter().filter_map(|p| p.text().map(String::from)).collect();
                    }
                }
            },
            "DATA" => loop {
                match r.peek() {
                    Some(b'#') => {
                        r.i += 1;
                        let id = r.number()?;
                        r.expect(b'=')?;
                        let parts = if r.peek() == Some(b'(') {
                            r.i += 1;
                            let mut parts = Vec::new();
                            while r.peek() != Some(b')') {
                                parts.push(r.part()?);
                            }
                            r.i += 1;
                            parts
                        } else {
                            vec![r.part()?]
                        };
                        r.expect(b';')?;
                        file.instances.insert(id, Instance { parts });
                        file.order.push(id);
                    }
                    _ => {
                        let end = r.keyword()?;
                        if end != "ENDSEC" {
                            return Err(r.err(format!("`{end}` in the data")));
                        }
                        r.expect(b';')?;
                        break;
                    }
                }
            },
            "END-ISO-10303-21" => return Ok(file),
            other => return Err(r.err(format!("`{other}` is not a section"))),
        }
        if r.peek().is_none() {
            return Err("the file stops before END-ISO-10303-21".into());
        }
    }
}
