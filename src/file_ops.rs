// native/src/file_ops.rs

use crate::errors::GitError;
use git2::{IndexEntry, IndexTime, Repository, Signature, Status, StatusOptions};
use log::{error, info};
use std::fs;

use crate::utils::normalize_git_path;
use std::path::Path;

pub fn move_file_impl(
    repo_path: &str,
    source_file: &str,
    dest_file: &str,
    message: &str,
    user_name: &str,
    user_email: &str,
) -> Result<String, GitError> {
    info!("move_file: source={} dest={}", source_file, dest_file);
    let start = std::time::Instant::now();

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("move_file"))?;

    // 1. Physically move the file
    let source_abs = Path::new(repo_path).join(source_file);
    let dest_abs = Path::new(repo_path).join(dest_file);

    if let Some(parent) = dest_abs.parent() {
        fs::create_dir_all(parent).map_err(|e| GitError::IoError {
            operation: "create_dir_all".to_string(),
            error: e.to_string(),
        })?;
    }
    fs::rename(&source_abs, &dest_abs).map_err(|e| GitError::IoError {
        operation: "rename".to_string(),
        error: e.to_string(),
    })?;

    // 2. Stage the rename using our improved staging function
    stage_rename_impl(repo_path, source_file, dest_file)?;

    // 3. Commit the changes
    let result = commit_impl(&repo, message, user_name, user_email)?;

    info!("move_file: success in {}ms", start.elapsed().as_millis());
    Ok(result)
}

pub fn move_directory_impl(
    repo_path: &str,
    source_dir: &str,
    dest_dir: &str,
    message: &str,
    user_name: &str,
    user_email: &str,
) -> Result<String, GitError> {
    info!("move_directory: source={} dest={}", source_dir, dest_dir);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("move_directory"))?;
    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("move_directory"))?;

    let source_abs = Path::new(repo_path).join(source_dir);
    let dest_abs = Path::new(repo_path).join(dest_dir);

    // Ensure destination parent directory exists
    if let Some(parent) = dest_abs.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| GitError::from(e).with_io_operation("create_dir_all"))?;
    }

    // Recursively move files and update index
    for entry in walkdir::WalkDir::new(&source_abs) {
        let entry = entry.map_err(|e| GitError::IoError {
            operation: "walkdir".to_string(),
            error: e.to_string(),
        })?;
        let path = entry.path();

        if path.is_file() {
            let relative_path_from_source =
                path.strip_prefix(&source_abs)
                    .map_err(|e| GitError::InvalidPath {
                        path: path.display().to_string(),
                        reason: format!("strip_prefix failed: {}", e),
                    })?;
            let new_abs_path = dest_abs.join(relative_path_from_source);
            let new_repo_relative_path = Path::new(dest_dir).join(relative_path_from_source);
            let old_repo_relative_path = Path::new(source_dir).join(relative_path_from_source);

            if let Some(parent) = new_abs_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| GitError::from(e).with_io_operation("create_dir_all"))?;
            }
            fs::rename(path, &new_abs_path)
                .map_err(|e| GitError::from(e).with_io_operation("rename"))?;

            index
                .remove_path(&old_repo_relative_path)
                .map_err(|e| GitError::from(e).with_operation("remove_path"))?;
            index
                .add_path(&new_repo_relative_path)
                .map_err(|e| GitError::from(e).with_operation("add_path"))?;
        }
    }

    // Remove the original (now empty) source directory
    fs::remove_dir_all(&source_abs)
        .map_err(|e| GitError::from(e).with_io_operation("remove_dir_all"))?;

    // Remove the source directory from the index if it was tracked as an empty directory
    // This is a bit tricky as git doesn't track empty directories directly.
    // We ensure all its contents are removed and new ones added.
    // If the directory itself was explicitly added (e.g., as a submodule or via .gitkeep),
    // we'd need more sophisticated logic. For now, assuming only files within are tracked.

    index
        .write()
        .map_err(|e| GitError::from(e).with_operation("write_index"))?;

    let result = commit_impl(&repo, message, user_name, user_email)?;

    info!(
        "move_directory: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(result)
}

pub fn commit_file_impl(
    repo_path: &str,
    file_path: &str,
    message: &str,
    user_name: &str,
    user_email: &str,
) -> Result<String, GitError> {
    info!("commit_file: path={}", file_path);
    let start = std::time::Instant::now();

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("commit_file"))?;
    let relative_path = crate::utils::validate_and_normalize_path_git(repo_path, file_path)?;

    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("commit_file"))?;
    index
        .add_path(&relative_path)
        .map_err(|e| GitError::from(e).with_operation("add_path"))?;
    index
        .write()
        .map_err(|e| GitError::from(e).with_operation("write_index"))?;

    let result = commit_impl(&repo, message, user_name, user_email)?;

    info!("commit_file: success in {}ms", start.elapsed().as_millis());
    Ok(result)
}

pub fn commit_files_impl(
    repo_path: &str,
    file_paths: &[String],
    message: &str,
    user_name: &str,
    user_email: &str,
) -> Result<String, GitError> {
    info!("commit_files: count={}", file_paths.len());
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("commit_files"))?;
    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("commit_files"))?;

    for file_path in file_paths {
        let relative_path = crate::utils::validate_and_normalize_path_git(repo_path, file_path)?;
        index
            .add_path(&relative_path)
            .map_err(|e| GitError::from(e).with_operation("add_path"))?;
    }

    index
        .write()
        .map_err(|e| GitError::from(e).with_operation("write_index"))?;

    let result = commit_impl(&repo, message, user_name, user_email)?;

    info!("commit_files: success in {}ms", start.elapsed().as_millis());
    Ok(result)
}

fn commit_impl(
    repo: &Repository,
    message: &str,
    user_name: &str,
    user_email: &str,
) -> Result<String, GitError> {
    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("commit_impl"))?;
    let tree_id = index
        .write_tree()
        .map_err(|e| GitError::from(e).with_operation("write_tree"))?;

    // Check if there are actually changes to commit
    if let Ok(head) = repo.head()
        && let Ok(head_commit) = head.peel_to_commit()
        && head_commit.tree_id() == tree_id
    {
        return Err(GitError::NothingToCommit);
    }

    // Get the tree object from the tree_id
    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| GitError::from(e).with_operation("find_tree"))?;

    let signature = Signature::now(user_name, user_email)
        .map_err(|e| GitError::from(e).with_operation("create_signature"))?;

    let parent_commit = match repo.head() {
        Ok(head) => {
            let target = head.target().ok_or(GitError::DetachedHead)?;
            Some(
                repo.find_commit(target)
                    .map_err(|e| GitError::from(e).with_operation("find_commit"))?,
            )
        }
        Err(_) => None,
    };

    let commit_id = match parent_commit {
        Some(parent) => repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[&parent],
            )
            .map_err(|e| GitError::from(e).with_operation("create_commit"))?,
        None => repo
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
            .map_err(|e| GitError::from(e).with_operation("create_commit"))?,
    };

    Ok(commit_id.to_string())
}

pub fn stage_file_impl(repo_path: &str, file_path: &str) -> Result<bool, GitError> {
    info!("stage_file: path={}", file_path);
    let start = std::time::Instant::now();

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("stage_file"))?;
    let relative_path = crate::utils::validate_and_normalize_path_git(repo_path, file_path)?;

    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("stage_file"))?;

    // Check if path is a directory by checking if it exists on disk
    let full_path = std::path::Path::new(repo_path).join(&relative_path);
    if full_path.is_dir() {
        // Use add_all for directories (recursive staging like `git add .nocturne`)
        let pathspec = relative_path.to_string_lossy().to_string();
        let pathspecs = [pathspec.as_str()];
        index
            .add_all(pathspecs.iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| GitError::from(e).with_operation("add_all"))?;
    } else {
        // Use add_path for individual files
        index
            .add_path(&relative_path)
            .map_err(|e| GitError::from(e).with_operation("add_path"))?;
    }

    index
        .write()
        .map_err(|e| GitError::from(e).with_operation("write_index"))?;

    info!("stage_file: success in {}ms", start.elapsed().as_millis());
    Ok(true)
}

/// Stage a file deletion
pub fn stage_deletion_impl(repo_path: &str, file_path: &str) -> Result<bool, GitError> {
    info!("stage_deletion: path={}", file_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("stage_deletion"))?;
    let relative_path = crate::utils::validate_and_normalize_path_git(repo_path, file_path)?;

    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("stage_deletion"))?;
    index
        .remove_path(&relative_path)
        .map_err(|e| GitError::from(e).with_operation("remove_path"))?;
    index
        .write()
        .map_err(|e| GitError::from(e).with_operation("write_index"))?;

    info!(
        "stage_deletion: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

/// Stage a file rename (combination of deletion and addition)
pub fn stage_rename_impl(
    repo_path: &str,
    old_path: &str,
    new_path: &str,
) -> Result<bool, GitError> {
    info!("stage_rename: old={} new={}", old_path, new_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("stage_rename"))?;
    let old_relative_path = crate::utils::validate_and_normalize_path_git(repo_path, old_path)?;
    let new_relative_path = crate::utils::validate_and_normalize_path_git(repo_path, new_path)?;

    // Verify the new file exists on disk before staging
    let new_abs_path = std::path::Path::new(repo_path).join(&new_relative_path);
    if !new_abs_path.exists() {
        return Err(GitError::FileNotFound {
            path: format!("{} (destination for rename from {})", new_path, old_path),
        });
    }

    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("stage_rename"))?;

    // Remove the old path from index
    index
        .remove_path(&old_relative_path)
        .map_err(|e| GitError::from(e).with_operation("remove_path"))?;

    // Add the new path from working directory
    index
        .add_path(&new_relative_path)
        .map_err(|e| GitError::from(e).with_operation("add_path"))?;

    // Verify the new path was actually added to the index
    if index.get_path(&new_relative_path, 0).is_none() {
        return Err(GitError::GitOperationFailure {
            operation: "stage_rename".to_string(),
            class: 0,
            code: 0,
            message: format!("File exists at {} but failed to add to Git index", new_path),
        });
    }

    index
        .write()
        .map_err(|e| GitError::from(e).with_operation("write_index"))?;

    info!("stage_rename: success in {}ms", start.elapsed().as_millis());
    Ok(true)
}

/// Commit all staged changes (whatever is in the index)
pub fn commit_staged_changes_impl(
    repo_path: &str,
    message: &str,
    user_name: &str,
    user_email: &str,
) -> Result<String, GitError> {
    info!("commit_staged_changes");
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("commit_staged_changes"))?;

    let result = commit_impl(&repo, message, user_name, user_email)?;

    info!(
        "commit_staged_changes: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(result)
}

/// Unstage a file from the index (reset to HEAD state)
///
/// This operation resets the index entry for the specified file to match HEAD,
/// effectively unstaging any changes. The working tree is always preserved,
/// so no data is lost - changes simply become "unstaged" instead of "staged".
///
/// # Safety Checks
/// 1. Verifies repository is not bare (needs working tree)
/// 2. Returns success if file is not staged (idempotent)
/// 3. Requires HEAD to exist (fails in empty repository)
///
/// # Arguments
/// * `repo_path` - Path to repository
/// * `file_path` - Path to file to unstage
/// * `force` - Reserved for future use (unstaging is inherently safe)
///
/// # Behavior
/// - If file exists in HEAD: resets index entry to HEAD version
/// - If file not in HEAD (newly added): removes from index entirely
/// - Working tree is never modified
///
/// # Returns
/// * `Ok(true)` - File successfully unstaged or was not staged
/// * `Err(GitError)` - Repository error, bare repo, or empty repository
pub fn unstage_file_impl(repo_path: &str, file_path: &str, _force: bool) -> Result<bool, GitError> {
    info!("unstage_file: path={}", file_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("unstage_file"))?;

    // Safety check: ensure repository has a working tree
    if repo.is_bare() {
        return Err(GitError::GitOperationFailure {
            operation: "unstage_file".to_string(),
            class: 0,
            code: 0,
            message: "Cannot unstage files in bare repository (no working tree)".to_string(),
        });
    }

    let relative_path = crate::utils::validate_and_normalize_path_git(repo_path, file_path)?;

    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("unstage_file"))?;

    // Check if file is actually staged
    if index.get_path(&relative_path, 0).is_none() {
        // File not staged - nothing to do, return success
        info!("unstage_file: file not staged, nothing to do");
        return Ok(true);
    }

    // Get HEAD commit (error if no HEAD)
    let head = repo.head().map_err(|_| GitError::GitOperationFailure {
        operation: "get_head".to_string(),
        class: 0,
        code: 0,
        message: "Cannot unstage in empty repository (no HEAD)".to_string(),
    })?;

    let target = head.target().ok_or(GitError::DetachedHead)?;
    let commit = repo
        .find_commit(target)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;
    let tree = commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    // Note: Unstaging is generally safe because it preserves the working tree.
    // It only resets the index entry to match HEAD (or removes it if file not in HEAD).
    // Working tree changes are preserved as "unstaged changes".
    // The `force` parameter is provided for API consistency but isn't strictly necessary
    // for unstaging operations.

    // Perform the unstage operation
    if let Ok(tree_entry) = tree.get_path(&relative_path) {
        // File exists in HEAD - reset index entry to HEAD version
        index
            .add(&IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: tree_entry.filemode() as u32,
                uid: 0,
                gid: 0,
                file_size: 0,
                id: tree_entry.id(),
                flags: 0,
                flags_extended: 0,
                path: relative_path.to_string_lossy().as_bytes().to_vec(),
            })
            .map_err(|e| GitError::from(e).with_operation("add_index_entry"))?;
    } else {
        // File not in HEAD - remove from index entirely
        // This is for newly added files that were staged
        index
            .remove_path(&relative_path)
            .map_err(|e| GitError::from(e).with_operation("remove_path"))?;
    }

    index
        .write()
        .map_err(|e| GitError::from(e).with_operation("write_index"))?;

    info!("unstage_file: success in {}ms", start.elapsed().as_millis());
    Ok(true)
}

pub fn get_staged_files_impl(repo_path: &str) -> Result<Vec<String>, GitError> {
    info!("get_staged_files");
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_staged_files"))?;

    let mut opts = StatusOptions::new();
    opts.include_untracked(true);
    opts.include_ignored(false);

    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| GitError::from(e).with_operation("get_status"))?;
    let mut staged_files = Vec::new();

    for entry in statuses.iter() {
        let status_flags = entry.status();

        if status_flags.contains(Status::INDEX_MODIFIED)
            || status_flags.contains(Status::INDEX_NEW)
            || status_flags.contains(Status::INDEX_DELETED)
        {
            staged_files.push(normalize_git_path(entry.path().unwrap_or("invalid_path")));
        }
    }

    info!(
        "get_staged_files: found {} files in {}ms",
        staged_files.len(),
        start.elapsed().as_millis()
    );
    Ok(staged_files)
}

// Restore file from commit
pub fn restore_file_from_commit_impl(
    repo_path: &str,
    file_path: &str,
    commit_hash: &str,
) -> Result<bool, GitError> {
    info!(
        "restore_file_from_commit: path={} commit={}",
        file_path, commit_hash
    );
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("restore_file_from_commit"))?;

    let oid = git2::Oid::from_str(commit_hash).map_err(|_e| GitError::InvalidCommitHash {
        hash: commit_hash.to_string(),
    })?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;
    let tree = commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    let relative_path = std::path::Path::new(file_path);
    let absolute_path = std::path::Path::new(repo_path).join(relative_path);

    match tree.get_path(relative_path) {
        Ok(tree_entry) => {
            let blob = repo
                .find_blob(tree_entry.id())
                .map_err(|e| GitError::from(e).with_operation("find_blob"))?;

            // Check for conflict: file exists with uncommitted changes
            if absolute_path.exists() {
                // Check if file has uncommitted changes by comparing with HEAD
                let head = repo
                    .head()
                    .map_err(|e| GitError::from(e).with_operation("get_head"))?;
                let head_commit = head
                    .peel_to_commit()
                    .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
                let head_tree = head_commit
                    .tree()
                    .map_err(|e| GitError::from(e).with_operation("get_head_tree"))?;

                // Check if file exists in HEAD and compare content
                if let Ok(head_entry) = head_tree.get_path(relative_path) {
                    let head_blob = repo
                        .find_blob(head_entry.id())
                        .map_err(|e| GitError::from(e).with_operation("find_head_blob"))?;

                    // Read current working directory content
                    let working_content =
                        fs::read(&absolute_path).map_err(|e| GitError::IoError {
                            operation: "read_working_file".to_string(),
                            error: e.to_string(),
                        })?;

                    // If working content differs from HEAD, we have uncommitted changes
                    if working_content != head_blob.content() {
                        return Err(GitError::UnstagedChangesWouldBeLost {
                            files: vec![file_path.to_string()],
                        });
                    }
                } else {
                    // File exists in working tree but not in HEAD (new file)
                    return Err(GitError::UnstagedChangesWouldBeLost {
                        files: vec![file_path.to_string()],
                    });
                }
            }

            // Create parent directories if needed
            if let Some(parent) = absolute_path.parent() {
                fs::create_dir_all(parent).map_err(|e| GitError::IoError {
                    operation: "create_dir_all".to_string(),
                    error: e.to_string(),
                })?;
            }

            // Write file content
            fs::write(&absolute_path, blob.content()).map_err(|e| GitError::IoError {
                operation: "write_file".to_string(),
                error: e.to_string(),
            })?;

            info!(
                "restore_file_from_commit: success in {}ms",
                start.elapsed().as_millis()
            );
            Ok(true)
        }
        Err(_) => {
            error!("restore_file_from_commit: file not found in commit");
            Err(GitError::FileNotFound {
                path: file_path.to_string(),
            })
        }
    }
}

/// Discard uncommitted changes in a file (restore to HEAD state)
///
/// This operation restores the working tree file to match the HEAD commit,
/// discarding any uncommitted changes. Both the working tree and index are
/// updated to match HEAD. Preserves symlinks, executable bits, and permissions.
///
/// # Arguments
/// * `repo_path` - Path to the git repository
/// * `file_path` - Path to the file to discard changes for
///
/// # Returns
/// * `Ok(true)` - Changes successfully discarded
/// * `Err(GitError::FileNotFound)` - File not in HEAD tree
/// * `Err(GitError::DetachedHead)` - Repository is in detached HEAD state
///
/// # Example
/// ```no_run
/// # fn main() -> Result<(), liminal_git::GitError> {
/// use liminal_git::file_ops::discard_changes_impl;
///
/// discard_changes_impl("/repo", "file.txt")?;
/// # Ok(())
/// # }
/// ```
pub fn discard_changes_impl(repo_path: &str, file_path: &str) -> Result<bool, GitError> {
    info!("discard_changes: path={}", file_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("discard_changes"))?;
    let relative_path = crate::utils::validate_and_normalize_path_git(repo_path, file_path)?;

    // Get HEAD commit
    let head = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;
    let target = head.target().ok_or(GitError::DetachedHead)?;
    let commit = repo
        .find_commit(target)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;
    let tree = commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    // Verify the file exists in the tree
    tree.get_path(&relative_path)
        .map_err(|_| GitError::FileNotFound {
            path: file_path.to_string(),
        })?;

    // Use checkout to restore the file with proper symlink/permission handling
    let mut checkout_builder = git2::build::CheckoutBuilder::new();
    checkout_builder
        .path(&relative_path)
        .force() // Overwrite working tree changes
        .update_index(true) // Update index to match HEAD
        .disable_filters(false); // Ensure CRLF filters are applied

    repo.checkout_tree(tree.as_object(), Some(&mut checkout_builder))
        .map_err(|e| GitError::from(e).with_operation("checkout_tree"))?;

    info!(
        "discard_changes: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

/// Amend the last commit with new changes
///
/// This operation amends the most recent commit with whatever is currently
/// staged in the index, optionally updating the commit message.
///
/// # Arguments
/// * `repo_path` - Path to the git repository
/// * `message` - New commit message (if empty, reuse previous message)
/// * `user_name` - Optional user name (None = read from config)
/// * `user_email` - Optional user email (None = read from config)
///
/// # Returns
/// * `Ok(commit_hash)` - Hash of the amended commit
/// * `Err(GitError::DetachedHead)` - Repository is in detached HEAD state
/// * `Err(GitError::ConfigMissing)` - user.name/email not configured
///
/// # Example
/// ```no_run
/// # fn main() -> Result<(), liminal_git::GitError> {
/// use liminal_git::file_ops::commit_amend_impl;
///
/// // Amend with new message
/// commit_amend_impl("/repo", "Updated message", Some("Alice"), Some("alice@example.com"))?;
///
/// // Amend keeping original message
/// commit_amend_impl("/repo", "", None, None)?;
/// # Ok(())
/// # }
/// ```
pub fn commit_amend_impl(
    repo_path: &str,
    message: &str,
    user_name: Option<&str>,
    user_email: Option<&str>,
) -> Result<String, GitError> {
    info!("commit_amend: message_len={}", message.len());
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("commit_amend"))?;

    // Get HEAD commit
    let head = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;
    let target = head.target().ok_or(GitError::DetachedHead)?;
    let head_commit = repo
        .find_commit(target)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;

    // Get the new tree from the index
    let mut index = repo
        .index()
        .map_err(|e| GitError::from(e).with_operation("get_index"))?;
    let tree_id = index
        .write_tree()
        .map_err(|e| GitError::from(e).with_operation("write_tree"))?;
    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| GitError::from(e).with_operation("find_tree"))?;

    // Determine the commit message (empty = reuse original)
    let commit_message = if message.trim().is_empty() {
        head_commit.message().unwrap_or("(no message)")
    } else {
        message
    };

    // Get signature using the new helper (lenient validation)
    let signature = crate::utils::read_user_signature(&repo, user_name, user_email)?;

    // Get parent commits (amend keeps the same parents as original)
    // We collect parents into a Vec to ensure they live long enough
    let parents: Vec<_> = head_commit.parents().collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    // Create the amended commit without updating any reference
    let new_commit_id = repo
        .commit(
            None, // Don't update any reference yet
            &signature,
            &signature,
            commit_message,
            &tree,
            &parent_refs,
        )
        .map_err(|e| GitError::from(e).with_operation("create_commit"))?;

    // Update the branch reference (not HEAD directly to avoid detaching)
    // If HEAD is symbolic (points to a branch), update that branch
    // If HEAD is detached, update HEAD directly
    let reflog_message = format!("commit (amend): {}", commit_message);

    if head.is_branch() {
        // HEAD points to a branch - update the branch reference
        let branch_name = head.name().ok_or_else(|| GitError::GitOperationFailure {
            operation: "get_branch_name".to_string(),
            class: 0,
            code: 0,
            message: "HEAD has no name".to_string(),
        })?;

        repo.reference(
            branch_name,
            new_commit_id,
            true, // Force update
            &reflog_message,
        )
        .map_err(|e| GitError::from(e).with_operation("update_branch"))?;
    } else {
        // HEAD is detached - update HEAD directly
        repo.reference(
            "HEAD",
            new_commit_id,
            true, // Force update
            &reflog_message,
        )
        .map_err(|e| GitError::from(e).with_operation("update_head"))?;
    }

    info!("commit_amend: success in {}ms", start.elapsed().as_millis());
    Ok(new_commit_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn setup_test_repo() -> (tempfile::TempDir, PathBuf) {
        // Use consistent temp dir location to avoid cross-device link issues
        let temp_dir =
            tempfile::TempDir::new_in(std::env::temp_dir()).expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize a git repository
        Repository::init(&repo_path).expect("Failed to initialize test repository");

        // Set user config
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("user.name", "Test User")
            .expect("Failed to set user.name");
        config
            .set_str("user.email", "test@example.com")
            .expect("Failed to set user.email");

        (temp_dir, repo_path)
    }

    fn create_test_file(repo_path: &Path, file_path: &str, content: &str) {
        let full_path = repo_path.join(file_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent dir");
        }
        fs::write(&full_path, content).expect("Failed to write file");
    }

    fn commit_file(repo_path: &Path, file_path: &str, message: &str) -> String {
        let repo = Repository::open(repo_path).expect("Failed to open repository");
        let relative_path = Path::new(file_path);

        let mut index = repo.index().expect("Failed to get index");
        index.add_path(relative_path).expect("Failed to add path");
        index.write().expect("Failed to write index");

        let tree_id = index.write_tree().expect("Failed to write tree");
        let tree = repo.find_tree(tree_id).expect("Failed to find tree");
        let signature =
            Signature::now("Test User", "test@example.com").expect("Failed to create signature");

        let parent_commit = repo.head().ok().and_then(|head| {
            head.target()
                .and_then(|target| repo.find_commit(target).ok())
        });

        let commit_id = match parent_commit {
            Some(parent) => repo
                .commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    message,
                    &tree,
                    &[&parent],
                )
                .expect("Failed to commit"),
            None => repo
                .commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .expect("Failed to commit"),
        };

        commit_id.to_string()
    }

    // ========== discard_changes_impl tests ==========

    #[test]
    fn test_discard_changes_basic() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create and commit a file
        create_test_file(&repo_path, "test.txt", "original content");
        commit_file(&repo_path, "test.txt", "Initial commit");

        // Modify the file
        create_test_file(&repo_path, "test.txt", "modified content");

        // Discard changes
        let result = discard_changes_impl(repo_path.to_str().unwrap(), "test.txt");

        assert!(result.is_ok());

        // Verify file content is restored
        let content = fs::read_to_string(repo_path.join("test.txt")).expect("Failed to read file");
        assert_eq!(content, "original content");
    }

    #[test]
    fn test_discard_changes_staged_and_unstaged() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create and commit a file
        create_test_file(&repo_path, "test.txt", "original");
        commit_file(&repo_path, "test.txt", "Initial commit");

        // Modify and stage
        create_test_file(&repo_path, "test.txt", "staged");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Modify again (unstaged)
        create_test_file(&repo_path, "test.txt", "unstaged");

        // Discard changes
        let result = discard_changes_impl(repo_path.to_str().unwrap(), "test.txt");

        assert!(result.is_ok());

        // Verify both working tree and index restored
        let content = fs::read_to_string(repo_path.join("test.txt")).expect("Failed to read file");
        assert_eq!(content, "original");
    }

    #[test]
    fn test_discard_changes_file_not_in_head() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit with a file
        create_test_file(&repo_path, "existing.txt", "content");
        commit_file(&repo_path, "existing.txt", "Initial commit");

        // Create a new file (not in HEAD)
        create_test_file(&repo_path, "new.txt", "new content");

        // Try to discard changes for new file
        let result = discard_changes_impl(repo_path.to_str().unwrap(), "new.txt");

        assert!(result.is_err());
        match result {
            Err(GitError::FileNotFound { .. }) => {
                // Expected error
            }
            _ => panic!("Expected FileNotFound error"),
        }
    }

    #[test]
    fn test_discard_changes_detached_head() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create and commit a file
        create_test_file(&repo_path, "test.txt", "content");
        let commit_hash = commit_file(&repo_path, "test.txt", "Initial commit");

        // Detach HEAD
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let oid = git2::Oid::from_str(&commit_hash).expect("Failed to parse OID");
        repo.set_head_detached(oid).expect("Failed to detach HEAD");

        // Modify file
        create_test_file(&repo_path, "test.txt", "modified");

        // Discard should still work with detached HEAD
        let result = discard_changes_impl(repo_path.to_str().unwrap(), "test.txt");

        // Should succeed - detached HEAD still has a target
        assert!(result.is_ok());
    }

    // ========== commit_amend_impl tests ==========

    #[test]
    fn test_commit_amend_with_new_message() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "test.txt", "content");
        let original_hash = commit_file(&repo_path, "test.txt", "Original message");

        // Stage a change
        create_test_file(&repo_path, "test.txt", "updated content");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Amend with new message
        let result = commit_amend_impl(
            repo_path.to_str().unwrap(),
            "Amended message",
            Some("Test User"),
            Some("test@example.com"),
        );

        assert!(result.is_ok());
        let new_hash = result.unwrap();

        // Hash should be different
        assert_ne!(original_hash, new_hash);

        // Verify message changed
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(commit.message().unwrap(), "Amended message");
    }

    #[test]
    fn test_commit_amend_keep_original_message() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "test.txt", "content");
        commit_file(&repo_path, "test.txt", "Original message");

        // Stage a change
        create_test_file(&repo_path, "test.txt", "updated content");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Amend with empty message (should keep original)
        let result = commit_amend_impl(
            repo_path.to_str().unwrap(),
            "",
            Some("Test User"),
            Some("test@example.com"),
        );

        assert!(result.is_ok());

        // Verify message unchanged
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(commit.message().unwrap(), "Original message");
    }

    #[test]
    fn test_commit_amend_with_config_signature() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "test.txt", "content");
        commit_file(&repo_path, "test.txt", "Original message");

        // Stage a change
        create_test_file(&repo_path, "test.txt", "updated content");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Amend with None signature (should read from config)
        let result = commit_amend_impl(repo_path.to_str().unwrap(), "Amended", None, None);

        assert!(result.is_ok());

        // Verify signature from config
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(commit.author().name().unwrap(), "Test User");
        assert_eq!(commit.author().email().unwrap(), "test@example.com");
    }

    #[test]
    fn test_commit_amend_detached_head() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "test.txt", "content");
        let commit_hash = commit_file(&repo_path, "test.txt", "Initial commit");

        // Detach HEAD
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let oid = git2::Oid::from_str(&commit_hash).expect("Failed to parse OID");
        repo.set_head_detached(oid).expect("Failed to detach HEAD");

        // Try to amend
        let result = commit_amend_impl(
            repo_path.to_str().unwrap(),
            "Amended",
            Some("Test User"),
            Some("test@example.com"),
        );

        // Should succeed - detached HEAD can still be amended
        assert!(result.is_ok());

        // Verify HEAD is still detached (not moved to a branch)
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let head = repo.head().expect("Failed to get HEAD");
        assert!(
            !head.is_branch(),
            "HEAD should still be detached after amend"
        );
    }

    #[test]
    fn test_commit_amend_empty_repository() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Try to amend in empty repository (no commits)
        let result = commit_amend_impl(
            repo_path.to_str().unwrap(),
            "Amended",
            Some("Test User"),
            Some("test@example.com"),
        );

        // Should fail - no HEAD to amend
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(not(target_env = "msvc"), serial_test::serial)]
    fn test_commit_amend_missing_config() {
        use std::env;

        // Save original env vars
        let orig_home = env::var("HOME").ok();
        let orig_xdg = env::var("XDG_CONFIG_HOME").ok();
        let orig_git_config = env::var("GIT_CONFIG_GLOBAL").ok();

        // Set up completely isolated config environment
        let base_temp = std::env::temp_dir();
        let config_dir =
            tempfile::TempDir::new_in(&base_temp).expect("Failed to create temp config dir");
        let config_file = config_dir.path().join("gitconfig");
        let home_dir =
            tempfile::TempDir::new_in(&base_temp).expect("Failed to create temp home dir");

        unsafe {
            env::set_var("GIT_CONFIG_GLOBAL", config_file.as_os_str());
            env::set_var("HOME", home_dir.path().as_os_str());
            env::set_var("XDG_CONFIG_HOME", home_dir.path().as_os_str());
            env::set_var("GIT_CONFIG_NOSYSTEM", "1"); // Don't read system config
        }

        // Create repo WITHOUT setting user config
        let temp_dir = tempfile::TempDir::new_in(&base_temp).expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();
        Repository::init(&repo_path).expect("Failed to initialize test repository");

        // Create initial commit with explicit user
        create_test_file(&repo_path, "test.txt", "content");
        commit_file(&repo_path, "test.txt", "Initial commit");

        // Stage a change
        create_test_file(&repo_path, "test.txt", "updated");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Try to amend without config and without explicit params
        let result = commit_amend_impl(repo_path.to_str().unwrap(), "Amended", None, None);

        // Restore original env vars
        unsafe {
            if let Some(val) = orig_home {
                env::set_var("HOME", val);
            } else {
                env::remove_var("HOME");
            }
            if let Some(val) = orig_xdg {
                env::set_var("XDG_CONFIG_HOME", val);
            } else {
                env::remove_var("XDG_CONFIG_HOME");
            }
            if let Some(val) = orig_git_config {
                env::set_var("GIT_CONFIG_GLOBAL", val);
            } else {
                env::remove_var("GIT_CONFIG_GLOBAL");
            }
            env::remove_var("GIT_CONFIG_NOSYSTEM");
        }

        // Should fail - config missing
        assert!(result.is_err());
        match result {
            Err(GitError::ConfigMissing { .. }) => {
                // Expected error
            }
            _ => panic!("Expected ConfigMissing error, got: {:?}", result),
        }
    }

    #[test]
    fn test_commit_amend_preserves_branch() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "test.txt", "content");
        commit_file(&repo_path, "test.txt", "Initial commit");

        // Stage a change
        create_test_file(&repo_path, "test.txt", "updated");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Amend
        let result = commit_amend_impl(
            repo_path.to_str().unwrap(),
            "Amended",
            Some("Test User"),
            Some("test@example.com"),
        );

        assert!(result.is_ok());

        // Verify HEAD is still on a branch (not detached)
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let head = repo.head().expect("Failed to get HEAD");
        assert!(head.is_branch(), "HEAD should still point to a branch");

        // Verify the branch ref was updated
        let branch = repo
            .find_branch("master", git2::BranchType::Local)
            .or_else(|_| repo.find_branch("main", git2::BranchType::Local))
            .expect("Failed to find main/master branch");
        let branch_target = branch.get().target().expect("Branch has no target");
        let head_target = head.target().expect("HEAD has no target");
        assert_eq!(
            branch_target, head_target,
            "Branch should point to same commit as HEAD"
        );
    }

    #[test]
    fn test_discard_changes_bare_repository() {
        let temp_dir =
            tempfile::TempDir::new_in(std::env::temp_dir()).expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize a bare repository
        Repository::init_bare(&repo_path).expect("Failed to initialize bare repository");

        // Try to discard changes in bare repo
        let result = discard_changes_impl(repo_path.to_str().unwrap(), "test.txt");

        // Should fail - bare repos have no working tree
        assert!(result.is_err());
    }

    // ========== unstage_file tests ==========

    #[test]
    fn test_unstage_file_basic() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "test.txt", "original");
        commit_file(&repo_path, "test.txt", "Initial commit");

        // Modify and stage
        create_test_file(&repo_path, "test.txt", "modified");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Verify file is staged
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let index = repo.index().expect("Failed to get index");
        let entry_before = index.get_path(std::path::Path::new("test.txt"), 0);
        assert!(entry_before.is_some(), "File should be staged");
        drop(index);
        drop(repo);

        // Unstage the file
        let result = unstage_file_impl(repo_path.to_str().unwrap(), "test.txt", false);
        assert!(result.is_ok(), "Unstage should succeed");

        // Verify file is unstaged (index matches HEAD, not the modified version)
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let head = repo.head().expect("Failed to get HEAD");
        let commit = head.peel_to_commit().expect("Failed to get commit");
        let tree = commit.tree().expect("Failed to get tree");
        let tree_entry = tree
            .get_path(std::path::Path::new("test.txt"))
            .expect("File should exist in HEAD");

        let index = repo.index().expect("Failed to get index");
        let index_entry = index
            .get_path(std::path::Path::new("test.txt"), 0)
            .expect("File should still be in index");

        assert_eq!(
            index_entry.id,
            tree_entry.id(),
            "Index should match HEAD after unstaging"
        );

        // Verify working tree is preserved
        let full_path = repo_path.join("test.txt");
        let workdir_content =
            std::fs::read_to_string(&full_path).expect("Failed to read working tree file");
        assert_eq!(
            workdir_content, "modified",
            "Working tree should be preserved"
        );
    }

    #[test]
    fn test_unstage_file_not_staged() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "test.txt", "content");
        commit_file(&repo_path, "test.txt", "Initial commit");

        // Try to unstage a file that is not staged (idempotent operation)
        let result = unstage_file_impl(repo_path.to_str().unwrap(), "test.txt", false);
        assert!(
            result.is_ok(),
            "Unstaging non-staged file should succeed (idempotent)"
        );
    }

    #[test]
    fn test_unstage_file_empty_repository() {
        let temp_dir =
            tempfile::TempDir::new_in(std::env::temp_dir()).expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize repository without any commits
        Repository::init(&repo_path).expect("Failed to initialize repository");

        // Create and stage a file
        create_test_file(&repo_path, "test.txt", "content");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "test.txt");

        // Try to unstage in empty repository (no HEAD)
        let result = unstage_file_impl(repo_path.to_str().unwrap(), "test.txt", false);

        // Should fail - no HEAD to reset to
        assert!(result.is_err(), "Unstaging in empty repository should fail");
        match result {
            Err(GitError::GitOperationFailure { ref message, .. }) => {
                assert!(
                    message.contains("empty repository") || message.contains("no HEAD"),
                    "Error should mention empty repository or no HEAD, got: {}",
                    message
                );
            }
            _ => panic!("Expected GitOperationFailure, got: {:?}", result),
        }
    }

    #[test]
    fn test_unstage_file_bare_repository() {
        let temp_dir =
            tempfile::TempDir::new_in(std::env::temp_dir()).expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize a bare repository
        Repository::init_bare(&repo_path).expect("Failed to initialize bare repository");

        // Try to unstage in bare repo
        let result = unstage_file_impl(repo_path.to_str().unwrap(), "test.txt", false);

        // Should fail - bare repos have no working tree
        assert!(result.is_err(), "Unstaging in bare repository should fail");
        match result {
            Err(GitError::GitOperationFailure { ref message, .. }) => {
                assert!(
                    message.contains("bare repository"),
                    "Error should mention bare repository, got: {}",
                    message
                );
            }
            _ => panic!("Expected GitOperationFailure, got: {:?}", result),
        }
    }

    #[test]
    fn test_unstage_newly_added_file() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_test_file(&repo_path, "existing.txt", "existing");
        commit_file(&repo_path, "existing.txt", "Initial commit");

        // Create and stage a new file (not in HEAD)
        create_test_file(&repo_path, "new.txt", "new content");
        let _ = stage_file_impl(repo_path.to_str().unwrap(), "new.txt");

        // Verify file is staged
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let index = repo.index().expect("Failed to get index");
        assert!(
            index.get_path(std::path::Path::new("new.txt"), 0).is_some(),
            "New file should be staged"
        );
        drop(index);
        drop(repo);

        // Unstage the new file
        let result = unstage_file_impl(repo_path.to_str().unwrap(), "new.txt", false);
        assert!(result.is_ok(), "Unstaging new file should succeed");

        // Verify file is removed from index
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let index = repo.index().expect("Failed to get index");
        assert!(
            index.get_path(std::path::Path::new("new.txt"), 0).is_none(),
            "New file should be removed from index"
        );

        // Verify working tree file still exists
        let full_path = repo_path.join("new.txt");
        assert!(full_path.exists(), "Working tree file should still exist");
        let content = std::fs::read_to_string(&full_path).expect("Failed to read file");
        assert_eq!(
            content, "new content",
            "Working tree content should be preserved"
        );
    }
}
