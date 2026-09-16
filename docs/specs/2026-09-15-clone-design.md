# clone, and a code for "not a repository"

Design for issues #16 (add `clone`) and #15 (`REPOSITORY_NOT_FOUND`). They
share one design because they are one argument: a failure should say which
failure it was. They ship as two commits.

## Why

A host can do everything with a repository except acquire one. `clone` is the
one operation on the remote surface keyed by a URL and a destination rather
than by a repository path, because the point of it is that no repository
exists yet.

The case for it over a `git` subprocess is entirely about failure. Four
outcomes are different and a host has to tell them apart: the remote could not
be reached, the credentials were refused, the destination was occupied, and the
transfer began and died. The last decides who cleans up.

## Surface

```ts
clone(
  url: string,
  destPath: string,
  credentials?: RemoteCredentials | null,
  options?: CloneOptions | null
): Promise<CloneResult>

interface CloneOptions {
  /** Branch to check out. Defaults to the remote's HEAD. */
  branch?: string
}

interface CloneResult {
  /** Branch checked out. Present even for an empty remote, where it is unborn. */
  branch: string
  /** Commit that branch resolved to. Absent when the branch is unborn. */
  commit?: string
  receivedObjects: number
  receivedBytes: number
}
```

`RemoteCredentials` is reused unchanged. `clone_impl` builds its `FetchOptions`
from the existing `fetch_options()` helper, so the credential ladder and the
transfer counters are the same code `fetch` runs, not a second copy of it. A
parallel ladder is how two paths start disagreeing about what an SSH agent
means.

It lives in `remote_ops.rs`. Clone creates a repository, which argues for
`repository_ops.rs`, but it shares the credential machinery and the counters,
and splitting one function across two modules to satisfy a taxonomy is worse
than either.

### Decisions on the surface

**No `remote` field.** It would always read `"origin"`, and a field whose
value is a constant carries no information. `options.remoteName` is not offered
either. Both arrive together if a caller ever wants `clone -o`; adding them
later breaks nobody.

**No `depth`, no progress callback.** Shallow clone quietly breaks reading at
past commits, which is what the caller who asked for this does. Progress needs
a `ThreadsafeFunction` for a spinner nobody has asked to replace. Both are
additive later.

**`branch` is required; `commit` is optional.** An unborn HEAD still has a
symbolic target, so a clone of an empty remote reports the branch it would
commit to. `commit` is absent exactly when that branch is unborn.

For an empty remote the branch name may come from local `init.defaultBranch`
rather than from the remote — an empty repository over protocol v0 advertises
no refs, so there may be nothing to honour. `remote.default_branch()` is
preferred where it returns something. The doc comment says so, because a host
reading `branch` as a fact about the remote will put its first commit on the
wrong one. Consumers check `result.commit == null`, which holds whether
napi-rs writes `null` or omits the key.

## Errors

### Classification, not new variants (#15)

`From<git2::Error>` cannot construct `RepositoryNotFound { path }`: a
`git2::Error` carries class, code and message, and no path. Converting there
would produce `details: { path: "" }`, worse than today, where the message
names the path.

So classification reads `class` and `code` off `GitOperationFailure` inside
`error_code()`, the idiom `is_retryable()` already uses:

```rust
GitError::GitOperationFailure { class, code, .. } => match (class, code) {
    (Repository, NotFound) => "REPOSITORY_NOT_FOUND",
    (_, Auth)              => "AUTHENTICATION_FAILED",
    (Net | Http, NotFound) => "REMOTE_NOT_FOUND",
    (Net | Http, _)        => "REMOTE_UNREACHABLE",
    _                      => "GIT_OPERATION_FAILURE",
}
```

Compared against git2's enums rather than their integers, as the existing
comment there requires. One classification point, no call sites touched, and
the payload is unchanged — only the code becomes precise.

**Arm ordering is load-bearing.** `(_, Auth)` sits above the transport arms so
a transport-class auth failure reads as auth. A comment says so and a test
asserts `(Net, Auth)` classifies as auth, so regrouping the arms fails a test
rather than changing behaviour quietly.

`is_retryable()` changes with it, and this is a behaviour change on its own.
Today it answers true only for `Locked`, `Os` and `Filesystem`, so a network
failure is not retriable. `REMOTE_UNREACHABLE` must be, so the transport arm
is added there too — and `REMOTE_NOT_FOUND` must not be, so it is excluded
explicitly rather than by falling through. The two arms are written as one
match on `(class, code)` shared with `error_code()`, so a code and its
retriability cannot drift apart.

**`GitError::RepositoryNotFound` is deleted.** It is never constructed, and
leaving it would let one code string mean two different `details` shapes.

**`credential_callback`'s give-up errors change class.** They are built with
`Error::from_str` (`remote_ops.rs:57`, `:100`), which yields `Generic/Generic`
and would not classify — the auth failure most likely to happen would be the
one that falls through. They become
`Error::new(ErrorCode::Auth, ErrorClass::Callback, ...)`. `is_retryable`
answers false for that pair, as it does for `Generic/Generic` today, so a
rejected key still does not invite a retry loop.

### What a clone failure says

| Case | Code | Retriable |
|---|---|---|
| Could not reach the remote | `REMOTE_UNREACHABLE` | yes |
| Remote answered, no such repository | `REMOTE_NOT_FOUND` | no |
| Credentials rejected or absent | `AUTHENTICATION_FAILED` | no |
| Destination exists with files | `DESTINATION_NOT_EMPTY` | no |
| Transfer began, then died | `CLONE_INCOMPLETE` | only if cleanup succeeded |

"Could not reach" and "answered 404" are split because they want opposite
retry advice. Filed as one code, a host with a retry loop spins on a typo.
Issue #16's own text conflates them; a comment on the issue records the
correction.

`CLONE_INCOMPLETE` is distinguished by whether anything arrived, which the
`ProgressHandle` already counts. A connection that authenticates and drops
before the first object reads as unreachable; both are retriable, so the
imprecision costs nothing.

### Cleanup is reported on every failure that created anything

libgit2 creates the directory and `.git` before it authenticates, so a
rejected credential also leaves a directory behind. Reporting cleanup only on
`CLONE_INCOMPLETE` would put the lie in the likeliest case: not-retriable is
correct advice about the credential and wrong about the destination, and a
person who supplies the right token then meets `DESTINATION_NOT_EMPTY` on a
directory they never created.

One wrapper carries it:

```rust
CloneFailed {
    class, code, message,      // the underlying libgit2 failure, preserved
    destination: String,
    partial_removed: bool,
    received_objects: u32,
}
```

`error_code()` delegates to the same `(class, code)` classification, except
that `received_objects > 0` yields `CLONE_INCOMPLETE`. `is_retryable()` is
`partial_removed && underlying_retriable`: a failed cleanup makes any clone
failure non-retriable, because the retry would hit `DESTINATION_NOT_EMPTY`.

`DESTINATION_NOT_EMPTY` stays a plain variant — it is the only failure that
happens before anything is created.

Two new variants need arms in six places: `Display`, `error_code`,
`is_retryable`, `build_details_object`, `to_serializable`, and the napi
`Status` map in `utils.rs`. Only `is_retryable` has a `_ =>` arm, so the
compiler finds five of the six and that one must be edited deliberately —
a new variant landing in its default is silently non-retriable, which is
exactly wrong for `CloneFailed`.

## Cleanup mechanics

The probe runs before any network work.

| Destination | Action | On failure |
|---|---|---|
| Exists, has files | `DESTINATION_NOT_EMPTY` immediately | nothing created |
| Exists, empty | proceed, `created_dir = false` | remove the contents, keep the directory |
| Missing, parent exists | `create_dir`, `created_dir = true` | `remove_dir_all(destPath)` |
| Missing, parent missing | `INVALID_PATH`, "parent directory does not exist" | nothing created |

Leading directories are deliberately not created. `git clone` creates them;
refusing keeps the unwind to two shapes instead of an unbounded chain of
parents. Relaxing this later is non-breaking.

`create_dir`, not `create_dir_all`: the parent must already exist, and
`AlreadyExists` then comes back as a real error mapping to
`DESTINATION_NOT_EMPTY` rather than silently succeeding into a directory
another process created between the probe and the create.

**"Empty" is strict.** Any entry counts, including dotfiles, matching
`init_repository_impl` (`repository_ops.rs:269`) and `git clone`. The error
names the first entry it found, so a refusal over a `.DS_Store` a person
cannot see in Finder is explicable.

**Locking.** `lock_repo` cannot be used: `lock_identity`
(`utils.rs:217`) canonicalizes the path, which fails when the destination does
not exist, and `lock_file_path` (`utils.rs:234`) puts the lock inside
`<dest>/.git`, which cleanup deletes. Clone takes the in-process mutex only,
keyed on the canonicalized parent joined with the final component.

This is a weaker guarantee than `fetch` and `push` have, and the doc comment
says so: two processes cloning into one destination are not excluded. It
degrades safely — the loser gets `DESTINATION_NOT_EMPTY` against a tree the
winner owns, not an interleaved one. A cross-process lock has nowhere to live
when the destination does not exist, and putting it in the parent reintroduces
the ordering problem the layout above removes.

## Testing

Tests call `clone_impl` directly against local bare repositories, matching the
existing harness: `npm test` is `cargo test --no-default-features`, so the napi
layer is compiled out and the `*_impl` functions are what is reachable.

Integration, in `tests/remote_ops_tests.rs`:

1. clones a repository — branch, commit, counters, working tree contents
2. creates a missing destination
3. accepts an existing empty destination
4. refuses a non-empty destination, leaving the existing files untouched
5. refuses a dotfile-only destination, and names the dotfile
6. refuses a missing parent
7. empty remote — branch present, commit absent; records what libgit2 resolves
8. failure removes a destination it created
9. failure leaves a pre-existing empty directory in place, still empty
10. `options.branch` checks out the named branch
11. cleanup failure, Unix-only: parent chmod'd read-only, `partial_removed`
    comes back false and retriability goes false with it; skipped as root

Unit, in the `errors.rs` test module, over synthetic
`git2::Error::new(code, class, msg)` values:

- `(Repository, NotFound)` -> `REPOSITORY_NOT_FOUND`
- `(Http, NotFound)` -> `REMOTE_NOT_FOUND`, not retriable
- `(Net, Generic)` -> `REMOTE_UNREACHABLE`, retriable
- `(Net, Auth)` -> `AUTHENTICATION_FAILED`, the arm-ordering guard
- an unrelated class still -> `GIT_OPERATION_FAILURE`, retriability unchanged
- `CloneFailed` shaped as an auth failure carries `destination` and
  `partial_removed` like every other post-creation failure
- `CloneFailed` with `partial_removed: false` -> not retriable even when the
  underlying cause is
- `CloneFailed` with `received_objects > 0` -> `CLONE_INCOMPLETE`

Plus an `#[ignore]` test that hits a real missing repository over HTTPS, so
the premise behind the `REMOTE_NOT_FOUND` arm is runnable on demand rather
than remembered. The synthetic tests prove the arm; only this one proves the
input. The observed `(class, code)` goes in a comment naming the libgit2
version it was seen against.

### What the tests cannot reach

- **Authentication end to end.** A local path never asks for credentials. The
  auth paths have classification coverage only. The README already says this
  about `credential_callback`; the note extends to clone.
- **The runtime value of `commit`.** `cargo test --no-default-features`
  compiles napi out. CI already regenerates and diffs `index.d.ts`
  (`ci.yml:178`), which settles the published type; whether napi-rs writes
  `null` or omits the key is checked by hand once against the built addon, and
  the doc comment specifies `== null`, which holds either way.

## Delivery

Two commits, per one issue one commit:

1. **#15** — classification in `error_code()`, the `RepositoryNotFound`
   deletion, the `credential_callback` class change, and their tests. Ships
   `REPOSITORY_NOT_FOUND`, `AUTHENTICATION_FAILED`, `REMOTE_NOT_FOUND` and
   `REMOTE_UNREACHABLE` across the whole surface.
2. **#16** — `clone`, `CloneOptions`, `CloneResult`, `DestinationNotEmpty`,
   `CloneFailed`, the probe and cleanup, and their tests.

A minor version: the surface gains an operation and no existing call changes
shape. Consumers matching `GIT_OPERATION_FAILURE` for a mistyped path see
`REPOSITORY_NOT_FOUND` instead, which is the point of #15 and worth a release
note.
