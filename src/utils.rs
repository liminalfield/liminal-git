// utils.rs
use crate::errors::GitError;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[cfg(feature = "napi-binding")]
use napi::Error as NapiError;
#[cfg(feature = "napi-binding")]
use napi::Status;

// Per-repo lock registry (#390). Mutating git ops on the same repo run under
// this lock so concurrent IPC handlers can't race the index/HEAD. Read-only ops
// don't take it. BTreeMap::new() is const, so no lazy init.
static REPO_LOCKS: Mutex<BTreeMap<String, Arc<Mutex<()>>>> = Mutex::new(BTreeMap::new());

/// The mutating-operation lock for a repo path (created on first use).
///
/// The key is the canonicalised path, not the caller's string. The path
/// arrives from JavaScript and two callers can name one repository
/// differently — `/books/a` and `/books/a/`, a relative path against the
/// process cwd, or a symlink into the real location. Keyed by the raw string
/// those produce different entries, so both operations acquire different
/// locks and proceed at once: a mutual exclusion that silently isn't one, in
/// exactly the concurrent situation the lock exists for.
///
/// `canonicalize` requires the path to exist. When it doesn't there is no
/// repository to serialise access to, so falling back to the raw string is
/// safe — the caller is about to fail its own path validation anyway.
pub fn repo_lock(repo_path: &str) -> Arc<Mutex<()>> {
    let key = std::fs::canonicalize(repo_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| repo_path.to_string());

    let mut locks = REPO_LOCKS.lock().unwrap_or_else(|p| p.into_inner());
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Run a blocking git2 operation off the JS thread on tokio's blocking pool,
/// converting a GitError to a NAPI error with the caller's structured-errors
/// flag. Keeps napi async methods from blocking the main event loop (#390).
#[cfg(feature = "napi-binding")]
pub async fn run_blocking<T, F>(structured: bool, f: F) -> Result<T, NapiError>
where
    F: FnOnce() -> Result<T, GitError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| NapiError::from_reason(format!("git task failed to run: {e}")))?
        .map_err(|e| git_error_to_napi_with_flags(e, structured))
}

// There were two more path helpers here: `validate_and_normalize_path`, a
// napi-flavoured copy of `validate_and_normalize_path_git` below, and
// `validate_and_normalize_path_anyhow`, a third wrapper around the same
// logic. Neither had a caller. Two unexercised copies of path-traversal
// checking is a liability, not redundancy — the copy nothing runs is the copy
// that drifts. One implementation, `..._git`, is what the eight call sites in
// file_ops use, and it is the one the tests cover.

/// Convert GitError to NAPI error with appropriate status codes
///
/// # Structured Errors (when `structured` = true)
///
/// Serializes a complete error object to JSON for transport:
/// ```json
/// {
///   "code": "FILE_NOT_FOUND",
///   "message": "File not found: /path/to/file",
///   "retriable": false,
///   "details": { "path": "/path/to/file" }
/// }
/// ```
///
/// JavaScript consumers should use `parseStructuredGitError(err)` to reconstruct
/// the typed error object with proper properties.
///
/// ## Why JSON Transport?
///
/// napi-rs 3.3 only supports `napi::Error::new(status, message)` - there's no way
/// to attach arbitrary properties to the error object. We serialize to JSON as a
/// pragmatic bridge until napi-rs adds native structured error support.
///
/// The `GitError::build_details_object()` method exists for future upgrade when
/// we can attach properties directly, but is currently unused in the error path.
///
/// # Simple Errors (when `structured` = false)
///
/// Returns just the display message for backward compatibility.
#[cfg(feature = "napi-binding")]
pub fn git_error_to_napi_with_flags(error: GitError, structured: bool) -> NapiError {
    use crate::errors::GitError;

    // Map error variants to NAPI status codes
    let status = match &error {
        GitError::FileNotFound { .. } => Status::GenericFailure,
        GitError::PathTraversal { .. } => Status::InvalidArg,
        GitError::InvalidPath { .. } => Status::InvalidArg,
        GitError::InvalidArgument { .. } => Status::InvalidArg,
        GitError::InvalidBranchName { .. } => Status::InvalidArg,
        GitError::InvalidTagName { .. } => Status::InvalidArg,
        GitError::InvalidCommitHash { .. } => Status::InvalidArg,
        GitError::BranchNotFound { .. } => Status::GenericFailure,
        GitError::BranchAlreadyExists { .. } => Status::GenericFailure,
        GitError::TagNotFound { .. } => Status::GenericFailure,
        GitError::TagAlreadyExists { .. } => Status::GenericFailure,
        GitError::UncommittedChanges { .. } => Status::GenericFailure,
        GitError::UnstagedChangesWouldBeLost { .. } => Status::GenericFailure,
        GitError::ConfigMissing { .. } => Status::GenericFailure,
        GitError::CannotDeleteCurrentBranch { .. } => Status::GenericFailure,
        GitError::BranchNotMerged { .. } => Status::GenericFailure,
        GitError::RepositoryNotFound { .. } => Status::GenericFailure,
        GitError::RepositoryCorrupted { .. } => Status::GenericFailure,
        GitError::InvalidRepository { .. } => Status::InvalidArg,
        GitError::FileNotInRepository { .. } => Status::InvalidArg,
        GitError::NothingToCommit => Status::GenericFailure,
        GitError::MergeConflict { .. } => Status::GenericFailure,
        GitError::DetachedHead => Status::GenericFailure,
        GitError::IoError { .. } => Status::GenericFailure,
        GitError::GitOperationFailure { .. } => Status::GenericFailure,
    };

    let message = if structured {
        // Serialize complete error structure to JSON
        let serialized = error.to_serializable();
        serde_json::to_string(&serialized).unwrap_or_else(|_| {
            // Fallback if serialization fails (should never happen). A raw
            // string keeps this readable; it was previously a format! with no
            // arguments and every brace and quote escaped.
            r#"{"code":"SERIALIZATION_ERROR","message":"Failed to serialize error","retriable":false,"details":{}}"#
                .to_string()
        })
    } else {
        // Simple string message for backward compatibility
        error.to_string()
    };

    NapiError::new(status, message)
}

// GitError version for internal use
pub fn validate_and_normalize_path_git(
    repo_path: &str,
    file_path: &str,
) -> Result<std::path::PathBuf, GitError> {
    // Validate repository path exists and is a directory
    let repo_path_buf = Path::new(repo_path);
    if !repo_path_buf.exists() {
        return Err(GitError::InvalidRepository {
            path: repo_path.to_string(),
        });
    }

    if !repo_path_buf.is_dir() {
        return Err(GitError::InvalidRepository {
            path: repo_path.to_string(),
        });
    }

    // Convert file path to relative path from repository root
    let abs_file_path = Path::new(file_path);

    let relative_path = if abs_file_path.is_absolute() {
        // Check for path traversal attempts
        abs_file_path
            .strip_prefix(repo_path_buf)
            .map_err(|_| GitError::FileNotInRepository {
                path: file_path.to_string(),
            })?
    } else {
        abs_file_path
    };

    // Additional security check - ensure no ".." components
    if relative_path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(GitError::PathTraversal {
            attempted_path: file_path.to_string(),
        });
    }

    Ok(relative_path.to_path_buf())
}

/// Normalize path separators to forward slashes for Git compatibility
/// Git internally uses forward slashes regardless of platform
pub fn normalize_git_path(path: &str) -> String {
    path.replace('\\', "/")
}

pub fn format_timestamp(time: git2::Time) -> String {
    // Simple timestamp formatting without external dependencies
    format!("{}", time.seconds())
}

pub fn is_valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.contains('\0')
        && !name.ends_with('/')
        && !name.ends_with(".lock")
}

pub fn is_valid_tag_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.contains('\0')
        && !name.ends_with('/')
        && !name.ends_with(".lock")
}

// Two `has_uncommitted_changes` helpers lived here (a GitError one and a
// napi one, plus an anyhow one before that), all three unreferenced. The only
// live answer to that question is computed inline in
// `repository_ops::get_repository_info_impl`, and deliberately not on the
// same terms: these helpers set `include_untracked(false)`, while the live
// code passes `None` and takes libgit2's defaults. Keeping a dead helper that
// answers a subtly different question than the live code is a trap for
// whoever reaches for it next, so they are gone rather than merged.

/// Read user signature from git config with lenient validation
///
/// This helper resolves user name and email for git operations (commits, tags).
/// It implements the hybrid signature strategy: explicit parameters override config,
/// empty/whitespace parameters fall back to config.
///
/// # Arguments
/// * `repo` - Repository reference
/// * `explicit_name` - Optional explicit user name (Some("") treated as None)
/// * `explicit_email` - Optional explicit user email (Some("") treated as None)
///
/// # Behavior
/// 1. If explicit value is non-empty → use it
/// 2. If explicit value is empty/whitespace OR None → read from config (repo → global)
/// 3. If config missing → return GitError::ConfigMissing
///
/// # Returns
/// * `Ok(Signature)` - Valid signature constructed
/// * `Err(GitError::ConfigMissing)` - Required config key not found
/// * `Err(GitError::GitOperationFailure)` - Failed to create signature
///
/// # Example
///
/// Not run: `read_user_signature` is `pub(crate)`, so a doc test — which is
/// compiled as an external consumer of the crate — cannot call it.
///
/// ```ignore
/// // Use explicit override
/// let sig = read_user_signature(&repo, Some("Alice"), Some("alice@example.com"))?;
///
/// // Read from config (empty string treated as None)
/// let sig = read_user_signature(&repo, Some(""), None)?;
///
/// // Read both from config
/// let sig = read_user_signature(&repo, None, None)?;
/// ```
pub(crate) fn read_user_signature<'a>(
    repo: &'a git2::Repository,
    explicit_name: Option<&str>,
    explicit_email: Option<&str>,
) -> Result<git2::Signature<'a>, GitError> {
    use crate::repository_ops::get_config_impl;

    // Get repository path (working tree for non-bare, .git dir for bare)
    // For non-bare repos: repo.workdir() = /repo/, repo.path() = /repo/.git/
    // For bare repos: repo.workdir() = None, repo.path() = /repo.git/
    let repo_path = if let Some(workdir) = repo.workdir() {
        // Non-bare repo: use working tree path
        workdir.to_str()
    } else {
        // Bare repo: use .git directory path
        repo.path().to_str()
    }
    .ok_or_else(|| GitError::InvalidRepository {
        path: "(unknown)".to_string(),
    })?;

    // Resolve name with lenient validation
    let name = match explicit_name {
        Some(val) if !val.trim().is_empty() => {
            // Non-empty explicit value wins
            val.to_string()
        }
        Some(_) | None => {
            // Empty/whitespace/None → read from config
            get_config_impl(repo_path, "user.name", true)?.ok_or_else(|| {
                GitError::ConfigMissing {
                    key: "user.name".to_string(),
                    tried_locations: vec![
                        "repository config".to_string(),
                        "global config".to_string(),
                    ],
                }
            })?
        }
    };

    // Resolve email with lenient validation
    let email = match explicit_email {
        Some(val) if !val.trim().is_empty() => {
            // Non-empty explicit value wins
            val.to_string()
        }
        Some(_) | None => {
            // Empty/whitespace/None → read from config
            get_config_impl(repo_path, "user.email", true)?.ok_or_else(|| {
                GitError::ConfigMissing {
                    key: "user.email".to_string(),
                    tried_locations: vec![
                        "repository config".to_string(),
                        "global config".to_string(),
                    ],
                }
            })?
        }
    };

    // Create signature
    git2::Signature::now(&name, &email).map_err(|e| GitError::GitOperationFailure {
        operation: "create_signature".to_string(),
        class: e.class() as i32,
        code: e.code() as i32,
        message: e.message().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Every rule these two functions actually enforce.
    ///
    /// The only coverage they had was a smoke test on the deleted
    /// `GitServiceCore`, asserting the empty case and one good name — four
    /// of the twelve branches below. The two functions are byte-identical
    /// today, so they are tested against the same table; if they ever
    /// diverge (git's tag rules are not quite git's branch rules), this is
    /// where the divergence gets written down.
    #[test]
    fn ref_name_validity_rules() {
        let valid = ["main", "feature/thing", "v1.0.0", "release-2"];
        let invalid = [
            "",               // empty
            "-leading-dash",  // git reads a leading dash as a flag
            "a..b",           // reserved range syntax
            "with\0null",     // NUL is never legal in a ref name
            "trailing/",      // a ref cannot end in a separator
            "something.lock", // collides with git's lockfile convention
        ];

        for name in valid {
            assert!(
                is_valid_branch_name(name),
                "branch {name:?} should be valid"
            );
            assert!(is_valid_tag_name(name), "tag {name:?} should be valid");
        }
        for name in invalid {
            assert!(
                !is_valid_branch_name(name),
                "branch {name:?} should be rejected"
            );
            assert!(!is_valid_tag_name(name), "tag {name:?} should be rejected");
        }
    }

    /// Different spellings of one repository must share one lock.
    ///
    /// This is the property the registry exists for, and the one a raw-string
    /// key quietly failed to provide. `Arc::ptr_eq` is the assertion that
    /// matters: two equal-but-distinct `Mutex<()>` values would satisfy any
    /// weaker comparison while excluding nothing.
    #[test]
    fn repo_lock_is_keyed_by_identity_not_spelling() {
        let (_temp, repo_path) = setup_test_repo();
        let canonical = std::fs::canonicalize(&repo_path).unwrap();
        let base = canonical.to_string_lossy().into_owned();

        let spellings = [
            base.clone(),
            format!("{base}/"),                       // trailing separator
            format!("{base}/."),                      // same directory, said the long way
            format!("{base}/sub/.."),                 // a round trip through a child
            repo_path.to_string_lossy().into_owned(), // possibly a symlinked /tmp
        ];
        std::fs::create_dir_all(canonical.join("sub")).unwrap();

        let first = repo_lock(&spellings[0]);
        for spelling in &spellings[1..] {
            assert!(
                Arc::ptr_eq(&first, &repo_lock(spelling)),
                "{spelling:?} got a different lock than {:?} — two writers to \
                 the same repository would run concurrently",
                spellings[0],
            );
        }
    }

    /// The converse: distinct repositories must not contend.
    #[test]
    fn repo_lock_separates_distinct_repos() {
        let (_temp_a, path_a) = setup_test_repo();
        let (_temp_b, path_b) = setup_test_repo();

        assert!(!Arc::ptr_eq(
            &repo_lock(&path_a.to_string_lossy()),
            &repo_lock(&path_b.to_string_lossy()),
        ));
    }

    fn setup_test_repo() -> (tempfile::TempDir, PathBuf) {
        let temp_dir =
            tempfile::TempDir::new_in(std::env::temp_dir()).expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize a git repository
        git2::Repository::init(&repo_path).expect("Failed to initialize test repository");

        (temp_dir, repo_path)
    }

    #[test]
    fn test_read_user_signature_with_explicit_values() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = git2::Repository::open(&repo_path).expect("Failed to open repository");

        // Test with explicit name and email
        let result = read_user_signature(&repo, Some("Test User"), Some("test@example.com"));

        assert!(result.is_ok());
        let signature = result.unwrap();
        assert_eq!(signature.name(), Some("Test User"));
        assert_eq!(signature.email(), Some("test@example.com"));
    }

    #[test]
    fn test_read_user_signature_from_config() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = git2::Repository::open(&repo_path).expect("Failed to open repository");

        // Set user.name and user.email in repo config
        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("user.name", "Config User")
            .expect("Failed to set user.name");
        config
            .set_str("user.email", "config@example.com")
            .expect("Failed to set user.email");

        // Test reading from config (None parameters)
        let result = read_user_signature(&repo, None, None);

        assert!(result.is_ok());
        let signature = result.unwrap();
        assert_eq!(signature.name(), Some("Config User"));
        assert_eq!(signature.email(), Some("config@example.com"));
    }

    #[test]
    fn test_read_user_signature_lenient_validation() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = git2::Repository::open(&repo_path).expect("Failed to open repository");

        // Set user.name and user.email in repo config
        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("user.name", "Fallback User")
            .expect("Failed to set user.name");
        config
            .set_str("user.email", "fallback@example.com")
            .expect("Failed to set user.email");

        // Test with empty strings (should fall back to config)
        let result = read_user_signature(&repo, Some(""), Some("   "));

        assert!(result.is_ok());
        let signature = result.unwrap();
        assert_eq!(signature.name(), Some("Fallback User"));
        assert_eq!(signature.email(), Some("fallback@example.com"));
    }

    #[test]
    fn test_read_user_signature_explicit_overrides_config() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = git2::Repository::open(&repo_path).expect("Failed to open repository");

        // Set user.name and user.email in repo config
        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("user.name", "Config User")
            .expect("Failed to set user.name");
        config
            .set_str("user.email", "config@example.com")
            .expect("Failed to set user.email");

        // Test that explicit values override config
        let result =
            read_user_signature(&repo, Some("Explicit User"), Some("explicit@example.com"));

        assert!(result.is_ok());
        let signature = result.unwrap();
        assert_eq!(signature.name(), Some("Explicit User"));
        assert_eq!(signature.email(), Some("explicit@example.com"));
    }

    #[test]
    fn test_read_user_signature_missing_name_config() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = git2::Repository::open(&repo_path).expect("Failed to open repository");

        // Set only email but not name in repo config
        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("user.email", "test@example.com")
            .expect("Failed to set user.email");

        // Unset user.name if it exists (from global config)
        let _ = config.remove("user.name");

        // Test that missing user.name returns ConfigMissing error
        // Note: This may succeed if global config has user.name, which is common
        // So we make this test lenient - either ConfigMissing or success is acceptable
        let result = read_user_signature(&repo, None, None);

        if result.is_err() {
            match result {
                Err(GitError::ConfigMissing {
                    key,
                    tried_locations,
                }) => {
                    // Should fail on user.name
                    assert_eq!(key, "user.name");
                    assert_eq!(tried_locations.len(), 2);
                    assert!(tried_locations.contains(&"repository config".to_string()));
                    assert!(tried_locations.contains(&"global config".to_string()));
                }
                _ => panic!("Expected ConfigMissing error if error occurs"),
            }
        }
        // If it succeeds, that's also OK (global config provided user.name)
    }

    #[test]
    fn test_read_user_signature_partial_explicit() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = git2::Repository::open(&repo_path).expect("Failed to open repository");

        // Set user.email in config
        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("user.email", "config@example.com")
            .expect("Failed to set user.email");

        // Test with explicit name but config email
        let result = read_user_signature(&repo, Some("Explicit User"), None);

        assert!(result.is_ok());
        let signature = result.unwrap();
        assert_eq!(signature.name(), Some("Explicit User"));
        assert_eq!(signature.email(), Some("config@example.com"));
    }
}
