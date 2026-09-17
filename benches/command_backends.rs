// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Measures real command and MIME backend latency with bounded batch
//! concurrency.

use std::hint::black_box;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::Barrier;
use std::thread;
use std::time::Instant;

use qubit_command::Command;
use qubit_command::CommandRunner;
use qubit_mime::FfprobeCommandMediaStreamClassifier;
use qubit_mime::FileBasedMediaStreamClassifier;
use qubit_mime::FileCommandMimeDetector;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeConfig;

/// Measures 200 operations after worker warmup, reporting failures outside
/// timing. Each worker blocks at a barrier before collecting latency samples.
fn measure(operation: &(dyn Fn() -> bool + Sync), workers: usize) -> (u128, u128, f64, usize) {
    let barrier = Barrier::new(workers + 1);
    let (mut samples, failures, elapsed) = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let barrier = &barrier;
                scope.spawn(move || {
                    let mut failures = 0;
                    for _ in 0..10 {
                        failures += usize::from(!operation());
                    }
                    barrier.wait();
                    let mut samples = Vec::with_capacity(200 / workers);
                    for _ in 0..200 / workers {
                        let start = Instant::now();
                        let success = black_box(operation());
                        samples.push(start.elapsed().as_nanos());
                        failures += usize::from(!success);
                    }
                    (samples, failures)
                })
            })
            .collect();
        barrier.wait();
        let start = Instant::now();
        let mut samples = Vec::with_capacity(200);
        let mut failures = 0;
        for handle in handles {
            let (worker_samples, worker_failures) = handle.join().expect("benchmark worker must not panic");
            samples.extend(worker_samples);
            failures += worker_failures;
        }
        (samples, failures, start.elapsed())
    });
    samples.sort_unstable();
    (
        samples[(samples.len() - 1) * 50 / 100],
        samples[(samples.len() - 1) * 95 / 100],
        200.0 / elapsed.as_secs_f64(),
        failures,
    )
}

/// Builds matching structured arguments for either external command.
fn arguments(program: &str, path: &Path) -> Vec<std::ffi::OsString> {
    let flags = if program == "file" {
        vec!["--mime-type", "--brief", "--"]
    } else {
        vec![
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
            "-i",
        ]
    };
    flags
        .into_iter()
        .map(Into::into)
        .chain(std::iter::once(path.as_os_str().to_owned()))
        .collect()
}

/// Preflights fixtures, then measures std output, managed output and public
/// MIME calls. Missing tools are reported explicitly; executed operations must
/// all succeed.
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real_files");
    let config = MimeConfig::default();
    for (program, fixture, expected) in [("file", "test.txt", "text/plain"), ("ffprobe", "test.mp3", "audio")] {
        let path = root.join(fixture);
        assert!(path.is_file(), "benchmark fixture must exist");
        let args = arguments(program, &path);
        let probe = match ProcessCommand::new(program).args(&args).output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!("SKIP {program}: {error}");
                continue;
            }
            Err(error) => panic!("cannot start {program}: {error}"),
        };
        assert!(probe.status.success(), "fixture command must succeed");
        assert!(
            std::str::from_utf8(&probe.stdout)
                .expect("fixture output must be UTF-8")
                .contains(expected)
        );
        let runner = CommandRunner::new(config.command_timeout())
            .bounded_output(config.command_output_max_bytes())
            .disable_logging(true);
        let file = FileCommandMimeDetector::default().with_command_runner(runner.clone());
        let ffprobe = FfprobeCommandMediaStreamClassifier::new().with_command_runner(runner.clone());
        if program == "file" {
            assert_eq!(
                file.detect_file_by_content(&path).expect("text fixture must classify"),
                Some("text/plain".to_owned())
            );
        } else {
            assert_eq!(
                ffprobe
                    .classify_by_local_file(&path)
                    .expect("audio fixture must classify"),
                MediaStreamType::AudioOnly
            );
        }
        let standard = || {
            ProcessCommand::new(program)
                .args(&args)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        let managed = || runner.run(Command::new(program).args_os(&args)).is_ok();
        let public = || {
            if program == "file" {
                file.detect_file_by_content(&path).is_ok()
            } else {
                ffprobe.classify_by_local_file(&path).is_ok()
            }
        };
        for (mode, operation) in [
            ("std", &standard as &(dyn Fn() -> bool + Sync)),
            ("runner", &managed),
            ("mime", &public),
        ] {
            assert!(operation(), "benchmark preflight must succeed");
            for workers in [1, 4] {
                for round in 1..=3 {
                    let (p50, p95, throughput, failures) = measure(operation, workers);
                    println!("{program},{mode},{workers},{round},200,{p50},{p95},{throughput:.3},{failures}");
                    assert_eq!(
                        failures, 0,
                        "failed operations must not count as a successful benchmark"
                    );
                }
            }
        }
    }
}
