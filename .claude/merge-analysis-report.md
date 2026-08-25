# mergeAnalysis + fastForward

Implements the first two functions of Phase 2 (`docs/features/version-control.md` §5
in nocturne-writer): reporting what a merge would do, and moving the current branch
forward when doing so is a strict fast-forward. This is the minimum needed for a
"pull when the local side is simply behind" flow: `fetch` → `mergeAnalysis` →
`fastForward`.

## What was added

- `src/types.rs`: two new `#[napi(object)]` structs.
  - `MergeAnalysis { kind: String, ahead: u32, behind: u32 }` — `kind` is
    `"up-to-date" | "fast-forward" | "normal"`. `ahead`/`behind` are always
    relative to `branch`, matching `AheadBehind`'s style.
  - `FastForwardResult { branch: String, previous_commit_hash: String, commit_hash: String }`.
- `src/errors.rs`: new `GitError::NotFastForward { branch: String, reason: String }`,
  code `NOT_FAST_FORWARD`, `reason` is `"up-to-date"` or `"diverged"`. Wired through
  `Display`, `error_code()`, `build_details_object()`, `to_serializable()`, and
  `utils::git_error_to_napi_with_flags`'s status match (all five places the compiler
  and pattern-matching required — none skipped). Unit test
  `test_serialization_not_fast_forward` added next to the existing
  `BranchNotMerged` serialization test.
- `src/branch_ops.rs`:
  - `resolve_branch_ref` — resolves `branch` (local name, `"origin/main"`, or any
    other short ref libgit2 recognises) via `resolve_reference_from_short_name`,
    mapping a miss to `BranchNotFound` the same way `checkout_branch_internal_impl` does.
  - `ahead_behind_of_head_impl` — `graph_ahead_behind(HEAD, their_id)`. Handles the
    unborn-HEAD edge case (fresh repo, no commits) by catching
    `ErrorCode::UnbornBranch` from `repo.head()` and reporting `(0, <full revwalk
    count of their_id>)` instead of propagating — an empty repo has no commits to be
    "ahead" of anything with.
  - `merge_analysis_impl` — opens the repo, resolves `branch`, calls
    `repo.merge_analysis(&[&annotated])`, and maps the returned flags:
    `is_up_to_date()` → `"up-to-date"`, else `is_fast_forward()` → `"fast-forward"`,
    else `"normal"`. Checking fast-forward before falling through matters: libgit2
    sets both `ANALYSIS_FASTFORWARD` and `ANALYSIS_UNBORN` together for an unborn
    HEAD (confirmed by reading `libgit2/src/libgit2/merge.c`), so this ordering is
    what makes that case read as fast-forward rather than wrongly falling to
    "normal". Read-only — never touches the tree, index, or a ref.
  - `fast_forward_impl` — requires HEAD to be on a branch (`DetachedHead` otherwise,
    reusing the existing variant/pattern from `file_ops.rs`), runs the same
    `merge_analysis`, refuses with `NotFastForward` when up-to-date or diverged,
    then checks out the target tree and moves the current branch's ref forward with
    `Reference::set_target`, followed by `checkout_head()` to finalize the
    tree/index — see "What I matched" below.
  - Two `#[cfg(feature = "napi-binding")]` wrappers: `merge_analysis` (no lock, it's
    read-only, matching `list_branches`/`get_current_branch`) and `fast_forward`
    (takes `utils::lock_repo`, matching `create_branch`/`checkout_branch`/`delete_branch`).
- `src/git_service.rs`: two thin `#[napi]` async methods, `merge_analysis` and
  `fast_forward`, each just delegating to `branch_ops::*` — no logic, matching the
  existing branch methods exactly.
- `src/utils.rs`: added `GitError::NotFastForward { .. } => Status::GenericFailure`
  to the exhaustive napi-status match (compiler caught the missing arm immediately).
- Tests:
  - `src/branch_ops_tests.rs` (inline, using the module's `setup_test_repo` /
    `write_and_commit` / `create_branch` / `checkout_default` helpers): 9 new tests —
    `test_merge_analysis_up_to_date`, `test_merge_analysis_fast_forward`,
    `test_merge_analysis_diverged`, `test_merge_analysis_unknown_branch`,
    `test_fast_forward_moves_ref_and_working_tree`,
    `test_fast_forward_refuses_diverged_branch`,
    `test_fast_forward_refuses_up_to_date_branch`.
  - `tests/branch_ops_tests.rs` (separate integration harness, using its own
    `TestRepo` helper — this module already duplicates coverage for the neighbouring
    branch functions in both harnesses, so I matched that): 4 new tests —
    `test_merge_analysis_impl_fast_forward`, `test_merge_analysis_impl_diverged_is_normal`,
    `test_fast_forward_impl_moves_ref_and_working_tree`,
    `test_fast_forward_impl_refuses_a_diverged_branch`.
- `README.md`: updated the Scope section (`mergeAnalysis`/`fastForward` are now
  supported; `pull`/full `merge` still are not, with the reasoning kept), added
  `NOT_FAST_FORWARD` to the Branches error-code list, and noted in the Configuration
  section that `fastForward` honours `liminal.checkoutStrategy` the same way
  `checkoutBranch` does.

## What I matched from `checkout_branch_impl`'s checkout path

Read `checkout_branch_internal_impl` (src/branch_ops.rs) before writing anything.
It:

1. Reads `liminal.checkoutStrategy` via `repository_ops::get_config_impl(repo_path,
   "liminal.checkoutStrategy", false)`, defaulting to `"safe"` for anything missing
   or unrecognised (a comment there explicitly says refusing is the safer failure
   for an unrecognised value too).
2. In **safe** mode: builds a `CheckoutBuilder` with `.safe()`, calls
   `repo.checkout_tree(target, ...)`. If that errors with `Uncommitted`, `Modified`,
   or a message containing `"conflict"`, it calls `collect_actual_conflicts` to get
   the *actual* list of files that would be overwritten (not just every dirty file)
   and returns `GitError::UnstagedChangesWouldBeLost { files }`. Any other error is
   passed through with `.with_operation("checkout_tree")`.
3. In **force** mode: builds with `.force()` and just does it.
4. After a successful `checkout_tree`, it updates the ref (`set_head` there, since
   it's switching branches) and then does a *second* checkout — `checkout_head()`
   with a fresh safe builder — to finalize the working tree/index against the
   now-current HEAD.

`fast_forward_impl` reuses this exact policy and even the same private
`collect_actual_conflicts` helper, with one necessary difference: `checkout_branch`
switches HEAD to point at a *different* branch (`repo.set_head(branch_ref_name)`),
but `fastForward` stays on the *same* branch — only the commit that branch's ref
points to changes. So instead of `set_head`, it calls
`head.set_target(their_commit.id(), "fast-forward: <old> -> <new>")` on the
already-resolved HEAD reference, then does the same finalizing `checkout_head()`
step. The tree-checkout-before-ref-move ordering is deliberate and preserved: if
`checkout_tree` fails (real conflict), the ref is never touched, so a refused
fast-forward leaves the repository exactly as it was — verified by
`test_fast_forward_refuses_diverged_branch`, which asserts HEAD's target and the
absence of the incoming file are unchanged after the refusal.

## Mutation testing (proving the tests discriminate)

**Mutation 1 — fast-forward refusal.** Changed both guard conditions in
`fast_forward_impl` from `if analysis.is_up_to_date()` / `if
!analysis.is_fast_forward()` to `if false && analysis.is_up_to_date()` / `if false
&& !analysis.is_fast_forward()`, effectively disabling the refusal entirely so a
diverged or already-up-to-date branch would be "fast-forwarded" (wrongly moving
the ref and losing history). Ran `cargo test -- fast_forward merge_analysis`:

- `test_fast_forward_refuses_diverged_branch` — FAILED (`expected NotFastForward,
  got Ok(FastForwardResult { .. })`)
- `test_fast_forward_refuses_up_to_date_branch` — FAILED (same shape)
- `tests/branch_ops_tests.rs::test_fast_forward_impl_refuses_a_diverged_branch` —
  FAILED (same shape)

All three refusal tests caught it. Reverted; full suite green again.

**Mutation 2 — the three analysis kinds.** In `merge_analysis_impl`, changed the
`kind` computation to `if false && analysis.is_up_to_date() { .. } else if false &&
analysis.is_fast_forward() { .. } else { "normal" }`, so every analysis reports
`"normal"` regardless of the real libgit2 result. Ran the same filter:

- `test_merge_analysis_up_to_date` — FAILED (`left: "normal", right: "up-to-date"`)
- `test_merge_analysis_fast_forward` — FAILED (`left: "normal", right: "fast-forward"`)
- `test_merge_analysis_diverged` — still passed (expected: "normal" happens to be
  correct there, so this test alone would not have caught the mutation — it's the
  other two that do, which is exactly the coverage the task asked for)
- `tests/branch_ops_tests.rs::test_merge_analysis_impl_fast_forward` — FAILED
  (same shape)

Reverted; full suite green again (see gate output below, captured after revert).

## Gate output (final, after reverting both mutations)

```
$ cargo fmt --check
(no output — clean)

$ cargo build --no-default-features
   Compiling liminal-git v1.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s

$ cargo clippy --no-default-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
(no warnings)

$ cargo test --no-default-features --no-fail-fast
test result: ok. 124 passed; 0 failed  (unit tests, src/lib.rs — incl. branch_ops::tests, errors::tests)
test result: ok. 20 passed; 0 failed   (tests/branch_ops_tests.rs)
test result: ok. 22 passed; 0 failed   (tests/file_ops_tests.rs)
test result: ok. 12 passed; 0 failed   (tests/history_ops_tests.rs)
test result: ok. 16 passed; 0 failed   (tests/integration_tests.rs)
test result: ok. 7 passed; 0 failed    (tests/phase2_tests.rs)
test result: ok. 10 passed; 0 failed   (tests/remote_ops_tests.rs)
test result: ok. 26 passed; 0 failed   (tests/repository_ops_tests.rs)
test result: ok. 13 passed; 0 failed   (tests/tag_ops_tests.rs)
test result: ok. 3 passed; 0 failed, 2 ignored  (doc-tests)
```

Also ran `cargo check` (default features, i.e. with `napi-binding` on) to confirm
`git_service.rs`'s new `#[napi]` methods and the `MergeAnalysis`/`FastForwardResult`
imports compile against the real napi types, not just the pure-Rust
`--no-default-features` path — clean.

Note: on the very first run, `tests/integration_tests.rs` had two failures
(`test_create_all_fixtures`, `test_repository_with_complex_history`) with errors
like "could not read index timestamp" / "Directory not empty". I confirmed via `git
stash` that these fail identically on unmodified `main` — a leftover/racy
`test-fixtures/` directory on this filesystem from a prior interrupted run, unrelated
to this change. A second clean run (after the stashed changes were restored) passed
all 16 integration tests, and the final full-suite run above shows the whole
`integration_tests` target green.

## Design notes / things I decided that weren't fully spec'd

- `branch` accepts anything `Repository::resolve_reference_from_short_name`
  resolves — a local branch name or a remote-tracking name like `"origin/main"` —
  since the `pull` composition in §5 needs to pass a freshly-fetched
  remote-tracking branch through both calls.
- Both `mergeAnalysis` and `fastForward` tolerate the "unborn HEAD" case (a
  repository with no commits yet) without panicking: `mergeAnalysis` reports
  `"fast-forward"` with `behind` = the full commit count reachable from `branch`
  (there is no shared history to subtract), matching what libgit2 itself signals
  (`ANALYSIS_FASTFORWARD | ANALYSIS_UNBORN` together). `fastForward` would still
  reach the same checkout/set_target path in that state — untested directly, since
  no test in this repo's convention constructs a HEAD-less repo scenario for
  branch ops. Everything is written with `?`/`map_err`, no `unwrap`/`expect` in
  library code, so it fails safely if libgit2's behavior here ever differs from
  what I read in `merge.c`.
- `fastForward`'s `NotFastForward.reason` is a plain string (`"up-to-date"` /
  `"diverged"`) rather than a separate error per case, since both are the same
  refusal ("not a fast-forward") with a different cause, and a JS caller can
  already get the `MergeAnalysis.kind` beforehand via `mergeAnalysis` if it wants
  to distinguish proactively rather than parse the error.
