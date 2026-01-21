//! Infrastructure commune pour les tests d'intégration PeerReview
//!
//! Ce module fournit:
//! - TestCluster: Simulation d'un cluster de nœuds
//! - FaultInjector: Injection de fautes byzantines
//! - TestNode: Nœud de test avec journal et état PeerReview

pub mod cluster;
pub mod fault_injector;
pub mod test_node;

pub use cluster::{TestCluster, ClusterConfig, ExposureProof, EvidenceType, Challenge};
pub use fault_injector::{FaultType, FaultInjector};
pub use test_node::{TestNode, TestMessage, StoredAuthenticator};
