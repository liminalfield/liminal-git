// utils.rs
use crate::errors::GitError;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(feature = "napi-binding")]
use napi::Error as NapiError;
#[cfg(feature = "napi-binding")]
use napi::Status;

/// How long to wait for a repository lock before giving up.
///
/// Blocking forever is the wrong failure: a wedged or stopped holder would hang
/// the caller with no diagnosis. A bounded wait turns that into a retriable
/// `RepositoryLocked` naming the path and the time spent. Ten seconds is far
/// longer than any operation here takes and short enough that a person notices
/// an error rather than a freeze.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll interval while waiting. Deliberately not a blocking `flock` call — see
/// `lock_repo`, which needs a deadline that a blocking acquire cannot give it.
const LOCK_POLL: Duration = Duration::from_millis(25);

/// In-process half of the lock (#390), keyed by canonicalised repo path.
///
/// The values are `&'static` rather than `Arc` so that a guard can own its
/// `MutexGuard<'static, ()>` without a self-referential struct. Leaking is
/// sound here because the registry never removed entries anyway: it is bounded
/// by the number of distinct repositories a process touches.
static REPO_LOCKS: Mutex<BTreeMap<String, &'static Mutex<()>>> = Mutex::new(BTreeMap::new());

/// Held for the duration of a mutating operation. Releases on drop.
///
/// Fields drop in declaration order: the file lock is released first, then the
/// in-process mutex. Either order is correct; this one is stated so a later
/// reordering is a visible decision rather than an accident.
#[must_use = "the lock is released as soon as the guard is dropped, so binding \
              it to `_` protects nothing"]
pub struct RepoLock {
    _file: File,
    _process: MutexGuard<'static, ()>,
}

/// Resolve a caller's path string to the two things a lock is identified by:
/// the registry key and the lock file.
///
/// Split out from `lock_repo` so it can be tested directly. The property that
/// matters — that every spelling of one repository resolves to one identity —
/// is otherwise only observable by watching two threads contend, which is a
/// slow and indirect way to assert something exact.
pub(crate) fn lock_identity(repo_path: &str) -> Result<(String, PathBuf), GitError> {
    let canonical = std::fs::canonicalize(repo_path).map_err(|e| GitError::IoError {
        operation: "canonicalize_repo_path".to_string(),
        error: format!("{}: {}", repo_path, e),
    })?;

    let key = canonical.to_string_lossy().into_owned();
    let path = lock_file_path(&canonical);
    Ok((key, path))
}

/// Where the lock file lives for a repository.
///
/// Inside the git directory, which pins the lock to the *repository* rather
/// than to a user or a machine. A path under `$TMPDIR` or `~/.local/state`
/// would be simpler and wrong: two users sharing a repository on a network
/// volume would take different locks and exclude nobody. `.git/` also keeps it
/// out of `git status`, which a file in the working tree would not.
fn lock_file_path(repo_path: &Path) -> PathBuf {
    let dot_git = repo_path.join(".git");

    // A worktree or submodule has `.git` as a *file* containing "gitdir: …".
    // Following it keeps every checkout of one repository on one lock.
    if dot_git.is_file()
        && let Ok(contents) = std::fs::read_to_string(&dot_git)
        && let Some(rest) = contents.trim().strip_prefix("gitdir:")
    {
        let resolved = PathBuf::from(rest.trim());
        let resolved = if resolved.is_absolute() {
            resolved
        } else {
            repo_path.join(resolved)
        };
        return resolved.join("liminal-git.lock");
    }

    dot_git.join("liminal-git.lock")
}

/// Take the mutating-operation lock for a repository.
///
/// Two layers, because one is not enough:
///
/// * An in-process mutex, which is all this used to be. It serialises threads
///   within one process and nothing else.
/// * An OS advisory file lock, which serialises *processes*. Without it, a
///   second application instance, a CLI built on this library, or a background
///   job in another process could interleave with an operation here and corrupt
///   the index.
///
/// Note precisely what the second layer does and does not cover. It excludes
/// other users of *this library*, because exclusion requires agreeing on the
/// same lock file. It does not exclude `git` itself: a commit run from a
/// terminal knows nothing about `.git/liminal-git.lock`. Git defends its own
/// index with `.git/index.lock`, which gives partial overlap by accident rather
/// than by coordination. Taking git's lock instead was considered and rejected
/// — holding it across a whole multi-step operation would make ordinary git
/// commands fail in confusing ways, and libgit2 takes it internally already.
///
/// The advisory lock is deliberately an `flock`/`LockFileEx` on an open file
/// rather than a sentinel file that is created and deleted. The kernel drops
/// the lock when the descriptor closes — including when the process is killed
/// or crashes — so there is no stale lock to detect, no owner PID to record,
/// and no cleanup path to get wrong. A sentinel file would need all three, and
/// would deadlock the repository the first time a process died at the wrong
/// moment.
///
/// The key is the canonicalised path, not the caller's string. One repository
/// can be named `/srv/repo`, `/srv/repo/`, a relative path, or a symlink into the
/// real location; keyed literally, those are different entries, and both
/// callers proceed at once — a mutual exclusion that silently isn't one, in
/// exactly the situation the lock exists for.
pub fn lock_repo(repo_path: &str) -> Result<RepoLock, GitError> {
    let (key, path) = lock_identity(repo_path)?;

    let process_mutex: &'static Mutex<()> = {
        let mut locks = REPO_LOCKS.lock().unwrap_or_else(|p| p.into_inner());
        locks
            .entry(key.clone())
            .or_insert_with(|| Box::leak(Box::new(Mutex::new(()))))
    };
    let process_guard = process_mutex.lock().unwrap_or_else(|p| p.into_inner());

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| GitError::IoError {
            operation: "open_repo_lock_file".to_string(),
            error: format!("{}: {}", path.display(), e),
        })?;

    // try_lock in a loop rather than a blocking acquire, because a blocking
    // acquire has no deadline and a wedged holder would hang the caller.
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => {
                return Ok(RepoLock {
                    _file: file,
                    _process: process_guard,
                });
            }
            Err(TryLockError::WouldBlock) => {
                if started.elapsed() >= LOCK_TIMEOUT {
                    return Err(GitError::RepositoryLocked {
                        path: key,
                        waited_ms: started.elapsed().as_millis() as u64,
                    });
                }
                std::thread::sleep(LOCK_POLL);
            }
            Err(TryLockError::Error(e)) => {
                return Err(GitError::IoError {
                    operation: "lock_repository".to_string(),
                    error: e.to_string(),
                });
            }
        }
    }
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
        // No napi status means "try again shortly", so this rides on
        // GenericFailure. Callers distinguish it by the structured error's
        // code (REPOSITORY_LOCKED) and its retriable flag, which is the only
        // variant here that sets it for a reason other than I/O.
        GitError::RepositoryLocked { .. } => Status::GenericFailure,
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

// Two `has_uncommitted_changes` helpers lived here (a GitError one and a napi
// one, plus an anyhow one before that), all three unreferenced. The only live
// answer to that question is computed inline in
// `repository_ops::get_repository_info_impl`, and not on the same terms:
// these helpers set `include_untracked(false)`, so a repository holding only
// a new file looked clean to them. Keeping a dead helper that answers a
// subtly different question than the live code is a trap for whoever reaches
// for it next, so they are gone rather than merged.

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

    /// Every spelling of one repository must resolve to one lock identity.
    ///
    /// This is the property the registry exists for, and the one a raw-string
    /// key quietly failed to provide: distinct strings meant distinct mutexes,
    /// so two writers to the same repository both proceeded.
    #[test]
    fn lock_identity_collapses_spellings() {
        let (_temp, repo_path) = setup_test_repo();
        let canonical = std::fs::canonicalize(&repo_path).unwrap();
        std::fs::create_dir_all(canonical.join("sub")).unwrap();

        // Spellings are built from the *non-canonical* path, and joined with
        // the platform separator rather than "/".
        //
        // Both matter on Windows, and this test failed there for getting it
        // wrong. `canonicalize` returns a verbatim path — \\?\C:\... — and
        // verbatim paths are passed to the filesystem without normalisation,
        // so a forward slash is a literal character rather than a separator.
        // Appending "/" produced \\?\C:\...\tmpdir/ and os error 123, "the
        // filename, directory name, or volume label syntax is incorrect".
        //
        // The canonical form is still included below, as the last spelling, so
        // it is exercised as an input — just not used to build the others.
        let sep = std::path::MAIN_SEPARATOR;
        let base = repo_path.to_string_lossy().into_owned();

        let spellings = [
            base.clone(),
            format!("{base}{sep}"),           // trailing separator
            format!("{base}{sep}."),          // the same directory, said the long way
            format!("{base}{sep}sub{sep}.."), // a round trip through a child
            canonical.to_string_lossy().into_owned(), // already canonical, and on Unix
                                              // possibly a symlinked /tmp
        ];

        let first = lock_identity(&spellings[0]).expect("resolve identity");
        for spelling in &spellings[1..] {
            let other = lock_identity(spelling).expect("resolve identity");
            assert_eq!(
                first, other,
                "{spelling:?} resolved to a different lock than {:?} — two \
                 writers to the same repository would run concurrently",
                spellings[0],
            );
        }
    }

    /// The converse: distinct repositories must not contend.
    #[test]
    fn lock_identity_separates_distinct_repos() {
        let (_temp_a, path_a) = setup_test_repo();
        let (_temp_b, path_b) = setup_test_repo();

        let a = lock_identity(&path_a.to_string_lossy()).unwrap();
        let b = lock_identity(&path_b.to_string_lossy()).unwrap();
        assert_ne!(a, b);
    }

    /// The lock file belongs to the repository, not the working tree.
    ///
    /// Inside `.git/` it is invisible to `git status`. In the working tree it
    /// would show up as an untracked file in every repository the library
    /// touches — which, for an application managing a user's documents, means
    /// junk appearing in their project.
    #[test]
    fn lock_file_lives_inside_the_git_directory() {
        let (_temp, repo_path) = setup_test_repo();
        let (_key, lock_path) = lock_identity(&repo_path.to_string_lossy()).unwrap();

        assert_eq!(lock_path.file_name().unwrap(), "liminal-git.lock");
        assert_eq!(lock_path.parent().unwrap().file_name().unwrap(), ".git");
    }

    /// Two threads must not hold the lock at once.
    ///
    /// Asserted by timing rather than by inspection: the second acquisition
    /// cannot complete until the first releases, so it must take at least as
    /// long as the first holder slept. A test that merely acquired twice in
    /// sequence would pass against a lock that does nothing.
    #[test]
    fn lock_excludes_a_second_thread() {
        let (_temp, repo_path) = setup_test_repo();
        let path = repo_path.to_string_lossy().into_owned();
        let hold = Duration::from_millis(250);

        let guard = lock_repo(&path).expect("first acquire");

        let other = {
            let path = path.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                let _guard = lock_repo(&path).expect("second acquire");
                started.elapsed()
            })
        };

        std::thread::sleep(hold);
        drop(guard);

        let waited = other.join().expect("thread join");
        assert!(
            waited >= hold,
            "second acquisition took {waited:?}, less than the {hold:?} the \
             first holder was holding it — the lock is not excluding anything"
        );
    }

    /// The lock must exclude *other processes*, which is the entire reason it
    /// is a file lock and not just the mutex it used to be.
    ///
    /// A separate process probes the lock file directly rather than calling
    /// `lock_repo`, so the assertion is immediate instead of waiting out
    /// `LOCK_TIMEOUT`. The probe runs as a test in a fresh copy of this same
    /// binary, which keeps it portable — spawning `flock(1)` would work only
    /// on Linux.
    #[test]
    fn lock_excludes_another_process() {
        // Re-entered as the child; the real work is in `lock_file_probe`.
        if std::env::var("LIMINAL_LOCK_PROBE").is_ok() {
            return;
        }

        let (_temp, repo_path) = setup_test_repo();
        let path = repo_path.to_string_lossy().into_owned();
        let (_key, lock_path) = lock_identity(&path).unwrap();

        let _guard = lock_repo(&path).expect("parent acquires");

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "utils::tests::lock_file_probe", "--nocapture"])
            .env("LIMINAL_LOCK_PROBE", &lock_path)
            .output()
            .expect("spawn probe process");

        assert!(
            output.status.success(),
            "a separate process was able to take the lock this one holds\n{}",
            String::from_utf8_lossy(&output.stdout),
        );
    }

    /// The child half of `lock_excludes_another_process`. Inert without the
    /// environment variable, so a normal test run does nothing here.
    #[test]
    fn lock_file_probe() {
        let Ok(lock_path) = std::env::var("LIMINAL_LOCK_PROBE") else {
            return;
        };

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .expect("open the lock file the parent holds");

        match file.try_lock() {
            Err(TryLockError::WouldBlock) => {} // correct: the parent holds it
            Ok(()) => panic!("acquired a lock the parent process is holding"),
            Err(e) => panic!("probing the lock failed: {e}"),
        }
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
