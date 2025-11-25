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

    println!("✓ Nœud 1 et Nœud 2 initialisés\n");

    // === Scénario 1: Communication réussie ===
    println!("--- Scénario 1: Communication réussie ---");
    
    // Algorithm 1: Nœud 1 envoie un message à Nœud 2
    let message = "Bonjour depuis le nœud 1!";
    let send_msg = node1.send_message(2, message)?;
    println!("✓ Message créé par le nœud 1\n");

    // Algorithm 2 & 3: Nœud 2 reçoit et vérifie le message, puis envoie un acquittement
    let ack_msg = node2.receive_message(&send_msg, 1, &node1_private_key)?;
    
    if let Some(ack) = ack_msg {
        println!("✓ Nœud 2 a créé un acquittement\n");
        
        // Algorithm 4: Nœud 1 vérifie l'acquittement
        let is_valid = node1.verify_recv_message(
            &ack,
            2,
            &node2_private_key,
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

    // === Scénario 2: Message avec signature invalide ===
    println!("\n--- Scénario 2: Tentative avec signature invalide ---");
    
    let mut invalid_msg = send_msg.clone();
    invalid_msg.signature = [0xFF; 32]; // Signature corrompue
    
    let ack_invalid = node2.receive_message(&invalid_msg, 1, &node1_private_key)?;
    
    if ack_invalid.is_none() {
        println!("✓ Nœud 2 a correctement détecté le message invalide et créé un challenge\n");
    }

    // === Scénario 3: Envoi avec timeout (simulation) ===
    println!("--- Scénario 3: Simulation de timeout ---");
    
    // Simulation: pas d'acquittement reçu
    let result = node1.send_with_acknowledgment(
        2,
        &node2_private_key,
        "Test timeout",
        Duration::from_millis(100),
        None, // Pas d'acquittement
    )?;
    
    if !result {
        println!("✓ Challenge créé suite au timeout\n");
    }

    println!("=== Démonstration terminée ===");

    // Test de get_log() de la branche journal
    let mut test_logger = Logger::new("journal.log", 10, 200)?;
    let result = test_logger.get_log(100)?;
    println!("\nTest get_log - Taille : {}", result.len());

    Ok(())
}
