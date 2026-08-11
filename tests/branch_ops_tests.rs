mod common;

#[cfg(test)]
mod branch_ops_tests {
    use crate::common::*;

    #[test]
    fn test_list_branches_impl_new_repo() {
        let test_repo = TestRepo::new().unwrap();

        // New repo should have no branches initially (until first commit)
        let result = list_branches_impl(test_repo.path_str(), false);
        assert!(result.is_ok());
        let branches = result.unwrap();
        assert_eq!(branches.len(), 0);
    }

    #[test]
    fn test_list_branches_impl_with_commits() {
        let test_repo = TestRepo::new().unwrap();

        // Create initial commit to establish main branch
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let result = list_branches_impl(test_repo.path_str(), false);
        assert!(result.is_ok());
        let branches = result.unwrap();
        assert_eq!(branches.len(), 1);

        let main_branch = &branches[0];
        // Git uses "master" as default branch name
        assert!(main_branch.name == "main" || main_branch.name == "master");
        assert!(main_branch.is_current);
        assert!(!main_branch.is_remote);
        assert!(!main_branch.commit_hash.is_empty());
    }

    #[test]
    fn test_get_current_branch_impl() {
        let test_repo = TestRepo::new().unwrap();

        // New repo should have no current branch
        let result = get_current_branch_impl(test_repo.path_str());
        assert!(result.is_err()); // No HEAD in new repo

        // After first commit, should have main branch
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let result = get_current_branch_impl(test_repo.path_str());
        assert!(result.is_ok());
        let current_branch = result.unwrap();
        assert!(current_branch.is_some());

        let branch = current_branch.unwrap();
        assert!(branch.name == "main" || branch.name == "master");
        assert!(branch.is_current);
    }

    #[test]
    fn test_create_branch_impl() {
        let test_repo = TestRepo::new().unwrap();

        // Create initial commit
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: false,
        };

        let result = create_branch_impl(test_repo.path_str(), &options);
        assert!(result.is_ok());

        let branch_info = result.unwrap();
        assert_eq!(branch_info.name, "feature-branch");
        assert!(!branch_info.is_current); // We didn't check it out

        // Verify branch exists in repository
        let branches = list_branches_impl(test_repo.path_str(), false).unwrap();
        assert_eq!(branches.len(), 2);
        assert!(branches.iter().any(|b| b.name == "feature-branch"));
    }

    #[test]
    fn test_create_branch_impl_with_checkout() {
        let test_repo = TestRepo::new().unwrap();

        // Create initial commit
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: true,
        };

        let result = create_branch_impl(test_repo.path_str(), &options);
        assert!(result.is_ok());

        let branch_info = result.unwrap();
        assert_eq!(branch_info.name, "feature-branch");
        assert!(branch_info.is_current); // We checked it out

        // Verify current branch changed
        let current = get_current_branch_impl(test_repo.path_str())
            .unwrap()
            .unwrap();
        assert_eq!(current.name, "feature-branch");
    }

    #[test]
    fn test_create_branch_impl_invalid_name() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let options = CreateBranchOptions {
            name: "".to_string(), // Invalid empty name
            from_commit: None,
            checkout: false,
        };

        let result = create_branch_impl(test_repo.path_str(), &options);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid branch name")
        );
    }

    #[test]
    fn test_create_branch_impl_already_exists() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: false,
        };

        // Create branch first time - should succeed
        let result1 = create_branch_impl(test_repo.path_str(), &options);
        assert!(result1.is_ok());

        // Try to create same branch again - should fail
        let result2 = create_branch_impl(test_repo.path_str(), &options);
        assert!(result2.is_err());
        assert!(result2.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn test_checkout_branch_impl() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        // Create a new branch
        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: false,
        };
        create_branch_impl(test_repo.path_str(), &options).unwrap();

        // Switch to the new branch
        let result = checkout_branch_impl(test_repo.path_str(), "feature-branch");
        assert!(result.is_ok());

        let branch_info = result.unwrap();
        assert_eq!(branch_info.name, "feature-branch");
        assert!(branch_info.is_current);

        // Verify current branch changed
        let current = get_current_branch_impl(test_repo.path_str())
            .unwrap()
            .unwrap();
        assert_eq!(current.name, "feature-branch");
    }

    #[test]
    fn test_checkout_branch_impl_nonexistent() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let result = checkout_branch_impl(test_repo.path_str(), "nonexistent-branch");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    /// Switching branches carries non-conflicting work across, as git does.
    ///
    /// This asserted the opposite — that *any* uncommitted change blocks a
    /// checkout — which has never been true here. `checkout_branch_impl` runs
    /// git2's "safe" strategy, which refuses only when the switch would lose
    /// something. A newly staged file that does not exist on the target branch
    /// conflicts with nothing, so it comes along. Nothing caught the wrong
    /// assertion because CI only ever ran `cargo test --lib`; this target
    /// compiled but was never executed.
    #[test]
    fn test_checkout_branch_impl_carries_non_conflicting_changes_across() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: false,
        };
        create_branch_impl(test_repo.path_str(), &options).unwrap();

        test_repo
            .add_file("uncommitted.txt", "uncommitted content")
            .unwrap();
        test_repo.stage_file("uncommitted.txt").unwrap();

        let branch = checkout_branch_impl(test_repo.path_str(), "feature-branch")
            .expect("a staged file absent from the target branch conflicts with nothing");
        assert_eq!(branch.name, "feature-branch");

        // The work came across rather than being discarded.
        assert!(test_repo.path.join("uncommitted.txt").exists());
    }

    /// The case the test above was reaching for: a change that genuinely
    /// cannot survive the switch, because the target branch has its own
    /// version of the same file.
    #[test]
    fn test_checkout_branch_impl_refuses_when_changes_would_be_lost() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        // A branch where test.txt has diverged.
        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: true,
        };
        create_branch_impl(test_repo.path_str(), &options).unwrap();
        test_repo
            .add_and_commit("test.txt", "feature content", "Diverge")
            .unwrap();

        let default_branch = get_current_branch_impl(test_repo.path_str())
            .unwrap()
            .unwrap()
            .name;
        assert_eq!(default_branch, "feature-branch");
        checkout_branch_impl(test_repo.path_str(), "master")
            .or_else(|_| checkout_branch_impl(test_repo.path_str(), "main"))
            .expect("should return to the default branch");

        // Uncommitted edit to the same file that differs on the target branch.
        test_repo.add_file("test.txt", "local edit").unwrap();

        let result = checkout_branch_impl(test_repo.path_str(), "feature-branch");
        assert!(result.is_err(), "expected a refusal, got {result:?}");

        // The local edit survives the refusal.
        let on_disk = std::fs::read_to_string(test_repo.path.join("test.txt")).unwrap();
        assert_eq!(on_disk, "local edit");
    }

    #[test]
    fn test_delete_branch_impl() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        // Get the default branch name (could be main or master)
        let default_branch = get_current_branch_impl(test_repo.path_str())
            .unwrap()
            .unwrap()
            .name;

        // Create and switch to a feature branch
        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: true,
        };
        create_branch_impl(test_repo.path_str(), &options).unwrap();

        // Add a commit to the feature branch
        test_repo
            .add_and_commit("feature.txt", "feature content", "Feature commit")
            .unwrap();

        // Switch back to default branch
        checkout_branch_impl(test_repo.path_str(), &default_branch).unwrap();

        // Delete the feature branch with force (since it's not merged)
        let result = delete_branch_impl(test_repo.path_str(), "feature-branch", true);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify branch is gone
        let branches = list_branches_impl(test_repo.path_str(), false).unwrap();
        assert_eq!(branches.len(), 1);
        assert!(!branches.iter().any(|b| b.name == "feature-branch"));
    }

    #[test]
    fn test_delete_branch_impl_current_branch() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        // Get the current branch name and try to delete it - should fail
        let current_branch = get_current_branch_impl(test_repo.path_str())
            .unwrap()
            .unwrap()
            .name;
        // Assert the variant, not the prose. This matched "currently checked
        // out", which stopped being the wording when the duplicate error
        // modules were unified.
        let result = delete_branch_impl(test_repo.path_str(), &current_branch, true);
        assert!(
            matches!(result, Err(GitError::CannotDeleteCurrentBranch { .. })),
            "got {result:?}"
        );
    }

    #[test]
    fn test_delete_branch_impl_nonexistent() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        let result = delete_branch_impl(test_repo.path_str(), "nonexistent-branch", false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_delete_branch_impl_not_merged() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        // Get the default branch name
        let default_branch = get_current_branch_impl(test_repo.path_str())
            .unwrap()
            .unwrap()
            .name;

        // Create and switch to a feature branch
        let options = CreateBranchOptions {
            name: "feature-branch".to_string(),
            from_commit: None,
            checkout: true,
        };
        create_branch_impl(test_repo.path_str(), &options).unwrap();

        // Add a commit to the feature branch
        test_repo
            .add_and_commit("feature.txt", "feature content", "Feature commit")
            .unwrap();

        // Switch back to default branch
        checkout_branch_impl(test_repo.path_str(), &default_branch).unwrap();

        // Try to delete without force - should fail. Assert the variant, not
        // the prose: this matched "not fully merged", which stopped being the
        // wording when the duplicate error modules were unified.
        let result = delete_branch_impl(test_repo.path_str(), "feature-branch", false);
        assert!(
            matches!(result, Err(GitError::BranchNotMerged { .. })),
            "got {result:?}"
        );
    }

    #[test]
    fn test_branch_sorting() {
        let test_repo = TestRepo::new().unwrap();
        test_repo
            .add_and_commit("test.txt", "content", "Initial commit")
            .unwrap();

        // Create several branches
        let branch_names = vec!["zebra-branch", "alpha-branch", "beta-branch"];
        for name in &branch_names {
            let options = CreateBranchOptions {
                name: name.to_string(),
                from_commit: None,
                checkout: false,
            };
            create_branch_impl(test_repo.path_str(), &options).unwrap();
        }

        let branches = list_branches_impl(test_repo.path_str(), false).unwrap();

        // Current branch (main or master) should be first
        assert!(branches[0].name == "main" || branches[0].name == "master");
        assert!(branches[0].is_current);

        // Other branches should be sorted alphabetically
        assert_eq!(branches[1].name, "alpha-branch");
        assert_eq!(branches[2].name, "beta-branch");
        assert_eq!(branches[3].name, "zebra-branch");
    }
}
