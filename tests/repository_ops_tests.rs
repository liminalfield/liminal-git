
mod common;

#[cfg(test)]
mod repository_ops_tests {
    use crate::common::*;


    #[test]
    fn test_is_repository_impl_valid_repo() {
        let test_repo = TestRepo::new().unwrap();
        assert!(is_repository_impl(test_repo.path_str()));
    }

    #[test]
    fn test_is_repository_impl_invalid_repo() {
        let temp_dir = TempDir::new().unwrap();
        assert!(!is_repository_impl(temp_dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_is_repository_impl_nonexistent_path() {
        assert!(!is_repository_impl("/nonexistent/path"));
    }

    #[test]
    fn test_get_status_impl_empty_repo() {
        let test_repo = TestRepo::new().unwrap();
        let status = get_status_impl(test_repo.path_str()).unwrap();

        assert!(status.is_clean);
        assert_eq!(status.modified_files.len(), 0);
        assert_eq!(status.deleted_files.len(), 0);
        assert_eq!(status.added_files.len(), 0);
        assert_eq!(status.untracked_files.len(), 0);
        assert_eq!(status.staged_files.len(), 0);
    }

    #[test]
    fn test_get_status_impl_with_untracked_files() {
        let test_repo = TestRepo::new().unwrap();

        // Add some untracked files
        test_repo.add_file("untracked1.txt", "content1").unwrap();
        test_repo.add_file("untracked2.txt", "content2").unwrap();

        let status = get_status_impl(test_repo.path_str()).unwrap();

        assert!(!status.is_clean);
        assert_eq!(status.untracked_files.len(), 2);
        assert_eq!(status.modified_files.len(), 0);
        assert_eq!(status.staged_files.len(), 0);

        // Check file names
        let file_names: Vec<&str> = status.untracked_files.iter()
            .map(|f| f.path.as_str())
            .collect();
        assert!(file_names.contains(&"untracked1.txt"));
        assert!(file_names.contains(&"untracked2.txt"));
    }

    #[test]
    fn test_get_status_impl_with_staged_files() {
        let test_repo = TestRepo::new().unwrap();

        // Add and stage files
        test_repo.add_file("staged1.txt", "content1").unwrap();
        test_repo.add_file("staged2.txt", "content2").unwrap();
        test_repo.stage_file("staged1.txt").unwrap();
        test_repo.stage_file("staged2.txt").unwrap();

        let status = get_status_impl(test_repo.path_str()).unwrap();

        assert!(!status.is_clean);
        assert_eq!(status.staged_files.len(), 2);
        assert_eq!(status.untracked_files.len(), 0);
        assert_eq!(status.modified_files.len(), 0);

        // Verify all staged files have staged flag set
        for file in &status.staged_files {
            assert!(file.staged);
            assert_eq!(file.status, "staged_added");
        }
    }

    #[test]
    fn test_get_status_impl_with_modified_files() {
        let test_repo = TestRepo::new().unwrap();

        // Create and commit a file first
        test_repo.add_and_commit("tracked.txt", "initial content", "Initial commit").unwrap();

        // Now modify it
        test_repo.add_file("tracked.txt", "modified content").unwrap();

        let status = get_status_impl(test_repo.path_str()).unwrap();

        assert!(!status.is_clean);
        assert_eq!(status.modified_files.len(), 1);
        assert_eq!(status.modified_files[0].path, "tracked.txt");
        assert_eq!(status.modified_files[0].status, "modified");
        assert!(!status.modified_files[0].staged);
    }

    #[test]
    fn test_get_status_impl_mixed_states() {
        let test_repo = TestRepo::new().unwrap();

        // Create initial commit
        test_repo.add_and_commit("existing.txt", "content", "Initial commit").unwrap();

        // Create various file states
        test_repo.add_file("existing.txt", "modified content").unwrap(); // Modified
        test_repo.add_file("untracked.txt", "untracked content").unwrap(); // Untracked
        test_repo.add_file("staged.txt", "staged content").unwrap(); // Will be staged
        test_repo.stage_file("staged.txt").unwrap();

        let status = get_status_impl(test_repo.path_str()).unwrap();

        assert!(!status.is_clean);
        assert_eq!(status.modified_files.len(), 1);
        assert_eq!(status.untracked_files.len(), 1);
        assert_eq!(status.staged_files.len(), 1);

        // Verify file paths
        assert_eq!(status.modified_files[0].path, "existing.txt");
        assert_eq!(status.untracked_files[0].path, "untracked.txt");
        assert_eq!(status.staged_files[0].path, "staged.txt");
    }

    #[test]
    fn test_get_status_impl_invalid_repo() {
        let temp_dir = TempDir::new().unwrap();
        let result = get_status_impl(temp_dir.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_concurrent_status_checks() {
        use std::thread;

        // Create multiple repositories for concurrent access since git2::Repository is not Send/Sync
        let test_repos: Vec<_> = (0..5).map(|i| {
            let repo = TestRepo::new().unwrap();
            repo.add_file(&format!("concurrent{}.txt", i), &format!("content{}", i)).unwrap();
            repo.add_file(&format!("concurrent{}_2.txt", i), &format!("content{}_2", i)).unwrap();
            let path = repo.path_str().to_string();
            (repo, path)
        }).collect();

        let mut handles = vec![];

        // Spawn multiple threads checking status on different repositories
        for (_repo, repo_path) in &test_repos {
            let repo_path = repo_path.clone();
            let handle = thread::spawn(move || {
                get_status_impl(&repo_path)
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            let result = handle.join().unwrap();
            if let Err(e) = &result {
                eprintln!("Concurrent status check failed with error: {:?}", e);
            }
            assert!(result.is_ok());
            let status = result.unwrap();
            assert_eq!(status.untracked_files.len(), 2);
        }

        // Keep test_repos alive until threads complete
        drop(test_repos);
    }
    

    #[test]
    fn test_init_repository_impl() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        let result = init_repository_impl(&path);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify .git directory exists
        assert!(temp_dir.path().join(".git").exists());

        // Verify it's a valid repository
        assert!(is_repository_impl(&path));
    }

    #[test]
    fn test_init_repository_with_config_impl() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        let config = RepositoryConfig {
            description: Some("Test repository description".to_string()),
            default_branch: Some("main".to_string()),
            line_ending: Some("lf".to_string()),
        };

        let result = init_repository_with_config_impl(&path, &config);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify description file
        let desc_path = temp_dir.path().join(".git").join("description");
        assert!(desc_path.exists());
        let content = fs::read_to_string(desc_path).unwrap();
        assert_eq!(content, "Test repository description");
    }

    #[test]
    fn test_init_repository_nonexistent_path() {
        let result = init_repository_impl("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_repository_healthy_impl() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        // Initialize repository first
        init_repository_impl(&path).unwrap();

        // Create and commit a test file to establish HEAD properly
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "Hello, world!").unwrap();

        commit_file_impl(
            &path,
            &test_file.to_string_lossy(),
            "Initial commit",
            "Test User",
            "test@example.com",
        ).unwrap();

        let health = is_repository_healthy_impl(&path);
        assert!(health.is_ok());

        let health_result = health.unwrap();
        assert!(health_result.is_healthy);
        assert!(health_result.issues.is_empty());
    }

    #[test]
    fn test_repository_health_with_warnings() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        init_repository_impl(&path).unwrap();

        // Create stale lock file
        let lock_path = temp_dir.path().join(".git").join("index.lock");
        fs::write(&lock_path, "").unwrap();

        let health = is_repository_healthy_impl(&path);
        assert!(health.is_ok());

        let health_result = health.unwrap();
        assert!(!health_result.warnings.is_empty());
    }

    #[test]
    fn test_repair_repository_impl() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        init_repository_impl(&path).unwrap();

        // Create stale lock files
        fs::write(temp_dir.path().join(".git").join("index.lock"), "").unwrap();
        fs::write(temp_dir.path().join(".git").join("HEAD.lock"), "").unwrap();

        let result = repair_repository_impl(&path);
        assert!(result.is_ok());
        assert!(result.unwrap()); // Should report repairs were made

        // Verify lock files removed
        assert!(!temp_dir.path().join(".git").join("index.lock").exists());
        assert!(!temp_dir.path().join(".git").join("HEAD.lock").exists());
    }

    #[test]
    fn test_configure_repository_impl() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        init_repository_impl(&path).unwrap();

        let config = GitConfig {
            user_name: Some("Test User".to_string()),
            user_email: Some("test@example.com".to_string()),
            core_autocrlf: Some("false".to_string()),
            core_safecrlf: Some("warn".to_string()),
        };

        let result = configure_repository_impl(&path, &config);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_create_gitignore_impl() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        let patterns = vec![
            "*.log".to_string(),
            "node_modules/".to_string(),
            ".env".to_string(),
        ];

        let result = create_gitignore_impl(&path, &patterns);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify file exists and has correct content
        let gitignore_path = temp_dir.path().join(".gitignore");
        assert!(gitignore_path.exists());

        let content = fs::read_to_string(gitignore_path).unwrap();
        assert_eq!(content, patterns.join("\n"));
    }

    #[test]
    fn test_create_gitattributes_impl() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        let rules = vec![
            "*.txt text".to_string(),
            "*.jpg binary".to_string(),
            "*.sh text eol=lf".to_string(),
        ];

        let result = create_gitattributes_impl(&path, &rules);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify file exists and has correct content
        let gitattributes_path = temp_dir.path().join(".gitattributes");
        assert!(gitattributes_path.exists());

        let content = fs::read_to_string(gitattributes_path).unwrap();
        assert_eq!(content, rules.join("\n"));
    }

    #[test]
    fn test_get_repository_info_impl_empty() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        init_repository_impl(&path).unwrap();

        let info = get_repository_info_impl(&path);
        assert!(info.is_ok());

        let repo_info = info.unwrap();
        assert_eq!(repo_info.path, path);
        assert!(!repo_info.is_bare);
        assert!(repo_info.head_commit.is_none());
        assert_eq!(repo_info.branch_count, 0);
        assert_eq!(repo_info.commit_count, 0);
        assert!(!repo_info.has_uncommitted_changes);
        assert!(repo_info.remote_urls.is_empty());
    }

    #[test]
    fn test_get_repository_info_impl_with_commit() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_string_lossy().to_string();

        init_repository_impl(&path).unwrap();

        // Create and commit a test file
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "Hello, world!").unwrap();

        let commit_hash = commit_file_impl(
            &path,
            &test_file.to_string_lossy(),
            "Initial commit",
            "Test User",
            "test@example.com",
        ).unwrap();

        let info = get_repository_info_impl(&path);
        assert!(info.is_ok());

        let repo_info = info.unwrap();
        assert!(repo_info.head_commit.is_some());
        assert_eq!(repo_info.head_commit.unwrap(), commit_hash);
        assert_eq!(repo_info.commit_count, 1);
        assert!(!repo_info.has_uncommitted_changes);
    }
}