// types.rs

#[cfg(feature = "napi-binding")]
use napi_derive::napi;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct FileStatus {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct GitStatus {
    pub modified_files: Vec<FileStatus>,
    pub deleted_files: Vec<FileStatus>,
    pub added_files: Vec<FileStatus>,
    pub untracked_files: Vec<FileStatus>,
    pub staged_files: Vec<FileStatus>,
    pub renamed_files: Vec<RenamedStatus>,
    pub is_clean: bool,
    pub current_branch: Option<String>,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct RenamedStatus {
    pub old_path: String,
    pub new_path: String,
    pub staged: bool,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct RepositoryConfig {
    pub description: Option<String>,
    pub default_branch: Option<String>,
    pub line_ending: Option<String>, // "lf", "crlf", "auto"
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct RepositoryHealth {
    pub is_healthy: bool,
    pub issues: Vec<String>,
    pub warnings: Vec<String>,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct GitConfig {
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub core_autocrlf: Option<String>,
    pub core_safecrlf: Option<String>,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct RepositoryInfo {
    pub path: String,
    pub is_bare: bool,
    pub head_commit: Option<String>,
    pub branch_count: i32,
    pub commit_count: i32,
    pub has_uncommitted_changes: bool,
    pub remote_urls: Vec<String>,
}

// Add these to types.rs

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub short_hash: String,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: String,
    pub parent_hashes: Vec<String>,
    pub file_changes: i32,
    pub insertions: i32,
    pub deletions: i32,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct CommitHistory {
    pub commits: Vec<CommitInfo>,
    pub total_count: i32,
    pub has_more: bool,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct FileAtCommit {
    pub path: String,
    pub content: String,
    pub exists: bool,
    pub commit_hash: String,
}

/// One path present in a commit's tree, as reported by `getTreeAtCommit`.
///
/// Directories are not entries. They are implied by the paths of the files
/// inside them, which is what lets the result be zipped directly with
/// `getFileAtCommit` calls against the same commit.
#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct TreeEntry {
    /// Repository-relative path, using forward slashes on every platform.
    pub path: String,
    /// `"file"`, `"symlink"` or `"submodule"`.
    pub kind: String,
    /// Size of the underlying blob in bytes. For a `"symlink"` this is the
    /// length of the target path, since that is what the blob holds. For a
    /// `"submodule"` there is no blob and this is `0`.
    pub size: i64,
    /// Full oid of the blob at this path. For a `"submodule"` there is no
    /// blob, and this is the commit oid the parent repository has recorded
    /// for it.
    pub blob_hash: String,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct DeletedFileEntry {
    /// Repo-relative file path (e.g., "pages/foo.md")
    pub path: String,
    /// Unix timestamp (in seconds) when the file was deleted
    pub deleted_at: i64,
    /// Commit hash where the file was last seen (before deletion)
    pub last_commit: String,
    /// Commit message of the deletion commit
    pub last_commit_message: String,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: i32,
    pub old_lines: i32,
    pub new_start: i32,
    pub new_lines: i32,
    pub lines: Vec<DiffLine>,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_type: String, // "added", "removed", "context"
    pub content: String,
    pub old_line_number: Option<i32>,
    pub new_line_number: Option<i32>,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub file_path: String,
    pub old_path: Option<String>,
    pub status: String, // "added", "deleted", "modified", "renamed"
    pub hunks: Vec<DiffHunk>,
    pub additions: i32,
    pub deletions: i32,
    pub is_binary: bool,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct CommitDiff {
    pub commit_hash: String,
    pub parent_hash: Option<String>,
    pub files: Vec<FileDiff>,
    pub total_additions: i32,
    pub total_deletions: i32,
    pub files_changed: i32,
}

// Information about a git branch
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct BranchInfo {
    /// Branch name (e.g., "main", "feature/new-feature")
    pub name: String,
    /// Whether this is the currently checked out branch
    pub is_current: bool,
    /// Whether this is a remote branch
    pub is_remote: bool,
    /// Hash of the commit this branch points to
    pub commit_hash: String,
    /// Message of the commit this branch points to
    pub commit_message: String,
    /// ISO timestamp of when this branch was last updated
    pub last_updated: String,
    /// How many commits ahead/behind the default branch (for local branches)
    pub ahead_behind: Option<AheadBehind>,
}

/// Tracking information relative to upstream branch
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct AheadBehind {
    /// Number of commits ahead of upstream
    pub ahead: u32,
    /// Number of commits behind upstream
    pub behind: u32,
}

/// What merging `branch` into HEAD would do, without doing it.
///
/// `ahead`/`behind` are always reported relative to `branch`, regardless of
/// `kind` — even when `kind` is `"up-to-date"` HEAD may be ahead by any
/// number of commits; `"up-to-date"` only means `behind` is zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct MergeAnalysis {
    /// `"up-to-date"` — HEAD already contains everything `branch` has.
    /// `"fast-forward"` — HEAD is an ancestor of `branch`; moving the ref is
    /// sufficient, no merge commit is needed.
    /// `"normal"` — HEAD and `branch` have diverged; a real merge is required.
    pub kind: String,
    /// Commits HEAD has that `branch` does not.
    pub ahead: u32,
    /// Commits `branch` has that HEAD does not.
    pub behind: u32,
}

/// Result of moving the current branch forward to `branch` via `fastForward`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct FastForwardResult {
    /// Name of the branch that was moved (the branch HEAD was on).
    pub branch: String,
    /// Commit the branch pointed to before the fast-forward.
    pub previous_commit_hash: String,
    /// Commit the branch points to now — the same commit `branch` pointed to.
    pub commit_hash: String,
}

/// Information about a git tag
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct TagInfo {
    /// Tag name (e.g., "v1.0.0", "release-candidate")
    pub name: String,
    /// Hash of the commit this tag points to
    pub commit_hash: String,
    /// Message of the commit this tag points to
    pub commit_message: String,
    /// Tag message (for annotated tags only)
    pub tag_message: Option<String>,
    /// Tagger information (for annotated tags only)
    pub tagger: Option<String>,
    /// ISO timestamp of when this tag was created
    pub created: String,
    /// Whether this is an annotated tag (vs lightweight tag)
    pub is_annotated: bool,
}

/// Options for creating a new branch
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct CreateBranchOptions {
    /// Name for the new branch
    pub name: String,
    /// Commit hash to branch from (if None, branches from current HEAD)
    pub from_commit: Option<String>,
    /// Whether to immediately switch to the new branch after creation
    pub checkout: bool,
}

/// Options for creating a new tag
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct CreateTagOptions {
    /// Name for the new tag
    pub name: String,
    /// Commit hash to tag (if None, tags current HEAD)
    pub target_commit: Option<String>,
    /// Tag message (if provided, creates annotated tag; otherwise lightweight tag)
    pub message: Option<String>,
    /// Whether to overwrite existing tag with same name
    pub force: bool,
    /// User name for annotated tag signature (optional, falls back to git config)
    pub user_name: Option<String>,
    /// User email for annotated tag signature (optional, falls back to git config)
    pub user_email: Option<String>,
}

// ===== REMOTE OPERATIONS =====

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    /// Fetch URL. `None` for a remote configured with no URL, which git allows.
    pub url: Option<String>,
    /// Push URL when it differs from the fetch URL, as `remote.<name>.pushurl`.
    pub push_url: Option<String>,
}

/// Credentials for a single remote operation.
///
/// Deliberately passed per call rather than stored. This library has no
/// business owning secrets: the host application knows where they came from
/// (an OS keychain, an environment variable, a prompt) and how long they may
/// live, and it is far better placed to decide. Everything here is optional —
/// see `remote_ops::credential_callback` for the fallback order when a field
/// is absent.
#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone, Default)]
pub struct RemoteCredentials {
    /// Username for HTTPS. For a personal access token on GitHub this can be
    /// anything non-empty; the token goes in `password`.
    pub username: Option<String>,
    /// Password or personal access token for HTTPS.
    pub password: Option<String>,
    /// Path to an SSH private key. The matching `.pub` is used if present.
    pub ssh_private_key_path: Option<String>,
    /// Passphrase for the SSH private key, when it has one.
    pub ssh_passphrase: Option<String>,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct FetchResult {
    pub remote: String,
    /// Refs whose remote-tracking branch moved, as "refs/heads/main".
    pub updated_refs: Vec<String>,
    pub received_objects: u32,
    pub received_bytes: f64,
}

#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct PushResult {
    pub remote: String,
    /// Refspecs that were pushed.
    pub pushed_refs: Vec<String>,
}

/// How the local branch stands against its remote-tracking branch.
///
/// Distinct from `AheadBehind`, which compares two local branches. This one
/// answers "do I need to push, pull, both, or neither", and is only meaningful
/// after a fetch — git cannot know what a remote holds without asking it.
#[cfg_attr(feature = "napi-binding", napi(object))]
#[derive(Debug, Clone)]
pub struct UpstreamStatus {
    pub branch: String,
    /// The tracking branch, as "origin/main". `None` if the branch has no
    /// upstream configured, which is not an error.
    pub upstream: Option<String>,
    /// Local commits the upstream does not have.
    pub ahead: u32,
    /// Upstream commits the local branch does not have.
    pub behind: u32,
    /// True when there is no upstream to compare against, so `ahead` and
    /// `behind` are both zero for lack of information rather than for lack of
    /// difference. Callers must distinguish these or they will report a branch
    /// as up to date when they simply do not know.
    pub no_upstream: bool,
}

// ===== MERGE OPERATIONS =====

/// What `merge` did. Exactly one of the four kinds, and the library never
/// leaves an in-progress merge behind for any of them: a `"conflicted"`
/// outcome is a *report*, not a state — nothing was written, there is no
/// `MERGE_HEAD`, and the caller is free to walk away.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct MergeOutcome {
    /// `"merged"` — a real merge commit was created.
    /// `"fast-forwarded"` — the branch ref moved; no merge commit exists.
    /// `"up-to-date"` — HEAD already contained everything; nothing was written.
    /// `"conflicted"` — nothing was written; see `conflicts`.
    pub kind: String,
    /// The commit HEAD points at after the operation. `None` — and only
    /// `None` — when `kind` is `"conflicted"`.
    pub commit_hash: Option<String>,
    /// The contested paths, sorted by path. Empty unless `kind` is
    /// `"conflicted"`.
    pub conflicts: Vec<ConflictedFile>,
}

/// One path libgit2 could not merge on its own, with the three blobs needed
/// to show the writer what happened. Each side is `None` when that side has
/// no version of the page: no ancestor means the page is new on both sides,
/// and a missing `ours`/`theirs` means that side deleted it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct ConflictedFile {
    /// Repo-relative path of the contested file.
    pub path: String,
    /// Blob oid of the common ancestor's version, if there is one.
    pub ancestor_oid: Option<String>,
    /// Blob oid of HEAD's version, if our side still has the file.
    pub ours_oid: Option<String>,
    /// Blob oid of the merged branch's version, if their side still has it.
    pub theirs_oid: Option<String>,
}

/// A writer's decision about one contested page, handed back to `commitMerge`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "napi-binding", napi(object))]
pub struct ResolvedFile {
    /// Repo-relative path. Must be one of the paths `merge` reported as
    /// conflicted.
    pub path: String,
    /// The resolved text of the page. `None` resolves the conflict as a
    /// deletion — the page is not in the merge commit's tree.
    pub content: Option<String>,
}
