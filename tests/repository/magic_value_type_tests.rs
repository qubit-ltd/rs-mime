use qubit_mime::MagicValueType;

#[test]
fn test_magic_value_type_round_trips_freedesktop_names() {
    let cases = [
        ("string", MagicValueType::String),
        ("host16", MagicValueType::Host16),
        ("host32", MagicValueType::Host32),
        ("big16", MagicValueType::Big16),
        ("big32", MagicValueType::Big32),
        ("little16", MagicValueType::Little16),
        ("little32", MagicValueType::Little32),
        ("byte", MagicValueType::Byte),
    ];

    for (name, value_type) in cases {
        assert_eq!(Some(value_type), MagicValueType::from_name(name));
        assert_eq!(name, value_type.name());
    }
    assert_eq!(None, MagicValueType::from_name("unknown"));
}
