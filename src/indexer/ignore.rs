use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Heavy build/dependency directories that should never be watched or indexed by default.
pub const DEFAULT_IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".cache",
];

/// Returns true if a path component matches a default ignored directory name.
pub fn is_default_ignored_dir_name(name: &str) -> bool {
    DEFAULT_IGNORED_DIRS.contains(&name)
}

/// Returns true if a file looks like a secret/key/env file that should never be indexed by default.
pub fn is_default_secret_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = file_name.to_lowercase();

    if lower == ".env" || lower.starts_with(".env.") {
        return true;
    }
    if matches!(
        lower.as_str(),
        "id_rsa"
            | "id_dsa"
            | "id_ecdsa"
            | "id_ed25519"
            | ".npmrc"
            | ".netrc"
            | ".pypirc"
            | ".git-credentials"
    ) {
        return true;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        if matches!(
            ext_lower.as_str(),
            "pem" | "key" | "p12" | "pfx" | "jks" | "keystore"
        ) {
            return true;
        }
    }
    false
}

fn is_default_lockfile(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "cargo.lock"
            | "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "uv.lock"
            | "poetry.lock"
            | "composer.lock"
            | "gemfile.lock"
            | "go.sum"
    )
}

/// Returns true if the path passes through any default-ignored directory or is a secret/lock file.
pub fn is_default_excluded(path: &Path, is_dir: bool) -> bool {
    for comp in path.components() {
        if let Component::Normal(os_str) = comp {
            if let Some(name) = os_str.to_str() {
                if is_default_ignored_dir_name(name) {
                    return true;
                }
            }
        }
    }
    !is_dir && (is_default_secret_file(path) || is_default_lockfile(path))
}

pub struct IgnoreChecker {
    gitignore: Gitignore,
    root: PathBuf,
    raw_root: PathBuf,
}

impl IgnoreChecker {
    pub fn new(root: &Path) -> Self {
        let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let mut builder = GitignoreBuilder::new(&canonical_root);

        for ignore_file in [".contextignore", ".gitignore"] {
            let p = canonical_root.join(ignore_file);
            if p.exists() {
                if let Some(err) = builder.add(&p) {
                    if !err.is_io() {
                        eprintln!("Warning: error loading {:?}: {}", p, err);
                    }
                }
            }
        }

        let gitignore = match builder.build() {
            Ok(gi) => gi,
            Err(err) => {
                eprintln!(
                    "Warning: failed to build ignore matcher for {:?}: {}",
                    canonical_root, err
                );
                Gitignore::empty()
            }
        };

        Self {
            gitignore,
            root: canonical_root,
            raw_root: root.to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        // Resolve relative path cleanly against canonical or raw root
        let resolved = if path.exists() {
            fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        } else if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            fs::canonicalize(parent)
                .map(|p| p.join(name))
                .unwrap_or_else(|_| path.to_path_buf())
        } else {
            path.to_path_buf()
        };

        let relative_path = resolved
            .strip_prefix(&self.root)
            .or_else(|_| path.strip_prefix(&self.root))
            .or_else(|_| path.strip_prefix(&self.raw_root))
            .unwrap_or(path);

        if is_default_excluded(relative_path, is_dir) {
            return true;
        }

        // Check the path itself
        if self.gitignore.matched(relative_path, is_dir).is_ignore() {
            return true;
        }

        // Check ancestor directories ONLY (skip relative_path itself so files aren't tested as dirs!)
        let mut cur = relative_path.parent();
        while let Some(parent) = cur {
            if parent == Path::new("") {
                break;
            }
            if self.gitignore.matched(parent, true).is_ignore() {
                return true;
            }
            cur = parent.parent();
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_ignore_checker() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let mut file = File::create(root.join(".contextignore")).unwrap();
        writeln!(file, "*.tmp").unwrap();
        writeln!(file, "custom_ignored/").unwrap();

        let checker = IgnoreChecker::new(root);

        assert!(checker.is_ignored(&root.join("test.tmp"), false));
        assert!(checker.is_ignored(&root.join("node_modules/package.json"), false));
        assert!(checker.is_ignored(&root.join("node_modules"), true));
        assert!(checker.is_ignored(&root.join("target/debug/app"), false));
        assert!(checker.is_ignored(&root.join(".env"), false));
        assert!(checker.is_ignored(&root.join(".env.local"), false));
        assert!(checker.is_ignored(&root.join("certs/server.pem"), false));
        assert!(checker.is_ignored(&root.join("keys/id_rsa"), false));

        // Ensure directory-only rule `custom_ignored/` does NOT wrongly ignore a file named `custom_ignored`
        assert!(checker.is_ignored(&root.join("custom_ignored"), true));
        assert!(!checker.is_ignored(&root.join("custom_ignored"), false));
        assert!(checker.is_ignored(&root.join("custom_ignored/file.txt"), false));

        assert!(!checker.is_ignored(&root.join("test.txt"), false));
        assert!(!checker.is_ignored(&root.join("src/main.rs"), false));
    }
}
