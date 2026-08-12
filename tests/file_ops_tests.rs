mod common;

#[cfg(test)]
mod file_ops_tests {
    use crate::common::*;
    use git2::{Repository, Signature};

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

    fn commit_impl(
        repo: &Repository,
        message: &str,
        name: &str,
        email: &str,
    ) -> Result<git2::Oid, Box<dyn std::error::Error>> {
        let signature = Signature::now(name, email)?;
        let mut index = repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        // Check if we have any changes by comparing with HEAD
        let head = repo.head();
        if let Ok(head_ref) = head {
            let parent_commit = repo.find_commit(head_ref.target().unwrap())?;
            let head_tree = parent_commit.tree()?;

            // Compare current tree with HEAD tree
            if tree.id() == head_tree.id() {
                return Err("No changes to commit".into());
            }

            let commit_id = repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[&parent_commit],
            )?;
            Ok(commit_id)
        } else {
            // First commit - check if index has any entries
            if index.is_empty() {
                return Err("No changes to commit".into());
            }

            let commit_id =
                repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])?;
            Ok(commit_id)
        }
    }

    #[test]
    fn test_commit_file_impl_success() {
        let test_repo = TestRepo::new().unwrap();
        test_repo.add_file("test.txt", SAMPLE_FILE_CONTENT).unwrap();

        let result = commit_file_impl(
            test_repo.path_str(),
            "test.txt",
            INITIAL_COMMIT_MSG,
            TEST_USER_NAME,
            TEST_USER_EMAIL,
        );

        assert_result_is_ok(&result);
        let commit_hash = result.unwrap();
        assert_valid_commit_hash(&commit_hash);

        // Verify repository is clean after commit
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_commit_file_impl_nonexistent_file() {
        let test_repo = TestRepo::new().unwrap();

        let result = commit_file_impl(
            test_repo.path_str(),
            "nonexistent.txt",
            INITIAL_COMMIT_MSG,
            TEST_USER_NAME,
            TEST_USER_EMAIL,
        );

        assert_result_is_error(&result);
    }

    #[test]
    fn test_commit_file_impl_no_changes() {
        let test_repo = TestRepo::new().unwrap();

        // Create and commit a file
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        // Try to commit the same file again without changes
        let result = commit_file_impl(
            test_repo.path_str(),
            "test.txt",
            "Another commit",
            "Test User",
            "test@example.com",
        );

        // Assert the variant, not the prose. This matched on "No changes"
        // until the duplicate error modules were unified and the message
        // became "Nothing to commit" — and nothing noticed, because this
        // target had not compiled since.
        assert!(
            matches!(result, Err(GitError::NothingToCommit)),
            "got {result:?}"
        );
    }

    #[test]
    fn test_commit_files_impl_success() {
        let test_repo = TestRepo::new().unwrap();

        test_repo.add_file("file1.txt", "content1").unwrap();
        test_repo.add_file("file2.txt", "content2").unwrap();

        let files = vec!["file1.txt".to_string(), "file2.txt".to_string()];
        let result = commit_files_impl(
            test_repo.path_str(),
            &files,
            "Multi-file commit",
            "Test User",
            "test@example.com",
        );

        assert!(result.is_ok());
        let commit_hash = result.unwrap();
        assert!(!commit_hash.is_empty());

        // Verify repository is clean after commit
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_commit_files_impl_mixed_existing_and_new() {
        let test_repo = TestRepo::new().unwrap();

        // Create initial commit
        test_repo
            .add_and_commit("existing.txt", "initial", "Initial commit")
            .unwrap();

        // Modify existing file and add new file
        test_repo.add_file("existing.txt", "modified").unwrap();
        test_repo.add_file("new.txt", "new content").unwrap();

        let files = vec!["existing.txt".to_string(), "new.txt".to_string()];
        let result = commit_files_impl(
            test_repo.path_str(),
            &files,
            "Update existing and add new",
            "Test User",
            "test@example.com",
        );

        assert!(result.is_ok());

        // Verify repository is clean after commit
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_commit_impl_with_parent() {
        let test_repo = TestRepo::new().unwrap();

        // Create initial commit
        test_repo
            .add_and_commit("first.txt", "first", "First commit")
            .unwrap();

        // Stage a new file
        test_repo.add_file("second.txt", "second").unwrap();
        test_repo.stage_file("second.txt").unwrap();

        let result = commit_impl(
            &test_repo.repo,
            "Second commit",
            "Test User",
            "test@example.com",
        );
        assert!(result.is_ok());

        // Verify we now have 2 commits
        let mut revwalk = test_repo.repo.revwalk().unwrap();
        revwalk.push_head().unwrap();
        let commits: Vec<_> = revwalk.collect();
        assert_eq!(commits.len(), 2);
    }

    #[test]
    fn test_commit_impl_no_staged_changes() {
        let test_repo = TestRepo::new().unwrap();

        // Create initial commit
        test_repo
            .add_and_commit("first.txt", "first", "First commit")
            .unwrap();

        // Try to commit without any staged changes
        let result = commit_impl(
            &test_repo.repo,
            "Empty commit",
            "Test User",
            "test@example.com",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No changes"));
    }

    #[test]
    fn test_large_file_operations() {
        let test_repo = TestRepo::new().unwrap();

        // Create a large file (1MB)
        let large_content = "a".repeat(1024 * 1024);
        test_repo.add_file("large.txt", &large_content).unwrap();

        // Stage and commit the large file
        let result = commit_file_impl(
            test_repo.path_str(),
            "large.txt",
            "Add large file",
            "Test User",
            "test@example.com",
        );

        assert!(result.is_ok());

        // Verify repository is clean
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_binary_file_operations() {
        let test_repo = TestRepo::new().unwrap();

        // Create a binary file
        let binary_content = vec![0x00, 0x01, 0x02, 0xFF, 0xFE, 0xFD];
        let binary_path = test_repo.path.join("binary.dat");
        fs::write(&binary_path, &binary_content).unwrap();

        // Stage and commit the binary file
        let result = commit_file_impl(
            test_repo.path_str(),
            "binary.dat",
            "Add binary file",
            "Test User",
            "test@example.com",
        );

        assert!(result.is_ok());

        // Verify repository is clean
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_unicode_file_names() {
        let test_repo = TestRepo::new().unwrap();

        // Create files with Unicode names
        test_repo.add_file("测试.txt", "Chinese filename").unwrap();
        test_repo.add_file("файл.txt", "Russian filename").unwrap();
        test_repo.add_file("🦀_rust.rs", "Emoji filename").unwrap();

        let files = vec![
            "测试.txt".to_string(),
            "файл.txt".to_string(),
            "🦀_rust.rs".to_string(),
        ];

        let result = commit_files_impl(
            test_repo.path_str(),
            &files,
            "Add unicode files",
            "Test User",
            "test@example.com",
        );

        assert!(result.is_ok());

        // Verify repository is clean
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_nested_directory_operations() {
        let test_repo = TestRepo::new().unwrap();

        // Create files in nested directories
        test_repo
            .add_file("deep/nested/path/file.txt", "nested content")
            .unwrap();
        test_repo
            .add_file("another/deep/structure/test.rs", "rust content")
            .unwrap();

        let files = vec![
            "deep/nested/path/file.txt".to_string(),
            "another/deep/structure/test.rs".to_string(),
        ];

        let result = commit_files_impl(
            test_repo.path_str(),
            &files,
            "Add nested files",
            "Test User",
            "test@example.com",
        );

        assert!(result.is_ok());

        // Verify all files are tracked
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_performance_many_files() {
        let test_repo = TestRepo::new().unwrap();

        // Create many small files
        let file_count = 100;
        let mut files = Vec::new();

        for i in 0..file_count {
            let filename = format!("file_{:03}.txt", i);
            test_repo
                .add_file(&filename, &format!("Content for file {}", i))
                .unwrap();
            files.push(filename);
        }

        let start = std::time::Instant::now();
        let result = commit_files_impl(
            test_repo.path_str(),
            &files,
            "Add many files",
            "Test User",
            "test@example.com",
        );
        let duration = start.elapsed();

        assert!(result.is_ok());
        assert!(duration < std::time::Duration::from_secs(5)); // Should complete in reasonable time

        // Verify all files are committed
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert!(status.is_clean);
    }

    #[test]
    fn test_stage_file_impl_success() {
        let test_repo = TestRepo::new().unwrap();
        test_repo.add_file("test.txt", "content").unwrap();

        let result = stage_file_impl(test_repo.path_str(), "test.txt");
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify file is staged
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert_eq!(status.staged_files.len(), 1);
        assert_eq!(status.staged_files[0].path, "test.txt");
    }

    #[test]
    fn test_stage_file_impl_nonexistent_file() {
        let test_repo = TestRepo::new().unwrap();
        let result = stage_file_impl(test_repo.path_str(), "nonexistent.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_stage_file_impl_invalid_repo() {
        let temp_dir = TempDir::new().unwrap();
        let result = stage_file_impl(temp_dir.path().to_str().unwrap(), "test.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_unstage_file_impl_success() {
        let test_repo = TestRepo::new().unwrap();

        // Create and commit initial file
        test_repo
            .add_and_commit("test.txt", "initial content", "Initial commit")
            .unwrap();

        // Modify and stage the file
        test_repo.add_file("test.txt", "modified content").unwrap();
        test_repo.stage_file("test.txt").unwrap();

        // Verify it's staged
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert_eq!(status.staged_files.len(), 1);

        // Unstage it
        let result = unstage_file_impl(test_repo.path_str(), "test.txt");
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify it's no longer staged but still modified
        let status = get_status_impl(test_repo.path_str()).unwrap();
        assert_eq!(status.staged_files.len(), 0);
        assert_eq!(status.modified_files.len(), 1);
    }

    #[test]
    fn test_unstage_file_impl_invalid_repo() {
        let temp_dir = TempDir::new().unwrap();
        let result = unstage_file_impl(temp_dir.path().to_str().unwrap(), "test.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_staged_files_impl_empty() {
        let test_repo = TestRepo::new().unwrap();
        let result = get_staged_files_impl(test_repo.path_str()).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_get_staged_files_impl_with_files() {
        let test_repo = TestRepo::new().unwrap();

        // Add and stage multiple files
        test_repo.add_file("file1.txt", "content1").unwrap();
        test_repo.add_file("file2.txt", "content2").unwrap();
        test_repo.stage_file("file1.txt").unwrap();
        test_repo.stage_file("file2.txt").unwrap();

        let result = get_staged_files_impl(test_repo.path_str()).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"file1.txt".to_string()));
        assert!(result.contains(&"file2.txt".to_string()));
    }

    /// Find a commit by its message.
    ///
    /// These tests used to index into the history — `commits[1]` with the
    /// comment "Initial commit (oldest)", though the fixture makes three
    /// commits, so the oldest is at index 2. Looking it up by message says
    /// what is meant and does not depend on the ordering the API returns.
    fn commit_hash_by_message(path: &str, message: &str) -> String {
        let history = get_commit_history_impl(path, None, None).unwrap();
        history
            .commits
            .iter()
            .find(|c| c.message.trim() == message)
            .unwrap_or_else(|| panic!("no commit with message {message:?}"))
            .hash
            .clone()
    }

    #[test]
    fn test_restore_file_from_commit_impl() {
        let (temp_dir, path) = create_test_repo_with_history();
        let initial = commit_hash_by_message(&path, "Initial commit");

        // file1.txt matches HEAD here, so restoring an older revision of it
        // destroys no uncommitted work and the guard below does not fire.
        let file_path = temp_dir.path().join("file1.txt");
        let restored = restore_file_from_commit_impl(&path, "file1.txt", &initial)
            .expect("restoring over a clean working file should succeed");
        assert!(restored);

        // Verify file was restored
        let restored_content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(restored_content, "Initial content");
    }

    /// Restoring must not silently destroy work in the working tree.
    ///
    /// Nothing covered this guard, which is why the test above could sit
    /// broken: it modified the file first and then expected the restore to
    /// succeed, which is exactly the case the guard now refuses.
    ///
    /// Note there is no way to override this — restore has no `force`
    /// parameter — so a caller that wants "discard my changes and restore"
    /// cannot express it yet.
    #[test]
    fn test_restore_file_from_commit_impl_refuses_to_discard_uncommitted_changes() {
        let (temp_dir, path) = create_test_repo_with_history();
        let initial = commit_hash_by_message(&path, "Initial commit");

        let file_path = temp_dir.path().join("file1.txt");
        fs::write(&file_path, "Unsaved work").unwrap();

        let result = restore_file_from_commit_impl(&path, "file1.txt", &initial);
        assert!(
            matches!(result, Err(GitError::UnstagedChangesWouldBeLost { .. })),
            "expected the guard to fire, got {result:?}"
        );

        // The refusal must leave the working file exactly as it was.
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "Unsaved work");
    }

    #[test]
    fn test_restore_nonexistent_file() {
        let (_temp_dir, path) = create_test_repo_with_history();

        let history = get_commit_history_impl(&path, None, None).unwrap();
        let commit_hash = &history.commits[0].hash;

        let result = restore_file_from_commit_impl(&path, "nonexistent.txt", commit_hash);
        assert!(result.is_err());
    }
}
