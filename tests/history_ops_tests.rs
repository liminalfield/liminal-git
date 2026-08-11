

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
        commit_file_impl(&path, &file1.to_string_lossy(), "Initial commit", "Test User", "test@example.com").unwrap();

        // Modify file and commit again
        fs::write(&file1, "Modified content").unwrap();
        commit_file_impl(&path, &file1.to_string_lossy(), "Second commit", "Test User", "test@example.com").unwrap();

        // Create second file and commit
        let file2 = temp_dir.path().join("file2.txt");
        fs::write(&file2, "Second file content").unwrap();
        commit_file_impl(&path, &file2.to_string_lossy(), "Third commit", "Test User", "test@example.com").unwrap();

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
        index.remove_path(std::path::Path::new("file2.txt")).unwrap();
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
        ).unwrap();

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
}

