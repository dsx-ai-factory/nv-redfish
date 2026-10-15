// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/// Configuration of Rust server model generation.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Maximum number of required fields accepted directly by `new`.
    ///
    /// As with client Action functions, the boundary is inclusive. Larger
    /// constructors receive a generated `<Type>Required` structure instead.
    pub constructor_max_param_number_threshold: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            constructor_max_param_number_threshold: 3,
        }
    }
}
