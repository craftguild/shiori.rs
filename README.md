<h1 align="center">shiori.rs</h1>

<p align="center">
  <a href="https://github.com/Essk/ai-contribution-level">
    <img src="https://raw.githubusercontent.com/Essk/ai-contribution-level/main/badges/level-3.svg" alt="AI Contribution">
  </a>
</p>

shiori.rs is a filesystem Markdown CMS from CraftGuild. Authors edit local files; applications query a read-only GraphQL API for article metadata, rendered HTML, typed documents, tags, search and UTC monthly archives. Local JPEG, PNG and WebP images receive responsive variants without an external image service.

> [!WARNING]
> This project has not reached version 1.0 and may be unstable. Its behavior and specifications may change, and upgrades may require manual migration. Our team uses it in internal production systems to refine its design and implementation, but it may not meet your requirements. Please keep these limitations in mind when using it.

## Quick start

Install the latest release for macOS Apple Silicon or Linux x86_64 GNU:

```sh
curl -fsSL https://raw.githubusercontent.com/craftguild/shiori.rs/main/scripts/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
mkdir -p my-blog
shiori --root my-blog generate --title 'Hello world'
shiori --root my-blog serve --graphiql
```

The installer defaults to `~/.local/bin` and leaves shell configuration unchanged. Edit the article file printed by `generate`, then open [GraphiQL](http://127.0.0.1:4000/graphiql) and run `{ posts { nodes { id title } } }`. The server reloads saved articles automatically. Place public images under the content root's `assets/` directory and reference them as `/assets/<path>`.

## System requirements

- macOS on arm64 or Linux on x86_64 with a GNU userspace; release CI qualifies these native targets.
- An existing readable document root. Missing article and asset directories are created with the execution user’s permissions; existing directories need read access for serving.
- A Unix filesystem with directory handles and atomic hard links for generation.
- POSIX sh, curl and a SHA-256 tool for the download installer.

## Build requirements

Use the pinned Rust/Cargo 1.95.0 toolchain, a native C compiler/linker and Python 3.9 or later. On macOS, install Xcode Command Line Tools. Dependency revisions and registry checksums are recorded in `Cargo.lock`.

```sh
git clone https://github.com/craftguild/shiori.rs.git
cd shiori.rs
make setup
cargo fetch --locked
make build
make install BINDIR="$HOME/.local/bin"
```

`make build` uses the pinned Standards engine and produces `target/<host-triple>/release/shiori`, with its embedded SBOM and local verification receipt. `make install` copies that verified output without rebuilding. See [verification and contract generation](docs/operations.md#build-and-verification) for additional check tools and focused commands.

## Usage

Article storage defaults to `./articles` and asset storage to `./assets`, relative to `--root`. Override them with `SHIORI_ARTICLES_DIR` and `SHIORI_ASSETS_DIR`. Dates come from Git history in a worktree and filesystem metadata otherwise; see the [storage and timestamp decision](docs/adr/0001-article-storage-and-timestamps.md).

```sh
shiori --root /srv/blog generate --title 'A new article'
shiori --root /srv/blog serve \
  --bind 127.0.0.1:4000 --public-base-url https://cms.example/blog/
shiori --version
shiori --machine-version
shiori sbom
```

`--root` defaults to the current directory. `serve` defaults to `127.0.0.1:4000` with public base `http://127.0.0.1:4000`. GraphiQL and introspection are enabled only with `--graphiql`; `--watch` additionally enables development change notifications.

```graphql
query Articles($after: ID) {
  posts(first: 5, after: $after) {
    nodes { id title author createdAt updatedAt sourcePath tags }
    totalCount
    pageInfo { startCursor endCursor hasPreviousPage hasNextPage }
  }
}
```

Select `post(id: ID!) { blocks { id kind html } assets { id kind mimeType content } }` for rendered bodies and resources, or `structured` for the typed document projection. The [public schema](graphql/schema.graphql), [contract](docs/headless-cms-design.md) and [operations guide](docs/operations.md) describe metadata, pagination, image negotiation, resource handling and recovery.

## Known limitations

- Every valid indexed file is public, including future-dated articles; there are no drafts, authentication, mutations or scheduled publication.
- Article keys are relative `.md` or `.markdown` paths without the extension, including nested and natural-language filenames. Renaming a key changes its route without redirects.
- Search uses literal Unicode-lowercase substrings across titles, Markdown and tags; it has no relevance ranking.
- Serving uses HTTP; TLS and public-prefix forwarding belong to the reverse proxy.
- Image processing supports bounded static JPEG, PNG and WebP; unsupported or uninspectable images retain original URLs.
- Caches are process-local and disappear on restart; article and asset updates are live rather than immutable publication snapshots.

## Community

Report bugs and ask usage questions in [GitHub Issues](https://github.com/craftguild/shiori.rs/issues). Contributions should preserve the public CLI/schema contracts and run the relevant checks before review.

## Security

Use [private vulnerability reporting](https://github.com/craftguild/shiori.rs/security/advisories/new) for vulnerabilities; see [SECURITY.md](SECURITY.md).

## License

First-party source is available under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Embedded dependencies, fonts and codec data retain their [third-party terms](docs/third-party-notices.md).
