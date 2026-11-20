# reastream

I asked a reverse-engineering tool for a small piece of evidence. It replied with **1,264,678,208 bytes**. I only wanted the useful records buried inside its graph. Reading the whole thing into memory felt like buying a house because I liked one doorknob.

So I wrote a Rust reader that decodes and streams the result, keeping only the records I asked for. The input remains enormous. My laptop doesn't have to experience all of it at once.

## The diet: 1.26 GB in, 6.55 MB kept

One measured run with REA 6.1.0:

| What | Recorded result |
| --- | ---: |
| Input file | **1,264,678,208 bytes** (about 1.26 GB) |
| Records extracted | **19,962** |
| Kept data | **6,547,610 bytes** (about 6.55 MB) |
| Output vs. input on disk | **0.52%** |
| Output vs. decoded JSON | **1.04%** |
| Elapsed time | **110 seconds** |

The numbers are from [one recorded workload](docs/measurements/README.md), not a guarantee that other datasets will behave so nicely. The input was UTF-16LE because Windows shell redirection wrote it that way; treating it as ordinary UTF-8 would have been a short and confusing debugging session.

## Why not just delete the big part?

REA's enormous `graph` contains the evidence I actually needed. Filtering out the whole graph made the output smaller, sure. It also made the result rather useless. A spectacular speedup, if your goal is to lose the answer.

The reader decodes Windows UTF-16LE output, scans incrementally and projects selected fields. The interesting bits are [decoding](src/decoding.rs), [lexing](src/lexer.rs), [projection](src/projector.rs), [dossier construction](src/dossier.rs), and [tests](tests/).

One deliberate limit: the reader **copies REA's evidence IDs**, but doesn't independently recompute or validate REA's evidence hash. It's extracting evidence, not certifying it.

[How to run it](docs/usage.md) · [Measured failures and setup](docs/measurements/README.md)

MIT licensed. This is a reader for analysis results, **not** the reverse-engineering engine. My laptop has enough jobs already.
