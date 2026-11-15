# ADR 0001: Path-based articles and source timestamps

Status: Implemented (2026-11-16).

## Context

Epoch filenames cannot express natural-language paths or reliable creation and modification dates. Filesystem modification times also change when content is checked out, so a Git-managed document root needs dates derived from committed history. Sorted lists and monthly archives require a complete metadata view before serving requests.

## Decision

`SHIORI_ARTICLES_DIR` defaults to `./articles` and `SHIORI_ASSETS_DIR` defaults to `./assets`, relative to an existing document root. Both directories are created with the execution user's privileges when absent. Absolute paths, parent traversal, symlink traversal and overlapping directories are rejected. No privilege escalation is used.

Article keys are recursive article-directory-relative UTF-8 paths with the final `.md` or `.markdown` extension removed. Duplicate keys are errors. Generation retains epoch-millisecond filenames and atomic publication. Asset URLs retain their relative hierarchy under `/assets`, while responsive images retain `/images`; storage configuration does not change these HTTP mounts.

When the root belongs to a Git worktree, a Rust Git library resolves committer dates from current-path history without following renames. Creation is the current path's addition and modification is its latest change. A move or re-addition creates a new path lifetime. Select addition and change commits by standard Git current-path history order without rename following or a first-parent shortcut. Preserve their recorded committer dates even if clocks go backward; do not choose commits by timestamp minima/maxima or clamp modification after creation. Fixed merge and skewed-clock fixtures must identify the same commits as the standard no-rename Git path log. Dirty tracked files retain committed dates; files absent from HEAD use filesystem dates. Outside Git, creation uses filesystem birth time with modification-time fallback, and modification uses filesystem modification time. Unix ctime is never a creation date. Incomplete or corrupt history produces an explicit error rather than silently selecting filesystem mode.

The initial catalog resolves and caches all article metadata before serving requests. File changes refresh affected metadata; periodic reconciliation detects Git HEAD changes, including date-only changes. GraphQL requests use immutable catalog snapshots without history traversal. Invalid article content or metadata retains that article while other valid updates can publish. Enumeration, storage-root, identity, duplicate-key, and Git-history failures retain the entire prior catalog. Startup rejects invalid regular Markdown or configured storage roots. New source symlinks are excluded; replacement of a published file with a symlink retains that article, and an untrustworthy published ancestor fails the whole scan. Every failure reports its cause. Metadata-only changes also change the preview revision.

`Post.id` and `post(id:)` retain their GraphQL names and use string article keys. `createdAt`, `updatedAt` and `sourcePath` are required strings. Dates use UTC ISO 8601 with millisecond precision and years 1 through 9999, consistently across ingestion, cursors and calendar APIs. Unsupported dates are explicit metadata errors; a committed Git date never silently falls back to filesystem time. `sourcePath` is relative to the Git worktree when present, otherwise to the document root, and never exposes an absolute path. Lists and search sort by creation date descending and UTF-8 key bytes ascending. UTC monthly archives use creation dates. Versioned opaque cursors carry the creation date and key; numeric cursors are not supported.

## Consequences

Startup includes metadata/history resolution so the first query has correct global ordering and archive counts. History is cached in memory rather than queried per request. Operators can explicitly select `./contents` to retain an existing storage location; no content is moved automatically. Committing a path move changes its creation date by design. Git CLI installation, a database and front-matter dates are unnecessary.

Tests use fixed Git history fixtures without creating commits or signing keys. Product verification runs without sudo. Markdown prose uses one source line per paragraph.

The reference selection is the first entry of `git log --no-renames --format='%H %ct' -- <worktree-relative-path>` for modification, and the first entry with `--diff-filter=A` for creation. The selected `%ct` values are used unchanged. Fixed fixtures cover a TREESAME merge, a merge changed against both parents, deletion/re-addition and a descendant whose clock predates its parent. This reference is a verification tool, not a runtime dependency.

## Directory replacement and cache lifetime

Directory references are disposable process-local state. A replacement detected during an asset operation cancels that operation with a retryable error; the next ordinary operation resolves and validates the configured path again. Do not continue serving the retired directory or add a separate tracking service. Raw delivery, image inspection/validation and preview share this policy, and a mid-read change cancels the read. Structured invalidation diagnostics are sufficient for this transient cache; no persistent business state or Header-Detail aggregate is introduced.

## Engineering defaults and regression requirements

The current defaults are 8192 UTF-8 bytes per article key, 64 nested article directories and a 30-second periodic HEAD reconciliation interval. These are implementation choices, not original human requirements: bound request/catalog work and retain the existing reconciliation cadence. URL and cursor encoding must remain consistent with transport limits.

Changing the date source must preserve inclusive month starts and exclusive next-month starts, common/leap February, year rollover, December 9999, and years 1-99 without an implicit 1900 offset. Body/resource verification must select the articles inside `postsByMonth.nodes`, rather than a separate `post` field. Cross-review findings and validation provenance are tracked in Beads; tests passing before remediation do not close those findings.
