use peerreview_rust::journal::Logger;
use peerreview_rust::types::node::Node as PeerReviewNode;

use rand::rngs::OsRng;
use ed25519_dalek::Keypair;

fn main() -> std::io::Result<()> {
    println!("=== Démonstration du protocole PeerReview ===\n");
    println!("Scénario : 4 nœuds avec témoins, communication avec détection de faute\n");

    // === Configuration : 4 nœuds, chacun avec son témoin ===
    let mut witnesses_map = std::collections::HashMap::new();
    witnesses_map.insert(1, vec![3]); // Nœud 1 surveillé par témoin 3
    witnesses_map.insert(2, vec![4]); // Nœud 2 surveillé par témoin 4
    witnesses_map.insert(3, vec![1]); // Témoin 3 surveillé par nœud 1
    witnesses_map.insert(4, vec![2]); // Témoin 4 surveillé par nœud 2

    println!("=== Configuration ===");
    println!("Nœud 1 : témoin = {:?}", witnesses_map.get(&1).unwrap());
    println!("Nœud 2 : témoin = {:?}", witnesses_map.get(&2).unwrap());
    println!("Témoin 3 : témoin = {:?}", witnesses_map.get(&3).unwrap());
    println!("Témoin 4 : témoin = {:?}\n", witnesses_map.get(&4).unwrap());

    // Générer les keypairs pour chaque nœud
    use ed25519_dalek::Keypair;
    use rand::rngs::OsRng;
    let mut rng = OsRng;
    
    let keypair1 = Keypair::generate(&mut rng);
    let keypair2 = Keypair::generate(&mut rng);
    let keypair3 = Keypair::generate(&mut rng);
    let keypair4 = Keypair::generate(&mut rng);
    
    let node1_public_key = keypair1.public;
    let node2_public_key = keypair2.public;
    let node3_public_key = keypair3.public;
    let node4_public_key = keypair4.public;

    // Création des loggers
    let logger_node1 = Logger::new("node1_journal.log", 5000, 200)?;
    let logger_node2 = Logger::new("node2_journal.log", 5000, 200)?;
    let logger_node3 = Logger::new("node3_journal.log", 5000, 200)?;
    let logger_node4 = Logger::new("node4_journal.log", 5000, 200)?;

    // Génération des paires de clés pour chaque nœud
    let keypair_node1 = Keypair::generate(&mut OsRng);
    let keypair_node2 = Keypair::generate(&mut OsRng);
    let keypair_node3 = Keypair::generate(&mut OsRng);
    let keypair_node4 = Keypair::generate(&mut OsRng);

    // Extraction des clés publiques
    let node1_public_key = keypair_node1.public;
    let node2_public_key = keypair_node2.public;
    let node3_public_key = keypair_node3.public;
    let node4_public_key = keypair_node4.public;

    // Configuration des clés publiques pour chaque nœud
    let mut peer_keys_node1 = std::collections::HashMap::new();
    peer_keys_node1.insert(2, node2_public_key);
    peer_keys_node1.insert(3, node3_public_key);
    peer_keys_node1.insert(4, node4_public_key);

    let mut peer_keys_node2 = std::collections::HashMap::new();
    peer_keys_node2.insert(1, node1_public_key);
    peer_keys_node2.insert(3, node3_public_key);
    peer_keys_node2.insert(4, node4_public_key);

    let mut peer_keys_node3 = std::collections::HashMap::new();
    peer_keys_node3.insert(1, node1_public_key);
    peer_keys_node3.insert(2, node2_public_key);
    peer_keys_node3.insert(4, node4_public_key);

    let mut peer_keys_node4 = std::collections::HashMap::new();
    peer_keys_node4.insert(1, node1_public_key);
    peer_keys_node4.insert(2, node2_public_key);
    peer_keys_node4.insert(3, node3_public_key);

    // Création des nœuds
    let mut node1 = PeerReviewNode::new_peerreview(
        1,
        logger_node1,
        keypair_node1,
        witnesses_map.clone(),
        peer_keys_node1,
    );

    let mut node2 = PeerReviewNode::new_peerreview(
        2,
        logger_node2,
        keypair_node2,
        witnesses_map.clone(),
        peer_keys_node2,
    );

    let mut node3 = PeerReviewNode::new_peerreview(
        3,
        logger_node3,
        keypair_node3,
        witnesses_map.clone(),
        peer_keys_node3,
    );

    let mut node4 = PeerReviewNode::new_peerreview(
        4,
        logger_node4,
        keypair_node4,
        witnesses_map.clone(),
        peer_keys_node4,
    );

    println!("✓ 4 nœuds initialisés\n");

    // === Étape 1: Communication entre Nœud 1 et Nœud 2 ===
    println!("--- Étape 1: Communication Nœud 1 ↔ Nœud 2 ---");
    println!("Les authenticators sont envoyés aux témoins EN TEMPS RÉEL\n");

    // Nœud 1 envoie 5 messages à Nœud 2
    for i in 1..=5 {
        let msg = node1.send_message(2, &format!("Message de 1 vers 2 ({})", i))?;
        
        // Nœud 2 reçoit le message
        if let Some(_ack) = node2.receive_message(&msg, 1)? {
            let (seq, sig) = match &msg {
                peerreview_rust::types::PeerReviewMsg::Send { seq, sig, .. } => (*seq, *sig),
                _ => continue,
            };
            println!("✓ Message {} : Nœud 1 → Nœud 2 (seq={})", i, seq);
            
            // IMPORTANT : Nœud 2 envoie IMMÉDIATEMENT l'authenticator au témoin 3
            println!("  [Nœud 2] → Envoi authenticator seq={} au témoin 3", seq);
            node3.store_authenticator(1, seq, sig);
        }
    }

    println!("\n✓ 5 messages échangés\n");

    // === Étape 2: Challenge automatique du témoin 3 ===
    println!("--- Étape 2: Challenge automatique du témoin 3 ---\n");
    
    if let Some(challenge) = node3.challenge_witnessed_node(1)? {
        let (min_seq, max_seq) = match &challenge.kind {
            peerreview_rust::types::messages::ChallengeKind::Audit { min_auth, max_auth } => (min_auth.seq, max_auth.seq),
            _ => (0, 0),
        };
        let seq_count = if max_seq >= min_seq { max_seq - min_seq + 1 } else { 0 };
        println!("✓ Témoin 3 challenge le nœud 1 pour {} logs", seq_count);
        println!("  Séquences demandées: {} à {}\n", min_seq, max_seq);
        
        // Nœud 1 répond au challenge
        let logs = node1.logger.get_log(1, node1.logger.s_k)?;
        println!("✓ Nœud 1 répond avec {} logs\n", logs.len());
        
        // === Étape 3: Vérification - Nœud 1 HONNÊTE ===
        println!("--- Étape 3: Vérification des logs du Nœud 1 (HONNÊTE) ---\n");
        
        let is_valid = node3.verify_logs_as_witness(1, &logs);
        
        if is_valid {
            println!("✓ RÉSULTAT: Nœud 1 est HONNÊTE");
            println!("✓ Témoin 3 libère les authenticators\n");
            node3.clear_authenticators(1);
        } else {
            println!("✗ RÉSULTAT: Nœud 1 est FAUTIF (inattendu!)\n");
        }
    }

    // === Étape 4: Communication Nœud 2 → Nœud 1 (NŒUD FAUTIF) ===
    println!("--- Étape 4: Communication Nœud 2 → Nœud 1 (Nœud 2 sera FAUTIF) ---\n");

    // Nœud 2 envoie 5 messages à Nœud 1
    for i in 1..=5 {
        let msg = node2.send_message(1, &format!("Message de 2 vers 1 ({})", i))?;
        
        // Nœud 1 reçoit le message
        if let Some(_ack) = node1.receive_message(&msg, 2)? {
            let (seq, sig) = match &msg {
                peerreview_rust::types::PeerReviewMsg::Send { seq, sig, .. } => (*seq, *sig),
                _ => continue,
            };
            println!("✓ Message {} : Nœud 2 → Nœud 1 (seq={})", i, seq);
            
            // Nœud 1 envoie l'authenticator au témoin 4
            println!("  [Nœud 1] → Envoi authenticator seq={} au témoin 4", seq);
            node4.store_authenticator(2, seq, sig);
        }
    }

    println!("\n✓ 5 messages échangés\n");

    // === Étape 5: Challenge du témoin 4 - Nœud 2 FAUTIF ===
    println!("--- Étape 5: Challenge du témoin 4 ---\n");
    
    if let Some(challenge) = node4.challenge_witnessed_node(2)? {
        let (min_seq, max_seq) = match &challenge.kind {
            peerreview_rust::types::messages::ChallengeKind::Audit { min_auth, max_auth } => (min_auth.seq, max_auth.seq),
            _ => (0, 0),
        };
        let seq_count = if max_seq >= min_seq { max_seq - min_seq + 1 } else { 0 };
        println!("✓ Témoin 4 challenge le nœud 2 pour {} logs", seq_count);
        println!("  Séquences demandées: {} à {}\n", min_seq, max_seq);
        
        // Nœud 2 répond avec des logs CORROMPUS (simulation de fraude)
        println!("⚠️  SIMULATION: Nœud 2 va répondre avec des logs corrompus\n");
        
        let mut logs = node2.logger.get_log(1, node2.logger.s_k)?;
        
        // CORROMPRE la chaîne de hash du premier log
        if !logs.is_empty() {
            logs[0].hash[0] = logs[0].hash[0].wrapping_add(1);
            println!("🔴 Hash du log seq={} CORROMPU pour simulation\n", logs[0].s_k);
        }
        
        // === Étape 6: Vérification - Détection de la faute ===
        println!("--- Étape 6: Vérification des logs du Nœud 2 (FAUTIF) ---\n");
        
        let is_valid = node4.verify_logs_as_witness(2, &logs);
        
        if !is_valid {
            println!("✗ RÉSULTAT: Nœud 2 est FAUTIF");
            println!("✓ Témoin 4 a détecté l'incohérence");
            println!("✓ Protocole Evidence activé pour propager la preuve\n");
        }
    }

    // === Étape 7: Propagation des preuves via Evidence ===
    println!("--- Étape 7: Diffusion de la preuve à TOUS les nœuds ---\n");
    
    // Le témoin 4 diffuse la preuve d'exposition du nœud 2
    use peerreview_rust::types::{Proof, EvidenceType, Authenticator, MsgLogEntry, messages::MsgContent, MsgType};
    
    let logs_node2 = node2.logger.get_log(node2.logger.s_k - 4, node2.logger.s_k)?;
    
    let authenticator = if !logs_node2.is_empty() {
        Authenticator {
            seq: logs_node2[0].s_k,
            hash: logs_node2[0].hash,
            sig: logs_node2[0].sig,
        }
    } else {
        Authenticator {
            seq: 0,
            hash: [0u8; 32],
            sig: [0u8; 64],
        }
    };

    let log_suffix: Vec<MsgLogEntry> = logs_node2.iter().map(|log| MsgLogEntry {
        seq: log.s_k,
        msg_type: MsgType::Send,
        dest: log.corr,
        hash: log.hash,
        sig: log.sig,
        content: MsgContent::SendContent {
            dest: log.corr,
            message: log.msg.clone(),
        },
    }).collect();
    
    let exposure_proof = Proof {
        guilty_node: 2,
        accuser_node: 4,  // Témoin 4 a détecté
        evidence_type: EvidenceType::BrokenHashChain,
        authenticator,
        log_suffix,
        reason: "Chaîne de hash brisée détectée".to_string(),
    };
    
    println!("📢 Témoin 4 diffuse la preuve d'exposition du nœud 2 à TOUS les nœuds");
    println!("  Source: Témoin {}", exposure_proof.accuser_node);
    println!("  Type: {:?}", exposure_proof.evidence_type);
    println!("  Raison: {}\n", exposure_proof.reason);
    
    // TOUS les nœuds reçoivent et vérifient la preuve
    println!("=== Chaque nœud reçoit la preuve et met à jour sa table ===\n");
    
    // Nœud 1 vérifie la preuve
    println!("--- Nœud 1 ---");
    let node1_verdict = verify_and_update_detection_table(&mut node1, &exposure_proof, "Nœud 1");
    
    // Nœud 2 vérifie la preuve (même lui-même!)
    println!("\n--- Nœud 2 (le nœud accusé) ---");
    let node2_verdict = verify_and_update_detection_table(&mut node2, &exposure_proof, "Nœud 2");
    
    // Témoin 3 vérifie la preuve
    println!("\n--- Témoin 3 ---");
    let node3_verdict = verify_and_update_detection_table(&mut node3, &exposure_proof, "Témoin 3");
    
    // Témoin 4 vérifie la preuve (celui qui l'a émise)
    println!("\n--- Témoin 4 (émetteur de la preuve) ---");
    let node4_verdict = verify_and_update_detection_table(&mut node4, &exposure_proof, "Témoin 4");

    // === Résumé final - Table de détection de chaque nœud ===
    println!("\n=== Résumé Final : Tables de détection de chaque nœud ===\n");
    
    println!("📊 Table du Nœud 1:");
    println!("  - Nœud 1: {:?}", node1.get_detection_state(1));
    println!("  - Nœud 2: {:?} {}", node1.get_detection_state(2), if node1_verdict { "✓" } else { "✗" });
    println!("  - Témoin 3: {:?}", node1.get_detection_state(3));
    println!("  - Témoin 4: {:?}", node1.get_detection_state(4));
    
    println!("\n📊 Table du Nœud 2:");
    println!("  - Nœud 1: {:?}", node2.get_detection_state(1));
    println!("  - Nœud 2: {:?} {}", node2.get_detection_state(2), if node2_verdict { "✓" } else { "✗" });
    println!("  - Témoin 3: {:?}", node2.get_detection_state(3));
    println!("  - Témoin 4: {:?}", node2.get_detection_state(4));
    
    println!("\n📊 Table du Témoin 3:");
    println!("  - Nœud 1: {:?}", node3.get_detection_state(1));
    println!("  - Nœud 2: {:?} {}", node3.get_detection_state(2), if node3_verdict { "✓" } else { "✗" });
    println!("  - Témoin 3: {:?}", node3.get_detection_state(3));
    println!("  - Témoin 4: {:?}", node3.get_detection_state(4));
    
    println!("\n📊 Table du Témoin 4:");
    println!("  - Nœud 1: {:?}", node4.get_detection_state(1));
    println!("  - Nœud 2: {:?} {}", node4.get_detection_state(2), if node4_verdict { "✓" } else { "✗" });
    println!("  - Témoin 3: {:?}", node4.get_detection_state(3));
    println!("  - Témoin 4: {:?}", node4.get_detection_state(4));
    
    println!("\n✅ Tous les nœuds ont la même vue : Nœud 2 est EXPOSED");

    Ok(())
}

/// Chaque nœud vérifie la preuve et met à jour sa propre table de détection
fn verify_and_update_detection_table(
    node: &mut PeerReviewNode,
    proof: &peerreview_rust::types::Proof,
    node_name: &str,
) -> bool {
    println!("[{}] Réception de la preuve d'exposition du nœud {}...", node_name, proof.guilty_node);
    
    // Vérifier que le témoin émetteur est légitime
    let witnesses = node.get_witnesses(proof.guilty_node);
    if !witnesses.contains(&proof.accuser_node) {
        println!("[{}] ✗ Le nœud {} n'est pas un témoin valide du nœud {}", 
            node_name, proof.accuser_node, proof.guilty_node);
        println!("[{}] ✗ Preuve rejetée\n", node_name);
        return false;
    }
    
    // Rejouer la vérification des logs
    println!("[{}] Rejeu de la vérification...", node_name);
    
    match proof.evidence_type {
        peerreview_rust::types::EvidenceType::BrokenHashChain => {
            use sha2::{Digest, Sha256};
            use peerreview_rust::journal::entry::LogType;
            
            for (i, log) in proof.log_suffix.iter().enumerate() {
                if i > 0 {
                    let prev_log = &proof.log_suffix[i - 1];
                    
                    // Recalculer le hash attendu
                    let mut hasher = Sha256::new();
                    hasher.update(prev_log.hash);
                    hasher.update(log.seq.to_be_bytes());
                    hasher.update((LogType::Send as u8).to_be_bytes());
                    
                    let expected_hash: [u8; 32] = hasher.finalize().into();
                    
                    if log.hash != expected_hash {
                        println!("[{}] ✓ Incohérence confirmée à seq={}", node_name, log.seq);
                        println!("[{}] ✓ Preuve VALIDE → Mise à jour table: Nœud {} = EXPOSED", 
                            node_name, proof.guilty_node);
                        
                        // Mettre à jour la table de détection de ce nœud
                        use peerreview_rust::types::PeerStatus as DetectionState;
                        node.set_detection_state(proof.guilty_node, DetectionState::Exposed);
                        return true;
                    }
                }
            }
            
            println!("[{}] ✗ Aucune incohérence trouvée", node_name);
            println!("[{}] ✗ Preuve INVALIDE → Table non modifiée", node_name);
            false
        }
        _ => {
            println!("[{}] Type de preuve non implémenté", node_name);
            false
        }
    }
}
