//! Built-in `.devcontainer/devcontainer.json` templates and project detection.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub content: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "rust",
        label: "Rust",
        description: "Official Rust image with rust-analyzer & lldb ready",
        content: r#"{
  "name": "Rust Dev Container",
  "image": "mcr.microsoft.com/devcontainers/rust:1-bookworm",
  "customizations": {
    "proton": {
      "shell": "/bin/bash"
    }
  },
  "postCreateCommand": "rustc --version && cargo --version"
}
"#,
    },
    Template {
        id: "python",
        label: "Python 3",
        description: "Python 3.12 with pip, pytest & pyright-langserver",
        content: r#"{
  "name": "Python 3 Dev Container",
  "image": "mcr.microsoft.com/devcontainers/python:3.12-bookworm",
  "postCreateCommand": "python3 --version && ([ -f requirements.txt ] && pip install -r requirements.txt || true)"
}
"#,
    },
    Template {
        id: "node",
        label: "Node.js & TypeScript",
        description: "Node.js 22 LTS with TypeScript language server",
        content: r#"{
  "name": "Node.js & TypeScript",
  "image": "mcr.microsoft.com/devcontainers/javascript-node:22-bookworm",
  "forwardPorts": [3000, 5173],
  "postCreateCommand": "([ -f package.json ] && npm install || true)"
}
"#,
    },
    Template {
        id: "cpp",
        label: "C / C++",
        description: "GCC/Clang, CMake, clangd, clang-format & gdb",
        content: r#"{
  "name": "C/C++ Dev Container",
  "image": "mcr.microsoft.com/devcontainers/cpp:bookworm",
  "postCreateCommand": "g++ --version && cmake --version"
}
"#,
    },
    Template {
        id: "go",
        label: "Go",
        description: "Go 1.23 with gopls and Delve",
        content: r#"{
  "name": "Go Dev Container",
  "image": "mcr.microsoft.com/devcontainers/go:1.23-bookworm",
  "postCreateCommand": "go version"
}
"#,
    },
    Template {
        id: "base",
        label: "Ubuntu / Base Linux",
        description: "Clean Debian Bookworm container with git and common utilities",
        content: r#"{
  "name": "Base Linux Dev Container",
  "image": "mcr.microsoft.com/devcontainers/base:bookworm",
  "remoteUser": "vscode"
}
"#,
    },
];

/// Suggest the best-matching template id for a workspace based on project files.
pub fn suggest(root: &Path) -> &'static str {
    if root.join("Cargo.toml").is_file() {
        "rust"
    } else if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
        "python"
    } else if root.join("package.json").is_file() {
        "node"
    } else if root.join("go.mod").is_file() {
        "go"
    } else if root.join("CMakeLists.txt").is_file() || root.join("Makefile").is_file() {
        "cpp"
    } else {
        "base"
    }
}

/// Find a template by id.
pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

/// Write `.devcontainer/devcontainer.json` for `id`, refusing to overwrite an
/// existing configuration file.
pub fn write(root: &Path, id: &str) -> Result<PathBuf, String> {
    let tpl = find(id).ok_or_else(|| format!("unknown dev container template: {id}"))?;
    let dir = root.join(".devcontainer");
    let file = dir.join("devcontainer.json");
    if file.exists() {
        return Err(format!("{} already exists", file.display()));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    std::fs::write(&file, tpl.content)
        .map_err(|e| format!("could not write {}: {e}", file.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonc;

    #[test]
    fn all_templates_parse_as_valid_devcontainer_configs() {
        for tpl in TEMPLATES {
            let stripped = jsonc::strip(tpl.content);
            let parsed: crate::config::DevContainerConfig =
                serde_json::from_str(&stripped).unwrap_or_else(|e| {
                    panic!("template {} failed to parse: {e}", tpl.id);
                });
            assert!(parsed.image.is_some(), "template {} needs image", tpl.id);
        }
    }

    #[test]
    fn suggests_template_from_project_markers_and_refuses_overwrite() {
        let dir = std::env::temp_dir().join(format!("proton-tpl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(suggest(&dir), "base");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        assert_eq!(suggest(&dir), "rust");

        let created = write(&dir, "rust").unwrap();
        assert!(created.is_file());
        let err = write(&dir, "rust").unwrap_err();
        assert!(err.contains("already exists"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
