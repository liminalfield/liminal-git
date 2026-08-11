// git_service.rs - NAPI bindings (only compiled with napi-binding feature)

use crate::branch_ops;
use crate::feature_flags::FeatureFlags;
use crate::file_ops::*;
use crate::history_ops::*;
use crate::repository_ops::*;
use crate::tag_ops;
use crate::types::GitStatus;
use crate::types::{BranchInfo, CreateBranchOptions, CreateTagOptions, TagInfo};
use crate::types::{CommitDiff, CommitHistory, DeletedFileEntry, FileAtCommit, FileDiff};
use crate::types::{GitConfig, RepositoryConfig, RepositoryHealth, RepositoryInfo};
use crate::utils;
use crate::validation::*;
use log::info;
use napi::Result;
use napi_derive::napi;

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

    #[napi]
    pub async fn commit_file(
        &self,
        repo_path: String,
        file_path: String,
        message: String,
        user_name: String,
        user_email: String,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            commit_file_impl(&repo_path, &file_path, &message, &user_name, &user_email)
        })
        .await
    }

    #[napi]
    pub async fn commit_files(
        &self,
        repo_path: String,
        file_paths: Vec<String>,
        message: String,
        user_name: String,
        user_email: String,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_file_paths(&file_paths)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            commit_files_impl(&repo_path, &file_paths, &message, &user_name, &user_email)
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
    /// * `force` - Reserved for future use (currently ignored, unstaging is inherently safe)
    #[napi]
    pub async fn unstage_file(
        &self,
        repo_path: String,
        file_path: String,
        force: Option<bool>,
    ) -> Result<bool> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&file_path)?;

        let force_flag = force.unwrap_or(false);
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            unstage_file_impl(&repo_path, &file_path, force_flag)
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
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            commit_staged_changes_impl(&repo_path, &message, &user_name, &user_email)
        })
        .await
    }

    #[napi]
    pub async fn move_file(
        &self,
        repo_path: String,
        source_path: String,
        dest_path: String,
        message: String,
        user_name: String,
        user_email: String,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_file_path(&source_path)?;
        validate_file_path(&dest_path)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
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
            )
        })
        .await
    }

    #[napi]
    pub async fn move_directory(
        &self,
        repo_path: String,
        source_path: String,
        dest_path: String,
        message: String,
        user_name: String,
        user_email: String,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
        validate_directory_path(&source_path)?;
        validate_directory_path(&dest_path)?;
        validate_commit_message(&message)?;
        validate_user_info(&user_name, &user_email)?;
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
    pub async fn init_repository_in_existing_dir(&self, path: String) -> Result<bool> {
        validate_repo_path(&path)?;
        let structured = self.feature_flags().structured_errors;
        // Unlocked for the same reason as `init_repository`: no .git yet, so
        // nowhere to put the lock file and no repository state to protect.
        utils::run_blocking(structured, move || {
            init_repository_in_existing_dir_impl(&path)
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

    // File content at commit
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

    // File restoration
    #[napi]
    pub async fn restore_file_from_commit(
        &self,
        repo_path: String,
        file_path: String,
        commit_hash: String,
    ) -> Result<bool> {
        validate_restore_operation(&repo_path, &file_path, &commit_hash)?;
        let structured = self.feature_flags().structured_errors;
        utils::run_blocking(structured, move || {
            let _guard = utils::lock_repo(&repo_path)?;
            restore_file_from_commit_impl(&repo_path, &file_path, &commit_hash)
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
    #[napi]
    pub async fn commit_amend(
        &self,
        repo_path: String,
        message: String,
        user_name: String,
        user_email: String,
    ) -> Result<String> {
        validate_repo_path(&repo_path)?;
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
            commit_amend_impl(&repo_path, &message, name, email)
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

    // Four more `pub(crate)` helpers followed, each one line forwarding to the
    // identically-named `utils::` function, each carrying `#[allow(dead_code)]`
    // and a comment reading "reserved for future use by operation modules".
    // The operation modules import `utils` and call those functions directly,
    // which is why the forwarders were never called and why the compiler had
    // to be silenced to keep them. A `#[allow(dead_code)]` on a delegate is
    // not a reservation; it is an unused method with the warning turned off.
}
