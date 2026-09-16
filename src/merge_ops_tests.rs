use super::*;
use crate::branch_ops::checkout_branch_impl;
use crate::errors::GitError;
use crate::types::ResolvedFile;
use git2::{Repository, RepositoryState};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ===== FIXTURES =====
//
// Same shape as branch_ops_tests.rs: TMPDIR is pinned to the temp directory's
// parent so git2's lock-file renames stay on one filesystem, and every test is
// #[serial] because that environment variable is process-wide.

fn setup_test_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    if let Some(parent) = temp_dir.path().parent() {
        unsafe {
            std::env::set_var("TMPDIR", parent);
            std::env::set_var("TMP", parent);
            std::env::set_var("TEMP", parent);
        }
    }

    Repository::init(&repo_path).expect("Failed to initialize test repository");

    let repo = Repository::open(&repo_path).expect("Failed to open repository");
    let mut config = repo.config().expect("Failed to get config");
    config
        .set_str("user.name", "Test User")
        .expect("Failed to set user.name");
    config
        .set_str("user.email", "test@example.com")
        .expect("Failed to set user.email");

    // Pinned because these tests assert file *contents* after a checkout, and
    // the merge operations write the working tree. Git for Windows ships with
    // core.autocrlf=true globally, which rewrites LF to CRLF on the way out,
    // so "theirs\n" comes back as "theirs\r\n" and three assertions fail on
    // Windows alone. That is git doing exactly what it was configured to do;
    // the tests are about merge semantics, not line-ending translation, so the
    // repository states what it needs rather than inheriting the machine's.
    config
        .set_bool("core.autocrlf", false)
        .expect("Failed to set core.autocrlf");

    (temp_dir, repo_path)
}

fn write_file(repo_path: &Path, file: &str, content: &str) {
    let full_path = repo_path.join(file);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create parent directories");
    }
    std::fs::write(&full_path, content).expect("Failed to write file");
}

fn read_file(repo_path: &Path, file: &str) -> String {
    std::fs::read_to_string(repo_path.join(file)).expect("Failed to read file")
}

/// Stage everything in the working tree — additions, modifications and
/// deletions — and commit it to the current branch.
fn commit_all(repo_path: &Path, message: &str) -> String {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let mut index = repo.index().expect("Failed to get index");
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .expect("Failed to add all");
    // add_all does not notice files that vanished; update_all does.
    index
        .update_all(["*"].iter(), None)
        .expect("Failed to update all");
    index.write().expect("Failed to write index");

    let tree_id = index.write_tree().expect("Failed to write tree");
    let tree = repo.find_tree(tree_id).expect("Failed to find tree");
    let signature =
        git2::Signature::now("Test User", "test@example.com").expect("Failed to create signature");

    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let commit_id = match parent {
        Some(parent) => repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        ),
        None => repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[]),
    };

    commit_id.expect("Failed to create commit").to_string()
}

fn write_and_commit(repo_path: &Path, file: &str, content: &str, message: &str) -> String {
    write_file(repo_path, file, content);
    commit_all(repo_path, message)
}

fn create_branch(repo_path: &Path, branch_name: &str) {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let commit = repo
        .head()
        .expect("Failed to get HEAD")
        .peel_to_commit()
        .expect("Failed to get commit");
    repo.branch(branch_name, &commit, false)
        .expect("Failed to create branch");
}

fn checkout_default(repo_path: &Path) {
    checkout_branch_impl(repo_path.to_str().unwrap(), "master")
        .or_else(|_| checkout_branch_impl(repo_path.to_str().unwrap(), "main"))
        .expect("checkout default branch");
}

fn head_hash(repo_path: &Path) -> String {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    repo.head()
        .expect("Failed to get HEAD")
        .peel_to_commit()
        .expect("Failed to peel HEAD")
        .id()
        .to_string()
}

fn branch_tip(repo_path: &Path, branch: &str) -> String {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    repo.find_branch(branch, git2::BranchType::Local)
        .expect("Failed to find branch")
        .get()
        .peel_to_commit()
        .expect("Failed to peel branch")
        .id()
        .to_string()
}

fn parents_of(repo_path: &Path, hash: &str) -> Vec<String> {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let oid = git2::Oid::from_str(hash).expect("Failed to parse oid");
    repo.find_commit(oid)
        .expect("Failed to find commit")
        .parent_ids()
        .map(|id| id.to_string())
        .collect()
}

fn commit_count(repo_path: &Path) -> usize {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let mut walk = repo.revwalk().expect("Failed to create revwalk");
    walk.push_head().expect("Failed to push head");
    walk.count()
}

/// Every tracked-or-untracked change the repository reports. Empty means the
/// working tree and index agree with HEAD.
fn status_paths(repo_path: &Path) -> Vec<String> {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    repo.statuses(Some(&mut opts))
        .expect("Failed to get statuses")
        .iter()
        .filter_map(|entry| entry.path().ok().map(|p| p.to_string()))
        .collect()
}

/// Path -> bytes for every file under `dir`, recursively. Used for the
/// byte-identical assertions: nothing weaker would catch a stray `MERGE_HEAD`
/// or a rewritten index.
fn snapshot_tree(root: &Path, skip_git: bool) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, skip_git: bool, out: &mut BTreeMap<String, Vec<u8>>) {
        let entries = std::fs::read_dir(dir).expect("Failed to read dir");
        for entry in entries {
            let entry = entry.expect("Failed to read dir entry");
            let path = entry.path();
            if skip_git && path.file_name().map(|n| n == ".git").unwrap_or(false) {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, skip_git, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("Failed to strip prefix")
                    .to_string_lossy()
                    .into_owned();
                out.insert(rel, std::fs::read(&path).expect("Failed to read file"));
            }
        }
    }

    let mut out = BTreeMap::new();
    walk(root, root, skip_git, &mut out);
    out
}

fn snapshot_worktree(repo_path: &Path) -> BTreeMap<String, Vec<u8>> {
    snapshot_tree(repo_path, true)
}

fn snapshot_git_dir(repo_path: &Path) -> BTreeMap<String, Vec<u8>> {
    snapshot_tree(&repo_path.join(".git"), false)
}

/// base -> ours/theirs both editing `page.md`, the classic contested page.
/// Returns the hash of our side's commit, i.e. HEAD before the merge.
fn diverged_on_page(repo_path: &Path) -> String {
    write_and_commit(repo_path, "page.md", "base\n", "base");
    create_branch(repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(repo_path, "page.md", "theirs\n", "their edit");

    checkout_default(repo_path);
    write_and_commit(repo_path, "page.md", "ours\n", "our edit")
}

// ===== merge: the four outcomes =====

#[test]
#[serial_test::serial]
fn test_merge_clean_disjoint_files_creates_two_parent_commit() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B", "their file");
    let feature_tip = branch_tip(&repo_path, "feature");

    checkout_default(&repo_path);
    let ours = write_and_commit(&repo_path, "d.txt", "D", "our file");

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    assert_eq!(outcome.kind, "merged");
    assert!(outcome.conflicts.is_empty());
    let merge_commit = outcome
        .commit_hash
        .expect("a merged outcome carries a hash");

    assert_eq!(
        parents_of(&repo_path, &merge_commit),
        vec![ours.clone(), feature_tip.clone()],
        "HEAD must be the first parent and the merged branch the second"
    );
    assert_eq!(head_hash(&repo_path), merge_commit, "HEAD must have moved");

    // Both sides' work is on disk, not merely in the tree object.
    assert_eq!(read_file(&repo_path, "b.txt"), "B");
    assert_eq!(read_file(&repo_path, "d.txt"), "D");
    assert_eq!(read_file(&repo_path, "a.txt"), "A");

    assert!(
        status_paths(&repo_path).is_empty(),
        "index and working tree must agree with the new HEAD"
    );
    let repo = Repository::open(&repo_path).unwrap();
    assert_eq!(repo.state(), RepositoryState::Clean);
    assert!(!repo_path.join(".git/MERGE_HEAD").exists());
}

#[test]
#[serial_test::serial]
fn test_merge_up_to_date_writes_nothing() {
    let (_tmp, repo_path) = setup_test_repo();
    let base = write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature"); // feature == HEAD

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    assert_eq!(outcome.kind, "up-to-date");
    assert_eq!(outcome.commit_hash, Some(base.clone()));
    assert!(outcome.conflicts.is_empty());
    assert_eq!(head_hash(&repo_path), base);
    assert_eq!(commit_count(&repo_path), 1, "no new commit may be created");
}

#[test]
#[serial_test::serial]
fn test_merge_fast_forward_creates_no_merge_commit() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B", "their file");
    let feature_tip = branch_tip(&repo_path, "feature");

    checkout_default(&repo_path);

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    assert_eq!(outcome.kind, "fast-forwarded");
    assert_eq!(outcome.commit_hash, Some(feature_tip.clone()));
    assert_eq!(head_hash(&repo_path), feature_tip);
    assert_eq!(
        parents_of(&repo_path, &feature_tip).len(),
        1,
        "a fast-forward must not manufacture a merge commit"
    );
    assert_eq!(read_file(&repo_path, "b.txt"), "B");
}

#[test]
#[serial_test::serial]
fn test_merge_conflict_reports_all_three_sides() {
    let (_tmp, repo_path) = setup_test_repo();
    diverged_on_page(&repo_path);

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    assert_eq!(outcome.kind, "conflicted");
    assert_eq!(outcome.commit_hash, None, "nothing was committed");
    assert_eq!(outcome.conflicts.len(), 1);

    let conflict = &outcome.conflicts[0];
    assert_eq!(conflict.path, "page.md");

    let path = repo_path.to_str().unwrap();
    let ancestor = conflict.ancestor_oid.as_ref().expect("common ancestor");
    let ours = conflict.ours_oid.as_ref().expect("our side");
    let theirs = conflict.theirs_oid.as_ref().expect("their side");

    // The oids are what the caller will feed straight back to readBlob, so
    // check them by their content rather than by shape.
    assert_eq!(read_blob_impl(path, ancestor).unwrap(), "base\n");
    assert_eq!(read_blob_impl(path, ours).unwrap(), "ours\n");
    assert_eq!(read_blob_impl(path, theirs).unwrap(), "theirs\n");
}

#[test]
#[serial_test::serial]
fn test_merge_delete_modify_conflict_has_no_oid_for_the_deleting_side() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "page.md", "base\n", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    std::fs::remove_file(repo_path.join("page.md")).expect("delete page");
    commit_all(&repo_path, "their deletion");

    checkout_default(&repo_path);
    write_and_commit(&repo_path, "page.md", "ours\n", "our edit");

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    assert_eq!(outcome.kind, "conflicted");
    assert_eq!(outcome.conflicts.len(), 1);
    let conflict = &outcome.conflicts[0];
    assert_eq!(conflict.path, "page.md");
    assert!(
        conflict.ancestor_oid.is_some(),
        "the page existed at the base"
    );
    assert!(conflict.ours_oid.is_some(), "our side still has the page");
    assert_eq!(
        conflict.theirs_oid, None,
        "the deleting side must report no blob"
    );
}

#[test]
#[serial_test::serial]
fn test_merge_unknown_branch_is_branch_not_found() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");

    match merge_impl(
        repo_path.to_str().unwrap(),
        "does-not-exist",
        None,
        None,
        None,
    ) {
        Err(GitError::BranchNotFound { name }) => assert_eq!(name, "does-not-exist"),
        other => panic!("expected BranchNotFound, got {:?}", other),
    }
}

// ===== the property the whole design exists for =====

#[test]
#[serial_test::serial]
fn test_conflicted_merge_leaves_the_repository_byte_identical() {
    let (_tmp, repo_path) = setup_test_repo();
    diverged_on_page(&repo_path);

    let worktree_before = snapshot_worktree(&repo_path);
    let git_dir_before = snapshot_git_dir(&repo_path);
    let head_before = head_hash(&repo_path);
    let status_before = status_paths(&repo_path);

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");
    assert_eq!(outcome.kind, "conflicted");

    let worktree_after = snapshot_worktree(&repo_path);
    let git_dir_after = snapshot_git_dir(&repo_path);

    assert_eq!(
        worktree_before, worktree_after,
        "a conflicted merge must not touch a single byte of the working tree"
    );
    assert_eq!(
        git_dir_before.keys().collect::<Vec<_>>(),
        git_dir_after.keys().collect::<Vec<_>>(),
        "a conflicted merge must not add or remove any file under .git"
    );
    assert_eq!(
        git_dir_before, git_dir_after,
        "a conflicted merge must not rewrite any file under .git"
    );
    assert_eq!(head_before, head_hash(&repo_path), "HEAD must not move");
    assert_eq!(status_before, status_paths(&repo_path));

    assert!(
        !repo_path.join(".git/MERGE_HEAD").exists(),
        "MERGE_HEAD is the in-progress state this library refuses to create"
    );
    let repo = Repository::open(&repo_path).unwrap();
    assert_eq!(
        repo.state(),
        RepositoryState::Clean,
        "the repository must not be left mid-merge"
    );
    assert!(
        !repo.index().unwrap().has_conflicts(),
        "the on-disk index must be free of conflict stages"
    );
}

// ===== merge: the dirty working tree =====

#[test]
#[serial_test::serial]
fn test_merge_refuses_when_a_file_in_the_merge_set_is_dirty() {
    let (_tmp, repo_path) = setup_test_repo();
    write_file(&repo_path, "page.md", "base\n");
    write_and_commit(&repo_path, "notes.md", "notes\n", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "page.md", "theirs\n", "their edit");

    checkout_default(&repo_path);
    let ours = write_and_commit(&repo_path, "extra.md", "extra\n", "our unrelated commit");

    // The writer has unsaved edits to the very page the merge would rewrite.
    write_file(&repo_path, "page.md", "unsaved\n");

    match merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None) {
        Err(GitError::UnstagedChangesWouldBeLost { files }) => {
            assert_eq!(files, vec!["page.md".to_string()]);
        }
        other => panic!("expected UnstagedChangesWouldBeLost, got {:?}", other),
    }

    assert_eq!(
        head_hash(&repo_path),
        ours,
        "a refused merge must not commit"
    );
    assert_eq!(
        read_file(&repo_path, "page.md"),
        "unsaved\n",
        "the writer's unsaved edit must survive untouched"
    );
    let repo = Repository::open(&repo_path).unwrap();
    assert_eq!(repo.state(), RepositoryState::Clean);
    assert!(!repo_path.join(".git/MERGE_HEAD").exists());
}

#[test]
#[serial_test::serial]
fn test_merge_ignores_a_dirty_file_outside_the_merge_set() {
    let (_tmp, repo_path) = setup_test_repo();
    write_file(&repo_path, "page.md", "base\n");
    write_and_commit(&repo_path, "notes.md", "notes\n", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "page.md", "theirs\n", "their edit");

    checkout_default(&repo_path);
    write_and_commit(&repo_path, "extra.md", "extra\n", "our unrelated commit");

    // notes.md is dirty but the merge does not touch it.
    write_file(&repo_path, "notes.md", "an unsaved draft\n");

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    assert_eq!(outcome.kind, "merged");
    assert_eq!(
        read_file(&repo_path, "notes.md"),
        "an unsaved draft\n",
        "a dirty file outside the merge set must survive the merge untouched"
    );
    assert_eq!(read_file(&repo_path, "page.md"), "theirs\n");
}

// ===== readBlob =====

#[test]
#[serial_test::serial]
fn test_merge_names_the_untracked_files_in_the_way() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "page.md", "from the branch\n", "their page");

    checkout_default(&repo_path);
    write_and_commit(&repo_path, "d.txt", "D", "our file");

    // Untracked, and exactly where the merge wants to write.
    write_file(&repo_path, "page.md", "an unsaved draft\n");
    let head_before = head_hash(&repo_path);

    let error = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect_err("an untracked file in the way must stop the merge");

    match error {
        GitError::UntrackedFilesWouldBeOverwritten { files } => {
            assert_eq!(files, vec!["page.md".to_string()])
        }
        other => panic!("expected UntrackedFilesWouldBeOverwritten, got {:?}", other),
    }

    assert_eq!(head_hash(&repo_path), head_before, "HEAD must not move");
    assert_eq!(read_file(&repo_path, "page.md"), "an unsaved draft\n");
}

#[test]
#[serial_test::serial]
fn test_read_blob_round_trips_content() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(
        &repo_path,
        "page.md",
        "Chapter One\n\nIt was a dark night.\n",
        "base",
    );

    let repo = Repository::open(&repo_path).unwrap();
    let oid = repo
        .head()
        .unwrap()
        .peel_to_tree()
        .unwrap()
        .get_path(Path::new("page.md"))
        .unwrap()
        .id()
        .to_string();

    let content = read_blob_impl(repo_path.to_str().unwrap(), &oid).expect("read_blob should work");
    assert_eq!(content, "Chapter One\n\nIt was a dark night.\n");
}

#[test]
#[serial_test::serial]
fn test_read_blob_refuses_non_utf8_rather_than_mangling_it() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");

    let repo = Repository::open(&repo_path).unwrap();
    let oid = repo
        .blob(&[0x48, 0x69, 0xff, 0xfe, 0x00, 0x21])
        .expect("write raw blob")
        .to_string();

    match read_blob_impl(repo_path.to_str().unwrap(), &oid) {
        Err(GitError::BlobNotUtf8 { oid: reported }) => assert_eq!(reported, oid),
        other => panic!("expected BlobNotUtf8, got {:?}", other),
    }
}

#[test]
#[serial_test::serial]
fn test_read_blob_rejects_a_malformed_oid() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");

    match read_blob_impl(repo_path.to_str().unwrap(), "not-a-hash") {
        Err(GitError::InvalidCommitHash { hash }) => assert_eq!(hash, "not-a-hash"),
        other => panic!("expected InvalidCommitHash, got {:?}", other),
    }
}

#[test]
#[serial_test::serial]
fn test_read_blob_rejects_an_object_that_is_not_a_blob() {
    let (_tmp, repo_path) = setup_test_repo();
    let commit = write_and_commit(&repo_path, "a.txt", "A", "base");

    match read_blob_impl(repo_path.to_str().unwrap(), &commit) {
        Err(GitError::FileNotFound { path }) => assert_eq!(path, commit),
        other => panic!("expected FileNotFound, got {:?}", other),
    }
}

// ===== commitMerge =====

/// base has `page.md` and `notes.md`; their side edits both and adds
/// `chapter.md`; our side edits `page.md` only. `page.md` conflicts, the rest
/// merges cleanly — which is exactly what commitMerge must preserve without
/// the caller restating it.
fn conflicted_with_clean_neighbours(repo_path: &Path) {
    write_file(repo_path, "page.md", "base\n");
    write_and_commit(repo_path, "notes.md", "notes\n", "base");
    create_branch(repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_file(repo_path, "page.md", "theirs\n");
    write_file(repo_path, "chapter.md", "chapter\n");
    write_and_commit(repo_path, "notes.md", "their notes\n", "their edits");

    checkout_default(repo_path);
    write_and_commit(repo_path, "page.md", "ours\n", "our edit");
}

#[test]
#[serial_test::serial]
fn test_commit_merge_happy_path() {
    let (_tmp, repo_path) = setup_test_repo();
    conflicted_with_clean_neighbours(&repo_path);

    let ours = head_hash(&repo_path);
    let theirs = branch_tip(&repo_path, "feature");

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");
    assert_eq!(outcome.kind, "conflicted");
    assert_eq!(outcome.conflicts.len(), 1);
    assert_eq!(outcome.conflicts[0].path, "page.md");

    let info = commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &ours,
        &[ResolvedFile {
            path: "page.md".to_string(),
            content: Some("ours and theirs, reconciled\n".to_string()),
        }],
        "Merge branch 'feature'",
        None,
        None,
        None,
    )
    .expect("commit_merge should succeed");

    assert_eq!(
        info.parent_hashes,
        vec![ours.clone(), theirs.clone()],
        "HEAD first, the merged branch second"
    );
    assert_eq!(head_hash(&repo_path), info.hash);
    assert_eq!(info.message, "Merge branch 'feature'");

    // The resolution is on disk, and so is everything libgit2 merged cleanly
    // without the caller ever mentioning it.
    assert_eq!(
        read_file(&repo_path, "page.md"),
        "ours and theirs, reconciled\n"
    );
    assert_eq!(read_file(&repo_path, "notes.md"), "their notes\n");
    assert_eq!(read_file(&repo_path, "chapter.md"), "chapter\n");

    assert!(
        status_paths(&repo_path).is_empty(),
        "the index must be clean after a resolved merge"
    );
    let repo = Repository::open(&repo_path).unwrap();
    assert_eq!(repo.state(), RepositoryState::Clean);
    assert!(!repo_path.join(".git/MERGE_HEAD").exists());
}

#[test]
#[serial_test::serial]
fn test_commit_merge_refuses_when_head_moved() {
    let (_tmp, repo_path) = setup_test_repo();
    conflicted_with_clean_neighbours(&repo_path);

    let stale = head_hash(&repo_path);
    merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    // Another window commits while the resolution sits open.
    let moved = write_and_commit(&repo_path, "elsewhere.md", "elsewhere\n", "another window");

    match commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &stale,
        &[ResolvedFile {
            path: "page.md".to_string(),
            content: Some("reconciled\n".to_string()),
        }],
        "Merge branch 'feature'",
        None,
        None,
        None,
    ) {
        Err(GitError::HeadMoved { expected, actual }) => {
            assert_eq!(expected, stale);
            assert_eq!(actual, moved);
        }
        other => panic!("expected HeadMoved, got {:?}", other),
    }

    assert_eq!(
        head_hash(&repo_path),
        moved,
        "a refused commit must not move HEAD"
    );
    assert_eq!(read_file(&repo_path, "page.md"), "ours\n");
}

#[test]
#[serial_test::serial]
fn test_commit_merge_refuses_a_path_that_is_not_conflicted() {
    let (_tmp, repo_path) = setup_test_repo();
    conflicted_with_clean_neighbours(&repo_path);
    let ours = head_hash(&repo_path);

    match commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &ours,
        &[
            ResolvedFile {
                path: "page.md".to_string(),
                content: Some("reconciled\n".to_string()),
            },
            ResolvedFile {
                path: "notes.md".to_string(),
                content: Some("hand-written notes\n".to_string()),
            },
        ],
        "Merge branch 'feature'",
        None,
        None,
        None,
    ) {
        Err(GitError::InvalidArgument { argument, reason }) => {
            assert_eq!(argument, "resolvedFiles");
            assert!(
                reason.contains("notes.md"),
                "the refusal must name the offending path, got: {}",
                reason
            );
        }
        other => panic!("expected InvalidArgument, got {:?}", other),
    }

    assert_eq!(head_hash(&repo_path), ours);
    assert_eq!(read_file(&repo_path, "notes.md"), "notes\n");
}

#[test]
#[serial_test::serial]
fn test_commit_merge_refuses_a_conflicted_path_left_unresolved() {
    let (_tmp, repo_path) = setup_test_repo();
    write_file(&repo_path, "page.md", "base\n");
    write_and_commit(&repo_path, "other.md", "base\n", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_file(&repo_path, "page.md", "theirs\n");
    write_and_commit(&repo_path, "other.md", "theirs\n", "their edits");

    checkout_default(&repo_path);
    write_file(&repo_path, "page.md", "ours\n");
    let ours = write_and_commit(&repo_path, "other.md", "ours\n", "our edits");

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");
    assert_eq!(outcome.conflicts.len(), 2, "both pages contest");
    assert_eq!(
        outcome
            .conflicts
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        vec!["other.md", "page.md"],
        "conflicts are reported sorted by path, so two runs agree"
    );

    match commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &ours,
        &[ResolvedFile {
            path: "page.md".to_string(),
            content: Some("reconciled\n".to_string()),
        }],
        "Merge branch 'feature'",
        None,
        None,
        None,
    ) {
        Err(GitError::UnresolvedConflicts { files }) => {
            // A distinct variant, not InvalidArgument: a resolution in
            // progress is the ordinary state, and the caller shows the writer
            // these pages rather than an error. Routing on it must not mean
            // parsing prose.
            assert_eq!(files, vec!["other.md".to_string()]);
        }
        other => panic!("expected UnresolvedConflicts, got {:?}", other),
    }

    assert_eq!(
        head_hash(&repo_path),
        ours,
        "a partial resolution must not commit"
    );
}

#[test]
#[serial_test::serial]
fn test_commit_merge_refuses_when_the_merge_no_longer_conflicts() {
    let (_tmp, repo_path) = setup_test_repo();
    conflicted_with_clean_neighbours(&repo_path);
    let ours = head_hash(&repo_path);

    // The writer sees the conflict and starts resolving page.md.
    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");
    assert_eq!(outcome.kind, "conflicted");

    // While the resolution sits open, the other side settles the same page to
    // our text. HEAD has not moved, so the expectedHead guard cannot catch
    // this — but the merge the resolution was built against no longer exists.
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "page.md", "ours\n", "settle page.md our way");
    checkout_default(&repo_path);
    assert_eq!(head_hash(&repo_path), ours, "HEAD must be untouched");

    match commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &ours,
        &[ResolvedFile {
            path: "page.md".to_string(),
            content: Some("reconciled\n".to_string()),
        }],
        "Merge branch 'feature'",
        None,
        None,
        None,
    ) {
        Err(GitError::MergeNoLongerConflicts { branch }) => {
            assert_eq!(branch, "feature");
        }
        other => panic!("expected MergeNoLongerConflicts, got {:?}", other),
    }

    assert_eq!(
        head_hash(&repo_path),
        ours,
        "a stale resolution must not commit"
    );
    assert_eq!(
        read_file(&repo_path, "page.md"),
        "ours\n",
        "and must not write its resolved content over the working tree"
    );
}

#[test]
#[serial_test::serial]
fn test_commit_merge_refuses_when_a_merge_set_path_is_dirty() {
    let (_tmp, repo_path) = setup_test_repo();
    conflicted_with_clean_neighbours(&repo_path);
    let ours = head_hash(&repo_path);

    // notes.md merges cleanly to their version, so it IS in the merge set —
    // and the writer has unsaved edits to it.
    write_file(&repo_path, "notes.md", "unsaved\n");

    match commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &ours,
        &[ResolvedFile {
            path: "page.md".to_string(),
            content: Some("reconciled\n".to_string()),
        }],
        "Merge branch 'feature'",
        None,
        None,
        None,
    ) {
        Err(GitError::UnstagedChangesWouldBeLost { files }) => {
            assert_eq!(files, vec!["notes.md".to_string()]);
        }
        other => panic!("expected UnstagedChangesWouldBeLost, got {:?}", other),
    }

    assert_eq!(head_hash(&repo_path), ours);
    assert_eq!(
        read_file(&repo_path, "notes.md"),
        "unsaved\n",
        "the unsaved edit must survive the refusal"
    );
    assert_eq!(read_file(&repo_path, "page.md"), "ours\n");
}

#[test]
#[serial_test::serial]
fn test_commit_merge_resolves_a_delete_modify_as_a_deletion() {
    let (_tmp, repo_path) = setup_test_repo();
    write_file(&repo_path, "page.md", "base\n");
    write_and_commit(&repo_path, "keep.md", "keep\n", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    std::fs::remove_file(repo_path.join("page.md")).expect("delete page");
    commit_all(&repo_path, "their deletion");

    checkout_default(&repo_path);
    let ours = write_and_commit(&repo_path, "page.md", "ours\n", "our edit");

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");
    assert_eq!(outcome.kind, "conflicted");

    let info = commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &ours,
        &[ResolvedFile {
            path: "page.md".to_string(),
            content: None,
        }],
        "Merge branch 'feature'",
        None,
        None,
        None,
    )
    .expect("commit_merge should succeed");

    assert_eq!(info.parent_hashes.len(), 2);
    assert!(
        !repo_path.join("page.md").exists(),
        "resolving as a deletion must remove the page from the working tree"
    );

    let repo = Repository::open(&repo_path).unwrap();
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(
        tree.get_path(Path::new("page.md")).is_err(),
        "and from the merge commit's tree"
    );
    assert!(tree.get_path(Path::new("keep.md")).is_ok());
    assert!(status_paths(&repo_path).is_empty());
    assert_eq!(repo.state(), RepositoryState::Clean);
}

#[test]
#[serial_test::serial]
fn test_merge_refuses_to_delete_a_page_with_unsaved_edits() {
    let (_tmp, repo_path) = setup_test_repo();
    write_file(&repo_path, "gone.md", "gone\n");
    write_and_commit(&repo_path, "page.md", "base\n", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    std::fs::remove_file(repo_path.join("gone.md")).expect("delete gone.md");
    commit_all(&repo_path, "their deletion");

    checkout_default(&repo_path);
    let ours = write_and_commit(&repo_path, "extra.md", "extra\n", "our unrelated commit");

    // The writer is still editing the page their side deleted.
    write_file(&repo_path, "gone.md", "unsaved draft\n");

    match merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None) {
        Err(GitError::UnstagedChangesWouldBeLost { files }) => {
            assert_eq!(files, vec!["gone.md".to_string()]);
        }
        other => panic!("expected UnstagedChangesWouldBeLost, got {:?}", other),
    }

    assert_eq!(head_hash(&repo_path), ours);
    assert_eq!(
        read_file(&repo_path, "gone.md"),
        "unsaved draft\n",
        "the page the merge wanted to delete must still be there, unchanged"
    );
}

#[test]
#[serial_test::serial]
fn test_merge_deletes_a_page_the_writer_also_deleted() {
    let (_tmp, repo_path) = setup_test_repo();
    write_file(&repo_path, "gone.md", "gone\n");
    write_and_commit(&repo_path, "page.md", "base\n", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    std::fs::remove_file(repo_path.join("gone.md")).expect("delete gone.md");
    commit_all(&repo_path, "their deletion");

    checkout_default(&repo_path);
    write_and_commit(&repo_path, "extra.md", "extra\n", "our unrelated commit");

    // The writer deleted it too, without committing. The merge agrees; there
    // is nothing to lose and nothing to refuse.
    std::fs::remove_file(repo_path.join("gone.md")).expect("delete gone.md locally");

    let outcome = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");
    assert_eq!(outcome.kind, "merged");
    assert!(!repo_path.join("gone.md").exists());
}

// ===== committer identity, and a merge with somewhere to put it =====

/// `((author name, author email), (committer name, committer email))`.
fn signatures_of(repo_path: &Path, hash: &str) -> ((String, String), (String, String)) {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let commit = repo
        .find_commit(git2::Oid::from_str(hash).expect("valid oid"))
        .expect("Failed to find commit");
    let author = commit.author();
    let committer = commit.committer();
    (
        (
            author.name().unwrap().to_string(),
            author.email().unwrap().to_string(),
        ),
        (
            committer.name().unwrap().to_string(),
            committer.email().unwrap().to_string(),
        ),
    )
}

fn merge_as(committer_name: &str, committer_email: &str, no_fast_forward: bool) -> MergeOptions {
    MergeOptions {
        committer_name: Some(committer_name.to_string()),
        committer_email: Some(committer_email.to_string()),
        no_fast_forward: Some(no_fast_forward),
    }
}

#[test]
#[serial_test::serial]
fn test_merge_records_the_committer_separately_from_the_author() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B", "their file");

    checkout_default(&repo_path);
    write_and_commit(&repo_path, "d.txt", "D", "our file");

    let outcome = merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        Some("The PM"),
        Some("pm@example.com"),
        Some(&merge_as("gantry", "gantry@example.com", false)),
    )
    .expect("merge should succeed");

    assert_eq!(outcome.kind, "merged");
    let hash = outcome
        .commit_hash
        .expect("a merged outcome carries a hash");
    let (author, committer) = signatures_of(&repo_path, &hash);
    assert_eq!(author, ("The PM".to_string(), "pm@example.com".to_string()));
    assert_eq!(
        committer,
        ("gantry".to_string(), "gantry@example.com".to_string())
    );
}

#[test]
#[serial_test::serial]
fn test_merge_no_fast_forward_writes_a_merge_commit_under_the_given_identity() {
    let (_tmp, repo_path) = setup_test_repo();
    let base = write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B", "agent change");
    let feature_tip = branch_tip(&repo_path, "feature");

    checkout_default(&repo_path);

    // Nothing has moved on the target branch, so this would fast-forward and
    // leave the agent's commit as the tip with no record of the review.
    let outcome = merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        Some("The PM"),
        Some("pm@example.com"),
        Some(&merge_as("gantry", "gantry@example.com", true)),
    )
    .expect("merge should succeed");

    assert_eq!(outcome.kind, "merged");
    let hash = outcome
        .commit_hash
        .expect("a merged outcome carries a hash");
    assert_eq!(head_hash(&repo_path), hash, "HEAD must have moved");
    assert_eq!(
        parents_of(&repo_path, &hash),
        vec![base, feature_tip],
        "HEAD must be the first parent and the merged branch the second"
    );

    let (author, committer) = signatures_of(&repo_path, &hash);
    assert_eq!(author, ("The PM".to_string(), "pm@example.com".to_string()));
    assert_eq!(
        committer,
        ("gantry".to_string(), "gantry@example.com".to_string())
    );

    // The merged work is on disk, not merely in the tree object.
    assert_eq!(read_file(&repo_path, "b.txt"), "B");
    assert!(
        status_paths(&repo_path).is_empty(),
        "index and working tree must agree with the new HEAD"
    );
    let repo = Repository::open(&repo_path).unwrap();
    assert_eq!(repo.state(), RepositoryState::Clean);
}

#[test]
#[serial_test::serial]
fn test_merge_fast_forwards_when_not_asked_to_refuse() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B", "their file");
    let feature_tip = branch_tip(&repo_path, "feature");

    checkout_default(&repo_path);

    let outcome = merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        None,
        None,
        Some(&merge_as("gantry", "gantry@example.com", false)),
    )
    .expect("merge should succeed");

    assert_eq!(
        outcome.kind, "fast-forwarded",
        "noFastForward: false must leave the default alone"
    );
    assert_eq!(outcome.commit_hash, Some(feature_tip));
}

#[test]
#[serial_test::serial]
fn test_merge_no_fast_forward_leaves_an_up_to_date_merge_alone() {
    let (_tmp, repo_path) = setup_test_repo();
    let base = write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature"); // feature == HEAD

    let outcome = merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        Some("The PM"),
        Some("pm@example.com"),
        Some(&merge_as("gantry", "gantry@example.com", true)),
    )
    .expect("merge should succeed");

    assert_eq!(
        outcome.kind, "up-to-date",
        "there is nothing to merge, so there is nothing to attribute"
    );
    assert_eq!(outcome.commit_hash, Some(base));
    assert_eq!(commit_count(&repo_path), 1, "no new commit may be created");
}

#[test]
#[serial_test::serial]
fn test_commit_merge_records_the_committer_separately_from_the_author() {
    let (_tmp, repo_path) = setup_test_repo();
    conflicted_with_clean_neighbours(&repo_path);

    let ours = head_hash(&repo_path);
    merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed");

    let info = commit_merge_impl(
        repo_path.to_str().unwrap(),
        "feature",
        &ours,
        &[ResolvedFile {
            path: "page.md".to_string(),
            content: Some("ours and theirs, reconciled\n".to_string()),
        }],
        "Merge branch 'feature'",
        Some("The PM"),
        Some("pm@example.com"),
        Some(&CommitOptions {
            committer_name: Some("gantry".to_string()),
            committer_email: Some("gantry@example.com".to_string()),
        }),
    )
    .expect("commit_merge should succeed");

    let (author, committer) = signatures_of(&repo_path, &info.hash);
    assert_eq!(author, ("The PM".to_string(), "pm@example.com".to_string()));
    assert_eq!(
        committer,
        ("gantry".to_string(), "gantry@example.com".to_string())
    );
    assert_eq!(
        info.author_name, "The PM",
        "CommitInfo must report the author it was given"
    );
    assert_eq!(
        info.committer_name, "gantry",
        "a committer that cannot be read back is a committer that was not recorded"
    );
    assert_eq!(info.committer_email, "gantry@example.com");
}

// ===== cherry_pick =====
//
// The landing that keeps history linear: replay one commit onto an advanced
// HEAD, and fail outright the moment the no-collision assumption is wrong.
// Same stance as the rest of this module — a conflict is detected, never
// resolved, and nothing is written when one is found.

/// `commit_all`, but signed by someone in particular, so a test can tell the
/// original author apart from whoever performed the cherry-pick.
fn commit_all_as(repo_path: &Path, message: &str, name: &str, email: &str) -> String {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let mut index = repo.index().expect("Failed to get index");
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .expect("Failed to add all");
    index
        .update_all(["*"].iter(), None)
        .expect("Failed to update all");
    index.write().expect("Failed to write index");

    let tree_id = index.write_tree().expect("Failed to write tree");
    let tree = repo.find_tree(tree_id).expect("Failed to find tree");
    let signature = git2::Signature::now(name, email).expect("Failed to create signature");
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<_> = parent.iter().collect();

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )
    .expect("Failed to create commit")
    .to_string()
}

/// The shape cherry-pick exists for: a session branch with one commit, and a
/// scheduled job that advanced the default branch meanwhile, touching a
/// different file. Returns the session commit.
fn diverged_without_collision(repo_path: &Path) -> String {
    write_and_commit(repo_path, "a.txt", "A", "base");
    create_branch(repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_file(repo_path, "session.md", "session work\n");
    let picked = commit_all_as(
        repo_path,
        "session work",
        "The Writer",
        "writer@example.com",
    );

    checkout_default(repo_path);
    write_and_commit(repo_path, "scheduled.md", "job\n", "scheduled job");

    picked
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_replays_a_commit_onto_an_advanced_head() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_without_collision(&repo_path);
    let ours = head_hash(&repo_path);

    let hash = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect("cherry_pick should succeed");

    assert_eq!(
        parents_of(&repo_path, &hash),
        vec![ours],
        "a cherry-pick has one parent — that is the whole point of it"
    );
    assert_eq!(head_hash(&repo_path), hash, "HEAD must have moved");

    // Both the replayed work and what HEAD already had are on disk.
    assert_eq!(read_file(&repo_path, "session.md"), "session work\n");
    assert_eq!(read_file(&repo_path, "scheduled.md"), "job\n");
    assert_eq!(read_file(&repo_path, "a.txt"), "A");

    assert!(
        status_paths(&repo_path).is_empty(),
        "index and working tree must agree with the new HEAD"
    );
    let repo = Repository::open(&repo_path).unwrap();
    assert_eq!(repo.state(), RepositoryState::Clean);
    assert!(!repo_path.join(".git/CHERRY_PICK_HEAD").exists());
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_preserves_the_author_and_message_and_records_the_caller() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_without_collision(&repo_path);

    let hash = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect("cherry_pick should succeed");

    let (author, committer) = signatures_of(&repo_path, &hash);
    assert_eq!(
        author,
        ("The Writer".to_string(), "writer@example.com".to_string()),
        "the change still belongs to whoever wrote it"
    );
    assert_eq!(
        committer,
        ("gantry".to_string(), "gantry@example.com".to_string()),
        "the replay was performed by the caller"
    );

    let repo = Repository::open(&repo_path).unwrap();
    let commit = repo
        .find_commit(git2::Oid::from_str(&hash).unwrap())
        .unwrap();
    assert_eq!(commit.message().unwrap(), "session work");
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_conflict_leaves_the_repository_byte_identical() {
    let (_tmp, repo_path) = setup_test_repo();
    diverged_on_page(&repo_path);
    // The feature tip: both sides edited page.md, so replaying it collides.
    let picked = branch_tip(&repo_path, "feature");

    let before_worktree = snapshot_worktree(&repo_path);
    let before_git = snapshot_git_dir(&repo_path);

    let error = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect_err("a colliding cherry-pick must fail");

    match error {
        GitError::MergeConflict { files } => assert_eq!(files, vec!["page.md".to_string()]),
        other => panic!("expected MergeConflict, got {:?}", other),
    }

    assert_eq!(snapshot_worktree(&repo_path), before_worktree);
    assert_eq!(snapshot_git_dir(&repo_path), before_git);
    assert!(!repo_path.join(".git/CHERRY_PICK_HEAD").exists());
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_refuses_a_merge_commit() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");
    create_branch(&repo_path, "feature");
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B", "their file");
    checkout_default(&repo_path);
    write_and_commit(&repo_path, "d.txt", "D", "our file");

    let merge_commit = merge_impl(repo_path.to_str().unwrap(), "feature", None, None, None)
        .expect("merge should succeed")
        .commit_hash
        .expect("a merged outcome carries a hash");

    create_branch(&repo_path, "later");
    checkout_branch_impl(repo_path.to_str().unwrap(), "later").expect("checkout later");

    let error = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &merge_commit,
        "gantry",
        "gantry@example.com",
    )
    .expect_err("a merge commit has no single change to replay");

    match error {
        GitError::InvalidArgument { argument, .. } => assert_eq!(argument, "commit_hash"),
        other => panic!("expected InvalidArgument, got {:?}", other),
    }
}

/// As `diverged_without_collision`, but the session commit edits a page that
/// already existed, so the replay's change set contains a tracked path.
fn diverged_editing_a_tracked_page(repo_path: &Path) -> String {
    write_and_commit(repo_path, "page.md", "base\n", "base");
    create_branch(repo_path, "feature");

    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_file(repo_path, "page.md", "session work\n");
    let picked = commit_all_as(
        repo_path,
        "session work",
        "The Writer",
        "writer@example.com",
    );

    checkout_default(repo_path);
    write_and_commit(repo_path, "scheduled.md", "job\n", "scheduled job");

    picked
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_refuses_when_a_path_it_would_change_is_dirty() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_editing_a_tracked_page(&repo_path);

    // The replay rewrites page.md, and the writer has unsaved edits there.
    write_file(&repo_path, "page.md", "unsaved draft\n");
    let head_before = head_hash(&repo_path);

    let error = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect_err("a dirty path in the change set must stop the replay");

    match error {
        GitError::UnstagedChangesWouldBeLost { files } => {
            assert_eq!(files, vec!["page.md".to_string()])
        }
        other => panic!("expected UnstagedChangesWouldBeLost, got {:?}", other),
    }

    assert_eq!(head_hash(&repo_path), head_before, "HEAD must not move");
    assert_eq!(read_file(&repo_path, "page.md"), "unsaved draft\n");
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_names_the_untracked_files_in_the_way_and_writes_nothing() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_without_collision(&repo_path);

    // An untracked file standing exactly where the replay wants to write. It
    // has no committed history, so there are no unsaved changes to lose — what
    // it stands to lose is itself, and the remedy is to move it rather than to
    // save or discard edits. The refusal must also cost nothing: the commit
    // object was created before the checkout and is left unreferenced for gc,
    // and the ref never moved.
    write_file(&repo_path, "session.md", "unsaved draft\n");
    let head_before = head_hash(&repo_path);

    let error = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect_err("an untracked file in the way must stop the replay");

    match error {
        GitError::UntrackedFilesWouldBeOverwritten { files } => {
            assert_eq!(files, vec!["session.md".to_string()])
        }
        other => panic!("expected UntrackedFilesWouldBeOverwritten, got {:?}", other),
    }

    assert_eq!(head_hash(&repo_path), head_before, "HEAD must not move");
    assert_eq!(
        read_file(&repo_path, "session.md"),
        "unsaved draft\n",
        "the writer's unsaved page must survive untouched"
    );
    let repo = Repository::open(&repo_path).unwrap();
    assert_eq!(repo.state(), RepositoryState::Clean);
    assert!(!repo_path.join(".git/CHERRY_PICK_HEAD").exists());
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_ignores_a_dirty_file_outside_its_change_set() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_without_collision(&repo_path);

    // An unrelated page the writer has open must not block the replay.
    write_file(&repo_path, "a.txt", "an open draft\n");

    cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect("cherry_pick should succeed");

    assert_eq!(read_file(&repo_path, "session.md"), "session work\n");
    assert_eq!(
        read_file(&repo_path, "a.txt"),
        "an open draft\n",
        "the unrelated draft must survive untouched"
    );
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_of_an_already_applied_commit_is_nothing_to_commit() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_without_collision(&repo_path);

    cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect("the first replay should succeed");
    let head_before = head_hash(&repo_path);

    let error = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect_err("replaying a commit already applied changes nothing");

    match error {
        GitError::NothingToCommit => {}
        other => panic!("expected NothingToCommit, got {:?}", other),
    }
    assert_eq!(head_hash(&repo_path), head_before, "HEAD must not move");
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_rejects_an_abbreviated_hash_rather_than_zero_filling_it() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_without_collision(&repo_path);

    let error = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked[..8],
        "gantry",
        "gantry@example.com",
    )
    .expect_err("an abbreviated hash must not be zero-filled into another oid");

    match error {
        GitError::InvalidCommitHash { hash } => assert_eq!(hash, picked[..8].to_string()),
        other => panic!("expected InvalidCommitHash, got {:?}", other),
    }
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_refuses_on_a_detached_head() {
    let (_tmp, repo_path) = setup_test_repo();
    let picked = diverged_without_collision(&repo_path);

    let repo = Repository::open(&repo_path).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap().id();
    repo.set_head_detached(head).expect("detach HEAD");
    drop(repo);

    let error = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &picked,
        "gantry",
        "gantry@example.com",
    )
    .expect_err("a detached HEAD has no branch to land on");

    match error {
        GitError::DetachedHead => {}
        other => panic!("expected DetachedHead, got {:?}", other),
    }
}

#[test]
#[serial_test::serial]
fn test_cherry_pick_replays_a_root_commit() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A", "base");

    // An orphan branch has a root commit — no parent to diff against. libgit2
    // takes the empty tree as the base, so the whole commit is the change.
    let repo = Repository::open(&repo_path).unwrap();
    repo.set_head("refs/heads/orphan")
        .expect("point HEAD at an orphan");
    drop(repo);
    std::fs::remove_file(repo_path.join("a.txt")).expect("clear the working tree");
    write_file(&repo_path, "root.md", "root\n");
    let root = commit_all_as(
        &repo_path,
        "orphan root",
        "The Writer",
        "writer@example.com",
    );

    checkout_default(&repo_path);

    let hash = cherry_pick_impl(
        repo_path.to_str().unwrap(),
        &root,
        "gantry",
        "gantry@example.com",
    )
    .expect("a parentless commit is still one change to replay");

    assert_eq!(read_file(&repo_path, "root.md"), "root\n");
    assert_eq!(
        read_file(&repo_path, "a.txt"),
        "A",
        "replaying an orphan root must not delete what HEAD already had"
    );
    assert_eq!(parents_of(&repo_path, &hash).len(), 1);
}
