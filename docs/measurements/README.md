# Measured bottlenecks

Every number here was produced on this machine, not estimated. The commands
below reproduce them.

Environment: Windows, Node 22.19.0, REA 6.1.0, Ghidra 12.1.4, JDK 21.0.12.

## 1. JavaScript application analysis

```sh
node rea.mjs analyze-javascript-application "C:/Users/thely/OneDrive/Documents/Default Project/rea/src" --json
```

| Measurement | Value |
| --- | --- |
| Exit status | 0 (completed successfully) |
| Wall clock | 5 522.6 s (1 h 32 m) |
| Result size | 1 264 678 208 bytes = **1 206.1 MB** |
| JSON characters | 632 339 104 |

The document is UTF-16LE, because Windows shell redirection wrote it that way —
so the file on disk is **double** the length of its JSON. A reader that assumes
UTF-8 sees NULs where every character should be and finds no keys at all.

## 2. Where the document's length goes

Measured with `scripts/normalized-offsets.mjs` from the companion `reaforensics`
project, walking brace depth over all 1 264 678 208 bytes:

| `normalized_result` child | Start | Span | Share |
| --- | --- | --- | --- |
| `@normalized_start` | 764 | 632 338 340 | 100.00% |
| `input_path` | 792 | 28 | 0.00% |
| `format` | 879 | 28 | 0.00% |
| `root_artifact_sha256` | 907 | 97 | 0.00% |
| `inventory_manifest_id` | 1 004 | 102 | 0.00% |
| `inventory_graph_sha256` | 1 106 | 99 | 0.00% |
| **`graph`** | **1 205** | **632 337 899** | **100.00%** |

`graph` accounts for the whole document. The envelope, the digests and the
metadata together occupy the first **440 characters**.

The capability information is not lost with the graph — it is *inside* it. A
node carries `node_id`, `kind`, an `identity` block, and an `observations` array
whose entries hold `label`, `properties.semantic_role`, `evidence.state` and
`evidence.confidence`. That is what a projection should keep.

An earlier test case produced **zero** capabilities for exactly this reason: the
capability data sits inside the subtree the filtering discarded.

## 3. Native binary analysis (Ghidra provider)

Target: `rustc.exe`, 12 721 664 bytes, copied to a fixed path because the
Windows Ghidra P0 provider rejects mutable-path targets.

| Run | `REA_GHIDRA_STARTUP_TIMEOUT_MS` | Result |
| --- | --- | --- |
| 1 | default (330 000 ms) | `provider_timeout` after 343.4 s, operation `list_documents` |
| 2 | 1 800 000 ms | `provider_timeout` after 1 825.1 s, operation `list_documents` |

The timeout applies to *startup*, not to a single query
(`docs/mcp-contracts.md`: REA allows 330 000 ms by default, configurable from
1 to 2 147 483 647 ms). Raising the deadline thirty-fold changes nothing: a
12 MB PE does not reach "default auto-analysis complete" at all. The failure is
a timeout with no partial result, not an error that names the cause.

## 4. What a reader can do instead

`reastream` over the same 632 339 104-character document:

| | |
| --- | --- |
| Records | 19 962 |
| Bytes read | 1 264 678 208 |
| Bytes decoded | 632 339 154 |
| Bytes kept | 6 547 610 (**1.04%**) |
| Wall clock | 110 s (release build) |
| Peak memory | bounded by the largest single value |

The whole point is the ratio. A consumer that wants the envelope and a
capability index never needs anything else.
