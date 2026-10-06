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
    /// Word, OpenDocument, RTF, EPUB: read through pandoc.
    Document,
    Slides,
    Image,
    Audio,
    Video,
    /// Source code: added to a notebook by hand, never searched.
    Code,
    /// A page of the web, kept as Markdown: never a file of the knowledge base.
    Web,
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

    /// Whether its words are read straight from the file, here and now —
    /// in a search of the whole knowledge base too.
    pub fn readable(self) -> bool {
        matches!(
            self,
            Kind::Markdown | Kind::Text | Kind::Pdf | Kind::Notebook | Kind::Tex | Kind::Captions | Kind::Table | Kind::Code
        )
    }

    /// Whether it is read only when it is made a source, through a program
    /// the machine has: a Word document through pandoc, a PowerPoint's
    /// slides through unzip, a picture's words through tesseract. Never in a
    /// search of the whole knowledge base, where reading thousands would
    /// take hours.
    pub fn converted(self) -> bool {
        matches!(self, Kind::Document | Kind::Slides | Kind::Image)
    }

    /// Whether it is a recording: added as one, and read only when it is
    /// transcribed, as the person chooses.
    pub fn recording(self) -> bool {
        matches!(self, Kind::Audio | Kind::Video)
    }

    /// Whether it can be made a source at all.
    pub fn addable(self) -> bool {
        self.readable() || self.converted() || self.recording()
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
    /// capped by [`crate::pdf::LARGEST`]. A Word document or a deck is mostly
    /// its pictures too, and only its text is read; a photograph of a
    /// whiteboard is a few megabytes.
    pub fn largest(self) -> u64 {
        match self {
            Kind::Pdf => crate::pdf::LARGEST,
            Kind::Notebook => 32 << 20,
            Kind::Captions => 4 << 20,
            Kind::Document => 128 << 20,
            Kind::Slides => 512 << 20,
            Kind::Image => 32 << 20,
            // Only a recording's sound is sent to be heard, and it is never
            // read whole.
            Kind::Audio => 4 << 30,
            Kind::Video => 32 << 30,
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
            Kind::Web => "web",
            Kind::Other => "other",
        }
    }
}

/// Every kind, for reading one back by name.
const ALL: [Kind; 15] = [
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
    Kind::Web,
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
        assert!(!Kind::Slides.readable() && Kind::Slides.converted(), "read when it is made a source, never searched");
        assert!(!Kind::Video.converted());
    }
}
