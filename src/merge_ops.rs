// native/src/merge_ops.rs
//
// Merging, done entirely in memory.
//
// The organising rule of this module is that a merge never leaves an
// in-progress state on disk. There is no `MERGE_HEAD`, no conflicted on-disk
// index, no file rewritten with conflict markers, and no detached HEAD at any
// point. A merge either completes as a commit or reports what stands in the
// way and writes nothing.
//
// That is why `Repository::merge` is not called here and must not be: it
// writes `MERGE_HEAD` and mutates the on-disk index, and a caller who then
// walks away leaves the repository mid-merge. `merge_commits` gives back a
// detached in-memory `Index` and touches nothing, so an abandoned resolution
// costs the writer nothing at all.

use crate::errors::GitError;
use crate::types::{
    CommitInfo, CommitOptions, ConflictedFile, MergeOptions, MergeOutcome, ResolvedFile,
};
use git2::{Commit, Index, IndexConflict, IndexEntry, IndexTime, Oid, Repository, Tree};
use log::info;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

// NAPI imports only when feature is enabled
#[cfg(feature = "napi-binding")]
use crate::GitService;
#[cfg(feature = "napi-binding")]
use napi::bindgen_prelude::*;

/// File mode used for a resolved page when neither side of the conflict has
/// an entry to copy a mode from — a plain non-executable file.
const DEFAULT_FILE_MODE: u32 = 0o100644;

// ===== PURE GIT IMPLEMENTATIONS (always available) =====

/// Merge `branch` into HEAD.
///
/// Four outcomes, and only one of them writes a merge commit:
///
/// - `"up-to-date"` — HEAD already contains everything on `branch`. Merging
///   something already contained is a no-op, not an error, so this is a
///   success with HEAD's own hash.
/// - `"fast-forwarded"` — delegated wholesale to `fast_forward_impl`, whose
///   refusal policy (and `liminal.checkoutStrategy` handling) is inherited
///   rather than restated.
/// - `"conflicted"` — the three sides of every contested path are reported
///   and **nothing is written**. The repository is byte-for-byte what it was.
/// - `"merged"` — a two-parent commit, HEAD first and `branch` second.
///
/// Before writing anything for a clean merge, every path the merge would
/// change is checked against the working tree; if the writer has unsaved
/// edits to one of them the whole merge is refused with
/// `UnstagedChangesWouldBeLost`, the same shape `checkoutBranch` returns.
/// Files outside the merge set are not consulted and are left alone.
pub fn merge_impl(
    repo_path: &str,
    branch: &str,
    user_name: Option<&str>,
    user_email: Option<&str>,
    options: Option<&MergeOptions>,
) -> std::result::Result<MergeOutcome, GitError> {
    info!("merge: branch={}", branch);
    let start = std::time::Instant::now();

    let committer = crate::utils::committer_halves(
        options.and_then(|options| options.committer_name.as_deref()),
        options.and_then(|options| options.committer_email.as_deref()),
    )?;
    let no_fast_forward = options.and_then(|options| options.no_fast_forward) == Some(true);

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("merge"))?;

    // Resolving first means an unknown branch is reported as BranchNotFound
    // before anything else is attempted.
    let their_ref = crate::branch_ops::resolve_branch_ref(&repo, branch)?;
    let their_commit = their_ref
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

    // Reuse the analysis rather than re-deriving it: the three kinds have to
    // agree with what `mergeAnalysis` told the caller a moment ago, and the
    // only way to guarantee that is to ask the same function.
    let analysis = crate::branch_ops::merge_analysis_impl(repo_path, branch)?;

    match analysis.kind.as_str() {
        "up-to-date" => {
            let head_commit = repo
                .head()
                .and_then(|head| head.peel_to_commit())
                .map_err(|e| GitError::from(e).with_operation("get_head_commit"))?;
            info!(
                "merge: up-to-date in {}ms — nothing written",
                start.elapsed().as_millis()
            );
            return Ok(MergeOutcome {
                kind: "up-to-date".to_string(),
                commit_hash: Some(head_commit.id().to_string()),
                conflicts: Vec::new(),
            });
        }
        // Falling through to the merge path is the whole of --no-ff: the
        // three-way merge of a branch that contains HEAD produces that
        // branch's tree with no conflicts, and finish_merge gives it two
        // parents and somewhere to record who approved it.
        "fast-forward" if !no_fast_forward => {
            let result = crate::branch_ops::fast_forward_impl(repo_path, branch)?;
            info!(
                "merge: fast-forwarded to {} in {}ms",
                result.commit_hash,
                start.elapsed().as_millis()
            );
            return Ok(MergeOutcome {
                kind: "fast-forwarded".to_string(),
                commit_hash: Some(result.commit_hash),
                conflicts: Vec::new(),
            });
        }
        _ => {}
    }

    let head_ref = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;
    let our_commit = head_ref
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

    let mut index = repo
        .merge_commits(&our_commit, &their_commit, None)
        .map_err(|e| GitError::from(e).with_operation("merge_commits"))?;

    if index.has_conflicts() {
        let conflicts = conflicted_files(&index)?;
        info!(
            "merge: {} conflicted path(s) in {}ms — nothing written",
            conflicts.len(),
            start.elapsed().as_millis()
        );
        return Ok(MergeOutcome {
            kind: "conflicted".to_string(),
            commit_hash: None,
            conflicts,
        });
    }

    let message = format!("Merge branch '{}'", branch);
    let commit_id = finish_merge(
        &repo,
        &mut index,
        &head_ref,
        &our_commit,
        &their_commit,
        &message,
        user_name,
        user_email,
        committer,
    )?;

    info!(
        "merge: created merge commit {} in {}ms",
        commit_id,
        start.elapsed().as_millis()
    );

    Ok(MergeOutcome {
        kind: "merged".to_string(),
        commit_hash: Some(commit_id.to_string()),
        conflicts: Vec::new(),
    })
}

/// Read a blob's content as text.
///
/// Refuses rather than converts when the bytes are not UTF-8. A novelist's
/// manuscript is the payload here, and `from_utf8_lossy` would hand back a
/// page with U+FFFD where a character used to be — damage the writer cannot
/// see and the app cannot undo. `BlobNotUtf8` names the blob and lets the
/// caller decide.
pub fn read_blob_impl(repo_path: &str, oid: &str) -> std::result::Result<String, GitError> {
    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("read_blob"))?;

    let parsed = Oid::from_str(oid).map_err(|_| GitError::InvalidCommitHash {
        hash: oid.to_string(),
    })?;

    // find_blob fails both for an object that is not there and for one that is
    // there but is a tree or a commit; neither is a blob the caller can read.
    let blob = repo.find_blob(parsed).map_err(|_| GitError::FileNotFound {
        path: oid.to_string(),
    })?;

    String::from_utf8(blob.content().to_vec()).map_err(|_| GitError::BlobNotUtf8 {
        oid: oid.to_string(),
    })
}

/// Commit a merge the writer resolved by hand.
///
/// Takes only the pages the writer actually decided about. The merge is re-run
/// in memory and the resolutions are laid over its result, so the caller never
/// has to reproduce libgit2's merge of the pages that merged cleanly — and
/// cannot get it subtly wrong.
///
/// Refuses, without writing anything, when:
///
/// - HEAD is not `expected_head_hash` (`HeadMoved`) — another window committed
///   while the resolution sat open;
/// - the re-run merge no longer conflicts — the ground moved in a subtler way,
///   and committing would build something the writer never saw;
/// - `resolved_files` and the conflict set do not match exactly, in either
///   direction — a partial resolution silently drops one side of a conflict;
/// - a path the merge would change has unsaved edits on disk.
#[allow(clippy::too_many_arguments)]
pub fn commit_merge_impl(
    repo_path: &str,
    their_ref: &str,
    expected_head_hash: &str,
    resolved_files: &[ResolvedFile],
    message: &str,
    user_name: Option<&str>,
    user_email: Option<&str>,
    options: Option<&CommitOptions>,
) -> std::result::Result<CommitInfo, GitError> {
    info!(
        "commit_merge: their_ref={} resolved={} file(s)",
        their_ref,
        resolved_files.len()
    );
    let start = std::time::Instant::now();

    let committer = crate::utils::committer_pair(options)?;

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("commit_merge"))?;

    let their_reference = crate::branch_ops::resolve_branch_ref(&repo, their_ref)?;
    let their_commit = their_reference
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

    let head_ref = repo
        .head()
        .map_err(|e| GitError::from(e).with_operation("get_head"))?;
    let our_commit = head_ref
        .peel_to_commit()
        .map_err(|e| GitError::from(e).with_operation("peel_to_commit"))?;

    let actual_head = our_commit.id().to_string();
    if actual_head != expected_head_hash {
        return Err(GitError::HeadMoved {
            expected: expected_head_hash.to_string(),
            actual: actual_head,
        });
    }

    let mut index = repo
        .merge_commits(&our_commit, &their_commit, None)
        .map_err(|e| GitError::from(e).with_operation("merge_commits"))?;

    if !index.has_conflicts() {
        // HEAD is where the caller left it, yet the merge is now clean. Some
        // other input changed — the merged branch moved, or a config that
        // affects merging did. Committing would produce a tree nobody
        // reviewed, so refuse and make the caller re-run the merge.
        return Err(GitError::MergeNoLongerConflicts {
            branch: their_ref.to_string(),
        });
    }

    // Collect the raw conflicts once: they carry both the paths to validate
    // against and the file modes the overlay needs.
    let raw_conflicts: Vec<IndexConflict> = index
        .conflicts()
        .map_err(|e| GitError::from(e).with_operation("index_conflicts"))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| GitError::from(e).with_operation("iterate_conflicts"))?;

    let mut conflicted_paths: BTreeSet<String> = BTreeSet::new();
    let mut modes: BTreeMap<String, u32> = BTreeMap::new();
    for conflict in &raw_conflicts {
        let path = conflict_path(conflict)?;
        let mode = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .map(|entry| entry.mode)
            .unwrap_or(DEFAULT_FILE_MODE);
        modes.insert(path.clone(), mode);
        conflicted_paths.insert(path);
    }

    validate_resolution_set(&conflicted_paths, resolved_files)?;

    // Overlay. `remove_path` clears every stage of the conflict (libgit2 moves
    // the three entries to the resolve-undo section); a `None` resolution
    // simply stops there, which is what resolving a delete/modify as a
    // deletion means.
    for resolved in resolved_files {
        let path = Path::new(&resolved.path);
        index
            .remove_path(path)
            .map_err(|e| GitError::from(e).with_operation("index_remove_path"))?;

        if let Some(content) = &resolved.content {
            let blob_oid = repo
                .blob(content.as_bytes())
                .map_err(|e| GitError::from(e).with_operation("write_blob"))?;
            let mode = modes
                .get(&resolved.path)
                .copied()
                .unwrap_or(DEFAULT_FILE_MODE);
            index
                .add(&stage_zero_entry(
                    &resolved.path,
                    blob_oid,
                    mode,
                    content.len(),
                ))
                .map_err(|e| GitError::from(e).with_operation("index_add"))?;
        }
    }

    let commit_id = finish_merge(
        &repo,
        &mut index,
        &head_ref,
        &our_commit,
        &their_commit,
        message,
        user_name,
        user_email,
        committer,
    )?;

    let commit = repo
        .find_commit(commit_id)
        .map_err(|e| GitError::from(e).with_operation("find_commit"))?;

    info!(
        "commit_merge: created merge commit {} in {}ms",
        commit_id,
        start.elapsed().as_millis()
    );

    Ok(crate::history_ops::commit_info_from(&commit))
}

// ===== INTERNALS =====

/// The three sides of every contested path, sorted by path so two runs of the
/// same merge produce the same list.
///
/// The common ancestor comes out of the index conflict itself, so there is no
/// separate merge-base lookup to get wrong.
fn conflicted_files(index: &Index) -> std::result::Result<Vec<ConflictedFile>, GitError> {
    let mut files = Vec::new();

    for conflict in index
        .conflicts()
        .map_err(|e| GitError::from(e).with_operation("index_conflicts"))?
    {
        let conflict =
            conflict.map_err(|e| GitError::from(e).with_operation("iterate_conflicts"))?;
        files.push(ConflictedFile {
            path: conflict_path(&conflict)?,
            ancestor_oid: conflict.ancestor.as_ref().map(|e| e.id.to_string()),
            ours_oid: conflict.our.as_ref().map(|e| e.id.to_string()),
            theirs_oid: conflict.their.as_ref().map(|e| e.id.to_string()),
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The path of a conflict, taken from whichever side still has an entry.
fn conflict_path(conflict: &IndexConflict) -> std::result::Result<String, GitError> {
    let entry = conflict
        .ancestor
        .as_ref()
        .or(conflict.our.as_ref())
        .or(conflict.their.as_ref())
        .ok_or_else(|| GitError::GitOperationFailure {
            operation: "read_conflict_path".to_string(),
            class: 0,
            code: 0,
            message: "index conflict has no ancestor, ours or theirs entry".to_string(),
        })?;

    String::from_utf8(entry.path.clone()).map_err(|_| GitError::InvalidPath {
        path: String::from_utf8_lossy(&entry.path).into_owned(),
        reason: "path is not valid UTF-8".to_string(),
    })
}

/// A stage-0 index entry. Stage lives in the flag bits; leaving `flags` at
/// zero is what makes this a resolved entry rather than a fourth conflict
/// stage. libgit2 recomputes the path-length bits on insert.
fn stage_zero_entry(path: &str, id: Oid, mode: u32, size: usize) -> IndexEntry {
    IndexEntry {
        ctime: IndexTime::new(0, 0),
        mtime: IndexTime::new(0, 0),
        dev: 0,
        ino: 0,
        mode,
        uid: 0,
        gid: 0,
        file_size: size as u32,
        id,
        flags: 0,
        flags_extended: 0,
        path: path.as_bytes().to_vec(),
    }
}

/// The resolution must cover the conflict set exactly — no extra paths, none
/// missing, no duplicates. Each mismatch is reported separately and names the
/// offending paths, because "your resolution is wrong" is not something a
/// caller can act on.
fn validate_resolution_set(
    conflicted: &BTreeSet<String>,
    resolved_files: &[ResolvedFile],
) -> std::result::Result<(), GitError> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut duplicates: BTreeSet<String> = BTreeSet::new();
    for resolved in resolved_files {
        if !seen.insert(resolved.path.clone()) {
            duplicates.insert(resolved.path.clone());
        }
    }

    if !duplicates.is_empty() {
        return Err(GitError::InvalidArgument {
            argument: "resolvedFiles".to_string(),
            reason: format!("resolved more than once: {}", join_paths(duplicates.iter())),
        });
    }

    let unknown: Vec<&String> = seen.difference(conflicted).collect();
    if !unknown.is_empty() {
        return Err(GitError::InvalidArgument {
            argument: "resolvedFiles".to_string(),
            reason: format!(
                "not conflicted in this merge: {}",
                join_paths(unknown.into_iter())
            ),
        });
    }

    let missing: Vec<&String> = conflicted.difference(&seen).collect();
    if !missing.is_empty() {
        return Err(GitError::UnresolvedConflicts {
            files: missing.into_iter().cloned().collect(),
        });
    }

    Ok(())
}

fn join_paths<'a>(paths: impl Iterator<Item = &'a String>) -> String {
    paths.cloned().collect::<Vec<_>>().join(", ")
}

/// Turn a conflict-free in-memory index into a merge commit, updating the
/// working tree, the on-disk index and the current ref.
///
/// The order is deliberate and is the whole reason an abandoned or refused
/// merge costs nothing:
///
/// 1. write the tree and check the working tree against it — a dirty page in
///    the merge set stops everything here, before a ref has moved;
/// 2. create the commit object with no ref update, so it is still unreachable;
/// 3. check out the merged tree while HEAD is still the old commit — this is
///    the step that can fail on a working-tree conflict, and at this point the
///    only thing written is an unreferenced object that `git gc` removes;
/// 4. move the ref last, which is the step that cannot meaningfully fail.
///
/// `write_tree_to` rather than `write_tree` because the index is detached and
/// has no repository of its own to write into.
#[allow(clippy::too_many_arguments)]
fn finish_merge(
    repo: &Repository,
    index: &mut Index,
    head_ref: &git2::Reference<'_>,
    our_commit: &Commit<'_>,
    their_commit: &Commit<'_>,
    message: &str,
    user_name: Option<&str>,
    user_email: Option<&str>,
    committer: Option<(&str, &str)>,
) -> std::result::Result<Oid, GitError> {
    let tree_id = index
        .write_tree_to(repo)
        .map_err(|e| GitError::from(e).with_operation("write_tree_to"))?;
    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| GitError::from(e).with_operation("find_tree"))?;

    // Only pages the merge would actually change are consulted, so an
    // unrelated draft the writer has open does not block the merge.
    let mut dirty = crate::branch_ops::collect_actual_conflicts(repo, &tree)?;
    dirty.extend(dirty_deletions(repo, &tree)?);
    dirty.sort();
    dirty.dedup();
    if !dirty.is_empty() {
        info!(
            "merge: refused — {} dirty file(s) in merge set",
            dirty.len()
        );
        return Err(GitError::UnstagedChangesWouldBeLost { files: dirty });
    }

    let signature = crate::utils::read_user_signature(repo, user_name, user_email)?;
    let committer = crate::utils::committer_signature(&signature, committer)?;

    // No ref update yet: an unreachable commit is not a state anyone has to
    // clean up if the checkout below refuses.
    let commit_id = repo
        .commit(
            None,
            &signature,
            &committer,
            message,
            &tree,
            &[our_commit, their_commit],
        )
        .map_err(|e| GitError::from(e).with_operation("create_commit"))?;

    checkout_merged_tree(repo, &tree)?;

    let reflog_message = format!("commit (merge): {}", message);
    if head_ref.is_branch() {
        let branch_ref_name = head_ref.name().ok_or_else(|| GitError::InvalidBranchName {
            name: "<non-UTF-8 branch ref>".to_string(),
        })?;
        repo.reference(branch_ref_name, commit_id, true, &reflog_message)
            .map_err(|e| GitError::from(e).with_operation("update_branch"))?;
    } else {
        repo.reference("HEAD", commit_id, true, &reflog_message)
            .map_err(|e| GitError::from(e).with_operation("update_head"))?;
    }

    Ok(commit_id)
}

/// Pages the merge would delete that the writer has unsaved edits to.
///
/// `collect_actual_conflicts` answers "which dirty files does the target tree
/// hold a *different version* of", which by construction cannot see a file the
/// target tree does not hold at all. A merge that removes a page still open in
/// the editor is exactly that case, and it is the single worst thing this
/// library could get wrong — so it is named here rather than left to libgit2's
/// "1 conflict prevents checkout", which tells the caller nothing it can route
/// on and nothing it can show the writer.
///
/// A page the writer has *also* deleted is not reported: the merge agrees with
/// them, and there is nothing to lose.
fn dirty_deletions(
    repo: &Repository,
    tree: &Tree<'_>,
) -> std::result::Result<Vec<String>, GitError> {
    let head_tree = match repo.head().and_then(|head| head.peel_to_tree()) {
        Ok(tree) => tree,
        // No HEAD tree means no deletions are possible.
        Err(_) => return Ok(Vec::new()),
    };

    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false);
    opts.include_ignored(false);
    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| GitError::from(e).with_operation("get_status"))?;

    let mut files = Vec::new();
    for entry in statuses.iter() {
        if !entry
            .status()
            .intersects(git2::Status::INDEX_MODIFIED | git2::Status::WT_MODIFIED)
        {
            continue;
        }
        let Some(path) = entry.path() else { continue };
        let as_path = Path::new(path);
        if head_tree.get_path(as_path).is_ok() && tree.get_path(as_path).is_err() {
            files.push(path.to_string());
        }
    }

    Ok(files)
}

/// Materialise the merged tree in the working tree and the on-disk index,
/// safely — a local edit that would be overwritten aborts the checkout rather
/// than being lost, and is reported as the files it would have destroyed.
///
/// This runs before the ref moves, so libgit2's baseline is the old HEAD tree
/// and the diff it applies is exactly what the merge changed. Doing it after
/// the ref moved would make baseline and target the same tree and quietly
/// write nothing.
fn checkout_merged_tree(repo: &Repository, tree: &Tree<'_>) -> std::result::Result<(), GitError> {
    let mut builder = git2::build::CheckoutBuilder::new();
    builder.safe();

    if let Err(e) = repo.checkout_tree(tree.as_object(), Some(&mut builder)) {
        // Same conflict detection as checkout_branch_internal_impl and
        // fast_forward_impl: safe mode's own error does not say which files
        // stood in the way, so re-derive the list.
        let is_conflict_error = e.code() == git2::ErrorCode::Uncommitted
            || e.code() == git2::ErrorCode::Modified
            || e.message().contains("conflict");

        if is_conflict_error {
            let files = crate::branch_ops::collect_actual_conflicts(repo, tree)?;
            if !files.is_empty() {
                return Err(GitError::UnstagedChangesWouldBeLost { files });
            }
        }
        return Err(GitError::from(e).with_operation("checkout_tree"));
    }

    Ok(())
}

// ===== NAPI WRAPPERS (only compiled with napi-binding feature) =====

#[cfg(feature = "napi-binding")]
pub async fn merge(
    service: &GitService,
    repo_path: String,
    branch: String,
    user_name: Option<String>,
    user_email: Option<String>,
    options: Option<MergeOptions>,
) -> Result<MergeOutcome> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        merge_impl(
            &repo_path,
            &branch,
            user_name.as_deref(),
            user_email.as_deref(),
            options.as_ref(),
        )
    })
    .await
}

/// No repository lock: reading a blob by oid neither writes nor depends on
/// anything a concurrent write could move. Objects are immutable.
#[cfg(feature = "napi-binding")]
pub async fn read_blob(service: &GitService, repo_path: String, oid: String) -> Result<String> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || read_blob_impl(&repo_path, &oid)).await
}

#[cfg(feature = "napi-binding")]
#[allow(clippy::too_many_arguments)]
pub async fn commit_merge(
    service: &GitService,
    repo_path: String,
    their_ref: String,
    expected_head_hash: String,
    resolved_files: Vec<ResolvedFile>,
    message: String,
    user_name: Option<String>,
    user_email: Option<String>,
    options: Option<CommitOptions>,
) -> Result<CommitInfo> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        commit_merge_impl(
            &repo_path,
            &their_ref,
            &expected_head_hash,
            &resolved_files,
            &message,
            user_name.as_deref(),
            user_email.as_deref(),
            options.as_ref(),
        )
    })
    .await
}

#[cfg(test)]
#[path = "merge_ops_tests.rs"]
mod tests;
