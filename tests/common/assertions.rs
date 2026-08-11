// tests/common/assertions.rs - Test assertion helpers

use std::path::Path;
use git2::Repository;
use liminal_git::types::GitStatus;

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

/// Assert that result is a specific error type
pub fn assert_result_is_error<T, E>(result: &Result<T, E>) {
    assert!(result.is_err(), "Expected error result but got Ok");
}

/// Assert that result is successful
pub fn assert_result_is_ok<T, E>(result: &Result<T, E>) {
    assert!(result.is_ok(), "Expected Ok result but got error: {:?}",
        result.as_ref().err().map(|_| "error"));
}

/// Assert commit hash is valid (40 character SHA-1)
pub fn assert_valid_commit_hash(hash: &str) {
    assert_eq!(hash.len(), 40, "Commit hash should be 40 characters");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()),
        "Commit hash should only contain hex digits");
}