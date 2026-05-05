use qubit_mime::{
    MagicValueType,
    MimeGlob,
    MimeMagic,
    MimeMagicMatcher,
    MimeTypeBuilder,
};

#[test]
fn test_mime_type_builder_collects_metadata_and_matching_rules() {
    let matcher =
        MimeMagicMatcher::new(MagicValueType::String, 0, 0, b"DATA".to_vec(), None, vec![])
            .unwrap();
    let mime_type = MimeTypeBuilder::new("application/x-data")
        .description("", "Data file")
        .alias("application/data")
        .glob(MimeGlob::new("*.data", 80, false).unwrap())
        .magic(MimeMagic::new(50, vec![matcher]))
        .super_type("application/octet-stream")
        .build();

    assert_eq!("application/x-data", mime_type.name());
    assert_eq!(Some("Data file"), mime_type.description());
    assert_eq!(&["application/data".to_owned()], mime_type.aliases());
    assert_eq!(Some("data"), mime_type.preferred_extension());
    assert_eq!(1, mime_type.magics().len());
    assert_eq!(
        &["application/octet-stream".to_owned()],
        mime_type.super_types()
    );
}
