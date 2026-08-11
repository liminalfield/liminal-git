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
