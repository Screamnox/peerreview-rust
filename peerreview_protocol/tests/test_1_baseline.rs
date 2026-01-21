//! Test 1: Baseline - Comportement Correct
//!
//! Objectif: Valider que le système fonctionne correctement sans fautes
//! et que PeerReview n'a pas de faux positifs.
//!
//! Configuration:
//! - 10 nœuds avec 3 arbres, fanout=2
//! - Taille du set de témoins: 2 (ψ=2)
//! - Intervalle d'audit: 10 secondes
//!
//! Résultats attendus:
//! - Tous les 10 nœuds reçoivent le message
//! - 0 nœuds exposés (pas de faux positifs)
//! - 0 nœuds suspectés
//! - Tous les nœuds status = TRUSTED

mod common;

use std::time::Duration;

use peerreview_protocol::metrics::NodeStatus;

use common::{TestCluster, ClusterConfig};

/// Test baseline: comportement correct sans fautes
#[test]
fn test_1_baseline_no_faults() {
    println!("\n{:=^70}", " TEST 1: BASELINE - NO FAULTS ");

    // Configuration du cluster
    let config = ClusterConfig {
        num_nodes: 10,
        num_trees: 3,
        fanout: 2,
        witness_set_size: 2,
        authenticator_threshold: 5,
        audit_interval_ms: 10_000,
    };

    let mut cluster = TestCluster::with_config("test_1_baseline", config).unwrap();

    // Phase 1: Publication d'un message
    println!("\n[Phase 1] Publication du message STREAM_CHUNK_001");
    let content = b"This is a test message for baseline validation - 18.7KB content simulation";
    cluster.publish("STREAM_CHUNK_001", content);

    // Phase 2: Attendre la propagation
    println!("[Phase 2] Attente de la propagation (100ms simulé)");
    cluster.wait(Duration::from_millis(100));

    // Phase 3: Déclencher les audits
    println!("[Phase 3] Déclenchement des audits sur tous les nœuds");
    cluster.trigger_all_audits();

    // Phase 4: Vérifier les résultats
    println!("[Phase 4] Vérification des résultats");

    // Compter les réceptions
    let receivers = cluster.count_receivers("STREAM_CHUNK_001");
    println!("  - Nœuds ayant reçu le message: {}/10", receivers);

    // Vérifier les statuts
    let exposed = cluster.count_exposed_nodes();
    let suspected = cluster.count_suspected_nodes();
    let trusted = cluster.count_trusted_nodes();

    println!("  - Nœuds exposés: {}", exposed);
    println!("  - Nœuds suspectés: {}", suspected);
    println!("  - Nœuds de confiance: {}", trusted);

    // Finaliser les métriques
    cluster.finalize_metrics();
    cluster.get_metrics().print_summary();

    // Assertions
    assert!(receivers >= 9, "Au moins 9 nœuds doivent recevoir le message");
    assert_eq!(exposed, 0, "Aucun nœud ne doit être exposé (pas de faux positifs)");
    assert_eq!(suspected, 0, "Aucun nœud ne doit être suspecté");
    assert_eq!(trusted, 10, "Tous les nœuds doivent être de confiance");

    // Vérifier les métriques
    let metrics = cluster.get_metrics();
    assert_eq!(metrics.accuracy.false_positives, 0, "Pas de faux positifs");
    assert!(metrics.content_delivery.avg_delivery_rate >= 90.0,
            "Taux de livraison doit être >= 90%");

    println!("\n[SUCCESS] Test 1 Baseline passé avec succès!");
}

/// Test baseline avec plusieurs messages
#[test]
fn test_1_baseline_multiple_messages() {
    println!("\n{:=^70}", " TEST 1B: BASELINE - MULTIPLE MESSAGES ");

    let mut cluster = TestCluster::new("test_1b_baseline_multi").unwrap();

    // Publier plusieurs messages
    for i in 1..=5 {
        let msg_id = format!("MSG_{:03}", i);
        let content = format!("Message content #{}", i);
        cluster.publish(&msg_id, content.as_bytes());
    }

    cluster.wait(Duration::from_millis(100));
    cluster.trigger_all_audits();

    // Vérifications
    let exposed = cluster.count_exposed_nodes();
    let trusted = cluster.count_trusted_nodes();

    assert_eq!(exposed, 0, "Aucun nœud exposé");
    assert_eq!(trusted, 10, "Tous les nœuds de confiance");

    cluster.finalize_metrics();
    let metrics = cluster.get_metrics();

    assert_eq!(metrics.content_delivery.total_chunks_sent, 5);
    assert_eq!(metrics.accuracy.false_positives, 0);

    println!("\n[SUCCESS] Test 1B passé avec succès!");
}

/// Test baseline: vérification des logs
#[test]
fn test_1_baseline_log_integrity() {
    println!("\n{:=^70}", " TEST 1C: BASELINE - LOG INTEGRITY ");

    let mut cluster = TestCluster::new("test_1c_log_integrity").unwrap();

    // Publier un message
    cluster.publish("INTEGRITY_TEST", b"Test log integrity");
    cluster.wait(Duration::from_millis(50));

    // Vérifier l'intégrité des logs de chaque nœud
    let mut all_valid = true;
    for node_id in 1..=10 {
        let valid = cluster.audit_node(node_id);
        if !valid {
            println!("  [FAIL] Node {} log invalid", node_id);
            all_valid = false;
        }
    }

    assert!(all_valid, "Tous les logs doivent être valides");

    let exposed = cluster.count_exposed_nodes();
    assert_eq!(exposed, 0, "Aucun nœud ne doit être exposé pour des logs valides");

    println!("\n[SUCCESS] Test 1C passé - Tous les logs sont intègres!");
}

/// Test baseline: vérification de la propagation dans les arbres
#[test]
fn test_1_baseline_tree_propagation() {
    println!("\n{:=^70}", " TEST 1D: BASELINE - TREE PROPAGATION ");

    let config = ClusterConfig {
        num_nodes: 10,
        num_trees: 3,
        fanout: 3, // fanout plus large
        ..Default::default()
    };

    let mut cluster = TestCluster::with_config("test_1d_trees", config).unwrap();

    cluster.publish("TREE_TEST", b"Testing multi-tree propagation");
    cluster.wait(Duration::from_millis(100));

    let receivers = cluster.count_receivers("TREE_TEST");
    println!("  - Réception via arbres multiples: {}/10 nœuds", receivers);

    // Avec un fanout de 3 et 3 arbres, la propagation doit être efficace
    assert!(receivers >= 8, "La propagation multi-arbres doit atteindre la majorité");

    cluster.finalize_metrics();
    let metrics = cluster.get_metrics();

    println!("  - Latence min: {} ms", metrics.propagation.min_latency.as_millis());
    println!("  - Latence max: {} ms", metrics.propagation.max_latency.as_millis());

    println!("\n[SUCCESS] Test 1D passé - Propagation multi-arbres fonctionne!");
}
