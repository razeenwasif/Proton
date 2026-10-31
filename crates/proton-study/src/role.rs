//! What a source is to a course, guessed from its name.
//!
//! A course's files say what they are in their names — `Exam_2020.pdf`,
//! `Assignment 2 solutions.pdf`, `Week 3 Lecture.pdf` — and an exam built
//! from past papers needs to know which are which, and to keep solutions
//! back. The guess is only a start: the person corrects it.

use proton_library::Kind;

use crate::model::Role;

/// The role a file's path under its root and its kind suggest: the folders
/// say as much as the name — `Past papers/2019.pdf`.
pub fn guess(name: &str, kind: Kind) -> Role {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let has = |want: &[&str]| words.iter().any(|w| want.contains(w));
    let joined = |want: &[&str]| want.iter().any(|w| lower.contains(w));

    // Answers first: `Exam 2020 solutions` is a solution, not an exam.
    if has(&["solution", "solutions", "sol", "sols", "answers", "answer", "memo", "marking"]) || joined(&["_sol.", "-sol.", "answerkey", "answer key", "worked"]) {
        return Role::Solution;
    }
    // A university id in the name is one's own submission: `u1234567_A2.pdf`.
    if words.iter().any(|w| w.len() == 8 && w.starts_with('u') && w[1..].bytes().all(|b| b.is_ascii_digit())) {
        return Role::OwnWork;
    }
    // Not "paper" or "final" alone: a folder of papers to read is not past
    // papers, and a final report is not an exam.
    if has(&["exam", "exams", "midterm", "midsem"]) || joined(&["mid-sem", "mid-term", "pastpaper", "past paper", "practice exam", "sample exam"]) {
        return Role::PastPaper;
    }
    if has(&["assignment", "assignments", "assn", "hw", "homework", "project", "spec"]) {
        return Role::Assignment;
    }
    if has(&["lab", "labs", "tutorial", "tutorials", "tut", "workshop", "prac", "practical", "exercise", "exercises"]) || kind == Kind::Notebook {
        return Role::Lab;
    }
    if has(&["lecture", "lectures", "lec", "slides", "slide", "week", "wk"]) && matches!(kind, Kind::Pdf | Kind::Slides) {
        return Role::Slides;
    }
    match kind {
        Kind::Markdown | Kind::Text => Role::Notes,
        Kind::Captions | Kind::Video | Kind::Audio => Role::Recording,
        Kind::Pdf | Kind::Document | Kind::Html | Kind::Tex => Role::Reading,
        Kind::Slides => Role::Slides,
        _ => Role::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_course_s_files_say_what_they_are() {
        let cases = [
            ("Exam_2020.pdf", Kind::Pdf, Role::PastPaper),
            ("COMP3670 Final Exam 2019 Solutions.pdf", Kind::Pdf, Role::Solution),
            ("exam2021_sol.pdf", Kind::Pdf, Role::Solution),
            ("Mid-sem practice.pdf", Kind::Pdf, Role::PastPaper),
            ("u7654321_assignment2.pdf", Kind::Pdf, Role::OwnWork),
            ("Assignment 2.pdf", Kind::Pdf, Role::Assignment),
            ("Lab 3 - Search.ipynb", Kind::Notebook, Role::Lab),
            ("lab4.ipynb", Kind::Notebook, Role::Lab),
            ("Week 3 Lecture - Informed search.pdf", Kind::Pdf, Role::Slides),
            ("Russell & Norvig ch 3.pdf", Kind::Pdf, Role::Reading),
            ("Attention.md", Kind::Markdown, Role::Notes),
            ("Lecture 4.vtt", Kind::Captions, Role::Recording),
            ("helper.py", Kind::Code, Role::Other),
            ("COMP3670/Past papers/2019.pdf", Kind::Pdf, Role::PastPaper),
            ("Readings/Papers/Attention.pdf", Kind::Pdf, Role::Reading),
            ("Final Report.pdf", Kind::Pdf, Role::Reading),
        ];
        for (name, kind, role) in cases {
            assert_eq!(guess(name, kind), role, "{name}");
        }
    }
}
