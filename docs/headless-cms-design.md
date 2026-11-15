# Filesystem CMS contract

shiori.rs owns filesystem article identity, bounded YAML metadata, request snapshots, reloads, typed GraphQL projection and HTTP image delivery. Chigiri owns the canonical generated resolver contract and HTTP execution; Oriha owns parsing, structured derivation, rendering, references and resource production. Their public HTTPS revisions are fixed in Cargo.lock and chigiri.toml.

Article files have no durable draft/publication lifecycle or business line-item aggregate. Generation uses filesystem reservation and atomic publication; transient watcher/cache states do not add a business state-history database or Header-Detail model. The [operations guide](operations.md) specifies identity, metadata, bounds, query behavior, image negotiation, failure isolation and recovery.

## Public GraphQL schema

The following SDL is identical to the source used by canonical contract generation.

```graphql
type Query {
  post(id: ID!): Post
  posts(first: Int = 20, after: ID, before: ID, tag: String, query: String): PostConnection!
  postsByMonth(year: Int!, month: Int!, first: Int = 20, after: ID, before: ID): PostConnection!
  tags: [Tag!]!
  monthlyArchives: [MonthlyArchive!]!
}

type Subscription {
  contentChanged: ContentChange!
}

type ContentChange {
  revision: ID!
}

type Post {
  id: ID!
  title: String!
  author: String
  description: String!
  tags: [String!]!
  createdAt: String!
  updatedAt: String!
  sourcePath: String!
  blocks: [HtmlBlock!]
  assets: [RenderAsset!]
  structured: StructuredDocument
}

type HtmlBlock {
  id: ID!
  kind: String!
  html: String!
}

type RenderAsset {
  id: ID!
  kind: String!
  mimeType: String!
  content: String!
}

type PostConnection {
  nodes: [Post!]!
  totalCount: Int!
  pageInfo: PageInfo!
}

type PageInfo {
  startCursor: ID
  hasPreviousPage: Boolean!
  endCursor: ID
  hasNextPage: Boolean!
}

type Tag {
  name: String!
  count: Int!
}

type MonthlyArchive {
  year: Int!
  month: Int!
  count: Int!
}

# CMS projection of renderer Structured schema v1. IDs are document-local.
type StructuredDocument {
  schemaVersion: Int!
  rootIds: [ID]!
  blocks: [StructuredBlock!]!
  inlines: [StructuredInline!]!
  references: [StructuredReference!]!
  footnotes: [StructuredFootnote!]!
  anchors: [String!]!
}
interface StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
}
interface StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}
enum TableAlignment { NONE LEFT CENTER RIGHT }
enum StructuredTrustLevel { UNTRUSTED SANITIZED TRUSTED }
type StructuredTableCell { text: String! richTextIds: [ID]! }
type StructuredTableRow { cells: [StructuredTableCell!]! }
type StructuredEntry { key: String! value: String! }
type StructuredReference { label: String! destination: String! title: String }
type StructuredFootnote { label: String! richTextIds: [ID]! }

type BlockDocument implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
}

type BlockParagraph implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  richTextIds: [ID]!
  tight: Boolean!
}

type BlockHeading implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  level: Int!
  richTextIds: [ID]!
}

type BlockFencedCode implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  language: String
  info: String!
  closed: Boolean!
  indent: Int!
}

type BlockIndentedCode implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  language: String
  info: String!
  closed: Boolean!
  indent: Int!
}

type BlockMathBlock implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  display: Boolean!
  closed: Boolean!
}

type BlockThematicBreak implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
}

type BlockHtmlBlock implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
}

type BlockLinkReferenceDefinition implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  source: String!
}

type BlockBlockQuote implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
}

type BlockList implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  ordered: Boolean!
  loose: Boolean!
  marker: String!
  start: String
}

type BlockListItem implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  marker: String!
  checked: Boolean
}

type BlockGfmTable implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  alignments: [TableAlignment!]!
  header: [StructuredTableCell!]!
  rows: [StructuredTableRow!]!
}

type BlockBlankLine implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
}

type BlockCallout implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  calloutKind: String!
  title: String
}

type BlockForeignHtml implements StructuredBlock {
  id: ID!
  text: String
  childIds: [ID]!
  origin: String!
  trustLevel: StructuredTrustLevel!
}

type InlineAnnotation implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
  annotationKind: String!
  annotationId: String!
  classes: [String!]!
  data: [StructuredEntry!]!
}

type InlineText implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineEmoji implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
  shortcode: String!
}

type InlineCode implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineMath implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineHtml implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
  tagfilter: Boolean!
}

type InlineSoftBreak implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineHardBreak implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineEmphasis implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineStrong implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineStrikethrough implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
}

type InlineLink implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
  url: String!
  normalizedUrl: String!
  title: String
}

type InlineImage implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
  url: String!
  normalizedUrl: String!
  title: String
}

type InlineFootnoteReference implements StructuredInline {
  id: ID!
  text: String
  childIds: [ID]!
  label: String!
}
```
