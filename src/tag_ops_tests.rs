use super::*;
use git2::Repository;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn setup_test_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new_in(std::env::temp_dir()).expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    Repository::init(&repo_path).expect("Failed to initialize test repository");

    let repo = Repository::open(&repo_path).expect("Failed to open repository");
    let mut config = repo.config().expect("Failed to get config");
    config
        .set_str("user.name", "Test User")
        .expect("Failed to set user.name");
    config
        .set_str("user.email", "test@example.com")
        .expect("Failed to set user.email");

    (temp_dir, repo_path)
}

fn create_test_file(repo_path: &Path, file_path: &str, content: &str) {
    let full_path = repo_path.join(file_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create parent directories");
    }
    std::fs::write(&full_path, content).expect("Failed to write file");
}

fn commit_file(repo_path: &Path, file_path: &str, message: &str) -> String {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let relative_path = std::path::Path::new(file_path);

    let mut index = repo.index().expect("Failed to get index");
    index.add_path(relative_path).expect("Failed to add path");
    index.write().expect("Failed to write index");

    let tree_id = index.write_tree().expect("Failed to write tree");
    let tree = repo.find_tree(tree_id).expect("Failed to find tree");
    let signature =
        git2::Signature::now("Test User", "test@example.com").expect("Failed to create signature");

    let parent_commit = repo.head().ok().and_then(|head| {
        head.target()
            .and_then(|target| repo.find_commit(target).ok())
    });

    let commit_id = if let Some(parent) = parent_commit {
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
    } else {
        repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
    };

    commit_id.expect("Failed to create commit").to_string()
}

#[test]
fn test_create_annotated_tag_with_explicit_signature() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Create initial commit
    create_test_file(&repo_path, "test.txt", "content");
    commit_file(&repo_path, "test.txt", "Initial commit");

    // Create annotated tag with explicit signature
    let options = CreateTagOptions {
        name: "v1.0.0".to_string(),
        target_commit: None,
        message: Some("Release 1.0.0".to_string()),
        force: false,
        user_name: Some("Custom User".to_string()),
        user_email: Some("custom@example.com".to_string()),
    };

    let result = create_tag_impl(repo_path.to_str().unwrap(), &options);
    assert!(result.is_ok(), "Creating annotated tag should succeed");

    // Verify tag was created with correct signature
    let repo = Repository::open(&repo_path).expect("Failed to open repository");
    let tag_ref = repo
        .find_reference("refs/tags/v1.0.0")
        .expect("Tag should exist");
    let tag = tag_ref.peel_to_tag().expect("Should be an annotated tag");

    assert_eq!(tag.tagger().unwrap().name().unwrap(), "Custom User");
    assert_eq!(tag.tagger().unwrap().email().unwrap(), "custom@example.com");
    // git2 0.21: message() is Result<Option<&str>, _> rather than Option<&str>.
    assert_eq!(tag.message().unwrap(), Some("Release 1.0.0"));
}

#[test]
fn test_create_annotated_tag_with_config_fallback() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Create initial commit
    create_test_file(&repo_path, "test.txt", "content");
    commit_file(&repo_path, "test.txt", "Initial commit");

    // Create annotated tag without explicit signature (should use config)
    let options = CreateTagOptions {
        name: "v2.0.0".to_string(),
        target_commit: None,
        message: Some("Release 2.0.0".to_string()),
        force: false,
        user_name: None,
        user_email: None,
    };

    let result = create_tag_impl(repo_path.to_str().unwrap(), &options);
    assert!(result.is_ok(), "Creating annotated tag should succeed");

    // Verify tag was created with config signature
    let repo = Repository::open(&repo_path).expect("Failed to open repository");
    let tag_ref = repo
        .find_reference("refs/tags/v2.0.0")
        .expect("Tag should exist");
    let tag = tag_ref.peel_to_tag().expect("Should be an annotated tag");

    assert_eq!(tag.tagger().unwrap().name().unwrap(), "Test User");
    assert_eq!(tag.tagger().unwrap().email().unwrap(), "test@example.com");
}

#[test]
#[cfg_attr(not(target_env = "msvc"), serial_test::serial)]
fn test_create_annotated_tag_missing_config() {
    use std::env;

    // Save original env vars
    let orig_home = env::var("HOME").ok();
    let orig_xdg = env::var("XDG_CONFIG_HOME").ok();
    let orig_git_config = env::var("GIT_CONFIG_GLOBAL").ok();

    // Set up completely isolated config environment
    let base_temp = std::env::temp_dir();
    let config_dir = TempDir::new_in(&base_temp).expect("Failed to create temp config dir");
    let config_file = config_dir.path().join("gitconfig");
    let home_dir = TempDir::new_in(&base_temp).expect("Failed to create temp home dir");

    unsafe {
        env::set_var("GIT_CONFIG_GLOBAL", config_file.as_os_str());
        env::set_var("HOME", home_dir.path().as_os_str());
        env::set_var("XDG_CONFIG_HOME", home_dir.path().as_os_str());
        env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    }

    // Create repo WITHOUT setting user config
    let temp_dir = TempDir::new_in(&base_temp).expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();
    Repository::init(&repo_path).expect("Failed to initialize test repository");

    // Create initial commit with explicit user
    create_test_file(&repo_path, "test.txt", "content");
    commit_file(&repo_path, "test.txt", "Initial commit");

    // Try to create annotated tag without config and without explicit params
    let options = CreateTagOptions {
        name: "v3.0.0".to_string(),
        target_commit: None,
        message: Some("Release 3.0.0".to_string()),
        force: false,
        user_name: None,
        user_email: None,
    };

    let result = create_tag_impl(repo_path.to_str().unwrap(), &options);

    // Restore original env vars
    unsafe {
        if let Some(val) = orig_home {
            env::set_var("HOME", val);
        } else {
            env::remove_var("HOME");
        }
        if let Some(val) = orig_xdg {
            env::set_var("XDG_CONFIG_HOME", val);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(val) = orig_git_config {
            env::set_var("GIT_CONFIG_GLOBAL", val);
        } else {
            env::remove_var("GIT_CONFIG_GLOBAL");
        }
        env::remove_var("GIT_CONFIG_NOSYSTEM");
    }

    // Should fail - config missing
    assert!(result.is_err(), "Creating tag without config should fail");
    match result {
        Err(GitError::ConfigMissing { .. }) => {
            // Expected error
        }
        _ => panic!("Expected ConfigMissing error, got: {:?}", result),
    }
}

#[test]
fn test_create_lightweight_tag() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Create initial commit
    create_test_file(&repo_path, "test.txt", "content");
    commit_file(&repo_path, "test.txt", "Initial commit");

    // Create lightweight tag (no message, no signature needed)
    let options = CreateTagOptions {
        name: "v4.0.0".to_string(),
        target_commit: None,
        message: None, // Lightweight tag
        force: false,
        user_name: None,
        user_email: None,
    };

    let result = create_tag_impl(repo_path.to_str().unwrap(), &options);
    assert!(result.is_ok(), "Creating lightweight tag should succeed");

    // Verify tag was created
    let repo = Repository::open(&repo_path).expect("Failed to open repository");
    let tag_ref = repo
        .find_reference("refs/tags/v4.0.0")
        .expect("Tag should exist");

    // Lightweight tags don't have tag objects, they point directly to commits
    assert!(
        tag_ref.peel_to_tag().is_err(),
        "Should not be an annotated tag"
    );
    assert!(tag_ref.peel_to_commit().is_ok(), "Should point to a commit");
}
