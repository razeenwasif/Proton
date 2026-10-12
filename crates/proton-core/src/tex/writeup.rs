//! An investigation written up as a paper: `paper/<slug>/` in the project,
//! with a `main.tex` gathered from what Research wrote — the brief, the
//! hypotheses as the tournament ranked them, the experiments, the runs'
//! figures, the verdicts and the overview — and a `references.bib` of its
//! literature, each `P003` in the prose a `\cite` of it.
//!
//! It is a first draft, gathered rather than written: the agent can be
//! asked to write the prose round it, and its edits wait for review.

use std::path::{Path, PathBuf};

use proton_research::model::{HypothesisStatus, Record};
use proton_research::store::Folder;
use proton_tex::bib;
use proton_tex::markdown::to_latex;

/// What a write-up made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenUp {
    pub folder: PathBuf,
    pub main: PathBuf,
    pub figures: usize,
    pub papers: usize,
}

/// Research's record as BibTeX's: the authors past the fifty a record
/// keeps said as `and others`, which BibTeX sets as "et al.".
pub fn record_of(r: &Record) -> bib::Record {
    let mut authors = r.authors.clone();
    if r.authors_more > 0 {
        authors.push("others".into());
    }
    bib::Record {
        title: r.title.clone(),
        authors,
        year: r.year.map(|y| y.to_string()),
        venue: r.venue.clone(),
        doi: r.doi.clone(),
        arxiv: r.arxiv.clone(),
        category: None,
        url: r.url.clone(),
    }
}

/// The text under a brief's `## name`, to the next heading.
fn section(markdown: &str, name: &str) -> String {
    let mut out = Vec::new();
    let mut inside = false;
    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            inside = heading.trim().eq_ignore_ascii_case(name);
            continue;
        }
        if line.starts_with("# ") {
            inside = false;
            continue;
        }
        if inside {
            out.push(line);
        }
    }
    out.join("\n").trim().to_string()
}

/// The first paragraph of Markdown that is prose, not a heading.
fn first_paragraph(markdown: &str) -> String {
    markdown
        .split("\n\n")
        .map(str::trim)
        .find(|p| !p.is_empty() && !p.starts_with('#') && !p.starts_with("**") && !p.starts_with('-') && !p.starts_with('|'))
        .unwrap_or("")
        .to_string()
}

/// The figure formats pdfLaTeX includes.
const FIGURES: &[&str] = &["png", "jpg", "jpeg", "pdf"];

/// Write the investigation up in `root/paper/<slug>/` — or `<slug>-2/`
/// beside an earlier write-up, which is left as it is.
pub fn write_up(investigation: &Folder, root: &Path) -> Result<WrittenUp, String> {
    let slug = investigation.slug();
    let base = root.join("paper");
    let empty = |dir: &Path| std::fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(false);
    let folder = (1..100)
        .map(|n| if n == 1 { base.join(slug) } else { base.join(format!("{slug}-{n}")) })
        .find(|f| !f.exists() || empty(f))
        .ok_or_else(|| format!("{}: too many write-ups already", base.display()))?;
    std::fs::create_dir_all(folder.join("figures")).map_err(|e| format!("{}: {e}", folder.display()))?;

    // The literature first: the prose cites it by key.
    let papers = investigation.papers();
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut references = String::new();
    for paper in &papers {
        let taken: Vec<String> = keys.iter().map(|(_, k)| k.clone()).collect();
        let (key, entry) = bib::entry_for(&record_of(&paper.record), &taken);
        references.push_str(&entry);
        references.push('\n');
        keys.push((paper.id.clone(), key));
    }
    let cite = |id: &str| keys.iter().find(|(i, _)| i == id).map(|(_, k)| k.clone());
    let md = |text: &str, top: usize| to_latex(text, top, &cite);

    let title = investigation.investigation().title.clone();
    let brief = investigation.brief();
    let goal = section(&brief, "Goal");
    let constraints = section(&brief, "Constraints");
    let overview = investigation.overview();
    let verdicts = investigation.verdicts();
    let summary = match first_paragraph(&overview) {
        p if !p.is_empty() => p,
        _ => goal.clone(),
    };

    let mut body = String::new();
    body.push_str("\\section{Introduction}\n\\label{sec:introduction}\n\n");
    body.push_str(&md(if goal.is_empty() { "What this investigation set out to find." } else { &goal }, 2));
    if !constraints.is_empty() {
        body.push_str("\n\\paragraph{Constraints.}\n");
        body.push_str(&md(&constraints, 4));
    }

    // The hypotheses, as the tournament ranked them; the retired left out.
    let board = proton_research::view::Board::read(investigation);
    let hypotheses = investigation.hypotheses();
    let ranked: Vec<_> = board
        .rows()
        .iter()
        .filter_map(|row| hypotheses.iter().find(|h| h.id == row.id))
        .filter(|h| h.status != HypothesisStatus::Retired)
        .cloned()
        .collect();
    if !ranked.is_empty() {
        body.push_str("\n\\section{Hypotheses}\n\\label{sec:hypotheses}\n\n");
        for h in &ranked {
            body.push_str(&format!("\\subsection{{{}}}\n\\label{{hyp:{}}}\n\n", to_latex(&h.title, 2, &cite).trim(), h.id));
            body.push_str(&md(&h.statement, 3));
            if !h.rationale.trim().is_empty() {
                body.push_str("\n\\paragraph{Rationale.}\n");
                body.push_str(&md(&h.rationale, 4));
            }
            if !h.predictions.is_empty() {
                body.push_str("\n\\paragraph{Predictions.}\n");
                body.push_str(&md(&h.predictions.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n"), 4));
            }
            body.push('\n');
        }
    }

    let experiments = investigation.experiments();
    if !experiments.is_empty() {
        body.push_str("\\section{Experiments}\n\\label{sec:experiments}\n\n");
        for e in &experiments {
            let testing = e.hypothesis.as_deref().map(|h| format!(", testing Hypothesis~\\ref{{hyp:{h}}}")).filter(|_| ranked.iter().any(|r| Some(r.id.as_str()) == e.hypothesis.as_deref())).unwrap_or_default();
            body.push_str(&format!("\\subsection{{Experiment {}{testing}}}\n\\label{{exp:{}}}\n\n", e.id, e.id));
            body.push_str(&md(&e.purpose, 3));
            if !e.expectation.trim().is_empty() {
                body.push_str("\n\\paragraph{Expected.}\n");
                body.push_str(&md(&e.expectation, 4));
            }
            body.push('\n');
        }
    }

    // The runs' figures, copied in: the paper must typeset without the
    // investigation beside it.
    let runs = investigation.dir().join("runs");
    let mut figures = 0;
    let mut figure_text = String::new();
    let mut rows = proton_lab::run::rows(&runs);
    rows.reverse();
    for row in rows {
        let Some(detail) = proton_lab::run::detail(&runs.join(&row.id)) else { continue };
        for figure in &detail.figures {
            let from = runs.join(&row.id).join(figure);
            let ext = from.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if !FIGURES.contains(&ext.as_str()) {
                continue;
            }
            let stem = from.file_stem().and_then(|s| s.to_str()).unwrap_or("figure");
            let name = format!("{}-{}.{ext}", row.id, stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect::<String>());
            if std::fs::copy(&from, folder.join("figures").join(&name)).is_err() {
                continue;
            }
            figures += 1;
            let of = [row.experiment.as_deref().map(|e| format!("Experiment {e}")), row.hypothesis.as_deref().map(|h| format!("Hypothesis {h}"))].into_iter().flatten().collect::<Vec<_>>().join(", ");
            let caption = format!("{}, from run {}{}.", stem.replace(['_', '-'], " "), row.id, if of.is_empty() { String::new() } else { format!(" ({of})") });
            figure_text.push_str(&format!(
                "\\begin{{figure}}[ht]\n  \\centering\n  \\includegraphics[width=0.8\\linewidth]{{figures/{name}}}\n  \\caption{{{}}}\n  \\label{{fig:{}}}\n\\end{{figure}}\n\n",
                to_latex(&caption, 2, &cite).trim(),
                name.rsplit_once('.').map_or(name.as_str(), |(n, _)| n)
            ));
        }
    }

    body.push_str("\\section{Results}\n\\label{sec:results}\n\n");
    body.push_str(&figure_text);
    if verdicts.trim().is_empty() {
        body.push_str("What the runs showed.\n");
    } else {
        body.push_str(&md(&verdicts, 2));
    }
    if !overview.trim().is_empty() {
        body.push_str("\n\\section{Discussion}\n\\label{sec:discussion}\n\n");
        body.push_str(&md(&overview, 2));
    }

    let bibliography = if papers.is_empty() { String::new() } else { "\n\\bibliographystyle{plainnat}\n\\bibliography{references}\n".to_string() };
    let main = format!(
        "% Written up by Proton from the investigation {slug}: a first draft, gathered from\n\
         % what Research wrote. Edit it freely; nothing here is written to again.\n\
         \\documentclass[11pt]{{article}}\n\n\
         \\usepackage{{amsmath,amssymb}}\n\
         \\usepackage{{graphicx}}\n\
         \\usepackage{{booktabs}}\n\
         \\usepackage[margin=1in]{{geometry}}\n\
         \\usepackage[numbers]{{natbib}}\n\
         \\usepackage{{hyperref}}\n\n\
         \\title{{{}}}\n\
         \\author{{Your Name}}\n\
         \\date{{\\today}}\n\n\
         \\begin{{document}}\n\n\
         \\maketitle\n\n\
         \\begin{{abstract}}\n{}\\end{{abstract}}\n\n\
         {body}{bibliography}\n\
         \\end{{document}}\n",
        proton_tex::templates::escape(&title),
        md(&summary, 4),
    );
    let main_path = folder.join("main.tex");
    std::fs::write(&main_path, main).map_err(|e| format!("{}: {e}", main_path.display()))?;
    if !papers.is_empty() {
        std::fs::write(folder.join("references.bib"), references).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    if figures == 0 {
        let _ = std::fs::remove_dir(folder.join("figures"));
    }
    Ok(WrittenUp { folder, main: main_path, figures, papers: papers.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brief_s_sections_are_found_by_name() {
        let brief = "# The sky\n\n**Discipline:** Physics\n\n## Goal\n\nWhy is it blue?\n\n## Constraints\n\n- No balloons.\n";
        assert_eq!(section(brief, "Goal"), "Why is it blue?");
        assert_eq!(section(brief, "constraints"), "- No balloons.");
        assert_eq!(first_paragraph("# Overview\n\n**Round 2**\n\nScattering explains it.\n\nMore."), "Scattering explains it.");
    }
}
