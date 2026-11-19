# Using reastream

The profile README is a tour, not a command-line tutorial. This page has the commands.

## Build and run

Requires a Rust toolchain. From the repository root:

```sh
cargo test
cargo build --release
./target/release/reastream result.json --count
./target/release/reastream result.json > index.txt
```

The source argument is optional; the CLI can read standard input. `--count` prints the summary. Without it, projected records go to stdout and the summary goes to stderr for clean piping.

On Windows, the recorded toolchain uses `x86_64-pc-windows-gnu` with portable `llvm-mingw`, not Visual Studio Build Tools. You will need the compatible Rust target/toolchain installed. See `Cargo.toml` and the measurements for the setup used in the original experiment.

## Use the library

```rust
use reastream::{Dossier, application_rules};

let rules = application_rules();
let dossier = Dossier::with_rules(std::fs::File::open("result.json")?, &rules)?;
for record in &dossier.records {
    // record.fields contains pairs of (name, value)
}
```

This is illustrative library usage, not a complete standalone Rust program (the `?` operator requires a suitable function return type).

## What this does not do

The reader does not execute REA analysis targets, re-compute REA's evidence digest, or validate a proof beyond extracting it. Read [the measurements](measurements/README.md) for tested scale and the exact input environment.
