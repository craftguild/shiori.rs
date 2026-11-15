# Operating shiori.rs

## Files and generation

The selected root must exist. `SHIORI_ARTICLES_DIR=./articles` and `SHIORI_ASSETS_DIR=./assets` select directories relative to it. Both `generate` and `serve` create missing directories and parents as the execution user, without privilege escalation. Absolute paths, parent traversal, symlinks and overlapping storage directories are rejected. Existing directories need no write permission for serving. Set `SHIORI_ARTICLES_DIR=./contents` explicitly to retain old storage; no files are migrated automatically.

Articles are UTF-8 ordinary `.md` or `.markdown` files, discovered recursively. The key is the relative path without its final extension: `articles/記事1.md` is `記事1`, and `articles/foo/記事2.markdown` is `foo/記事2`. Two files with the same key are an error. `generate` retains its epoch-millisecond filename convention, independently of publication dates. Each article starts with YAML front matter:

```yaml
---
title: Hello world
description: A short summary
author: Display name
tags: [Rust, Notes]
---
Article body.
```

Titles are required and nonblank; optional description/author values are trimmed strings, with blank authors represented as null. Tags are trimmed, deduplicated in authored order and matched case-sensitively. All valid articles are public, including future-dated IDs. There is no draft lifecycle or authenticated writing API.

Use an unprivileged service account with read/search access to the root and its files. Authors need write access; the server has no database, persistent index or disk-cache write requirement. Prefer same-directory temporary files followed by rename when saving. Non-Markdown editor temporaries are ignored, and article subdirectories are scanned recursively; invalid regular Markdown candidates fail initial startup.

Generation reserves an unused asset directory and publishes the article with an atomic hard link, retrying clock collisions without overwriting existing content. An unpublished empty reservation is removed on failure; published articles retain their assets even if a later flush/cleanup fails. The article and asset directory are not a multi-file transaction: after a crash, inspect any empty orphan reservation before manually removing it.

## Networking and GraphiQL

```sh
shiori --root /srv/blog serve \
  --bind 127.0.0.1:4000 --public-base-url https://cms.example/blog/
```

The global root defaults to the current directory. The listener defaults to `127.0.0.1:4000`, independently of public base `http://127.0.0.1:4000`. The public base must be absolute HTTP(S), without credentials, query or fragment; an optional path prefix is supported. Set it to an address browsers can reach and restart after changing it. IPv6 literals need brackets, for example `--bind '[::1]:4000'` and `--public-base-url 'http://[::1]:4000'`.

The binary serves HTTP; a reverse proxy terminates TLS and strips any public prefix when forwarding `/graphql`, `/assets/...` and `/images/...`. All origins may use GET, HEAD, POST and OPTIONS without credentials; CORS provides no authentication. GraphQL GET requests require `x-chigiri-preflight: 1`. Configure proxy bounds deliberately and do not cache GraphQL responses without an application policy. Host and forwarding headers never select the configured public origin.

Enable `--graphiql` to expose `/graphiql`, its embedded scripts/CSS/workers/fonts/notices and schema introspection. Without it, editor routes return 404 and introspection stays disabled. With a public prefix, forward prefixed `/graphiql` and `/graphiql/assets/...` as well. GraphiQL prefix segments accept ASCII letters, digits and `._~-`; unsupported prefixes fail startup in this mode. No CDN, npm installation or browser renderer is needed by the executable.

## Querying articles

`createdAt` and `updatedAt` are UTC ISO 8601 strings with millisecond precision. Inside a Git worktree, committer dates come from current-path history without following renames: addition creates the path and the latest path change updates it. Moving or re-adding a file resets creation. Dirty tracked files retain committed dates. Files absent from HEAD use filesystem birth time and modification time, as do files outside Git; birth-time failure falls back to modification time, never Unix ctime. The executable uses a Rust Git library and does not require the Git command. Shallow or corrupt history is an explicit error. `sourcePath` is worktree-relative in Git and document-root-relative otherwise, never absolute.

Startup resolves the complete article metadata cache before serving. Changes and periodic HEAD reconciliation refresh it; requests do not traverse Git history. Date-only changes invalidate preview revisions as well. See [ADR 0001](adr/0001-article-storage-and-timestamps.md).

The complete [schema](../graphql/schema.graphql) is checked into source. `posts` defaults to 20 entries and accepts `first`, `after`, `before`, an exact `tag` and literal `query` search. Results are ordered by creation date descending, then UTF-8 article-key bytes ascending. Opaque versioned date/key cursors remain boundaries even if the referenced article is deleted; old numeric cursors are invalid. Pass only one non-null direction and reset cursors when changing filters. `totalCount` counts the filtered catalog before pagination; empty pages have null cursors and false page flags.

`postsByMonth(year, month)` uses UTC creation time, including the month's start and excluding the next month's start. Years range from 1 to 9999 and months from 1 to 12; this query has no tag argument. `monthlyArchives` lists populated UTC months globally, newest first, without rendering. Malformed cursors, invalid page arguments and invalid month values return `BAD_USER_INPUT`.

Search covers titles, raw Markdown bodies and tags through Unicode-lowercase substring matching. Every Unicode-whitespace-separated term must match, possibly in different fields. Code blocks and link destinations are searchable; authors and descriptions are excluded. The input is bounded to 256 Unicode scalar values and 16 terms. Search uses one immutable request snapshot and bounded blocking workers.

Select `post(id)` metadata without rendering. `blocks` and `assets` share one memoized render result per request; selected producer Null entries are removed while empty HTML remains. Math, Mermaid and PlantUML are rendered inside the binary with raw authored HTML disabled. Rendering failure yields a nullable field and `RENDER_FAILED`, preserving unrelated metadata. Returned resources retain their IDs, kinds, MIME types and encoding: `font/otf` is base64 STIX Two Math; SVG/CSS resources are text. Applications own layout and resource application.

`structured` exposes the typed document projection without HTML rendering or image processing. Its IDs are document-local; references, footnotes and anchors retain their canonical producer relationships.

## Assets and responsive images

`/assets/<path>` serves live public regular files, rejecting symlink components, traversal, directories and oversized reads. Responses have content-hash ETags, MIME by extension, `Cache-Control: no-cache`, nosniff and public CORS; HEAD and 304 are supported. Missing, unsafe and oversized assets return 404; invalid decoded paths return 400. Content files and arbitrary root files are never asset routes.

A detected asset-directory replacement cancels the current operation with 503 and `no-store`. The next operation reopens and validates the configured directory. A symlink or unavailable storage directory stays unavailable until a valid directory is restored; the old directory is not served indefinitely. Raw assets, responsive-image reads and preview fingerprints use the same boundary. Preview retains its last valid revision when a scan is cancelled and retries on the next refresh.

Parsed local `/assets/` image destinations are rebased to the configured public URL, including images nested in tables, quotations and footnotes. External URLs are preserved; other relative forms have no rebasing guarantee. Eligible static JPEG/PNG/WebP images receive orientation-corrected intrinsic dimensions and `/images/1280/<path>` fallback URLs. Candidates use widths `320`, `640`, `960`, `1280`, `1920`, `2560`, capped by source dimensions, followed by `auto` for the smaller of source width and 2560. Aspect ratio is preserved without crop or upscale; authored display bounds, alt text and captions remain. Query/fragment data and URL encoding are retained and do not select quality or variants.

`Accept` negotiates WebP or the conventional JPEG/PNG fallback; explicit ranges override wildcards, including `q=0` exclusions. Equal explicit preference favors WebP; absent or unrestricted Accept favors conventional delivery. Lossy output uses quality 80, while PNG-origin WebP and PNG output are lossless. An unresized original may be retained when re-encoding grows or exceeds limits. Every image is not guaranteed to shrink.

Unsupported formats, animation, unsupported color/depth metadata, missing files and inspection failures retain original URLs in HTML. Variant requests return 415 for unsupported input, 422 for corrupt or out-of-bounds input, 406 when no representation is acceptable, and 503 when admission is full or closing. At the accepted 16,384-pixel side boundary, WebP's 16,383 codec ceiling selects an acceptable conventional representation or returns 406 for WebP-only clients unless the original WebP is usable. Re-encoded output applies EXIF orientation, retains supported RGB ICC profiles and omits EXIF/XMP. Original delivery preserves original metadata and is not a privacy scrubber.

Variants have selected-byte ETags, HEAD/304, `Vary: Accept`, no-cache, nosniff and public CORS. No variants are written to disk. Image source changes invalidate HTML dependency hashes, including same-size/mtime edits; unstable sources fall back to originals without caching stale optimized HTML. Previously served HTML and subsequent live image requests are not an immutable content snapshot.

## Limits and recovery

| Resource | Bound |
| --- | --- |
| Candidate articles / distinct tags | 9,999 each |
| Article / front matter / retained sources | 1,000,000 bytes / 65,536 bytes / 512 MiB |
| Authored tag entries | 1,000 per article before normalization |
| YAML | depth 32, 16,384 nodes, 32,768 events, 64 aliases/anchors |
| Renderer | 20,000 blocks, nesting 256, aggregate encoded output 2,000,000 bytes |
| GraphQL | 256 KiB body/document, depth 32, 1,000 expanded fields, 100 aliases/variables, 8 MiB response, 30-second execution deadline |
| Render cache | 64 MiB process-local LRU; oversized single values are not retained |
| Image input/output | 32 MiB, 16,384 pixels per side, 64 million pixels |
| Image work / cache | one or two workers, at most 64 waiters, independent 64 MiB variant LRU |
| Asset path / read | 8,192 encoded bytes / 32 MiB, four readers |
| Public base URL | 4,096 bytes |

Limits bound work and owned buffers, without promising process RSS or response-time performance. Startup validates the full catalog and fails on invalid articles. A request sees one immutable catalog snapshot across all resolvers. Serialized reloads retain last-valid articles on invalid edits and publish deletions only after confirmed absence in a successful scan. A later valid edit recovers automatically. Caches use content/profile identities, single-flight work and cancellation-safe worker ownership. Restart discards all caches and rebuilds from source files.

Logs are structured JSON on stderr; use `RUST_LOG=shiori=debug` for diagnostics. Inspect `startup`, `reload`, `invalid_article`, render/image failure and fatal events to distinguish source errors from worker failures. SIGINT/SIGTERM stop admission and drain owned work; supervisors should allow the documented server/runtime shutdown bounds before forcing termination. Back up article/asset files and their permissions; there is no database or generated cache to recover. `serve --watch` additionally exposes the development notification stream; leave it disabled when unused.

## Build and verification

```sh
make setup
make setup-tools
cargo fetch --locked
cargo fetch --locked --manifest-path tools/contract/Cargo.toml
make build PROFILE=debug
make check
make build
make acceptance
```

Checks need rustfmt, Clippy, tokei 15.0.0, cargo-deny 0.19.8 and Python 3.9+. Browser acceptance additionally uses its declared Playwright tooling; native platform qualification belongs to the actual CI matrix. Run permission tests as an unprivileged user. The generated contract package version is owned by `chigiri.toml` and canonical regeneration, independently of the executable release version. `make contract-update` deliberately regenerates the owned contract with published Chigiri; `make contract-check` checks exact bytes and converts its provenance for common verification. Standards reruns isolated canonical reproduction and checks inventory/hashes before exempting generated Rust from implementation size limits. Handwritten implementation remains limited to 400 Code lines; tests and documents are excluded by role. Build, install, package and receipt handling use the pinned engine; product scripts retain only dependency asset evidence and product contract connections. `make release RELEASE_ARGS=--json` previews release preparation; `make release` applies it without creating commits, tags or a GitHub Release.
