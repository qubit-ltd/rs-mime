/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! MIME detector implementations.

pub(crate) mod abstract_mime_detector;
pub(crate) mod detection_source;
pub(crate) mod file_based_mime_detector;
pub(crate) mod file_command_mime_detector;
pub(crate) mod repository_mime_detector;
pub(crate) mod stream_based_mime_detector;
pub(crate) mod string_list_mime_detector_backend;

pub use abstract_mime_detector::AbstractMimeDetector;
pub use detection_source::DetectionSource;
pub use file_based_mime_detector::FileBasedMimeDetector;
pub use file_command_mime_detector::FileCommandMimeDetector;
pub use repository_mime_detector::RepositoryMimeDetector;
pub use stream_based_mime_detector::StreamBasedMimeDetector;
pub use string_list_mime_detector_backend::StringListMimeDetectorBackend;
