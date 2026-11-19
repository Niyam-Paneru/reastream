# reastream

I had a **1.26 GB UTF-16LE analysis result** and only needed its evidence envelope and a useful capability index. Loading the whole file (or filtering away the evidence) was a bad trade.

So I built `reastream`: a Rust streaming reader that projects the required records without materializing the graph. On the [documented REA 6.1.0 run](docs/measurements/README.md), it read **1,264,678,208 bytes**, kept **6,547,610 bytes** across **19,962 records**, and completed in **110 seconds** in release mode. The 632 MB figure refers to *decoded JSON*, not the file on disk.

The graph was large enough to need a chaperone; a streaming parser volunteered.

**Try it / check the behavior:**

```bash
cargo test
cargo build --release
./target/release/reastream result.json --count
```

Those numbers describe one measured environment and input, not a universal performance guarantee.

## Why

Every consumer of REA inherits the full cost of the result it is handed:

- **Memory.** The 1.26 GB on-disk document (632 MB after decoding) cannot be buffered, and the tooling goes out of
  its way to avoid it: the companion `reaforensics` project ships
  `structure.mjs`, a brace-depth scanner rather than a JSON parser, precisely
  because Node cannot hold the document.
- **Latency.** The analysis itself took 5 522 s. Reading its result back is not
  free.
- **A trap in the current workaround.** `--filter-output` keeps ingestion
  bounded but drops the evidence envelope (`evidence_id`, `subject`,
  `provider`), because REA filters inside the result payload. A caller either
  keeps the entire document, or loses the evidence chain. That is not a trade-off, it is
  a design limit.

The envelope is at the *front* of the document, which means a streaming reader
can take it without asking REA for anything different. That is what this does.

## What it does

| | |
| --- | --- |
| Envelope | `evidence_id`, `subject`, `provider`, `operation`, `predicate_type` |
| Capability index | one record per graph node: `node_id`, `kind`, `identity.namespace`, `identity.key` |
| Coverage | per observation: `label`, `semantic_role`, `resolution_status`, `state`, `confidence` |
| Honesty | what was read, what was kept, whether the document closed |

Memory is bounded by the largest single value, not by the document. A node is
projected and released, so its siblings are never held.

## Use

```sh
cargo build --release
./target/release/reastream result.json --count
./target/release/reastream result.json > index.txt
```

`--count` reports what was read and what was kept. Default output is one record
per line; the summary goes to stderr so the records can be piped.

As a library:

```rust
use reastream::{Dossier, application_rules};

let rules = application_rules();
let dossier = Dossier::with_rules(std::fs::File::open("result.json")?, &rules)?;
for record in &dossier.records {
    // record.fields is [(name, value)]
}
```

## What it handles

A REA document captured on Windows arrives as **UTF-16LE**, because that is
what shell redirection writes. Reading it as UTF-8 turns every character into a
NUL and hides the whole document. `reastream` detects the BOM, transcodes, and
reports which encoding it read — see `tests/decoding.rs`.

## What it does not do

It does not verify evidence. REA is the authority for its own `evidence_id`,
which is a content digest over REA's canonical form; this crate copies the
identifier and never recomputes it. Reimplementing that hashing would make this
crate able to disagree with REA about whether a record is authentic, and that
is the one bug it must not have.

It does not execute targets, and it does not claim a capability it has not read.

## Building

Rust is the right tool for one reason: the work is a tight scan over bytes where
`--filter-output` is not good enough, and a conventional JSON parser cannot do
it at all.

No Visual Studio is required. The default target is `x86_64-pc-windows-gnu`
with the portable `llvm-mingw` toolchain, because the Microsoft toolchain's
import libraries only ship with Build Tools.

```sh
cargo test      # 42 tests
cargo build --release
```

## Layout

```
src/decoding.rs   BOM detection and UTF-16LE → UTF-8
src/lexer.rs      incremental JSON lexer; a string in key position is a key
src/path.rs       paths, rules, and the dotted syntax they are written in
src/projector.rs  walks tokens, emits records, drops everything else
src/dossier.rs    the whole job, plus what it kept and dropped
```

## License

MIT
