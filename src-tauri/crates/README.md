# Rust crates

This directory contains reusable Rust capabilities:

- `mangodisk-core` owns product domains, use cases, rules, indexing, cleanup,
  history, and reporting.
- `mangodisk-platform` implements macOS and Windows contracts for volumes,
  paths, links, system exclusions, application inventory, and permanent deletion.
- `mangodisk-cli` is a sibling adapter over Core use cases.

The Tauri crate only assembles the application, converts command arguments,
and forwards progress events. It does not own platform policy or scanning
behavior.

## Scan exclusions

Adapters select per-module scopes and pass an immutable `ScanExclusionOptions`
into Core. Exact name rules distinguish files from folders, match case-sensitively
at every depth on all platforms, and accept no paths or wildcards. Native walkers
prune matching directories before descending; cached results use the same policy.
Name rules are included in the index configuration fingerprint. Windows volume-wide
NTFS layout aggregates cannot represent name exclusions, so active name rules use
the directory-walking fallback, as path exclusions already do. The fallback reason
is logged explicitly; unfiltered scans retain their existing native fast path.
Disk-analysis sessions retain the resolved exclusion paths used by traversal, so
parent deletion cannot bypass exclusions expressed through system path aliases.
Storage and cleanup share `filesystem::exclusion_paths` for exclusion validation
and resolution. Missing descendants retain the resolved existing parent; ambiguous
parent components and user links are rejected. Disconnected volume roots remain
configured. Fixed system aliases and path identity comparisons belong to Platform;
domain modules own only scope, pruning, and cache policy.

Legacy cleanup path exclusions protect project artifacts only. Cleanup name rules
also protect declarative and custom rules. Atomic directory removal checks for
protected descendants before deletion and again during staged removal. Specialized
cleaners that cannot preserve names return an explicit, unselectable `excluded`
status. The deep-cleanup adapter skips application leftovers with active name
rules and rejects execution after the exclusion snapshot changes.

Frontend exclusion preferences migrate schema 2 to schema 3 by preserving path
scopes and adding an empty name list. No new exclusions are enabled by migration.

For a repeatable storage workload, build the `scan_exclusion_benchmark` Core
example in release mode and supply an isolated fixture directory plus `none`,
`paths`, or `names`. It creates 6,144 dependency files, 24 source files, and two
64 MiB duplicates. Alternate baseline and candidate processes over the same
fixture, discard warmups, and compare repeated measurements with matching result
counts. Keep fixture data and raw measurements outside tracked source.
