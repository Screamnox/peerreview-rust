use std::io;

use crate::{journal::entry::LogEntry, protocols::node::PeerReviewNode};

#[derive(Clone, Copy)]
pub struct Snapchot {
    // Ici doivent figurer tout les éléments importants (données) au bon fonctionnement de l'application 
    // afin que le témoins puisse simuler avec sa machine à état
}

impl Snapchot {
    pub fn new() -> Self{
        unimplemented!();
    }
}

impl PeerReviewNode {
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
            let snapchot = Snapchot::new();
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
        let snapchot = Snapchot::new();

        self.snapchot_list_witness
            .entry(target_id)
            .or_insert_with(Vec::new)
            .push(snapchot);
    }
}
