# reastream

I had a **1.26 GB analysis result** and wanted an evidence header plus a list of capabilities. The obvious solution was to load the JSON and pick out those fields.

Unfortunately, the JSON was approximately the size of my patience.

So I wrote `reastream` in Rust. It reads a REA result as a stream, keeps the evidence envelope, projects the bits of the semantic graph that matter, and discards the rest as it goes.

### The numbers (from one measured run)

| | Measured |
| --- | ---: |
| File read | 1,264,678,208 bytes (UTF-16LE on disk) |
| Decoded JSON | ~632 MB |
| Records emitted | 19,962 |
| Output kept | 6,547,610 bytes (~1.04% of decoded JSON) |
| Runtime | 110 seconds, release build |

Those aren't marketing benchmarks. They come from a REA 6.1.0 run, with environment details and reproduction steps in [the measurements](docs/measurements/README.md). Results will differ with input and machine.

## Use it

```sh
cargo test
cargo build --release
./target/release/reastream result.json --count
./target/release/reastream result.json > index.txt
```

`--count` tells you what it read and kept. Otherwise records go to stdout, summary to stderr, so you can pipe the output without having to ask it nicely.

As a Rust library:

```rust
use reastream::{Dossier, application_rules};

let rules = application_rules();
let dossier = Dossier::with_rules(std::fs::File::open("result.json")?, &rules)?;
for record in &dossier.records {
    // record.fields is a collection of (name, value) pairs
}
```

## Why a custom reader?

The document's `graph` occupies virtually the entire normalized result. REA's own `--filter-output` can discard that graph, but it also loses the evidence-bearing fields this consumer needs. A normal JSON parser would materialize far more than the desired projection.

The reader recognizes UTF-16LE BOMs (a common Windows redirection surprise), transcodes, and projects records as the stream advances. Memory use is bounded by the largest value being processed, rather than the whole document.

Code map: [decoder](src/decoding.rs) · [lexer](src/lexer.rs) · [projector](src/projector.rs) · [dossier](src/dossier.rs) · [tests](tests/).

### The part I won't claim

`reastream` **copies** REA's `evidence_id`; it does not independently verify the hash. REA owns that canonical format. Reimplementing it here would risk a second, incompatible definition of "authentic."

This tool doesn't run an analysis target, either. It reads the result of one.

MIT licensed. See [Cargo.toml](Cargo.toml) and the [measured failure cases](docs/measurements/README.md).
