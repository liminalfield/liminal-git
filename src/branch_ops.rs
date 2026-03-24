// native/src/branch_ops.rs

use git2::{Branch, BranchType, Repository};
use crate::{BranchInfo, CreateBranchOptions, AheadBehind};
use crate::utils;
use crate::errors::GitError;
use log::info;

// NAPI imports only when feature is enabled
#[cfg(feature = "napi-binding")]
use napi::bindgen_prelude::*;
#[cfg(feature = "napi-binding")]
use crate::GitService;
#[cfg(feature = "napi-binding")]
use crate::utils::git_error_to_napi_with_flags;

// ===== PURE GIT IMPLEMENTATIONS (always available) =====

/// List all branches in the repository
pub fn list_branches_impl(
    repo_path: &str,
    include_remote: bool,
) -> std::result::Result<Vec<BranchInfo>, GitError> {
    info!("list_branches: include_remote={}", include_remote);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("list_branches"))?;
    let mut branches = Vec::new();

    // Get local branches
    let local_branches = repo.branches(Some(BranchType::Local))
        .map_err(|e| GitError::from(e).with_operation("list_local_branches"))?;

    for branch_result in local_branches {
        let (branch, branch_type) = branch_result
            .map_err(|e| GitError::from(e).with_operation("iterate_branches"))?;

        if let Some(branch_info) = extract_branch_info_impl(&repo, branch, branch_type)? {
            branches.push(branch_info);
        }
    }

    // Get remote branches if requested
    if include_remote {
        let remote_branches = repo.branches(Some(BranchType::Remote))
            .map_err(|e| GitError::from(e).with_operation("list_remote_branches"))?;

        for branch_result in remote_branches {
            let (branch, branch_type) = branch_result
                .map_err(|e| GitError::from(e).with_operation("iterate_remote_branches"))?;

            if let Some(branch_info) = extract_branch_info_impl(&repo, branch, branch_type)? {
                branches.push(branch_info);
            }
        }
    }

    // Sort branches: current first, then alphabetically
    branches.sort_by(|a, b| {
        if a.is_current {
            std::cmp::Ordering::Less
        } else if b.is_current {
            std::cmp::Ordering::Greater
        } else {
            a.name.cmp(&b.name)
        }
    });

    info!("list_branches: found {} branches in {}ms", branches.len(), start.elapsed().as_millis());
    Ok(branches)
}

/// Get information about the current branch
pub fn get_current_branch_impl(repo_path: &str) -> std::result::Result<Option<BranchInfo>, GitError> {
    info!("get_current_branch");
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_current_branch"))?;

    let head = repo.head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;

    if !head.is_branch() {
        info!("get_current_branch: detached HEAD in {}ms", start.elapsed().as_millis());
        return Ok(None); // Detached HEAD state
    }

    let branch = repo.find_branch(head.shorthand().unwrap_or(""), BranchType::Local)
        .map_err(|e| GitError::from(e).with_operation("find_current_branch"))?;

    let result = extract_branch_info_impl(&repo, branch, BranchType::Local)?;
    info!("get_current_branch: found {:?} in {}ms", result.as_ref().map(|b| &b.name), start.elapsed().as_millis());
    Ok(result)
}

/// Create a new branch
pub fn create_branch_impl(
    repo_path: &str,
    options: &CreateBranchOptions,
) -> std::result::Result<BranchInfo, GitError> {
    info!("create_branch: name={}", options.name);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("create_branch"))?;

    // Validate branch name
    if !utils::is_valid_branch_name(&options.name) {
        return Err(GitError::InvalidBranchName {
            name: options.name.clone(),
        });
    }

    // Check if branch already exists
    if repo.find_branch(&options.name, BranchType::Local).is_ok() {
        return Err(GitError::BranchAlreadyExists {
            name: options.name.clone(),
        });
    }

    // Determine the commit to branch from
    let target_commit = if let Some(ref commit_hash) = options.from_commit {
        let oid = git2::Oid::from_str(commit_hash)
            .map_err(|_| GitError::InvalidCommitHash { hash: commit_hash.clone() })?;
        repo.find_commit(oid)
            .map_err(|e| GitError::from(e).with_operation("find_commit"))?
    } else {
        // Use current HEAD
        let head = repo.head()
            .map_err(|e| GitError::from(e).with_operation("get_head"))?;
        head.peel_to_commit()
            .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?
    };

    // Create the branch
    let branch = repo.branch(&options.name, &target_commit, false)
        .map_err(|e| GitError::from(e).with_operation("create_branch"))?;

    // Checkout the new branch if requested
    if options.checkout {
        // Force=false is safe here - new branch points to current HEAD, no conflicts
        checkout_branch_internal_impl(&repo, &options.name, false)?;
    }

    // Return branch info
    let result = extract_branch_info_impl(&repo, branch, BranchType::Local)?
        .ok_or_else(|| GitError::BranchNotFound {
            name: options.name.clone(),
        })?;

    info!("create_branch: success in {}ms", start.elapsed().as_millis());
    Ok(result)
}

/// Switch to a different branch
/// Checkout a branch with configurable conflict handling
///
/// # Strategy
/// Reads `liminal.checkoutStrategy` config:
/// - "safe" (default): Only allows checkout if no conflicts. Returns UnstagedChangesWouldBeLost with
///   actual conflicting file list if local changes would be overwritten.
/// - "force": Overwrites local changes unconditionally (dangerous, use with caution)
///
/// If config is not set, defaults to "safe" behavior.
pub fn checkout_branch_impl(repo_path: &str, branch_name: &str) -> std::result::Result<BranchInfo, GitError> {
    info!("checkout_branch: name={}", branch_name);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("checkout_branch"))?;

    // Read checkout strategy from config (defaults to "safe")
    let strategy = crate::repository_ops::get_config_impl(
        repo_path,
        "liminal.checkoutStrategy",
        false, // Don't fallback to global - this is a repo-specific setting
    )?
    .unwrap_or_else(|| "safe".to_string());

    let force = match strategy.to_lowercase().as_str() {
        "force" => {
            info!("checkout_branch: using force strategy (from config)");
            true
        }
        "safe" | _ => {
            info!("checkout_branch: using safe strategy");
            false
        }
    };

    // Attempt checkout with configured strategy
    checkout_branch_internal_impl(&repo, branch_name, force)?;

    // Return updated branch info
    let branch = repo.find_branch(branch_name, BranchType::Local)
        .map_err(|_e| GitError::BranchNotFound { name: branch_name.to_string() })?;

    let result = extract_branch_info_impl(&repo, branch, BranchType::Local)?
        .ok_or_else(|| GitError::BranchNotFound { name: branch_name.to_string() })?;

    info!("checkout_branch: success in {}ms", start.elapsed().as_millis());
    Ok(result)
}

/// Delete a branch (with safety checks)
pub fn delete_branch_impl(
    repo_path: &str,
    branch_name: &str,
    force: bool,
) -> std::result::Result<bool, GitError> {
    info!("delete_branch: name={} force={}", branch_name, force);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("delete_branch"))?;

    // Cannot delete current branch
    let head = repo.head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;

    if let Some(current_branch) = head.shorthand() {
        if current_branch == branch_name {
            return Err(GitError::CannotDeleteCurrentBranch {
                name: branch_name.to_string(),
            });
        }
    }

    let mut branch = repo.find_branch(branch_name, BranchType::Local)
        .map_err(|_| GitError::BranchNotFound { name: branch_name.to_string() })?;

    // Check if branch is merged (unless force)
    if !force && !is_branch_merged_impl(&repo, &branch)? {
        return Err(GitError::BranchNotMerged {
            name: branch_name.to_string(),
            commits_ahead: 1, // Simplified - would need graph walking for exact count
        });
    }

    branch.delete()
        .map_err(|e| GitError::from(e).with_operation("delete_branch"))?;

    info!("delete_branch: success in {}ms", start.elapsed().as_millis());
    Ok(true)
}

// ===== HELPER FUNCTIONS =====

fn extract_branch_info_impl(
    repo: &Repository,
    branch: Branch,
    branch_type: BranchType,
) -> std::result::Result<Option<BranchInfo>, GitError> {
    let name = branch.name()
        .map_err(|e| GitError::from(e).with_operation("get_branch_name"))?
        .unwrap_or("unknown")
        .to_string();

    let is_current = branch.is_head();
    let is_remote = branch_type == BranchType::Remote;

    let commit = branch.get().peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

    let commit_hash = commit.id().to_string();
    let commit_message = commit.message().unwrap_or("").to_string();
    let last_updated = utils::format_timestamp(commit.time());

    // Calculate ahead/behind for local branches
    let ahead_behind = if !is_remote {
        calculate_ahead_behind_impl(repo, &branch).ok()
    } else {
        None
    };

    Ok(Some(BranchInfo {
        name,
        is_current,
        is_remote,
        commit_hash,
        commit_message,
        last_updated,
        ahead_behind,
    }))
}

fn checkout_branch_internal_impl(repo: &Repository, branch_name: &str, force: bool) -> std::result::Result<(), GitError> {
    let branch = repo.find_branch(branch_name, BranchType::Local)
        .map_err(|_| GitError::BranchNotFound { name: branch_name.to_string() })?;

    let branch_ref = branch.get();
    let target_tree = branch_ref.peel_to_tree()
        .map_err(|e| GitError::from(e).with_operation("peel_to_tree"))?;

    // In safe mode, attempt checkout to target tree first to detect actual conflicts
    // This lets git2 determine which files would actually conflict
    if !force {
        let mut checkout_builder = git2::build::CheckoutBuilder::new();
        checkout_builder.safe();

        // Try checkout to target tree (doesn't move HEAD yet)
        match repo.checkout_tree(target_tree.as_object(), Some(&mut checkout_builder)) {
            Ok(_) => {
                // Checkout succeeded - now update HEAD
                repo.set_head(branch_ref.name().unwrap())
                    .map_err(|e| GitError::from(e).with_operation("set_head"))?;

                // Refresh working tree to match new HEAD
                // The checkout_tree above was essentially a dry-run; now we need to
                // actually update the working directory to match the branch
                let mut final_builder = git2::build::CheckoutBuilder::new();
                final_builder.safe();
                repo.checkout_head(Some(&mut final_builder))
                    .map_err(|e| GitError::from(e).with_operation("checkout_head"))?;

                Ok(())
            }
            Err(e) => {
                // Checkout would fail - likely due to conflicts
                // git2 returns various error codes (Uncommitted, Modified, or general checkout failure)
                // Check if it's a conflict-related error by looking for common patterns
                let is_conflict_error = e.code() == git2::ErrorCode::Uncommitted
                    || e.code() == git2::ErrorCode::Modified
                    || e.message().contains("conflict");

                if is_conflict_error {
                    // Collect files that have uncommitted changes that would conflict
                    let conflicting_files = collect_actual_conflicts(repo, &target_tree)?;

                    if !conflicting_files.is_empty() {
                        info!("checkout_branch: safe mode blocked - {} actual conflicting files", conflicting_files.len());
                        Err(GitError::UnstagedChangesWouldBeLost {
                            files: conflicting_files,
                        })
                    } else {
                        // No conflicts found, but checkout still failed - pass through original error
                        Err(GitError::from(e).with_operation("checkout_tree"))
                    }
                } else {
                    Err(GitError::from(e).with_operation("checkout_tree"))
                }
            }
        }
    } else {
        // Force mode - overwrite local changes
        let mut checkout_builder = git2::build::CheckoutBuilder::new();
        checkout_builder.force();

        repo.checkout_tree(target_tree.as_object(), Some(&mut checkout_builder))
            .map_err(|e| GitError::from(e).with_operation("checkout_tree"))?;

        repo.set_head(branch_ref.name().unwrap())
            .map_err(|e| GitError::from(e).with_operation("set_head"))?;

        Ok(())
    }
}

/// Collect files that would actually conflict with the target tree
/// Only reports files where:
/// - The file has local modifications (staged or unstaged)
/// - AND the target tree has a different version of that file
fn collect_actual_conflicts(repo: &Repository, target_tree: &git2::Tree) -> std::result::Result<Vec<String>, GitError> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false); // Only care about tracked files
    opts.include_ignored(false);

    let statuses = repo.statuses(Some(&mut opts))
        .map_err(|e| GitError::from(e).with_operation("get_status"))?;

    let mut files = Vec::new();

    for entry in statuses.iter() {
        let status = entry.status();

        // Skip files that have no local changes
        if !status.intersects(
            git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::WT_MODIFIED
                | git2::Status::WT_DELETED
                | git2::Status::WT_RENAMED
        ) {
            continue;
        }

        if let Some(path) = entry.path() {
            // Check if this file exists in the target tree and is different
            match target_tree.get_path(std::path::Path::new(path)) {
                Ok(target_entry) => {
                    // File exists in target tree - check if it's different from working directory
                    // Get the current file content/blob
                    let head = repo.head().ok();
                    let head_tree = head.and_then(|h| h.peel_to_tree().ok());

                    // If the file content differs between HEAD and target, and we have local changes,
                    // this is a conflict
                    if let Some(head_t) = head_tree {
                        if let Ok(head_entry) = head_t.get_path(std::path::Path::new(path)) {
                            // File exists in both HEAD and target
                            if head_entry.id() != target_entry.id() {
                                // Target tree has different content than HEAD
                                // This file would be overwritten by checkout
                                files.push(path.to_string());
                            }
                            // else: target tree has same content as HEAD, no conflict even with local changes
                        } else {
                            // File doesn't exist in HEAD but exists in target
                            // Local changes to a file being created = conflict
                            files.push(path.to_string());
                        }
                    }
                }
                Err(_) => {
                    // File doesn't exist in target tree
                    // If we're deleting it, that's fine unless target also wants to modify it
                    // In this case, file doesn't exist in target, so no conflict
                }
            }
        }
    }

    Ok(files)
}

fn is_branch_merged_impl(repo: &Repository, branch: &Branch) -> std::result::Result<bool, GitError> {
    let branch_commit = branch.get().peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
    let head_commit = repo.head().and_then(|head| head.peel_to_commit())
        .map_err(|e| GitError::from(e).with_operation("get_head_commit"))?;

    // Check if branch commit is an ancestor of HEAD
    let is_ancestor = repo.graph_descendant_of(head_commit.id(), branch_commit.id())
        .map_err(|e| GitError::from(e).with_operation("graph_descendant_of"))?;

    Ok(is_ancestor)
}

fn calculate_ahead_behind_impl(repo: &Repository, branch: &Branch) -> std::result::Result<AheadBehind, GitError> {
    // This is a simplified version - in a real implementation you'd check against upstream
    // For now, we'll compare against main/master branch
    let default_branches = ["main", "master"];

    for default_branch in &default_branches {
        if let Ok(default_ref) = repo.find_branch(default_branch, BranchType::Local) {
            let branch_commit = branch.get().peel_to_commit()
                .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
            let default_commit = default_ref.get().peel_to_commit()
                .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

            let (ahead, behind) = repo.graph_ahead_behind(branch_commit.id(), default_commit.id())
                .map_err(|e| GitError::from(e).with_operation("graph_ahead_behind"))?;
            return Ok(AheadBehind {
                ahead: ahead as u32,
                behind: behind as u32,
            });
        }
    }

    // If no default branch found, return zeros
    Ok(AheadBehind { ahead: 0, behind: 0 })
}

// ===== NAPI WRAPPERS (only compiled with napi-binding feature) =====

#[cfg(feature = "napi-binding")]
pub async fn list_branches(
    service: &GitService,
    repo_path: String,
    include_remote: Option<bool>,
) -> Result<Vec<BranchInfo>> {
    let include_remote = include_remote.unwrap_or(false);
    let structured = service.feature_flags().structured_errors;
    list_branches_impl(&repo_path, include_remote)
        .map_err(|e| git_error_to_napi_with_flags(e, structured))
}

#[cfg(feature = "napi-binding")]
pub async fn get_current_branch(service: &GitService, repo_path: String) -> Result<Option<BranchInfo>> {
    let structured = service.feature_flags().structured_errors;
    get_current_branch_impl(&repo_path)
        .map_err(|e| git_error_to_napi_with_flags(e, structured))
}

#[cfg(feature = "napi-binding")]
pub async fn create_branch(
    service: &GitService,
    repo_path: String,
    options: CreateBranchOptions,
) -> Result<BranchInfo> {
    let structured = service.feature_flags().structured_errors;
    create_branch_impl(&repo_path, &options)
        .map_err(|e| git_error_to_napi_with_flags(e, structured))
}

#[cfg(feature = "napi-binding")]
pub async fn checkout_branch(service: &GitService, repo_path: String, branch_name: String) -> Result<BranchInfo> {
    let structured = service.feature_flags().structured_errors;
    checkout_branch_impl(&repo_path, &branch_name)
        .map_err(|e| git_error_to_napi_with_flags(e, structured))
}

#[cfg(feature = "napi-binding")]
pub async fn delete_branch(
    service: &GitService,
    repo_path: String,
    branch_name: String,
    force: Option<bool>,
) -> Result<bool> {
    let force = force.unwrap_or(false);
    let structured = service.feature_flags().structured_errors;
    delete_branch_impl(&repo_path, &branch_name, force)
        .map_err(|e| git_error_to_napi_with_flags(e, structured))
}

#[cfg(test)]
#[path = "branch_ops_tests.rs"]
mod tests;