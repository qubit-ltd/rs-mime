/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! MIME repository data model and freedesktop rule parsing.

pub(crate) mod magic_value_type;
pub(crate) mod mime_glob;
pub(crate) mod mime_magic;
pub(crate) mod mime_magic_matcher;
pub(crate) mod mime_repository;
pub(crate) mod mime_type;
pub(crate) mod mime_type_builder;

pub use magic_value_type::MagicValueType;
pub use mime_glob::MimeGlob;
pub use mime_magic::MimeMagic;
pub use mime_magic_matcher::MimeMagicMatcher;
pub use mime_repository::MimeRepository;
pub use mime_type::MimeType;
pub use mime_type_builder::MimeTypeBuilder;
