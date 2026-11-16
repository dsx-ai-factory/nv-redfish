// SPDX-FileCopyrightText: Copyright (c) 2025 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

#[cfg(feature = "reqwest-client")]
#[allow(dead_code)]
pub mod test_utils {
    use nv_redfish_bmc_http::reqwest::Client;
    use nv_redfish_bmc_http::reqwest::ClientParams;
    use nv_redfish_bmc_http::BmcCredentials;
    use nv_redfish_bmc_http::CacheSettings;
    use nv_redfish_bmc_http::HttpBmc;
    use nv_redfish_core::{action::Action, EntityTypeRef, Expandable, ODataETag, ODataId};
    use serde::{Deserialize, Serialize};
    use url::Url;
    use wiremock::MockServer;

    /// Test resource struct used across integration tests
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct TestResource {
        #[serde(rename = "@odata.id")]
        pub id: ODataId,
        #[serde(rename = "@odata.etag")]
        pub etag: Option<ODataETag>,
        pub name: String,
        pub value: i32,
    }

    impl EntityTypeRef for TestResource {
        /// Returns the resource `@odata.id`.
        fn odata_id(&self) -> &ODataId {
            &self.id
        }

        /// Returns the resource `@odata.etag` when present.
        fn etag(&self) -> Option<&ODataETag> {
            self.etag.as_ref()
        }
    }

    impl Expandable for TestResource {}

    /// Fake resources for test HTTP requests
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct CreateRequest {
        pub name: String,
        pub value: i32,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct UpdateRequest {
        pub name: Option<String>,
        pub value: Option<i32>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct ActionRequest {
        pub parameter: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct ActionResponse {
        pub result: String,
        pub success: bool,
    }

    /// Creates an [`ODataId`] from a string slice.
    pub fn create_odata_id(s: &str) -> ODataId {
        ODataId::from(s.to_string())
    }

    /// Creates an [`ODataETag`] from a string slice.
    pub fn create_odata_etag(s: &str) -> ODataETag {
        ODataETag::from(s.to_string())
    }

    /// Creates a test [`Action`] targeting `target`.
    pub fn create_test_action(target: &str) -> Action<ActionRequest, ActionResponse> {
        let json = format!(r#"{{"target": "{}"}}"#, target);
        serde_json::from_str(&json).unwrap()
    }

    /// Creates a [`TestResource`] with the given `path`, optional `etag`, `name`, and `value`.
    pub fn create_test_resource(
        path: &str,
        etag: Option<&str>,
        name: &str,
        value: i32,
    ) -> TestResource {
        TestResource {
            id: create_odata_id(path),
            etag: etag.map(create_odata_etag),
            name: name.to_string(),
            value,
        }
    }

    /// Creates test BMC credentials with a default username and password.
    pub fn create_test_credentials() -> BmcCredentials {
        BmcCredentials::username_password("root".to_string(), Some("password".to_string()))
    }

    /// Builds a test [`Client`] from `params`, installing the test
    /// [`rustls::crypto::CryptoProvider`] first when needed.
    pub fn test_client(params: ClientParams) -> Client {
        install_test_crypto_provider();
        Client::with_params(params).unwrap()
    }

    /// Creates a test [`HttpBmc`] connected to `mock_server`.
    pub fn create_test_bmc(mock_server: &MockServer) -> HttpBmc<Client> {
        let client = test_client(ClientParams::new());
        let credentials = create_test_credentials();
        HttpBmc::new(
            client,
            Url::parse(&mock_server.uri()).unwrap(),
            credentials,
            CacheSettings::default(),
        )
    }

    /// Creates a test [`HttpBmc`] connected to `mock_server` with explicit `credentials`.
    pub fn create_test_bmc_with_credentials(
        mock_server: &MockServer,
        credentials: BmcCredentials,
    ) -> HttpBmc<Client> {
        let client = test_client(ClientParams::new());
        HttpBmc::new(
            client,
            Url::parse(&mock_server.uri()).unwrap(),
            credentials,
            CacheSettings::default(),
        )
    }

    /// Creates a test [`HttpBmc`] connected to `mock_server` with `custom_headers`.
    pub fn create_test_bmc_with_custom_headers(
        mock_server: &MockServer,
        custom_headers: http::HeaderMap,
    ) -> HttpBmc<Client> {
        let client = test_client(ClientParams::new());
        let credentials = create_test_credentials();
        HttpBmc::with_custom_headers(
            client,
            Url::parse(&mock_server.uri()).unwrap(),
            credentials,
            CacheSettings::default(),
            custom_headers,
        )
    }

    /// Installs a minimal process-default [`rustls::crypto::CryptoProvider`]
    /// when `reqwest-rustls-no-provider` is tested without `reqwest`'s
    /// `aws-lc-rs` provider. All integration tests connect to plain-HTTP mock
    /// servers, so the provider only needs to satisfy
    /// `reqwest::ClientBuilder::build`.
    #[cfg(all(feature = "reqwest-rustls-no-provider", not(feature = "reqwest")))]
    fn install_test_crypto_provider() {
        use std::sync::Arc;

        use rustls::crypto::cipher::AeadKey;
        use rustls::crypto::cipher::Iv;
        use rustls::crypto::cipher::MessageDecrypter;
        use rustls::crypto::cipher::MessageEncrypter;
        use rustls::crypto::cipher::Tls13AeadAlgorithm;
        use rustls::crypto::cipher::UnsupportedOperationError;
        use rustls::crypto::hash::Context as HashContext;
        use rustls::crypto::hash::Hash;
        use rustls::crypto::hash::HashAlgorithm;
        use rustls::crypto::hash::Output as HashOutput;
        use rustls::crypto::hmac::Hmac;
        use rustls::crypto::hmac::Key as HmacKey;
        use rustls::crypto::hmac::Tag as HmacTag;
        use rustls::crypto::tls13::HkdfUsingHmac;
        use rustls::crypto::ActiveKeyExchange;
        use rustls::crypto::CipherSuiteCommon;
        use rustls::crypto::CryptoProvider;
        use rustls::crypto::GetRandomFailed;
        use rustls::crypto::KeyProvider;
        use rustls::crypto::SecureRandom;
        use rustls::crypto::SupportedKxGroup;
        use rustls::crypto::WebPkiSupportedAlgorithms;
        use rustls::pki_types::PrivateKeyDer;
        use rustls::sign::SigningKey;
        use rustls::CipherSuite;
        use rustls::ConnectionTrafficSecrets;
        use rustls::Error as RustlsError;
        use rustls::NamedGroup;
        use rustls::SupportedCipherSuite;
        use rustls::Tls13CipherSuite;

        #[derive(Debug)]
        struct TestCrypto;

        impl SecureRandom for TestCrypto {
            /// Returns `Err(GetRandomFailed)` because TLS handshakes do not run in plain-HTTP tests.
            fn fill(&self, _buf: &mut [u8]) -> Result<(), GetRandomFailed> {
                Err(GetRandomFailed)
            }
        }

        impl KeyProvider for TestCrypto {
            /// Returns an error because client private keys are unused in plain-HTTP tests.
            fn load_private_key(
                &self,
                _key_der: PrivateKeyDer<'static>,
            ) -> Result<Arc<dyn SigningKey>, RustlsError> {
                Err(RustlsError::General(
                    "unused in plain-HTTP tests".to_string(),
                ))
            }
        }

        impl SupportedKxGroup for TestCrypto {
            /// Returns an error because TLS key exchange is unused in plain-HTTP tests.
            fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, RustlsError> {
                Err(RustlsError::General(
                    "unused in plain-HTTP tests".to_string(),
                ))
            }

            /// Advertises `X25519` so `ClientConfig` protocol-version validation succeeds.
            fn name(&self) -> NamedGroup {
                NamedGroup::X25519
            }
        }

        impl Hash for TestCrypto {
            /// Starts a stub hash context.
            fn start(&self) -> Box<dyn HashContext> {
                Box::new(Self)
            }

            /// Returns a zeroed 32-byte digest.
            fn hash(&self, _data: &[u8]) -> HashOutput {
                HashOutput::new(&[0u8; 32])
            }

            /// Returns the SHA-256 output length in bytes.
            fn output_len(&self) -> usize {
                32
            }

            /// Identifies the hash algorithm as `SHA256`.
            fn algorithm(&self) -> HashAlgorithm {
                HashAlgorithm::SHA256
            }
        }

        impl HashContext for TestCrypto {
            /// Returns a zeroed 32-byte digest without consuming the context.
            fn fork_finish(&self) -> HashOutput {
                HashOutput::new(&[0u8; 32])
            }

            /// Forks the stub hash context.
            fn fork(&self) -> Box<dyn HashContext> {
                Box::new(Self)
            }

            /// Finishes the stub hash context with a zeroed 32-byte digest.
            fn finish(self: Box<Self>) -> HashOutput {
                HashOutput::new(&[0u8; 32])
            }

            /// Ignores input bytes in the stub hash context.
            fn update(&mut self, _data: &[u8]) {}
        }

        impl Hmac for TestCrypto {
            /// Creates a stub HMAC key.
            fn with_key(&self, _key: &[u8]) -> Box<dyn HmacKey> {
                Box::new(Self)
            }

            /// Returns the SHA-256 hash output length in bytes.
            fn hash_output_len(&self) -> usize {
                32
            }
        }

        impl HmacKey for TestCrypto {
            /// Returns a zeroed 32-byte HMAC tag.
            fn sign_concat(&self, _first: &[u8], _middle: &[&[u8]], _last: &[u8]) -> HmacTag {
                HmacTag::new(&[0u8; 32])
            }

            /// Returns the SHA-256 HMAC tag length in bytes.
            fn tag_len(&self) -> usize {
                32
            }
        }

        impl Tls13AeadAlgorithm for TestCrypto {
            /// Unreachable in plain-HTTP integration tests.
            fn encrypter(&self, _key: AeadKey, _iv: Iv) -> Box<dyn MessageEncrypter> {
                unreachable!("TLS encryption is unused in plain-HTTP tests")
            }

            /// Unreachable in plain-HTTP integration tests.
            fn decrypter(&self, _key: AeadKey, _iv: Iv) -> Box<dyn MessageDecrypter> {
                unreachable!("TLS decryption is unused in plain-HTTP tests")
            }

            /// Returns the AES-128-GCM key length in bytes.
            fn key_len(&self) -> usize {
                16
            }

            /// Returns `Err(UnsupportedOperationError)` for kernel TLS key extraction.
            fn extract_keys(
                &self,
                _key: AeadKey,
                _iv: Iv,
            ) -> Result<ConnectionTrafficSecrets, UnsupportedOperationError> {
                Err(UnsupportedOperationError)
            }
        }

        static TEST_HKDF: HkdfUsingHmac<'static> = HkdfUsingHmac(&TestCrypto);

        static TEST_TLS13_SUITE: Tls13CipherSuite = Tls13CipherSuite {
            common: CipherSuiteCommon {
                suite: CipherSuite::TLS13_AES_128_GCM_SHA256,
                hash_provider: &TestCrypto,
                confidentiality_limit: u64::MAX,
            },
            hkdf_provider: &TEST_HKDF,
            aead_alg: &TestCrypto,
            quic: None,
        };

        if CryptoProvider::get_default().is_none() {
            let _ = CryptoProvider {
                cipher_suites: vec![SupportedCipherSuite::Tls13(&TEST_TLS13_SUITE)],
                kx_groups: vec![&TestCrypto],
                signature_verification_algorithms: WebPkiSupportedAlgorithms {
                    all: &[],
                    mapping: &[],
                },
                secure_random: &TestCrypto,
                key_provider: &TestCrypto,
            }
            .install_default();
        }
    }

    /// No-op when a TLS provider is already configured by the active feature set.
    #[cfg(not(all(feature = "reqwest-rustls-no-provider", not(feature = "reqwest"))))]
    const fn install_test_crypto_provider() {}

    pub mod names {
        pub const TEST_CHASSIS: &str = "Test Chassis";
        pub const TEST_SYSTEM: &str = "Test System";
        pub const TEST_MANAGER: &str = "Test Manager";
    }

    pub mod paths {
        pub const CHASSIS_1: &str = "/redfish/v1/Chassis/1";
        pub const MANAGERS_1: &str = "/redfish/v1/Managers/1";
        pub const SYSTEMS_1: &str = "/redfish/v1/Systems/1";
        pub const NONEXISTENT: &str = "/redfish/v1/nonexistent";
    }
}
