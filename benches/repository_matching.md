# Repository matching benchmark

`repository_matching.rs` records repository construction and repeated content
queries. Measurements below use the same release benchmark, inputs, and one
CPU on the same machine. Each printed sample is the elapsed time divided by
100 repository constructions or 100,000 queries; values are nanoseconds per
operation.

## Environment

- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, LLVM 21.1.8
- Target: `x86_64-unknown-linux-gnu`
- OS: Linux 6.8.0-147-generic
- CPU: Intel Xeon E5-2680 v4 @ 2.40GHz; 56 logical CPUs reported by the host
- Affinity: CPU 0 via `taskset -c 0`
- Command: `taskset -c 0 cargo bench --bench repository_matching`

The host is shared, and CPU affinity does not prevent other processes from
using CPU 0. These samples are directional measurements, not a stable latency
guarantee.

## Results

The baseline uses the original two-pass `MagicIndex` lookup and the
allocation-based numeric matcher at commit `e94a1e1` (MIME-001 included, before
MIME-003). The optimized code is commit `7432a98` plus final XML prolog
validation, which does not affect this benchmark. Both sides use the same
benchmark source and configuration.

| Case (ns/op) | Baseline samples | Optimized samples |
| --- | ---: | ---: |
| Repository construction | 17,602 / 16,151 / 16,160 | 18,038 / 16,726 / 17,094 |
| Bundled PDF query | 30,111 / 30,303 / 29,531 | 13,787 / 13,075 / 13,165 |
| Unmatched query | 256 / 256 / 257 | 213 / 211 / 211 |
| Highest priority | 365 / 365 / 365 | 321 / 312 / 311 |
| Equal priority | 420 / 417 / 417 | 319 / 320 / 319 |
| Offset range | 559 / 555 / 557 | 405 / 404 / 405 |
| Numeric mask | 415 / 413 / 413 | 289 / 288 / 289 |

The pinned query samples were consistent within each case and were lower for
the optimized matcher cases. Construction samples were also close, with the
optimized median slightly higher. No fixed speedup target was set; the
deterministic implementation changes are that matching no longer evaluates
rules in a second pass and numeric comparisons no longer allocate reordered
value or mask vectors.
