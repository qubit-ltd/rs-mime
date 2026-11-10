// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Measures repository construction and repeated content matching.

use std::hint::black_box;
use std::time::Instant;

use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeRepository;

const XML: &str = r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-range"><magic priority="40"><match type="string" value="RANGE" offset="0:64"/></magic></mime-type>
  <mime-type type="application/x-number"><magic priority="70"><match type="big32" value="0x12345670" mask="0xfffffff0" offset="0"/></magic></mime-type>
  <mime-type type="application/x-tie-a"><magic priority="80"><match type="string" value="TIED" offset="0"/></magic></mime-type>
  <mime-type type="application/x-tie-b"><magic priority="80"><match type="string" value="TIED" offset="0"/></magic></mime-type>
  <mime-type type="application/x-duplicate"><magic priority="60"><match type="string" value="DUP" offset="0"/><match type="string" value="DUP" offset="0"/></magic></mime-type>
  <mime-type type="application/x-high"><magic priority="100"><match type="string" value="HIGH" offset="0"/></magic></mime-type>
</mime-info>"#;

fn measure_construction() -> u128 {
    let start = Instant::now();
    for _ in 0..100 {
        black_box(MimeRepository::from_xml(black_box(XML)).expect("benchmark XML must parse"));
    }
    start.elapsed().as_nanos() / 100
}

fn measure_queries(repository: &MimeRepository, bytes: &[u8]) -> u128 {
    let start = Instant::now();
    for _ in 0..100_000 {
        black_box(repository.detect(
            "sample.bin",
            black_box(bytes),
            MimeDetectionPolicy::ContentFirst,
        ));
    }
    start.elapsed().as_nanos() / 100_000
}

fn main() {
    println!("repository_matching: 100 construction / 100000 queries per sample; ns/op");
    for round in 1..=3 {
        println!("construction round {round}: {}", measure_construction());
    }
    let repository = MimeRepository::from_xml(XML).expect("benchmark XML must parse");
    let bundled = MimeRepository::bundled();
    for round in 1..=3 {
        println!(
            "query bundled_pdf round {round}: {}",
            measure_queries(bundled, b"%PDF-1.7")
        );
    }
    for (name, sample) in [
        ("unmatched", b"not a known sample".as_slice()),
        ("highest", b"HIGH".as_slice()),
        ("multiple_equal", b"TIED".as_slice()),
        (
            "range_end",
            b"............................................................RANGE".as_slice(),
        ),
        ("numeric_mask", &[0x12, 0x34, 0x56, 0x7f]),
    ] {
        for round in 1..=3 {
            println!(
                "query {name} round {round}: {}",
                measure_queries(&repository, sample)
            );
        }
    }
}
