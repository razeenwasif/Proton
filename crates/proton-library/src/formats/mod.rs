//! Kinds of file a course leaves behind besides notes and PDFs, read into
//! words a library can cut into passages: Jupyter notebooks, LaTeX, the
//! captions beside a lecture's video; and, through programs the machine has,
//! Word documents and their kin, PowerPoints, and pictures' words; and pages
//! of the web, and a video's captions.

pub mod captions;
pub mod ipynb;
pub mod office;
pub mod tex;
pub mod web;
