use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;

pub struct IgnoreChecker {
    gitignore: Gitignore,
}

impl IgnoreChecker {
    pub fn new(root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(root);

        // Add .contextignore
        if let Some(err) = builder.add(root.join(".contextignore")) {
            if !err.is_io() { // Ignore IO errors (missing file)
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
        }
    }

    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let matched = self.gitignore.matched(path, is_dir);
        matched.is_ignore()
    }
}
