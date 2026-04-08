pub mod attestation;
pub mod completion;
pub mod conversation;
pub mod error;
pub mod gate;
pub mod graph;
pub mod id;
pub mod key_binding;
pub mod message;
pub mod notification;
pub mod protocol;
pub mod trace;
pub mod view_query;
pub mod view_spec;

pub use attestation::{
    Attestation, AttestationInput, AttestationMetadata, AttestationOutput, DerivedInput,
    GateAttestation, GateConstraint, InputContent, OutputConstraint, PersonaRef,
    ScopeConstraint, SignedInput, TrustAnchor, TrustAnchorType, VerificationResult,
};
pub use completion::{AgentCompletionSummary, Decision};
pub use conversation::{ConversationProvenance, TurnCommitLink};
pub use error::GyreError;
pub use gate::{GateStatus, GateType};
pub use graph::{EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence, Visibility};
pub use id::Id;
pub use key_binding::KeyBinding;
pub use message::{Destination, Message, MessageKind, MessageOrigin, MessageTier, TelemetryBuffer};
pub use notification::{Notification, NotificationType};
pub use protocol::{ActivityEventData, AgEventType, SubscribeScope, WsMessage};
pub use trace::{GateTrace, SpanKind, SpanStatus, TraceSpan};
