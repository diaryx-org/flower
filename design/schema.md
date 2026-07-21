# The schema layer

Status: design. Greenfield — no schema type exists yet in `flower-core`, `fig`,
or the FFI (grep confirms: only doc-comment TODOs today).

## What this is for

fig parses *bytes → `Value`* and edits losslessly. It has no notion of "what
keys/values are valid here" — the roadmap calls that layer "the big one, ours to
add." Today Flower fakes it in three places:

- `tree::parse_scalar` coerces an edit buffer by **literal shape** (`true`→Bool,
  digits→Int, else Str). Its own doc says *"a schema layer would instead pick the
  type the key expects and validate against it."*
- `VKind` classifies an *existing* value's shape, for styling.
- Swift's `FlowerPalette` picks an icon by **guessing from the key name** (`host`→
  globe, `secret`→lock). Its doc says *"a schema layer would override these per
  key."*

The schema layer replaces the guessing with knowledge: the type a field expects,
the values it allows, and how to present it (icon, colour, label, help).

## Where it lives, and how it arrives

Two hard boundaries the existing architecture already commits to, which this
design preserves:

1. **fig stays schema-free.** fig is format-level, released as a versioned crate,
   and shared by consumers (flower, prov, leaf) with *different* schema sources.
   Per-key semantics don't belong in the parser.
2. **flower-core stays config-generic; prov stays consumer-agnostic.** The
   app-specific bridge lives in `provui`, not here — `flower-core` never learns
   the word "prov."

So the mechanism (a generic `Schema` type, validation, type-directed parsing,
presentation) lives in **`flower-core`**. The prov-specific *source* (translating
prov's controlled vocabularies + relations into that generic `Schema`) lives in
**`provui`**, exactly mirroring how `ProvBackend` already adapts prov's
`MetaEditor` into `flower_core::Backend`.

Schema travels the **same seam as edits** — the `Backend` trait — via one new,
defaulted method:

```rust
pub trait Backend {
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError>;
    fn to_value(&self) -> Result<Value, BackendError>;
    fn source(&self) -> Result<String, BackendError>;
    fn schema(&self) -> Option<Schema> { None }   // new
}
```

- `FigBackend::schema()` → `None`. A standalone config file has no schema.
  (Later, optionally, a *generic* detector — `$schema` key or filename — could
  fill this in; that is a separate, prov-independent path.)
- `ProvBackend::schema()` (in provui) → `Some(schema)` built from the resolved
  workspace config.

The backend is exactly the component that knows *where the document came from*,
so it is the right place to know what governs it. This also matches the existing
`hidden_keys` injection precedent (`Model::with_hidden`).

## The generic `Schema` type

```rust
// crates/flower-core/src/schema.rs — generic, prov-agnostic

pub struct Schema { rules: Vec<FieldRule> }

pub struct FieldRule {
    pub at: PathPat,                    // which node(s) this governs
    pub ty: Option<FieldType>,          // expected type: type-directed parse + widget
    pub constraint: Option<Constraint>,
    pub present: Presentation,          // icon / colour / label / help
}

/// Addressing must reach list *elements*, not only scalars-at-path:
/// prov's `tags:` constrains each item of a sequence, not the sequence value.
pub struct PathPat(pub Vec<SegPat>);
pub enum SegPat { Key(String), AnyKey, Index(usize), EachItem }

pub enum FieldType { Null, Bool, Int, Float, Str, Ref, Map, Seq }

pub enum Constraint {
    /// A controlled vocabulary — an enumerated set of allowed values.
    Enum { values: Vec<Term>, closed: bool },
    /// A relation / link field. Spanning lives here.
    Reference { relation: String, cardinality: Cardinality, spanning: bool },
}

pub enum Cardinality { One, Many }

pub struct Term {
    pub value: String,
    pub label: Option<String>,
    pub description: Option<String>,    // human gloss
    pub retired: bool,                  // known, but not offered in the picker
    pub tint: Option<Tint>,             // per-value colour (e.g. public=green)
}
```

The two constraint kinds are the two required features:

### Vocabulary → `Constraint::Enum`

- `closed: true` → an unknown value is **rejected** at the `Model::commit` choke
  point (before `backend.apply`; fig's reparse remains the last-resort backstop).
- `closed: false` → **suggest and warn** — unknown values are allowed, a
  near-miss to a known term surfaces a soft warning in the status line.
- `retired` terms still render if already present, but are excluded from the
  picker's offered set.

### Spanning + relations → `Constraint::Reference`

- `spanning: true` flags the containment backbone (default `contents`). Flower
  can give it a node-picker widget and, later, navigation into children.
- Overlay relations and inverses (`part_of`, …) are the same kind with
  `spanning: false`.
- `cardinality` selects single-link vs list-of-links rendering.

`Reference` is a distinct kind from `Enum` on purpose: an enum value is a literal
drawn from a fixed set; a reference value is a *link into the workspace*, so its
picker is populated from documents, not from a static term list.

## Presentation — renderer-neutral

`flower-core` drives both a ratatui TUI and SwiftUI, so it must emit **semantic**
hints, never SF Symbols or RGB. Each frontend maps them (Swift → SF Symbols +
theme-adaptive `Color`; ratatui → unicode + ANSI):

```rust
pub struct Presentation {
    pub title: Option<String>,          // human field label
    pub description: Option<String>,    // help text / section subtitle
    pub icon: Option<Icon>,
    pub tint: Option<Tint>,
}
pub enum Icon { Link, Enum, Toggle, Lock, Globe, Clock, Tag, Text, Other(String) }
pub enum Tint { Accent, Neutral, Positive, Warning, Danger }
```

Resolution is two-tier, so nothing regresses when a document has no schema:

1. **Schema-driven** (ProvBackend): explicit `icon`/`tint` if the prov config or
   term carries one; otherwise a default derived from the constraint
   (`Reference`→`Link`, `Enum`→`Tag`, bool→`Toggle`).
2. **Heuristic fallback** (FigBackend, or any row no rule matches): today's
   `FlowerPalette` name-guessing table, unchanged — demoted to the fallback layer.

**Schema overrides heuristics; heuristics fill the gap.**

## How prov feeds the schema (the provui adapter)

`ProvBackend::schema()` builds a `Schema` from the resolved `WorkspaceConfig`
(`prov/src/config.rs`, `prov/src/vocabulary.rs`, `prov/src/relation.rs`):

| prov source | → flower `FieldRule` |
|---|---|
| `fields[k]`, closed vocab | `at: [Key(k)]` (or `[Key(k), EachItem]` if the field is a list), `Constraint::Enum { closed: true, values }` |
| `fields[k]`, open vocab | same, `closed: false` |
| `Vocabulary.terms` | `Vec<Term>` — `means`→`description`, `retired`→`retired`; term `id` retained for rename-safety |
| `RelationSet` spanning field (`contents`) | `Constraint::Reference { relation, cardinality, spanning: true }` |
| overlay relations + inverses | `Constraint::Reference { …, spanning: false }`, `means`→`description` |
| conventional `icon:` / `color:` payload on a field or term | `Presentation.icon` / `.tint` (prov carries arbitrary opaque payload; the adapter reads convention keys) |

Prov already *detects* all of this by resolving the workspace config, so Flower
never re-implements detection — it consumes the resolved `Schema`. This is why
"schema detection" is not a flower-core concern for prov documents: prov owns
detection, flower owns application.

## Wiring into the model

Three integration points, all existing choke points:

1. **`tree::parse_scalar` → schema-aware.** Given the selected path's `FieldRule`,
   coerce the edit buffer to the expected `FieldType` instead of guessing by
   literal shape. Falls back to today's heuristic when no rule matches.
2. **`Model::commit`.** The single funnel every mutation passes through. Validate
   here: hard-reject for `closed` enums, soft-warn for `open`. fig's reparse stays
   the backstop for anything the schema doesn't cover.
3. **`Row` / `RowView`.** Carry schema-derived fields so frontends can render the
   right widget and presentation without re-deriving anything.

## FFI / Swift surface

`RowView` (`crates/flower-ffi/src/lib.rs`) extends its existing renderer-class-id
pattern (the `kind: String` field) with optional schema fields:

```rust
pub struct RowView {
    // …existing…
    pub expected_kind: Option<String>,   // schema type, if known
    pub enum_options: Vec<String>,       // offered vocabulary values (empty = none)
    pub icon: Option<String>,            // semantic icon name
    pub tint: Option<String>,            // semantic tint name
    pub description: Option<String>,     // help text
    pub ref_relation: Option<String>,    // set when this is a Reference field
}
```

`FlowerSettingsView` then renders:

- `enum_options` present → a `Picker` instead of a free-text field.
- `ref_relation` present → a node/link picker (spanning fields get this).
- `icon` / `tint` present → schema-driven, replacing the `FlowerPalette` guess.
- `description` present → inline help / section subtitle.

`FlowerPalette` stays as the fallback for rows with no schema info, so the
FigBackend experience is unchanged.

## Build order

1. `crates/flower-core/src/schema.rs` — the types above + `Backend::schema()`
   defaulted to `None`. Compiles standalone, changes no behaviour.
2. Wire the three model integration points behind "if a rule matches"; absent a
   schema, behaviour is byte-for-byte what it is today.
3. Extend `RowView` + `FlowerSettingsView` for enum pickers and schema-driven
   presentation.
4. `provui`: `ProvBackend::schema()` — the vocabulary + relation adapter. This is
   where prov's controlled vocabularies and spanning finally reach the UI.
5. (Later, independent) a generic detector in `FigBackend::schema()` for
   standalone files via `$schema` / filename.

## Open questions

- **Reference pickers need workspace context.** A `Reference` field's picker is
  populated from *other documents*, which `flower-core` (single-document, no fs)
  cannot enumerate. Likely the schema carries the relation identity and the
  candidate list is supplied by the embedder (provui) — another injected input,
  like `hidden_keys`.
- **Per-value tint** (`public`=green) implies the enum picker colours each option;
  confirm the Swift theme can express semantic tints per row *and* per option.
- **`reify: true` vocabularies** model each term as a real node. Does the field
  render as an `Enum` (pick a term) or a `Reference` (link to the term's node)?
  Probably `Reference`, since reified terms have identity and backlinks.
