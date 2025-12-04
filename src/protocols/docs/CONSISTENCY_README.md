# Protocole de Consistency - PeerReview

## Vue d'ensemble

Le protocole de consistency permet de vérifier l'intégrité et la cohérence des journaux (logs) des nœuds dans le système PeerReview. Il complète le protocole de commitment en permettant aux nœuds de s'auditer mutuellement en comparant leurs historiques d'événements cryptographiquement vérifiables.

**Objectif** : Détecter les nœuds malveillants qui tentent de falsifier leur historique en vérifiant la cohérence des chaînes de hash et des signatures Ed25519.

## Architecture

### Structure `ConsistencyChallenge`

```rust
pub struct ConsistencyChallenge {
    pub challenger_id: u32,      // ID du nœud qui lance le challenge
    pub target_id: u32,           // ID du nœud ciblé par le challenge
    pub seq_num_start: usize,     // Début de la plage de logs demandée
    pub seq_num_end: usize,       // Fin de la plage de logs demandée
}
```

Cette structure représente une demande d'audit. Le nœud challenger demande au nœud target de fournir toutes ses entrées de journal entre `seq_num_start` et `seq_num_end`.

---

## Fonctions implémentées

### 1. Envoyer un challenge de consistency

```rust
pub fn send_consistency_challenge(
    &mut self,
    target_id: u32,
    seq_start: usize,
    seq_end: usize,
) -> std::io::Result<ConsistencyChallenge>
```

**Description** : Un nœud demande à un autre nœud de fournir ses entrées de log sur une plage donnée.

**Étapes détaillées** :

1. **Créer le message de challenge** :
   ```rust
   let challenge_msg = format!(
       "CHALLENGE_CONSISTENCY: Demande logs [{}, {}]",
       seq_start, seq_end
   );
   ```

2. **Logger le challenge** : Utilise `logger.log_send()` pour enregistrer la demande
   - Calcule automatiquement le hash et la signature
   - Crée une trace vérifiable du challenge

3. **Mettre à jour prev_hash** : Récupère le hash de l'entrée créée pour la prochaine opération

4. **Retourner la structure `ConsistencyChallenge`** : Contient toutes les informations nécessaires à la réponse

**Utilisation** :
```rust
let challenge = node_auditor.send_consistency_challenge(suspect_id, 10, 20)?;
// Demande les logs 10 à 20 du nœud suspect
```

---

### 2. Répondre à un challenge

```rust
pub fn respond_to_consistency_challenge(
    &mut self,
    challenge: &ConsistencyChallenge,
) -> std::io::Result<Vec<LogEntry>>
```

**Description** : Le nœud ciblé répond en fournissant les entrées de log demandées.

**Étapes détaillées** :

1. **Calculer le nombre d'entrées demandées** :
   ```rust
   let requested_count = challenge.seq_num_end
       .saturating_sub(challenge.seq_num_start) + 1;
   ```

2. **Récupérer les logs depuis le Logger** :
   ```rust
   let logs = self.logger.get_log(requested_count)?;
   ```
   - Le Logger retourne les `n` dernières entrées
   - Chaque entrée contient : `s_k`, `log_type`, `corr`, `s_k_corr`, `hash`, `sig`, `msg`

3. **Logger la réponse** : Enregistre qu'une réponse a été envoyée
   ```rust
   let response_msg = format!("RESPONSE_CONSISTENCY: {} entrées envoyées", logs.len());
   self.logger.log_send(challenge.challenger_id, &response_msg)?;
   ```

4. **Retourner les logs** : Le vecteur de `LogEntry` est envoyé au nœud challenger

**Utilisation** :
```rust
let my_logs = node_target.respond_to_consistency_challenge(&challenge)?;
// Retourne Vec<LogEntry> avec toutes les entrées demandées
```

---

### 3. Vérifier la consistency

```rust
pub fn verify_consistency(
    &self,
    target_id: u32,
    received_logs: &[LogEntry],
) -> bool
```

**Description** : Vérifie que les logs reçus sont cohérents et n'ont pas été falsifiés.

**Vérifications actuelles** :

1. **Vérifier que des logs ont été reçus** :
   ```rust
   if received_logs.is_empty() {
       println!("Erreur: Aucune entrée reçue");
       return false;
   }
   ```

2. **Vérifier la consécutivité des numéros de séquence** :
   ```rust
   for window in received_logs.windows(2) {
       if window[1].s_k != window[0].s_k + 1 {
           println!("Erreur: Numéros non consécutifs ({} -> {})", 
                    window[0].s_k, window[1].s_k);
           return false;
       }
   }
   ```

**Vérifications futures (TODO)** :

Pour chaque entrée `i` (sauf la première), le nœud auditeur doit :

1. **Recalculer le hash du commitment** :
   - **Pour SEND** : `c_k = H(corr || msg)`
   - **Pour RECV** : `c_k = H(corr || s_k_corr || msg)`

2. **Recalculer le hash de l'entrée** :
   ```
   ĥ_k = H(h_{k-1} || s_k || log_type || c_k)
   ```
   où `h_{k-1}` est le hash de l'entrée précédente

3. **Vérifier que le hash correspond** :
   ```rust
   if received_logs[i].hash != h_k_computed {
       return false;  // La chaîne de hash est brisée !
   }
   ```

4. **Vérifier la signature Ed25519** :
   ```rust
   let mut signed_data = [0u8; 40];
   signed_data[..8].copy_from_slice(&s_k.to_be_bytes());
   signed_data[8..].copy_from_slice(&h_k);
   
   if public_key.verify(&signed_data, &signature).is_err() {
       return false;  // Signature invalide !
   }
   ```

**Point clé** : Si la chaîne de hash est valide et toutes les signatures sont correctes, cela prouve mathématiquement que les logs n'ont pas été modifiés depuis leur création.

**Utilisation** :
```rust
let is_valid = node_auditor.verify_consistency(target_id, &received_logs);
if !is_valid {
    println!("ALERTE: Le nœud {} a fourni des logs incohérents !", target_id);
}
```

---

### 4. Vérification croisée (Cross-check)

```rust
pub fn cross_check_consistency(
    &mut self,
    node_a_id: u32,
    node_b_id: u32,
    seq_start: usize,
    seq_end: usize,
) -> std::io::Result<bool>
```

**Description** : Compare les logs de deux nœuds sur une même plage pour détecter des divergences.

**Principe** : Si deux nœuds ont échangé des messages, leurs logs doivent être cohérents :
- Une entrée SEND chez A → doit correspondre à une entrée RECV chez B
- Les signatures doivent être vérifiables mutuellement
- Les hash doivent correspondre aux messages échangés

**Étapes actuelles** :

1. **Logger la demande de vérification** :
   ```rust
   let check_msg = format!("CROSS_CHECK: Vérification nœuds {} et {} [{}, {}]",
                           node_a_id, node_b_id, seq_start, seq_end);
   self.logger.log_send(node_a_id, &check_msg)?;
   ```

2. **Mettre à jour prev_hash** : Maintient la cohérence de notre propre journal

**Implémentation future (TODO)** :

1. **Demander les logs aux deux nœuds** :
   ```rust
   let challenge_a = self.send_consistency_challenge(node_a_id, seq_start, seq_end)?;
   let challenge_b = self.send_consistency_challenge(node_b_id, seq_start, seq_end)?;
   
   let logs_a = // réponse de A
   let logs_b = // réponse de B
   ```

2. **Comparer les entrées correspondantes** :
   - Trouver les paires SEND/RECV entre A et B
   - Vérifier que les messages correspondent
   - Vérifier que les signatures sont cohérentes

3. **Détecter les divergences** :
   - Messages présents chez A mais absents chez B
   - Hash différents pour le même message
   - Signatures invalides

**Utilisation** :
```rust
let result = node_auditor.cross_check_consistency(alice_id, bob_id, 1, 100)?;
if !result {
    println!("Les logs d'Alice et Bob divergent !");
}
```

---

### 5. Détection d'incohérences avec témoins

```rust
pub fn detect_inconsistency(
    &self,
    target_id: u32,
    target_logs: &[LogEntry],
    witness_logs: &HashMap<u32, Vec<LogEntry>>,
) -> Vec<String>
```

**Description** : Compare les logs d'un nœud suspect avec ceux de plusieurs nœuds témoins pour détecter des falsifications.

**Principe des témoins** : Dans PeerReview, chaque nœud a un ensemble de témoins (witnesses) qui :
- Conservent des copies des logs du nœud
- Peuvent être interrogés pour confirmer ou infirmer les déclarations du nœud
- Permettent de détecter les tentatives de révision d'historique

**Vérifications actuelles** :

1. **Vérifier que le nœud a fourni des logs** :
   ```rust
   if target_logs.is_empty() {
       inconsistencies.push("Nœud n'a fourni aucune entrée de log");
   }
   ```

2. **Comparer la taille des logs** :
   ```rust
   for (witness_id, logs) in witness_logs {
       if logs.len() != target_logs.len() {
           inconsistencies.push(format!(
               "Divergence de taille: nœud {} ({} entrées) vs témoin {} ({} entrées)",
               target_id, target_logs.len(), witness_id, logs.len()
           ));
       }
   }
   ```

**Vérifications futures (TODO)** :

Pour chaque témoin, comparer les entrées de log une par une :

1. **Comparer les hash** :
   ```rust
   for i in 0..target_logs.len() {
       if target_logs[i].hash != witness_logs[witness_id][i].hash {
           inconsistencies.push(format!(
               "Hash différent à l'entrée {}: target={} vs témoin={}",
               i, hex::encode(target_logs[i].hash),
               hex::encode(witness_logs[witness_id][i].hash)
           ));
       }
   }
   ```

2. **Comparer les messages** :
   - Si les messages diffèrent pour le même `s_k`, c'est une preuve de falsification

3. **Vérifier la cohérence temporelle** :
   - Les témoins doivent avoir reçu les logs dans le bon ordre

**Résultat** : Retourne un vecteur de strings décrivant toutes les incohérences détectées.

**Utilisation** :
```rust
let mut witnesses = HashMap::new();
witnesses.insert(witness1_id, witness1_logs);
witnesses.insert(witness2_id, witness2_logs);
witnesses.insert(witness3_id, witness3_logs);

let issues = node_auditor.detect_inconsistency(suspect_id, &suspect_logs, &witnesses);

if !issues.is_empty() {
    println!("⚠️  {} incohérence(s) détectée(s) :", issues.len());
    for issue in &issues {
        println!("  - {}", issue);
    }
}
```

---

### 6. Créer un rapport d'audit

```rust
pub fn create_audit_report(
    &mut self,
    target_id: u32,
    inconsistencies: &[String],
) -> std::io::Result<()>
```

**Description** : Crée un rapport formel suite à la détection d'incohérences.

**Étapes détaillées** :

1. **Vérifier qu'il y a des incohérences** :
   ```rust
   if inconsistencies.is_empty() {
       return Ok(());  // Rien à signaler
   }
   ```

2. **Créer le message de rapport** :
   ```rust
   let report_msg = format!(
       "AUDIT_REPORT: {} incohérence(s) détectée(s) pour nœud {}",
       inconsistencies.len(), target_id
   );
   ```

3. **Logger le rapport** : Enregistre le rapport dans notre journal
   - Crée une trace permanente et vérifiable
   - Le rapport devient partie de notre historique immuable

4. **Afficher les détails** : Liste toutes les incohérences détectées
   ```rust
   for (i, inc) in inconsistencies.iter().enumerate() {
       println!("  {}. {}", i + 1, inc);
   }
   ```

**Point important** : Le rapport d'audit est lui-même une entrée de journal signée et hashée, ce qui le rend vérifiable et infalsifiable.

**Utilisation** :
```rust
let issues = node_auditor.detect_inconsistency(suspect_id, &suspect_logs, &witnesses);

if !issues.is_empty() {
    node_auditor.create_audit_report(suspect_id, &issues)?;
    // Le rapport est maintenant enregistré de manière permanente
}
```

---

## Flux d'utilisation typique

### Scénario 1 : Audit simple d'un nœud

```rust
// 1. Envoyer un challenge au nœud suspect
let challenge = node_auditor.send_consistency_challenge(suspect_id, 1, 100)?;

// 2. Le nœud suspect répond (ou pas)
let logs = node_suspect.respond_to_consistency_challenge(&challenge)?;

// 3. Vérifier la cohérence des logs reçus
let is_valid = node_auditor.verify_consistency(suspect_id, &logs);

// 4. Si invalide, créer un rapport
if !is_valid {
    let issues = vec!["Chaîne de hash brisée à l'entrée 42".to_string()];
    node_auditor.create_audit_report(suspect_id, &issues)?;
}
```

### Scénario 2 : Audit avec témoins

```rust
// 1. Demander les logs au nœud suspect
let challenge = node_auditor.send_consistency_challenge(suspect_id, 1, 50)?;
let suspect_logs = node_suspect.respond_to_consistency_challenge(&challenge)?;

// 2. Récupérer les logs des témoins
let mut witness_logs = HashMap::new();

for witness_id in [witness1_id, witness2_id, witness3_id] {
    let challenge = node_auditor.send_consistency_challenge(witness_id, 1, 50)?;
    let logs = // réponse du témoin
    witness_logs.insert(witness_id, logs);
}

// 3. Détecter les incohérences
let issues = node_auditor.detect_inconsistency(suspect_id, &suspect_logs, &witness_logs);

// 4. Créer un rapport si nécessaire
if !issues.is_empty() {
    node_auditor.create_audit_report(suspect_id, &issues)?;
}
```

### Scénario 3 : Vérification croisée entre deux nœuds

```rust
// Vérifier que Alice et Bob sont cohérents sur leurs échanges
let result = node_auditor.cross_check_consistency(alice_id, bob_id, 1, 100)?;

if !result {
    println!("⚠️  Incohérence détectée entre Alice et Bob !");
    // Investiguer plus en détail...
}
```

---

## Propriétés cryptographiques

### 1. Immuabilité de l'historique

Grâce à la chaîne de hash : `h_k = H(h_{k-1} || s_k || log_type || c_k)`

- Toute modification d'une entrée passée casse la chaîne
- Le nœud ne peut pas modifier son historique sans être détecté
- La vérification est O(n) où n est le nombre d'entrées

### 2. Non-répudiation

Grâce aux signatures Ed25519 :

- Chaque entrée est signée : `sig = Ed25519_Sign(s_k || h_k)`
- Le nœud ne peut pas nier avoir créé une entrée
- Les signatures sont vérifiables par tous les autres nœuds

### 3. Détection de falsification

Si un nœud tente de :
- **Supprimer une entrée** : Les numéros de séquence deviennent non-consécutifs
- **Modifier une entrée** : Le hash calculé ne correspond plus au hash stocké
- **Forger une entrée** : La signature Ed25519 sera invalide
- **Réordonner les entrées** : La chaîne de hash sera brisée

### 4. Consensus par témoins

En comparant avec plusieurs témoins :
- Un nœud isolé ne peut pas mentir sans être détecté
- La majorité des témoins honnêtes expose les nœuds malhonnêtes
- Résistance aux collusions si > 50% de témoins honnêtes

---

## État actuel vs. Implémentation future

### ✅ Actuellement implémenté

- Structure de base des challenges
- Envoi et réception de challenges
- Vérification de la consécutivité des `s_k`
- Détection de divergences de taille
- Création de rapports d'audit
- Logging de toutes les opérations

### 🔄 En cours d'implémentation

- Vérification complète de la chaîne de hash SHA-256
- Vérification des signatures Ed25519
- Recalcul des commitments pour validation

### 📋 À implémenter

- **Comparaison complète des hash** : Vérifier entry par entry
- **Système de témoins distribués** : Gestion automatique des témoins
- **Agrégation des preuves** : Combiner plusieurs rapports d'audit
- **Mécanisme de pénalité** : Sanctionner les nœuds malhonnêtes
- **Optimisations** :
  - Cache des logs fréquemment demandés
  - Indexation pour recherche rapide
  - Pagination pour grandes plages
  - Compression des logs archivés

---

## Tests

Des tests unitaires sont inclus dans `consistency.rs` :

```rust
#[test]
fn test_consistency_challenge() {
    // Teste l'envoi d'un challenge
}

#[test]
fn test_verify_consistency_empty() {
    // Teste le rejet de logs vides
}
```

**Exécuter les tests** :
```bash
cargo test consistency
```

**Ajouter des tests** :
```bash
# Test avec logs valides
cargo test test_verify_consistency_valid

# Test avec chaîne de hash brisée
cargo test test_verify_consistency_broken_chain

# Test avec signatures invalides
cargo test test_verify_consistency_invalid_signature
```

---

## Intégration avec les autres protocoles

### Commitment

Le protocole de consistency **vérifie** que les commitments créés par le protocole de commitment sont cohérents dans le temps :
- Les hash de commitment doivent être recalculables
- Les signatures doivent être valides
- La chaîne de hash ne doit jamais être brisée

### Network (futur)

Les challenges et réponses seront transmis via le module réseau :
```rust
// Envoyer un challenge via le réseau
network.send_message(target_id, MessageType::ConsistencyChallenge, challenge)?;

// Recevoir une réponse
let response = network.receive_message(target_id, MessageType::ConsistencyResponse)?;
```

---

## Configuration recommandée

### Taille des challenges

```rust
// Petits challenges pour vérifications fréquentes
let challenge = node.send_consistency_challenge(target_id, recent_seq - 10, recent_seq)?;

// Grands challenges pour audits complets (risque de surcharge)
let challenge = node.send_consistency_challenge(target_id, 1, 10000)?;
```

### Fréquence des audits

- **Audits légers** : Toutes les 100 entrées (vérifier les 10 dernières)
- **Audits complets** : Toutes les 1000 entrées (vérifier tout l'historique)
- **Audits d'urgence** : En cas de suspicion (vérifier avec témoins)

### Nombre de témoins

- **Minimum recommandé** : 3 témoins
- **Optimal** : 5-7 témoins
- **Maximum pratique** : 10 témoins (au-delà, overhead important)

---

## Sécurité et considérations

### Attaques possibles

1. **Denial of Service** : Spammer de challenges
   - **Mitigation** : Rate limiting sur les challenges

2. **Collusion de témoins** : Témoins malhonnêtes s'accordent pour mentir
   - **Mitigation** : Majorité honnête requise (> 50%)

3. **Révision d'historique** : Tenter de modifier des entrées passées
   - **Mitigation** : Chaîne de hash détecte toute modification

4. **Forge de signatures** : Tenter de créer de fausses entrées
   - **Mitigation** : Ed25519 résiste aux forges

### Bonnes pratiques

- ✅ Toujours vérifier avec plusieurs témoins
- ✅ Conserver les rapports d'audit de manière permanente
- ✅ Implémenter un timeout pour les réponses
- ✅ Logger toutes les opérations d'audit
- ❌ Ne jamais faire confiance à un seul nœud
- ❌ Ne pas accepter de logs non-signés

---

## Compilation et tests

```bash
# Compiler le projet
cargo build

# Exécuter les tests de consistency
cargo test consistency

# Exécuter l'application de démonstration
cargo run

# Vérifier le code (warnings, etc.)
cargo clippy
```

---

## Références

- [PeerReview: Practical Accountability for Distributed Systems](https://www.cs.rice.edu/~eugeneng/papers/SOSP07.pdf) - Paper original
- `PROTOCOL_README.md` - Documentation du protocole de commitment
- `src/journal/logger.rs` - Implémentation du Logger avec SHA-256 et Ed25519
- `src/protocols/commitment.rs` - Protocole de commitment

---

## Contributeurs

Développé dans le cadre du projet PeerReview en Rust.
