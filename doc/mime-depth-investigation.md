# MIME matcher depth investigation

## Conclusion

MIME-002 is confirmed. On a 2 MiB worker thread, the repository's `from_xml`
path aborted with stack overflow for 1,024 nested `<match>` elements. The
locked `roxmltree` parser itself handled XML element depths through 16,384 on
the same stack, so the failure is in the repository's processing path rather
than `roxmltree`'s XML tokenizer. The public `from_xml` API combines matcher
parsing and index construction, so the precise internal recursive call that
overflows cannot be separated through the public API without instrumentation.

Programmatically constructed matcher trees also have unsafe recursive
operations: cloning a depth-4,096 matcher aborted on a 2 MiB stack. At larger
depths, `max_test_bytes`, `matches`, and eventually drop also aborted. This
supports guarding both XML parsing and programmatic construction. The
implementation sets the maximum matcher depth to 128, with the root counted as
level one. Bundled rules have a maximum depth of 5. Focused boundary tests
accept 128 and reject 129 for both entry points.

## Environment and inputs

- OS/platform: Linux `mas-server`, kernel `6.8.0-147-generic`, x86_64,
  `x86_64-unknown-linux-gnu`.
- Rust: `rustc 1.94.0` (`4a4ef493e`, dated 2026-03-02).
- Locked XML dependency: `roxmltree 0.21.2` from `Cargo.lock`.
- Main process stack: `ulimit -s` reported 8192 KiB.
- Small worker stack: `std::thread::Builder::stack_size(2 * 1024 * 1024)`.
- Timeout/core handling: every risky probe ran with `ulimit -c 0` and GNU
  `timeout 30s`; the process wrapper was `script -q -e -c ... /dev/null` so
  progress markers were visible. Stack-overflow aborts returned status 134
  (SIGABRT); no core file was produced.
- XML test input: standard shared-MIME-info `mime-info` / `mime-type` / `magic`
  wrappers around `depth` nested
  `<match type="byte" value="1" offset="0">` elements and matching closing
  tags.
- Programmatic test input: a leaf `Byte` matcher repeatedly wrapped as the
  sole child of a new `MimeMagicMatcher`.
- Bundled data scan: Python 3 `xml.etree.ElementTree` parsed
  `resources/freedesktop.org-v2.4.xml` and recursively counted consecutive
  `<match>` descendants below each `<magic>`. Maximum observed depth was 5.

## Upstream parser inspection

The locked source was inspected at
`/home/starfish/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/roxmltree-0.21.2/src/tokenizer.rs`.
Its `parse_content` implementation (around line 662) explicitly tracks nested
element depth in a loop instead of recursing. The depth 10 limit in
`parse.rs` (around lines 471–475) applies to nested entity references and does
not constrain XML element depth or this crate's matcher recursion.

## Probe commands and results

The standalone repository probe was compiled against the already-built
baseline library, before the guard was added:

```bash
rustc --edition=2024 /tmp/mime_depth_probe.rs \
  -o /tmp/mime_depth_probe \
  --extern qubit_mime=target/debug/deps/libqubit_mime-03b3b35a0438ed48.rlib \
  -L dependency=target/debug/deps
```

For each mode (`xml`, `tree`), the following matrix was run in a fresh process
with `ulimit -c 0` and `timeout 30s`; `main` means the 8 MiB main-thread stack,
and `2097152` selects the 2 MiB worker stack:

```bash
for mode in xml tree; do
  for stack in main 2097152; do
    for depth in 32 128 256 1024; do
      if [ "$stack" = main ]; then
        timeout 30s /tmp/mime_depth_probe "$mode" "$depth"
      else
        timeout 30s /tmp/mime_depth_probe "$mode" "$depth" "$stack"
      fi
    done
  done
done
```

The optional stack argument was omitted for `main`. The command was run after
compiling the probe against the baseline library, before guard changes; the
same Cargo artifact path was later rebuilt for guarded tests. XML mode called
`MimeRepository::from_xml` (parse plus indexing), then content detection,
repository clone, and drop. Tree mode constructed the tree, then called
`max_test_bytes`, `matches`, clone, and drop.

| Input and stack | Result |
| --- | --- |
| XML depths 32, 128, 256, 1,024 on 8 MiB main stack | All stages passed, exit 0 |
| XML depths 32, 128, 256 on 2 MiB worker stack | All stages passed, exit 0 |
| XML depth 1,024 on 2 MiB worker stack | `from_xml` overflowed before returning; abort, exit 134 |
| Programmatic depths 32, 128, 256, 1,024 on 8 MiB main stack | Construction, byte-length query, match, clone, and drop passed; exit 0 |
| Programmatic depths 32, 128, 256, 1,024 on 2 MiB worker stack | Same stages passed; exit 0 |
| XML depths 128 and 129 before guard, on both stacks | All stages passed; exit 0 |
| Programmatic depths 128 and 129 before guard, on both stacks | All stages passed; exit 0 |

To isolate recursive matcher operations, the stage-marked baseline probe was
compiled against the pre-guard library artifact in the original checkout:

```bash
rustc --edition=2024 /tmp/mime_depth_probe_stages.rs \
  -o /tmp/mime_depth_probe_stages \
  --extern qubit_mime=/data/working/qubit/rust-common/rs-mime/target/debug/deps/libqubit_mime-b3545763c4da5a23.rlib \
  -L dependency=/data/working/qubit/rust-common/rs-mime/target/debug/deps
```

Each stage ran in a fresh process using:

```bash
ulimit -c 0
timeout 30s script -q -e -c \
  "/tmp/mime_depth_probe_stages tree STAGE DEPTH 2097152" /dev/null
```

For `clone`, the helper printed immediately after clone construction and leaked
the original and clone so drop could not confound the result. For `drop`, it
printed immediately before and after dropping the tree. Results:

| Depth | `max_test_bytes` | `matches` | Clone | Drop |
| ---: | --- | --- | --- | --- |
| 4,096 | Passed | Passed | Stack overflow before post-clone marker; abort, exit 134 | Passed |
| 8,192 | Stack overflow after construction marker; abort, exit 134 | Same | Same | Passed |
| 16,384 | Stack overflow after construction marker; abort, exit 134 | Same | Same | Stack overflow after pre-drop marker; abort, exit 134 |

The direct upstream-parser probe used a separate binary calling
`roxmltree::Document::parse` on nested elements:

```bash
timeout 30s script -q -e -c \
  "/tmp/roxml_depth_probe DEPTH 2097152" /dev/null
```

With core dumps disabled, depths 1,024, 8,192, and 16,384 each printed
`Document::parse:ok`, dropped the document, and exited 0.

## Guard verification

The focused test command was:

```bash
cargo test --locked --test mod magic_depth
```

Result: 2 passed, 0 failed (288 filtered out). The tests verify that both XML
and programmatic construction accept depth 128 and reject depth 129. At depth
128 they also check the repository index/content match, matcher byte length,
matcher match, clone, and drop paths. The XML rejection is
`MimeError::InvalidXmlAttribute`; programmatic construction returns
`MimeError::InvalidMagicMatcher`.

This limit tightens behavior for custom inputs deeper than 128 levels. The
bundled database remains below the limit (depth 5); callers can simplify or
restructure unusually deep custom rules. No evidence here establishes a
failure at depths 128 or 129; those probes passed before the guard, and 129 is
rejected now to enforce the chosen bound consistently.
