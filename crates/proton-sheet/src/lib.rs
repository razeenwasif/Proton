//! Excel mode's workbooks.
//!
//! - [`model`]: a workbook as kept — sheets, cells as typed, styles.
//! - [`folder`]: the `Name.sheet/` folder it is saved as, in text that diffs.
//! - [`formula`]: formulas read into a tree and written back.
//! - [`book`]: the engine — what each formula comes to, worked out again
//!   only where a change reaches.
//! - [`functions`]: the functions formulas call.
//! - [`edit`]: rows and columns put in and taken out, formulas following;
//!   copying and filling.
//! - [`arrange`]: sorting, filters, and conditional colour.
//! - [`numfmt`]: Excel's number formats, for showing a value.
//! - [`xlsx`], [`csv`]: Excel's files and delimited text, opened and saved.
//! - [`refs`], [`value`], [`dates`]: references, values, and dates as
//!   Excel's serial numbers.

pub mod arrange;
pub mod book;
pub mod csv;
pub mod dates;
pub mod edit;
pub mod folder;
pub mod formula;
pub mod functions;
pub mod model;
pub mod numfmt;
pub mod refs;
pub mod value;
pub mod xlsx;
mod xml;
mod zip;

pub use book::Book;
pub use model::{Cell, Sheet, Style, Workbook};
pub use refs::{CellRef, Range};
pub use value::{ErrorKind, Value};
