# ADR 0002: One flat typed document API

Status: Accepted — Implemented.

Date: 2026-11-17.

## Context

The previous article API exposed rendered HTML blocks, rendering assets and a separate structured representation. Those paths derived the same Markdown but offered different information and required consumers to understand both markup conventions and structured types. A single typed document provides the article semantics and the difficult rendering results without using HTML as the article transport.

GraphQL selections have a fixed written depth. Keeping Markdown children as nested GraphQL objects would require clients to anticipate article depth. A flat collection preserves arbitrary supported nesting through ID references while typed union members keep each node's data together. The existing structured path is already used for RSS and must migrate with the visible article path.

## Decision

### Replace the article-body fields together

Add nullable `Post.document: Document` as the only article-body field. Remove `Post.blocks`, `Post.assets`, `Post.structured` and their now-unused supporting types in the same breaking change. Do not retain aliases, deprecated fields or a compatibility period. Preserve article identity, metadata, source paths, timestamps, pagination, search ordering and subscriptions as specified by [ADR 0001](0001-article-storage-and-timestamps.md).

The following is an illustrative excerpt. The complete [current contract](../headless-cms-design.md), generated from the canonical SDL, includes every supported node kind.

```graphql
# Outline only: the union must cover every supported node kind.
type Document {
  schemaVersion: Int!
  rootIds: [ID!]!
  nodes: [DocumentNode!]!
  referenceIds: [ID!]!
  footnoteIds: [ID!]!
  fonts: [DocumentFont!]!
  diagnostics: [DocumentDiagnostic!]!
}

union DocumentNode = ParagraphNode | TextNode | HeadingNode | FencedCodeNode | MathBlockNode | ImageNode

type ParagraphNode {
  id: ID!
  childIds: [ID!]!
  tight: Boolean!
}

type TextNode {
  id: ID!
  text: String!
}

type MathBlockNode {
  id: ID!
  source: String!
  display: Boolean!
  closed: Boolean!
  svg: SvgContent
}
```

Use structured schema version 2. Project the full flat Oriha contract: blocks, inline content, table rows/cells and retained definitions share one typed node pool. Each union member has its own required payload, and ordered references carry hierarchy. Use explicit typed ID roles where content has different roles, such as image alt content and retained definition references. Do not expose recursive `children` objects, a generic JSON scalar, a standard-node property bag, per-kind detail arrays or a record containing every possible nullable node attribute.

GraphQL `__typename` identifies the concrete union member and generated Rust/TypeScript types carry its payload. The same finite selection works for shallow and deeply nested articles; the entire GraphQL query need not literally have depth one. IDs are opaque and document-local, remain independent of preparation options and are not DOM IDs or persistent editing identities. Validate ordered structural ownership separately from semantic links to definitions.

### Keep producer and application responsibilities distinct

Oriha owns Markdown parsing, normalized semantics, reference resolution, collision-free semantic anchors, flat structured conversion, syntax tokenization and math/Mermaid/PlantUML SVG generation. Consume its version-2 structured output directly. It contains optional prepared data on the originating nodes; do not render HTML and parse it back, recreate the producer's Markdown conversion or request separate responses that the caller must join.

shiori owns GraphQL projection, snapshot selection, bounded execution/caching and filesystem image information. Image nodes retain their original and normalized destinations, alt content and optional title. Add typed presentation information for the public URL, responsive candidates, sizes and intrinsic dimensions using the existing image service and invalidation rules. Gallery geometry, cropping, captions and slideshow controls remain display policy. Preserve `/assets` and `/images`, including non-image assets, proxy-compatible URLs, watch revisions, conditional delivery and directory-replacement behavior.

Provider embeds, viewer icons, application CSS, code colors and search-match decorations are consumer policy. Do not add YouTube, Spotify, Gist, gallery or search-highlight nodes to the producer contract or inject their HTML into the document API. This decision removes generated article HTML fields; it does not prohibit the GraphiQL UI, normal HTTP responses, SVG markup or independently constrained embedded documents.

### Prepare only selected data

Use the structured preparation options to generate syntax tokens and math/diagram SVG only when their fields are selected. Drive this through selected field resolvers or the execution contract's effective selection, including fragments, aliases and include/skip directives, rather than string matching on query text. Coalesce required work across a request without assuming resolver execution order. A body-display query selects the prepared fields it needs; an RSS text query selects structure and source only. A metadata-only query must not parse, inspect images, tokenize or render a body.

Retain one request-shared parse per source revision and bounded reusable derivations keyed by the actual source/configuration and preparation profile. Profile-specific cache entries must not confuse a structure-only result with prepared output. Keep existing worker admission, memory/output accounting, cancellation and image-dependency invalidation; this change does not justify an unbounded artifact cache or request-per-node worker scheduling. Selecting structure without image presentation must not trigger filesystem image inspection or transformations.

Fenced and indented code expose their source and optional typed highlighting data. Highlighting contains literal token text and a finite token-kind enum, with enough line information to preserve whitespace, empty lines, terminal-newline behavior and code/gutter alignment. No HTML spans or hard-coded CSS colors are returned. Unsupported languages use plain tokens.

Math nodes and supported math/diagram fences expose optional `SvgContent` directly on the originating node, preserving the source and authoring metadata. SVG data includes generated markup, intrinsic sizing/baseline information where needed and font IDs. Shared fonts appear once in `Document.fonts` with typed identity, family, MIME, encoding and content. Preserve existing font limits and notices; the math SVG is not assumed to contain glyph outlines. Standard documents do not gain a general CSS or HTML resource payload.

### Preserve truthful failure boundaries

A requested math or diagram that cannot render retains its source, has no successful SVG and contributes a typed node-associated diagnostic. The consumer can show that source as code while displaying the rest of the article. Omitted preparation does not itself create a failure diagnostic. Do not silently substitute a successful empty SVG or claim the document failed because one diagram is unsupported.

Parse failure, invalid structure, unknown/unprojectable required nodes, exhausted aggregate budgets and derivation-wide failures produce a null `document` with a GraphQL error at the body boundary. Preserve the article metadata and isolate the failure from other requested articles. No truncated graph is returned as complete. Cancellation remains cancellation rather than source fallback. Bound diagnostics and avoid exposing absolute source paths, internal endpoints or sensitive exception details.

Authored and foreign HTML remain typed source data, with the existing execution-disabled policy. They are displayed escaped rather than injected. Generated SVG remains a separate validated trust boundary with reference/font handling; being a string inside a typed node does not make arbitrary executable content acceptable.

## Migration and consequences

The backend and its consumers migrate as a compatible set. Generate the union contract through Chigiri, update the canonical SDL documentation, and regenerate SDK operations using truthful schema provenance. Never hand-edit generated output or report an old released binary as verification of the new API.

Implementation exposed one executor capability gap: the original field context did not expose effective child selections before returning the nullable body. Chigiri now provides read-only effective selection inspection through the existing executor collector and forwards the actual field context through generated resolver adapters. This enables complete bounded preparation without duplicating GraphQL parsing, assuming resolver order or changing concrete union generation.

Producer output, GraphQL projection, consumer rendering and RSS now use the same version-2 contract. Deployments must use matching producer, GraphQL API and generated client revisions; mixed old/new article-body APIs are unsupported.

Consumers construct their own presentation from one document and retain typed access to tokens and SVG. The backend no longer promises ready-to-insert article HTML. The extra ownership is explicit, and the protocol remains bounded independently of Markdown nesting depth.

## Acceptance and status

Verification exercises actual generated GraphQL union execution, concrete typenames, complete node-kind projection, fixed-depth queries for deeply nested block/inline content, table/definition references, semantic anchor collisions and malformed graph rejection. Metadata-only requests avoid body work, structure-only requests avoid unused rendering and image inspection, and mixed requests share parsing while preserving cancellation and invalidation.

Use real producer output for highlighting, SVG/font and source-fallback cases. Preserve RSS date-snapshot failure behavior, UTC month boundaries, hierarchical UTF-8 keys, arbitrary asset delivery and ordinary-startup subscription isolation. Run the existing repository checks and canonical generation ownership/hash verification; measure generated and handwritten Rust separately under the existing code-size policy.

The implementation has passed Rust unit and integration checks, canonical contract ownership/hash verification, executable HTTP acceptance and native image browser checks against actual producer output. The complete required local checks pass with immutable Git dependency revisions. Release builds must additionally pass the declared native artifact matrix and published installer verification; local source checks alone do not establish that qualification.
