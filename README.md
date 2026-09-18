# liminal-git

Git operations for Node.js, built on [libgit2](https://libgit2.org/) via
[napi-rs](https://napi.rs/). No subprocess, no parsing output meant for humans —
the library talks to the repository directly and returns typed data.

It was extracted from [Nocturne Writer](https://github.com/liminalfield/nocturne-writer),
where it provides version control for a writing application, so its 60
operations lean towards the things a content tool needs: file history, a file's
contents at a commit, restoring a deleted file, structured diffs. Branch and tag
management are complete, remotes and merging are supported, and what is left out
is left out deliberately (see [Scope](#scope)).

## Install

```bash
npm install liminal-git
```

**Node.js 20 or newer.** Nothing else. The addon ships as a prebuilt binary per
platform, so installing needs no Rust toolchain and no C compiler, and takes as
long as any other package.

Four platforms are published, which are exactly the four CI builds and tests:

| Platform            | Package                      |
| ------------------- | ---------------------------- |
| Linux x64 (glibc)   | `liminal-git-linux-x64-gnu`  |
| macOS Apple Silicon | `liminal-git-darwin-arm64`   |
| macOS Intel         | `liminal-git-darwin-x64`     |
| Windows x64         | `liminal-git-win32-x64-msvc` |

They are wired as `optionalDependencies`, so npm fetches only the one matching
the installing machine. A published binary nobody tests is a support promise
nobody made, which is why the list is not longer than the CI matrix.

### Building from source

Anywhere else — musl, ARM Linux, ARM Windows — and for working on the library
itself, install from a git tag:

```bash
npm install github:liminalfield/liminal-git#v1.10.0
```

That route runs the `prepare` script, which compiles the addon on the installing
machine. It needs:

- **Rust 1.89 or newer.** Enforced by `rust-version` in `Cargo.toml` and checked
  in CI. The floor is set by `std::fs::File::try_lock`, stabilised in 1.89.
- **A C compiler**, to build the vendored libgit2.

Expect roughly 100 seconds while the crate compiles in release mode. npm does not
cache the result between installs, which is the cost the published binaries
exist to remove. Pin a tag or a commit rather than a branch either way, so that a
rebuild cannot silently change what you depend on.

## Usage

Every operation except the constructor is asynchronous — 60 of them return a
Promise. Paths are absolute for the repository and repository-relative for files
within it.

```js
const fs = require('node:fs');
const path = require('node:path');
const { GitService } = require('liminal-git');

const git = new GitService();
const repo = '/srv/projects/my-repo';

// initRepository requires the directory to be empty.
await git.initRepository(repo);

// The file has to exist on disk; commitFile stages and commits what is there.
fs.mkdirSync(path.join(repo, 'notes'), { recursive: true });
fs.writeFileSync(path.join(repo, 'notes/chapter-one.md'), '# Chapter One\n');

await git.commitFile(
  repo,
  'notes/chapter-one.md',
  'Add the first chapter',
  'Ada Lovelace',
  'ada@example.com',
);

const status = await git.getStatus(repo);
console.log(status.isClean, status.modifiedFiles);

const history = await git.getCommitHistory(repo, 20, 0);
for (const commit of history.commits) {
  console.log(commit.shortHash, commit.message);
}
```

TypeScript declarations ship with the package and are generated from the Rust
source, so they cannot drift from it — CI fails if they do.

## Committing a decision

`commitFile` stages one path and commits. `commitFiles` takes a list and
writes **one** commit for the whole set:

```js
await git.commitFiles(
  repo,
  ['notes/chapter-one.md', 'notes/index.md'],
  'Rename chapter one and update the index',
  'Ada Lovelace',
  'ada@example.com',
);
```

The set lands or nothing does. That is the point rather than a nicety: a
change spanning several files is one decision, and as separate commits the
history stops recording decisions, reverting it stops being a single
operation, and between the commits HEAD holds half a change that any reader
pinned to it can observe.

**The state on disk is what gets staged**, for both operations and for every
path in the list:

| On disk | Tracked | Result |
|---|---|---|
| yes | either | its current content is staged |
| no | yes | staged as a deletion |
| no | no | `FILE_NOT_FOUND` |

So deleting one file and editing another is one commit, listing both paths.

Every path is validated and classified before the index is touched, so a
failure on any path in the list leaves the index and HEAD exactly as they
were: there is no partially staged set to clean up. Repeated paths are
de-duplicated, including the absolute and repository-relative spellings of the
same path.

`NOTHING_TO_COMMIT` is evaluated over the resulting tree, which makes it a
whole-set check rather than a per-path one. An empty list is
`INVALID_ARGUMENT`, and so is a list longer than **1000 paths**.

Neither operation takes options for amending or signing, and neither accepts a
glob or a "commit everything dirty" mode. The argument stays an explicit list
so that the caller states exactly what the commit contains.

## Who decided it, and who performed it

A commit carries two signatures. The **author** is who the change belongs to;
the **committer** is who wrote it into the repository. They are the same person
most of the time, which is why the distinction is easy to forget — and they
stop being the same person the moment a tool commits on someone's behalf.

Every operation that writes a commit takes an optional `committer`:

```js
await git.commitFiles(
  repo,
  ['notes/chapter-one.md'],
  'Approve the rewrite',
  'Ada Lovelace',        // decided it
  'ada@example.com',
  { committerName: 'my-agent', committerEmail: 'agent@example.com' },  // performed it
);
```

Omit it and the author signs both trailers, which is what every caller written
before this option got and still gets. Supply **both** halves or neither: a
name paired with the author's email names an identity that never existed, so a
lone half is `INVALID_ARGUMENT` rather than being quietly completed. The
refusal happens before anything is staged.

Every operation that writes a commit takes it: `commitFile`, `commitFiles`,
`commitStagedChanges`, `commitAmend`, `moveFile`, `moveDirectory`, `merge` and
`commitMerge`. That is deliberately the whole class rather than the operations
someone asked for — an operation that writes a commit and cannot record who
performed it is a side door back to collapsed identity.

`CommitInfo` reports both, so what was written can be read back:

```js
const [head] = (await git.getCommitHistory(repo, 1)).commits;
head.authorName;     // 'Ada Lovelace'
head.committerName;  // 'my-agent'
```

### A merge with somewhere to put it

`merge` fast-forwards when the target branch has not moved, and a fast-forward
writes no commit — so there is nothing to sign, and the identity passed to
`merge` goes nowhere. That matters most in the case you would least want it
to: where an agent drafted a change on a branch and a person reviewed and
merged it, the target's tip is then a commit authored by the agent, with no
record anywhere that a human approved it.

`noFastForward` refuses to fast-forward, the way `git merge --no-ff` does:

```js
await git.merge(repo, 'session/rewrite', 'Ada Lovelace', 'ada@example.com', {
  noFastForward: true,
  committerName: 'my-agent',
  committerEmail: 'agent@example.com',
});
// -> { kind: 'merged', ... }  rather than { kind: 'fast-forwarded', ... }
```

An `up-to-date` merge is unaffected: HEAD already contains the branch, so there
is nothing to merge and nothing to attribute.

## Concurrency

Mutating operations take a per-repository lock with two layers: an in-process
mutex, and an OS advisory file lock (`flock` / `LockFileEx`) at
`.git/liminal-git.lock`. The second layer is what makes concurrent *processes*
safe, not merely concurrent threads.

The advisory lock is released by the kernel when the file descriptor closes,
including when a process is killed, so a crash cannot leave a repository wedged.
If a lock cannot be acquired within ten seconds the call fails with
`REPOSITORY_LOCKED`, which is retriable.

This excludes other users of *this library*. It does not exclude `git` itself —
a commit run from a terminal knows nothing about `.git/liminal-git.lock`.

### Holding the lock across a sequence

A per-operation lock makes each operation safe. It cannot make a **sequence**
safe, and a host application's core write path is usually a sequence:

1. write one or more files to the working tree
2. run a validator over the whole tree — often an external process this
   library knows nothing about
3. commit if it passes, restore if it does not

Between steps 1 and 3 another process using this library can legally
interleave. Its edit can land mid-validation, be judged by the wrong validator
run, or be undone by step 3's restore. `acquireRepositoryLock` closes that
window:

```js
const lock = await git.acquireRepositoryLock(repo);
try {
  await writeFiles();
  if (await runValidator()) {
    await git.commitFiles(repo, paths, message, name, email);
  }
} finally {
  await lock.release();
}
```

`release()` is idempotent, so a `finally` that also runs on the success path is
safe. There is no `withRepositoryLock`: the four lines above are the whole of
it, and a wrapper would mean a hand-written JavaScript layer over the generated
binding for no gain a caller cannot get here.

Three behaviours are worth knowing exactly, because a caller builds on them.

**Operations issued while this process holds the lock proceed.** They do not
deadlock and do not wait. They skip the advisory lock, which the held scope
already owns, and still take the in-process mutex, so operations continue to
exclude each other within the process. Skipping both would have been simpler
and wrong: two threads of the holding process could then run operations
concurrently and corrupt the index, which the per-operation lock had been
preventing.

**A second `acquireRepositoryLock` on the same repository is refused**, with
`REPOSITORY_LOCKED` after the timeout, whether the current holder is this
process or another one. It is not handed back as a reentrant handle, because
nested scopes hide bugs about who owns what.

**A crash cannot wedge a repository.** The handle owns the file descriptor and
the kernel drops the advisory lock when the process dies, exactly as for a
per-operation lock. A handle leaked inside a *live* process is the caller's
bug, which is what the `finally` is for.

`timeoutMs` is an optional second argument and defaults to the same ten seconds
every other operation waits.

## Reading at a commit

`getFileAtCommit` and `getTreeAtCommit` are a pair, and the point is calling
both with the same commit hash. The first answers what is in a file at that
commit; the second answers which files are there at all. Together they give a
read-only consumer a snapshot-consistent view of a whole repository **with no
lock held and no writer blocked**:

```js
const { headCommit } = await git.getRepositoryInfo(repo);

const entries = await git.getTreeAtCommit(repo, headCommit);
for (const entry of entries) {
  const { content } = await git.getFileAtCommit(repo, entry.path, headCommit);
  // every file as it stood at one instant, even while a writer commits
}
```

Reading the working tree instead is what this replaces. A concurrent write can
expose file A updated and file B not yet, and a computation over that set is
reading a state that never existed.

Directories are not entries. They are implied by the paths of the files inside
them, and paths come back sorted, so the listing zips directly against the
per-file calls. An entry carries its `kind` (`"file"`, `"symlink"` or
`"submodule"`), the blob `size` in bytes, and the `blobHash`.

The optional third argument filters by path:

```js
await git.getTreeAtCommit(repo, headCommit, 'effort/');
```

By default it is a **literal string prefix** on the repository-relative path
rather than a directory match, so `'effort'` without the slash also matches a
file named `effortless.yaml`. The fourth argument makes it a directory:

```js
await git.getTreeAtCommit(repo, headCommit, 'effort', { directory: true });
```

That matches `effort/plan.yaml` and `effort/nested/deep.yaml`, and not
`effortless.yaml`. It normalises a missing trailing slash, so `'effort'` and
`'effort/'` mean the same thing under it — needing both the option and the slash
would leave the same trap one level up.

It is opt-in rather than the default because switching would silently change
results for anyone relying on prefix matching. A prefix that matches nothing
returns an empty array either way, which is an answer rather than a failure.

Both calls take a **raw commit hash and nothing else** — not a branch, not a
tag, not `"HEAD"`. That is deliberate rather than a missing feature: the single
property the pair depends on is that both calls name the same object, and
accepting a symbolic ref would let two calls in one snapshot resolve
differently if a writer moved the ref in between.

`resolveRef` is how you get the hash. One ref in, one commit hash out:

```js
const commit = await git.resolveRef(repo, 'HEAD');
const entries = await git.getTreeAtCommit(repo, commit, 'effort/');
const { content } = await git.getFileAtCommit(repo, 'project.yaml', commit);
```

It takes `"HEAD"`, a branch name, a tag name, or a full ref path such as
`refs/tags/v1.0.0`. An annotated tag peels through to the commit rather than
stopping at the tag object. A raw 40-character hash resolves to itself, so a
caller that accepts either form does not have to branch on which it got.

Anything that does not name a commit **throws, naming the ref** — never a null.
`REF_NOT_FOUND` covers a ref that does not exist and a ref that peels to
something other than a commit, such as a tag of a blob. `EMPTY_REPOSITORY`
covers a symbolic ref whose target does not exist yet: `"HEAD"` before the first
commit, and also a freshly orphaned branch in a repository that has plenty.
Those are two codes rather than one because a caller does different things with
them — one has no commits *yet*, which is a state that ends and is answered by
making one, and the other names something that was never going to resolve and is
answered by fixing the name. An abbreviated hash is not resolved either: `Oid::from_str`
zero-fills a short string into a different, well-formed oid rather than
rejecting it, and answering about the wrong object silently is worse than
saying no.

It is not a revparse grammar. `HEAD~3` and `main@{yesterday}` are out of scope;
the operation names a commit by a ref that exists rather than navigating from
one.

## Putting a file back

Two operations, and the difference is which version you want back.

`discardChanges` restores one path to **HEAD**, discarding whatever is in the
working tree:

```js
await git.discardChanges(repo, 'notes/draft.md');
```

It goes through a libgit2 checkout, so the index is updated to match and
symlinks, executable bits and CRLF filters are preserved. This is the recovery
for a write sequence that died partway and left a file half-written.

`restoreFileFromCommit` restores one path to **any commit**:

```js
await git.restoreFileFromCommit(repo, 'notes/draft.md', commit);
```

It refuses with `UNSTAGED_CHANGES_WOULD_BE_LOST` when the working copy of that
path differs from HEAD, or exists while HEAD has never heard of it. The check is
**per-path, not repository-wide** — an unrelated dirty file does not block a
restore.

The fourth argument overrides that refusal:

```js
await git.restoreFileFromCommit(repo, 'notes/draft.md', commit, true);
```

Which is the point of it: a dirty working copy is often exactly why you are
restoring. `force` overrides the guard and nothing else — a path absent from the
commit is still `FILE_NOT_FOUND`, and nothing on disk moves. It defaults to
`false`, so no existing call changes behaviour.

## Scope

Remote operations are supported: `listRemotes`, `addRemote`, `removeRemote`,
`setRemoteUrl`, `fetch`, `push`, `getUpstreamStatus` and `clone`.

Merging is supported, and it is performed entirely in memory. `mergeAnalysis`
reports, without changing anything, whether merging a branch into HEAD would be
a no-op, a fast-forward, or a real merge. `fastForward` performs the
fast-forward case, moving the current branch's ref, its working tree and its
index together. `merge` performs the general case: a real three-way merge that
either writes a two-parent commit or reports the three sides of every contested
path and writes nothing at all. `merge` fast-forwards where it can; pass
`{ noFastForward: true }` to require a merge commit instead, so the merge has
somewhere to record who performed it.

The rule the merge operations are built around is that **a merge never leaves an
in-progress state on disk**. There is no `MERGE_HEAD`, no conflicted on-disk
index, no file rewritten with conflict markers, and no detached HEAD at any
point. A caller who walks away from a conflict leaves the repository
byte-for-byte as it was, so an abandoned resolution costs nothing. `pull` is
therefore composable in full: `fetch` → `merge`.

A conflict is detected and reported, never resolved. `readBlob` fetches either
side of a contested path by oid, so the host application can show the writer
what it is choosing between, and `commitMerge` takes back only the paths the
writer actually decided about. The merge is re-run in memory and those decisions
are laid over its result, so a caller never has to reproduce libgit2's merge of
the paths that merged cleanly and cannot get it subtly wrong. If the repository
moved while the writer was deciding, `commitMerge` refuses with `HEAD_MOVED`
rather than committing against a stale base.

`cherryPick` is the same landing with linear history. Where `merge` writes a
two-parent commit, `cherryPick` replays one commit's change onto HEAD as a new
single-parent commit — the case being a session branch that accumulated work
while a scheduled job advanced the target, so `fastForward` is correctly
refused and the caller would rather not have a merge commit:

```js
const hash = await git.cherryPick(repo, sessionCommit, 'my-agent', 'agent@example.com');
```

It is strict in the same way the merge operations are: a conflict is detected,
never resolved. On any conflict it fails with `MERGE_CONFLICT` naming the
contested paths, and the working tree, the index and HEAD are exactly as they
were — no `CHERRY_PICK_HEAD`, no conflict markers, nothing to clean up.

The author is carried over from the commit being replayed, because the change
still belongs to whoever wrote it; the caller signs as committer. `commitHash`
must be the full 40 characters — an abbreviated hash is `INVALID_COMMIT_HASH`
rather than being zero-filled into a different, well-formed oid and applying
the wrong commit. It refuses a merge commit with `INVALID_ARGUMENT` (which of
its sides to replay is not a choice this library makes), a detached HEAD with
`DETACHED_HEAD`, an already-applied commit with `NOTHING_TO_COMMIT`, and a
change set containing a page with unsaved edits with
`UNSTAGED_CHANGES_WOULD_BE_LOST`.

### Two ways a page can be in the way

Every operation that writes the working tree — `checkoutBranch`, `fastForward`,
`merge`, `cherryPick` — refuses rather than overwrite a page the writer has
not committed. Which refusal you get says what to do about it, because the two
call for different remedies:

| The page is | Error | `details.files` | The remedy |
|---|---|---|---|
| tracked, with unsaved edits | `UNSTAGED_CHANGES_WOULD_BE_LOST` | the paths | save or discard the edits |
| untracked | `UNTRACKED_FILES_WOULD_BE_OVERWRITTEN` | the paths | move or delete the file |

An untracked page has no committed history, so it has no *changes* to lose —
what it stands to lose is itself, and telling the writer to "save or discard
your edits" would be advice they cannot act on. Both name the exact paths, so
a host application can offer the right thing.

Untracked files that the incoming tree does not want are none of these
operations' business and never block anything.

**Not** supported, deliberately: `revert`, cherry-pick ranges, and any
automatic conflict resolution strategy. Which of two versions of a writer's
work survives is not a decision this library will make on their behalf.

Credentials are passed **per operation** rather than held by the service. This
library has no business owning secrets; the host application knows where they
came from and how long they may live. Given explicit credentials it uses them;
without them it falls back to ssh-agent and then to git's credential helper, so
a machine where `git push` already works keeps working.

```js
await git.fetch(repo, 'origin', { password: process.env.GITHUB_TOKEN });
await git.push(repo, 'origin', 'main', {
  sshPrivateKeyPath: '/home/me/.ssh/id_ed25519',
});
```

### A runtime dependency on Linux

Enabling HTTPS means OpenSSL, and on Linux it is **dynamically** linked:

```
libssl.so.3     libcrypto.so.3
```

libgit2 and libssh2 are statically linked; OpenSSL is not. So anything
packaging this on Linux must declare that dependency — `openssl` for pacman,
`libssl3` for deb. Formats that cannot declare dependencies, AppImage in
particular, are the weak spot. Windows is unaffected: it uses winhttp and
schannel, and never pulls OpenSSL at all.

git2's `vendored-openssl` feature would link it statically and remove the
runtime dependency, at the cost of build time and of OpenSSL's licence terms
entering [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

### What the tests do and do not cover

Remote operations are tested end to end against **local bare repositories used
as remotes** — git treats a filesystem path as an ordinary remote, so refspecs,
ref updates, ahead/behind and push rejection are all exercised with no network
and no server.

Authentication is not covered, because a local path never asks for any. The
credential callback is exercised by no test, and that gap is real. The same
is true of `clone`'s authentication paths, for the same reason: a local path
or `file://` URL never prompts, so those paths have coverage only from the
error-classification tests (synthesised failures, not a real credential
exchange) — see `clone_reports_a_missing_remote_repository` in
`tests/remote_ops_tests.rs` for the one path that is checked against a real
remote, and only for the not-found case, not authentication.

## Errors

Errors carry a stable code, a message, a retriable flag, and structured details.
With the `structured_errors` feature flag on, they cross the N-API boundary as
JSON:

```json
{
  "code": "REPOSITORY_LOCKED",
  "message": "Repository is locked: /srv/projects/my-repo (waited 10003ms)",
  "retriable": true,
  "details": { "path": "/srv/projects/my-repo", "waitedMs": 10003 }
}
```

**Without the flag the codes do not cross the boundary at all.** Every error
arrives as a `GenericFailure` carrying the plain message, and two different
codes become indistinguishable without string-matching that message — which is
what the codes exist to prevent. A consumer that routes on error type wants
`LIMINAL_FEATURE_FLAGS=structured_errors` set.

The codes are stable:

- **Repository** — `REPOSITORY_NOT_FOUND`, `REPOSITORY_CORRUPTED`, `INVALID_REPOSITORY`, `REPOSITORY_LOCKED`
- **Remote** — `AUTHENTICATION_FAILED`, `REMOTE_NOT_FOUND`, `REMOTE_UNREACHABLE`, `DESTINATION_NOT_EMPTY`, `CLONE_INCOMPLETE`
- **Files** — `FILE_NOT_FOUND`, `FILE_NOT_IN_REPOSITORY`, `BLOB_NOT_UTF8`, `PATH_TRAVERSAL`
- **Operations** — `NOTHING_TO_COMMIT`, `MERGE_CONFLICT`, `UNCOMMITTED_CHANGES`, `UNSTAGED_CHANGES_WOULD_BE_LOST`, `UNTRACKED_FILES_WOULD_BE_OVERWRITTEN`, `DETACHED_HEAD`, `CONFIG_MISSING`
- **Merge resolution** — `HEAD_MOVED`, `UNRESOLVED_CONFLICTS`, `MERGE_NO_LONGER_CONFLICTS`
- **Branches** — `BRANCH_NOT_FOUND`, `BRANCH_ALREADY_EXISTS`, `CANNOT_DELETE_CURRENT_BRANCH`, `BRANCH_NOT_MERGED`, `NOT_FAST_FORWARD`
- **Tags** — `TAG_NOT_FOUND`, `TAG_ALREADY_EXISTS`
- **Refs** — `REF_NOT_FOUND`, `EMPTY_REPOSITORY`
- **Validation** — `INVALID_PATH`, `INVALID_ARGUMENT`, `INVALID_COMMIT_HASH`, `INVALID_BRANCH_NAME`, `INVALID_TAG_NAME`
- **System** — `IO_ERROR`, `GIT_OPERATION_FAILURE`

### What `retriable` means

**True means the same call may succeed if repeated. It is not a claim that it
will.** It says the failure was about the moment rather than about the request,
which is the distinction a retry loop needs: retry this one, surface that one
now.

Retriable:

- `IO_ERROR`, `REPOSITORY_CORRUPTED` and `REPOSITORY_LOCKED` — always.
- `REMOTE_UNREACHABLE` — always. The remote could not be reached at all —
  DNS, a dropped connection, a server that is down — which is exactly the
  condition a second attempt can outlive.
- `CLONE_INCOMPLETE` — only when the partially-created destination was
  removed. A transfer that died after objects had already arrived leaves
  nothing behind once cleanup succeeds, so the retry starts clean; if cleanup
  itself failed, the retry would only reach `DESTINATION_NOT_EMPTY`, so this
  is reported not retriable instead.
- `GIT_OPERATION_FAILURE` — **when it came from the operating system**. A
  libgit2 failure classifies more precisely than this wherever the failure
  itself says which kind it was — `REPOSITORY_NOT_FOUND`,
  `AUTHENTICATION_FAILED`, `REMOTE_NOT_FOUND` and `REMOTE_UNREACHABLE` above
  are all libgit2 failures read this way, from the `class` and `code` in
  `details`. `GIT_OPERATION_FAILURE` is what is left over: a libgit2 failure
  with no code-specific classification, retriable only when the `class` and
  `code` in `details` point at the operating system — libgit2's `Locked`
  code, or its `Os` or `Filesystem` classes. That is the case a retry loop is
  usually written for — a sync client (Google Drive, Dropbox, OneDrive)
  holding a file open while `discardChanges` or a checkout writes it, which
  is routine on Windows and gone a moment later.

Everything else is `false`, including a `GIT_OPERATION_FAILURE` from any other
class. `REMOTE_NOT_FOUND`, `AUTHENTICATION_FAILED`, `DESTINATION_NOT_EMPTY`
and `INVALID_ARGUMENT` are answers about the request, not the moment, so none
of them are retriable either: the remote is not going to start existing, a
rejected credential is not going to start being accepted, an occupied
destination is not going to empty itself, and a URL libgit2 cannot parse is
not going to parse itself, on a second attempt. A checkout conflict is
deliberately not retriable for the same reason: it is an answer about the
tree, not a transient failure to write it.

So the useful shape on the calling side is:

```js
if (parsed?.retriable) { /* back off and try again */ }
else { /* surface it now — a second attempt gets the same answer */ }
```

A blanket "retry everything three times" spends the backoff on `DETACHED_HEAD`
and `PATH_TRAVERSAL`, which will never succeed on a second attempt.

### The codes are API surface

Consumers branch on `code`, so a code is a promise in the way a TypeScript
signature is, and it is versioned the same way:

- **Renaming or removing a code is a breaking change.** Major bump, or at
  minimum a headline in the release notes — never a line under "also in this
  release".
- **Narrowing what an existing code covers is breaking too**, even though
  nothing is renamed. When `EMPTY_REPOSITORY` split out of `REF_NOT_FOUND`, a
  consumer matching `REF_NOT_FOUND` silently stopped seeing the empty-repository
  case.
- **Adding a code is not breaking**, provided it does not narrow an existing
  one.

This is written down because it was got wrong once. `UNBORN_HEAD` became
`EMPTY_REPOSITORY` in 1.6.0 and the change was described as free on the grounds
that the code was new and unadopted — an assumption about one consumer's
timeline rather than a fact, and it was wrong. Nothing throws when a code
changes underneath a caller; the branch just stops matching.

**This release narrows `GIT_OPERATION_FAILURE`.** A libgit2 failure that used
to arrive under that one code now classifies more precisely wherever the
failure itself says which kind it was: `REPOSITORY_NOT_FOUND`,
`AUTHENTICATION_FAILED`, `REMOTE_NOT_FOUND`, `REMOTE_UNREACHABLE` and
`INVALID_ARGUMENT` all split out of it. A consumer matching
`GIT_OPERATION_FAILURE` for a mistyped path, for instance, now sees
`REPOSITORY_NOT_FOUND` instead — the same case `UNBORN_HEAD` was, recorded
here rather than repeated.

## Feature flags

Set via the `LIMINAL_FEATURE_FLAGS` environment variable, read once when a
`GitService` is constructed. Matching is case-insensitive and comma-separated;
partial names do not match.

```bash
export LIMINAL_FEATURE_FLAGS=structured_errors,enhanced_status
```

- `structured_errors` — return the JSON payload above instead of a plain message
- `enhanced_status` — additional metadata on status results
- `enhanced_diff` — populate diff hunks with line content

All default to off.

## Configuration

Checkout behaviour is read from git config rather than passed per call, so it
can be set per repository:

```bash
git config liminal.checkoutStrategy safe    # default: block only on real conflicts
git config liminal.checkoutStrategy force   # overwrite local changes
```

In `safe` mode, `checkoutBranch` blocks only when a file would actually be
overwritten, and reports exactly which — not every dirty file. `fastForward`
honours the same setting the same way, since it also has to move the working
tree.

## Logging

Set `LIMINAL_LOG` to enable logging to stderr; the value is an
[`env_logger`](https://docs.rs/env_logger) filter string.

```bash
LIMINAL_LOG=info node app.js
```

## Development

```bash
cargo build --no-default-features    # library only
npm run build                        # the Node addon (napi build --release)
```

### Tests

```bash
cargo test --no-default-features
```

367 tests across ten targets. `--no-default-features` is required rather than
preferred: with the `napi-binding` feature on, a test binary fails at the
**linker**, because napi resolves its symbols from the host Node process at run
time and those symbols do not exist in a test executable. Disabling the feature
tests the pure-Rust layer, which is where all the logic lives — `git_service.rs`
is only the N-API adapter over it.

Some tests set `TMPDIR` to work around cross-device link failures on mounted
filesystems and are marked `#[serial_test::serial]`. If you see unexplained
flakiness, `-- --test-threads=1` will tell you whether that is the cause.

### Lint and format

```bash
cargo fmt
cargo clippy --no-default-features --all-targets -- -D warnings
cargo clippy --lib -- -D warnings
```

Both clippy invocations matter: the two feature sets compile different code, and
clean under one can be dirty under the other. CI runs both.

### Coverage

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --no-default-features
```

### CI

Every push and pull request runs the full matrix on Linux, macOS (Apple Silicon
and Intel) and Windows. A single platform can be targeted manually when
iterating on something platform-specific:

```bash
gh workflow run ci.yml -R liminalfield/liminal-git -f platforms=windows
```

It checks formatting, clippy under both feature sets, the test suite, the
declared MSRV, and — the check that matters most — that the package installs and
loads from a *packed tarball*, not merely that it builds inside a clone. Those
are different things, and the difference once hid a package that shipped every
Rust source file and no binary.

### Releasing

Publishing happens in CI and nowhere else. A local `npm publish` would ship
whatever that machine happened to have built, from whatever source tree it
happened to have, with no record of either.

1. Bump the version in `Cargo.toml` and `package.json`, run `cargo update -p
   liminal-git`, and **regenerate the bindings** with `npm run build`.
   `index.js` embeds the package version in its native-binding guard, so a bump
   without a rebuild ships a package that throws at `require` time. That is
   what went wrong with 1.3.1, and it is unfixable in place because a version
   number can never be reused.
2. Commit and push to `main`. Wait for CI to go green.
3. Tag the green commit and push the tag.

Pushing a `v*` tag runs the whole matrix again on that commit and then, only if
every job passes, publishes. The publish job checks the tag against
`package.json`, collects the four addons the matrix just built and loaded,
verifies each declared target has one, dry-runs, publishes the four platform
packages, and publishes the main package last — so it never exists on npm
pointing at platform packages that do not.

**There is no npm secret in this repository.** All five packages use npm
trusted publishing, so `npm publish` exchanges the OIDC token from the job's
`id-token: write` permission for a short-lived registry token. Nothing to leak,
to expire, or to paste in empty — which is how the first attempt at 1.6.0 failed.

The trust is bound to this repository **and this workflow's filename**, which npm
matches exactly and case-sensitively. Renaming `ci.yml` breaks publishing until
all five package configurations on npmjs.com are updated to match.

The first release could not work this way: npm requires a package to **exist**
before a trusted publisher can be configured for it, and there is no way to
publish a first version over OIDC ([npm/cli#8544][oidc-first]). 1.6.2 went out on
a token, which was revoked once the packages existed and the trust was configured.

[oidc-first]: https://github.com/npm/cli/issues/8544

The binaries published are the artifacts the matrix built and tested, not a
later recompilation of the same source that nobody exercised.

## Architecture

```
src/
├── lib.rs              module exports
├── types.rs            data structures shared with JavaScript
├── feature_flags.rs    feature flag parsing
├── git_service.rs      N-API bindings — the only napi-aware module
├── validation.rs       input validation, returns GitError
├── utils.rs            path handling, repository locking
├── errors.rs           GitError and its JSON serialisation
├── repository_ops.rs   init, status, info, health
├── file_ops.rs         stage, commit, move, restore
├── history_ops.rs      history, diffs, file-at-commit
├── branch_ops.rs       branch management
├── merge_ops.rs        in-memory merge, conflict reporting, resolution
├── remote_ops.rs       remotes, fetch, push, upstream status
└── tag_ops.rs          tag management
```

The `*_ops` modules are the library. Each public operation is a `*_impl`
function returning `Result<T, GitError>` with no knowledge of Node, and
`git_service.rs` is a thin layer that validates its arguments, takes the
repository lock, and converts errors at the boundary. That split is why the
tests can run at all, and it is what a second, non-Node consumer would build on.

## License

MIT — see [LICENSE](LICENSE).

The compiled addon is not only this project's code. libgit2 is **statically
linked** into every binary, under GPLv2 *with a linking exception*. That
exception is what lets liminal-git be MIT, and lets anything linking liminal-git
choose its own terms; the GPL still governs libgit2 itself.

Crate metadata will not tell you this — `libgit2-sys` declares
`MIT OR Apache-2.0`, which describes the Rust binding rather than the C library
it vendors and compiles in. See [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md),
which ships with the package.
