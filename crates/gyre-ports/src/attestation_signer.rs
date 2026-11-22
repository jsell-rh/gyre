//! Pluggable signing of merge attestation bundle components
//! (agent-gates.md §Part 3 "What Gets Signed and By Whom").

use anyhow::Result;
use async_trait::async_trait;

/// A signing party for merge attestation components.
///
/// Each party the spec's "What Gets Signed and By Whom" table names — the
/// forge (TestCommand/LintCommand results and the bundle itself) and each
/// gate agent (its AgentReview result) — implements this trait with its
/// own key material. The initial implementation signs with Ed25519
/// (server-held keys); the trait is the seam where Sigstore/OIDC
/// identities plug in later without changing the bundle format: every
/// signer covers the same canonical payload bytes regardless of the
/// underlying key scheme.
#[async_trait]
#[allow(clippy::double_must_use)] // async-trait 0.1.89 injects #[must_use] into every
                                  // expanded method while the boxed-future return type is
                                  // already #[must_use] (Future); upstream-suppressed set
                                  // (lint_suppress_without_body) does not cover this lint.
pub trait AttestationSigner: Send + Sync {
    /// Sign `payload` (canonical bundle/gate JSON bytes) and return the
    /// signature as a base64-encoded string.
    async fn sign(&self, payload: &[u8]) -> Result<String>;

    /// The signer's identity as recorded in the bundle's `signed_by`
    /// field: "forge" for the forge, "agent:<id>" for a gate agent.
    fn signer_identity(&self) -> String;
}
