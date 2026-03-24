// test_utils.rs

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{TempDir, Builder};
use git2::{Repository, Signature, Time, Oid};
use liminal_field_git::types::GitStatus;

/// Test utilities for git operations
pub struct TestRepo {
    pub temp_dir: TempDir,
    pub repo: Repository,
    pub path: PathBuf,
}

impl TestRepo {
    /// Create a new temporary git repository
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = Builder::new()
            .prefix("rust-git-test")
            .tempdir()?;

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
    pub fn add_file<P: AsRef<Path>>(&self, path: P, content: &str) -> Result<(), Box<dyn std::error::Error>> {
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

        let commit_id = self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &parents,
        )?;

        Ok(commit_id)
    }

    /// Add and commit a file in one operation
    pub fn add_and_commit<P: AsRef<Path>>(&self, path: P, content: &str, message: &str) -> Result<Oid, Box<dyn std::error::Error>> {
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

/// Test assertion helpers
pub mod assertions {
    use super::*;

    /// Assert that git status matches expected values
    pub fn assert_git_status_matches(
        actual: &GitStatus,
        expected_modified: usize,
        expected_untracked: usize,
        expected_staged: usize,
    ) {
        assert_eq!(
            actual.modified_files.len(),
            expected_modified,
            "Modified files count mismatch. Expected: {}, Actual: {}",
            expected_modified,
            actual.modified_files.len()
        );

        assert_eq!(
            actual.untracked_files.len(),
            expected_untracked,
            "Untracked files count mismatch. Expected: {}, Actual: {}",
            expected_untracked,
            actual.untracked_files.len()
        );

        assert_eq!(
            actual.staged_files.len(),
            expected_staged,
            "Staged files count mismatch. Expected: {}, Actual: {}",
            expected_staged,
            actual.staged_files.len()
        );
    }

    /// Assert that a commit exists with the given message
    pub fn assert_commit_exists(repo: &Repository, message: &str) -> bool {
        let mut revwalk = repo.revwalk().unwrap();
        revwalk.push_head().unwrap();

        for oid in revwalk {
            if let Ok(commit) = repo.find_commit(oid.unwrap()) {
                if commit.message().unwrap_or("").contains(message) {
                    return true;
                }
            }
        }
        false
    }

    /// Assert that a file is staged
    pub fn assert_file_staged(repo: &Repository, file_path: &str) -> bool {
        let index = repo.index().unwrap();
        index.get_path(Path::new(file_path), 0).is_some()
    }

    /// Assert error type matches expected
    pub fn assert_error_contains(error: &dyn std::error::Error, expected_text: &str) {
        assert!(
            error.to_string().contains(expected_text),
            "Error message '{}' does not contain expected text '{}'",
            error.to_string(),
            expected_text
        );
    }

    /// Assert NAPI error contains expected text (only available with NAPI feature)
    #[cfg(feature = "napi-binding")]
    pub fn assert_napi_error_contains(error: &napi::Error, expected_text: &str) {
        assert!(
            error.to_string().contains(expected_text),
            "Error message '{}' does not contain expected text '{}'",
            error.to_string(),
            expected_text
        );
    }
}

/// Test data constants
pub mod test_data {
    pub const SAMPLE_FILE_CONTENT: &str = r#"# Sample File
This is a test file for git operations.
It contains multiple lines to test diff operations.

## Section 1
Some content here.

## Section 2
More content here.
"#;

    pub const UPDATED_FILE_CONTENT: &str = r#"# Sample File (Updated)
This is a test file for git operations.
It contains multiple lines to test diff operations.
This line was added in an update.

## Section 1
Some content here.

## Section 2
More content here.
Additional content added.
"#;

    pub fn get_large_file_content() -> String {
        "0123456789".repeat(10000)
    }

    pub const BINARY_FILE_CONTENT: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    ];

    pub fn get_valid_paths() -> Vec<&'static str> {
        vec![
            "README.md",
            "src/main.rs",
            "docs/guide.md",
            "tests/test_file.rs",
            "deep/nested/path/file.txt",
        ]
    }

    pub fn get_invalid_paths() -> Vec<&'static str> {
        vec![
            "../outside_repo.txt",
            "/absolute/path.txt",
            "..\\windows_outside.txt",
            "\\windows_absolute.txt",
            "",
            "file\0with\0nulls.txt",
        ]
    }

    pub fn get_edge_case_inputs() -> Vec<String> {
        vec![
            "".to_string(),
            " ".to_string(),
            "\n".to_string(),
            "\t".to_string(),
            "a".repeat(1000),
            "file with spaces.txt".to_string(),
            "file-with-unicode-🦀.rs".to_string(),
            "file\nwith\nnewlines.txt".to_string(),
        ]
    }
}