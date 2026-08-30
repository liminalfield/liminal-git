// git_service.rs - NAPI bindings (only compiled with napi-binding feature)

use crate::branch_ops;
use crate::feature_flags::FeatureFlags;
use crate::file_ops::*;
use crate::history_ops::*;
use crate::merge_ops;
use crate::remote_ops;
use crate::repository_ops::*;
use crate::tag_ops;
use crate::types::GitStatus;
use crate::types::{
    BranchInfo, CreateBranchOptions, CreateTagOptions, FastForwardResult, MergeAnalysis, TagInfo,
};
use crate::types::{
    CommitDiff, CommitHistory, CommitInfo, DeletedFileEntry, FileAtCommit, FileDiff, TreeEntry,
    TreeFilterOptions,
};
use crate::types::{CommitOptions, MergeOptions, MergeOutcome, ResolvedFile};
use crate::types::{FetchResult, PushResult, RemoteCredentials, RemoteInfo, UpstreamStatus};
use crate::types::{GitConfig, RepositoryConfig, RepositoryHealth, RepositoryInfo};
use crate::utils;
use crate::validation::*;
use log::info;
use napi::Result;
use napi_derive::napi;
use std::sync::Mutex;

/// A repository lock held across a sequence of operations the caller defines.
///
/// Returned by `acquireRepositoryLock`. Not constructible from JavaScript:
/// a lock exists only because it was acquired.
#[napi]
pub struct RepositoryLock {
    scope: Mutex<Option<utils::RepositoryScope>>,
    structured_errors: bool,
}

#[napi]
impl RepositoryLock {
    /// Release the lock. Idempotent, so calling it from a `finally` that also
    /// runs on the success path is safe.
    ///
    /// Releasing waits for any operation already running inside the scope to
    /// finish, because such an operation skipped the advisory lock on the
    /// strength of this scope holding it.
    #[napi]
    pub async fn release(&self) -> Result<()> {
        let taken = self.scope.lock().unwrap_or_else(|p| p.into_inner()).take();
        let structured = self.structured_errors;
        utils::run_blocking(structured, move || {
            if let Some(mut scope) = taken {
                scope.release();
            }
            Ok(())
        })
        .await
    }

    /// Whether this lock is still held. False once `release` has run.
    #[napi(getter)]
    pub fn held(&self) -> bool {
        self.scope
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }
}

#[napi]
pub struct GitService {
    feature_flags: FeatureFlags,
}

#[napi]
impl GitService {
    /// Create a new GitService instance
    ///
    /// Initializes logging if LIMINAL_LOG environment variable is set.
    /// Loads feature flags from LIMINAL_FEATURE_FLAGS environment variable.
    /// Uses try_init() to safely handle multiple instantiations.
    // clippy suggests a Default impl alongside `new()`. Not here: this is a
    // #[napi(constructor)], reached from JavaScript as `new GitService()`, and
    // it has side effects — it initialises the logger and reads feature flags
    // from the environment. A Default impl would be unreachable from JS, be
    // called by nothing in Rust, and imply that constructing one is free.
    #[allow(clippy::new_without_default)]
    #[napi(constructor)]
    pub fn new() -> Self {
        // Initialize logging if LIMINAL_LOG is set
        // Use try_init to avoid panic if logger is already initialized
        // (can happen with multiple GitService instances)
        if std::env::var("LIMINAL_LOG").is_ok() {
            env_logger::builder().is_test(false).try_init().ok();
        }

        // Load feature flags from environment
        let feature_flags = FeatureFlags::from_env();
        info!(
            "GitService initialized with feature flags: structured_errors={}, enhanced_status={}, enhanced_diff={}",
            feature_flags.structured_errors,
            feature_flags.enhanced_status,
            feature_flags.enhanced_diff
        );

        GitService { feature_flags }
    }

    #[napi]
    pub async fn is_repository(&self, path: String) -> Result<bool> {
        validate_repo_path(&path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || Ok(is_repository_impl(&path))).await
    }

    #[napi]
    pub async fn get_status(&self, repo_path: String) -> Result<GitStatus> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || get_status_impl(&repo_path)).await
    }

    /// Stage one path and write a commit.
    ///
    /// **The state on disk is what gets staged.** A path present on disk
    /// stages its content; a tracked path that is no longer on disk stages as
    /// a deletion; a path that is neither on disk nor tracked fails with
    /// `FILE_NOT_FOUND`.
    #[napi]
    pub async fn commit_file(
        &self,
        repo_path: String,
        file_path: String,
        message: String,
        user_name: String,
        user_email: String,
        options: Option<CommitOptions>,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
        validate_committer(options.as_ref())?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            commit_file_impl(
                &repo_path,
                &file_path,
                &message,
                &user_name,
                &user_email,
                options.as_ref(),
            )
        })
        .await
    }

    /// Stage the listed paths and write **one** commit.
    ///
    /// The whole set lands or nothing does. A change spanning several files is
    /// one decision, and splitting it across commits means the history stops
    /// recording decisions, reverting it stops being a single operation, and
    /// between the commits HEAD holds half a change that a reader can observe.
    ///
    /// Each path follows the same rule as `commitFile`: the state on disk is
    /// what gets staged, so a deletion and an edit commit together. Every path
    /// is validated and classified before the index is touched, so a failure
    /// on any path leaves the index and HEAD exactly as they were.
    ///
    /// Repeated paths are de-duplicated, including the absolute and
    /// repository-relative spellings of the same path.
    ///
    /// `NOTHING_TO_COMMIT` is evaluated over the resulting tree, making it a
    /// whole-set check rather than a per-path one. An empty list is
    /// `INVALID_ARGUMENT`, and so is a list longer than 1000 paths.
    #[napi]
    pub async fn commit_files(
        &self,
        repo_path: String,
        file_paths: Vec<String>,
        message: String,
        user_name: String,
        user_email: String,
        options: Option<CommitOptions>,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_file_paths(&file_paths)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
        validate_committer(options.as_ref())?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            commit_files_impl(
                &repo_path,
                &file_paths,
                &message,
                &user_name,
                &user_email,
                options.as_ref(),
            )
        })
        .await
    }

    #[napi]
    pub async fn stage_file(&self, repo_path: String, file_path: String) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            stage_file_impl(&repo_path, &file_path)
        })
        .await
    }

    /// Unstage a file from the index (reset to HEAD state)
    ///
    /// This operation is safe and preserves the working tree. Changes simply
    /// become "unstaged" instead of "staged".
    ///
    /// # Arguments
    /// * `repo_path` - Path to repository
    /// * `file_path` - Path to file to unstage
    #[napi]
    pub async fn unstage_file(&self, repo_path: String, file_path: String) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;

        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            unstage_file_impl(&repo_path, &file_path)
        })
        .await
    }

    #[napi]
    pub async fn get_staged_files(&self, repo_path: String) -> Result<Vec<String>> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || get_staged_files_impl(&repo_path)).await
    }

    #[napi]
    pub async fn stage_deletion(&self, repo_path: String, file_path: String) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            stage_deletion_impl(&repo_path, &file_path)
        })
        .await
    }

    #[napi]
    pub async fn stage_rename(
        &self,
        repo_path: String,
        old_path: String,
        new_path: String,
    ) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&old_path)?;
        validate_file_path(&new_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            stage_rename_impl(&repo_path, &old_path, &new_path)
        })
        .await
    }

    #[napi]
    pub async fn commit_staged_changes(
        &self,
        repo_path: String,
        message: String,
        user_name: String,
        user_email: String,
        options: Option<CommitOptions>,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_committer(options.as_ref())?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            commit_staged_changes_impl(
                &repo_path,
                &message,
                &user_name,
                &user_email,
                options.as_ref(),
            )
        })
        .await
    }

    #[napi]
    #[allow(clippy::too_many_arguments)]
    pub async fn move_file(
        &self,
        repo_path: String,
        source_path: String,
        dest_path: String,
        message: String,
        user_name: String,
        user_email: String,
        options: Option<CommitOptions>,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&source_path)?;
        validate_file_path(&dest_path)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
        validate_committer(options.as_ref())?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            move_file_impl(
                &repo_path,
                &source_path,
                &dest_path,
                &message,
                &user_name,
                &user_email,
                options.as_ref(),
            )
        })
        .await
    }

    #[napi]
    #[allow(clippy::too_many_arguments)]
    pub async fn move_directory(
        &self,
        repo_path: String,
        source_path: String,
        dest_path: String,
        message: String,
        user_name: String,
        user_email: String,
        options: Option<CommitOptions>,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_directory_path(&source_path)?;
        validate_directory_path(&dest_path)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
        validate_committer(options.as_ref())?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            move_directory_impl(
                &repo_path,
                &source_path,
                &dest_path,
                &message,
                &user_name,
                &user_email,
                options.as_ref(),
            )
        })
        .await
    }

    // Repository initialization
    #[napi]
    pub async fn init_repository(&self, path: String) -> Result<bool> {
        validate_directory_for_init(&path)?;
        let structured = self.feature_flags().structured_errors;
        // Deliberately unlocked, and it cannot be otherwise: the lock file
        // lives inside .git, which does not exist yet, and creating it early
        // would make the directory non-empty — failing this operation's own
        // emptiness check. There is also nothing to protect. No repository
        // exists, so no index or HEAD can be raced, and two concurrent inits
        // resolve cleanly on their own: one wins and the other is rejected for
        // a non-empty directory.
        utils::run_blocking(structured, move || init_repository_impl(&path)).await
    }

    #[napi]
    pub async fn init_repository_with_config(
        &self,
        path: String,
        config: RepositoryConfig,
    ) -> Result<bool> {
        validate_directory_for_init(&path)?;
        validate_repository_config(&config)?;
        let structured = self.feature_flags().structured_errors;
        // Unlocked for the same reason as `init_repository`: no .git yet, so
        // nowhere to put the lock file and no repository state to protect.
        utils::run_blocking(structured, move || {
            init_repository_with_config_impl(&path, &config)
        })
        .await
    }

    /// Initialize a Git repository in a directory that already contains files.
    /// For duplicating an existing project: copy the content into place first,
    /// then initialise git over it.
    #[napi]
    pub async fn init_repository_in_existing_dir(
        &self,
        path: String,
        default_branch: Option<String>,
    ) -> Result<bool> {
        validate_repo_path(&path)?;
        if let Some(ref branch) = default_branch {
            validate_repository_config(&RepositoryConfig {
                description: None,
                default_branch: Some(branch.clone()),
                line_ending: None,
            })?;
        }
        let structured = self.feature_flags().structured_errors;
        // Unlocked for the same reason as `init_repository`: no .git yet, so
        // nowhere to put the lock file and no repository state to protect.
        utils::run_blocking(structured, move || {
            init_repository_in_existing_dir_impl(&path, default_branch.as_deref())
        })
        .await
    }

    /// Remove all remotes from a repository.
    /// Used when duplicating a repository with its history, so the copy cannot
    /// accidentally push to the original's remotes.
    #[napi]
    pub async fn remove_all_remotes(&self, repo_path: String) -> Result<Vec<String>> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            remove_all_remotes_impl(&repo_path)
        })
        .await
    }

    // Repository health and repair
    /// Take an explicit lock on a repository, held until `release`.
    ///
    /// Every mutating operation already takes this lock for its own duration,
    /// which makes each operation safe and cannot make a **sequence** safe. A
    /// host that writes files, runs an external validator over the working
    /// tree, then commits or restores, has a window between the write and the
    /// commit in which another process using this library can legally
    /// interleave: its edit can land mid-validation, be judged by the wrong
    /// validator run, or be undone by the first host's restore. Holding a lock
    /// across the whole sequence closes that window.
    ///
    /// ```js
    /// const lock = await git.acquireRepositoryLock(repo);
    /// try {
    ///   await writeFiles();
    ///   if (await validate()) {
    ///     await git.commitFiles(repo, paths, message, name, email);
    ///   }
    /// } finally {
    ///   await lock.release();
    /// }
    /// ```
    ///
    /// Operations issued while this process holds the lock proceed rather than
    /// deadlocking. They skip the advisory lock, which this scope already
    /// holds, and still take the in-process mutex, so they continue to exclude
    /// each other.
    ///
    /// A second `acquireRepositoryLock` on the same repository is refused with
    /// `REPOSITORY_LOCKED` rather than handed back as a reentrant handle,
    /// whether the holder is this process or another one. Nested scopes hide
    /// bugs about who owns what.
    ///
    /// `timeoutMs` defaults to the same ten seconds every other operation
    /// waits. As everywhere else, the advisory lock is released by the kernel
    /// if the process dies, so a crash cannot wedge a repository; a handle
    /// leaked inside a live process is the caller's bug, which is what the
    /// `finally` above is for.
    ///
    /// This excludes other users of **this library**. It does not exclude
    /// `git` run from a terminal.
    #[napi]
    pub async fn acquire_repository_lock(
        &self,
        repo_path: String,
        timeout_ms: Option<u32>,
    ) -> Result<RepositoryLock> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        let timeout = timeout_ms.map_or(utils::LOCK_TIMEOUT, |ms| {
            std::time::Duration::from_millis(u64::from(ms))
        });
        let scope = utils::run_blocking(structured, move || {
            utils::acquire_scope(&repo_path, timeout)
        })
        .await?;
        Ok(RepositoryLock {
            scope: Mutex::new(Some(scope)),
            structured_errors: structured,
        })
    }

    #[napi]
    pub async fn is_repository_healthy(&self, repo_path: String) -> Result<RepositoryHealth> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || is_repository_healthy_impl(&repo_path)).await
    }

    #[napi]
    pub async fn repair_repository(&self, repo_path: String) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            repair_repository_impl(&repo_path)
        })
        .await
    }

    // Repository configuration
    #[napi]
    pub async fn configure_repository(&self, repo_path: String, config: GitConfig) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_git_config(&config)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            configure_repository_impl(&repo_path, &config)
        })
        .await
    }

    /// Get a Git configuration value (repo-local only, no global fallback)
    #[napi]
    pub async fn get_config(&self, repo_path: String, key: String) -> Result<Option<String>> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || get_config_impl(&repo_path, &key, false)).await
    }

    /// Get a Git configuration value with global fallback
    #[napi]
    pub async fn get_config_with_fallback(
        &self,
        repo_path: String,
        key: String,
    ) -> Result<Option<String>> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || get_config_impl(&repo_path, &key, true)).await
    }

    /// Set a Git configuration value (repo-local only)
    #[napi]
    pub async fn set_config(&self, repo_path: String, key: String, value: String) -> Result<()> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            set_config_impl(&repo_path, &key, &value)
        })
        .await
    }

    /// Remove a Git configuration value (repo-local only)
    #[napi]
    pub async fn unset_config(&self, repo_path: String, key: String) -> Result<()> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            unset_config_impl(&repo_path, &key)
        })
        .await
    }

    // File management
    #[napi]
    pub async fn create_gitignore(&self, repo_path: String, patterns: Vec<String>) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_gitignore_patterns(&patterns)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            create_gitignore_impl(&repo_path, &patterns)
        })
        .await
    }

    #[napi]
    pub async fn create_gitattributes(
        &self,
        repo_path: String,
        rules: Vec<String>,
    ) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_gitattributes_rules(&rules)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            create_gitattributes_impl(&repo_path, &rules)
        })
        .await
    }

    // Repository information
    #[napi]
    pub async fn get_repository_info(&self, repo_path: String) -> Result<RepositoryInfo> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || get_repository_info_impl(&repo_path)).await
    }

    #[napi]
    pub async fn get_commit_history(
        &self,
        repo_path: String,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<CommitHistory> {
        validate_repo_path(&repo_path)?;
        let limit_usize = limit.map(|l| l as usize);
        let offset_usize = offset.map(|o| o as usize);
        validate_history_pagination(limit_usize, offset_usize)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            get_commit_history_impl(&repo_path, limit_usize, offset_usize)
        })
        .await
    }

    /// Get commit history for a specific file efficiently
    ///
    /// Uses tree entry OID comparison instead of full diffs for O(1) per-commit filtering.
    /// Much faster than scanning all commits and checking diffs.
    #[napi]
    pub async fn get_file_history(
        &self,
        repo_path: String,
        file_path: String,
        limit: Option<u32>,
    ) -> Result<CommitHistory> {
        validate_repo_path(&repo_path)?;
        validate_file_path_for_history(&file_path)?;
        let limit_usize = limit.map(|l| l as usize);
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            get_file_history_impl(&repo_path, &file_path, limit_usize)
        })
        .await
    }

    /// Resolve a symbolic ref to the commit hash it names, fully peeled.
    ///
    /// `refName` is `"HEAD"`, a branch name, a tag name, or a full ref path
    /// such as `refs/tags/v1.0.0`. An annotated tag peels through to the
    /// commit. A raw 40-character commit hash resolves to itself, so a caller
    /// can accept either form without branching on which it got.
    ///
    /// This is the intended way to obtain the hash that `getFileAtCommit` and
    /// `getTreeAtCommit` require: resolve once, then pass the result to every
    /// read in the snapshot, so all of them name the same object.
    ///
    /// Anything that does not name a commit is an error naming the ref, never
    /// a null. `REF_NOT_FOUND` for no such ref or a ref that peels to a
    /// non-commit such as a tag of a blob; `EMPTY_REPOSITORY` for a symbolic
    /// ref whose target does not exist yet, which is `"HEAD"` before the
    /// first commit and also a freshly orphaned branch in a repository that
    /// has plenty. Those are separate because a caller does different things
    /// with them: one has no commits *yet* and is answered by making one, the
    /// other names something that was never going to resolve. An abbreviated hash is not resolved.
    ///
    /// Not a revparse grammar: `HEAD~3` and `main@{yesterday}` are out of
    /// scope. One ref in, one hash out.
    ///
    /// Read-only. Takes no lock.
    #[napi]
    pub async fn resolve_ref(&self, repo_path: String, ref_name: String) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_ref_name(&ref_name)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || resolve_ref_impl(&repo_path, &ref_name)).await
    }

    /// File content at a commit.
    ///
    /// Takes a raw commit hash and nothing else. `resolveRef` is the intended
    /// way to obtain one from `"HEAD"`, a branch or a tag.
    #[napi]
    pub async fn get_file_at_commit(
        &self,
        repo_path: String,
        file_path: String,
        commit_hash: String,
    ) -> Result<FileAtCommit> {
        validate_repo_path(&repo_path)?;
        validate_file_path_for_history(&file_path)?;
        validate_commit_hash(&commit_hash)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            get_file_at_commit_impl(&repo_path, &file_path, &commit_hash)
        })
        .await
    }

    /// List the paths present at a commit.
    ///
    /// The paired half of `getFileAtCommit`. Calling both with the same
    /// commit hash gives a snapshot-consistent view of a whole repository
    /// with no lock held and no writer blocked: resolve a commit once, list
    /// its tree, then fetch each file at that same commit.
    ///
    /// Directories are not entries. They are implied by the paths of the
    /// files inside them, and paths come back sorted.
    ///
    /// Takes a raw commit hash and nothing else, exactly as `getFileAtCommit`
    /// does. Not a branch, not a tag, not `"HEAD"`. Resolve those first with
    /// `resolveRef`, which is what the snapshot pattern does anyway.
    ///
    /// `pathPrefix` is a literal string prefix on the repository-relative
    /// path by default, so `"effort"` also matches a file named
    /// `effortless.yaml`. Pass `{ directory: true }` to match only what is
    /// under the named directory; it normalises a missing trailing slash, so
    /// `"effort"` and `"effort/"` mean the same thing under it. A prefix
    /// matching nothing returns an empty array, not an error.
    #[napi]
    pub async fn get_tree_at_commit(
        &self,
        repo_path: String,
        commit_hash: String,
        path_prefix: Option<String>,
        options: Option<TreeFilterOptions>,
    ) -> Result<Vec<TreeEntry>> {
        validate_repo_path(&repo_path)?;
        validate_commit_hash(&commit_hash)?;
        let directory = options.and_then(|o| o.directory).unwrap_or(false);
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            get_tree_at_commit_impl(&repo_path, &commit_hash, path_prefix.as_deref(), directory)
        })
        .await
    }

    /// Restore one path to the version held in `commitHash`.
    ///
    /// Refuses with `UNSTAGED_CHANGES_WOULD_BE_LOST` when the working copy of
    /// **that path** differs from HEAD, or exists while HEAD has never heard
    /// of it. The check is per-path, not repository-wide: an unrelated dirty
    /// file does not block a restore.
    ///
    /// `force` overrides that refusal, which is the point of it. A write
    /// sequence that dies partway leaves a half-written file, and putting the
    /// bytes back is the recovery the default guard would otherwise refuse
    /// precisely because the write died. It overrides the guard and nothing
    /// else: a path absent from the commit is still `FILE_NOT_FOUND`, and
    /// nothing on disk moves. Defaults to `false`.
    ///
    /// For restoring to HEAD specifically, `discardChanges` is the better
    /// tool: it force-checks-out the one path through libgit2, so it also
    /// updates the index and preserves symlinks, executable bits and CRLF
    /// filters, where this operation writes the blob's bytes.
    #[napi]
    pub async fn restore_file_from_commit(
        &self,
        repo_path: String,
        file_path: String,
        commit_hash: String,
        force: Option<bool>,
    ) -> Result<bool> {
        validate_restore_operation(&repo_path, &file_path, &commit_hash)?;
        let force = force.unwrap_or(false);
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            restore_file_from_commit_impl(&repo_path, &file_path, &commit_hash, force)
        })
        .await
    }

    /// Discard uncommitted changes in a file (restore to HEAD state)
    ///
    /// This operation restores the working tree file to match the HEAD commit,
    /// discarding any uncommitted changes. Both the working tree and index are
    /// updated to match HEAD.
    #[napi]
    pub async fn discard_changes(&self, repo_path: String, file_path: String) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            discard_changes_impl(&repo_path, &file_path)
        })
        .await
    }

    /// Amend the last commit with new changes
    ///
    /// This operation amends the most recent commit with whatever is currently
    /// staged in the index, optionally updating the commit message.
    ///
    /// # Arguments
    /// * `repo_path` - Path to the git repository
    /// * `message` - New commit message (if empty, reuse previous message)
    /// * `user_name` - Optional user name (empty string = read from config)
    /// * `user_email` - Optional user email (empty string = read from config)
    /// * `options` - Optional committer, defaulting to the author
    #[napi]
    pub async fn commit_amend(
        &self,
        repo_path: String,
        message: String,
        user_name: String,
        user_email: String,
        options: Option<CommitOptions>,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_committer(options.as_ref())?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            // Convert empty strings to None for lenient validation.
            let name = if user_name.trim().is_empty() {
                None
            } else {
                Some(user_name.as_str())
            };
            let email = if user_email.trim().is_empty() {
                None
            } else {
                Some(user_email.as_str())
            };
            commit_amend_impl(&repo_path, &message, name, email, options.as_ref())
        })
        .await
    }

    // Deleted files recovery
    #[napi]
    pub async fn get_deleted_files(
        &self,
        repo_path: String,
        limit: Option<u32>,
    ) -> Result<Vec<DeletedFileEntry>> {
        validate_repo_path(&repo_path)?;
        let limit_usize = limit.map(|l| l as usize);
        validate_deleted_files_limit(limit_usize)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            get_deleted_files_impl(&repo_path, limit_usize)
        })
        .await
    }

    // Diff operations
    #[napi]
    pub async fn get_file_diff(&self, repo_path: String, file_path: String) -> Result<FileDiff> {
        validate_diff_parameters(&repo_path, Some(&file_path))?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            get_file_diff_impl(&repo_path, &file_path)
        })
        .await
    }

    #[napi]
    pub async fn get_commit_diff(
        &self,
        repo_path: String,
        commit_hash: String,
    ) -> Result<CommitDiff> {
        validate_repo_path(&repo_path)?;
        validate_commit_hash(&commit_hash)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            get_commit_diff_impl(&repo_path, &commit_hash)
        })
        .await
    }

    /// Get unified diff string for a file (working tree vs HEAD)
    ///
    /// Returns a unified diff string suitable for display in a diff viewer.
    /// Handles new files, binary files, and modified files.
    #[napi]
    pub async fn get_diff(&self, repo_path: String, file_path: String) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || get_diff_impl(&repo_path, &file_path)).await
    }

    /// List all branches in the repository
    #[napi]
    pub async fn list_branches(
        &self,
        repo_path: String,
        include_remote: Option<bool>,
    ) -> Result<Vec<BranchInfo>> {
        branch_ops::list_branches(self, repo_path, include_remote).await
    }

    /// Get information about the current branch
    #[napi]
    pub async fn get_current_branch(&self, repo_path: String) -> Result<Option<BranchInfo>> {
        branch_ops::get_current_branch(self, repo_path).await
    }

    /// Create a new branch
    #[napi]
    pub async fn create_branch(
        &self,
        repo_path: String,
        options: CreateBranchOptions,
    ) -> Result<BranchInfo> {
        branch_ops::create_branch(self, repo_path, options).await
    }

    /// Switch to a different branch
    #[napi]
    pub async fn checkout_branch(
        &self,
        repo_path: String,
        branch_name: String,
    ) -> Result<BranchInfo> {
        branch_ops::checkout_branch(self, repo_path, branch_name).await
    }

    /// Delete a branch (with safety checks)
    #[napi]
    pub async fn delete_branch(
        &self,
        repo_path: String,
        branch_name: String,
        force: Option<bool>,
    ) -> Result<bool> {
        branch_ops::delete_branch(self, repo_path, branch_name, force).await
    }

    /// What merging `branch` into HEAD would do — without doing it. Does not
    /// touch the working tree, the index, or any ref.
    #[napi]
    pub async fn merge_analysis(&self, repo_path: String, branch: String) -> Result<MergeAnalysis> {
        branch_ops::merge_analysis(self, repo_path, branch).await
    }

    /// Move the current branch forward to `branch` when that is a strict
    /// fast-forward. Refuses with `NOT_FAST_FORWARD` when HEAD is already
    /// up to date with `branch`, or when the two have diverged — either case
    /// needs a real merge, which this does not perform.
    #[napi]
    pub async fn fast_forward(
        &self,
        repo_path: String,
        branch: String,
    ) -> Result<FastForwardResult> {
        branch_ops::fast_forward(self, repo_path, branch).await
    }

    /// Merge `branch` into HEAD, entirely in memory.
    ///
    /// Never leaves an in-progress merge behind: a `"conflicted"` outcome
    /// reports the three sides of each contested path and writes nothing at
    /// all — no `MERGE_HEAD`, no conflict markers, no conflicted index — so a
    /// caller who abandons the resolution is left with the repository it
    /// started with. Refuses with `UNSTAGED_CHANGES_WOULD_BE_LOST` when a file
    /// the merge would change has unsaved edits on disk.
    ///
    /// Fast-forwards where it can, which writes no commit and so records
    /// nobody. `{ noFastForward: true }` requires a merge commit instead, so
    /// the merge has somewhere to say who performed it.
    #[napi]
    pub async fn merge(
        &self,
        repo_path: String,
        branch: String,
        user_name: Option<String>,
        user_email: Option<String>,
        options: Option<MergeOptions>,
    ) -> Result<MergeOutcome> {
        merge_ops::merge(self, repo_path, branch, user_name, user_email, options).await
    }

    /// Read a blob by oid as text — one side of a contested file.
    ///
    /// Fails with `BLOB_NOT_UTF8` rather than converting lossily, because
    /// silently replacing a byte with U+FFFD corrupts a manuscript in a way
    /// nobody notices until much later.
    #[napi]
    pub async fn read_blob(&self, repo_path: String, oid: String) -> Result<String> {
        merge_ops::read_blob(self, repo_path, oid).await
    }

    /// Commit a merge the user resolved by hand.
    ///
    /// Takes only the resolved pages: the merge is re-run in memory and the
    /// resolutions laid over it, so the caller never reproduces libgit2's
    /// merge of the pages that merged cleanly. Refuses with `HEAD_MOVED` when
    /// HEAD is no longer `expected_head_hash`, and with `INVALID_ARGUMENT`
    /// when the resolved set does not match the conflict set exactly.
    #[napi]
    #[allow(clippy::too_many_arguments)]
    pub async fn commit_merge(
        &self,
        repo_path: String,
        their_ref: String,
        expected_head_hash: String,
        resolved_files: Vec<ResolvedFile>,
        message: String,
        user_name: Option<String>,
        user_email: Option<String>,
        options: Option<CommitOptions>,
    ) -> Result<CommitInfo> {
        merge_ops::commit_merge(
            self,
            repo_path,
            their_ref,
            expected_head_hash,
            resolved_files,
            message,
            user_name,
            user_email,
            options,
        )
        .await
    }

    /// List all tags in the repository
    #[napi]
    pub async fn list_tags(&self, repo_path: String) -> Result<Vec<TagInfo>> {
        tag_ops::list_tags(self, repo_path).await
    }

    /// Create a new tag
    #[napi]
    pub async fn create_tag(
        &self,
        repo_path: String,
        options: CreateTagOptions,
    ) -> Result<TagInfo> {
        tag_ops::create_tag(self, repo_path, options).await
    }

    /// Delete a tag
    #[napi]
    pub async fn delete_tag(&self, repo_path: String, tag_name: String) -> Result<bool> {
        tag_ops::delete_tag(self, repo_path, tag_name).await
    }

    /// Get tag information by name
    #[napi]
    pub async fn get_tag(&self, repo_path: String, tag_name: String) -> Result<Option<TagInfo>> {
        tag_ops::get_tag(self, repo_path, tag_name).await
    }

    // ===== INTERNAL HELPER METHODS (for use by branch_ops and tag_ops modules) =====

    /// Get reference to feature flags
    pub(crate) fn feature_flags(&self) -> &FeatureFlags {
        &self.feature_flags
    }

    // ===== REMOTE OPERATIONS =====
    //
    // The only operations here that touch the network. Credentials are passed
    // per call rather than held by the service: this library has no business
    // owning secrets, and the host application knows where they came from and
    // how long they may live.

    /// List configured remotes.
    #[napi]
    pub async fn list_remotes(&self, repo_path: String) -> Result<Vec<RemoteInfo>> {
        validate_repo_path(&repo_path)?;
        remote_ops::list_remotes(self, repo_path).await
    }

    /// Add a remote.
    #[napi]
    pub async fn add_remote(
        &self,
        repo_path: String,
        name: String,
        url: String,
    ) -> Result<RemoteInfo> {
        validate_repo_path(&repo_path)?;
        remote_ops::add_remote(self, repo_path, name, url).await
    }

    /// Remove a remote and its remote-tracking branches.
    #[napi]
    pub async fn remove_remote(&self, repo_path: String, name: String) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        remote_ops::remove_remote(self, repo_path, name).await
    }

    /// Change a remote's fetch URL.
    #[napi]
    pub async fn set_remote_url(
        &self,
        repo_path: String,
        name: String,
        url: String,
    ) -> Result<RemoteInfo> {
        validate_repo_path(&repo_path)?;
        remote_ops::set_remote_url(self, repo_path, name, url).await
    }

    /// Fetch from a remote, updating remote-tracking branches.
    ///
    /// Touches neither the working tree nor any local branch, so nothing you
    /// have can be lost by it.
    #[napi]
    pub async fn fetch(
        &self,
        repo_path: String,
        remote_name: String,
        credentials: Option<RemoteCredentials>,
    ) -> Result<FetchResult> {
        validate_repo_path(&repo_path)?;
        remote_ops::fetch(self, repo_path, remote_name, credentials).await
    }

    /// Push a local branch to a remote.
    #[napi]
    pub async fn push(
        &self,
        repo_path: String,
        remote_name: String,
        branch: String,
        credentials: Option<RemoteCredentials>,
    ) -> Result<PushResult> {
        validate_repo_path(&repo_path)?;
        remote_ops::push(self, repo_path, remote_name, branch, credentials).await
    }

    /// Compare a local branch with its upstream.
    ///
    /// Reads only what the repository already knows, so the answer is as fresh
    /// as the last fetch. Call `fetch` first for current information.
    #[napi]
    pub async fn get_upstream_status(
        &self,
        repo_path: String,
        branch: String,
    ) -> Result<UpstreamStatus> {
        validate_repo_path(&repo_path)?;
        remote_ops::get_upstream_status(self, repo_path, branch).await
    }

    // Four more `pub(crate)` helpers followed, each one line forwarding to the
    // identically-named `utils::` function, each carrying `#[allow(dead_code)]`
    // and a comment reading "reserved for future use by operation modules".
    // The operation modules import `utils` and call those functions directly,
    // which is why the forwarders were never called and why the compiler had
    // to be silenced to keep them. A `#[allow(dead_code)]` on a delegate is
    // not a reservation; it is an unused method with the warning turned off.
}
