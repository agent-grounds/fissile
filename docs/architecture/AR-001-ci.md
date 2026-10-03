# AR-001-ci: CI mirrors the local quality gate and records performance

The CI workflow is the remote form of the local development gate. Anything that
can make the crate unbuildable, untestable, unformatted, ungrounded, or too slow
must fail in CI, so contributors cannot bypass the checks by skipping local hooks
or editing through a web UI. This supports §GOAL-001-fast-feedback and
§GOAL-003-friendly-output.

## 1. Matrix

The Rust build and test matrix runs on Linux, macOS, and Windows. Each leg
installs stable Rust with `rustfmt` and `clippy`, restores Cargo caches on a
best-effort basis, checks formatting, builds all targets with warnings denied,
runs all tests, and runs clippy with warnings denied.

Cache failures are not build failures. A cold cache must still reach the actual
format/build/test/lint steps and let those steps decide pass or fail.

## 2. Grounding

The Linux leg runs `grund check` so docs, source citations, and architecture
references stay valid. This is separate from Rust compilation: `cargo` proves the
crate builds, while `grund` proves the project explanation still resolves.

## 3. Packaging

The Linux leg runs `cargo package --locked --list` as a cheap packaging sanity
check. It verifies that Cargo can assemble the crate contents under the locked
dependency graph without doing a publish.

## 4. Performance smoke guard

CI carries a cheap wall-clock backstop for §GOAL-001-fast-feedback: the matrix
runs the `large_batch_smoke` release test under a generous 30 second timeout. The
budget itself is much tighter than that; this guard is for catastrophic
regressions such as an accidental quadratic path or a repeated scan over every
file. The precise per-commit meter is the benchmark job in §5.

The test binary is compiled in a separate step before the timed one, so the
clock measures the scan and not the compiler: the release profile is LTO with a
single codegen unit, and a link that grows with the dependency tree would
otherwise register as a runtime regression it is not.

## 5. Benchmark job

A separate Linux-only `bench` job runs the instruction-counting harness
(§AR-002-instruction-benchmarks). The job installs Valgrind and the
`iai-callgrind-runner` version that matches the crate dependency. On pull
requests it first records a base-branch baseline, then reruns the pull request
with `--callgrind-limits=ir=5.0%`; instruction-count growth beyond that limit is
a build failure. Pushes to `main` record current counts and upload the JSON
summaries for inspection.

The benchmark body is gated behind the `bench` Cargo feature, so ordinary build
and test jobs compile a no-op bench target and never require Valgrind.

## 6. PGO pre-release check

PGO stays out of push and pull-request CI. The manual `Pre-release checks`
workflow installs `llvm-tools-preview` and runs `scripts/pgo-build.sh`. That
keeps the ordinary feedback loop focused on format, build, test, lint,
grounding, smoke, and instruction counts, while still proving the PGO toolchain
before a release.

The PGO script trains on two instrumented workloads before merging one profile:
the release test suite, and the `fissile` CLI hot commands (`check` and `audit`)
run over this repository. Training the real commit-time path keeps the profile
aligned with §GOAL-001-fast-feedback rather than with test scaffolding. The
merged profile then drives a final profile-use rebuild of the release artifacts
under `target/release`.

## 7. Binary-size guard

The pre-release workflow strips the release binary and fails if it exceeds a
documented ceiling, closing the loop on the footprint promise
(§GOAL-002-tiny-footprint.3). The ceiling is generous relative to the current
artifact; it exists to catch a dependency or feature that silently inflates the
single-binary contract, not to police small movements.

## 8. Release workflow

Releases are a workflow, not a checklist. `release.yml` runs on a `v*.*.*` tag
push or a manual dispatch that names the version, and it publishes everything a
consumer can install (§GOAL-002-tiny-footprint.1):

- **Verify.** The requested version must match `Cargo.toml` exactly, and when
  crates.io publishing is requested the registry token must be present before
  any long build starts.
- **Build.** PGO release binaries (§6) for six targets: `x86_64` and `aarch64`
  Linux built inside pinned manylinux2014 containers so the glibc baseline
  cannot drift, plus native macOS (Intel and Apple silicon) and Windows
  (`x86_64` required, `aarch64` with an LTO fallback when PGO training fails on
  hosted runners). Every binary must answer `fissile --version` with the version
  being released before it is packaged (§FS-006-cli.3), and every artifact is
  measured against the §7 size ceiling.
- **Publish.** `cargo publish` runs only after all binaries built, and skips
  silently when the exact `fissile@<version>` already exists so a re-run after a
  partial failure is safe. The GitHub release uploads one archive plus a
  `.sha256` per target and takes its notes verbatim from the released section of
  `docs/changelog.md` via `scripts/prepare_changelog_release.py`. That section is
  written by the same script from the pull requests merged since the previous
  release (§AR-001-ci.8.3); nobody writes release notes by hand.

Two helper workflows prepare versions but never publish by themselves:
`auto-bump.yml` (scheduled) proposes a patch bump when substantive commits have
landed since the last tag and CI on the tip is green, and `release-minor.yml`
(manual) does the same for a minor bump. Both update `Cargo.toml`, the
version-pinned e2e case (§FS-006-cli.3), and the changelog via the same script,
then dispatch `release.yml`.

### 8.1 What the automation needs

Releases are meant to run without a human in the loop, so the standing state a
release depends on is recorded here rather than in someone's memory:

- **`CARGO_REGISTRY_TOKEN`** — a crates.io API token scoped to publish-update
  (and publish-new for the first release). `release.yml` fails fast when it is
  missing rather than after the build matrix.
- **`RELEASE_PAT`** — a fine-grained GitHub token with *Contents: read+write*,
  *Actions: read+write* and *Pull requests: read* on this repository. The bump
  workflows push the version commit straight to `main` and dispatch
  `release.yml`; the default `GITHUB_TOKEN` can do neither, because `main`
  requires pull requests and a `GITHUB_TOKEN` push never triggers another
  workflow. Both bump workflows also pass it to the preparation step as
  `GH_TOKEN`, which reads the merged pull requests (§AR-001-ci.8.3), and declare
  `pull-requests: read` among their permissions. It expires — a silently failing
  Monday bump is the symptom.
- **Full history and `gh`** — preparation lists the commits between the previous
  tag and the candidate tip, so the bump workflows check out with
  `fetch-depth: 0`, and it asks GitHub about them through the `gh` CLI the hosted
  runners carry. Running `prepare` locally needs the same: a complete clone and a
  `gh` that can read this repository's pull requests.
- **Repository rulesets** — `main protection` (pull requests, linear history,
  the three `cargo test` matrix jobs as required checks) and `release tags`
  (`v*.*.*` cannot be deleted or force-moved). Both list the repository-admin
  role as a bypass actor, which is what lets the `RELEASE_PAT` push land.

The scheduled bump derives the next version from the latest `v*.*.*` tag, so a
repository with no tag yet cannot bootstrap itself: the first release is cut by
pushing `v<version>` (or dispatching `release.yml` with the version). Every
release after that is automatic.

### 8.2 Between releases, main carries a dev version

A release leaves main holding the version it just published, so every build from
main until the next release reports the tag it is already ahead of. Nothing then
distinguishes a binary built from main from the released one, and a fix that is
merged but not installed looks exactly like one that is installed.

The release therefore advances main as its last act: after publishing `X.Y.Z` it
commits `X.Y.(Z+1)-dev`. The suffix is what makes `fissile --version` say which
side of the tag a build came from. A `-dev` manifest is never publishable — the
release path sets and verifies the clean version on its candidate branch — so
the guarantee that a released version has a tag is unchanged.

### 8.3 The release notes are the merged pull requests

A release's notes list the pull requests merged into `main` since the previous
release, each by its title. They are written by
`scripts/prepare_changelog_release.py prepare <version>` with no hand-written
input, because a step that waits on someone writing prose makes the automatic
release of §AR-001-ci.8.1 a manual one.

#### 8.3.1 The range

Before it asks anything, preparation freezes `HEAD` as the candidate tip. The
previous release is the highest tag of the exact form `vX.Y.Z`, compared as a
semantic version, that the frozen tip can reach; tags of any other form
(`v0.11.1-rc1`, `0.11.0`) and tags the tip cannot reach are not candidates. The
tag's version must equal the release inline in `docs/changelog.md`. The range is
`<tag>..<frozen tip>`: the tag's commit is out, the tip is in. Both bump
workflows pick their current version by the same rule.

#### 8.3.2 Acquisition is complete or it fails

The commits of the range are listed locally. For each one, preparation asks
GitHub's commit-to-pull-request endpoint which pull requests it belongs to,
reading every page (`gh api --paginate --slurp`), and deduplicates the numbers.
A pull request is listed only when all three hold:

- it is merged;
- its base is `main` of this repository;
- its `merge_commit_sha` is a commit of the range.

The third rule is what places a rebase merge: GitHub sets that SHA to the tip of
the base branch the merge produced, so the original branch commits are never
consulted, and a pull request merged after the frozen tip is left out. The
repository is `GITHUB_REPOSITORY`, or the GitHub `origin` remote when run
locally. Which files a pull request changed is never read.

#### 8.3.3 Every merged pull request is listed

There is no filter. A docs-only or CI-only pull request is listed like any
other: the notes say what was merged, while whether a scheduled patch release
happens at all is the separate question `auto-bump.yml`'s substantive-change gate
answers from its own diff. A commit pushed straight to `main` without a pull
request, such as a version commit, produces no line.

#### 8.3.4 Order and format

The list is ordered by `merged_at`, newest first; pull requests merged at the
same instant are ordered by number, highest first. Each is one line:

```markdown
- [<title>](https://github.com/<owner>/<repo>/pull/<n>) (PR #<n>)
```

Markdown in a title is backslash-escaped, so the title shows as the literal text
it is on GitHub. The list is the whole body of the release section.

#### 8.3.5 The changelog keeps its shape, without a pending section

`docs/changelog.md` has no `## Unreleased` section and no conventions for
writing entries. Its top-level sections are `## 1. [<version>] — <date>`, the
latest release inline, and `## 2. Older releases`. Preparation archives the
previous inline release to `docs/changelog/<version>.md` exactly as before,
relative links rewritten, links it at the top of the older releases with a
one-line summary, and puts the new section in its place as `## 1.`. A summary is
taken from an older prose body as before; for a generated list it is the first
pull request's title. Existing archives and the archive links already listed are
never rewritten, and `notes <version>` extracts the inline section unchanged.

#### 8.3.6 Refusals

Each of these exits 1 with an error that names it, and leaves
`docs/changelog.md` and every archive byte-for-byte as they were:

- no reachable `vX.Y.Z` tag, or one that differs from the inline release;
- a shallow clone;
- no merged pull request in the range;
- an invalid version, date or changelog, or a release or archive that already
  exists;
- malformed data from GitHub;
- an authentication or API failure, including any page that could not be read.

Everything is read and validated before anything is written, and when writing
fails part-way the archive already created is removed again, so a failed
preparation never leaves a partial changelog behind.
