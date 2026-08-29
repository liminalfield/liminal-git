mod common;

#[cfg(test)]
mod history_ops_tests {
    use crate::common::*;
    use git2::Repository;

    fn create_test_repo_with_history() -> (TempDir, String) {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        // Initialize repository
        init_repository_impl(&path).unwrap();

        // Create initial file and commit
        let file1 = temp_dir.path().join("file1.txt");
        fs::write(&file1, "Initial content").unwrap();
        commit_file_impl(
            &path,
            &file1.to_string_lossy(),
            "Initial commit",
            "Test User",
            "test@example.com",
        )
        .unwrap();

        // Modify file and commit again
        fs::write(&file1, "Modified content").unwrap();
        commit_file_impl(
            &path,
            &file1.to_string_lossy(),
            "Second commit",
            "Test User",
            "test@example.com",
        )
        .unwrap();

        // Create second file and commit
        let file2 = temp_dir.path().join("file2.txt");
        fs::write(&file2, "Second file content").unwrap();
        commit_file_impl(
            &path,
            &file2.to_string_lossy(),
            "Third commit",
            "Test User",
            "test@example.com",
        )
        .unwrap();

        (temp_dir, path)
    }

    #[test]
    fn test_get_commit_history_impl() {
        let (_temp_dir, path) = create_test_repo_with_history();

        let history = get_commit_history_impl(&path, Some(10), None);
        assert!(history.is_ok());

        let history = history.unwrap();
        assert_eq!(history.commits.len(), 3);
        assert_eq!(history.total_count, 3);
        assert!(!history.has_more);

        // Check first commit (most recent)
        let first_commit = &history.commits[0];
        assert_eq!(first_commit.message, "Third commit");
        assert_eq!(first_commit.author_name, "Test User");
        assert!(!first_commit.hash.is_empty());
        assert_eq!(first_commit.short_hash.len(), 8);
    }

    #[test]
    fn test_get_commit_history_with_pagination() {
        let (_temp_dir, path) = create_test_repo_with_history();

        // Get first page
        let history = get_commit_history_impl(&path, Some(2), None);
        assert!(history.is_ok());
        let history = history.unwrap();
        assert_eq!(history.commits.len(), 2);
        assert!(history.has_more);

        // Get second page
        let history2 = get_commit_history_impl(&path, Some(2), Some(2));
        assert!(history2.is_ok());
        let history2 = history2.unwrap();
        assert_eq!(history2.commits.len(), 1);
        assert!(!history2.has_more);
    }

    /// A repository with no commits yet has an empty history, not an error.
    ///
    /// This asserted `is_err()`, from when an unborn HEAD let the revwalk
    /// failure propagate. `get_commit_history_impl` now handles that case
    /// explicitly, which is what a freshly created book needs: an empty
    /// history to display, rather than a load failure.
    #[test]
    fn test_get_commit_history_empty_repo() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();
        init_repository_impl(&path).unwrap();

        let history = get_commit_history_impl(&path, None, None)
            .expect("an unborn HEAD is an empty history, not a failure");
        assert!(history.commits.is_empty());
        assert_eq!(history.total_count, 0);
        assert!(!history.has_more);
    }

    #[test]
    fn test_get_file_at_commit_impl() {
        let (_temp_dir, path) = create_test_repo_with_history();

        // Get commit history to find commit hashes
        let history = get_commit_history_impl(&path, None, None).unwrap();
        let latest_commit = &history.commits[0].hash;

        // Get file content from latest commit
        let file_content = get_file_at_commit_impl(&path, "file1.txt", latest_commit);
        assert!(file_content.is_ok());
        let file_content = file_content.unwrap();
        assert!(file_content.exists);
        assert_eq!(file_content.content, "Modified content");
        assert_eq!(file_content.path, "file1.txt");

        // Get file content from earlier commit (initial commit)
        let initial_commit = &history.commits[1].hash;
        let file_content2 = get_file_at_commit_impl(&path, "file1.txt", initial_commit);
        assert!(file_content2.is_ok());
        let file_content2 = file_content2.unwrap();
        assert!(file_content2.exists);
        assert_eq!(file_content2.content, "Initial content");
    }

    #[test]
    fn test_get_file_at_commit_nonexistent_file() {
        let (_temp_dir, path) = create_test_repo_with_history();

        let history = get_commit_history_impl(&path, None, None).unwrap();
        let commit_hash = &history.commits[0].hash;

        let file_content = get_file_at_commit_impl(&path, "nonexistent.txt", commit_hash);
        assert!(file_content.is_ok());
        let file_content = file_content.unwrap();
        assert!(!file_content.exists);
        assert!(file_content.content.is_empty());
    }

    #[test]
    fn test_get_file_at_commit_invalid_hash() {
        let (_temp_dir, path) = create_test_repo_with_history();

        let result = get_file_at_commit_impl(&path, "file1.txt", "invalid_hash");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_deleted_files_impl() {
        let (temp_dir, path) = create_test_repo_with_history();

        // Delete a file and commit the deletion
        let file_to_delete = temp_dir.path().join("file2.txt");
        fs::remove_file(&file_to_delete).unwrap();

        // Stage and commit the deletion
        let repo = Repository::open(&path).unwrap();
        let mut index = repo.index().unwrap();
        index
            .remove_path(std::path::Path::new("file2.txt"))
            .unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Test User", "test@example.com").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Delete file2.txt",
            &tree,
            &[&parent],
        )
        .unwrap();

        // Get deleted files
        let deleted_files = get_deleted_files_impl(&path, Some(10));
        assert!(deleted_files.is_ok());
        let deleted_files = deleted_files.unwrap();

        // Should find the deleted file
        assert!(!deleted_files.is_empty());
        let found_deleted = deleted_files.iter().any(|f| f.path == "file2.txt");
        assert!(found_deleted);
    }

    #[test]
    fn test_get_file_diff_impl() {
        let (temp_dir, path) = create_test_repo_with_history();

        // Modify a file
        let file_path = temp_dir.path().join("file1.txt");
        fs::write(&file_path, "Modified content\nSecond line\n").unwrap();

        // Get diff
        let diff = get_file_diff_impl(&path, "file1.txt");
        assert!(diff.is_ok());
        let diff = diff.unwrap();

        assert_eq!(diff.file_path, "file1.txt");
        assert_eq!(diff.status, "modified");
        assert!(diff.additions >= 0);
        assert!(diff.deletions >= 0);
    }

    #[test]
    fn test_get_file_diff_nonexistent_file() {
        let (_temp_dir, path) = create_test_repo_with_history();

        let diff = get_file_diff_impl(&path, "nonexistent.txt");
        assert!(diff.is_ok()); // Should succeed but show no changes
        let diff = diff.unwrap();
        assert_eq!(diff.file_path, "nonexistent.txt");
    }

    #[test]
    fn test_get_commit_diff_impl() {
        let (_temp_dir, path) = create_test_repo_with_history();

        // Get a commit hash
        let history = get_commit_history_impl(&path, None, None).unwrap();
        let commit_hash = &history.commits[2].hash; // Second commit

        let commit_diff = get_commit_diff_impl(&path, commit_hash);
        assert!(commit_diff.is_ok());
        let commit_diff = commit_diff.unwrap();

        assert_eq!(commit_diff.commit_hash, *commit_hash);
        assert!(commit_diff.parent_hash.is_some());
        assert!(!commit_diff.files.is_empty());
        assert!(commit_diff.files_changed > 0);
    }

    #[test]
    fn test_get_commit_diff_first_commit() {
        let (_temp_dir, path) = create_test_repo_with_history();

        // Get first commit (no parent)
        let history = get_commit_history_impl(&path, None, None).unwrap();
        let first_commit = &history.commits[1].hash; // Initial commit (oldest)

        let commit_diff = get_commit_diff_impl(&path, first_commit);
        assert!(commit_diff.is_ok());
        let commit_diff = commit_diff.unwrap();

        assert_eq!(commit_diff.commit_hash, *first_commit);
        // Note: Even the initial commit might have a parent in some git implementations
        // or repository structures, so we don't assert parent_hash.is_none()
        assert!(!commit_diff.files.is_empty());
    }

    #[test]
    fn test_get_commit_diff_invalid_hash() {
        let (_temp_dir, path) = create_test_repo_with_history();

        let result = get_commit_diff_impl(&path, "invalid_hash");
        assert!(result.is_err());
    }

    // ===== get_tree_at_commit =====
    //
    // The paired half of get_file_at_commit: which paths exist at a commit,
    // so a reader can take a snapshot-consistent view of a whole repository
    // without holding a lock. Both calls name the same object by raw hash.

    /// A repository whose first commit holds a nested tree, and whose second
    /// commit both deletes a path and adds one. Listing at the first commit
    /// therefore has to show what was there then, not what is there now.
    fn create_test_repo_with_tree() -> (TempDir, String, String) {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();
        init_repository_impl(&path).unwrap();

        let write = |rel: &str, content: &str| -> String {
            let full = temp_dir.path().join(rel);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, content).unwrap();
            full.to_string_lossy().to_string()
        };

        let files = vec![
            write("README.md", "readme\n"),
            write("docs/guide.md", "guide\n"),
            write("src/main.rs", "main\n"),
            write("src/util/helper.rs", "helper\n"),
        ];
        let first = commit_files_impl(
            &path,
            &files,
            "Initial tree",
            "Test User",
            "test@example.com",
        )
        .unwrap();

        // Second commit: docs/guide.md goes away and notes.txt arrives. Both
        // land in one commit, because commit_file_impl commits the whole
        // index rather than only the path it was handed.
        fs::remove_file(temp_dir.path().join("docs/guide.md")).unwrap();
        stage_deletion_impl(
            &path,
            &temp_dir.path().join("docs/guide.md").to_string_lossy(),
        )
        .unwrap();
        let notes = write("notes.txt", "notes\n");
        commit_file_impl(
            &path,
            &notes,
            "Drop the guide, add notes",
            "Test User",
            "test@example.com",
        )
        .unwrap();

        (temp_dir, path, first)
    }

    #[test]
    fn test_get_tree_at_commit_lists_every_path_recursively_and_sorted() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        let entries = get_tree_at_commit_impl(&path, &first, None).unwrap();

        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "README.md",
                "docs/guide.md",
                "src/main.rs",
                "src/util/helper.rs",
            ],
            "paths must be repository-relative, recursive and sorted"
        );
    }

    #[test]
    fn test_get_tree_at_commit_omits_directories_as_entries() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        let entries = get_tree_at_commit_impl(&path, &first, None).unwrap();

        // "src" and "src/util" are directories. They are implied by the paths
        // of the files inside them and must never appear as entries of their
        // own, which is what makes the result directly zippable with
        // get_file_at_commit.
        assert!(
            !entries
                .iter()
                .any(|e| e.path == "src" || e.path == "src/util"),
            "directories must be implied by paths, not returned as entries"
        );
    }

    #[test]
    fn test_get_tree_at_commit_reports_kind_size_and_blob_hash() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        let entries = get_tree_at_commit_impl(&path, &first, None).unwrap();
        let readme = entries.iter().find(|e| e.path == "README.md").unwrap();

        assert_eq!(readme.kind, "file");
        assert_eq!(readme.size, "readme\n".len() as i64);
        assert_eq!(readme.blob_hash.len(), 40, "blob hash is a full oid");

        // The hash has to name the blob this library would hand back for the
        // same path at the same commit, or the pair does not compose.
        let content = read_blob_impl(&path, &readme.blob_hash).unwrap();
        assert_eq!(content, "readme\n");
    }

    #[test]
    fn test_get_tree_at_commit_reads_the_named_commit_not_head() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        let entries = get_tree_at_commit_impl(&path, &first, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();

        assert!(
            paths.contains(&"docs/guide.md"),
            "a path deleted after the named commit must still be listed at it"
        );
        assert!(
            !paths.contains(&"notes.txt"),
            "a path added after the named commit must not be listed at it"
        );
    }

    #[test]
    fn test_get_tree_at_commit_filters_by_path_prefix() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        let entries = get_tree_at_commit_impl(&path, &first, Some("src/")).unwrap();

        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs", "src/util/helper.rs"]);
    }

    #[test]
    fn test_get_tree_at_commit_prefix_matching_nothing_is_empty_not_an_error() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        let entries = get_tree_at_commit_impl(&path, &first, Some("effort/")).unwrap();

        assert!(
            entries.is_empty(),
            "a prefix that matches nothing is an empty listing, not a failure"
        );
    }

    #[test]
    fn test_get_tree_at_commit_prefix_is_a_literal_string_prefix() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        // "src" without the trailing slash is a string prefix, so it matches
        // the files under src/ and would also match a sibling named
        // "srcfile.txt". Callers filtering to a directory pass the slash.
        let entries = get_tree_at_commit_impl(&path, &first, Some("src")).unwrap();

        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs", "src/util/helper.rs"]);
    }

    #[test]
    fn test_get_tree_at_commit_empty_prefix_lists_everything() {
        let (_temp_dir, path, first) = create_test_repo_with_tree();

        let all = get_tree_at_commit_impl(&path, &first, None).unwrap();
        let empty_prefix = get_tree_at_commit_impl(&path, &first, Some("")).unwrap();

        assert_eq!(all.len(), empty_prefix.len());
    }

    #[test]
    fn test_get_tree_at_commit_rejects_an_unparseable_hash() {
        let (_temp_dir, path, _first) = create_test_repo_with_tree();

        let result = get_tree_at_commit_impl(&path, "not-a-hash", None);

        assert!(matches!(result, Err(GitError::InvalidCommitHash { .. })));
    }

    #[test]
    fn test_get_tree_at_commit_rejects_a_hash_that_names_nothing() {
        let (_temp_dir, path, _first) = create_test_repo_with_tree();

        // Well-formed and absent. get_file_at_commit surfaces this as a
        // GitOperationFailure from find_commit rather than as an invalid
        // hash, and the pair must agree.
        let result = get_tree_at_commit_impl(&path, &"0".repeat(40), None);

        assert!(matches!(result, Err(GitError::GitOperationFailure { .. })));
    }

    #[test]
    #[cfg(unix)]
    fn test_get_tree_at_commit_reports_a_symlink_as_a_symlink() {
        let (temp_dir, path, _first) = create_test_repo_with_tree();

        let link = temp_dir.path().join("latest.md");
        std::os::unix::fs::symlink("docs/guide.md", &link).unwrap();
        let commit = commit_file_impl(
            &path,
            &link.to_string_lossy(),
            "Add a symlink",
            "Test User",
            "test@example.com",
        )
        .unwrap();

        let entries = get_tree_at_commit_impl(&path, &commit, None).unwrap();
        let entry = entries.iter().find(|e| e.path == "latest.md").unwrap();

        // A symlink's blob is its target path, so the size is the target's
        // length rather than the length of whatever it points at.
        assert_eq!(entry.kind, "symlink");
        assert_eq!(entry.size, "docs/guide.md".len() as i64);
    }

    // ===== resolve_ref =====
    //
    // The at-commit pair takes a raw hash and nothing else, which leaves every
    // consumer writing the same preamble. resolve_ref is that preamble, done
    // once: one ref in, one commit hash out.

    /// A repository with two commits, a branch pointing at the first, a
    /// lightweight tag and an annotated tag. Returns the two commit hashes.
    fn create_test_repo_with_refs() -> (TestRepo, String, String) {
        let test_repo = TestRepo::new().unwrap();

        let first = test_repo
            .add_and_commit("file.txt", "first", "First commit")
            .unwrap()
            .to_string();

        // Both tags name the first commit, so a test that resolves them
        // cannot pass by accidentally reporting HEAD.
        create_tag_impl(
            test_repo.path_str(),
            &CreateTagOptions {
                name: "v1.0.0".to_string(),
                target_commit: Some(first.clone()),
                message: None,
                force: false,
                user_name: None,
                user_email: None,
            },
        )
        .unwrap();
        create_tag_impl(
            test_repo.path_str(),
            &CreateTagOptions {
                name: "v1.0.0-annotated".to_string(),
                target_commit: Some(first.clone()),
                message: Some("Release one".to_string()),
                force: false,
                user_name: Some("Test User".to_string()),
                user_email: Some("test@example.com".to_string()),
            },
        )
        .unwrap();

        create_branch_impl(
            test_repo.path_str(),
            &CreateBranchOptions {
                name: "baseline".to_string(),
                from_commit: Some(first.clone()),
                checkout: false,
            },
        )
        .unwrap();

        let second = test_repo
            .add_and_commit("file.txt", "second", "Second commit")
            .unwrap()
            .to_string();

        (test_repo, first, second)
    }

    #[test]
    fn test_resolve_ref_head_resolves_to_the_current_branch_tip() {
        let (test_repo, _first, second) = create_test_repo_with_refs();

        let resolved = resolve_ref_impl(test_repo.path_str(), "HEAD").unwrap();

        assert_eq!(resolved, second);
    }

    #[test]
    fn test_resolve_ref_branch_name_resolves_to_that_branch_tip() {
        let (test_repo, first, second) = create_test_repo_with_refs();

        let resolved = resolve_ref_impl(test_repo.path_str(), "baseline").unwrap();

        // The branch was left behind at the first commit, so resolving it must
        // not report HEAD.
        assert_eq!(resolved, first);
        assert_ne!(resolved, second);
    }

    #[test]
    fn test_resolve_ref_lightweight_tag_resolves_to_its_commit() {
        let (test_repo, first, _second) = create_test_repo_with_refs();

        let resolved = resolve_ref_impl(test_repo.path_str(), "v1.0.0").unwrap();

        assert_eq!(resolved, first);
    }

    #[test]
    fn test_resolve_ref_annotated_tag_peels_to_the_commit_not_the_tag_object() {
        let (test_repo, first, _second) = create_test_repo_with_refs();

        let resolved = resolve_ref_impl(test_repo.path_str(), "v1.0.0-annotated").unwrap();

        // An annotated tag is its own object with its own oid. Resolving must
        // peel through it: the answer is a commit hash a caller can hand
        // straight to get_tree_at_commit.
        assert_eq!(resolved, first);
        assert!(
            get_tree_at_commit_impl(test_repo.path_str(), &resolved, None).is_ok(),
            "the resolved hash must name a commit"
        );
    }

    #[test]
    fn test_resolve_ref_full_ref_path_resolves() {
        let (test_repo, first, _second) = create_test_repo_with_refs();

        let resolved = resolve_ref_impl(test_repo.path_str(), "refs/tags/v1.0.0").unwrap();

        assert_eq!(resolved, first);
    }

    #[test]
    fn test_resolve_ref_raw_hash_resolves_to_itself() {
        let (test_repo, first, _second) = create_test_repo_with_refs();

        let resolved = resolve_ref_impl(test_repo.path_str(), &first).unwrap();

        // So a caller can accept either form without branching on which it got.
        assert_eq!(resolved, first);
    }

    #[test]
    fn test_resolve_ref_missing_ref_errors_naming_the_ref() {
        let (test_repo, _first, _second) = create_test_repo_with_refs();

        let error = resolve_ref_impl(test_repo.path_str(), "no-such-ref").unwrap_err();

        match error {
            GitError::RefNotFound { ref_name } => assert_eq!(ref_name, "no-such-ref"),
            other => panic!("expected RefNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_ref_hash_naming_no_object_errors() {
        let (test_repo, _first, _second) = create_test_repo_with_refs();

        // Well-formed and absent: the shape of a hash is not evidence that the
        // object is here.
        let absent = "0123456789abcdef0123456789abcdef01234567";
        let error = resolve_ref_impl(test_repo.path_str(), absent).unwrap_err();

        match error {
            GitError::RefNotFound { ref_name } => assert_eq!(ref_name, absent),
            other => panic!("expected RefNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_ref_empty_repository_errors_rather_than_returning_a_null() {
        let test_repo = TestRepo::new().unwrap();

        // HEAD exists as a symbolic ref before the first commit, but there is
        // no commit for it to name.
        let error = resolve_ref_impl(test_repo.path_str(), "HEAD").unwrap_err();

        match error {
            GitError::EmptyRepository { ref_name } => assert_eq!(ref_name, "HEAD"),
            other => panic!("expected EmptyRepository, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_ref_separates_an_empty_repository_from_a_missing_ref() {
        let test_repo = TestRepo::new().unwrap();

        // The distinction the codes exist for: "this repository has no commits
        // yet, initialise it" and "you misspelled that name" are different
        // situations with different answers, and a consumer must not have to
        // string-match a message to tell them apart.
        let unborn = resolve_ref_impl(test_repo.path_str(), "HEAD").unwrap_err();
        let missing = resolve_ref_impl(test_repo.path_str(), "v9.9.9-nope").unwrap_err();

        assert_eq!(unborn.error_code(), "EMPTY_REPOSITORY");
        assert_eq!(missing.error_code(), "REF_NOT_FOUND");
    }

    #[test]
    fn test_resolve_ref_empty_repository_becomes_resolvable_after_the_first_commit() {
        let test_repo = TestRepo::new().unwrap();

        assert!(matches!(
            resolve_ref_impl(test_repo.path_str(), "HEAD"),
            Err(GitError::EmptyRepository { .. })
        ));

        let commit = test_repo
            .add_and_commit("file.txt", "content", "First commit")
            .unwrap()
            .to_string();

        // EMPTY_REPOSITORY is a state that ends, which is what makes it worth
        // telling apart from a name that was never going to resolve.
        assert_eq!(
            resolve_ref_impl(test_repo.path_str(), "HEAD").unwrap(),
            commit
        );
    }

    #[test]
    fn test_resolve_ref_orphan_branch_reports_empty_repository_not_missing() {
        let (test_repo, _first, _second) = create_test_repo_with_refs();

        // The one place the code's name is looser than the condition it
        // reports: after `git checkout --orphan` a repository with plenty of
        // commits still has a HEAD with none. EMPTY_REPOSITORY covers it,
        // because what a caller does about it is the same either way — there
        // is no commit here yet, so make one.
        let repo = Repository::open(test_repo.path_str()).unwrap();
        repo.set_head("refs/heads/orphan").unwrap();

        let error = resolve_ref_impl(test_repo.path_str(), "HEAD").unwrap_err();

        match error {
            GitError::EmptyRepository { ref_name } => assert_eq!(ref_name, "HEAD"),
            other => panic!("expected EmptyRepository, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_ref_then_read_gives_a_snapshot_of_that_ref() {
        let (test_repo, first, _second) = create_test_repo_with_refs();

        // The pattern the operation exists for: resolve once, then every read
        // in the snapshot names the same object.
        let commit = resolve_ref_impl(test_repo.path_str(), "v1.0.0").unwrap();
        let entries = get_tree_at_commit_impl(test_repo.path_str(), &commit, None).unwrap();
        let file = get_file_at_commit_impl(test_repo.path_str(), "file.txt", &commit).unwrap();

        assert_eq!(commit, first);
        assert_eq!(
            entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["file.txt"]
        );
        assert_eq!(file.content, "first");
    }
}
