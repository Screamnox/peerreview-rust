mod journal;
mod protocols;

use journal::Logger;
use protocols::node::PeerReviewNode;

fn main() -> std::io::Result<()> {
    println!("=== Démonstration du protocole PeerReview ===\n");
    println!("Scénario : Communication honnête avec surveillance par témoins\n");

    // Configuration des témoins (statique, tous les nœuds la connaissent)
    let mut witnesses_map = std::collections::HashMap::new();
    witnesses_map.insert(1, vec![3]); // Nœud 1 a pour témoin: 3

    println!("=== Configuration ===");
    println!("Nœud 1: témoin = {:?}", witnesses_map.get(&1).unwrap());
    println!("Nœud 2: pas de témoin (simple destinataire)\n");

    // Création des loggers pour les nœuds
    let logger_node1 = Logger::new("node1_journal.log", 5000, 200)?;
    let logger_node2 = Logger::new("node2_journal.log", 5000, 200)?;

    // Récupérer les clés publiques
    let node1_public_key = *logger_node1.get_public_key();
    let node2_public_key = *logger_node2.get_public_key();

    // Configuration des clés publiques (statique, tous les nœuds la connaissent)
    let mut peer_keys_node1 = std::collections::HashMap::new();
    peer_keys_node1.insert(2, node2_public_key);

    let mut peer_keys_node2 = std::collections::HashMap::new();
    peer_keys_node2.insert(1, node1_public_key);

    // Création des nœuds PeerReview avec la configuration complète
    let mut node1 = PeerReviewNode::new(1, logger_node1, witnesses_map.clone(), peer_keys_node1);
    let mut node2 = PeerReviewNode::new(2, logger_node2, witnesses_map.clone(), peer_keys_node2);

    println!("✓ Nœud 1 et Nœud 2 initialisés\n");

    // === Étape 1: Échange de messages ===
    println!("--- Étape 1: Échange de messages entre nœuds ---\n");

    // Nœud 1 envoie 11 messages à Nœud 2
    for i in 1..=11 {
        let msg = node1.send_message(2, &format!("Message {}", i))?;
        
        // Nœud 2 reçoit et vérifie le message
        node2.receive_message(&msg, 1)?;
        
        println!("✓ Message {} : Nœud 1 → Nœud 2 (seq={})", i, msg.seq_num);
    }

    println!("\n✓ 11 messages échangés avec succès\n");

    // === Étape 2: Configuration du témoin ===
    println!("--- Étape 2: Mise en place du témoin ---\n");
    
    let logger_node3 = Logger::new("node3_journal.log", 5000, 200)?;
    let _node3_public_key = *logger_node3.get_public_key();

    let mut peer_keys_node3 = std::collections::HashMap::new();
    peer_keys_node3.insert(1, node1_public_key); // Témoin 3 surveille nœud 1

    let mut node3 = PeerReviewNode::new(3, logger_node3, witnesses_map.clone(), peer_keys_node3);

    println!("✓ Témoin 3 initialisé pour surveiller le nœud 1\n");

    // === Étape 3: Surveillance et accumulation d'authenticators ===
    println!("--- Étape 3: Accumulation d'authenticators par le témoin ---\n");

    // Simuler que le témoin 3 reçoit les authenticators des 11 messages précédents
    // (normalement envoyés automatiquement par receive_message)
    let all_logs = node1.logger.get_log(11)?;
    for log_entry in &all_logs {
        node3.store_authenticator(1, log_entry.s_k, log_entry.sig);
        println!("Témoin 3 a stocké l'authenticator seq={}", log_entry.s_k);
    }

    println!("\n✓ 11 authenticators stockés par le témoin 3\n");

    // === Étape 4: Challenge automatique ===
    println!("--- Étape 4: Challenge automatique du témoin ---\n");
    println!("Le témoin 3 a atteint le seuil de {} authenticators", node3.challenge_threshold);

    if let Some(challenge) = node3.challenge_witnessed_node(1)? {
        println!("✓ Challenge créé: témoin {} → nœud {}", challenge.witness_id, challenge.target_id);
        println!("  Séquences demandées: {:?}\n", &challenge.seq_nums);

        // === Étape 5: Réponse honnête du nœud ===
        println!("--- Étape 5: Réponse du nœud surveillé ---\n");
        let logs_response = node1.respond_to_challenge(&challenge)?;
        println!("✓ Nœud 1 a répondu avec {} logs\n", logs_response.len());

        // === Étape 6: Vérification par le témoin ===
        println!("--- Étape 6: Vérification des logs par le témoin ---\n");
        let is_valid = node3.verify_logs_as_witness(1, &logs_response);

        if is_valid {
            println!("✓ RÉSULTAT: Tous les logs sont valides, le nœud 1 est HONNÊTE!");
            println!("✓ Les authenticators ont été automatiquement supprimés");
            println!("✓ Le protocole de consistency a fonctionné correctement\n");
        } else {
            println!("✗ RÉSULTAT: Incohérence détectée\n");
        }

        // Afficher les nœuds exposés
        let exposed = node3.get_exposed_nodes();
        if exposed.is_empty() {
            println!("✓ Aucun nœud exposé : tous les nœuds sont honnêtes\n");
        } else {
            println!("⚠️  Nœuds exposés par le témoin 3: {:?}\n", exposed);
        }
    } else {
        println!("✗ Seuil non atteint, pas de challenge");
    }

    println!("\n=== Démonstration terminée ===");

    Ok(())
}
