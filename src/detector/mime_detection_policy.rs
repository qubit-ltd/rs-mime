/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Policy for resolving combined MIME detection from filename and content.

/// Policy for resolving combined MIME detection from filename and content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeDetectionPolicy {
    /// Prefer a definitive filename result without checking content magic.
    PreferFilename,

    /// Check content magic even when filename detection has a definitive result.
    VerifyContent,
}

impl MimeDetectionPolicy {
    /// Tells whether content magic should be checked for a definitive filename match.
    pub(crate) fn should_verify_content(self) -> bool {
        matches!(self, Self::VerifyContent)
    }
}

