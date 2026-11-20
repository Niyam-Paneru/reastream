# reastream

I asked a reverse-engineering tool for some evidence. It handed me a **1.26 GB analysis result**.

I needed the header and a capability index, not the whole file. My laptop has enough problems without loading it all into memory.

So I wrote a Rust streaming reader: decode the input, scan incrementally, and keep the selected records without loading the whole graph.

## One measured run

| Measurement | Recorded result |
| --- | ---: |
| Original REA analysis file | **1.26 GB** |
| Extracted records | **19,962** |
| Output records on disk | **6.55 MB** |
| Share of original on-disk file | **0.52%** |
| Share of decoded JSON | About **1.04%** |
| Processing time | **110 seconds** |

These figures come from **one REA 6.1.0 run**. The input and reproduction details live in [measurements](docs/measurements/README.md). The results describe that run, not a guarantee for every dataset.

## What the reader actually handles

The source file's enormous graph contains evidence I want to preserve. Dropping the graph entirely would make the output much smaller and much less useful — the classic “optimization” where you delete the requirement.

The reader handles **UTF-16LE from Windows redirection**, incremental scanning, and selected-field projection. Follow [decoding](src/decoding.rs), [lexer](src/lexer.rs), [projector](src/projector.rs), [dossier](src/dossier.rs), and [tests](tests/).

I deliberately **copy** REA's evidence IDs rather than reimplement its evidence hash. This tool doesn't independently authenticate those IDs. Two tools giving contradictory answers about the same evidence would defeat the whole exercise.

The CLI and library examples are in the [usage guide](docs/usage.md). MIT licensed. It's a **reader for analysis results**, not a reverse-engineering engine.
