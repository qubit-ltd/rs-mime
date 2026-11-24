# MIME-047 matcher baseline and follow-up

## Measurement setup

- Revision before the matcher change: `4b0ae370ffc10f05f2c38373502a049d991ea568`
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, LLVM 21.1.8
- Target: `x86_64-unknown-linux-gnu`
- OS: Linux `6.8.0-147-generic`
- CPU: Intel Xeon E5-2680 v4 @ 2.40 GHz; 56 logical CPUs reported
- Profile: Cargo test profile (`debug`, unoptimized); no CPU affinity set
- Each sample averages 200 synchronous content queries and 200 async matcher queries. Three rounds were collected per scenario. Async timing excludes repository construction and operation-manager setup. The normal-rule workload was measured after implementation, as required for post-change throughput.
- Command: `cargo test --lib sample_matcher_workloads -- --ignored --nocapture`

These are directional measurements from a shared host. The old matcher had no shared work counter, so old maximum work per poll was not instrumented. Old maximum poll durations were recorded by polling one async query at a time. New work counts are matcher units: magic rules, matcher nodes, candidate offsets, and byte comparisons. The new maximum poll duration is the slowest individual poll in the same manual polling harness.

## Workloads

| Scenario | Input and rules |
| --- | --- |
| Early failure | 256 `X` bytes; one `NEVER` string rule at offsets `0:255` |
| Late mismatch | 160 `A` bytes; one 128-byte `A…AZ` rule at offsets `0:31`, giving 32 candidate positions with a final-byte mismatch |
| Wide shallow | `P`; one matching parent with 512 shallow children, each requiring no value comparison because its value is wider than the input |
| Many sub-threshold roots | 32 `X` bytes; one magic rule with 128 `N{index}` roots, each configured for offsets `0:31` |
| Normal rules | `SIGNAL-063`; 64 ordinary MIME rules with fixed-width `SIGNAL-{index:03}` values at offset 0; the final rule matches |

## Results

Times are nanoseconds per query. `new poll work` and `new max poll` are from the counted implementation. Throughput is the approximate inverse of async time, rounded to thousands of queries per second.

| Scenario | Old async samples | New async samples | Old → new throughput | New work/query | New max work/poll | New max poll time (samples) |
| --- | --- | --- | ---: | ---: | ---: | --- |
| Early failure | 43,505 / 61,714 / 44,920 | 30,741 / 27,834 / 27,820 | 22k → 36k q/s | 506 | 506 | 113,746 / 48,424 / 62,346 ns |
| Late mismatch | 200,453 / 190,313 / 178,711 | 119,484 / 115,340 / 124,952 | 5k → 8k q/s | 4,130 | 4,096 | 156,810 / 286,726 / 291,779 ns |
| Wide shallow | 149,607 / 149,782 / 156,796 | 96,672 / 95,332 / 97,559 | 7k → 10k q/s | 516 | 516 | 183,761 / 143,500 / 171,808 ns |
| Many roots | 908,425 / 976,470 / 701,045 | 446,024 / 434,374 / 449,423 | 1k → 2k q/s | 7,773 | 4,096 | 595,418 / 332,814 / 332,949 ns |
| Normal rules (64) | — | 55,328 / 62,159 / 63,947 | — → 16k q/s | 772 | 772 | 112,761 / 96,398 / 158,263 ns |

New matcher yields per query were 0, 1, 0, 1, and 0, respectively. The work counter's deterministic test observed a maximum of exactly 4,096 units per poll across 1,000 magic roots, offsets, and byte comparisons. The wide shallow cancellation regression requests stop on the first pending poll and verifies that the operation settles as stopped. Its five stop-request-to-settlement samples were 195,434 / 90,776 / 96,358 / 82,273 / 92,011 ns (median 92,011 ns, maximum 195,434 ns).

The work counter initially used an async call for each unit. That version materially slowed late mismatches. Changing it to synchronous increments with an async yield only at the 4,096-unit boundary reduced per-unit overhead; the table reports the optimized version.

## Captured logs

- Old implementation, three rounds: `/tmp/t4-rs-mime-old-matcher-baseline.log`
- New implementation, three rounds: `/tmp/t4-rs-mime-new-matcher-samples.log`
- Stop-response samples: `/tmp/t4-rs-mime-stop-response.log`
- Red regression evidence: `/tmp/t4-rs-mime-wide-shallow-red.log`
- Green matcher tests: `/tmp/t4-rs-mime-cooperative-tests.log`
