//! What a citation names: a DOI, an arXiv paper, or one of the
//! investigation's own papers (`P003`) — read out of whatever an agent or a
//! person wrote, and keyed so that one paper is one paper however it was
//! spelled: `10.1364/JOSAA.18.000412` and `https://doi.org/10.1364/josaa.18.000412`,
//! `arXiv:1706.03762v5` and `arxiv.org/pdf/1706.03762`.
//!
//! The one definition of a paper's identity: the literature merges search
//! results by these keys, numbers papers so as not to add one twice, and
//! checks what the team cites against them.

/// A paper, as a citation names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ident {
    /// A DOI, without a resolver in front of it, as it was written.
    Doi(String),
    /// An arXiv id without its version: `1706.03762`, `hep-th/9901001`.
    Arxiv(String),
    /// One of the investigation's papers: `P003`.
    Paper(String),
}

impl Ident {
    /// One identifier as somebody typed it: a DOI (`10.…`, `doi:…`, a
    /// doi.org link), an arXiv id (`arXiv:…`, a bare new-style or old-style
    /// id, an arxiv.org link) or a paper of the investigation's.
    pub fn parse(text: &str) -> Option<Ident> {
        let text = text.trim().trim_matches(|c| matches!(c, '<' | '>'));
        if is_paper(text) {
            return Some(Ident::Paper(text.to_string()));
        }
        let lower = text.to_ascii_lowercase();
        if let Some(rest) = strip_url(&lower).and_then(|rest| rest.strip_prefix("doi.org/").or_else(|| rest.strip_prefix("dx.doi.org/"))) {
            let start = text.len() - rest.len();
            return doi(&decode(&text[start..]));
        }
        if let Some(rest) = strip_url(&lower).and_then(arxiv_path) {
            let start = text.len() - rest.len();
            return arxiv(&text[start..]).map(Ident::Arxiv);
        }
        if let Some(rest) = lower.strip_prefix("doi:") {
            return doi(text[text.len() - rest.len()..].trim());
        }
        if let Some(rest) = lower.strip_prefix("arxiv:") {
            return arxiv(text[text.len() - rest.len()..].trim()).map(Ident::Arxiv);
        }
        if text.starts_with("10.") {
            return doi(text);
        }
        arxiv(text).map(Ident::Arxiv)
    }

    /// What two spellings of one paper share: a DOI in lower case, an arXiv
    /// id without its version, a paper's own id.
    pub fn key(&self) -> String {
        match self {
            Ident::Doi(doi) => format!("doi:{}", doi.to_ascii_lowercase()),
            Ident::Arxiv(id) => format!("arxiv:{}", id.to_ascii_lowercase()),
            Ident::Paper(id) => format!("paper:{id}"),
        }
    }

    /// Where it can be read about: doi.org's resolver, arXiv's abstract page.
    pub fn url(&self) -> Option<String> {
        match self {
            Ident::Doi(doi) => Some(format!("https://doi.org/{doi}")),
            Ident::Arxiv(id) => Some(format!("https://arxiv.org/abs/{id}")),
            Ident::Paper(_) => None,
        }
    }

    /// As it is written back: `doi:10.…`, `arXiv:1706.03762`, `P003`.
    pub fn text(&self) -> String {
        match self {
            Ident::Doi(doi) => format!("doi:{doi}"),
            Ident::Arxiv(id) => format!("arXiv:{id}"),
            Ident::Paper(id) => id.clone(),
        }
    }
}

/// Every identifier in a piece of free text, in order, each with the text it
/// was read from. A bare number like `1706.03762` counts as arXiv's only
/// when "arXiv" or an arxiv.org link says so: in prose it is as likely a
/// measurement.
pub fn extract(text: &str) -> Vec<(String, Ident)> {
    let mut out: Vec<(String, Ident)> = Vec::new();
    let mut push = |raw: &str, ident: Ident| {
        if !out.iter().any(|(_, seen)| seen.key() == ident.key()) {
            out.push((raw.to_string(), ident));
        }
    };
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let mut at = 0;
    while at < text.len() {
        let boundary = at == 0 || !(bytes[at - 1] as char).is_ascii_alphanumeric();
        let rest = &text[at..];
        let rest_lower = &lower[at..];
        // A DOI: `10.` and a registrant, wherever it stands — after `doi:`,
        // in a doi.org link, in brackets.
        if boundary && rest.starts_with("10.") {
            if let Some((len, ident)) = doi_at(rest) {
                push(&rest[..len], ident);
                at += len;
                continue;
            }
        }
        // An arXiv id: after "arXiv", or in an arxiv.org link.
        if boundary && rest_lower.starts_with("arxiv") {
            let after = rest_lower["arxiv".len()..].trim_start_matches([':', ' ']);
            let skip = rest.len() - after.len();
            if let Some(len) = arxiv_len(&rest[skip..]) {
                if let Some(id) = arxiv(&rest[skip..skip + len]) {
                    push(&rest[..skip + len], Ident::Arxiv(id));
                    at += skip + len;
                    continue;
                }
            }
        }
        if let Some(path) = arxiv_path(rest_lower) {
            let skip = rest.len() - path.len();
            if let Some(len) = arxiv_len(&rest[skip..]) {
                if let Some(id) = arxiv(&rest[skip..skip + len]) {
                    push(&rest[..skip + len], Ident::Arxiv(id));
                    at += skip + len;
                    continue;
                }
            }
        }
        // One of the investigation's papers, as a word of its own: `P003`,
        // `[[literature/P003.pdf p. 4]]`.
        if boundary && rest.starts_with('P') {
            let digits = rest[1..].bytes().take_while(u8::is_ascii_digit).count();
            let end = 1 + digits;
            let after_ok = rest.as_bytes().get(end).is_none_or(|b| !(*b as char).is_ascii_alphanumeric());
            if digits >= 3 && after_ok {
                push(&rest[..end], Ident::Paper(rest[..end].to_string()));
                at += end;
                continue;
            }
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

fn is_paper(text: &str) -> bool {
    text.len() >= 4 && text.starts_with('P') && text[1..].bytes().all(|b| b.is_ascii_digit())
}

/// What follows `http(s)://` and `www.`, when there is one.
fn strip_url(lower: &str) -> Option<&str> {
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    Some(rest.strip_prefix("www.").unwrap_or(rest))
}

/// What follows an arxiv.org link's `/abs/` or `/pdf/`, anywhere in `lower`'s start.
fn arxiv_path(lower: &str) -> Option<&str> {
    let rest = strip_url(lower).unwrap_or(lower);
    let rest = rest.strip_prefix("export.").unwrap_or(rest);
    rest.strip_prefix("arxiv.org/abs/").or_else(|| rest.strip_prefix("arxiv.org/pdf/"))
}

/// A DOI's text checked: `10.`, a registrant of digits (and dots), a slash,
/// a suffix. arXiv's own DOIs name an arXiv paper.
fn doi(text: &str) -> Option<Ident> {
    let text = text.trim();
    let (registrant, suffix) = text.strip_prefix("10.")?.split_once('/')?;
    let registrant_ok = registrant.len() >= 4 && registrant.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if !registrant_ok || suffix.is_empty() || suffix.contains(char::is_whitespace) {
        return None;
    }
    if registrant == "48550" {
        if let Some(id) = suffix.strip_prefix("arXiv.").or_else(|| suffix.strip_prefix("ARXIV.")).and_then(arxiv) {
            return Some(Ident::Arxiv(id));
        }
    }
    Some(Ident::Doi(text.to_string()))
}

/// The length of a DOI at the start of `text`, with the punctuation that
/// ends a sentence or a bracket trimmed off it.
fn doi_at(text: &str) -> Option<(usize, Ident)> {
    let len = text
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '[' | ']' | '{' | '}' | '|' | '\\' | '`'))
        .map_or(text.len(), |(i, _)| i);
    let mut candidate = &text[..len];
    loop {
        let trimmed = candidate.trim_end_matches(['.', ',', ';', ':']);
        // A closing bracket is the DOI's only when it opened one too:
        // `10.1002/(SICI)1097-…` keeps its, `(see 10.1/x)` does not.
        let trimmed = match trimmed.strip_suffix(')') {
            Some(inner) if inner.matches('(').count() < inner.matches(')').count() + 1 => inner,
            _ => trimmed,
        };
        if trimmed.len() == candidate.len() {
            break;
        }
        candidate = trimmed;
    }
    let ident = doi(candidate)?;
    Some((candidate.len(), ident))
}

/// An arXiv id with its version removed: new style `YYMM.NNNNN`, or old
/// style `archive(.SUBJ)/YYMMNNN`. A `.pdf` on the end, from a link, goes too.
fn arxiv(text: &str) -> Option<String> {
    let text = text.trim().trim_end_matches(".pdf");
    let id = strip_version(text);
    let new_style = id.split_once('.').is_some_and(|(yymm, n)| {
        yymm.len() == 4 && yymm.bytes().all(|b| b.is_ascii_digit()) && (4..=5).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit())
    });
    let old_style = id.split_once('/').is_some_and(|(archive, n)| {
        let name = archive.split('.').next().unwrap_or("");
        !name.is_empty()
            && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
            && archive.split('.').nth(1).is_none_or(|subj| subj.len() == 2 && subj.bytes().all(|b| b.is_ascii_uppercase()))
            && n.len() == 7
            && n.bytes().all(|b| b.is_ascii_digit())
    });
    (new_style || old_style).then(|| id.to_string())
}

fn strip_version(id: &str) -> &str {
    match id.rfind('v') {
        Some(at) if at > 0 && at + 1 < id.len() && id[at + 1..].bytes().all(|b| b.is_ascii_digit()) => &id[..at],
        _ => id,
    }
}

/// How long the arXiv id at the start of `text` is, version and all.
fn arxiv_len(text: &str) -> Option<usize> {
    let len = text
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-')))
        .map_or(text.len(), |(i, _)| i);
    let candidate = text[..len].trim_end_matches('.');
    let candidate = candidate.strip_suffix(".pdf").unwrap_or(candidate);
    (!candidate.is_empty()).then_some(candidate.len())
}

/// `%2F` and its kind decoded, as doi.org links write a DOI's slashes and brackets.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(text.get(i + 1..i + 3).unwrap_or(""), 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doi_of(s: &str) -> Ident {
        Ident::Doi(s.into())
    }

    fn arxiv_of(s: &str) -> Ident {
        Ident::Arxiv(s.into())
    }

    #[test]
    fn one_identifier_is_read_however_it_was_typed() {
        let cases: [(&str, Option<Ident>); 16] = [
            ("10.1364/JOSAA.18.000412", Some(doi_of("10.1364/JOSAA.18.000412"))),
            ("doi:10.1119/1.2341808", Some(doi_of("10.1119/1.2341808"))),
            ("DOI: 10.1119/1.2341808", Some(doi_of("10.1119/1.2341808"))),
            ("https://doi.org/10.1175/1520-0426(1999)016%3C1854:ORODC%3E2.0.CO;2", Some(doi_of("10.1175/1520-0426(1999)016<1854:ORODC>2.0.CO;2"))),
            ("http://dx.doi.org/10.1103/PhysRevLett.116.061102", Some(doi_of("10.1103/PhysRevLett.116.061102"))),
            ("10.48550/arXiv.1706.03762", Some(arxiv_of("1706.03762"))),
            ("arXiv:1706.03762v5", Some(arxiv_of("1706.03762"))),
            ("1706.03762", Some(arxiv_of("1706.03762"))),
            ("2301.00001", Some(arxiv_of("2301.00001"))),
            ("https://arxiv.org/abs/2106.09685v2", Some(arxiv_of("2106.09685"))),
            ("https://arxiv.org/pdf/2106.09685.pdf", Some(arxiv_of("2106.09685"))),
            ("hep-th/9901001", Some(arxiv_of("hep-th/9901001"))),
            ("math.GT/0309136v2", Some(arxiv_of("math.GT/0309136"))),
            ("P003", Some(Ident::Paper("P003".into()))),
            ("10.1000", None),
            ("Rayleigh (1871)", None),
        ];
        for (text, want) in cases {
            assert_eq!(Ident::parse(text), want, "{text}");
        }
    }

    #[test]
    fn keys_make_one_paper_of_its_spellings() {
        let a = Ident::parse("10.1364/JOSAA.18.000412").unwrap();
        let b = Ident::parse("https://doi.org/10.1364/josaa.18.000412").unwrap();
        assert_eq!(a.key(), b.key());
        assert_eq!(Ident::parse("arXiv:1706.03762v5").unwrap().key(), Ident::parse("https://arxiv.org/pdf/1706.03762").unwrap().key());
        assert_ne!(a.key(), Ident::parse("10.1364/JOSAA.18.000413").unwrap().key());
        assert_eq!(a.url().as_deref(), Some("https://doi.org/10.1364/JOSAA.18.000412"));
        assert_eq!(Ident::Arxiv("1706.03762".into()).url().as_deref(), Some("https://arxiv.org/abs/1706.03762"));
        assert_eq!(Ident::Paper("P003".into()).url(), None);
        assert_eq!(Ident::Arxiv("1706.03762".into()).text(), "arXiv:1706.03762");
    }

    #[test]
    fn identifiers_are_found_in_free_text_and_their_punctuation_left_behind() {
        let found = |text: &str| extract(text).into_iter().map(|(_, ident)| ident).collect::<Vec<_>>();
        assert_eq!(
            found("Hernández-Andrés et al. (2001), DOI: 10.1364/JOSAA.18.000412."),
            [doi_of("10.1364/JOSAA.18.000412")],
            "the full stop ends the sentence, not the DOI"
        );
        assert_eq!(found("(see 10.1119/1.2341808)"), [doi_of("10.1119/1.2341808")]);
        assert_eq!(found("10.1002/(SICI)1097-0142(19990101)85:1<1::AID-CNCR1>3.0.CO;2-4"), [doi_of("10.1002/(SICI)1097-0142(19990101)85:1")], "a DOI's own brackets stay");
        assert_eq!(found("Vaswani et al., arXiv:1706.03762v5; also arXiv 2106.09685"), [arxiv_of("1706.03762"), arxiv_of("2106.09685")]);
        assert_eq!(found("https://arxiv.org/abs/hep-th/9901001."), [arxiv_of("hep-th/9901001")]);
        assert_eq!(found("A peak at 1706.03762 nm is not a paper."), Vec::<Ident>::new(), "a bare number is a measurement");
        assert_eq!(found("As [[literature/P003.pdf p. 4]] shows, and P012 agrees"), [Ident::Paper("P003".into()), Ident::Paper("P012".into())]);
        assert_eq!(found("MP003, P12 and Pa001 name nothing"), Vec::<Ident>::new());
        assert_eq!(found("doi:10.1119/1.2341808 and again https://doi.org/10.1119/1.2341808"), [doi_of("10.1119/1.2341808")], "once");
        assert_eq!(found("version v1.2.3 and the year 2020.1234"), Vec::<Ident>::new());
        let raw: Vec<String> = extract("cf. arXiv:1706.03762v5.").into_iter().map(|(raw, _)| raw).collect();
        assert_eq!(raw, ["arXiv:1706.03762v5"], "what it was read from, as written");
    }
}
