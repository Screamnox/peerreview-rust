use crate::journal::Logger;
use std::time::Duration;
use std::thread;

/// Envoie un message à un autre nœud
pub fn send_message(logger: &mut Logger, destinataire: &str, message: &str) -> std::io::Result<()> {
    logger.log("SEND", destinataire, message)?;
    println!("Message envoyé à {} : {}", destinataire, message);
    Ok(())
}

/// Fonction appelée en cas de problème avec l'acquittement (absent ou invalide)
pub fn challenge(logger: &mut Logger, destinataire: &str, raison: &str) -> std::io::Result<()> {
    let challenge_msg = format!("CHALLENGE: {}", raison);
    logger.log("SEND", destinataire, &challenge_msg)?;
    println!("Challenge envoyé à {} : {}", destinataire, raison);
    Ok(())
}

/// Vérifie si l'acquittement reçu est valide
pub fn verify_ack(ack: &str, expected: &str) -> bool {
    ack == expected
}

/// Envoie un message et attend un acquittement pendant un temps T
/// Si l'acquittement n'est pas reçu ou est invalide, envoie un challenge
pub fn send_with_ack(
    logger: &mut Logger,
    destinataire: &str,
    message: &str,
    timeout: Duration,
    expected_ack: &str,
) -> std::io::Result<()> {
    // Envoi du message
    send_message(logger, destinataire, message)?;
    
    // Attente du timeout
    println!("Attente de l'acquittement pendant {:?}...", timeout);
    thread::sleep(timeout);
    
    // Simulation de la réception (à remplacer par une vraie réception réseau)
    // Pour l'instant, on simule qu'aucun acquittement n'est reçu
    let ack_received: Option<String> = None; // Remplacer par la vraie logique de réception
    
    match ack_received {
        Some(ack) => {
            if verify_ack(&ack, expected_ack) {
                println!("Acquittement valide reçu de {}", destinataire);
                logger.log("RECV", destinataire, &ack)?;
                Ok(())
            } else {
                println!("Acquittement invalide reçu de {}", destinataire);
                challenge(logger, destinataire, "Acquittement invalide")
            }
        }
        None => {
            println!("Aucun acquittement reçu de {}", destinataire);
            challenge(logger, destinataire, "Timeout - pas d'acquittement")
        }
    }
}