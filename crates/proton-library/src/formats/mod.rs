//! Kinds of file a course leaves behind besides notes and PDFs, read into
//! words a library can cut into passages: Jupyter notebooks, LaTeX, and the
//! captions beside a lecture's video.

pub mod captions;
pub mod ipynb;
pub mod tex;
