use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Serializable error payload for transport across N-API boundary
///
/// This struct bridges the gap between Rust's typed GitError enum and JavaScript.
/// Due to napi-rs 3.3 limitations (napi::Error only accepts status + message string),
/// we serialize this struct to JSON for transport, then parse it on the JS side.
///
/// When napi-rs adds richer error support, we can switch to attaching properties
/// directly without changing the variant logic or JS consumer API.
///
/// Note: Not gated by napi-binding feature because it's pure Rust + serde logic.
/// This allows tests to run without NAPI linking issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedGitError {
    /// Machine-readable error code (e.g., "FILE_NOT_FOUND")
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Whether this error is safe to retry
    pub retriable: bool,
    /// Additional structured details specific to error variant
    pub details: HashMap<String, serde_json::Value>,
}

/// Structured error types for Git operations
///
/// Provides detailed error information with context for better
/// debugging and error handling in calling code.
#[derive(Debug, Clone)]
pub enum GitError {
    // Repository errors
    RepositoryNotFound {
        path: String,
    },
    RepositoryCorrupted {
        path: String,
        details: String,
    },
    InvalidRepository {
        path: String,
    },

    // File errors
    FileNotFound {
        path: String,
    },
    FileNotInRepository {
        path: String,
    },
    PathTraversal {
        attempted_path: String,
    },

    // Operation errors
    NothingToCommit,
    MergeConflict {
        files: Vec<String>,
    },
    UncommittedChanges {
        count: usize,
    },
    UnstagedChangesWouldBeLost {
        files: Vec<String>,
    },
    DetachedHead,
    /// Another holder of the repository lock did not release it in time.
    ///
    /// Unlike every other variant here, nothing is actually wrong: the
    /// repository is intact and the request was valid. It is the one genuinely
    /// retriable failure this library produces, and callers are expected to
    /// treat it that way rather than surfacing it as a fault.
    RepositoryLocked {
        path: String,
        waited_ms: u64,
    },

    // Config errors
    ConfigMissing {
        key: String,
        tried_locations: Vec<String>,
    },

    // Branch/Tag errors
    BranchNotFound {
        name: String,
    },
    BranchAlreadyExists {
        name: String,
    },
    CannotDeleteCurrentBranch {
        name: String,
    },
    BranchNotMerged {
        name: String,
        commits_ahead: u32,
    },
    TagNotFound {
        name: String,
    },
    TagAlreadyExists {
        name: String,
    },

    // Validation errors
    InvalidPath {
        path: String,
        reason: String,
    },
    /// An argument that is not a path failed validation — a commit message, a
    /// user name, a pagination limit. `argument` names the parameter so a
    /// caller can point at the right field; `reason` is the message shown to
    /// the user and is what crosses the N-API boundary verbatim.
    InvalidArgument {
        argument: String,
        reason: String,
    },
    InvalidCommitHash {
        hash: String,
    },
    InvalidBranchName {
        name: String,
    },
    InvalidTagName {
        name: String,
    },

    // System errors
    IoError {
        operation: String,
        error: String,
    },

    // Git operation failures (renamed from GitError to avoid confusion with enum name)
    GitOperationFailure {
        operation: String,
        class: i32, // git2::ErrorClass as i32
        code: i32,  // git2::ErrorCode as i32
        message: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GitError::RepositoryNotFound { path } => write!(f, "Repository not found: {}", path),
            GitError::RepositoryCorrupted { path, details } => {
                write!(f, "Repository corrupted at {}: {}", path, details)
            }
            GitError::InvalidRepository { path } => write!(f, "Invalid repository: {}", path),

            GitError::FileNotFound { path } => write!(f, "File not found: {}", path),
            GitError::FileNotInRepository { path } => write!(f, "File not in repository: {}", path),
            GitError::PathTraversal { attempted_path } => {
                write!(f, "Path traversal attempt: {}", attempted_path)
            }

            GitError::NothingToCommit => write!(f, "Nothing to commit"),
            GitError::MergeConflict { files } => {
                write!(f, "Merge conflict in {} file(s): {:?}", files.len(), files)
            }
            GitError::UncommittedChanges { count } => {
                write!(f, "Uncommitted changes: {} file(s)", count)
            }
            GitError::UnstagedChangesWouldBeLost { files } => write!(
                f,
                "Operation would lose uncommitted changes in {} file(s)",
                files.len()
            ),
            GitError::DetachedHead => write!(f, "Detached HEAD state"),
            GitError::RepositoryLocked { path, waited_ms } => write!(
                f,
                "Repository is locked by another process: {} (waited {}ms)",
                path, waited_ms
            ),

            GitError::ConfigMissing {
                key,
                tried_locations,
            } => write!(
                f,
                "Required config key '{}' not found in: {}",
                key,
                tried_locations.join(", ")
            ),

            GitError::BranchNotFound { name } => write!(f, "Branch not found: {}", name),
            GitError::BranchAlreadyExists { name } => write!(f, "Branch already exists: {}", name),
            GitError::CannotDeleteCurrentBranch { name } => {
                write!(f, "Cannot delete current branch: {}", name)
            }
            GitError::BranchNotMerged {
                name,
                commits_ahead,
            } => write!(
                f,
                "Branch '{}' not merged ({} commits ahead)",
                name, commits_ahead
            ),

            GitError::TagNotFound { name } => write!(f, "Tag not found: {}", name),
            GitError::TagAlreadyExists { name } => write!(f, "Tag already exists: {}", name),

            GitError::InvalidPath { path, reason } => {
                write!(f, "Invalid path '{}': {}", path, reason)
            }
            GitError::InvalidArgument { argument, reason } => {
                write!(f, "Invalid argument '{}': {}", argument, reason)
            }
            GitError::InvalidCommitHash { hash } => write!(f, "Invalid commit hash: {}", hash),
            GitError::InvalidBranchName { name } => write!(f, "Invalid branch name: {}", name),
            GitError::InvalidTagName { name } => write!(f, "Invalid tag name: {}", name),

            GitError::IoError { operation, error } => {
                write!(f, "I/O error during '{}': {}", operation, error)
            }

            GitError::GitOperationFailure {
                operation,
                class,
                code,
                message,
            } => write!(
                f,
                "Git operation '{}' failed (class={}, code={}): {}",
                operation, class, code, message
            ),
        }
    }
}

impl std::error::Error for GitError {}

/// Convert to napi::Error at the N-API boundary.
///
/// This exists so the validators in `validation.rs` can return GitError — and
/// therefore be tested without linking napi — while the ~79 call sites in
/// git_service.rs keep using a bare `?`.
///
/// The two validation variants map to `Status::InvalidArg` carrying the bare
/// `reason`, which is byte-for-byte the message those call sites produced when
/// validation returned `napi::Error` directly. JS callers see no change.
///
/// Errors from git operations do not travel this path — they go through
/// `utils::run_blocking`, which honours the `structured_errors` feature flag.
/// The generic arm here is a backstop, not the main road.
#[cfg(feature = "napi-binding")]
impl From<GitError> for napi::Error {
    fn from(err: GitError) -> Self {
        match err {
            GitError::InvalidPath { ref reason, .. }
            | GitError::InvalidArgument { ref reason, .. } => {
                napi::Error::new(napi::Status::InvalidArg, reason.clone())
            }
            other => napi::Error::new(napi::Status::GenericFailure, other.to_string()),
        }
    }
}

/// Convert from git2::Error to GitError
///
/// Note: The operation context is lost during automatic conversion.
/// Prefer using `with_operation()` to add context.
impl From<git2::Error> for GitError {
    fn from(err: git2::Error) -> Self {
        GitError::GitOperationFailure {
            operation: "unknown".to_string(),
            class: err.class() as i32,
            code: err.code() as i32,
            message: err.message().to_string(),
        }
    }
}

/// Convert from std::io::Error to GitError
impl From<std::io::Error> for GitError {
    fn from(err: std::io::Error) -> Self {
        GitError::IoError {
            operation: "io operation".to_string(),
            error: err.to_string(),
        }
    }
}

impl GitError {
    /// Add operation context to a GitOperationFailure
    ///
    /// Example (a fragment — `repo` and `name` come from the caller):
    /// ```ignore
    /// let result = repo.find_branch(name, BranchType::Local)
    ///     .map_err(|e| GitError::from(e).with_operation("find_branch"))?;
    /// ```
    pub fn with_operation(self, operation: impl Into<String>) -> Self {
        match self {
            GitError::GitOperationFailure {
                class,
                code,
                message,
                ..
            } => GitError::GitOperationFailure {
                operation: operation.into(),
                class,
                code,
                message,
            },
            other => other,
        }
    }

    /// Add I/O operation context
    pub fn with_io_operation(self, operation: impl Into<String>) -> Self {
        match self {
            GitError::IoError { error, .. } => GitError::IoError {
                operation: operation.into(),
                error,
            },
            other => other,
        }
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            GitError::IoError { .. }
                | GitError::RepositoryCorrupted { .. }
                | GitError::RepositoryLocked { .. }
        )
    }

    /// Get error code for structured error responses
    pub fn error_code(&self) -> &'static str {
        match self {
            GitError::RepositoryNotFound { .. } => "REPOSITORY_NOT_FOUND",
            GitError::RepositoryCorrupted { .. } => "REPOSITORY_CORRUPTED",
            GitError::InvalidRepository { .. } => "INVALID_REPOSITORY",
            GitError::FileNotFound { .. } => "FILE_NOT_FOUND",
            GitError::FileNotInRepository { .. } => "FILE_NOT_IN_REPOSITORY",
            GitError::PathTraversal { .. } => "PATH_TRAVERSAL",
            GitError::NothingToCommit => "NOTHING_TO_COMMIT",
            GitError::MergeConflict { .. } => "MERGE_CONFLICT",
            GitError::UncommittedChanges { .. } => "UNCOMMITTED_CHANGES",
            GitError::UnstagedChangesWouldBeLost { .. } => "UNSTAGED_CHANGES_WOULD_BE_LOST",
            GitError::DetachedHead => "DETACHED_HEAD",
            GitError::RepositoryLocked { .. } => "REPOSITORY_LOCKED",
            GitError::ConfigMissing { .. } => "CONFIG_MISSING",
            GitError::BranchNotFound { .. } => "BRANCH_NOT_FOUND",
            GitError::BranchAlreadyExists { .. } => "BRANCH_ALREADY_EXISTS",
            GitError::CannotDeleteCurrentBranch { .. } => "CANNOT_DELETE_CURRENT_BRANCH",
            GitError::BranchNotMerged { .. } => "BRANCH_NOT_MERGED",
            GitError::TagNotFound { .. } => "TAG_NOT_FOUND",
            GitError::TagAlreadyExists { .. } => "TAG_ALREADY_EXISTS",
            GitError::InvalidPath { .. } => "INVALID_PATH",
            GitError::InvalidArgument { .. } => "INVALID_ARGUMENT",
            GitError::InvalidCommitHash { .. } => "INVALID_COMMIT_HASH",
            GitError::InvalidBranchName { .. } => "INVALID_BRANCH_NAME",
            GitError::InvalidTagName { .. } => "INVALID_TAG_NAME",
            GitError::IoError { .. } => "IO_ERROR",
            GitError::GitOperationFailure { .. } => "GIT_OPERATION_FAILURE",
        }
    }

    /// Build details object for structured errors (FUTURE USE)
    ///
    /// This method creates a proper JavaScript Object with typed properties using napi-rs.
    /// It's currently UNUSED because napi-rs 3.3 doesn't support attaching properties to
    /// napi::Error - we use JSON serialization via `to_serializable()` instead.
    ///
    /// **Why keep this?** When napi-rs adds native structured error support, we can switch
    /// to using this method without rewriting the variant logic. The exhaustive match ensures
    /// compiler enforcement when new error variants are added.
    ///
    /// This method has an exhaustive match - compiler will error if new variants are added
    /// without updating this method. Every variant must populate at least one property.
    #[cfg(feature = "napi-binding")]
    #[allow(dead_code)]
    pub fn build_details_object(
        &self,
        env: &napi::Env,
    ) -> napi::Result<napi::bindgen_prelude::Object<'_>> {
        use napi::bindgen_prelude::{Array, Object};

        let mut details = Object::new(env)?;

        match self {
            GitError::RepositoryNotFound { path } => {
                details.set("path", path.as_str())?;
            }
            GitError::RepositoryCorrupted {
                path,
                details: error_details,
            } => {
                details.set("path", path.as_str())?;
                details.set("errorDetails", error_details.as_str())?;
            }
            GitError::InvalidRepository { path } => {
                details.set("path", path.as_str())?;
            }
            GitError::FileNotFound { path } => {
                details.set("path", path.as_str())?;
            }
            GitError::FileNotInRepository { path } => {
                details.set("path", path.as_str())?;
            }
            GitError::PathTraversal { attempted_path } => {
                details.set("attemptedPath", attempted_path.as_str())?;
            }
            GitError::NothingToCommit => {
                // No additional details for this variant
            }
            GitError::MergeConflict { files } => {
                let files_strs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
                let files_array = Array::from_vec(env, files_strs)?;
                details.set("files", files_array)?;
            }
            GitError::UncommittedChanges { count } => {
                details.set("count", *count as u32)?;
            }
            GitError::UnstagedChangesWouldBeLost { files } => {
                let files_strs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
                let files_array = Array::from_vec(env, files_strs)?;
                details.set("files", files_array)?;
                details.set("count", files.len() as u32)?;
            }
            GitError::DetachedHead => {
                // No additional details for this variant
            }
            GitError::RepositoryLocked { path, waited_ms } => {
                details.set("path", path.as_str())?;
                details.set("waitedMs", *waited_ms as u32)?;
            }
            GitError::ConfigMissing {
                key,
                tried_locations,
            } => {
                details.set("key", key.as_str())?;
                let locations_strs: Vec<&str> =
                    tried_locations.iter().map(|s| s.as_str()).collect();
                let locations_array = Array::from_vec(env, locations_strs)?;
                details.set("triedLocations", locations_array)?;
            }
            GitError::BranchNotFound { name } => {
                details.set("name", name.as_str())?;
            }
            GitError::BranchAlreadyExists { name } => {
                details.set("name", name.as_str())?;
            }
            GitError::CannotDeleteCurrentBranch { name } => {
                details.set("name", name.as_str())?;
            }
            GitError::BranchNotMerged {
                name,
                commits_ahead,
            } => {
                details.set("name", name.as_str())?;
                details.set("commitsAhead", *commits_ahead)?;
            }
            GitError::TagNotFound { name } => {
                details.set("name", name.as_str())?;
            }
            GitError::TagAlreadyExists { name } => {
                details.set("name", name.as_str())?;
            }
            GitError::InvalidPath { path, reason } => {
                details.set("path", path.as_str())?;
                details.set("reason", reason.as_str())?;
            }
            GitError::InvalidArgument { argument, reason } => {
                details.set("argument", argument.as_str())?;
                details.set("reason", reason.as_str())?;
            }
            GitError::InvalidCommitHash { hash } => {
                details.set("hash", hash.as_str())?;
            }
            GitError::InvalidBranchName { name } => {
                details.set("name", name.as_str())?;
            }
            GitError::InvalidTagName { name } => {
                details.set("name", name.as_str())?;
            }
            GitError::IoError { operation, error } => {
                details.set("operation", operation.as_str())?;
                details.set("error", error.as_str())?;
            }
            GitError::GitOperationFailure {
                operation,
                class,
                code,
                message,
            } => {
                details.set("operation", operation.as_str())?;
                details.set("class", *class)?;
                details.set("code", *code)?;
                details.set("gitMessage", message.as_str())?;
            } // No default case - compiler enforces exhaustiveness
        }

        Ok(details)
    }

    /// Convert GitError to serializable form for JSON transport
    ///
    /// This method has an exhaustive match - compiler will error if new variants are added.
    /// Uses the same variant coverage as build_details_object() to ensure consistency.
    ///
    /// # Transport Constraint
    /// napi-rs 3.3 only supports status + message string for errors. We serialize this
    /// struct to JSON and embed it in the message field. JavaScript consumers use
    /// parseStructuredGitError() to reconstruct the typed error.
    ///
    /// When napi-rs adds native structured error support, we can switch to using
    /// build_details_object() directly without changing this logic.
    ///
    /// Note: Not gated by napi-binding because it's pure Rust + serde, allowing
    /// comprehensive test coverage without NAPI linking issues.
    pub fn to_serializable(&self) -> SerializedGitError {
        let mut details = HashMap::new();

        match self {
            GitError::RepositoryNotFound { path } => {
                details.insert("path".to_string(), serde_json::Value::String(path.clone()));
            }
            GitError::RepositoryCorrupted {
                path,
                details: error_details,
            } => {
                details.insert("path".to_string(), serde_json::Value::String(path.clone()));
                details.insert(
                    "errorDetails".to_string(),
                    serde_json::Value::String(error_details.clone()),
                );
            }
            GitError::InvalidRepository { path } => {
                details.insert("path".to_string(), serde_json::Value::String(path.clone()));
            }
            GitError::FileNotFound { path } => {
                details.insert("path".to_string(), serde_json::Value::String(path.clone()));
            }
            GitError::FileNotInRepository { path } => {
                details.insert("path".to_string(), serde_json::Value::String(path.clone()));
            }
            GitError::PathTraversal { attempted_path } => {
                details.insert(
                    "attemptedPath".to_string(),
                    serde_json::Value::String(attempted_path.clone()),
                );
            }
            GitError::NothingToCommit => {
                // No additional details
            }
            GitError::MergeConflict { files } => {
                let files_array: Vec<serde_json::Value> = files
                    .iter()
                    .map(|f| serde_json::Value::String(f.clone()))
                    .collect();
                details.insert("files".to_string(), serde_json::Value::Array(files_array));
            }
            GitError::UncommittedChanges { count } => {
                details.insert(
                    "count".to_string(),
                    serde_json::Value::Number((*count as u64).into()),
                );
            }
            GitError::UnstagedChangesWouldBeLost { files } => {
                let files_array: Vec<serde_json::Value> = files
                    .iter()
                    .map(|f| serde_json::Value::String(f.clone()))
                    .collect();
                details.insert(
                    "files".to_string(),
                    serde_json::Value::Array(files_array.clone()),
                );
                details.insert(
                    "count".to_string(),
                    serde_json::Value::Number((files.len() as u64).into()),
                );
            }
            GitError::DetachedHead => {
                // No additional details
            }
            GitError::RepositoryLocked { path, waited_ms } => {
                details.insert("path".to_string(), serde_json::Value::String(path.clone()));
                details.insert(
                    "waitedMs".to_string(),
                    serde_json::Value::Number((*waited_ms).into()),
                );
            }
            GitError::ConfigMissing {
                key,
                tried_locations,
            } => {
                details.insert("key".to_string(), serde_json::Value::String(key.clone()));
                let locations_array: Vec<serde_json::Value> = tried_locations
                    .iter()
                    .map(|l| serde_json::Value::String(l.clone()))
                    .collect();
                details.insert(
                    "triedLocations".to_string(),
                    serde_json::Value::Array(locations_array),
                );
            }
            GitError::BranchNotFound { name } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
            }
            GitError::BranchAlreadyExists { name } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
            }
            GitError::CannotDeleteCurrentBranch { name } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
            }
            GitError::BranchNotMerged {
                name,
                commits_ahead,
            } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
                details.insert(
                    "commitsAhead".to_string(),
                    serde_json::Value::Number((*commits_ahead as u64).into()),
                );
            }
            GitError::TagNotFound { name } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
            }
            GitError::TagAlreadyExists { name } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
            }
            GitError::InvalidPath { path, reason } => {
                details.insert("path".to_string(), serde_json::Value::String(path.clone()));
                details.insert(
                    "reason".to_string(),
                    serde_json::Value::String(reason.clone()),
                );
            }
            GitError::InvalidArgument { argument, reason } => {
                details.insert(
                    "argument".to_string(),
                    serde_json::Value::String(argument.clone()),
                );
                details.insert(
                    "reason".to_string(),
                    serde_json::Value::String(reason.clone()),
                );
            }
            GitError::InvalidCommitHash { hash } => {
                details.insert("hash".to_string(), serde_json::Value::String(hash.clone()));
            }
            GitError::InvalidBranchName { name } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
            }
            GitError::InvalidTagName { name } => {
                details.insert("name".to_string(), serde_json::Value::String(name.clone()));
            }
            GitError::IoError { operation, error } => {
                details.insert(
                    "operation".to_string(),
                    serde_json::Value::String(operation.clone()),
                );
                details.insert(
                    "error".to_string(),
                    serde_json::Value::String(error.clone()),
                );
            }
            GitError::GitOperationFailure {
                operation,
                class,
                code,
                message,
            } => {
                details.insert(
                    "operation".to_string(),
                    serde_json::Value::String(operation.clone()),
                );
                details.insert(
                    "class".to_string(),
                    serde_json::Value::Number((*class as i64).into()),
                );
                details.insert(
                    "code".to_string(),
                    serde_json::Value::Number((*code as i64).into()),
                );
                details.insert(
                    "gitMessage".to_string(),
                    serde_json::Value::String(message.clone()),
                );
            } // No default case - compiler enforces exhaustiveness
        }

        SerializedGitError {
            code: self.error_code().to_string(),
            message: format!("{}", self),
            retriable: self.is_retryable(),
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = GitError::FileNotFound {
            path: "/test/file.txt".to_string(),
        };
        assert_eq!(err.to_string(), "File not found: /test/file.txt");
    }

    #[test]
    fn test_error_code() {
        let err = GitError::BranchNotFound {
            name: "main".to_string(),
        };
        assert_eq!(err.error_code(), "BRANCH_NOT_FOUND");
    }

    #[test]
    fn test_with_operation() {
        let err = GitError::GitOperationFailure {
            operation: "unknown".to_string(),
            class: 1,
            code: 2,
            message: "test".to_string(),
        };

        let err_with_op = err.with_operation("test_operation");
        match err_with_op {
            GitError::GitOperationFailure { operation, .. } => {
                assert_eq!(operation, "test_operation");
            }
            _ => panic!("Expected GitOperationFailure"),
        }
    }

    #[test]
    fn test_is_retryable() {
        let retryable = GitError::IoError {
            operation: "read".to_string(),
            error: "timeout".to_string(),
        };
        assert!(retryable.is_retryable());

        let not_retryable = GitError::InvalidBranchName {
            name: "bad name".to_string(),
        };
        assert!(!not_retryable.is_retryable());
    }

    #[test]
    fn test_serialization_simple_variant() {
        let err = GitError::FileNotFound {
            path: "/test/file.txt".to_string(),
        };
        let serialized = err.to_serializable();

        assert_eq!(serialized.code, "FILE_NOT_FOUND");
        assert_eq!(serialized.message, "File not found: /test/file.txt");
        assert!(!serialized.retriable);
        assert_eq!(serialized.details.len(), 1);
        assert_eq!(
            serialized.details.get("path").unwrap(),
            &serde_json::Value::String("/test/file.txt".to_string())
        );
    }

    #[test]
    fn test_serialization_with_multiple_fields() {
        let err = GitError::BranchNotMerged {
            name: "feature-branch".to_string(),
            commits_ahead: 5,
        };
        let serialized = err.to_serializable();

        assert_eq!(serialized.code, "BRANCH_NOT_MERGED");
        assert!(serialized.message.contains("feature-branch"));
        assert!(!serialized.retriable);
        assert_eq!(serialized.details.len(), 2);
        assert_eq!(
            serialized.details.get("name").unwrap(),
            &serde_json::Value::String("feature-branch".to_string())
        );
        assert_eq!(
            serialized.details.get("commitsAhead").unwrap(),
            &serde_json::Value::Number(5u64.into())
        );
    }

    #[test]
    fn test_serialization_with_array() {
        let err = GitError::MergeConflict {
            files: vec!["file1.txt".to_string(), "file2.txt".to_string()],
        };
        let serialized = err.to_serializable();

        assert_eq!(serialized.code, "MERGE_CONFLICT");
        assert_eq!(serialized.details.len(), 1);

        let files = serialized.details.get("files").unwrap();
        match files {
            serde_json::Value::Array(arr) => {
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0], serde_json::Value::String("file1.txt".to_string()));
                assert_eq!(arr[1], serde_json::Value::String("file2.txt".to_string()));
            }
            _ => panic!("Expected array for files"),
        }
    }

    #[test]
    fn test_serialization_no_details() {
        let err = GitError::NothingToCommit;
        let serialized = err.to_serializable();

        assert_eq!(serialized.code, "NOTHING_TO_COMMIT");
        assert_eq!(serialized.message, "Nothing to commit");
        assert!(!serialized.retriable);
        assert_eq!(serialized.details.len(), 0); // No details for this variant
    }

    #[test]
    fn test_serialization_to_json() {
        let err = GitError::InvalidPath {
            path: "/invalid/../path".to_string(),
            reason: "Path traversal detected".to_string(),
        };
        let serialized = err.to_serializable();

        // Verify it can be serialized to JSON
        let json = serde_json::to_string(&serialized).unwrap();
        assert!(json.contains("INVALID_PATH"));
        assert!(json.contains("/invalid/../path"));
        assert!(json.contains("Path traversal detected"));

        // Verify it can be deserialized back
        let deserialized: SerializedGitError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.code, "INVALID_PATH");
        assert_eq!(deserialized.details.len(), 2);
    }

    #[test]
    fn test_serialize_unstaged_changes_would_be_lost() {
        let err = GitError::UnstagedChangesWouldBeLost {
            files: vec!["file1.txt".to_string(), "file2.txt".to_string()],
        };
        let serialized = err.to_serializable();

        assert_eq!(serialized.code, "UNSTAGED_CHANGES_WOULD_BE_LOST");
        assert!(serialized.message.contains("2 file(s)"));
        assert!(!serialized.retriable);
        assert_eq!(serialized.details.len(), 2);

        // Verify files array
        let files = serialized.details.get("files").unwrap();
        match files {
            serde_json::Value::Array(arr) => {
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0], serde_json::Value::String("file1.txt".to_string()));
                assert_eq!(arr[1], serde_json::Value::String("file2.txt".to_string()));
            }
            _ => panic!("Expected array for files"),
        }

        // Verify count
        assert_eq!(
            serialized.details.get("count").unwrap(),
            &serde_json::Value::Number(2u64.into())
        );
    }

    #[test]
    fn test_serialize_config_missing() {
        let err = GitError::ConfigMissing {
            key: "user.name".to_string(),
            tried_locations: vec!["repository config".to_string(), "global config".to_string()],
        };
        let serialized = err.to_serializable();

        assert_eq!(serialized.code, "CONFIG_MISSING");
        assert!(serialized.message.contains("user.name"));
        assert!(serialized.message.contains("repository config"));
        assert!(serialized.message.contains("global config"));
        assert!(!serialized.retriable);
        assert_eq!(serialized.details.len(), 2);

        // Verify key
        assert_eq!(
            serialized.details.get("key").unwrap(),
            &serde_json::Value::String("user.name".to_string())
        );

        // Verify tried_locations array
        let locations = serialized.details.get("triedLocations").unwrap();
        match locations {
            serde_json::Value::Array(arr) => {
                assert_eq!(arr.len(), 2);
                assert_eq!(
                    arr[0],
                    serde_json::Value::String("repository config".to_string())
                );
                assert_eq!(
                    arr[1],
                    serde_json::Value::String("global config".to_string())
                );
            }
            _ => panic!("Expected array for triedLocations"),
        }
    }
}
