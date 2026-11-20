# reastream

I asked a reverse-engineering tool for a small piece of evidence. It replied with **1,264,678,208 bytes**.

At that point I had two options: ask my laptop to swallow the entire file, or write a Rust streaming reader. Somehow the *second* option felt kinder.

The reader scans the result incrementally and keeps just the records I want. The file is still enormous; my memory usage doesn't have to be. This project was supposed to save me from doing extra work. It has a decent sense of irony.

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
