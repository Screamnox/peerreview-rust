use crate::journal::{Logger, entry::LogEntry};
use std::{collections::HashMap, hash::Hash, io, ptr::null};

/// Type de message : Send ou Recv
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum MessageType {
    Send,
    Recv,
}

/// Structure d'un message PeerReview transmis sur le réseau
#[derive(Debug, Clone)]
pub struct PeerReviewMessage {
    pub msg_type: MessageType,
    pub seq_num: usize,
    #[allow(dead_code)]
    pub prev_hash: [u8; 32],
    pub signature: [u8; 64],
    #[allow(dead_code)]
    pub dest: u32,
    pub payload: String,
}

/// Authenticator stocké par un témoin
#[derive(Debug, Clone)]
pub struct StoredAuthenticator {
    pub node_id: u32,       // Nœud surveillé
    pub seq_num: usize,     // Numéro de séquence
    pub signature: [u8; 64], // Signature Ed25519
}

/// État de détection d'un nœud (Algorithm 15)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetectionState {
    Trusted,        // État par défaut: nœud correct/de confiance
    Suspected,      // Nœud suspect (challenge en attente)
    Exposed,        // Nœud définitivement fautif
}

/// Challenge en attente pour un nœud suspect
#[derive(Debug, Clone)]
pub struct PendingChallenge {
    pub target_node_id: u32,
    pub seq_nums: Vec<usize>,
}

/// Structure d'un nœud PeerReview
pub struct PeerReviewNode {
    pub node_id: u32,
    pub logger: Logger,
    pub prev_hash: [u8; 32],
    pub keypair: ed25519_dalek::Keypair,
    pub peer_public_keys: HashMap<u32, ed25519_dalek::PublicKey>,
    /// Configuration des témoins : HashMap<node_id, Vec<witness_ids>>
    /// Tous les nœuds connaissent les témoins de tous les autres nœuds
    pub witnesses_map: HashMap<u32, Vec<u32>>,
    /// Authenticators stockés en tant que témoin : HashMap<node_id_surveillé, Vec<authenticators>>
    pub stored_authenticators: HashMap<u32, Vec<StoredAuthenticator>>,
    /// Seuil d'authenticators avant de challenger (par défaut: 10)
    pub challenge_threshold: usize,
    /// Nœuds marqués comme EXPOSED (fautifs)
    pub exposed_nodes: Vec<u32>,
    /// États de détection pour chaque nœud (Algorithm 15)
    pub detection_states: HashMap<u32, DetectionState>,
    /// Challenges en attente pour les nœuds suspects
    pub pending_challenges: HashMap<u32, Vec<PendingChallenge>>,
    /// Liste des snapchot des témoins
    pub snapchot_list_witness: HashMap<u32, Vec<Snapchot>>
}

impl PeerReviewNode {
    /// Crée un nouveau nœud PeerReview avec la configuration des témoins et les clés publiques
    pub fn new(
        node_id: u32,
        mut logger: Logger,
        keypair: ed25519_dalek::Keypair,
        witnesses_map: HashMap<u32, Vec<u32>>,
        peer_public_keys: HashMap<u32, ed25519_dalek::PublicKey>,
    ) -> Self {
        // Récupérer le hash initial du logger (dernier hash enregistré ou HASH_INIT)
        let prev_hash = if logger.s_k == 0 {
            // Pas encore de logs, utiliser HASH_INIT du Logger
            const HASH_INIT: [u8; 32] = [
                0x3A, 0x92, 0x11, 0xDE, 0x77, 0xC4, 0x0B, 0xE8, 0x5F, 0xA2, 0x39, 0x6C, 0x00, 0x4D,
                0x8B, 0x17, 0xD1, 0x20, 0xFE, 0x58, 0x93, 0xA7, 0x51, 0xCE, 0x29, 0x74, 0x66, 0x01,
                0xB8, 0x42, 0xDA, 0x10,
            ];
            HASH_INIT
        } else {
            // Récupérer le dernier hash du logger
            match logger.get_log(logger.s_k.saturating_sub(0), logger.s_k) {
                Ok(logs) if !logs.is_empty() => logs[0].hash,
                _ => [0u8; 32], // Fallback sur hash nul si erreur
            }
        };

        Self {
            node_id,
            logger,
            prev_hash,
            keypair,
            peer_public_keys,
            witnesses_map,
            stored_authenticators: HashMap::new(),
            challenge_threshold: 10,
            exposed_nodes: Vec::new(),
            detection_states: HashMap::new(),
            pending_challenges: HashMap::new(),
            snapchot_list_witness: HashMap::new(),
        }
    }

    /// Retourne les témoins d'un nœud donné
    pub fn get_witnesses(&self, node_id: u32) -> Vec<u32> {
        self.witnesses_map
            .get(&node_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Envoie un authenticator (signature) aux témoins d'un nœud
    /// C'est appelé quand on reçoit un message d'un autre nœud
    pub fn send_authenticator_to_witnesses(
        &self,
        observed_node_id: u32,
        seq_num: usize,
        signature: [u8; 64],
    ) -> std::io::Result<()> {
        let witnesses = self.get_witnesses(observed_node_id);

        if witnesses.is_empty() {
            println!(
                "[Nœud {}] Aucun témoin configuré pour le nœud {}",
                self.node_id, observed_node_id
            );
            return Ok(());
        }

        println!(
            "[Nœud {}] Envoi de l'authenticator (seq={}) du nœud {} aux {} témoin(s)",
            self.node_id,
            seq_num,
            observed_node_id,
            witnesses.len()
        );

        for witness_id in &witnesses {
            println!(
                "[Nœud {}] → Témoin {} : authenticator seq={} sig={}",
                self.node_id,
                witness_id,
                seq_num,
                hex::encode(&signature[..8])
            );
            // TODO: Implémenter l'envoi réseau réel de l'authenticator au témoin
        }

        Ok(())
    }

    /// Stocke un authenticator reçu en tant que témoin
    pub fn store_authenticator(
        &mut self,
        observed_node_id: u32,
        seq_num: usize,
        signature: [u8; 64],
    ) {
        let auth = StoredAuthenticator {
            node_id: observed_node_id,
            seq_num,
            signature,
        };

        self.stored_authenticators
            .entry(observed_node_id)
            .or_insert_with(Vec::new)
            .push(auth);

        let count = self.stored_authenticators.get(&observed_node_id).unwrap().len();
        println!(
            "[Témoin {}] Authenticator stocké pour nœud {} (seq={}). Total: {}",
            self.node_id, observed_node_id, seq_num, count
        );
    }

    /// Vérifie si le seuil d'authenticators est atteint pour challenger un nœud
    pub fn should_challenge(&self, observed_node_id: u32) -> bool {
        if let Some(auths) = self.stored_authenticators.get(&observed_node_id) {
            auths.len() >= self.challenge_threshold
        } else {
            false
        }
    }

    /// Algorithme 9 : Audit d’un nœud i
    pub fn perform_audit(
        &mut self,
        target_id: u32,
        ) -> std::io::Result<()> 
    {
        println!("[Nœud {}] Début de l’audit du nœud {}", self.node_id, target_id);

        // 1. Récupérer le dernier authenticator α_i_k

        let auth_table = &self.stored_authenticators[&target_id];
        let alpha_k = &auth_table[auth_table.len()];

        let last_s_k = alpha_k.seq_num;

        // 2. Envoyer le challenge d’audit
        println!(
            "[Nœud {}] Audit Request envoyé à {} (s_k_start={})",
            self.node_id, target_id, last_s_k
        );

        // 3. Récupération des nouveaux logs
        println!(
            "[Noeud {}] Reception des logs de {}",
            self.node_id, target_id
        );

        /* TODO : Cette partie est normalement faite à distance, il s'agit directement du résultat*/
        let log_peer = self.logger.get_log(last_s_k, self.logger.s_k)?;

        // 4. Rejouer avec Algo 10
        self.replay_and_verify(log_peer, target_id)?;

        Ok(())
    }

    /// Algorithme 10 : Replay & Verification
    pub fn replay_and_verify(
        &mut self,
        log_peer: Vec<LogEntry>,
        target_id: u32
    ) -> std::io::Result<()> 
    {
        //Charger la dernière snapshot si elle existe
        if self.snapchot_list_witness.get(&target_id).is_none() {
            self.snapchot_list_witness.insert(target_id, Vec::new() as Vec<Snapchot>);
            let snapchot = Snapchot::new(0);
        } else {
            let snapchot = *self
                            .snapchot_list_witness
                            .get(&target_id)
                            .ok_or(io::Error::
                                new(io::ErrorKind::NotFound,
                                    "Le vecteur snapchot n'as pas été trouvé"))
                            ?
                            .last()
                            .ok_or(io::Error::
                                new(io::ErrorKind::NotFound,
                                    "La snapchot n'as pas été trouvée"))
                            ?;
        }
        //La machine à état rejoue l'output des logs à partir de la snapshot et des événement extérieur
        // /!\/!\/!\ Ici, on clone simplement les données, dans les faits il faut adapter ce code à l'application afin de reproduire
        // les logs output /!\/!\/!\ 
        let log_state_machine = log_peer.clone();

        if log_peer.len() != log_state_machine.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Le nombre de log de la state machine et ceux reçu ne correspondent pas",
            ));
        }

        // Ici on vérifie log par log l'égalité entre la state machine et les logs reçus
        for i in 0..log_peer.len() {
            if log_peer.get(i) != log_state_machine.get(i) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Le nombre de log de la state machine et ceux reçu ne correspondent pas",
                )); 
            }
        }

        Ok(())
    }

    pub fn create_snapchot(&mut self, target_id: u32) {
        //Ici doivent être sauvegardé toute les données de la machine à état dans la struct snapchot qui sera ensuite ajouté
        //aux vec de snapshot du noeud
        let snapchot = Snapchot::new(0);

        self.snapchot_list_witness
            .entry(target_id)
            .or_insert_with(Vec::new)
            .push(snapchot);
    }
}

#[derive(Clone, Copy)]
pub struct Snapchot {
    // Ici doivent figurer tout les éléments importants (données) au bon fonctionnement de l'application 
    // afin que le témoins puisse simuler avec sa machine à état
    example: usize
}

impl Snapchot {
    pub fn new(example: usize) -> Self{
        Self{
            example
        }
    }
}
