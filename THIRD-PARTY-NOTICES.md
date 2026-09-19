# Third-Party Notices

`liminal-git` is MIT licensed (see [LICENSE](LICENSE)). The compiled addon it
produces is not only liminal-git: it is a single `.node` file with several other
projects' code linked into it. This file records what those are.

Verified against the published 1.10.0 binaries for all three platforms. Figures
come from reading the binaries — embedded version strings, ELF `NEEDED` entries,
Mach-O load commands and PE imports — not from crate metadata. See the warning
under libgit2 for why that distinction matters.

---

## libgit2 — GPLv2 **with a linking exception**

This is the one that needs attention.

libgit2 is **statically linked** into every distributed binary. There is no
`libgit2.so` dependency; the code is compiled in. All three platform binaries
embed the version string `libgit2 1.9.7`, from the source tree vendored by
`libgit2-sys 0.18.8`. On Linux x64 the 10.7 MB addon carries 1256 `git_*`
symbols.

> ### Why a license scanner will not tell you this
>
> The `libgit2-sys` crate declares `license = "MIT OR Apache-2.0"`. That describes
> the Rust binding, not the C library it vendors and compiles. A tool that reads
> crate metadata — `cargo-deny`, `cargo-license`, GitHub's dependency graph —
> reports this dependency tree as uniformly permissive and never mentions the GPL
> at all. The GPL-licensed code is real, it is in the binary, and the metadata
> does not know.

libgit2 is Copyright (C) the libgit2 contributors. Its license is GPL version 2 —
and only that version — plus the following exception, quoted verbatim from the
libgit2 `COPYING` file:

```
			LINKING EXCEPTION

 In addition to the permissions in the GNU General Public License,
 the authors give you unlimited permission to link the compiled
 version of this library into combinations with other programs,
 and to distribute those combinations without any restriction
 coming from the use of this file.  (The General Public License
 restrictions do apply in other respects; for example, they cover
 modification of the file, and distribution when not linked into
 a combined executable.)
```

**What this means in practice.** The exception is what makes this arrangement
work: linking libgit2 into a combined binary and distributing that binary carries
no GPL obligation on the rest of the combination. liminal-git stays MIT, and so
does anything that links liminal-git. This is the same basis on which every other
libgit2 consumer operates.

The GPL still governs libgit2 itself. Two consequences worth remembering:

1. **Modifying libgit2** — as opposed to linking it — puts those modifications
   under GPLv2. This project does not modify it; it consumes the unmodified
   vendored source through `libgit2-sys`.
2. **Distributing libgit2 not linked into a combined executable** is outside the
   exception. Shipping the `.node` addon is inside it.

Preserve this notice in anything that redistributes the compiled addon.

Full license text: `libgit2/COPYING` inside the `libgit2-sys` crate source, and
<https://github.com/libgit2/libgit2/blob/main/COPYING>.

I am not a lawyer, and this section is a description of what the license says
rather than legal advice. If liminal-git ever ends up inside something sold under
terms that make the GPL relationship consequential, have someone qualified read
it.

---

## zlib

Linkage is **platform-dependent**, and differs between the published binaries:

- **Linux x64** — dynamically linked against the system zlib (`libz.so.1`). Not
  redistributed.
- **macOS arm64** — dynamically linked against the system zlib
  (`/usr/lib/libz.1.dylib`). Not redistributed.
- **Windows x64** — **statically linked**. `libz-sys` vendors zlib there, and the
  binary embeds `deflate 1.3.1`. This notice travels with the Windows binary.

Verify on any platform you ship with:

```sh
ldd <binary> | grep -i libz          # Linux
otool -L <binary> | grep -i libz     # macOS
```

zlib is Copyright (C) 1995-2024 Jean-loup Gailly and Mark Adler, under the zlib
license — permissive, with no attribution requirement for binary distribution.

## libssh2

Statically linked on all three platforms, from vendored C source, because
`remote_ops` supports SSH remotes. The binaries embed `libssh2_1.11.1`. libssh2
is Copyright (c) the libssh2 contributors, under the
**BSD 3-Clause** license — permissive, requiring that the copyright notice and
disclaimer be retained in redistributions, which this file does.

Source: <https://github.com/libssh2/libssh2>

## OpenSSL — used, but not distributed

This is the one place where the distinction between *linking against* and
*shipping* a library changes the obligation, so it is worth being exact.

On **Linux**, OpenSSL is **dynamically** linked. The compiled addon records a
dependency on `libssl.so.3` and `libcrypto.so.3` and resolves them from the
host at load time; no OpenSSL code is contained in, or distributed with, this
package. The licence obligations that attach to redistributing OpenSSL
therefore do not arise here — but the *runtime dependency* does, and anything
packaging this must declare it.

On **macOS**, OpenSSL is also **dynamically** linked, but not from the system —
macOS ships no OpenSSL. The published arm64 binary carries hard load commands on
Homebrew's copy at an absolute path:

```
/opt/homebrew/opt/openssl@3/lib/libssl.3.dylib
/opt/homebrew/opt/openssl@3/lib/libcrypto.3.dylib
```

No OpenSSL is redistributed, so again no redistribution obligation arises. The
runtime consequence is serious, though: on a Mac without Homebrew's `openssl@3`
at exactly that path, the addon fails to load. This is a defect in the macOS
build, not a requirement consumers are expected to meet.

On **Windows**, OpenSSL is not involved at all: the binary imports no OpenSSL,
and libgit2 uses winhttp and schannel instead.

If `vendored-openssl` is ever enabled, OpenSSL becomes statically linked and
this section must change: it would then be redistributed, and OpenSSL's terms
(Apache-2.0 for 3.x; the dual OpenSSL/SSLeay license before that) would need
reproducing in full.

Verify on any platform you ship with:

```sh
ldd <binary> | grep -iE 'ssl|crypto'      # Linux
otool -L <binary> | grep -iE 'ssl|crypto' # macOS
```

---

## Rust crates

80 crates are compiled into the Linux x64 addon with default features. All are
permissive. Tally by declared SPDX expression:

| licenses | crates |
|---|---:|
| MIT OR Apache-2.0 (in either spelling) | 46 |
| Unicode-3.0 | 18 |
| MIT | 9 |
| Unlicense OR MIT | 5 |
| (MIT OR Apache-2.0) AND Unicode-3.0 | 1 |
| Apache-2.0 OR BSL-1.0 | 1 |

The 18 Unicode-3.0 crates are the ICU family, reached through `url` → `idna`.
That license requires its notice be retained in distributions; it is permissive
and imposes no copyleft.

Where a crate offers a choice, this project takes the MIT option where available,
for consistency with its own license.

To regenerate the tally:

```sh
cargo tree -e normal --target x86_64-unknown-linux-gnu --prefix none \
  | awk 'NF>=2 {print $1" "$2}' | sort -u
```

then read `license` from each crate's `Cargo.toml` in the cargo registry. Note
again that this reflects *declared* metadata only, and so will not surface
vendored C code — which is exactly how libgit2's GPL would be missed.
