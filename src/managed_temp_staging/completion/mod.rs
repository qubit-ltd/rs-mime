// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! One-shot completion channel used by staging workers.

mod completer;
mod completion;
mod completion_future;
mod completion_pair;

pub(in crate::managed_temp_staging) use completer::Completer;
pub(in crate::managed_temp_staging) use completion::Completion;
pub(in crate::managed_temp_staging) use completion_future::CompletionFuture;
pub(in crate::managed_temp_staging) use completion_pair::completion_pair;
