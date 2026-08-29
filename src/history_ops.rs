use crate::errors::GitError;
use crate::types::{
    CommitDiff, CommitHistory, CommitInfo, DeletedFileEntry, FileAtCommit, FileDiff, TreeEntry,
};
use crate::utils::normalize_git_path;
use git2::{DiffFindOptions, DiffOptions, Repository};
use log::info;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

// Per-repo cache for deleted files, keyed by repo path — valid if HEAD matches
// OR the entry is < 30s old (time-based validity avoids thrashing during rapid
// auto-commits). Keying by path stops multiple open repositories from evicting
// each other's entries (the old single-slot cache thrashed across repos). BTreeMap
// is used because its `new()` is const, so the static needs no lazy init.
static DELETED_FILES_CACHE: Mutex<BTreeMap<String, DeletedFilesCache>> =
    Mutex::new(BTreeMap::new());

struct DeletedFilesCache {
    head_commit: String,
    files: Vec<DeletedFileEntry>,
    cached_at: Instant,
}

/// Get commit history for a specific file, following renames (like `git log --follow`).
///
/// This implementation tracks the file through rename operations by:
/// 1. Walking commits from HEAD backwards
/// 2. For each commit, checking if the file at the current tracked path was modified
/// 3. Using rename detection to find if the file was renamed in this commit
/// 4. If renamed, updating the tracked path to follow the file's previous name
pub fn get_file_history_impl(
    repo_path: &str,
    file_path: &str,
    limit: Option<usize>,
) -> Result<CommitHistory, GitError> {
    info!(
        "get_file_history: path={} limit={:?} (with --follow)",
        file_path, limit
    );
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_file_history"))?;

    let mut revwalk = repo
        .revwalk()
        .map_err(|e| GitError::from(e).with_operation("create_revwalk"))?;
    revwalk
        .set_sorting(git2::Sort::TIME)
        .map_err(|e| GitError::from(e).with_operation("set_sorting"))?;

    // Gracefully handle unborn HEAD (new repo with no commits)
    if revwalk.push_head().is_err() {
        return Ok(CommitHistory {
            commits: Vec::new(),
            total_count: 0,
            has_more: false,
        });
    }

    let limit = limit.unwrap_or(20);

    // Track the current path - this will change as we follow renames backwards
    let mut current_path = file_path.to_string();

    let mut commits = Vec::new();
    let mut scanned = 0;
    let max_scan = 500; // Hard cap to prevent runaway on large repos

    for oid in revwalk {
        if commits.len() >= limit || scanned >= max_scan {
            break;
        }
        scanned += 1;

        let oid = oid.map_err(|e| GitError::from(e).with_operation("walk_commits"))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| GitError::from(e).with_operation("find_commit"))?;

        // Check if this commit touched our file (at the current tracked path)
        // and detect any renames
        let (file_touched, old_path, insertions, deletions) =
            check_file_in_commit_with_rename(&repo, &commit, &current_path)?;

        if file_touched {
            commits.push(CommitInfo {
                hash: commit.id().to_string(),
                short_hash: commit.id().to_string()[..7.min(commit.id().to_string().len())]
                    .to_string(),
                message: commit.message().unwrap_or("").to_string(),
                author_name: commit.author().name().unwrap_or("").to_string(),
                author_email: commit.author().email().unwrap_or("").to_string(),
                timestamp: commit.time().seconds().to_string(),
                parent_hashes: commit.parent_ids().map(|id| id.to_string()).collect(),
                file_changes: 1,
                insertions,
                deletions,
            });
        }

        // If the file was renamed in this commit, follow the old path for earlier commits
        if let Some(old) = old_path {
            info!(
                "get_file_history: detected rename {} -> {}",
                old, current_path
            );
            current_path = old;
        }
    }

    let has_more = scanned >= max_scan && commits.len() >= limit;
    let total_count = commits.len() as i32;

    info!(
        "get_file_history: found {} commits (scanned {}) in {}ms",
        total_count,
        scanned,
        start.elapsed().as_millis()
    );

    Ok(CommitHistory {
        commits,
        total_count,
        has_more,
    })
}

/// Check if a file was touched in a commit and detect renames.
/// Returns (was_touched, old_path_if_renamed, insertions, deletions)
fn check_file_in_commit_with_rename(
    repo: &Repository,
    commit: &git2::Commit,
    file_path: &str,
) -> Result<(bool, Option<String>, i32, i32), GitError> {
    let file_path_obj = std::path::Path::new(file_path);
    let tree = commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    // Get parent tree (or None for first commit)
    let parent_tree = if commit.parent_count() > 0 {
        commit.parent(0).ok().and_then(|p| p.tree().ok())
    } else {
        None
    };

    // Create diff between parent and this commit
    let mut diff_opts = DiffOptions::new();

    let mut diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut diff_opts))
        .map_err(|e| GitError::from(e).with_operation("create_diff"))?;

    // Enable rename detection
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    find_opts.rename_threshold(50); // 50% similarity threshold (git default)

    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::from(e).with_operation("find_similar"))?;

    let mut file_touched = false;
    let mut old_path: Option<String> = None;
    let mut insertions = 0i32;
    let mut deletions = 0i32;

    // Iterate through deltas to find our file
    for delta in diff.deltas() {
        let new_file_path = delta
            .new_file()
            .path()
            .map(|p| p.to_string_lossy().to_string());
        let old_file_path = delta
            .old_file()
            .path()
            .map(|p| p.to_string_lossy().to_string());

        // Check if this delta involves our file
        let involves_our_file = new_file_path.as_ref().is_some_and(|p| p == file_path)
            || old_file_path.as_ref().is_some_and(|p| p == file_path);

        if !involves_our_file {
            continue;
        }

        file_touched = true;

        // Check for rename: new path matches our file, old path is different
        if let (Some(new_path), Some(old_path_str)) = (&new_file_path, &old_file_path)
            && new_path == file_path
            && old_path_str != file_path
        {
            // This is a rename TO our current path
            if delta.status() == git2::Delta::Renamed {
                old_path = Some(old_path_str.clone());
            }
        }
    }

    // If file was touched, get diff stats
    if file_touched {
        // Re-create diff with pathspec to get accurate stats for this file
        let mut stats_diff_opts = DiffOptions::new();
        stats_diff_opts.pathspec(file_path_obj);

        // Also check old path if there was a rename
        if let Some(ref old) = old_path {
            stats_diff_opts.pathspec(std::path::Path::new(old));
        }

        let stats_diff = repo
            .diff_tree_to_tree(
                parent_tree.as_ref(),
                Some(&tree),
                Some(&mut stats_diff_opts),
            )
            .map_err(|e| GitError::from(e).with_operation("create_stats_diff"))?;

        stats_diff
            .foreach(
                &mut |_delta, _progress| true,
                None,
                None,
                Some(&mut |_delta, _hunk, line| {
                    match line.origin() {
                        '+' => insertions += 1,
                        '-' => deletions += 1,
                        _ => {}
                    }
                    true
                }),
            )
            .map_err(|e| GitError::from(e).with_operation("foreach_lines"))?;
    }

    Ok((file_touched, old_path, insertions, deletions))
}

/// Build the `CommitInfo` this crate hands back for a commit, without diff
/// statistics — `file_changes`, `insertions` and `deletions` are left at zero
/// because computing them costs a diff per commit and most callers do not
/// look at them.
///
/// `get_file_history_impl` deliberately does not use this: it already has the
/// per-file counts in hand and reports a 7-character short hash for
/// historical reasons. Everything else that returns a `CommitInfo` should
/// come through here rather than restating the eleven fields.
pub(crate) fn commit_info_from(commit: &git2::Commit) -> CommitInfo {
    let hash = commit.id().to_string();
    CommitInfo {
        short_hash: hash[..8.min(hash.len())].to_string(),
        hash,
        message: commit.message().unwrap_or("").to_string(),
        author_name: commit.author().name().unwrap_or("").to_string(),
        author_email: commit.author().email().unwrap_or("").to_string(),
        timestamp: commit.time().seconds().to_string(),
        parent_hashes: commit.parent_ids().map(|id| id.to_string()).collect(),
        file_changes: 0,
        insertions: 0,
        deletions: 0,
    }
}

pub fn get_commit_history_impl(
    repo_path: &str,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<CommitHistory, GitError> {
    info!("get_commit_history: limit={:?} offset={:?}", limit, offset);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_commit_history"))?;

    let mut revwalk = repo
        .revwalk()
        .map_err(|e| GitError::from(e).with_operation("create_revwalk"))?;
    revwalk
        .set_sorting(git2::Sort::TIME)
        .map_err(|e| GitError::from(e).with_operation("set_sorting"))?;
    // Gracefully handle unborn HEAD (new repo with no commits)
    if revwalk.push_head().is_err() {
        return Ok(CommitHistory {
            commits: Vec::new(),
            total_count: 0,
            has_more: false,
        });
    }

    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let mut commits = Vec::new();
    let mut total_processed = 0;
    let mut collected = 0;
    let mut has_more = false;

    for oid in revwalk {
        let oid = oid.map_err(|e| GitError::from(e).with_operation("walk_commits"))?;

        if total_processed < offset {
            total_processed += 1;
            continue;
        }

        if collected >= limit {
            has_more = true;
            break;
        }

        let commit = repo
            .find_commit(oid)
            .map_err(|e| GitError::from(e).with_operation("find_commit"))?;

        commits.push(commit_info_from(&commit));
        collected += 1;
        total_processed += 1;
    }

    let result = CommitHistory {
        commits,
        total_count: total_processed as i32,
        has_more,
    };

    info!(
        "get_commit_history: found {} commits in {}ms",
        total_processed,
        start.elapsed().as_millis()
    );
    Ok(result)
}

/// Get file content at a specific commit.
///
/// Takes a raw commit hash and nothing else; `resolve_ref_impl` is the
/// intended way to obtain one from `"HEAD"`, a branch or a tag.
pub fn get_file_at_commit_impl(
    repo_path: &str,
    file_path: &str,
    commit_hash: &str,
) -> Result<FileAtCommit, GitError> {
    info!(
        "get_file_at_commit: path={} commit={}",
        file_path, commit_hash
    );
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_file_at_commit"))?;

    let oid = git2::Oid::from_str(commit_hash).map_err(|_| GitError::InvalidCommitHash {
        hash: commit_hash.to_string(),
    })?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;
    let tree = commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    let relative_path = std::path::Path::new(file_path);

    let result = match tree.get_path(relative_path) {
        Ok(tree_entry) => {
            let blob = repo
                .find_blob(tree_entry.id())
                .map_err(|e| GitError::from(e).with_operation("find_blob"))?;

            let content = if blob.is_binary() {
                "[Binary file]".to_string()
            } else {
                String::from_utf8_lossy(blob.content()).to_string()
            };

            FileAtCommit {
                path: file_path.to_string(),
                content,
                exists: true,
                commit_hash: commit_hash.to_string(),
            }
        }
        Err(_) => FileAtCommit {
            path: file_path.to_string(),
            content: String::new(),
            exists: false,
            commit_hash: commit_hash.to_string(),
        },
    };

    info!(
        "get_file_at_commit: exists={} in {}ms",
        result.exists,
        start.elapsed().as_millis()
    );
    Ok(result)
}

/// List the paths present at a commit.
///
/// The paired half of `get_file_at_commit_impl`. That one answers "what is in
/// this file at this commit"; this one answers "which files are there at
/// all", and calling both with the same hash gives a reader a
/// snapshot-consistent view of a whole repository with no lock held and no
/// writer blocked.
///
/// Ref resolution is deliberately identical to `get_file_at_commit_impl`: a
/// raw commit hash and nothing else. Not a branch, not a tag, not `"HEAD"`.
/// A caller wanting the repository as of a tag resolves it with
/// `resolve_ref_impl` first, which is the same thing it must already do to
/// call `get_file_at_commit_impl`. Accepting anything wider here would break
/// the one property the pair depends on, which is that both calls name the
/// same object.
///
/// `path_prefix` is a **literal string prefix** on the repository-relative
/// path, not a directory match. `Some("src")` therefore matches `src/main.rs`
/// and would also match a sibling file named `srcfile.txt`; a caller
/// filtering to a directory passes the trailing slash. A prefix matching
/// nothing yields an empty listing rather than an error, because "no files
/// under this directory at this commit" is an answer and not a failure.
///
/// Read-only. Takes no lock.
pub fn get_tree_at_commit_impl(
    repo_path: &str,
    commit_hash: &str,
    path_prefix: Option<&str>,
) -> Result<Vec<TreeEntry>, GitError> {
    info!(
        "get_tree_at_commit: commit={} prefix={:?}",
        commit_hash, path_prefix
    );
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_tree_at_commit"))?;

    let oid = git2::Oid::from_str(commit_hash).map_err(|_| GitError::InvalidCommitHash {
        hash: commit_hash.to_string(),
    })?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;
    let tree = commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    let prefix = path_prefix.unwrap_or("");
    let mut entries: Vec<TreeEntry> = Vec::new();

    // A blob lookup inside the callback can fail, and the callback has no way
    // to carry a Result out. Recording the first failure and checking it after
    // the walk keeps the error rather than swallowing it.
    let mut failure: Option<GitError> = None;

    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        // libgit2 gives the directory with its trailing slash already on it,
        // and "" at the top level, so this concatenation is the full path.
        let name = match entry.name() {
            Some(name) => name,
            // A name that is not valid UTF-8 cannot cross the N-API boundary.
            // Skipping the entry matches how every other operation here
            // handles a non-UTF-8 value, rather than failing the listing
            // around it.
            None => return git2::TreeWalkResult::Ok,
        };
        let path = format!("{}{}", root, name);

        let kind = match entry.filemode() {
            // A directory is implied by the paths of the files inside it.
            // Returning Ok rather than Skip is what descends into it.
            0o040000 => return git2::TreeWalkResult::Ok,
            0o120000 => "symlink",
            0o160000 => "submodule",
            _ => "file",
        };

        if !path.starts_with(prefix) {
            return git2::TreeWalkResult::Ok;
        }

        // A submodule's entry id is a commit in the submodule's own object
        // database, which this repository does not have. There is no blob to
        // size.
        let size = if kind == "submodule" {
            0
        } else {
            match repo.find_blob(entry.id()) {
                Ok(blob) => blob.size() as i64,
                Err(e) => {
                    failure = Some(GitError::from(e).with_operation("find_blob"));
                    return git2::TreeWalkResult::Abort;
                }
            }
        };

        entries.push(TreeEntry {
            path,
            kind: kind.to_string(),
            size,
            blob_hash: entry.id().to_string(),
        });
        git2::TreeWalkResult::Ok
    })
    .map_err(|e| GitError::from(e).with_operation("tree_walk"))?;

    if let Some(error) = failure {
        return Err(error);
    }

    // Sorted explicitly rather than relying on libgit2's walk order. Git sorts
    // tree entries as though every directory carried a trailing slash, which
    // happens to agree with a plain sort of the full paths, but the callers
    // this exists for zip the result against other listings and the guarantee
    // is worth stating in code rather than deriving.
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    info!(
        "get_tree_at_commit: {} entries in {}ms",
        entries.len(),
        start.elapsed().as_millis()
    );
    Ok(entries)
}

// Get deleted files from commit history (with caching)
pub fn get_deleted_files_impl(
    repo_path: &str,
    limit: Option<usize>,
) -> Result<Vec<DeletedFileEntry>, GitError> {
    info!("get_deleted_files: limit={:?}", limit);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_deleted_files"))?;

    // Get current HEAD commit for cache validation
    let head_commit = match repo.head().and_then(|h| h.peel_to_commit()) {
        Ok(c) => c.id().to_string(),
        Err(_) => return Ok(Vec::new()), // No commits yet
    };

    // Check this repo's cache entry — return if HEAD unchanged OR recent (< 30s).
    {
        let cache = DELETED_FILES_CACHE
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = cache.get(repo_path) {
            let cache_valid = cached.head_commit == head_commit
                || cached.cached_at.elapsed() < Duration::from_secs(30);
            if cache_valid {
                info!(
                    "get_deleted_files: cache hit in {}ms",
                    start.elapsed().as_millis()
                );
                return Ok(cached.files.clone());
            }
        }
    }

    // Cache miss - compute deleted files
    let mut revwalk = repo
        .revwalk()
        .map_err(|e| GitError::from(e).with_operation("create_revwalk"))?;
    revwalk
        .set_sorting(git2::Sort::TIME)
        .map_err(|e| GitError::from(e).with_operation("set_sorting"))?;
    // Gracefully handle unborn HEAD (new repo with no commits)
    if revwalk.push_head().is_err() {
        return Ok(Vec::new());
    }

    let head_tree = repo.head().and_then(|head| head.peel_to_tree()).ok();

    let commit_limit = limit.unwrap_or(20); // Default for reasonable coverage
    let max_deleted_files = 50; // Stop early once we have enough deleted files
    let mut deleted_files = Vec::new();
    let mut processed = 0;
    let mut seen_deleted = std::collections::HashSet::new();

    for oid in revwalk {
        // Stop if we've processed enough commits or found enough deleted files
        if processed >= commit_limit || deleted_files.len() >= max_deleted_files {
            break;
        }

        let oid = oid.map_err(|e| GitError::from(e).with_operation("walk_commits"))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| GitError::from(e).with_operation("find_commit"))?;

        if commit.parent_count() > 0 {
            let parent = commit
                .parent(0)
                .map_err(|e| GitError::from(e).with_operation("get_parent"))?;

            let diff = repo
                .diff_tree_to_tree(
                    Some(
                        &parent
                            .tree()
                            .map_err(|e| GitError::from(e).with_operation("get_parent_tree"))?,
                    ),
                    Some(
                        &commit
                            .tree()
                            .map_err(|e| GitError::from(e).with_operation("get_commit_tree"))?,
                    ),
                    None,
                )
                .map_err(|e| GitError::from(e).with_operation("create_diff"))?;

            // First pass: collect deleted paths and added filenames in this commit
            // We need both to detect moves (delete + add of same filename = move)
            let mut commit_deleted: Vec<String> = Vec::new();
            let mut commit_added_filenames: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            diff.foreach(
                &mut |delta, _progress| {
                    match delta.status() {
                        git2::Delta::Deleted => {
                            if let Some(path) = delta.old_file().path() {
                                commit_deleted.push(normalize_git_path(&path.to_string_lossy()));
                            }
                        }
                        git2::Delta::Added => {
                            if let Some(path) = delta.new_file().path() {
                                // Extract just the filename for move detection
                                if let Some(filename) = path.file_name() {
                                    commit_added_filenames
                                        .insert(filename.to_string_lossy().to_string());
                                }
                            }
                        }
                        git2::Delta::Renamed => {
                            // Git detected rename - the old path should NOT be considered deleted
                        }
                        _ => {}
                    }
                    true
                },
                None,
                None,
                None,
            )
            .map_err(|e| GitError::from(e).with_operation("foreach_diff"))?;

            // Second pass: process deletions, filtering out moves
            for path_str in commit_deleted {
                if seen_deleted.contains(&path_str) {
                    continue; // Already processed a more recent deletion
                }

                // Extract filename to check if it was added elsewhere (move detection)
                let filename = std::path::Path::new(&path_str)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string());

                // If same filename was added in this commit, it's likely a move, not a deletion
                let is_likely_move = filename
                    .as_ref()
                    .map(|f| commit_added_filenames.contains(f))
                    .unwrap_or(false);

                if is_likely_move {
                    continue; // Skip - this is a move, not a real deletion
                }

                // Check if the file exists in the current HEAD (restored at same path)
                let is_restored_in_head = if let Some(tree) = &head_tree {
                    tree.get_path(std::path::Path::new(&path_str)).is_ok()
                } else {
                    false
                };

                if !is_restored_in_head {
                    deleted_files.push(DeletedFileEntry {
                        path: path_str.clone(),
                        deleted_at: commit.time().seconds(),
                        last_commit: parent.id().to_string(),
                        last_commit_message: parent.message().unwrap_or("").to_string(),
                    });
                    seen_deleted.insert(path_str);
                }
            }
        }
        processed += 1;
    }

    info!(
        "get_deleted_files: found {} files, processed {} commits in {}ms",
        deleted_files.len(),
        processed,
        start.elapsed().as_millis()
    );

    // Store this repo's entry (keyed by path, so other open repos are untouched).
    {
        let mut cache = DELETED_FILES_CACHE
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        cache.insert(
            repo_path.to_string(),
            DeletedFilesCache {
                head_commit,
                files: deleted_files.clone(),
                cached_at: Instant::now(),
            },
        );
    }

    Ok(deleted_files)
}

// Get file diff
pub fn get_file_diff_impl(repo_path: &str, file_path: &str) -> Result<FileDiff, GitError> {
    info!("get_file_diff: path={}", file_path);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_file_diff"))?;

    let head = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;
    let head_commit = head
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(file_path);

    let mut diff = repo
        .diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut diff_opts))
        .map_err(|e| GitError::from(e).with_operation("create_diff"))?;

    // Enable comprehensive rename detection
    let mut find_opts = DiffFindOptions::new();
    find_opts
        .renames(true)
        .copies(true)
        .rename_threshold(50)
        .copy_threshold(50)
        .remove_unmodified(true);

    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::from(e).with_operation("find_similar"))?;

    let mut file_diff = FileDiff {
        file_path: file_path.to_string(),
        old_path: None,
        status: "modified".to_string(),
        hunks: Vec::new(),
        additions: 0,
        deletions: 0,
        is_binary: false,
    };

    // First pass: collect file metadata
    diff.foreach(
        &mut |delta, _progress| {
            file_diff.status = match delta.status() {
                git2::Delta::Added => "added".to_string(),
                git2::Delta::Deleted => "deleted".to_string(),
                git2::Delta::Modified => "modified".to_string(),
                git2::Delta::Renamed => "renamed".to_string(),
                _ => "modified".to_string(),
            };

            if let Some(old_path) = delta.old_file().path()
                && old_path != std::path::Path::new(file_path)
            {
                file_diff.old_path = Some(old_path.to_string_lossy().to_string());
            }

            file_diff.is_binary = delta.new_file().is_binary();
            true
        },
        None,
        None,
        None,
    )?;

    // Second pass: collect line changes (just counts, not detailed hunks for now)
    diff.foreach(
        &mut |_delta, _progress| true,
        None,
        None,
        Some(&mut |_delta, _hunk, line| {
            let line_type = match line.origin() {
                '+' => "added",
                '-' => "removed",
                _ => "context",
            };

            if line_type == "added" {
                file_diff.additions += 1;
            } else if line_type == "removed" {
                file_diff.deletions += 1;
            }

            true
        }),
    )
    .map_err(|e| GitError::from(e).with_operation("foreach_diff"))?;

    info!(
        "get_file_diff: status={} in {}ms",
        file_diff.status,
        start.elapsed().as_millis()
    );
    Ok(file_diff)
}

// Get unified diff string for a file
// Returns simple unified diff output for display in UI
pub fn get_diff_impl(repo_path: &str, file_path: &str) -> Result<String, GitError> {
    info!("get_diff: path={}", file_path);
    let start = std::time::Instant::now();

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("get_diff"))?;

    // Check if file exists in working directory
    let full_path = std::path::Path::new(repo_path).join(file_path);
    let file_exists = full_path.exists();

    // Get HEAD tree
    let head = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;
    let head_commit = head
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::from(e).with_operation("get_tree"))?;

    // Check if file exists in HEAD
    let file_in_head = head_tree.get_path(std::path::Path::new(file_path)).is_ok();

    // Handle new files (not in HEAD)
    if !file_in_head && file_exists {
        // For new files, show entire content as additions
        let content = std::fs::read_to_string(&full_path).map_err(|e| GitError::IoError {
            operation: "read_file".to_string(),
            error: e.to_string(),
        })?;

        // Check if binary by looking for null bytes
        if content.contains('\0') {
            info!(
                "get_diff: binary file detected in {}ms",
                start.elapsed().as_millis()
            );
            return Ok("Binary file\n".to_string());
        }

        if content.is_empty() {
            info!("get_diff: empty file in {}ms", start.elapsed().as_millis());
            return Ok("Empty file\n".to_string());
        }

        // Format as unified diff with all lines as additions
        let mut diff_output = format!("diff --git a/{} b/{}\n", file_path, file_path);
        diff_output.push_str("new file\n");
        diff_output.push_str("--- /dev/null\n");
        diff_output.push_str(&format!("+++ b/{}\n", file_path));
        diff_output.push_str(&format!("@@ -0,0 +1,{} @@\n", content.lines().count()));

        for line in content.lines() {
            diff_output.push_str(&format!("+{}\n", line));
        }

        info!(
            "get_diff: new file processed in {}ms",
            start.elapsed().as_millis()
        );
        return Ok(diff_output);
    }

    // Handle modified files - use git diff
    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(file_path);

    let diff = repo
        .diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut diff_opts))
        .map_err(|e| GitError::from(e).with_operation("create_diff"))?;

    // Check if binary
    let mut is_binary = false;
    diff.foreach(
        &mut |delta, _progress| {
            is_binary = delta.new_file().is_binary();
            true
        },
        None,
        None,
        None,
    )
    .map_err(|e| GitError::from(e).with_operation("check_binary"))?;

    if is_binary {
        info!("get_diff: binary file in {}ms", start.elapsed().as_millis());
        return Ok("Binary file\n".to_string());
    }

    // Generate unified diff output
    let mut diff_output = Vec::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();

        match origin {
            '+' | '-' | ' ' => {
                diff_output.push(origin as u8);
                diff_output.extend_from_slice(line.content());
            }
            'H' | 'F' => {
                // Header or file header
                diff_output.extend_from_slice(line.content());
            }
            _ => {}
        }
        true
    })
    .map_err(|e| GitError::from(e).with_operation("print_diff"))?;

    let result = String::from_utf8(diff_output).map_err(|e| GitError::IoError {
        operation: "decode_utf8".to_string(),
        error: e.to_string(),
    })?;

    info!("get_diff: completed in {}ms", start.elapsed().as_millis());
    Ok(result)
}

// Get commit diff
pub fn get_commit_diff_impl(repo_path: &str, commit_hash: &str) -> Result<CommitDiff, GitError> {
    info!("get_commit_diff: commit={}", commit_hash);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_commit_diff"))?;

    let oid = git2::Oid::from_str(commit_hash).map_err(|_| GitError::InvalidCommitHash {
        hash: commit_hash.to_string(),
    })?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;

    let parent_hash = if commit.parent_count() > 0 {
        Some(
            commit
                .parent_id(0)
                .map_err(|e| GitError::from(e).with_operation("get_parent_id"))?
                .to_string(),
        )
    } else {
        None
    };

    let mut diff = if let Some(parent_id) = commit.parent_ids().next() {
        let parent_commit = repo
            .find_commit(parent_id)
            .map_err(|e| GitError::from(e).with_operation("find_parent_commit"))?;
        repo.diff_tree_to_tree(
            Some(
                &parent_commit
                    .tree()
                    .map_err(|e| GitError::from(e).with_operation("get_parent_tree"))?,
            ),
            Some(
                &commit
                    .tree()
                    .map_err(|e| GitError::from(e).with_operation("get_commit_tree"))?,
            ),
            None,
        )
        .map_err(|e| GitError::from(e).with_operation("create_diff"))?
    } else {
        // First commit - diff against empty tree
        repo.diff_tree_to_tree(
            None,
            Some(
                &commit
                    .tree()
                    .map_err(|e| GitError::from(e).with_operation("get_commit_tree"))?,
            ),
            None,
        )
        .map_err(|e| GitError::from(e).with_operation("create_diff"))?
    };

    // Enable comprehensive rename detection
    let mut find_opts = DiffFindOptions::new();
    find_opts
        .renames(true)
        .copies(true)
        .rename_threshold(50)
        .copy_threshold(50)
        .remove_unmodified(true);

    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::from(e).with_operation("find_similar"))?;

    let mut files = Vec::new();
    let mut total_additions = 0;
    let mut total_deletions = 0;
    let mut files_changed = 0;

    // First pass: collect file information
    diff.foreach(
        &mut |delta, _progress| {
            if let Some(new_path) = delta.new_file().path() {
                let file_diff = FileDiff {
                    file_path: new_path.to_string_lossy().to_string(),
                    old_path: delta
                        .old_file()
                        .path()
                        .map(|p| p.to_string_lossy().to_string()),
                    status: match delta.status() {
                        git2::Delta::Added => "added".to_string(),
                        git2::Delta::Deleted => "deleted".to_string(),
                        git2::Delta::Modified => "modified".to_string(),
                        git2::Delta::Renamed => "renamed".to_string(),
                        _ => "modified".to_string(),
                    },
                    hunks: Vec::new(),
                    additions: 0,
                    deletions: 0,
                    is_binary: delta.new_file().is_binary(),
                };

                files.push(file_diff);
                files_changed += 1;
            }

            true
        },
        None,
        None,
        None,
    )?;

    // Second pass: collect line change statistics
    diff.foreach(
        &mut |_delta, _progress| true,
        None,
        None,
        Some(&mut |_delta, _hunk, line| {
            match line.origin() {
                '+' => total_additions += 1,
                '-' => total_deletions += 1,
                _ => {}
            }
            true
        }),
    )
    .map_err(|e| GitError::from(e).with_operation("foreach_diff"))?;

    let result = CommitDiff {
        commit_hash: commit_hash.to_string(),
        parent_hash,
        files,
        total_additions,
        total_deletions,
        files_changed,
    };

    info!(
        "get_commit_diff: {} files changed in {}ms",
        files_changed,
        start.elapsed().as_millis()
    );
    Ok(result)
}

/// Resolve a symbolic ref to the commit hash it names, fully peeled.
///
/// `ref_name` is `"HEAD"`, a branch name, a tag name, or a full ref path such
/// as `refs/tags/v1.0.0`. A raw 40-character commit hash resolves to itself,
/// so a caller can accept either form without branching on which it got.
///
/// This is the preamble `get_file_at_commit_impl` and `get_tree_at_commit_impl`
/// make every consumer write. Those two deliberately take a raw hash and
/// nothing else, because the property they depend on is that both calls name
/// the same object — a symbolic ref could resolve differently between them if
/// a writer moved it in between. Resolving once here and passing the result to
/// every read in a snapshot keeps that property by construction.
///
/// Refs are tried before hashes: an exact ref path first, then the short-name
/// lookup that turns `main` into `refs/heads/main` and `v1.0.0` into
/// `refs/tags/v1.0.0`. So a branch improbably named as 40 hex characters wins
/// over the object of that name.
///
/// Only a full 40-character hash is accepted as a hash. An abbreviated one is
/// not resolved, because `Oid::from_str` zero-fills a short string into a
/// different, well-formed oid rather than rejecting it, and silently answering
/// about the wrong object is worse than saying no.
///
/// Anything else is an error naming the ref, never a null. `RefNotFound` for
/// no such ref, or a ref that peels to something other than a commit such as
/// a tag of a blob. `UnbornHead` for a symbolic ref whose target does not
/// exist yet — `HEAD` in a repository with no commits, or on a freshly
/// orphaned branch. The two are separate because a caller does different
/// things with them: one branch has no commits *yet*, the other names
/// something that was never going to resolve.
///
/// Not a revparse grammar. `HEAD~3` and `main@{yesterday}` are out of scope:
/// the need is naming a commit by a ref that exists, not navigating from one.
///
/// Read-only. Takes no lock.
pub fn resolve_ref_impl(repo_path: &str, ref_name: &str) -> Result<String, GitError> {
    info!("resolve_ref: ref={}", ref_name);
    let start = std::time::Instant::now();

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("resolve_ref"))?;

    let not_found = || GitError::RefNotFound {
        ref_name: ref_name.to_string(),
    };

    let object = match repo
        .find_reference(ref_name)
        .or_else(|_| repo.resolve_reference_from_short_name(ref_name))
    {
        Ok(reference) => match reference.peel(git2::ObjectType::Commit) {
            Ok(object) => object,
            // A symbolic ref whose target does not exist is unborn, not
            // absent: the branch has no commits yet. Told apart from a
            // missing ref because the two call for different things from a
            // caller — one is a state that ends, the other a name that was
            // never going to resolve.
            Err(_) if reference.symbolic_target().is_some() && reference.resolve().is_err() => {
                return Err(GitError::UnbornHead {
                    ref_name: ref_name.to_string(),
                });
            }
            Err(_) => return Err(not_found()),
        },
        Err(_) => {
            if ref_name.len() != 40 || !ref_name.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(not_found());
            }
            let oid = git2::Oid::from_str(ref_name).map_err(|_| not_found())?;
            repo.find_object(oid, None)
                .map_err(|_| not_found())?
                .peel(git2::ObjectType::Commit)
                .map_err(|_| not_found())?
        }
    };

    let commit_hash = object.id().to_string();
    info!(
        "resolve_ref: {} -> {} in {}ms",
        ref_name,
        commit_hash,
        start.elapsed().as_millis()
    );
    Ok(commit_hash)
}
