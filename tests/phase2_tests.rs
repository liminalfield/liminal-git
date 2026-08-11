// tests/phase2_tests.rs
//
// These tests used to drive `GitService`, the N-API surface. That surface
// cannot be exercised from a Rust test binary at all — not "is not currently",
// but cannot: it links napi, whose symbols are resolved by the host Node
// process at run time, so the target fails at the linker rather than at the
// compiler. The tests therefore never ran, and drifted until they no longer
// compiled either.
//
// What they were really asserting is that bad input is rejected: an unknown
// line ending, an empty gitignore list, an unrecognised core.autocrlf value.
// `GitService` gets that behaviour by composing `validate_*` with `*_impl`,
// and both halves are plain Rust, so both are exercised directly here.
//
// The composition itself — that GitService calls the validator *before* the
// impl, for every method — is only observable through Node. There is no JS
// suite covering it today; jest is configured in this package but matches no
// files. That gap is real and is not closed by this file.

use liminal_git::repository_ops::{
    configure_repository_impl, create_gitignore_impl, init_repository_impl,
    init_repository_with_config_impl, is_repository_impl,
};
use liminal_git::validation::{
    validate_git_config, validate_gitignore_patterns, validate_repository_config,
};
use liminal_git::{GitConfig, GitError, RepositoryConfig};
use tempfile::TempDir;

#[test]
fn test_init_repository_success() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().to_string_lossy().to_string();

    assert!(init_repository_impl(&path).unwrap());
    assert!(temp_dir.path().join(".git").exists());
    assert!(is_repository_impl(&path));
}

#[test]
fn test_repository_config_accepts_a_valid_configuration() {
    let valid = RepositoryConfig {
        description: Some("Valid description".to_string()),
        default_branch: Some("main".to_string()),
        line_ending: Some("lf".to_string()),
    };

    assert!(validate_repository_config(&valid).is_ok());

    let temp_dir = TempDir::new().unwrap();
    let created =
        init_repository_with_config_impl(&temp_dir.path().to_string_lossy(), &valid).unwrap();
    assert!(created);
    assert!(temp_dir.path().join(".git").exists());
}

#[test]
fn test_repository_config_rejects_an_unknown_line_ending() {
    let invalid = RepositoryConfig {
        description: None,
        default_branch: None,
        line_ending: Some("invalid".to_string()),
    };

    let result = validate_repository_config(&invalid);
    assert!(
        matches!(result, Err(GitError::InvalidArgument { .. })),
        "got {result:?}"
    );
}

#[test]
fn test_gitignore_rejects_an_empty_pattern_list() {
    let result = validate_gitignore_patterns(&[]);
    assert!(
        matches!(result, Err(GitError::InvalidArgument { .. })),
        "got {result:?}"
    );
}

#[test]
fn test_gitignore_writes_valid_patterns() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().to_string_lossy().to_string();
    init_repository_impl(&path).unwrap();

    let patterns = vec!["*.log".to_string()];
    assert!(validate_gitignore_patterns(&patterns).is_ok());
    assert!(create_gitignore_impl(&path, &patterns).unwrap());

    let written = std::fs::read_to_string(temp_dir.path().join(".gitignore")).unwrap();
    assert!(written.contains("*.log"), "got {written:?}");
}

#[test]
fn test_git_config_rejects_an_unknown_autocrlf_value() {
    let invalid = GitConfig {
        user_name: Some("Test User".to_string()),
        user_email: Some("test@example.com".to_string()),
        core_autocrlf: Some("invalid".to_string()),
        core_safecrlf: None,
    };

    let result = validate_git_config(&invalid);
    assert!(
        matches!(result, Err(GitError::InvalidArgument { .. })),
        "got {result:?}"
    );
}

#[test]
fn test_git_config_applies_a_valid_configuration() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().to_string_lossy().to_string();
    init_repository_impl(&path).unwrap();

    let valid = GitConfig {
        user_name: Some("Test User".to_string()),
        user_email: Some("test@example.com".to_string()),
        core_autocrlf: Some("input".to_string()),
        core_safecrlf: None,
    };

    assert!(validate_git_config(&valid).is_ok());
    assert!(configure_repository_impl(&path, &valid).unwrap());
}
