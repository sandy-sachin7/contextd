use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

pub struct IgnoreChecker {
    gitignore: Gitignore,
    root: PathBuf,
}

impl IgnoreChecker {
    pub fn new(root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(root);

        // Add .contextignore
        if let Some(err) = builder.add(root.join(".contextignore")) {
            if !err.is_io() {
                // Ignore IO errors (missing file)
                eprintln!("Error loading .contextignore: {}", err);
            }
        }

        // Add .gitignore
        if let Some(err) = builder.add(root.join(".gitignore")) {
            if !err.is_io() {
                eprintln!("Error loading .gitignore: {}", err);
            }
        }

        Self {
            gitignore: builder.build().unwrap(),
            root: root.to_path_buf(),
        }
    }

    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let relative_path = path.strip_prefix(&self.root).unwrap_or(path);

        // Check the path itself
        if self.gitignore.matched(relative_path, is_dir).is_ignore() {
            return true;
        }

        // Check parents
        for parent in relative_path.ancestors() {
            if parent == Path::new("") {
                continue;
            }
            if self.gitignore.matched(parent, true).is_ignore() {
                return true;
            }
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

        // Create .contextignore
        let mut file = File::create(root.join(".contextignore")).unwrap();
        writeln!(file, "*.tmp").unwrap();
        writeln!(file, "node_modules").unwrap();

        let checker = IgnoreChecker::new(root);

        // Test ignored files
        assert!(checker.is_ignored(&root.join("test.tmp"), false));
        assert!(checker.is_ignored(&root.join("node_modules/package.json"), false));
        assert!(checker.is_ignored(&root.join("node_modules"), true));

        // Test allowed files
        assert!(!checker.is_ignored(&root.join("test.txt"), false));
        assert!(!checker.is_ignored(&root.join("src/main.rs"), false));
    }
}
