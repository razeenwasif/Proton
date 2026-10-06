//! What a file is, by its name: what can be read of it, and how it is cited.

use std::path::Path;

/// What a file is, from its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Markdown,
    Text,
    Pdf,
    /// A Jupyter notebook.
    Notebook,
    Tex,
    /// Subtitles beside a video: SRT or WebVTT.
    Captions,
    /// CSV or TSV.
    Table,
    /// Word, OpenDocument, RTF, EPUB: read later, through pandoc.
    Document,
    Slides,
    Image,
    Audio,
    Video,
    /// Source code: added to a notebook by hand, never searched.
    Code,
    Other,
}

impl Kind {
    pub fn of(path: &Path) -> Kind {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return Kind::Other;
        };
        match ext.to_ascii_lowercase().as_str() {
            "md" | "markdown" | "mdx" => Kind::Markdown,
            "txt" | "text" => Kind::Text,
            "pdf" => Kind::Pdf,
            "ipynb" => Kind::Notebook,
            "tex" | "ltx" => Kind::Tex,
            "srt" | "vtt" => Kind::Captions,
            "csv" | "tsv" => Kind::Table,
            "docx" | "doc" | "odt" | "rtf" | "epub" | "pages" => Kind::Document,
            "pptx" | "ppt" | "key" | "odp" => Kind::Slides,
            "png" | "jpg" | "jpeg" | "gif" | "heic" | "webp" | "tif" | "tiff" | "bmp" | "svg" => Kind::Image,
            "mp3" | "m4a" | "wav" | "aac" | "flac" | "ogg" | "opus" => Kind::Audio,
            "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => Kind::Video,
            "py" | "c" | "h" | "cpp" | "cc" | "hpp" | "java" | "rs" | "js" | "ts" | "jsx" | "tsx" | "hs" | "ml"
            | "r" | "jl" | "m" | "sql" | "sh" | "go" | "kt" | "swift" | "scala" | "rb" | "php" | "lua" | "s"
            | "asm" | "cu" | "f90" | "pl" | "v" | "vhd" | "lean" | "agda" | "elm" | "ex" | "erl" | "clj" => Kind::Code,
            _ => Kind::Other,
        }
    }

    /// Whether its words can be read here, now.
    pub fn readable(self) -> bool {
        matches!(
            self,
            Kind::Markdown | Kind::Text | Kind::Pdf | Kind::Notebook | Kind::Tex | Kind::Captions | Kind::Table | Kind::Code
        )
    }

    /// Whether a library of the whole knowledge base searches it. Code is
    /// readable but not searched: a course's thousand helper scripts would
    /// drown its notes in every answer.
    pub fn indexed(self) -> bool {
        self.readable() && self != Kind::Code
    }

    /// The most a file of this kind may weigh and still be read.
    ///
    /// A megabyte of prose is a log, not prose. A notebook is mostly its
    /// outputs' pictures, which are dropped, so it may weigh more; a PDF is
    /// capped by [`crate::pdf::LARGEST`].
    pub fn largest(self) -> u64 {
        match self {
            Kind::Pdf => crate::pdf::LARGEST,
            Kind::Notebook => 32 << 20,
            Kind::Captions => 4 << 20,
            _ => 1 << 20,
        }
    }

    /// The kind a name names: [`name`](Self::name) read back.
    pub fn from_name(name: &str) -> Kind {
        ALL.into_iter().find(|kind| kind.name() == name).unwrap_or(Kind::Other)
    }

    /// Its name in a settings file or a view.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Markdown => "markdown",
            Kind::Text => "text",
            Kind::Pdf => "pdf",
            Kind::Notebook => "notebook",
            Kind::Tex => "tex",
            Kind::Captions => "captions",
            Kind::Table => "table",
            Kind::Document => "document",
            Kind::Slides => "slides",
            Kind::Image => "image",
            Kind::Audio => "audio",
            Kind::Video => "video",
            Kind::Code => "code",
            Kind::Other => "other",
        }
    }
}

/// Every kind, for reading one back by name.
const ALL: [Kind; 14] = [
    Kind::Markdown,
    Kind::Text,
    Kind::Pdf,
    Kind::Notebook,
    Kind::Tex,
    Kind::Captions,
    Kind::Table,
    Kind::Document,
    Kind::Slides,
    Kind::Image,
    Kind::Audio,
    Kind::Video,
    Kind::Code,
    Kind::Other,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_reads_back_from_its_name() {
        for kind in ALL {
            assert_eq!(Kind::from_name(kind.name()), kind);
        }
        assert_eq!(Kind::from_name("hologram"), Kind::Other);
    }

    #[test]
    fn a_file_is_known_by_its_extension_whatever_its_case() {
        assert_eq!(Kind::of(Path::new("Week 3/Lecture.PDF")), Kind::Pdf);
        assert_eq!(Kind::of(Path::new("lab.ipynb")), Kind::Notebook);
        assert_eq!(Kind::of(Path::new("talk.en.vtt")), Kind::Captions);
        assert_eq!(Kind::of(Path::new("Makefile")), Kind::Other);
        assert_eq!(Kind::of(Path::new(".hidden")), Kind::Other);
    }

    #[test]
    fn code_is_read_but_not_searched() {
        assert!(Kind::Code.readable());
        assert!(!Kind::Code.indexed());
        assert!(Kind::Notebook.indexed());
        assert!(!Kind::Slides.readable(), "not until pandoc reads them");
    }
}
