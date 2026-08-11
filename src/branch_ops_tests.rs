use super::*;
use git2::Repository;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn setup_test_repo() -> (TempDir, PathBuf) {
    // Create temp directory and ensure git2's internal temp files use the same filesystem
    // to avoid "Invalid cross-device link" errors when git2 renames lock files into .git/objects
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    // Set TMPDIR to the temp directory's parent so git2 uses the same filesystem
    // for ALL git operations in this test (not just init, but also commits, checkouts, etc.)
    // We don't restore it - each test is isolated and setting TMPDIR is safe
    // Tests are marked #[serial_test::serial] to prevent concurrent TMPDIR modifications
    if let Some(parent) = temp_dir.path().parent() {
        unsafe {
            std::env::set_var("TMPDIR", parent);
            std::env::set_var("TMP", parent); // Also set TMP for Windows compatibility
            std::env::set_var("TEMP", parent);
        }
    }

    // Initialize repository with git2 now using the correct temp location
    Repository::init(&repo_path).expect("Failed to initialize test repository");

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
        std::fs::create_dir_all(parent).expect("Failed to create parent directories");
    }
    std::fs::write(&full_path, content).expect("Failed to write file");
}

fn commit_file(repo_path: &Path, file_path: &str, message: &str) -> String {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let relative_path = std::path::Path::new(file_path);

    let mut index = repo.index().expect("Failed to get index");
    index.add_path(relative_path).expect("Failed to add path");
    index.write().expect("Failed to write index");

    let tree_id = index.write_tree().expect("Failed to write tree");
    let tree = repo.find_tree(tree_id).expect("Failed to find tree");
    let signature =
        git2::Signature::now("Test User", "test@example.com").expect("Failed to create signature");

    let parent_commit = repo.head().ok().and_then(|head| {
        head.target()
            .and_then(|target| repo.find_commit(target).ok())
    });

    let commit_id = if let Some(parent) = parent_commit {
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
    } else {
        repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
    };

    commit_id.expect("Failed to create commit").to_string()
}

fn create_branch(repo_path: &Path, branch_name: &str) {
    let repo = Repository::open(repo_path).expect("Failed to open repository");
    let head = repo.head().expect("Failed to get HEAD");
    let commit = head.peel_to_commit().expect("Failed to get commit");
    repo.branch(branch_name, &commit, false)
        .expect("Failed to create branch");
}

#[test]
#[serial_test::serial]
fn test_checkout_branch_clean_tree() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Create initial commit
    create_test_file(&repo_path, "test.txt", "content");
    commit_file(&repo_path, "test.txt", "Initial commit");

    // Create a new branch
    create_branch(&repo_path, "feature");

    // Checkout should succeed with clean tree
    let result = checkout_branch_impl(repo_path.to_str().unwrap(), "feature");
    assert!(result.is_ok(), "Checkout with clean tree should succeed");

    // Verify we're on the new branch
    let repo = Repository::open(&repo_path).expect("Failed to open repository");
    let head = repo.head().expect("Failed to get HEAD");
    assert_eq!(head.shorthand().unwrap(), "feature");
}

#[test]
#[serial_test::serial]
fn test_checkout_branch_with_non_conflicting_changes() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Create initial commit
    create_test_file(&repo_path, "file1.txt", "content1");
    commit_file(&repo_path, "file1.txt", "Initial commit");

    // Create feature branch and add a different file there
    create_branch(&repo_path, "feature");
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("Checkout failed");
    create_test_file(&repo_path, "file2.txt", "content2");
    commit_file(&repo_path, "file2.txt", "Add file2");

    // Go back to main/master and modify file1 (doesn't exist in feature branch)
    checkout_branch_impl(repo_path.to_str().unwrap(), "master")
        .or_else(|_| checkout_branch_impl(repo_path.to_str().unwrap(), "main"))
        .expect("Checkout to main failed");

    create_test_file(&repo_path, "file3.txt", "content3");
    // Don't commit - leave it as uncommitted change

    // Checkout feature branch should succeed - file3.txt doesn't conflict
    let result = checkout_branch_impl(repo_path.to_str().unwrap(), "feature");
    assert!(
        result.is_ok(),
        "Checkout with non-conflicting changes should succeed: {:?}",
        result
    );

    // Verify file3.txt still exists (uncommitted changes preserved)
    let file3_path = repo_path.join("file3.txt");
    assert!(file3_path.exists(), "Uncommitted file should be preserved");
}

#[test]
#[serial_test::serial]
fn test_checkout_branch_with_conflicting_changes() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Create initial commit with shared file
    create_test_file(&repo_path, "shared.txt", "original content");
    commit_file(&repo_path, "shared.txt", "Initial commit");

    // Create feature branch and modify the shared file
    create_branch(&repo_path, "feature");
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("Checkout failed");
    create_test_file(&repo_path, "shared.txt", "feature content");
    commit_file(&repo_path, "shared.txt", "Modify shared.txt in feature");

    // Go back to main/master and modify the same file differently
    checkout_branch_impl(repo_path.to_str().unwrap(), "master")
        .or_else(|_| checkout_branch_impl(repo_path.to_str().unwrap(), "main"))
        .expect("Checkout to main failed");

    create_test_file(&repo_path, "shared.txt", "main uncommitted changes");
    // Don't commit - this creates a conflict

    // Checkout feature should fail with UnstagedChangesWouldBeLost
    let result = checkout_branch_impl(repo_path.to_str().unwrap(), "feature");
    assert!(
        result.is_err(),
        "Checkout with conflicting changes should fail"
    );

    match result {
        Err(GitError::UnstagedChangesWouldBeLost { files }) => {
            assert!(!files.is_empty(), "Should report conflicting files");
            assert!(
                files.iter().any(|f| f.contains("shared.txt")),
                "Should report shared.txt as conflicting file, got: {:?}",
                files
            );
        }
        _ => panic!(
            "Expected UnstagedChangesWouldBeLost error, got: {:?}",
            result
        ),
    }
}

#[test]
#[serial_test::serial]
fn test_checkout_branch_force_strategy_from_config() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Set liminal.checkoutStrategy to "force"
    let repo = Repository::open(&repo_path).expect("Failed to open repository");
    let mut config = repo.config().expect("Failed to get config");
    config
        .set_str("liminal.checkoutStrategy", "force")
        .expect("Failed to set config");
    drop(config);
    drop(repo);

    // Create initial commit with shared file
    create_test_file(&repo_path, "shared.txt", "original content");
    commit_file(&repo_path, "shared.txt", "Initial commit");

    // Create feature branch and modify the shared file
    create_branch(&repo_path, "feature");
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("Checkout failed");
    create_test_file(&repo_path, "shared.txt", "feature content");
    commit_file(&repo_path, "shared.txt", "Modify shared.txt in feature");

    // Go back to main/master and modify the same file differently
    checkout_branch_impl(repo_path.to_str().unwrap(), "master")
        .or_else(|_| checkout_branch_impl(repo_path.to_str().unwrap(), "main"))
        .expect("Checkout to main failed");

    create_test_file(&repo_path, "shared.txt", "main uncommitted changes");
    // Don't commit - this would normally conflict

    // Checkout feature should succeed with force strategy (overwrites local changes)
    let result = checkout_branch_impl(repo_path.to_str().unwrap(), "feature");
    assert!(
        result.is_ok(),
        "Checkout with force strategy should succeed even with conflicts: {:?}",
        result
    );

    // Verify the file was overwritten with feature branch content
    let content =
        std::fs::read_to_string(repo_path.join("shared.txt")).expect("Failed to read file");
    assert_eq!(
        content, "feature content",
        "File should be overwritten to feature branch content"
    );
}

#[test]
#[serial_test::serial]
fn test_checkout_branch_safe_strategy_default() {
    let (_temp_dir, repo_path) = setup_test_repo();

    // Don't set any config - should default to "safe"

    // Create initial commit with shared file
    create_test_file(&repo_path, "shared.txt", "original content");
    commit_file(&repo_path, "shared.txt", "Initial commit");

    // Create feature branch and modify the shared file
    create_branch(&repo_path, "feature");
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("Checkout failed");
    create_test_file(&repo_path, "shared.txt", "feature content");
    commit_file(&repo_path, "shared.txt", "Modify shared.txt in feature");

    // Go back to main/master and modify the same file differently
    checkout_branch_impl(repo_path.to_str().unwrap(), "master")
        .or_else(|_| checkout_branch_impl(repo_path.to_str().unwrap(), "main"))
        .expect("Checkout to main failed");

    create_test_file(&repo_path, "shared.txt", "main uncommitted changes");

    // Checkout should fail (safe is the default)
    let result = checkout_branch_impl(repo_path.to_str().unwrap(), "feature");
    assert!(
        result.is_err(),
        "Checkout should fail with default safe strategy"
    );

    match result {
        Err(GitError::UnstagedChangesWouldBeLost { .. }) => {
            // Expected
        }
        _ => panic!(
            "Expected UnstagedChangesWouldBeLost error, got: {:?}",
            result
        ),
    }
}

// ===== ahead/behind + real commits_ahead (#390 criterion 3) =====

/// Check out the default branch, tolerating either `master` or `main`.
fn checkout_default(repo_path: &Path) {
    checkout_branch_impl(repo_path.to_str().unwrap(), "master")
        .or_else(|_| checkout_branch_impl(repo_path.to_str().unwrap(), "main"))
        .expect("checkout default branch");
}

/// Write a file then commit it to the current branch.
fn write_and_commit(repo_path: &Path, file: &str, message: &str) {
    create_test_file(repo_path, file, message);
    commit_file(repo_path, file, message);
}

#[test]
#[serial_test::serial]
fn test_commits_ahead_of_head_counts_real_commits() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A"); // default branch = A
    create_branch(&repo_path, "feature"); // feature = A

    // Two commits on feature, then back to the default branch so HEAD = A.
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B");
    write_and_commit(&repo_path, "c.txt", "C");
    checkout_default(&repo_path);

    let repo = Repository::open(&repo_path).unwrap();
    let branch = repo
        .find_branch("feature", git2::BranchType::Local)
        .unwrap();
    // Two commits on feature are not reachable from HEAD — not the old stub of 1.
    assert_eq!(commits_ahead_of_head_impl(&repo, &branch).unwrap(), 2);
}

#[test]
#[serial_test::serial]
fn test_delete_unmerged_branch_reports_real_commits_ahead() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A");
    create_branch(&repo_path, "feature");
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "b.txt", "B");
    write_and_commit(&repo_path, "c.txt", "C");
    checkout_default(&repo_path);

    match delete_branch_impl(repo_path.to_str().unwrap(), "feature", false) {
        Err(GitError::BranchNotMerged { commits_ahead, .. }) => {
            assert_eq!(
                commits_ahead, 2,
                "reports the real ahead count, not the stub of 1"
            );
        }
        other => panic!("expected BranchNotMerged, got {:?}", other),
    }
}

#[test]
#[serial_test::serial]
fn test_calculate_ahead_behind_against_default() {
    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "a.txt", "A"); // default = A
    create_branch(&repo_path, "feature"); // feature = A

    // Advance the default one commit: feature is 0 ahead, 1 behind.
    write_and_commit(&repo_path, "d.txt", "D");
    {
        let repo = Repository::open(&repo_path).unwrap();
        let branch = repo
            .find_branch("feature", git2::BranchType::Local)
            .unwrap();
        let ab = calculate_ahead_behind_impl(&repo, &branch).unwrap();
        assert_eq!((ab.ahead, ab.behind), (0, 1));
    }

    // Advance feature two commits: now 2 ahead, 1 behind of the default.
    checkout_branch_impl(repo_path.to_str().unwrap(), "feature").expect("checkout feature");
    write_and_commit(&repo_path, "e.txt", "E");
    write_and_commit(&repo_path, "f.txt", "F");
    {
        let repo = Repository::open(&repo_path).unwrap();
        let branch = repo
            .find_branch("feature", git2::BranchType::Local)
            .unwrap();
        let ab = calculate_ahead_behind_impl(&repo, &branch).unwrap();
        assert_eq!((ab.ahead, ab.behind), (2, 1));
    }
}

// ===== per-repo mutex serialization (#390 criteria 2 + 4) =====

#[test]
#[serial_test::serial]
fn test_repo_lock_serializes_concurrent_commits() {
    use std::thread;

    let (_tmp, repo_path) = setup_test_repo();
    write_and_commit(&repo_path, "base.txt", "base"); // 1 base commit
    let path = repo_path.to_str().unwrap().to_string();

    // 8 threads each commit a distinct file to the SAME repo, serialized by the
    // per-repo lock. Without serialization the concurrent index/HEAD writes
    // would race and drop commits.
    let mut handles = Vec::new();
    for i in 0..8 {
        let path = path.clone();
        handles.push(thread::spawn(move || {
            let file = format!("f{}.txt", i);
            std::fs::write(
                std::path::Path::new(&path).join(&file),
                format!("content {}", i),
            )
            .expect("write file");

            let lock = crate::utils::repo_lock(&path);
            let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
            crate::commit_file_impl(&path, &file, &format!("commit {}", i), "T", "t@e.com")
                .expect("commit under lock");
        }));
    }
    for h in handles {
        h.join().expect("thread join");
    }

    // Serialized commits form a linear history: base + 8 = 9.
    let repo = Repository::open(&repo_path).unwrap();
    let mut walk = repo.revwalk().unwrap();
    walk.push_head().unwrap();
    assert_eq!(
        walk.count(),
        9,
        "all 8 concurrent commits serialized onto HEAD"
    );
}
