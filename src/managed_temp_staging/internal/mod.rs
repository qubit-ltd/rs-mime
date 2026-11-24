// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private payload types used by managed temporary staging.

mod create_staged_payload;
mod create_temp_job;
mod write_chunk_payload;

pub(super) use create_staged_payload::CreateStagedPayload;
pub(super) use create_temp_job::CreateTempJob;
pub(super) use write_chunk_payload::WriteChunkPayload;
