// native/src/branch_ops.rs

use crate::errors::GitError;
use crate::utils;
use crate::{AheadBehind, BranchInfo, CreateBranchOptions, FastForwardResult, MergeAnalysis};
use git2::{Branch, BranchType, Oid, Reference, Repository};
use log::info;

// NAPI imports only when feature is enabled
#[cfg(feature = "napi-binding")]
use crate::GitService;
#[cfg(feature = "napi-binding")]
use napi::bindgen_prelude::*;

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
    let local_branches = repo
        .branches(Some(BranchType::Local))
        .map_err(|e| GitError::from(e).with_operation("list_local_branches"))?;

    for branch_result in local_branches {
        let (branch, branch_type) =
            branch_result.map_err(|e| GitError::from(e).with_operation("iterate_branches"))?;

        if let Some(branch_info) = extract_branch_info_impl(&repo, branch, branch_type)? {
            branches.push(branch_info);
        }
    }

    // Get remote branches if requested
    if include_remote {
        let remote_branches = repo
            .branches(Some(BranchType::Remote))
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

    info!(
        "list_branches: found {} branches in {}ms",
        branches.len(),
        start.elapsed().as_millis()
    );
    Ok(branches)
}

/// Get information about the current branch
pub fn get_current_branch_impl(
    repo_path: &str,
) -> std::result::Result<Option<BranchInfo>, GitError> {
    info!("get_current_branch");
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_current_branch"))?;

    let head = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;

    if !head.is_branch() {
        info!(
            "get_current_branch: detached HEAD in {}ms",
            start.elapsed().as_millis()
        );
        return Ok(None); // Detached HEAD state
    }

    let branch = repo
        .find_branch(head.shorthand().unwrap_or(""), BranchType::Local)
        .map_err(|e| GitError::from(e).with_operation("find_current_branch"))?;

    let result = extract_branch_info_impl(&repo, branch, BranchType::Local)?;
    info!(
        "get_current_branch: found {:?} in {}ms",
        result.as_ref().map(|b| &b.name),
        start.elapsed().as_millis()
    );
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
        let oid = git2::Oid::from_str(commit_hash).map_err(|_| GitError::InvalidCommitHash {
            hash: commit_hash.clone(),
        })?;
        repo.find_commit(oid)
            .map_err(|e| GitError::from(e).with_operation("find_commit"))?
    } else {
        // Use current HEAD
        let head = repo
            .head()
            .map_err(|e| GitError::from(e).with_operation("get_head"))?;
        head.peel_to_commit()
            .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?
    };

    // Create the branch
    let branch = repo
        .branch(&options.name, &target_commit, false)
        .map_err(|e| GitError::from(e).with_operation("create_branch"))?;

    // Checkout the new branch if requested
    if options.checkout {
        // Force=false is safe here - new branch points to current HEAD, no conflicts
        checkout_branch_internal_impl(&repo, &options.name, false)?;
    }

    // Return branch info
    let result = extract_branch_info_impl(&repo, branch, BranchType::Local)?.ok_or_else(|| {
        GitError::BranchNotFound {
            name: options.name.clone(),
        }
    })?;

    info!(
        "create_branch: success in {}ms",
        start.elapsed().as_millis()
    );
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
pub fn checkout_branch_impl(
    repo_path: &str,
    branch_name: &str,
) -> std::result::Result<BranchInfo, GitError> {
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
        // "safe" is the documented default; anything unrecognised gets it
        // too, since refusing a destructive checkout is the safer failure.
        _ => {
            info!("checkout_branch: using safe strategy");
            false
        }
    };

    // Attempt checkout with configured strategy
    checkout_branch_internal_impl(&repo, branch_name, force)?;

    // Return updated branch info
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|_e| GitError::BranchNotFound {
            name: branch_name.to_string(),
        })?;

    let result = extract_branch_info_impl(&repo, branch, BranchType::Local)?.ok_or_else(|| {
        GitError::BranchNotFound {
            name: branch_name.to_string(),
        }
    })?;

    info!(
        "checkout_branch: success in {}ms",
        start.elapsed().as_millis()
    );
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
    let head = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;

    if let Ok(current_branch) = head.shorthand()
        && current_branch == branch_name
    {
        return Err(GitError::CannotDeleteCurrentBranch {
            name: branch_name.to_string(),
        });
    }

    let mut branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|_| GitError::BranchNotFound {
            name: branch_name.to_string(),
        })?;

    // Check if branch is merged (unless force)
    if !force && !is_branch_merged_impl(&repo, &branch)? {
        // Real count of commits on the branch not reachable from HEAD, so the
        // error tells the user exactly how much work would be lost.
        let commits_ahead = commits_ahead_of_head_impl(&repo, &branch).unwrap_or(1);
        return Err(GitError::BranchNotMerged {
            name: branch_name.to_string(),
            commits_ahead,
        });
    }

    branch
        .delete()
        .map_err(|e| GitError::from(e).with_operation("delete_branch"))?;

    info!(
        "delete_branch: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

// ===== HELPER FUNCTIONS =====

fn extract_branch_info_impl(
    repo: &Repository,
    branch: Branch,
    branch_type: BranchType,
) -> std::result::Result<Option<BranchInfo>, GitError> {
    let name = branch
        .name()
        .map_err(|e| GitError::from(e).with_operation("get_branch_name"))?
        .unwrap_or("unknown")
        .to_string();

    let is_current = branch.is_head();
    let is_remote = branch_type == BranchType::Remote;

    let commit = branch
        .get()
        .peel_to_commit()
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

fn checkout_branch_internal_impl(
    repo: &Repository,
    branch_name: &str,
    force: bool,
) -> std::result::Result<(), GitError> {
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|_| GitError::BranchNotFound {
            name: branch_name.to_string(),
        })?;

    let branch_ref = branch.get();
    let target_tree = branch_ref
        .peel_to_tree()
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
                repo.set_head(branch_ref.name().ok().ok_or_else(|| {
                    GitError::InvalidBranchName {
                        name: "<non-UTF-8 branch ref>".to_string(),
                    }
                })?)
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
            // Safe mode's own error says only that something conflicted, so
            // the answer is re-derived from the state on disk.
            Err(e) => Err(checkout_refusal(repo, &target_tree, "checkout_branch", e)),
        }
    } else {
        // Force mode - overwrite local changes
        let mut checkout_builder = git2::build::CheckoutBuilder::new();
        checkout_builder.force();

        repo.checkout_tree(target_tree.as_object(), Some(&mut checkout_builder))
            .map_err(|e| GitError::from(e).with_operation("checkout_tree"))?;

        repo.set_head(
            branch_ref
                .name()
                .ok()
                .ok_or_else(|| GitError::InvalidBranchName {
                    name: "<non-UTF-8 branch ref>".to_string(),
                })?,
        )
        .map_err(|e| GitError::from(e).with_operation("set_head"))?;

        Ok(())
    }
}

/// The error a refused safe checkout should report, derived from the state on
/// disk rather than from libgit2's message.
///
/// libgit2 says only that something conflicted — never which files, and never
/// whether the caller has edits to save or a file to move. Both have to be
/// re-derived, and they are reported as different errors because they call for
/// different remedies. Tracked first: a path that is both committed and dirty
/// is the case the caller is most likely to be in, and the one this library
/// has always named.
///
/// Anything libgit2 refused for some other reason passes through untouched.
pub(crate) fn checkout_refusal(
    repo: &Repository,
    target_tree: &git2::Tree<'_>,
    operation: &str,
    original: git2::Error,
) -> GitError {
    let is_conflict_error = original.code() == git2::ErrorCode::Uncommitted
        || original.code() == git2::ErrorCode::Modified
        || original.message().contains("conflict");

    if !is_conflict_error {
        return GitError::from(original).with_operation("checkout_tree");
    }

    match collect_actual_conflicts(repo, target_tree) {
        Ok(files) if !files.is_empty() => {
            info!(
                "{}: safe mode blocked - {} actual conflicting files",
                operation,
                files.len()
            );
            return GitError::UnstagedChangesWouldBeLost { files };
        }
        Ok(_) => {}
        Err(e) => return e,
    }

    match collect_untracked_collisions(repo, target_tree) {
        Ok(files) if !files.is_empty() => {
            info!(
                "{}: safe mode blocked - {} untracked file(s) in the way",
                operation,
                files.len()
            );
            GitError::UntrackedFilesWouldBeOverwritten { files }
        }
        Ok(_) => GitError::from(original).with_operation("checkout_tree"),
        Err(e) => e,
    }
}

/// Untracked files sitting exactly where the target tree has content.
///
/// `collect_actual_conflicts` cannot see these, and correctly so: it asks
/// which *tracked* paths have changes the checkout would discard, and a file
/// with no committed history has none. It stands to lose itself instead, which
/// libgit2 refuses the checkout over just the same.
///
/// `recurse_untracked_dirs` because otherwise a wholly new directory is
/// reported as one entry named `dir/`, which names no path in the target tree
/// and would make the collision invisible again.
pub(crate) fn collect_untracked_collisions(
    repo: &Repository,
    target_tree: &git2::Tree<'_>,
) -> std::result::Result<Vec<String>, GitError> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    opts.recurse_untracked_dirs(true);
    opts.include_ignored(false);

    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| GitError::from(e).with_operation("get_status"))?;

    let mut files = Vec::new();
    for entry in statuses.iter() {
        if !entry.status().contains(git2::Status::WT_NEW) {
            continue;
        }
        if let Some(path) = entry.path().ok()
            && target_tree.get_path(std::path::Path::new(path)).is_ok()
        {
            files.push(path.to_string());
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

/// Collect files that would actually conflict with the target tree
/// Only reports files where:
/// - The file has local modifications (staged or unstaged)
/// - AND the target tree has a different version of that file
pub(crate) fn collect_actual_conflicts(
    repo: &Repository,
    target_tree: &git2::Tree,
) -> std::result::Result<Vec<String>, GitError> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false); // Only care about tracked files
    opts.include_ignored(false);

    let statuses = repo
        .statuses(Some(&mut opts))
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
                | git2::Status::WT_RENAMED,
        ) {
            continue;
        }

        if let Ok(path) = entry.path() {
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

fn is_branch_merged_impl(
    repo: &Repository,
    branch: &Branch,
) -> std::result::Result<bool, GitError> {
    let branch_commit = branch
        .get()
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
    let head_commit = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(|e| GitError::from(e).with_operation("get_head_commit"))?;

    // Check if branch commit is an ancestor of HEAD
    let is_ancestor = repo
        .graph_descendant_of(head_commit.id(), branch_commit.id())
        .map_err(|e| GitError::from(e).with_operation("graph_descendant_of"))?;

    Ok(is_ancestor)
}

/// Number of commits on `branch` that are not reachable from HEAD.
fn commits_ahead_of_head_impl(
    repo: &Repository,
    branch: &Branch,
) -> std::result::Result<u32, GitError> {
    let branch_commit = branch
        .get()
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
    let head_commit = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(|e| GitError::from(e).with_operation("get_head_commit"))?;

    let (ahead, _behind) = repo
        .graph_ahead_behind(branch_commit.id(), head_commit.id())
        .map_err(|e| GitError::from(e).with_operation("graph_ahead_behind"))?;
    Ok(ahead as u32)
}

fn calculate_ahead_behind_impl(
    repo: &Repository,
    branch: &Branch,
) -> std::result::Result<AheadBehind, GitError> {
    let branch_commit = branch
        .get()
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

    // Prefer the branch's configured upstream (tracking) branch — that's the
    // ahead/behind a user actually cares about when one is set.
    if let Ok(upstream) = branch.upstream()
        && let Ok(upstream_commit) = upstream.get().peel_to_commit()
    {
        let (ahead, behind) = repo
            .graph_ahead_behind(branch_commit.id(), upstream_commit.id())
            .map_err(|e| GitError::from(e).with_operation("graph_ahead_behind"))?;
        return Ok(AheadBehind {
            ahead: ahead as u32,
            behind: behind as u32,
        });
    }

    // Fall back to the local default branch (main/master), skipping self so a
    // main branch isn't compared against itself.
    let branch_name = branch.name().ok().flatten();
    for default_branch in ["main", "master"] {
        if branch_name == Some(default_branch) {
            continue;
        }
        if let Ok(default_ref) = repo.find_branch(default_branch, BranchType::Local) {
            let default_commit = default_ref
                .get()
                .peel_to_commit()
                .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

            let (ahead, behind) = repo
                .graph_ahead_behind(branch_commit.id(), default_commit.id())
                .map_err(|e| GitError::from(e).with_operation("graph_ahead_behind"))?;
            return Ok(AheadBehind {
                ahead: ahead as u32,
                behind: behind as u32,
            });
        }
    }

    // No upstream and no default branch to compare against.
    Ok(AheadBehind {
        ahead: 0,
        behind: 0,
    })
}

/// Resolve `branch` — a local branch name, a remote-tracking name like
/// `"origin/main"`, or any other short ref name libgit2 recognises — to its
/// reference. Anything that doesn't resolve is reported the same way
/// `checkout_branch_impl` reports a missing branch.
pub(crate) fn resolve_branch_ref<'repo>(
    repo: &'repo Repository,
    branch: &str,
) -> std::result::Result<Reference<'repo>, GitError> {
    repo.resolve_reference_from_short_name(branch)
        .map_err(|_| GitError::BranchNotFound {
            name: branch.to_string(),
        })
}

/// `(ahead, behind)` of HEAD against `their_id`: commits HEAD has that
/// `their_id` does not, and commits `their_id` has that HEAD does not.
///
/// Handles the unborn-HEAD case — a freshly initialised repository with no
/// commits yet, where `repo.head()` itself fails — by reporting nothing
/// ahead and everything reachable from `their_id` as behind, since there is
/// no common history to subtract.
fn ahead_behind_of_head_impl(
    repo: &Repository,
    their_id: Oid,
) -> std::result::Result<(u32, u32), GitError> {
    match repo.head() {
        Ok(head) => {
            let head_commit = head
                .peel_to_commit()
                .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
            let (ahead, behind) = repo
                .graph_ahead_behind(head_commit.id(), their_id)
                .map_err(|e| GitError::from(e).with_operation("graph_ahead_behind"))?;
            Ok((ahead as u32, behind as u32))
        }
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => {
            let mut revwalk = repo
                .revwalk()
                .map_err(|e| GitError::from(e).with_operation("revwalk"))?;
            revwalk
                .push(their_id)
                .map_err(|e| GitError::from(e).with_operation("revwalk_push"))?;
            let behind = revwalk.count() as u32;
            Ok((0, behind))
        }
        Err(e) => Err(GitError::from(e).with_operation("get_head")),
    }
}

/// What merging `branch` into HEAD would do, without doing it. Reads only —
/// never touches the working tree, the index, or any ref.
///
/// Uses libgit2's own `merge_analysis` rather than hand-rolling ancestry
/// checks, so the three kinds match exactly what a real merge would decide.
pub fn merge_analysis_impl(
    repo_path: &str,
    branch: &str,
) -> std::result::Result<MergeAnalysis, GitError> {
    info!("merge_analysis: branch={}", branch);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("merge_analysis"))?;

    let their_ref = resolve_branch_ref(&repo, branch)?;
    let their_commit = their_ref
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
    let their_annotated = repo
        .reference_to_annotated_commit(&their_ref)
        .map_err(|e| GitError::from(e).with_operation("reference_to_annotated_commit"))?;

    let (analysis, _preference) = repo
        .merge_analysis(&[&their_annotated])
        .map_err(|e| GitError::from(e).with_operation("merge_analysis"))?;

    // Order matters: an unborn HEAD sets both FASTFORWARD and UNBORN, so
    // checking fast-forward before falling through to "normal" is what makes
    // that case read as "fast-forward" (move the ref) rather than "normal"
    // (which would wrongly imply a merge is needed).
    let kind = if analysis.is_up_to_date() {
        "up-to-date"
    } else if analysis.is_fast_forward() {
        "fast-forward"
    } else {
        "normal"
    }
    .to_string();

    let (ahead, behind) = ahead_behind_of_head_impl(&repo, their_commit.id())?;

    info!(
        "merge_analysis: kind={} ahead={} behind={} in {}ms",
        kind,
        ahead,
        behind,
        start.elapsed().as_millis()
    );

    Ok(MergeAnalysis {
        kind,
        ahead,
        behind,
    })
}

/// Move the current branch forward to `branch` when that is a strict
/// fast-forward. Refuses — with `NotFastForward` — when HEAD already has
/// everything `branch` has ("up-to-date"), or when the two have diverged
/// ("diverged"); both would require a real merge, which this does not do.
///
/// Updates the branch ref, the working tree and the index together, using
/// the same `liminal.checkoutStrategy` policy `checkoutBranch` honours: safe
/// by default (refuses if a local change would be overwritten, naming the
/// files via `UnstagedChangesWouldBeLost`), or force when configured.
pub fn fast_forward_impl(
    repo_path: &str,
    branch: &str,
) -> std::result::Result<FastForwardResult, GitError> {
    info!("fast_forward: branch={}", branch);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("fast_forward"))?;

    let mut head = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;

    if !head.is_branch() {
        return Err(GitError::DetachedHead);
    }

    let head_branch_name = head
        .shorthand()
        .ok()
        .ok_or_else(|| GitError::InvalidBranchName {
            name: "<non-UTF-8 branch ref>".to_string(),
        })?
        .to_string();
    let previous_commit = head
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

    let their_ref = resolve_branch_ref(&repo, branch)?;
    let their_commit = their_ref
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
    let their_annotated = repo
        .reference_to_annotated_commit(&their_ref)
        .map_err(|e| GitError::from(e).with_operation("reference_to_annotated_commit"))?;

    let (analysis, _preference) = repo
        .merge_analysis(&[&their_annotated])
        .map_err(|e| GitError::from(e).with_operation("merge_analysis"))?;

    if analysis.is_up_to_date() {
        return Err(GitError::NotFastForward {
            branch: branch.to_string(),
            reason: "up-to-date".to_string(),
        });
    }
    if !analysis.is_fast_forward() {
        return Err(GitError::NotFastForward {
            branch: branch.to_string(),
            reason: "diverged".to_string(),
        });
    }

    // Same policy as checkout_branch_impl: read liminal.checkoutStrategy,
    // defaulting to "safe" — refusing a destructive checkout is the safer
    // failure for anything unrecognised too.
    let strategy =
        crate::repository_ops::get_config_impl(repo_path, "liminal.checkoutStrategy", false)?
            .unwrap_or_else(|| "safe".to_string());
    let force = strategy.to_lowercase() == "force";

    let target_tree = their_ref
        .peel_to_tree()
        .map_err(|e| GitError::from(e).with_operation("peel_to_tree"))?;

    if force {
        info!("fast_forward: using force strategy (from config)");
        let mut checkout_builder = git2::build::CheckoutBuilder::new();
        checkout_builder.force();
        repo.checkout_tree(target_tree.as_object(), Some(&mut checkout_builder))
            .map_err(|e| GitError::from(e).with_operation("checkout_tree"))?;
    } else {
        info!("fast_forward: using safe strategy");
        let mut checkout_builder = git2::build::CheckoutBuilder::new();
        checkout_builder.safe();
        if let Err(e) = repo.checkout_tree(target_tree.as_object(), Some(&mut checkout_builder)) {
            // Same re-derivation as checkout_branch_internal_impl: safe mode's
            // error alone doesn't say which files stood in the way.
            return Err(checkout_refusal(&repo, &target_tree, "fast_forward", e));
        }
    }

    // Move the branch ref itself forward. Unlike checkout_branch, HEAD stays
    // on the same branch throughout — only the commit that branch points to
    // changes.
    head.set_target(
        their_commit.id(),
        &format!(
            "fast-forward: {} -> {}",
            previous_commit.id(),
            their_commit.id()
        ),
    )
    .map_err(|e| GitError::from(e).with_operation("set_target"))?;

    // Finalise the working tree and index against the now-updated HEAD,
    // mirroring checkout_branch_internal_impl's final materialisation step.
    let mut final_builder = git2::build::CheckoutBuilder::new();
    if force {
        final_builder.force();
    } else {
        final_builder.safe();
    }
    repo.checkout_head(Some(&mut final_builder))
        .map_err(|e| GitError::from(e).with_operation("checkout_head"))?;

    info!(
        "fast_forward: {} moved {} -> {} in {}ms",
        head_branch_name,
        previous_commit.id(),
        their_commit.id(),
        start.elapsed().as_millis()
    );

    Ok(FastForwardResult {
        branch: head_branch_name,
        previous_commit_hash: previous_commit.id().to_string(),
        commit_hash: their_commit.id().to_string(),
    })
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
    crate::utils::run_blocking(structured, move || {
        list_branches_impl(&repo_path, include_remote)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn get_current_branch(
    service: &GitService,
    repo_path: String,
) -> Result<Option<BranchInfo>> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || get_current_branch_impl(&repo_path)).await
}

#[cfg(feature = "napi-binding")]
pub async fn create_branch(
    service: &GitService,
    repo_path: String,
    options: CreateBranchOptions,
) -> Result<BranchInfo> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        create_branch_impl(&repo_path, &options)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn checkout_branch(
    service: &GitService,
    repo_path: String,
    branch_name: String,
) -> Result<BranchInfo> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        checkout_branch_impl(&repo_path, &branch_name)
    })
    .await
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
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        delete_branch_impl(&repo_path, &branch_name, force)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn merge_analysis(
    service: &GitService,
    repo_path: String,
    branch: String,
) -> Result<MergeAnalysis> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || merge_analysis_impl(&repo_path, &branch)).await
}

#[cfg(feature = "napi-binding")]
pub async fn fast_forward(
    service: &GitService,
    repo_path: String,
    branch: String,
) -> Result<FastForwardResult> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        fast_forward_impl(&repo_path, &branch)
    })
    .await
}

#[cfg(test)]
#[path = "branch_ops_tests.rs"]
mod tests;
