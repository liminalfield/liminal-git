use crate::errors::GitError;
use crate::types::{FileStatus, GitStatus, RenamedStatus};
use crate::types::{GitConfig, RepositoryConfig, RepositoryHealth, RepositoryInfo};
use crate::utils::normalize_git_path;
use git2::{Diff, DiffDelta, DiffFindOptions, DiffOptions, Repository, Status, StatusOptions};
use log::info;
use std::fs;
use std::path::PathBuf;

pub fn get_status_impl(repo_path: &str) -> Result<GitStatus, GitError> {
    info!("get_status: repo_path={}", repo_path);
    let start = std::time::Instant::now();

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("get_status"))?;

    let mut opts = StatusOptions::new();
    opts.include_untracked(true);
    opts.include_ignored(false);
    opts.recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut opts))?;

    let mut modified_files = Vec::new();
    let mut deleted_files = Vec::new();
    let mut added_files = Vec::new();
    let mut untracked_files = Vec::new();
    let mut staged_files = Vec::new();
    let mut renamed_files = Vec::new();

    let extract_paths = |delta: DiffDelta| -> Option<(String, String)> {
        let old = normalize_git_path(&delta.old_file().path()?.to_string_lossy());
        let new = normalize_git_path(&delta.new_file().path()?.to_string_lossy());
        Some((old, new))
    };

    for entry in statuses.iter() {
        let path = normalize_git_path(entry.path().unwrap_or("invalid_path"));
        let status_flags = entry.status();

        if status_flags.contains(Status::INDEX_RENAMED)
            && let Some(delta) = entry.head_to_index()
            && let Some((old_path, new_path)) = extract_paths(delta)
        {
            renamed_files.push(RenamedStatus {
                old_path,
                new_path,
                staged: true,
            });
            continue;
        }

        if status_flags.contains(Status::WT_RENAMED)
            && let Some(delta) = entry.index_to_workdir()
            && let Some((old_path, new_path)) = extract_paths(delta)
        {
            renamed_files.push(RenamedStatus {
                old_path,
                new_path,
                staged: false,
            });
            continue;
        }

        if status_flags.contains(Status::INDEX_MODIFIED) {
            staged_files.push(FileStatus {
                path: path.clone(),
                status: "staged_modified".to_string(),
                staged: true,
            });
        }

        if status_flags.contains(Status::INDEX_NEW) {
            // Newly added file (staged but not yet committed)
            added_files.push(FileStatus {
                path: path.clone(),
                status: "added".to_string(),
                staged: true,
            });
            // Also add to staged_files for compatibility
            staged_files.push(FileStatus {
                path: path.clone(),
                status: "staged_added".to_string(),
                staged: true,
            });
        }

        if status_flags.contains(Status::INDEX_DELETED) {
            staged_files.push(FileStatus {
                path: path.clone(),
                status: "staged_deleted".to_string(),
                staged: true,
            });
            // Also add to deleted_files so deletion checks work correctly
            deleted_files.push(FileStatus {
                path: path.clone(),
                status: "deleted".to_string(),
                staged: true,
            });
        }

        if status_flags.contains(Status::WT_MODIFIED) {
            modified_files.push(FileStatus {
                path: path.clone(),
                status: "modified".to_string(),
                staged: false,
            });
        }

        if status_flags.contains(Status::WT_DELETED) {
            deleted_files.push(FileStatus {
                path: path.clone(),
                status: "deleted".to_string(),
                staged: false,
            });
        }

        if status_flags.contains(Status::WT_NEW) {
            untracked_files.push(FileStatus {
                path: path.clone(),
                status: "untracked".to_string(),
                staged: false,
            });
        }
    }

    // Additional rename detection using diffs for staged and unstaged changes
    let mut track_diff_renames = |diff: Diff, staged: bool| {
        for delta in diff.deltas() {
            if delta.status() == git2::Delta::Renamed
                && let Some((old_path, new_path)) = extract_paths(delta)
            {
                renamed_files.push(RenamedStatus {
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                    staged,
                });
            }
        }
    };

    let mut diff_opts = DiffOptions::new();
    diff_opts.include_untracked(true);

    if let Ok(head_tree) = repo.head().and_then(|h| h.peel_to_tree())
        && let Ok(mut diff_index) =
            repo.diff_tree_to_index(Some(&head_tree), None, Some(&mut diff_opts))
    {
        let mut find_opts = DiffFindOptions::new();
        find_opts
            .renames(true)
            .rename_threshold(40)
            .copy_threshold(40);
        let _ = diff_index.find_similar(Some(&mut find_opts));
        track_diff_renames(diff_index, true);
    }

    if let Ok(mut diff_wt) = repo.diff_index_to_workdir(None, Some(&mut diff_opts)) {
        let mut find_opts = DiffFindOptions::new();
        find_opts
            .renames(true)
            .rename_threshold(40)
            .copy_threshold(40);
        let _ = diff_wt.find_similar(Some(&mut find_opts));
        track_diff_renames(diff_wt, false);
    }

    // Additional rename detection: HEAD to working directory (covers full moves)
    if let Ok(head_tree) = repo.head().and_then(|h| h.peel_to_tree())
        && let Ok(mut diff_full) =
            repo.diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut diff_opts))
    {
        let mut find_opts = DiffFindOptions::new();
        find_opts
            .renames(true)
            .rename_threshold(40)
            .copy_threshold(40);
        let _ = diff_full.find_similar(Some(&mut find_opts));

        for delta in diff_full.deltas() {
            if delta.status() == git2::Delta::Renamed
                && let Some((old_path, new_path)) = extract_paths(delta)
            {
                // Check if this rename isn't already recorded
                let already_exists = renamed_files
                    .iter()
                    .any(|r| r.old_path == old_path && r.new_path == new_path);
                if !already_exists {
                    renamed_files.push(RenamedStatus {
                        old_path: old_path.clone(),
                        new_path: new_path.clone(),
                        staged: false, // This is an unstaged rename
                    });
                }
            }
        }
    }

    // Remove renamed paths from deleted/untracked sets and surface new path as modified
    for rename in &renamed_files {
        deleted_files.retain(|entry| entry.path != rename.old_path);
        untracked_files.retain(|entry| entry.path != rename.new_path);
        if !modified_files
            .iter()
            .any(|entry| entry.path == rename.new_path)
        {
            modified_files.push(FileStatus {
                path: rename.new_path.clone(),
                status: "renamed".to_string(),
                staged: rename.staged,
            });
        }
    }

    let is_clean = modified_files.is_empty()
        && deleted_files.is_empty()
        && added_files.is_empty()
        && untracked_files.is_empty()
        && staged_files.is_empty()
        && renamed_files.is_empty();

    // Get current branch
    let current_branch = match repo.head() {
        Ok(head) => head.shorthand().ok().map(|s| s.to_string()),
        Err(_) => None,
    };

    let result = GitStatus {
        modified_files,
        deleted_files,
        added_files,
        untracked_files,
        staged_files,
        renamed_files,
        is_clean,
        current_branch,
    };

    info!(
        "get_status: is_clean={} in {}ms",
        result.is_clean,
        start.elapsed().as_millis()
    );
    Ok(result)
}

pub fn is_repository_impl(path: &str) -> bool {
    let start = std::time::Instant::now();
    let is_repo = Repository::open(path).is_ok();
    info!(
        "is_repository: path={} result={} in {}ms",
        path,
        is_repo,
        start.elapsed().as_millis()
    );
    is_repo
}

// Repository initialization
pub fn init_repository_impl(path: &str) -> Result<bool, GitError> {
    info!("init_repository: path={}", path);
    let start = std::time::Instant::now();

    // Validate path first
    let repo_path = std::path::Path::new(path);
    if repo_path.exists()
        && repo_path
            .read_dir()
            .map_err(|e| GitError::IoError {
                operation: "read_directory".to_string(),
                error: e.to_string(),
            })?
            .next()
            .is_some()
    {
        return Err(GitError::InvalidPath {
            path: path.to_string(),
            reason: "Directory is not empty".to_string(),
        });
    }

    Repository::init(path).map_err(|e| GitError::from(e).with_operation("init_repository"))?;

    info!(
        "init_repository: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

/// Initialize a Git repository in a directory that may already contain files.
/// For duplicating an existing project: content is copied into place first,
/// then git is initialised over it.
/// Unlike init_repository_impl, this does NOT check if directory is empty.
pub fn init_repository_in_existing_dir_impl(path: &str) -> Result<bool, GitError> {
    info!("init_repository_in_existing_dir: path={}", path);
    let start = std::time::Instant::now();

    Repository::init(path)
        .map_err(|e| GitError::from(e).with_operation("init_repository_in_existing_dir"))?;

    info!(
        "init_repository_in_existing_dir: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

/// Remove all remotes from a repository.
/// Used when duplicating a repository with its history, so the copy cannot
/// accidentally push to the original's remotes.
pub fn remove_all_remotes_impl(repo_path: &str) -> Result<Vec<String>, GitError> {
    info!("remove_all_remotes: path={}", repo_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("remove_all_remotes"))?;

    let remotes = repo
        .remotes()
        .map_err(|e| GitError::from(e).with_operation("list_remotes"))?;

    let mut removed = Vec::new();
    for remote_name in remotes.iter().filter_map(|n| n.ok()).flatten() {
        repo.remote_delete(remote_name)
            .map_err(|e| GitError::from(e).with_operation("delete_remote"))?;
        removed.push(remote_name.to_string());
    }

    info!(
        "remove_all_remotes: removed {} remotes in {}ms",
        removed.len(),
        start.elapsed().as_millis()
    );
    Ok(removed)
}

pub fn init_repository_with_config_impl(
    path: &str,
    config: &RepositoryConfig,
) -> Result<bool, GitError> {
    info!("init_repository_with_config: path={}", path);
    let start = std::time::Instant::now();

    let repo =
        Repository::init(path).map_err(|e| GitError::from(e).with_operation("init_repository"))?;

    // Configure the repository
    let mut repo_config = repo
        .config()
        .map_err(|e| GitError::from(e).with_operation("get_config"))?;

    if let Some(ref description) = config.description {
        fs::write(
            PathBuf::from(path).join(".git").join("description"),
            description,
        )
        .map_err(|e| GitError::IoError {
            operation: "write_description".to_string(),
            error: e.to_string(),
        })?;
    }

    if let Some(ref line_ending) = config.line_ending {
        let autocrlf_value = match line_ending.as_str() {
            "lf" => "false",
            "crlf" => "true",
            "auto" => "input",
            _ => {
                return Err(GitError::InvalidPath {
                    path: line_ending.clone(),
                    reason: "Invalid line ending option".to_string(),
                });
            }
        };
        repo_config
            .set_str("core.autocrlf", autocrlf_value)
            .map_err(|e| GitError::from(e).with_operation("set_autocrlf"))?;
    }

    if let Some(ref branch) = config.default_branch {
        repo_config
            .set_str("init.defaultBranch", branch)
            .map_err(|e| GitError::from(e).with_operation("set_default_branch"))?;
    }

    info!(
        "init_repository_with_config: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

// Repository health checking
pub fn is_repository_healthy_impl(repo_path: &str) -> Result<RepositoryHealth, GitError> {
    info!("is_repository_healthy: path={}", repo_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("is_repository_healthy"))?;

    let mut issues = Vec::new();
    let mut warnings = Vec::new();

    // Check for basic repository integrity
    match repo.odb() {
        Ok(_) => {}
        Err(_) => issues.push("Object database is corrupted".to_string()),
    }

    // Check if HEAD exists and is valid
    match repo.head() {
        Ok(head) => {
            if head.target().is_none() {
                warnings.push("HEAD has no target (empty repository)".to_string());
            }
        }
        Err(_) => issues.push("HEAD reference is missing or corrupted".to_string()),
    }

    // Check for index corruption
    match repo.index() {
        Ok(_) => {}
        Err(_) => issues.push("Index file is corrupted".to_string()),
    }

    // Check for stale lock files
    let git_dir = repo.path();
    if git_dir.join("index.lock").exists() {
        warnings.push("Stale index.lock file found".to_string());
    }
    if git_dir.join("refs").join("heads").join("*.lock").exists() {
        warnings.push("Stale ref lock files found".to_string());
    }

    let is_healthy = issues.is_empty();

    info!(
        "is_repository_healthy: healthy={} issues={} warnings={} in {}ms",
        is_healthy,
        issues.len(),
        warnings.len(),
        start.elapsed().as_millis()
    );

    Ok(RepositoryHealth {
        is_healthy,
        issues,
        warnings,
    })
}

pub fn repair_repository_impl(repo_path: &str) -> Result<bool, GitError> {
    info!("repair_repository: path={}", repo_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("repair_repository"))?;
    let git_dir = repo.path();

    let mut repairs_made = false;

    // Remove stale lock files
    let lock_files = [
        git_dir.join("index.lock"),
        git_dir.join("HEAD.lock"),
        git_dir.join("config.lock"),
    ];

    for lock_file in &lock_files {
        if lock_file.exists() {
            fs::remove_file(lock_file).map_err(|e| GitError::IoError {
                operation: "remove_lock_file".to_string(),
                error: e.to_string(),
            })?;
            repairs_made = true;
        }
    }

    // Attempt to rebuild index if corrupted
    if repo.index().is_err() {
        let head = repo
            .head()
            .map_err(|e| GitError::from(e).with_operation("get_head"))?;
        if let Ok(commit) = head.peel_to_commit() {
            let _tree = commit
                .tree()
                .map_err(|e| GitError::from(e).with_operation("get_tree"))?;
            repo.reset(commit.as_object(), git2::ResetType::Hard, None)
                .map_err(|e| GitError::from(e).with_operation("reset"))?;
            repairs_made = true;
        }
    }

    info!(
        "repair_repository: repairs_made={} in {}ms",
        repairs_made,
        start.elapsed().as_millis()
    );
    Ok(repairs_made)
}

// Repository configuration
pub fn configure_repository_impl(repo_path: &str, config: &GitConfig) -> Result<bool, GitError> {
    info!("configure_repository: path={}", repo_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("configure_repository"))?;
    let mut repo_config = repo
        .config()
        .map_err(|e| GitError::from(e).with_operation("get_config"))?;

    if let Some(ref user_name) = config.user_name {
        repo_config
            .set_str("user.name", user_name)
            .map_err(|e| GitError::from(e).with_operation("set_user_name"))?;
    }

    if let Some(ref user_email) = config.user_email {
        repo_config
            .set_str("user.email", user_email)
            .map_err(|e| GitError::from(e).with_operation("set_user_email"))?;
    }

    if let Some(ref autocrlf) = config.core_autocrlf {
        repo_config
            .set_str("core.autocrlf", autocrlf)
            .map_err(|e| GitError::from(e).with_operation("set_autocrlf"))?;
    }

    if let Some(ref safecrlf) = config.core_safecrlf {
        repo_config
            .set_str("core.safecrlf", safecrlf)
            .map_err(|e| GitError::from(e).with_operation("set_safecrlf"))?;
    }

    info!(
        "configure_repository: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

/// Internal helper to read config values
///
/// Reads a configuration value from the repository config, with optional fallback
/// to global config. This function is NOT exposed via NAPI - it's for internal
/// use only by other operations (e.g., read_user_signature).
///
/// # Arguments
/// * `repo_path` - Path to the repository
/// * `key` - Config key to read (e.g., "user.name", "user.email")
/// * `fallback_global` - If true, falls back to global config when repo config missing
///
/// # Returns
/// * `Ok(Some(value))` - Config value found
/// * `Ok(None)` - Config value not found in any checked location
/// * `Err(GitError)` - Error opening repository or reading config
pub fn get_config_impl(
    repo_path: &str,
    key: &str,
    fallback_global: bool,
) -> Result<Option<String>, GitError> {
    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("get_config"))?;

    // Try repository-local config first (only .git/config, not global)
    // We need to open the config and then get the local level explicitly
    let config = repo
        .config()
        .map_err(|e| GitError::from(e).with_operation("get_config"))?;

    // Open the local-only config (just .git/config)
    let local_config = config.open_level(git2::ConfigLevel::Local);

    // Try to read from local config
    match local_config.and_then(|cfg| cfg.get_string(key)) {
        Ok(value) => return Ok(Some(value)),
        Err(_) => {
            // Not found in local config, continue to global fallback
        }
    }

    // Fall back to global config if requested
    if fallback_global {
        // Check if GIT_CONFIG_GLOBAL is set (used in tests for isolation)
        let global_result = if let Ok(config_path) = std::env::var("GIT_CONFIG_GLOBAL") {
            // Explicitly open the config file pointed to by GIT_CONFIG_GLOBAL
            git2::Config::open(std::path::Path::new(&config_path))
                .and_then(|config| config.get_string(key))
        } else {
            // Use default global config search
            git2::Config::open_default().and_then(|config| config.get_string(key))
        };

        match global_result {
            Ok(value) => Ok(Some(value)),
            Err(_) => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// Set a single repository-local configuration value
///
/// Sets a configuration key to a specific value in the repository's local
/// config (.git/config). Does NOT affect global or system config.
///
/// # Arguments
/// * `repo_path` - Path to the repository
/// * `key` - Config key to set (e.g., "user.name", "user.email")
/// * `value` - Value to set
///
/// # Returns
/// * `Ok(())` - Config value set successfully
/// * `Err(GitError)` - Error opening repository or setting config
pub fn set_config_impl(repo_path: &str, key: &str, value: &str) -> Result<(), GitError> {
    info!("set_config: key={}, value={}", key, value);
    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("set_config"))?;

    let mut config = repo
        .config()
        .map_err(|e| GitError::from(e).with_operation("set_config"))?;

    config
        .set_str(key, value)
        .map_err(|e| GitError::from(e).with_operation("set_config"))?;

    Ok(())
}

/// Remove a repository-local configuration value
///
/// Removes a configuration key from the repository's local config (.git/config).
/// Does NOT affect global or system config. If the key doesn't exist, this is a no-op.
///
/// # Arguments
/// * `repo_path` - Path to the repository
/// * `key` - Config key to remove (e.g., "user.name", "user.email")
///
/// # Returns
/// * `Ok(())` - Config value removed successfully (or didn't exist)
/// * `Err(GitError)` - Error opening repository or removing config
pub fn unset_config_impl(repo_path: &str, key: &str) -> Result<(), GitError> {
    info!("unset_config: key={}", key);
    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("unset_config"))?;

    let mut config = repo
        .config()
        .map_err(|e| GitError::from(e).with_operation("unset_config"))?;

    // remove_multivar with None removes all values for the key
    // If the key doesn't exist, this returns an error, but we treat that as success
    match config.remove(key) {
        Ok(_) => Ok(()),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(()), // Key didn't exist - that's fine
        Err(e) => Err(GitError::from(e).with_operation("unset_config")),
    }
}

// File management
pub fn create_gitignore_impl(repo_path: &str, patterns: &[String]) -> Result<bool, GitError> {
    info!("create_gitignore: {} patterns", patterns.len());
    let start = std::time::Instant::now();

    let gitignore_path = PathBuf::from(repo_path).join(".gitignore");

    let content = patterns.join("\n");
    fs::write(&gitignore_path, content).map_err(|e| GitError::IoError {
        operation: "write_gitignore".to_string(),
        error: e.to_string(),
    })?;

    info!(
        "create_gitignore: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

pub fn create_gitattributes_impl(repo_path: &str, rules: &[String]) -> Result<bool, GitError> {
    info!("create_gitattributes: {} rules", rules.len());
    let start = std::time::Instant::now();

    let gitattributes_path = PathBuf::from(repo_path).join(".gitattributes");

    let content = rules.join("\n");
    fs::write(&gitattributes_path, content).map_err(|e| GitError::IoError {
        operation: "write_gitattributes".to_string(),
        error: e.to_string(),
    })?;

    info!(
        "create_gitattributes: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(true)
}

// Repository information
pub fn get_repository_info_impl(repo_path: &str) -> Result<RepositoryInfo, GitError> {
    info!("get_repository_info: path={}", repo_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_repository_info"))?;

    let is_bare = repo.is_bare();

    let head_commit = match repo.head() {
        Ok(head) => head.target().map(|oid| oid.to_string()),
        Err(_) => None,
    };

    // Count branches
    let branch_count = repo
        .branches(None)
        .map_err(|e| GitError::from(e).with_operation("get_branches"))?
        .count() as i32;

    // Count commits (approximate)
    let commit_count = match repo.head() {
        Ok(head) => {
            if let Ok(commit) = head.peel_to_commit() {
                let mut revwalk = repo
                    .revwalk()
                    .map_err(|e| GitError::from(e).with_operation("create_revwalk"))?;
                revwalk
                    .push(commit.id())
                    .map_err(|e| GitError::from(e).with_operation("push_commit"))?;
                revwalk.count() as i32
            } else {
                0
            }
        }
        Err(_) => 0,
    };

    // Check for uncommitted changes.
    //
    // These options are not decoration. Passing `None` here takes libgit2's
    // defaults, which include ignored files — so a repository whose working
    // tree holds nothing but an ignored directory reported uncommitted
    // changes, permanently and with no way for the user to clear it. Every
    // other status call in this crate says `include_ignored(false)`; this one
    // was the only one that didn't.
    //
    // The options match `get_status_impl` exactly, so this field and
    // `GitStatus::is_clean` answer the same question the same way. They are
    // shown side by side in the UI, and disagreeing is worse than either
    // answer alone.
    let has_uncommitted_changes = if !is_bare {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.include_ignored(false);
        opts.recurse_untracked_dirs(true);

        let statuses = repo
            .statuses(Some(&mut opts))
            .map_err(|e| GitError::from(e).with_operation("get_status"))?;
        !statuses.is_empty()
    } else {
        false
    };

    // Get remote URLs
    let mut remote_urls = Vec::new();
    if let Ok(remotes) = repo.remotes() {
        for remote_name in remotes.iter().filter_map(|n| n.ok()) {
            if let Some(name) = remote_name
                && let Ok(remote) = repo.find_remote(name)
                && let Ok(url) = remote.url()
            {
                remote_urls.push(url.to_string());
            }
        }
    }

    let result = RepositoryInfo {
        path: repo_path.to_string(),
        is_bare,
        head_commit,
        branch_count,
        commit_count,
        has_uncommitted_changes,
        remote_urls,
    };

    info!(
        "get_repository_info: success in {}ms",
        start.elapsed().as_millis()
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    fn setup_test_repo() -> (tempfile::TempDir, PathBuf) {
        let temp_dir =
            tempfile::TempDir::new_in(std::env::temp_dir()).expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize a git repository
        Repository::init(&repo_path).expect("Failed to initialize test repository");

        (temp_dir, repo_path)
    }

    fn setup_isolated_global_config() -> tempfile::TempDir {
        use std::fs;

        // Create a temporary directory for isolated global config
        let config_dir = tempfile::TempDir::new_in(std::env::temp_dir())
            .expect("Failed to create temp config dir");
        let config_file = config_dir.path().join("gitconfig");

        // Create an empty config file so git2 can lock and write to it
        fs::write(&config_file, "").expect("Failed to create config file");

        // Point GIT_CONFIG_GLOBAL to our temp file
        // SAFETY: This is only used in single-threaded tests with proper cleanup
        unsafe {
            env::set_var("GIT_CONFIG_GLOBAL", config_file.as_os_str());
        }

        config_dir
    }

    fn cleanup_global_config() {
        // SAFETY: This is only used in single-threaded tests to clean up test state
        unsafe {
            env::remove_var("GIT_CONFIG_GLOBAL");
        }
    }

    #[test]
    fn test_get_config_existing_value() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = Repository::open(&repo_path).expect("Failed to open repository");

        // Set a config value in the repository
        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("test.key", "test_value")
            .expect("Failed to set config");

        // Test reading the config value
        let result = get_config_impl(repo_path.to_str().unwrap(), "test.key", false);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("test_value".to_string()));
    }

    #[test]
    fn test_get_config_missing_returns_none() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Test reading a non-existent config value without fallback
        let result = get_config_impl(repo_path.to_str().unwrap(), "nonexistent.key", false);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    #[cfg_attr(not(target_env = "msvc"), serial_test::serial)]
    fn test_get_config_global_fallback() {
        // Set up isolated global config to avoid polluting user's ~/.gitconfig
        let config_dir = setup_isolated_global_config();
        let (_temp_dir, repo_path) = setup_test_repo();

        // Get the config file path and open it explicitly
        let config_file = config_dir.path().join("gitconfig");
        let mut global_config =
            git2::Config::open(&config_file).expect("Failed to open global config");
        global_config
            .set_str("test.globalkey", "global_value")
            .expect("Failed to set global config");

        // Test reading from global config when not in repo config
        let result = get_config_impl(repo_path.to_str().unwrap(), "test.globalkey", true);

        // Clean up
        cleanup_global_config();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("global_value".to_string()));
    }

    #[test]
    #[cfg_attr(not(target_env = "msvc"), serial_test::serial)]
    fn test_get_config_repo_overrides_global() {
        // Set up isolated global config to avoid polluting user's ~/.gitconfig
        let config_dir = setup_isolated_global_config();
        let (_temp_dir, repo_path) = setup_test_repo();
        let repo = Repository::open(&repo_path).expect("Failed to open repository");

        // Get the config file path and open it explicitly
        let config_file = config_dir.path().join("gitconfig");
        let mut global_config =
            git2::Config::open(&config_file).expect("Failed to open global config");
        global_config
            .set_str("test.priority", "global_value")
            .expect("Failed to set global config");

        let mut config = repo.config().expect("Failed to get config");
        config
            .set_str("test.priority", "repo_value")
            .expect("Failed to set config");

        // Test that repo config takes priority
        let result = get_config_impl(repo_path.to_str().unwrap(), "test.priority", true);

        // Clean up
        cleanup_global_config();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("repo_value".to_string()));
    }

    #[test]
    fn test_get_config_invalid_repo_path() {
        let result = get_config_impl("/nonexistent/path/to/repo", "test.key", false);

        assert!(result.is_err());
        // Should return GitError (type doesn't matter as long as it fails)
    }
}
