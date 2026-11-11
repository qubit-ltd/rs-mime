# MIME inheritance stack investigation

## Conclusion

MIME-005 was reproduced on the pre-fix implementation. A 16,384-node acyclic
inheritance chain parses successfully and reaches `matches_content`, then aborts
with a stack overflow on both a 2 MiB main-thread stack and a 2 MiB worker
thread stack. The 128, 1,024, and 4,096-node cases complete successfully on both
stacks. Each type has at most one parent and the final type has one shallow
magic matcher, so the 128-level nested-matcher limit is not involved. The
reproduction justifies replacing recursive inheritance traversal with an
explicit heap-backed DFS.

The probe measures the public `MimeType::matches_content` route. It does not
claim that `MimeRepository::detect_by_content` uses this traversal; that entry
point uses the separate MagicIndex. The observed practical boundary on this
toolchain is between 4,096 and 16,384 links for a 2 MiB stack, not a supported
maximum. The fix imposes no inheritance depth limit.

## Reproduction environment and commands

- Date: 2026-11-11 UTC
- Host: `Linux mas-server 6.8.0-147-generic x86_64 GNU/Linux`
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`
- Source before the traversal change: rs-mime HEAD `bf57cc9a88ab2170e5461cc776e70358f0d254d2`
- Probe crate: standalone temporary Cargo project. The original reproduction
  used the baseline checkout at the target worktree path and did not alter this
  crate's dependency files.
- Build: `cargo build --release --manifest-path /tmp/mime-inheritance-probe/Cargo.toml`
- Run all cases from a shell with core dumps disabled:

```sh
ulimit -c 0
for mode in main worker; do
  for length in 128 1024 4096 16384; do
    if [ "$mode" = main ]; then
      (ulimit -s 2048; timeout 30s /tmp/mime-inheritance-probe/target/release/mime-inheritance-probe "$length" main)
    else
      timeout 30s /tmp/mime-inheritance-probe/target/release/mime-inheritance-probe "$length" worker
    fi
    printf 'exit=%s\n' "$?"
  done
done
```

The main mode limits the process main stack with `ulimit -s 2048`. Worker mode
uses `std::thread::Builder::stack_size(2 * 1024 * 1024)`. Each probe invocation
is its own process with a 30-second timeout. `PARSED`, `CALL_BEGIN`, and
`CALL_END` stderr markers distinguish XML/repository setup from the matching
call.

## Results before the fix

| Stack | Chain | Markers/result | Exit | Interpretation |
|---|---:|---|---:|---|
| main, 2 MiB | 128 | PARSED, CALL_BEGIN, CALL_END true | 0 | passed |
| main, 2 MiB | 1,024 | PARSED, CALL_BEGIN, CALL_END true | 0 | passed |
| main, 2 MiB | 4,096 | PARSED, CALL_BEGIN, CALL_END true | 0 | passed |
| main, 2 MiB | 16,384 | PARSED, CALL_BEGIN, stack overflow; no CALL_END | 134 | traversal stack exhausted and process aborted |
| worker, 2 MiB | 128 | PARSED, CALL_BEGIN, CALL_END true | 0 | passed |
| worker, 2 MiB | 1,024 | PARSED, CALL_BEGIN, CALL_END true | 0 | passed |
| worker, 2 MiB | 4,096 | PARSED, CALL_BEGIN, CALL_END true | 0 | passed |
| worker, 2 MiB | 16,384 | PARSED, CALL_BEGIN, stack overflow; no CALL_END | 134 | traversal stack exhausted and process aborted |

The `timeout` wrapper printed the runtime abort message but did not time out. No
core file was retained (`ulimit -c` was `0`). A preliminary probe build failed
with Rust E0373/E0597 because the spawned closure borrowed local data; the probe
source was corrected to move the repository and arguments into the worker, and
the corrected release build succeeded. This was a probe implementation error,
not a repository parse or traversal result.

## Complete probe source

To recreate the original pre-fix source and its exact dependency graph, create
detached worktrees from the source repositories at these commits. The crate's
path dependencies resolve relative to its parent directory, so keep the
worktrees in this sibling layout:

```sh
mkdir -p /tmp/mime-inheritance-source
git -C /data/working/qubit/rust-common/rs-mime worktree add --detach /tmp/mime-inheritance-source/rs-mime bf57cc9a88ab2170e5461cc776e70358f0d254d2
git -C /data/working/qubit/rust-common/rs-fs worktree add --detach /tmp/mime-inheritance-source/rs-fs 59d77ad5691e6012f060e95c9970177254e64d1a
git -C /data/working/qubit/rust-common/rs-command worktree add --detach /tmp/mime-inheritance-source/rs-command e73dc5f45d9619a308ac523108fc73e9ab57137d
git -C /data/working/qubit/rust-common/rs-config worktree add --detach /tmp/mime-inheritance-source/rs-config a80fba0a9eeeb985cc77286ea70f7a5edd337da2
git -C /data/working/qubit/rust-common/rs-local-files worktree add --detach /tmp/mime-inheritance-source/rs-local-files fcad0a27335690f9374dd29ef865bade597a02df
git -C /data/working/qubit/rust-common/rs-redact worktree add --detach /tmp/mime-inheritance-source/rs-redact d3116d19fc40d6e23db0fdca4db3748d0dede7d8
git -C /data/working/qubit/rust-common/rs-fs-local worktree add --detach /tmp/mime-inheritance-source/rs-fs-local 599af36d5aea17e760b95a6f081957e6bbb5eddb
```

This avoids consuming uncommitted source-worktree changes. The commits above
are the exact dependency versions used by the probe.

Create `/tmp/mime-inheritance-probe/Cargo.toml`:

```toml
[package]
name = "mime-inheritance-probe"
version = "0.1.0"
edition = "2024"

[dependencies]
qubit-mime = { path = "/tmp/mime-inheritance-source/rs-mime" }
```

Create `/tmp/mime-inheritance-probe/src/main.rs`:

```rust
use qubit_mime::MimeRepository;
use std::env;
use std::thread;

fn probe(repository: MimeRepository, length: usize, mode: String) {
    eprintln!("CALL_BEGIN length={length} mode={mode}");
    let result = repository.get("application/x-node-0").expect("start node").matches_content(&repository, b"PROBE");
    eprintln!("CALL_END length={length} result={result}");
    assert!(result);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let length: usize = args[1].parse().expect("chain length");
    let mode = args[2].clone();
    let mut xml = String::from("<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">");
    for index in 0..length {
        xml.push_str(&format!("<mime-type type=\"application/x-node-{index}\">"));
        if index + 1 < length {
            xml.push_str(&format!("<sub-class-of type=\"application/x-node-{}\"/>", index + 1));
        } else {
            xml.push_str("<magic priority=\"50\"><match type=\"string\" value=\"PROBE\" offset=\"0\"/></magic>");
        }
        xml.push_str("</mime-type>");
    }
    xml.push_str("</mime-info>");
    let repository = MimeRepository::from_xml(&xml).expect("XML parse/repository build");
    eprintln!("PARSED length={length}");
    if mode == "worker" {
        thread::Builder::new().name("probe-2mib".into()).stack_size(2 * 1024 * 1024)
            .spawn(move || probe(repository, length, mode)).expect("spawn").join().expect("worker");
    } else {
        probe(repository, length, mode);
    }
}
```

## Post-fix check

The implementation now uses a `Vec<&MimeType>` pending stack and the existing
canonical-name `HashSet` for cycle/diamond deduplication. Parent names are
pushed in reverse so the previous depth-first order and equal-priority
last-visited-wins rule remain intact. A test-only 16,384-node chain passes via
`matches_content`. The same external subprocess matrix is rerun after the code
change; its results are appended here before handoff.

For the post-fix comparison, the same manifest's dependency path was pointed
at `/data/working/qubit/.worktrees/rs-mime/rs-mime`, then the release probe
was rebuilt and rerun. To repeat after this worktree is removed, point it at a
checkout containing the explicit-stack change instead; the probe source is
unchanged.

The diamond test confirms traversal terminates and the returned `MimeMagic`
identity follows DFS order. Exact visit counts are not externally observable:
`MimeMagic::matches` is immutable and deterministic, and revisiting the same
shared node cannot change the accumulated result. The visited-name set is
therefore asserted indirectly through cycle/diamond completion and the
identity/priority semantics rather than adding production instrumentation.

After the explicit-stack change, the probe was rebuilt with the same release
command and all eight subprocess cases were rerun with the same 2 MiB stacks,
30-second per-process timeout, and disabled core dumps. All lengths (128, 1,024,
4,096, and 16,384) in both main and worker modes printed PARSED,
CALL_BEGIN, CALL_END true, and exited 0. The formerly overflowing 16,384-node
case now completes in both modes.
