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
