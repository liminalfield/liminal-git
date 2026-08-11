// tests/common/test_repo.rs - TestRepo utility for creating test git repositories

use git2::{Oid, Repository, Signature, Time};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{Builder, TempDir};

/// Test utilities for git operations
pub struct TestRepo {
    pub temp_dir: TempDir,
    pub repo: Repository,
    pub path: PathBuf,
}

impl TestRepo {
    /// Create a new temporary git repository
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = Builder::new().prefix("rust-git-test").tempdir()?;

        let path = temp_dir.path().to_path_buf();
        let repo = Repository::init(&path)?;

        // Set up basic config
        let mut config = repo.config()?;
        config.set_str("user.name", "Test User")?;
        config.set_str("user.email", "test@example.com")?;

        Ok(TestRepo {
            temp_dir,
            repo,
            path,
        })
    }

    /// Create a test repository from a fixture
    pub fn from_fixture(fixture_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = Builder::new()
            .prefix(&format!("rust-git-test-{}", fixture_name))
            .tempdir()?;

        let fixture_path = PathBuf::from("test-fixtures").join(fixture_name);
        let dest_path = temp_dir.path().to_path_buf();

        // Copy fixture to temp directory
        copy_dir_recursive(&fixture_path, &dest_path)?;

        let repo = Repository::open(&dest_path)?;

        Ok(TestRepo {
            temp_dir,
            repo,
            path: dest_path,
        })
    }

    /// Add a file with content to the repository
    pub fn add_file<P: AsRef<Path>>(
        &self,
        path: P,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file_path = self.path.join(path);

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(file_path, content)?;
        Ok(())
    }

    /// Stage a file in the repository
    pub fn stage_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let mut index = self.repo.index()?;
        index.add_path(path.as_ref())?;
        index.write()?;
        Ok(())
    }

    /// Create a commit with the staged files
    pub fn commit(&self, message: &str) -> Result<Oid, Box<dyn std::error::Error>> {
        let sig = Signature::new("Test User", "test@example.com", &Time::new(0, 0))?;
        let mut index = self.repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;

        let parent_commit = match self.repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };

        let parents: Vec<&git2::Commit> = match &parent_commit {
            Some(commit) => vec![commit],
            None => vec![],
        };

        let commit_id = self
            .repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;

        Ok(commit_id)
    }

    /// Add and commit a file in one operation
    pub fn add_and_commit<P: AsRef<Path>>(
        &self,
        path: P,
        content: &str,
        message: &str,
    ) -> Result<Oid, Box<dyn std::error::Error>> {
        self.add_file(&path, content)?;
        self.stage_file(&path)?;
        self.commit(message)
    }

    /// Get the repository path as a string
    pub fn path_str(&self) -> &str {
        self.path.to_str().unwrap()
    }
}

/// Copy a directory recursively
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !src.is_dir() {
        return Err(format!("Source path is not a directory: {:?}", src).into());
    }

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}
