mod journal;
mod protocols;

use journal::Logger;
use protocols::node::PeerReviewNode;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    println!("=== Démonstration du protocole PeerReview ===\n");

    // Création des loggers pour les nœuds
    let logger_node1 = Logger::new("node1_journal.log", 5000, 200)?;
    let logger_node2 = Logger::new("node2_journal.log", 5000, 200)?;

    // Clés privées des nœuds (simulées)
    let node1_private_key: [u8; 32] = [1; 32];
    let node2_private_key: [u8; 32] = [2; 32];

    // Création des nœuds PeerReview
    let mut node1 = PeerReviewNode::new(1, logger_node1, node1_private_key);
    let mut node2 = PeerReviewNode::new(2, logger_node2, node2_private_key);

    // Enregistrer les clés publiques mutuelles
    let node1_public_key = node1.logger.get_public_key().clone();
    let node2_public_key = node2.logger.get_public_key().clone();
    
    node1.register_peer(2, node2_public_key);
    node2.register_peer(1, node1_public_key);

    println!("✓ Nœud 1 et Nœud 2 initialisés\n");

    // === Scénario 1: Communication réussie ===
    println!("--- Scénario 1: Communication réussie ---");
    
    // Algorithm 1: Nœud 1 envoie un message à Nœud 2
    let message = "Bonjour depuis le nœud 1!";
    let send_msg = node1.send_message(2, message)?;
    println!("✓ Message créé par le nœud 1\n");

    // Algorithm 2 & 3: Nœud 2 reçoit et vérifie le message, puis envoie un acquittement
    let ack_msg = node2.receive_message(&send_msg, 1)?;
    
    if let Some(ack) = ack_msg {
        println!("✓ Nœud 2 a créé un acquittement\n");
        
        // Algorithm 4: Nœud 1 vérifie l'acquittement
        let node2_pub_key = node1.peer_public_keys.get(&2).unwrap().clone();
        let is_valid = node1.verify_recv_message(
            &ack,
            2,
            &node2_pub_key,
            send_msg.seq_num,
            message,
        );
        
        if is_valid {
            println!("✓ Communication réussie entre nœud 1 et nœud 2!\n");
        } else {
            println!("✗ Échec de la vérification de l'acquittement\n");
        }
    } else {
        println!("✗ Pas d'acquittement reçu (message invalide)\n");
    }

   /*  // === Scénario 2: Message avec signature invalide ===
    println!("\n--- Scénario 2: Tentative avec signature invalide ---");
    
    let mut invalid_msg = send_msg.clone();
    invalid_msg.signature = [0xFF; 64]; // Signature corrompue
    
    let ack_invalid = node2.receive_message(&invalid_msg, 1)?;
    
    if ack_invalid.is_none() {
        println!("✓ Nœud 2 a correctement détecté le message invalide et créé un challenge\n");
    }

    // === Scénario 3: Envoi avec timeout (simulation) ===
    println!("--- Scénario 3: Simulation de timeout ---");
    
    // Simulation: pas d'acquittement reçu
    let result = node1.send_with_acknowledgment(
        2,
        "Test timeout",
        Duration::from_millis(100),
        None, // Pas d'acquittement
    )?;
    
    if !result {
        println!("✓ Challenge créé suite au timeout\n");
    }

    // === Scénario 4: Test du protocole de Consistency ===
    println!("--- Scénario 4: Protocole de Consistency ---");
    
    // Nœud 1 envoie un challenge de consistency au nœud 2
    let consistency_challenge = node1.send_consistency_challenge(2, 1, 5)?;
    println!("✓ Challenge de consistency envoyé\n");
    
    // Nœud 2 répond avec ses logs
    let logs_from_node2 = node2.respond_to_consistency_challenge(&consistency_challenge, 1, 5)?;
    println!("✓ Nœud 2 a répondu avec {} entrées\n", logs_from_node2.len());
    
    // Nœud 1 vérifie la consistency des logs reçus
    let is_consistent = node1.verify_consistency(2, &logs_from_node2);
    if is_consistent {
        println!("✓ Les logs du nœud 2 sont cohérents\n");
    } else {
        println!("✗ Incohérence détectée dans les logs du nœud 2\n");
    } */

    println!("=== Démonstration terminée ===");

    // Test de get_log() de la branche journal
    let mut test_logger = Logger::new("journal.log", 10, 200)?;
    let result = test_logger.get_log(10)?;
    
    println!("\nTest get_log - Taille : {}", result.len());
    
    if !result.is_empty() {
        println!("Première log du get_log : {:?}", result[0]);
    }

    let sig_recv: [u8; 64] = [
        0xAA, 0x19, 0xE3, 0x4F, 0x0C, 0xB2, 0x7D, 0x33, 0x91, 0x60, 0x18, 0x72, 0xBE, 0x05, 0xD9,
        0x27, 0x48, 0x9A, 0xF1, 0xC3, 0x14, 0x26, 0xE0, 0x8F, 0x55, 0x31, 0xB4, 0x7A, 0x02, 0x63,
        0xD5, 0xC0, 0xAA, 0x19, 0xE3, 0x4F, 0x0C, 0xB2, 0x7D, 0x33, 0x91, 0x60, 0x18, 0x72, 0xBE,
        0x05, 0xD9, 0x27, 0x48, 0x9A, 0xF1, 0xC3, 0x14, 0x26, 0xE0, 0x8F, 0x02, 0x63, 0xD5, 0xC0,
        0x55, 0x31, 0xB4, 0x7A,
    ];

    test_logger.log_send(42, "Salut je suis une base64")?;
    test_logger.log_recv(69, 40, sig_recv, "Salut je suis une base64")?;

    for log in test_logger.get_log(10)? {
        println!("{}", log);
    }

    Ok(())
}
