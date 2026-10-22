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

use core::pin::Pin;
use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

use futures_io::AsyncRead;

const OEM_PREFIX: &str = "Oem";

/// Async reader accepted by upload methods.
pub trait UploadReader: AsyncRead + Send + 'static {}

impl<T> UploadReader for T where T: AsyncRead + Send + 'static {}

/// Named data stream accepted by multipart upload methods.
pub struct DataStream<R> {
    /// Multipart filename for this stream.
    pub name: String,

    /// Streamed upload data.
    pub reader: R,

    /// Known stream length, when available.
    pub content_length: Option<u64>,
}

impl<R> DataStream<R> {
    /// Create a named data stream without a known content length.
    #[must_use]
    pub fn new(name: impl Into<String>, reader: R) -> Self {
        Self {
            name: name.into(),
            reader,
            content_length: None,
        }
    }

    /// Attach a known content length.
    #[must_use]
    pub const fn with_content_length(mut self, content_length: u64) -> Self {
        self.content_length = Some(content_length);
        self
    }
}

/// Streamed body accepted by deprecated raw upload methods.
pub struct UploadStream<R> {
    /// Streamed upload data.
    pub reader: R,

    /// Known stream length, when available.
    pub content_length: Option<u64>,
}

impl<R> UploadStream<R> {
    /// Create a streamed upload body without a known content length.
    #[must_use]
    pub const fn new(reader: R) -> Self {
        Self {
            reader,
            content_length: None,
        }
    }

    /// Attach a known content length.
    #[must_use]
    pub const fn with_content_length(mut self, content_length: u64) -> Self {
        self.content_length = Some(content_length);
        self
    }
}

/// Error returned when an OEM multipart part name is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OemMultipartPartNameError {
    /// Invalid multipart part name.
    pub name: String,
}

impl fmt::Display for OemMultipartPartNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OEM multipart part name must start with Oem: {}",
            self.name
        )
    }
}

impl StdError for OemMultipartPartNameError {}

/// Reader type used for OEM multipart form parts.
pub type OemMultipartPartReader = Pin<Box<dyn AsyncRead + Send + 'static>>;

/// OEM multipart form part.
pub struct OemMultipartPart {
    /// Multipart part name.
    pub name: String,

    /// Streamed part data.
    pub reader: OemMultipartPartReader,

    /// Optional part content type.
    pub content_type: Option<String>,

    /// Known part length, when available.
    pub content_length: Option<u64>,
}

impl OemMultipartPart {
    /// Checks if the OEM part name is valid per the spec.
    #[must_use]
    pub fn is_name_valid(&self) -> bool {
        self.name.starts_with(OEM_PREFIX)
    }
}

impl OemMultipartPart {
    /// Create an OEM multipart part.
    ///
    /// # Errors
    ///
    /// Returns an error if `name` does not start with `Oem`.
    pub fn new(
        name: impl Into<String>,
        reader: impl UploadReader,
    ) -> Result<Self, OemMultipartPartNameError> {
        let name = name.into();

        if !name.starts_with(OEM_PREFIX) {
            return Err(OemMultipartPartNameError { name });
        }

        Ok(Self {
            name,
            reader: Box::pin(reader),
            content_type: None,
            content_length: None,
        })
    }

    /// Attach a content type.
    #[must_use]
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Attach a known content length.
    #[must_use]
    pub const fn with_content_length(mut self, content_length: u64) -> Self {
        self.content_length = Some(content_length);
        self
    }
}

/// Multipart `UpdateService` upload request data.
pub struct MultipartUpdateRequest<'a, U, V> {
    /// Redfish `UpdateParameters` JSON part.
    pub update_parameters: &'a V,

    /// Named stream sent as the Redfish `UpdateFile` part.
    pub update_stream: DataStream<U>,

    /// Optional OEM-defined multipart parts.
    pub oem_parts: Vec<OemMultipartPart>,

    /// Timeout used only for this upload request.
    pub upload_timeout: Duration,
}

/// `UpdateService` raw `HttpPushUri` upload request data.
pub struct HttpPushUriUpdateRequest<U> {
    /// Streamed update image data.
    pub update_stream: UploadStream<U>,

    /// Timeout used only for this upload request.
    pub upload_timeout: Duration,
}
