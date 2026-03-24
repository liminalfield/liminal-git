// tests/common/fixtures.rs - Test data and fixtures

/// Test data constants
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

/// Common test user data
pub const TEST_USER_NAME: &str = "Test User";
pub const TEST_USER_EMAIL: &str = "test@example.com";

/// Standard commit messages for tests
pub const INITIAL_COMMIT_MSG: &str = "Initial commit";
pub const UPDATE_COMMIT_MSG: &str = "Update files";
pub const DELETE_COMMIT_MSG: &str = "Delete files";

/// Generate a repository with standard test history
pub use crate::common::TestRepo;

impl TestRepo {
    /// Create a repository with a standard multi-commit history
    pub fn with_history() -> Result<Self, Box<dyn std::error::Error>> {
        let repo = Self::new()?;

        // Initial commit
        repo.add_and_commit("file1.txt", SAMPLE_FILE_CONTENT, INITIAL_COMMIT_MSG)?;

        // Update commit
        repo.add_and_commit("file1.txt", UPDATED_FILE_CONTENT, UPDATE_COMMIT_MSG)?;

        // Add another file
        repo.add_and_commit("file2.txt", "Second file content", "Add second file")?;

        Ok(repo)
    }

    /// Create a repository with binary files
    pub fn with_binary_files() -> Result<Self, Box<dyn std::error::Error>> {
        let repo = Self::new()?;

        // Add text file
        repo.add_file("text.txt", SAMPLE_FILE_CONTENT)?;

        // Add binary file
        let binary_path = repo.path.join("binary.png");
        std::fs::write(&binary_path, BINARY_FILE_CONTENT)?;

        repo.stage_file("text.txt")?;
        repo.stage_file("binary.png")?;
        repo.commit("Add text and binary files")?;

        Ok(repo)
    }

    /// Create a repository with deeply nested files
    pub fn with_nested_structure() -> Result<Self, Box<dyn std::error::Error>> {
        let repo = Self::new()?;

        for path in get_valid_paths() {
            repo.add_and_commit(path, &format!("Content for {}", path),
                               &format!("Add {}", path))?;
        }

        Ok(repo)
    }
}