# reastream

A reverse-engineering tool answered my question with a **1.26 GB file**. I needed the header and a capability index. The rest of the file had apparently invited itself along.

Loading the whole thing into memory was an option. Writing a Rust streaming reader turned out to be the less ridiculous one, so that's what I did.

It reads the result incrementally, keeps the fields I ask for, and leaves the giant graph out of RAM. Not bad for something that began as “I only need a small part of this.”

## The receipt

One measured run with REA 6.1.0:

| What | Recorded result |
| --- | ---: |
| Input file | **1,264,678,208 bytes** (about 1.26 GB) |
| Records extracted | **19,962** |
| Kept data | **6,547,610 bytes** (about 6.55 MB) |
| Output vs. input on disk | **0.52%** |
| Output vs. decoded JSON | **1.04%** |
| Elapsed time | **110 seconds** |

The numbers are from [this recorded workload](docs/measurements/README.md), not a promise that every file will be this polite. The input was UTF-16LE because Windows shell redirection wrote it that way; treating it as ordinary UTF-8 would have been a short and confusing debugging session.

## Why not just delete the big part?

REA's enormous `graph` contains the evidence I actually needed. Filtering out the whole graph made the output smaller, sure. It also made the result rather useless. Excellent compression, terrible research.

The reader decodes Windows UTF-16LE output, scans incrementally and projects selected fields. The interesting bits are [decoding](src/decoding.rs), [lexing](src/lexer.rs), [projection](src/projector.rs), [dossier construction](src/dossier.rs), and [tests](tests/).

One deliberate limit: the reader **copies REA's evidence IDs**, but doesn't independently recompute or validate REA's evidence hash. It's extracting evidence, not certifying it.

[How to run it](docs/usage.md) · [Measured failures and setup](docs/measurements/README.md)

MIT licensed. This is a reader for analysis results, **not** the reverse-engineering engine. My laptop has enough jobs already.
