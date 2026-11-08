//! Security integration boundaries. There is deliberately no production provider yet.
use rit_core::security::{
    ActorContext, Assignment, AuditEntry, Capability, Principal, PrincipalId, Scope,
};
use thiserror::Error;

/// Missing capabilities are reported as errors, never simulated success.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapabilityError {
    /// The required storage foundation is not verified.
    #[error("operation unavailable: secure registry storage has not passed Phase 2.1 validation")]
    StorageUnverified,
    /// No production authentication implementation exists.
    #[error("operation unavailable: authentication provider is not implemented")]
    AuthenticationUnavailable,
    /// No verified OS isolation provider exists.
    #[error("operation unavailable: execution isolation is not verified")]
    IsolationUnavailable,
    /// Operation requires a later implementation or validation.
    #[error("operation unavailable: {0}")]
    Unavailable(&'static str),
}
/// Identity provider contract. No production implementation is registered.
pub trait IdentityProvider {
    /// Fetch a principal by stable ID after authentication and authorization.
    fn get(&self, actor: &ActorContext, id: PrincipalId) -> Result<Principal, CapabilityError>;
}
/// Credential metadata; raw secrets never appear here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialMetadata {
    /// Principal bound to the credential.
    pub principal: PrincipalId,
    /// Expiration in Unix seconds, if configured.
    pub expires_at: Option<u64>,
    /// Whether the credential has been revoked.
    pub revoked: bool,
}
/// Credential lifecycle contract; no persistence or issuance implementation is available.
pub trait CredentialStore {
    /// Inspect non-secret metadata after authorization.
    fn metadata(
        &self,
        actor: &ActorContext,
        id: &str,
    ) -> Result<CredentialMetadata, CapabilityError>;
    /// Revoke a credential after authorization.
    fn revoke(&mut self, actor: &ActorContext, id: &str) -> Result<(), CapabilityError>;
}
/// Central policy boundary. Evaluating policy does not authenticate a caller.
pub trait AuthorizationPolicy {
    /// Authorize an already authenticated actor for an operation and scope.
    fn authorize(
        &self,
        actor: &ActorContext,
        capability: Capability,
        scope: Scope,
    ) -> Result<(), CapabilityError>;
    /// Obtain applicable assignments after authorization.
    fn assignments(&self, actor: &ActorContext) -> Result<Vec<Assignment>, CapabilityError>;
}
/// Durable audit repository contract. No production implementation exists.
pub trait AuditRepository {
    /// Append an allowlisted entry in the same transaction as the corresponding mutation.
    fn append(&mut self, actor: &ActorContext, entry: &AuditEntry) -> Result<(), CapabilityError>;
}
/// Authentication contract. No production implementation exists.
pub trait AuthenticationProvider {
    /// Authenticate a credential file and return an actor without the credential.
    fn authenticate(
        &self,
        credential_file: &std::path::Path,
    ) -> Result<ActorContext, CapabilityError>;
}
/// Explicit fail-closed boundary for unavailable security-dependent operations.
pub fn require_secure_storage() -> Result<(), CapabilityError> {
    Err(CapabilityError::StorageUnverified)
}
