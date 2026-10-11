# protodiff

Diffs two `.proto` files for wire-breaking changes — a removed field,
a changed field number or type, a removed RPC — the same job
`buf breaking` (Go) does, without needing the `buf` CLI, `protoc`, or
a BSR account. Points a plain text diff of two `.proto` files at the
one question that actually matters before merging: *does this break
an existing client compiled against the old schema?*

## Installation

```sh
cargo install --path .
```

## Usage

```bash
protodiff old.proto new.proto
```

```
$ protodiff old.proto new.proto
BREAKING: Order: field 2 'customer_email' removed (or its number reused)
BREAKING: message 'CancelOrderRequest' removed
BREAKING: OrderStatus: enum value 2 'CANCELLED' removed (or its number reused)
BREAKING: OrderService: method 'CancelOrder' removed
```

Exit code `1` if anything BREAKING was found, `0` otherwise — a clean
run (including one with only WARNING-level findings, like a field
rename) exits `0`.

## What counts as breaking

Fields and enum values are matched between the two files **by
number**, since that's the real wire-format identity in protobuf —
not by name. A field whose number disappears is breaking, whether its
name moved to a new number, was deleted outright, or was reused for a
different field. Detected as `BREAKING`:

- A message, enum, or service removed entirely.
- A field or enum value's number removed (or reused for something
  else).
- A field's type changed for the same number.
- A field's cardinality changed (`repeated` <-> singular) for the same
  number — different wire encoding even with the same type.
- An RPC method removed from a service.
- An RPC method's input type, output type, or streaming shape changed.

Detected as `WARNING` (wire-compatible, but risky for anything keyed
on names — proto3 canonical JSON, reflection tooling, human callers):

- A field or enum value renamed while keeping the same number and
  type.

**Not flagged at all** (this is the point — additions aren't
breaking): a new field, a new enum value, a new message, a new
service, a new RPC method.

## Status: built and verified, including a real parsing bug caught by the tool's own test suite

- **20 unit tests**: field/enum/service/RPC parsing (streaming flags,
  bracketed field options, comments — both `//` and `/* */` — not
  confusing the parser, a string literal containing `"//"` not being
  mistaken for a comment start, nested messages correctly *not* being
  pulled into their parent's field list), and every diff rule's
  true-positive and true-negative case (removed field, type change,
  cardinality change, a rename producing only a WARNING not a
  BREAKING, a pure addition producing no finding at all, removed enum
  value, removed RPC, changed RPC input type, RPC gaining a streaming
  shape).
- **A real bug this suite caught itself, before any live run**: the
  field- and enum-value-matching regexes required each match to start
  at the beginning of a line (`^` in multiline mode). Real `.proto`
  files are almost always one field per line, so this looked fine at
  first — until several of this crate's own diff tests, written with
  compact single-line `.proto` text for brevity
  (`"message A { string x = 1; string y = 2; }"`), started silently
  losing every field after the first one on the line. Fixed by
  dropping the anchor — the existing brace-depth filter plus requiring
  the full `type name = number;` shape is specific enough without it
  — and locked down with a dedicated regression test
  (`parses_multiple_fields_packed_onto_a_single_line`).
- **Live-verified against a realistic before/after schema change**:
  a 4-message, 1-enum, 1-service `orders.proto` was evolved with 4
  real breaking changes (a field removed, a message removed, an enum
  value removed, an RPC method removed) alongside 4 real *additions*
  in the same diff (a new field, a new enum value, a new RPC, a new
  message) — all 4 breaking changes were correctly reported and all 4
  additions were correctly silent. Separately verified: an identical
  file against itself reports "no breaking changes found" with exit
  `0`, and a pure field rename reports exactly one `WARNING` with exit
  `0` (not `1`) — a rename alone must not fail a CI gate the way an
  actual removal should.

**Deliberate scope limits, stated plainly**: this is a lightweight
regex/depth-tracking parser, not a full protobuf grammar — nested
messages, nested enums, and `oneof` groups are recognized structurally
(so they don't corrupt their parent's field list) but their own
contents are not parsed or diffed in this v1. `import`-ed types
referenced across files aren't resolved or followed — a field's type
is compared as the raw type name text, so a genuinely incompatible
cross-file type change might not be caught if the type name itself
didn't change. `reserved` field number/name declarations aren't
parsed or cross-checked against reused numbers. No `protoc` or `buf`
dependency at all — this is intentionally lower-fidelity than a real
compiler in exchange for zero external tooling requirements.
