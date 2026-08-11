use std::fs;
use std::path::Path;
use git2::{Repository, Signature, Time};

/// Create all test fixture repositories
pub fn create_all_fixtures() -> Result<(), Box<dyn std::error::Error>> {
    let fixtures_dir = Path::new("test-fixtures");

    if fixtures_dir.exists() {
        fs::remove_dir_all(fixtures_dir)?;
    }
    fs::create_dir_all(fixtures_dir)?;

    create_empty_repo()?;
    create_simple_repo()?;
    create_complex_repo()?;

    Ok(())
}

/// Create an empty git repository
fn create_empty_repo() -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = Path::new("test-fixtures/empty-repo");
    let repo = Repository::init(repo_path)?;

    // Set basic config
    let mut config = repo.config()?;
    config.set_str("user.name", "Test User")?;
    config.set_str("user.email", "test@example.com")?;

    println!("Created empty repository at {:?}", repo_path);
    Ok(())
}

/// Create a simple repository with one commit
fn create_simple_repo() -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = Path::new("test-fixtures/simple-repo");
    let repo = Repository::init(repo_path)?;

    // Set basic config
    let mut config = repo.config()?;
    config.set_str("user.name", "Test User")?;
    config.set_str("user.email", "test@example.com")?;

    // Create README.md
    let readme_content = r#"# Simple Test Repository

This is a simple repository for testing basic git operations.

## Features
- Single commit
- Clean working directory
- Basic file structure
"#;
    fs::write(repo_path.join("README.md"), readme_content)?;

    // Create src/main.rs
    fs::create_dir_all(repo_path.join("src"))?;
    let main_content = r#"fn main() {
    println!("Hello, world!");
}
"#;
    fs::write(repo_path.join("src/main.rs"), main_content)?;

    // Stage and commit files
    let sig = Signature::new("Test User", "test@example.com", &Time::new(1640995200, 0))?; // 2022-01-01
    let mut index = repo.index()?;
    index.add_path(Path::new("README.md"))?;
    index.add_path(Path::new("src/main.rs"))?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Initial commit: Add README and main.rs",
        &tree,
        &[],
    )?;

    println!("Created simple repository at {:?}", repo_path);
    Ok(())
}

/// Create a complex repository with multiple commits and various states
fn create_complex_repo() -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = Path::new("test-fixtures/complex-repo");
    let repo = Repository::init(repo_path)?;

    // Set basic config
    let mut config = repo.config()?;
    config.set_str("user.name", "Test User")?;
    config.set_str("user.email", "test@example.com")?;

    let sig = Signature::new("Test User", "test@example.com", &Time::new(1640995200, 0))?;

    // First commit: Initial project structure
    fs::write(repo_path.join("README.md"), "# Complex Test Repository")?;
    fs::create_dir_all(repo_path.join("src"))?;
    fs::write(repo_path.join("src/lib.rs"), "pub fn hello() { println!(\"Hello\"); }")?;

    let mut index = repo.index()?;
    index.add_path(Path::new("README.md"))?;
    index.add_path(Path::new("src/lib.rs"))?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let first_commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Initial commit",
        &tree,
        &[],
    )?;

    // Second commit: Add documentation
    fs::create_dir_all(repo_path.join("docs"))?;
    fs::write(repo_path.join("docs/guide.md"), "# User Guide\n\nThis is a guide.")?;
    fs::write(repo_path.join("Cargo.toml"), r#"[package]
name = "test-project"
version = "0.1.0"
"#)?;

    index.add_path(Path::new("docs/guide.md"))?;
    index.add_path(Path::new("Cargo.toml"))?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let first_commit_obj = repo.find_commit(first_commit)?;
    let second_commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Add documentation and Cargo.toml",
        &tree,
        &[&first_commit_obj],
    )?;

    // Third commit: Update existing files
    fs::write(repo_path.join("README.md"), "# Complex Test Repository\n\nUpdated with more content.")?;
    fs::write(repo_path.join("src/lib.rs"), r#"pub fn hello() {
    println!("Hello from updated lib!");
}

pub fn goodbye() {
    println!("Goodbye!");
}
"#)?;

    index.add_path(Path::new("README.md"))?;
    index.add_path(Path::new("src/lib.rs"))?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let second_commit_obj = repo.find_commit(second_commit)?;
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "Update README and add goodbye function",
        &tree,
        &[&second_commit_obj],
    )?;

    // Create files in various states for testing

    // Modified file (not staged)
    fs::write(repo_path.join("src/lib.rs"), r#"pub fn hello() {
    println!("Hello from modified lib!");
}

pub fn goodbye() {
    println!("Goodbye!");
}

// This change is not staged
pub fn new_function() {
    println!("This is new!");
}
"#)?;

    // Staged file
    fs::write(repo_path.join("src/staged.rs"), "// This file is staged but not committed")?;
    index.add_path(Path::new("src/staged.rs"))?;
    index.write()?;

    // Untracked files
    fs::write(repo_path.join("untracked.txt"), "This file is not tracked by git")?;
    fs::create_dir_all(repo_path.join("temp"))?;
    fs::write(repo_path.join("temp/temp_file.txt"), "Temporary file")?;

    // Binary file
    fs::write(repo_path.join("binary.dat"), [0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD])?;

    println!("Created complex repository at {:?}", repo_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_all_fixtures() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        // Change to temp directory for testing
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let result = create_all_fixtures();

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "Failed to create fixtures: {:?}", result.err());
    }
}