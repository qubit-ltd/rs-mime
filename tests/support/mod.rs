/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared integration test support.

#[cfg(unix)]
mod path_env_guard;

#[cfg(unix)]
pub(crate) use path_env_guard::PathEnvGuard;
