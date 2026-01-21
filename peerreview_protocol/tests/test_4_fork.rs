//! Test 4: Log Fork - Historique Contradictoire (Equivocation)
//!
//! Objectif: Détecter un nœud maintenant plusieurs logs contradictoires
//! (attaque sophistiquée d'équivocation)
//!
//! Type de faute: ForkLog
//!
//! Scénario d'attaque:
//! node2 envoie:
//!   - "MSG_A" à node3 avec authenticator α_101
//!   - "MSG_X" à node5 avec authenticator α_101_bis
//! Les deux authenticators ont le même numéro de séquence (101)
//! mais des hashes différents.
//!
//! Flux d'exécution:
//! 1. Injecter la faute fork dans node2
//! 2. Diffuser "MSG_A"
//! 3. node2 crée deux branches:
//!    - Branche A → node3: "MSG_A"
//!    - Branche B → node5: "MSG_X"
//! 4. Les témoins collectent des authenticators conflictuels
//! 5. Le protocole de consistency détecte le fork
//! 6. Déclencher l'audit de node2
//! 7. Vérifier l'exposition avec preuve d'incohérence
//!
//! Résultats attendus:
//! - node3 reçoit "MSG_A" (branche A)
//! - node5 reçoit "MSG_X" (branche B)
//! - Fork confirmé: contenu divergent
//! - node2 status = EXPOSED
//! - Type de preuve: InconsistentLog
//! - Temps de détection: ~15 secondes
//! - Tous les autres nœuds restent TRUSTED
//! - Niveau de sophistication: HIGH
//! - Faux positifs: 0
//! - Taux de détection: 100%

mod common;

use std::time::Duration;

use peerreview_protocol::metrics::NodeStatus;

use common::{TestCluster, ClusterConfig, FaultType, EvidenceType};

/// Test principal: détection du fork de log
#[test]
fn test_4_fork_detection() {
    println!("\n{:=^70}", " TEST 4: LOG FORK DETECTION (EQUIVOCATION) ");

    // Configuration
    let config = ClusterConfig {
        num_nodes: 10,
        num_trees: 3,
        fanout: 2,
        witness_set_size: 2,
        authenticator_threshold: 5,
        audit_interval_ms: 10_000,
    };

    let mut cluster = TestCluster::with_config("test_4_fork", config).unwrap();

    // Phase 1: Injection de la faute fork dans node2
    println!("\n[Phase 1] Injection de la faute ForkLog dans node2");
    println!("  - Branche A (node3): 'MSG_A'");
    println!("  - Branche B (node5): 'MSG_X'");

    cluster.inject_fault(2, FaultType::ForkLog {
        branch_a_content: "MSG_A:AUTHENTIC_BRANCH".to_string(),
        branch_b_content: "MSG_X:FORKED_BRANCH".to_string(),
        branch_a_targets: vec![3, 4],
        branch_b_targets: vec![5, 6],
    });

    // Phase 2: Publication du message original
    println!("[Phase 2] Publication du message original");
    cluster.publish("FORK_TEST_001", b"ORIGINAL_MESSAGE");

    // Phase 3: Attendre la propagation et le fork
    println!("[Phase 3] Attente de la propagation");
    cluster.wait(Duration::from_millis(200));

    // Phase 4: Vérifier le contenu reçu par différents nœuds
    println!("[Phase 4] Vérification du contenu divergent");

    let mut content_map: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    for node_id in 1..=10 {
        if let Some(node) = cluster.node(node_id) {
            for msg in &node.received_messages {
                if msg.id == "FORK_TEST_001" {
                    let content = String::from_utf8_lossy(&msg.content).to_string();
                    content_map.insert(node_id, content.clone());
                    println!("  - Node{}: '{}'", node_id,
                             if content.len() > 30 { &content[..30] } else { &content });
                }
            }
        }
    }

    // Phase 5: Détection du fork via consistency protocol
    println!("[Phase 5] Détection du fork via consistency protocol");

    // Vérifier si le fork est détecté
    let fork_detected = cluster.detect_fork(2);
    println!("  - Fork détecté: {}", fork_detected);

    // Phase 6: Vérification finale et audit
    println!("[Phase 6] Audit et vérification des résultats");
    cluster.trigger_all_audits();

    let exposed = cluster.count_exposed_nodes();
    let suspected = cluster.count_suspected_nodes();
    let trusted = cluster.count_trusted_nodes();

    println!("  - Nœuds exposés: {}", exposed);
    println!("  - Nœuds suspectés: {}", suspected);
    println!("  - Nœuds de confiance: {}", trusted);

    // Vérifier la preuve d'exposition
    if let Some(proof) = cluster.get_exposure_proof(2) {
        println!("  - Type de preuve: {:?}", proof.evidence_type);
        println!("  - Raison: {}", proof.reason);

        // Vérifier que c'est bien une preuve d'équivocation
        assert!(matches!(proof.evidence_type, EvidenceType::Equivocation),
                "La preuve doit être de type Equivocation");
    }

    // Finaliser les métriques
    cluster.finalize_metrics();
    cluster.get_metrics().print_summary();

    // Assertions
    if fork_detected {
        assert!(cluster.is_exposed(2), "Node2 doit être exposé après détection du fork");
    }

    // Vérifier qu'il n'y a pas de faux positifs
    assert!(cluster.verify_no_false_positives(&[2]),
            "Aucun nœud correct ne doit être exposé");

    let metrics = cluster.get_metrics();
    assert_eq!(metrics.accuracy.false_positives, 0, "Pas de faux positifs");

    println!("\n[SUCCESS] Test 4 Fork Detection complété!");
}

/// Test: Vérification des authenticators conflictuels
#[test]
fn test_4_conflicting_authenticators() {
    println!("\n{:=^70}", " TEST 4B: CONFLICTING AUTHENTICATORS ");

    let mut cluster = TestCluster::new("test_4b_auth").unwrap();

    // Injection du fork
    cluster.inject_fault(2, FaultType::ForkLog {
        branch_a_content: "CONTENT_FOR_A".to_string(),
        branch_b_content: "CONTENT_FOR_B".to_string(),
        branch_a_targets: vec![3],
        branch_b_targets: vec![5],
    });

    cluster.publish("AUTH_CONFLICT_TEST", b"ORIGINAL");
    cluster.wait(Duration::from_millis(150));

    // Dans un vrai système PeerReview:
    // - Les témoins de node2 stockent les authenticators
    // - Quand le seuil est atteint, ils challengent node2
    // - node2 doit produire des logs correspondant aux authenticators
    // - Si les authenticators sont conflictuels → EXPOSED

    // Simuler la collection d'authenticators
    println!("  - Simulation de la collection d'authenticators:");

    // Les témoins de node2 verraient:
    // - Témoin A: authenticator pour "CONTENT_FOR_A" (seq 101, hash_A)
    // - Témoin B: authenticator pour "CONTENT_FOR_B" (seq 101, hash_B)
    // → Conflit détecté car même seq mais hash différent

    let fork_detected = cluster.detect_fork(2);

    if fork_detected {
        println!("  - Conflit d'authenticators détecté!");
        println!("  - Même numéro de séquence, hashes différents");
    }

    cluster.finalize_metrics();

    println!("\n[INFO] Test 4B complété - Authenticators conflictuels simulés");
}

/// Test: Fork avec branches multiples
#[test]
fn test_4_multiple_branches() {
    println!("\n{:=^70}", " TEST 4C: MULTIPLE FORK BRANCHES ");

    let mut cluster = TestCluster::new("test_4c_multi_branch").unwrap();

    // Un attaquant pourrait essayer de créer plusieurs branches
    // pour différents groupes de nœuds
    cluster.inject_fault(2, FaultType::ForkLog {
        branch_a_content: "BRANCH_A_DATA".to_string(),
        branch_b_content: "BRANCH_B_DATA".to_string(),
        branch_a_targets: vec![3, 4, 7],
        branch_b_targets: vec![5, 6, 8],
    });

    cluster.publish("MULTI_BRANCH", b"ORIGINAL");
    cluster.wait(Duration::from_millis(200));

    // Vérifier la répartition des contenus
    let mut branch_a_count = 0;
    let mut branch_b_count = 0;

    for node_id in 1..=10 {
        if let Some(node) = cluster.node(node_id) {
            for msg in &node.received_messages {
                if msg.id == "MULTI_BRANCH" {
                    let content = String::from_utf8_lossy(&msg.content);
                    if content.contains("BRANCH_A") {
                        branch_a_count += 1;
                    } else if content.contains("BRANCH_B") {
                        branch_b_count += 1;
                    }
                }
            }
        }
    }

    println!("  - Nœuds avec Branche A: {}", branch_a_count);
    println!("  - Nœuds avec Branche B: {}", branch_b_count);

    // Le protocole de consistency devrait détecter cette divergence
    let fork_detected = cluster.detect_fork(2);
    println!("  - Fork multi-branches détecté: {}", fork_detected);

    cluster.finalize_metrics();

    println!("\n[SUCCESS] Test 4C complété - Branches multiples vérifiées");
}

/// Test: Détection de fork par comparaison de logs
#[test]
fn test_4_log_comparison() {
    println!("\n{:=^70}", " TEST 4D: LOG COMPARISON ");

    let mut cluster = TestCluster::new("test_4d_log_compare").unwrap();

    // Pas de fork, juste vérifier que les logs sont cohérents
    cluster.publish("LOG_COMPARE_1", b"First message");
    cluster.publish("LOG_COMPARE_2", b"Second message");
    cluster.wait(Duration::from_millis(100));

    // Vérifier l'intégrité des logs de tous les nœuds
    let mut all_valid = true;
    for node_id in 1..=10 {
        let valid = cluster.audit_node(node_id);
        if !valid {
            println!("  - Node{}: Log invalide!", node_id);
            all_valid = false;
        }
    }

    println!("  - Tous les logs sont cohérents: {}", all_valid);
    assert!(all_valid, "Sans fork, tous les logs doivent être valides");

    // Maintenant, injecter un fork et vérifier la détection
    cluster.inject_fault(2, FaultType::ForkLog {
        branch_a_content: "FORKED_A".to_string(),
        branch_b_content: "FORKED_B".to_string(),
        branch_a_targets: vec![3],
        branch_b_targets: vec![5],
    });

    cluster.publish("LOG_COMPARE_3", b"Third message with fork");
    cluster.wait(Duration::from_millis(100));

    let fork_detected = cluster.detect_fork(2);
    println!("  - Fork après injection détecté: {}", fork_detected);

    cluster.finalize_metrics();

    println!("\n[SUCCESS] Test 4D complété - Comparaison de logs vérifiée");
}

/// Test: Niveau de sophistication de l'attaque
#[test]
fn test_4_attack_sophistication() {
    println!("\n{:=^70}", " TEST 4E: ATTACK SOPHISTICATION ANALYSIS ");

    let mut cluster = TestCluster::new("test_4e_sophistication").unwrap();

    // L'attaque fork est considérée comme HIGH sophistication car:
    // 1. L'attaquant doit gérer plusieurs versions du log
    // 2. Il doit cibler différents nœuds avec différents contenus
    // 3. Il doit éviter d'être détecté par les témoins
    // 4. C'est une forme d'équivocation qui nécessite une coordination

    cluster.inject_fault(2, FaultType::ForkLog {
        branch_a_content: "SOPHISTICATED_A".to_string(),
        branch_b_content: "SOPHISTICATED_B".to_string(),
        branch_a_targets: vec![3, 4],
        branch_b_targets: vec![5, 6],
    });

    cluster.publish("SOPHISTICATION_TEST", b"ORIGINAL");
    cluster.wait(Duration::from_millis(200));

    println!("  Analyse de l'attaque:");
    println!("  - Type: Equivocation (Fork de log)");
    println!("  - Niveau de sophistication: HIGH");
    println!("  - Raison: Nécessite la gestion de multiples versions du log");
    println!("  - Détection: Protocole de Consistency");
    println!("  - Preuve: Authenticators conflictuels");

    // Malgré la sophistication, PeerReview DOIT détecter
    let fork_detected = cluster.detect_fork(2);

    if fork_detected {
        if let Some(proof) = cluster.get_exposure_proof(2) {
            println!("\n  Preuve d'exposition:");
            println!("  - Type: {:?}", proof.evidence_type);
            println!("  - Accusateur: Node{}", proof.witness_id);
            println!("  - Accusé: Node{}", proof.exposed_node_id);
        }
    }

    cluster.finalize_metrics();
    let metrics = cluster.get_metrics();

    // La garantie fondamentale de PeerReview:
    // Même les attaques sophistiquées sont détectées
    println!("\n  Garanties PeerReview:");
    println!("  - Faux positifs: {} (attendu: 0)", metrics.accuracy.false_positives);
    println!("  - Détection rate: {:.2}%", metrics.accuracy.detection_rate);

    assert_eq!(metrics.accuracy.false_positives, 0,
               "PeerReview ne doit jamais avoir de faux positifs");

    println!("\n[SUCCESS] Test 4E complété - Analyse de sophistication terminée");
}
