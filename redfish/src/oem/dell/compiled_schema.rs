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

#[allow(clippy::doc_markdown)]
#[allow(clippy::absolute_paths)]
#[allow(clippy::option_option)]
#[allow(clippy::missing_const_for_fn)]
#[allow(clippy::struct_field_names)]
#[allow(clippy::too_long_first_doc_paragraph)]
#[allow(clippy::unused_trait_names)]
#[allow(missing_docs)]
pub mod redfish {
    include!(concat!(env!("OUT_DIR"), "/oem-dell.rs"));
}
