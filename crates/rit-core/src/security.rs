//! Pure security-domain models and policy decisions. Nothing here authenticates a caller.
//! Persistence, credential issuance, and application-level enforcement are not implemented.
use crate::repository::RepositoryId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str::FromStr};

/// Random, opaque principal identifier. Display names must not be used as identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrincipalId(uuid::Uuid);
impl PrincipalId {
    /// Generate an opaque random identifier.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
impl Default for PrincipalId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for PrincipalId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s).map(Self)
    }
}

/// Principal category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalKind {
    /// Human user.
    Human,
    /// AI agent.
    Agent,
}
/// Lifecycle state. Revocation is terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalState {
    /// Can be considered for authorization.
    Active,
    /// Temporarily disabled.
    Suspended,
    /// Permanently disabled.
    Revoked,
}
/// Pure principal record, not an authenticated actor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// Principal ID.
    pub id: PrincipalId,
    /// Human-readable name, not authority.
    pub display_name: String,
    /// Kind.
    pub kind: PrincipalKind,
    /// Lifecycle state.
    pub state: PrincipalState,
}
impl Principal {
    /// Return a new validated principal.
    pub fn new(name: &str, kind: PrincipalKind) -> Result<Self, &'static str> {
        if name.trim().is_empty() {
            return Err("empty principal name");
        }
        Ok(Self {
            id: PrincipalId::new(),
            display_name: name.into(),
            kind,
            state: PrincipalState::Active,
        })
    }
    /// Apply a permitted state transition; revocation cannot be reversed.
    pub fn transition(&mut self, next: PrincipalState) -> Result<(), &'static str> {
        if self.state == PrincipalState::Revoked && next != PrincipalState::Revoked {
            return Err("revoked principal cannot be reactivated");
        }
        self.state = next;
        Ok(())
    }
}
/// An authenticated actor handle. No public constructor is provided while authentication is unavailable.
#[derive(Debug)]
pub struct ActorContext {
    principal: PrincipalId,
}
impl ActorContext {
    /// Authenticated principal identity, never a credential.
    pub fn principal(&self) -> PrincipalId {
        self.principal
    }
}
/// Typed capability rather than arbitrary string permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Read repository records.
    RepositoryRead,
    /// Register repository metadata.
    RepositoryRegister,
    /// Manage principals.
    IdentityManage,
    /// Modify grants.
    PermissionManage,
    /// Read security audit.
    AuditRead,
    /// Create workspace.
    WorkspaceCreate,
    /// Execute an agent.
    AgentExecute,
    /// Create checkpoint.
    CheckpointCreate,
    /// Restore content.
    CheckpointRestore,
    /// Submit integration.
    IntegrationSubmit,
    /// Approve publication.
    IntegrationApprove,
    /// Publish target ref.
    IntegrationPublish,
}
/// Predefined role identifiers. Role decisions are advisory until an authenticated service enforces them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Full administration.
    Owner,
    /// Administrative operations.
    Administrator,
    /// Developer.
    Developer,
    /// Reviewer.
    Reviewer,
    /// Agent.
    Agent,
    /// Read-only user.
    ReadOnly,
}
impl Role {
    /// Whether this role includes the named capability.
    pub fn includes(self, c: Capability) -> bool {
        use Capability::*;
        match self {
            Self::Owner => true,
            Self::Administrator => !matches!(c, IntegrationPublish),
            Self::Developer => matches!(
                c,
                RepositoryRead
                    | RepositoryRegister
                    | WorkspaceCreate
                    | CheckpointCreate
                    | IntegrationSubmit
            ),
            Self::Reviewer => matches!(c, RepositoryRead | AuditRead | IntegrationApprove),
            Self::Agent => matches!(
                c,
                RepositoryRead
                    | WorkspaceCreate
                    | AgentExecute
                    | CheckpointCreate
                    | IntegrationSubmit
            ),
            Self::ReadOnly => matches!(c, RepositoryRead),
        }
    }
}
/// Permission scope. Global applies to any repository; a repository grant never applies globally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    /// Global scope.
    Global,
    /// One repository.
    Repository(RepositoryId),
}
/// Explicit allow/deny effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// Permit if no applicable denial exists.
    Allow,
    /// Deny even if another grant allows.
    Deny,
}
/// A scope-aware role assignment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// Assigned principal.
    pub subject: PrincipalId,
    /// Role.
    pub role: Role,
    /// Scope.
    pub scope: Scope,
    /// Effect.
    pub effect: Effect,
}
/// Policy outcome. This does not authenticate the principal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Explicitly permitted.
    Allow,
    /// Explicit deny, inactive subject or default deny.
    Deny,
}
/// Evaluate a principal's assignments. Deny overrides all applicable allows; default is deny.
pub fn evaluate(
    subject: &Principal,
    cap: Capability,
    scope: Scope,
    assignments: &[Assignment],
) -> Decision {
    if subject.state != PrincipalState::Active {
        return Decision::Deny;
    }
    let applicable = |a: &&Assignment| {
        a.subject == subject.id
            && a.role.includes(cap)
            && (a.scope == Scope::Global || a.scope == scope)
    };
    if assignments
        .iter()
        .filter(applicable)
        .any(|a| a.effect == Effect::Deny)
    {
        return Decision::Deny;
    }
    if assignments
        .iter()
        .filter(applicable)
        .any(|a| a.effect == Effect::Allow)
    {
        Decision::Allow
    } else {
        Decision::Deny
    }
}
/// Fixed allowlisted security event kind. Never put credentials or free-form messages here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityEventKind {
    /// A principal lifecycle changed.
    PrincipalStateChanged,
    /// A policy decision was made.
    AuthorizationDecision,
    /// Credential metadata changed.
    CredentialLifecycle,
}
/// Versioned audit entry in a deterministic hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Sequence, starting at one.
    pub sequence: u64,
    /// Unix timestamp supplied by the producer.
    pub timestamp: u64,
    /// Actor identifier.
    pub actor: PrincipalId,
    /// Allowlisted event kind.
    pub kind: SecurityEventKind,
    /// Link to previous hash (zero for genesis).
    pub previous: [u8; 32],
    /// SHA-256 over the v1 canonical entry.
    pub hash: [u8; 32],
}
fn event_code(kind: SecurityEventKind) -> u8 {
    match kind {
        SecurityEventKind::PrincipalStateChanged => 1,
        SecurityEventKind::AuthorizationDecision => 2,
        SecurityEventKind::CredentialLifecycle => 3,
    }
}
fn digest(
    sequence: u64,
    timestamp: u64,
    actor: PrincipalId,
    kind: SecurityEventKind,
    previous: [u8; 32],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"RIT-AUDIT-V1\0");
    h.update(sequence.to_be_bytes());
    h.update(timestamp.to_be_bytes());
    h.update(actor.0.as_bytes());
    h.update([event_code(kind)]);
    h.update(previous);
    h.finalize().into()
}
/// Construct the next audit entry. This is not durable storage and is not externally anchored.
pub fn next_audit(
    previous: Option<&AuditEntry>,
    timestamp: u64,
    actor: PrincipalId,
    kind: SecurityEventKind,
) -> Option<AuditEntry> {
    let sequence = previous.map_or(Some(1), |p| p.sequence.checked_add(1))?;
    let link = previous.map_or([0; 32], |p| p.hash);
    Some(AuditEntry {
        sequence,
        timestamp,
        actor,
        kind,
        previous: link,
        hash: digest(sequence, timestamp, actor, kind, link),
    })
}
/// Verify sequence, linkage, and checksums; a privileged attacker can rewrite an entire unanchored chain.
pub fn verify_audit(entries: &[AuditEntry]) -> bool {
    let mut prior = [0; 32];
    for (i, e) in entries.iter().enumerate() {
        let Ok(n) = u64::try_from(i + 1) else {
            return false;
        };
        if e.sequence != n
            || e.previous != prior
            || e.hash != digest(e.sequence, e.timestamp, e.actor, e.kind, e.previous)
        {
            return false;
        }
        prior = e.hash;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_is_default_deny_and_deny_wins() {
        let p = Principal::new("Alice", PrincipalKind::Human).unwrap();
        let repo = RepositoryId::new();
        let other = RepositoryId::new();
        let allow = Assignment {
            subject: p.id,
            role: Role::Developer,
            scope: Scope::Repository(repo),
            effect: Effect::Allow,
        };
        assert_eq!(
            evaluate(&p, Capability::RepositoryRead, Scope::Repository(repo), &[]),
            Decision::Deny
        );
        assert_eq!(
            evaluate(
                &p,
                Capability::RepositoryRead,
                Scope::Repository(other),
                std::slice::from_ref(&allow)
            ),
            Decision::Deny
        );
        assert_eq!(
            evaluate(
                &p,
                Capability::RepositoryRead,
                Scope::Global,
                std::slice::from_ref(&allow)
            ),
            Decision::Deny
        );
        let deny = Assignment {
            effect: Effect::Deny,
            scope: Scope::Global,
            ..allow.clone()
        };
        assert_eq!(
            evaluate(
                &p,
                Capability::RepositoryRead,
                Scope::Repository(repo),
                &[allow, deny]
            ),
            Decision::Deny
        );
    }
    #[test]
    fn lifecycle_is_terminal_and_inactive_denied() {
        let mut p = Principal::new("agent", PrincipalKind::Agent).unwrap();
        let a = Assignment {
            subject: p.id,
            role: Role::Owner,
            scope: Scope::Global,
            effect: Effect::Allow,
        };
        p.transition(PrincipalState::Suspended).unwrap();
        assert_eq!(
            evaluate(&p, Capability::IdentityManage, Scope::Global, &[a]),
            Decision::Deny
        );
        p.transition(PrincipalState::Revoked).unwrap();
        assert!(p.transition(PrincipalState::Active).is_err());
        assert!(Principal::new(" ", PrincipalKind::Human).is_err());
    }
    #[test]
    fn chain_detects_modification_reorder_and_truncation_of_middle() {
        let p = PrincipalId::new();
        let a = next_audit(None, 1, p, SecurityEventKind::CredentialLifecycle).unwrap();
        let b = next_audit(Some(&a), 2, p, SecurityEventKind::AuthorizationDecision).unwrap();
        assert!(verify_audit(&[a.clone(), b.clone()]));
        let mut changed = b.clone();
        changed.timestamp += 1;
        assert!(!verify_audit(&[a.clone(), changed]));
        assert!(!verify_audit(&[b.clone(), a]));
        assert!(!verify_audit(&[b]));
    }
}
