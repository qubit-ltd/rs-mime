// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashSet;

use qubit_mime::MediaStreamType;

#[test]
fn test_media_stream_type_is_copyable_comparable_and_hashable() {
    let audio = MediaStreamType::AudioOnly;
    let copied = audio;
    let mut set = HashSet::new();

    set.insert(MediaStreamType::None);
    set.insert(copied);
    set.insert(MediaStreamType::VideoOnly);
    set.insert(MediaStreamType::VideoWithAudio);

    assert_eq!(MediaStreamType::AudioOnly, copied);
    assert!(set.contains(&MediaStreamType::AudioOnly));
    assert_eq!(4, set.len());
}
