//! A debate between the models you already pay for.
//!
//! ## What this is not
//!
//! Not a port of Council. That project routes seven roles across six
//! providers, and the diversity *is* the product — reproducing it means API
//! keys for models nobody here has a subscription to. What is reachable
//! instead is three genuinely different model families, each already paid for
//! and each already installed as a CLI: Claude, Codex and Antigravity.
//!
//! Three is fewer than seven. It is also three real families rather than one
//! model wearing seven hats, which is the half of the idea that was doing the
//! work.
//!
//! ## Files are the protocol
//!
//! Every round is written to disk as markdown before the next one starts. That
//! is not incidental — it is the same rule the rest of Proton is built on, and
//! here it buys three things a shared context could not: each member is a
//! separate process with no memory of the others, so the only way they can see
//! each other's positions is a file; a debate can be read, edited and resumed
//! halfway through; and the brief at the end is a document you already have
//! rather than a transcript you have to save.
//!
//! ## What each member can see
//!
//! Whatever its own CLI can, which for all three includes the library over
//! MCP. That is what makes this worth doing inside Proton rather than in a
//! shell script: the members argue about the user's actual notes, and cite
//! them.

pub mod debate;
pub mod member;
pub mod running;

pub use debate::{Debate, Said};
pub use member::{Answers, Member, Prompt, Wiring};
pub use running::{held, Held, Running, Update};

use std::path::{Path, PathBuf};

/// Where `proton-mcp` is, so a member can be handed the library: what
/// `PROTON_MCP` names, if it is set; otherwise beside `exe`, the program
/// asking, or where its bundle keeps resources. `None` when it is nowhere,
/// and a council still runs — its members answer from what they know rather
/// than from the person's notes, which is worse and no reason to refuse.
pub fn mcp_server(exe: &Path) -> Option<PathBuf> {
    match std::env::var_os("PROTON_MCP") {
        Some(named) => Some(PathBuf::from(named)).filter(|path| path.is_file()),
        None => mcp_server_near(exe),
    }
}

/// The places a build or a bundle leaves `proton-mcp` beside `exe`: beside it
/// — a build, and Windows' installer; `Contents/Resources` in a Mac's app,
/// which keeps only the app's own program in `Contents/MacOS`; and Linux's
/// `lib/Proton`, under `/usr` or an AppImage's own.
pub fn mcp_server_near(exe: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "proton-mcp.exe" } else { "proton-mcp" };
    let dir = exe.parent()?;
    [dir.join(name), dir.join("../Resources").join(name), dir.join("../lib/Proton").join(name)]
        .into_iter()
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proton_mcp_is_found_where_each_bundle_keeps_it() {
        let root = std::env::temp_dir().join(format!("proton-council-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let name = if cfg!(windows) { "proton-mcp.exe" } else { "proton-mcp" };
        // A build and Windows, beside the program; a Mac's app, which keeps
        // only its own program in MacOS; Linux, under /usr or an AppImage.
        let layouts = [("beside", "bin", "bin"), ("mac", "Proton.app/Contents/MacOS", "Proton.app/Contents/Resources"), ("linux", "usr/bin", "usr/lib/Proton")];
        for (layout, program, server) in layouts {
            let base = root.join(layout);
            std::fs::create_dir_all(base.join(program)).unwrap();
            std::fs::create_dir_all(base.join(server)).unwrap();
            let exe = base.join(program).join("proton-shell");
            std::fs::write(&exe, "").unwrap();
            assert_eq!(mcp_server_near(&exe), None, "{layout}: found before it was there");
            std::fs::write(base.join(server).join(name), "").unwrap();
            let found = mcp_server_near(&exe).unwrap_or_else(|| panic!("{layout}: not found"));
            assert_eq!(found.canonicalize().unwrap(), base.join(server).join(name).canonicalize().unwrap(), "{layout}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
