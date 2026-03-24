/// Test data constants and utilities

pub const SAMPLE_COMMIT_MESSAGES: &[&str] = &[
    "Initial commit",
    "Add new feature",
    "Fix bug in validation",
    "Update documentation",
    "Refactor code structure",
    "feat: implement git operations",
    "fix: resolve path validation issue",
    "docs: update README with examples",
];

pub const SAMPLE_USER_NAMES: &[&str] = &[
    "Test User",
    "John Doe",
    "Jane Smith",
    "Developer Name",
    "Git Tester",
];

pub const SAMPLE_USER_EMAILS: &[&str] = &[
    "test@example.com",
    "john.doe@example.com",
    "jane.smith@company.org",
    "developer@test.local",
    "git.tester@domain.co.uk",
];

pub fn get_invalid_commit_messages() -> Vec<String> {
    vec![
        "".to_string(),
        "   ".to_string(),
        "\0null byte in message".to_string(),
        // Very long message (over 10000 chars)
        "a".repeat(10001),
    ]
}

pub fn get_invalid_user_names() -> Vec<String> {
    vec![
        "".to_string(),
        "\0null byte".to_string(),
        "a".repeat(256), // Too long
    ]
}

pub fn get_invalid_user_emails() -> Vec<String> {
    vec![
        "".to_string(),
        "invalid-email".to_string(),
        "no-at-symbol.com".to_string(),
        "no-domain@".to_string(),
        "@no-local-part.com".to_string(),
        "\0null@byte.com".to_string(),
        format!("{}@example.com", "a".repeat(250)), // Too long
    ]
}

pub const SAMPLE_FILE_NAMES: &[&str] = &[
    "README.md",
    "src/main.rs",
    "src/lib.rs",
    "tests/integration_test.rs",
    "docs/API.md",
    "Cargo.toml",
    "LICENSE",
    ".gitignore",
    "config/settings.toml",
    "scripts/build.sh",
];

pub const SAMPLE_FILE_CONTENTS: &[&str] = &[
    "# README\n\nThis is a sample project.",
    "fn main() {\n    println!(\"Hello, world!\");\n}",
    "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}",
    "#[test]\nfn test_basic() {\n    assert_eq!(2 + 2, 4);\n}",
    "# API Documentation\n\n## Overview\n\nThis API provides...",
    "[package]\nname = \"test-project\"\nversion = \"0.1.0\"",
    "MIT License\n\nCopyright (c) 2024",
    "target/\n*.log\n.env",
    "[database]\nurl = \"sqlite://test.db\"",
    "#!/bin/bash\ncargo build --release",
];

pub fn get_test_file_with_content(index: usize) -> (&'static str, &'static str) {
    let file_names = SAMPLE_FILE_NAMES;
    let file_contents = SAMPLE_FILE_CONTENTS;

    let name_index = index % file_names.len();
    let content_index = index % file_contents.len();

    (file_names[name_index], file_contents[content_index])
}

pub fn get_large_file_content(size_kb: usize) -> String {
    let line = "This is a line of text for testing large file operations.\n";
    let lines_needed = (size_kb * 1024) / line.len();
    line.repeat(lines_needed)
}

pub fn get_binary_file_content() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41,
        0x54, 0x08, 0x1D, 0x01, 0x01, 0x00, 0x00, 0xFF,
        0xFF, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x48,
        0xAF, 0xA4, 0x71, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

/// Generate a set of test files for performance testing
pub fn generate_performance_test_files(count: usize) -> Vec<(String, String)> {
    (0..count)
        .map(|i| {
            let filename = format!("perf_test_{:04}.txt", i);
            let content = format!("Performance test file {}\n{}", i, get_large_file_content(1));
            (filename, content)
        })
        .collect()
}

/// Common directory structures for testing
pub const COMMON_DIRECTORIES: &[&str] = &[
    "src",
    "tests",
    "docs",
    "examples",
    "scripts",
    "config",
    "assets",
    "target", // Should be ignored
    ".git",   // Should be ignored
    "node_modules", // Should be ignored
];

/// File extensions commonly used in projects
pub const COMMON_EXTENSIONS: &[&str] = &[
    ".rs",
    ".toml",
    ".md",
    ".txt",
    ".json",
    ".yaml",
    ".yml",
    ".sh",
    ".bat",
    ".gitignore",
];

/// Generate a realistic project structure
pub fn generate_project_structure() -> Vec<(String, String)> {
    let mut files = Vec::new();

    // Root files
    files.push(("README.md".to_string(), "# Test Project\n\nA test project for git operations.".to_string()));
    files.push(("Cargo.toml".to_string(), "[package]\nname = \"test-project\"\nversion = \"0.1.0\"\nedition = \"2021\"".to_string()));
    files.push(("LICENSE".to_string(), "MIT License\n\nCopyright (c) 2024".to_string()));
    files.push((".gitignore".to_string(), "target/\n*.log\n.env".to_string()));

    // Source files
    files.push(("src/main.rs".to_string(), "fn main() {\n    println!(\"Hello, world!\");\n}".to_string()));
    files.push(("src/lib.rs".to_string(), "pub mod utils;\npub mod models;".to_string()));
    files.push(("src/utils.rs".to_string(), "pub fn helper_function() -> String {\n    \"helper\".to_string()\n}".to_string()));
    files.push(("src/models.rs".to_string(), "pub struct Model {\n    pub id: u32,\n    pub name: String,\n}".to_string()));

    // Test files
    files.push(("tests/integration_test.rs".to_string(), "#[test]\nfn integration_test() {\n    assert!(true);\n}".to_string()));
    files.push(("tests/common/mod.rs".to_string(), "pub fn setup() {\n    // Test setup code\n}".to_string()));

    // Documentation
    files.push(("docs/API.md".to_string(), "# API Documentation\n\n## Overview\n\nThis is the API documentation.".to_string()));
    files.push(("docs/CHANGELOG.md".to_string(), "# Changelog\n\n## v0.1.0\n\n- Initial release".to_string()));

    files
}