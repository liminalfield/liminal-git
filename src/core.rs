// core.rs - GitServiceCore for testing without NAPI dependencies

use anyhow::Result;
use crate::types::*;
use crate::{repository_ops, file_ops, history_ops};

/// Core git service implementation without NAPI bindings
/// This struct provides the same functionality as GitService but with standard Rust error types
/// for testing without Node.js dependencies
pub struct GitServiceCore;

impl GitServiceCore {
    pub fn new() -> Self {
        GitServiceCore
    }

    // ===== REPOSITORY OPERATIONS =====

    pub fn init_repository(&self, repo_path: String) -> Result<bool> {
        repository_ops::init_repository_impl(&repo_path)
            .map_err(|e| anyhow::anyhow!("Init repository failed: {}", e))
    }

    pub fn get_status(&self, repo_path: String) -> Result<GitStatus> {
        repository_ops::get_status_impl(&repo_path)
            .map_err(|e| anyhow::anyhow!("Get status failed: {}", e))
    }

    pub fn is_repository(&self, repo_path: String) -> bool {
        repository_ops::is_repository_impl(&repo_path)
    }

    pub fn get_repository_info(&self, repo_path: String) -> Result<RepositoryInfo> {
        repository_ops::get_repository_info_impl(&repo_path)
            .map_err(|e| anyhow::anyhow!("Get repository info failed: {}", e))
    }

    // Note: These functions don't exist in repository_ops yet
    // pub fn get_repository_config(&self, repo_path: String) -> Result<RepositoryConfig>
    // pub fn update_repository_config(&self, repo_path: String, config: RepositoryConfig) -> Result<()>

    pub fn check_repository_health(&self, repo_path: String) -> Result<RepositoryHealth> {
        repository_ops::is_repository_healthy_impl(&repo_path)
            .map_err(|e| anyhow::anyhow!("Check repository health failed: {}", e))
    }

    // ===== FILE OPERATIONS =====

    pub fn commit_file(&self, repo_path: String, file_path: String, message: String, user_name: String, user_email: String) -> Result<String> {
        file_ops::commit_file_impl(&repo_path, &file_path, &message, &user_name, &user_email)
            .map_err(|e| anyhow::anyhow!("Commit file failed: {}", e))
    }

    pub fn commit_files(&self, repo_path: String, file_paths: Vec<String>, message: String, user_name: String, user_email: String) -> Result<String> {
        file_ops::commit_files_impl(&repo_path, &file_paths, &message, &user_name, &user_email)
            .map_err(|e| anyhow::anyhow!("Commit files failed: {}", e))
    }

    pub fn stage_file(&self, repo_path: String, file_path: String) -> Result<bool> {
        file_ops::stage_file_impl(&repo_path, &file_path)
            .map_err(|e| anyhow::anyhow!("Stage file failed: {}", e))
    }

    pub fn unstage_file(&self, repo_path: String, file_path: String) -> Result<bool> {
        file_ops::unstage_file_impl(&repo_path, &file_path, false)
            .map_err(|e| anyhow::anyhow!("Unstage file failed: {}", e))
    }

    pub fn stage_deletion(&self, repo_path: String, file_path: String) -> Result<bool> {
        file_ops::stage_deletion_impl(&repo_path, &file_path)
            .map_err(|e| anyhow::anyhow!("Stage deletion failed: {}", e))
    }

    pub fn stage_rename(&self, repo_path: String, old_path: String, new_path: String) -> Result<bool> {
        file_ops::stage_rename_impl(&repo_path, &old_path, &new_path)
            .map_err(|e| anyhow::anyhow!("Stage rename failed: {}", e))
    }

    pub fn commit_staged_changes(&self, repo_path: String, message: String, user_name: String, user_email: String) -> Result<String> {
        file_ops::commit_staged_changes_impl(&repo_path, &message, &user_name, &user_email)
            .map_err(|e| anyhow::anyhow!("Commit staged changes failed: {}", e))
    }

    // Note: discard_changes and get_file_diff not implemented yet
    // pub fn discard_changes(&self, repo_path: String, file_path: String) -> Result<()>
    // pub fn get_file_diff(&self, repo_path: String, file_path: String) -> Result<FileDiff>

    // ===== HISTORY OPERATIONS =====

    pub fn get_commit_history(&self, repo_path: String, limit: Option<u32>, offset: Option<u32>) -> Result<CommitHistory> {
        let limit_usize = limit.map(|l| l as usize);
        let offset_usize = offset.map(|o| o as usize);
        history_ops::get_commit_history_impl(&repo_path, limit_usize, offset_usize)
            .map_err(|e| anyhow::anyhow!("Get commit history failed: {}", e))
    }

    pub fn get_file_at_commit(&self, repo_path: String, file_path: String, commit_hash: String) -> Result<FileAtCommit> {
        history_ops::get_file_at_commit_impl(&repo_path, &file_path, &commit_hash)
            .map_err(|e| anyhow::anyhow!("Get file at commit failed: {}", e))
    }

    pub fn restore_file_from_commit(&self, repo_path: String, file_path: String, commit_hash: String) -> Result<bool> {
        file_ops::restore_file_from_commit_impl(&repo_path, &file_path, &commit_hash)
            .map_err(|e| anyhow::anyhow!("Restore file from commit failed: {}", e))
    }

    pub fn get_deleted_files(&self, repo_path: String, limit: Option<u32>) -> Result<Vec<DeletedFileEntry>> {
        let limit_usize = limit.map(|l| l as usize);
        history_ops::get_deleted_files_impl(&repo_path, limit_usize)
            .map_err(|e| anyhow::anyhow!("Get deleted files failed: {}", e))
    }

    pub fn get_commit_diff(&self, repo_path: String, commit_hash: String) -> Result<CommitDiff> {
        history_ops::get_commit_diff_impl(&repo_path, &commit_hash)
            .map_err(|e| anyhow::anyhow!("Get commit diff failed: {}", e))
    }

    // ===== UTILITY METHODS =====

    pub fn format_timestamp(&self, time: git2::Time) -> String {
        crate::utils::format_timestamp(time)
    }

    pub fn is_valid_branch_name(&self, name: &str) -> bool {
        crate::utils::is_valid_branch_name(name)
    }

    pub fn is_valid_tag_name(&self, name: &str) -> bool {
        crate::utils::is_valid_tag_name(name)
    }

    pub fn has_uncommitted_changes(&self, repo: &git2::Repository) -> Result<bool> {
        crate::utils::has_uncommitted_changes_anyhow(repo)
            .map_err(|e| anyhow::anyhow!("Check uncommitted changes failed: {}", e))
    }
}

impl Default for GitServiceCore {
    fn default() -> Self {
        Self::new()
    }
}