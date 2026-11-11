use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::paths::{data_dir, write_atomic};
use crate::Result;

/// A user-made group of repositories, shown like a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
}

/// A repository the user added to Cairn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoEntry {
    pub path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub category: Option<Uuid>,
    #[serde(default)]
    pub last_opened: i64,
}

/// Every repository and category the user has set up, plus open tabs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub open_tabs: Vec<PathBuf>,
    #[serde(default)]
    pub active_tab: Option<PathBuf>,
    #[serde(skip)]
    file: Option<PathBuf>,
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

impl Library {
    /// Loads the library from the data folder.
    ///
    /// # Returns
    /// The saved library, or an empty one on first run.
    pub fn load() -> Self {
        Self::load_from(data_dir().join("library.json"))
    }

    /// Loads the library from a specific file.
    ///
    /// # Arguments
    /// * `file` - JSON file to read and later save to
    ///
    /// # Returns
    /// The saved library, or an empty one if the file is missing.
    pub fn load_from(file: PathBuf) -> Self {
        let mut lib: Library = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        lib.file = Some(file);
        lib
    }

    /// Writes the library to disk.
    ///
    /// # Returns
    /// `Ok(())` when saved.
    pub fn save(&self) -> Result<()> {
        let file = self.file.clone().unwrap_or_else(|| data_dir().join("library.json"));
        write_atomic(&file, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    /// Creates a new category.
    ///
    /// # Arguments
    /// * `name` - display name
    ///
    /// # Returns
    /// The new category's id.
    pub fn create_category(&mut self, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.categories.push(Category { id, name: name.trim().to_string(), collapsed: false });
        id
    }

    /// Renames a category.
    ///
    /// # Arguments
    /// * `id` - category to rename
    /// * `name` - new display name
    ///
    /// # Returns
    /// `true` when the category exists.
    pub fn rename_category(&mut self, id: Uuid, name: &str) -> bool {
        match self.categories.iter_mut().find(|c| c.id == id) {
            Some(c) => {
                c.name = name.trim().to_string();
                true
            }
            None => false,
        }
    }

    /// Deletes a category. Its repositories stay in the library without a category.
    ///
    /// # Arguments
    /// * `id` - category to delete
    ///
    /// # Returns
    /// Nothing.
    pub fn delete_category(&mut self, id: Uuid) {
        self.categories.retain(|c| c.id != id);
        for repo in &mut self.repos {
            if repo.category == Some(id) {
                repo.category = None;
            }
        }
    }

    /// Moves a category one step up or down in the list.
    ///
    /// # Arguments
    /// * `id` - category to move
    /// * `delta` - `-1` for up, `1` for down
    ///
    /// # Returns
    /// Nothing.
    pub fn move_category(&mut self, id: Uuid, delta: isize) {
        if let Some(i) = self.categories.iter().position(|c| c.id == id) {
            let j = i as isize + delta;
            if j >= 0 && (j as usize) < self.categories.len() {
                self.categories.swap(i, j as usize);
            }
        }
    }

    /// Expands or collapses a category in the sidebar.
    ///
    /// # Arguments
    /// * `id` - category to toggle
    ///
    /// # Returns
    /// Nothing.
    pub fn toggle_collapsed(&mut self, id: Uuid) {
        if let Some(c) = self.categories.iter_mut().find(|c| c.id == id) {
            c.collapsed = !c.collapsed;
        }
    }

    /// Adds a repository, or moves it if it is already in the library.
    ///
    /// # Arguments
    /// * `path` - repository root
    /// * `category` - category to put it in, or `None`
    ///
    /// # Returns
    /// Nothing.
    pub fn add_repo(&mut self, path: &Path, category: Option<Uuid>) {
        if let Some(existing) = self.repos.iter_mut().find(|r| same_path(&r.path, path)) {
            existing.category = category;
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        self.repos.push(RepoEntry { path: path.to_path_buf(), name, category, last_opened: 0 });
    }

    /// Removes a repository from Cairn. Files on disk are not touched.
    ///
    /// # Arguments
    /// * `path` - repository root
    ///
    /// # Returns
    /// Nothing.
    pub fn remove_repo(&mut self, path: &Path) {
        self.repos.retain(|r| !same_path(&r.path, path));
        self.open_tabs.retain(|t| !same_path(t, path));
        if self.active_tab.as_deref().is_some_and(|a| same_path(a, path)) {
            self.active_tab = self.open_tabs.first().cloned();
        }
    }

    /// Moves a repository into another category.
    ///
    /// # Arguments
    /// * `path` - repository root
    /// * `category` - target category, or `None` for no category
    ///
    /// # Returns
    /// Nothing.
    pub fn set_repo_category(&mut self, path: &Path, category: Option<Uuid>) {
        if let Some(r) = self.repos.iter_mut().find(|r| same_path(&r.path, path)) {
            r.category = category;
        }
    }

    /// Changes the name shown for a repository.
    ///
    /// # Arguments
    /// * `path` - repository root
    /// * `name` - new display name
    ///
    /// # Returns
    /// Nothing.
    pub fn rename_repo(&mut self, path: &Path, name: &str) {
        if let Some(r) = self.repos.iter_mut().find(|r| same_path(&r.path, path)) {
            r.name = name.trim().to_string();
        }
    }

    /// Repositories in a category, sorted by name.
    ///
    /// # Arguments
    /// * `category` - category id, or `None` for repositories without one
    ///
    /// # Returns
    /// Matching repositories.
    pub fn repos_in(&self, category: Option<Uuid>) -> Vec<&RepoEntry> {
        let mut list: Vec<&RepoEntry> = self.repos.iter().filter(|r| r.category == category).collect();
        list.sort_by_key(|r| r.name.to_lowercase());
        list
    }

    /// Finds a repository entry by path.
    ///
    /// # Arguments
    /// * `path` - repository root
    ///
    /// # Returns
    /// The entry, if the repository is in the library.
    pub fn repo(&self, path: &Path) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| same_path(&r.path, path))
    }

    /// Opens a repository in a tab and makes it active.
    ///
    /// # Arguments
    /// * `path` - repository root
    ///
    /// # Returns
    /// Nothing.
    pub fn open_tab(&mut self, path: &Path) {
        if !self.open_tabs.iter().any(|t| same_path(t, path)) {
            self.open_tabs.push(path.to_path_buf());
        }
        self.active_tab = Some(path.to_path_buf());
        if let Some(r) = self.repos.iter_mut().find(|r| same_path(&r.path, path)) {
            r.last_opened = chrono::Utc::now().timestamp();
        }
    }

    /// Closes a repository tab.
    ///
    /// # Arguments
    /// * `path` - repository root
    ///
    /// # Returns
    /// Nothing.
    pub fn close_tab(&mut self, path: &Path) {
        let idx = self.open_tabs.iter().position(|t| same_path(t, path));
        self.open_tabs.retain(|t| !same_path(t, path));
        if self.active_tab.as_deref().is_some_and(|a| same_path(a, path)) {
            self.active_tab = idx
                .and_then(|i| self.open_tabs.get(i.saturating_sub(1)).or(self.open_tabs.first()))
                .cloned();
        }
    }

    /// Finds git repositories directly inside a folder, for adding many at once.
    ///
    /// # Arguments
    /// * `folder` - folder to scan (one level deep)
    ///
    /// # Returns
    /// Paths of folders that contain a `.git` entry, including `folder` itself.
    pub fn scan_folder(folder: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        if folder.join(".git").exists() {
            found.push(folder.to_path_buf());
        }
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() && p.join(".git").exists() {
                    found.push(p);
                }
            }
        }
        found.sort();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_hold_repos_and_survive_reload() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        let mut lib = Library::load_from(file.clone());
        let sc = lib.create_category("Aurora");
        lib.add_repo(Path::new("C:/repos/aurora"), Some(sc));
        lib.add_repo(Path::new("C:/repos/aurora-docs"), Some(sc));
        lib.add_repo(Path::new("C:/repos/dotfiles"), None);
        lib.save().unwrap();

        let mut lib = Library::load_from(file);
        assert_eq!(lib.repos_in(Some(sc)).len(), 2);
        assert_eq!(lib.repos_in(None).len(), 1);
        lib.delete_category(sc);
        assert_eq!(lib.repos_in(None).len(), 3);
    }

    #[test]
    fn adding_existing_repo_moves_it() {
        let mut lib = Library::default();
        let a = lib.create_category("A");
        lib.add_repo(Path::new("C:/x/repo"), None);
        lib.add_repo(Path::new("c:\\x\\repo\\"), Some(a));
        assert_eq!(lib.repos.len(), 1);
        assert_eq!(lib.repos[0].category, Some(a));
    }
}
