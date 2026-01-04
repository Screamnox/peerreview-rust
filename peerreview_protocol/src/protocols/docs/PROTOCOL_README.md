# Protocole PeerReview - Implémentation Rust

## Vue d'ensemble

Cette implémentation du protocole PeerReview en Rust suit les 4 algorithmes décrits dans la spécification :

1. **Algorithm 1** : Envoi d'un message
2. **Algorithm 2** : Réception d'un message
3. **Algorithm 3** : Vérification d'un message SEND
4. **Algorithm 4** : Vérification d'un acquittement (message RECV)

## Architecture

### Structure des fichiers

```
src/
├── journal/
│   ├── entry.rs          # LogEntry avec hash et signature
│   ├── logger.rs         # Logger avec cryptographie Ed25519
│   ├── file_utils.rs     # Utilitaires pour manipuler les fichiers
│   └── mod.rs
├── protocols/
│   ├── node.rs           # Structures de base (PeerReviewNode, PeerReviewMessage)
│   ├── commitment.rs     # Implémentation du protocole de commitment
│   ├── consistency.rs    # Implémentation du protocole de consistency
│   └── mod.rs
├── lib.rs
└── main.rs              # Démonstration du protocole
```

### Concepts clés

#### Commitment
Le commitment est une structure logique qui représente l'engagement d'un nœud :
- **ck** (commitment SEND) = `{destinataire, message}` - sans numéro de séquence
- **cl** (commitment RECV) = `{émetteur, seq_num, message}` - avec numéro de séquence

**Important** : Le protocole de commitment ne crée PAS de hash lui-même. C'est le Logger qui calcule les hash cryptographiques.

#### Journal (Logger)
Le journal est responsable de :
- Créer les entrées de log avec `LogEntry`
- **Calculer automatiquement les hash cryptographiques (SHA-256)**
- **Générer les signatures Ed25519 (64 bytes)**
- Sérialiser/désérialiser les entrées dans un fichier

Chaque entrée `LogEntry` contient :
- `s_k` : numéro séquentiel (incrémenté automatiquement)
- `log_type` : SEND ou RECV
- `corr` : correspondant (destinataire ou émetteur)
- `s_k_corr` : numéro de séquence du correspondant (pour RECV)
- `hash` : **hash SHA-256 de l'entrée** `H(h_prev || s_k || log_type || H(c_k))`
- `sig` : **signature Ed25519 (64 bytes)** de `(s_k || hash)`
- `msg` : message en base64

## Algorithmes implémentés

### Algorithm 1 : Envoi d'un message

```rust
pub fn send_message(&mut self, receiver_id: u32, message: &str) -> PeerReviewMessage
```

**Étapes détaillées** :

1. **Sauvegarder h_{k-1}** : Le hash de l'entrée précédente (avant de logger)
   ```rust
   let prev_hash_for_msg = self.prev_hash;
   ```

2. **Logger l'entrée SEND** : Le Logger calcule automatiquement :
   - **c_k** = `H(destinataire || message)` (hash du commitment)
   - **h_k** = `H(h_{k-1} || s_k || SEND || c_k)` (hash de l'entrée)
   - **α_k** = `Ed25519_Sign(s_k || h_k)` (signature Ed25519)
   ```rust
   self.logger.log_send(receiver_id, message)?;
   ```

3. **Récupérer les informations** : Lire l'entrée créée depuis le Logger
   ```rust
   let logs = self.logger.get_log(1)?;
   let log_entry = &logs[0];
   let current_seq = log_entry.s_k;      // s_k
   let current_hash = log_entry.hash;     // h_k
   let current_sig = log_entry.sig;       // α_k (64 bytes)
   ```

4. **Mettre à jour prev_hash** : Le h_k actuel devient le h_{k-1} pour la prochaine entrée
   ```rust
   self.prev_hash = current_hash;
   ```

5. **Créer le message SEND** : Message à envoyer sur le réseau `{s_k, h_{k-1}, α_k, m}`
   ```rust
   PeerReviewMessage {
       msg_type: MessageType::Send,
       seq_num: current_seq,           // s_k
       prev_hash: prev_hash_for_msg,   // h_{k-1} (hash de l'entrée PRÉCÉDENTE)
       signature: current_sig,         // α_k (signature de l'entrée ACTUELLE)
       dest: receiver_id,
       payload: message.to_string(),
   }
   ```

**Point clé** : Le message contient `h_{k-1}` (hash PRÉCÉDENT) et non `h_k` (hash actuel). Cela permet au destinataire de vérifier la chaîne de hash.

---

### Algorithm 3 : Vérification d'un message SEND

```rust
pub fn verify_send_message(&self, msg: &PeerReviewMessage, sender_id: u32, 
    sender_public_key: &ed25519_dalek::PublicKey) -> bool
```

**Étapes détaillées** :

1. **Vérifier le type** : Le message doit être de type SEND

2. **Extraire les données** :
   - `h_{k-1}` = `msg.prev_hash` (hash précédent)
   - `s_k` = `msg.seq_num` (numéro de séquence)
   - `α_k` = `msg.signature` (signature Ed25519, 64 bytes)
   - `m` = `msg.payload` (message)

3. **Calculer ĥ_k** (hash attendu) :
   - D'abord calculer `c_k = H(destinataire || message)`
   ```rust
   let mut hasher = Sha256::new();
   hasher.update(msg.dest.to_be_bytes());
   hasher.update(message.as_bytes());
   let c_k = hasher.finalize();
   ```
   
   - Puis calculer `ĥ_k = H(h_{k-1} || s_k || SEND || c_k)`
   ```rust
   hasher = Sha256::new();
   hasher.update(prev_hash);
   hasher.update(seq_num.to_be_bytes());
   hasher.update((LogType::Send as u8).to_be_bytes());
   hasher.update(c_k);
   let h_k_computed = hasher.finalize();
   ```

4. **Vérifier la signature Ed25519** :
   ```rust
   let mut signed_data = [0u8; 40];
   signed_data[..8].copy_from_slice(&seq_num.to_be_bytes());
   signed_data[8..].copy_from_slice(&h_k_computed);
   
   sender_public_key.verify(&signed_data, &signature_obj).is_ok()
   ```

**Point clé** : La vérification de signature prouve mathématiquement que `h_k == ĥ_k`. Si `verify()` réussit, alors le hash calculé par l'émetteur correspond exactement au hash que nous avons recalculé.

---

### Algorithm 2 : Réception d'un message

```rust
pub fn receive_message(&mut self, msg: &PeerReviewMessage, sender_id: u32) 
    -> std::io::Result<Option<PeerReviewMessage>>
```

**Étapes détaillées** :

1. **Récupérer la clé publique** : Obtenir la clé publique de l'émetteur
   ```rust
   let sender_public_key = self.peer_public_keys.get(&sender_id)?;
   ```

2. **Vérifier le message** : Utiliser Algorithm 3 pour valider
   ```rust
   let is_valid = self.verify_send_message(msg, sender_id, sender_public_key);
   ```

3. **Si invalide** : Créer un challenge d'audit et retourner `None`
   ```rust
   if !is_valid {
       self.create_audit_challenge(sender_id, "Message SEND invalide")?;
       return Ok(None);
   }
   ```

4. **Logger l'entrée RECV** : Le Logger calcule :
   - **c_l** = `H(émetteur || s_k_émetteur || message)`
   - **h_l** = `H(h_{l-1} || s_l || RECV || c_l)`
   - Signature fournie par l'émetteur (stockée telle quelle)
   ```rust
   self.logger.log_recv(sender_id, msg.seq_num, msg.signature, &msg.payload)?;
   let logs_recv = self.logger.get_log(1)?;
   let recv_hash = logs_recv[0].hash;  // h_l
   ```

5. **Logger l'entrée SEND (acquittement)** : Le Logger calcule :
   - **c_{l+1}** = `H(destinataire_ack || message_vide)`
   - **h_{l+1}** = `H(h_l || s_{l+1} || SEND || c_{l+1})`
   - **α_{l+1}** = `Ed25519_Sign(s_{l+1} || h_{l+1})`
   ```rust
   self.logger.log_send(sender_id, "")?;
   let logs_ack = self.logger.get_log(1)?;
   self.prev_hash = logs_ack[0].hash;
   ```

6. **Créer le message d'acquittement** : `{s_{l+1}, h_l, α_{l+1}}`
   ```rust
   PeerReviewMessage {
       msg_type: MessageType::Send,
       seq_num: logs_ack[0].s_k,        // s_{l+1}
       prev_hash: recv_hash,             // h_l (hash de l'entrée RECV)
       signature: logs_ack[0].sig,       // α_{l+1}
       dest: sender_id,
       payload: String::new(),
   }
   ```

**Point clé** : L'acquittement contient `h_l` (hash de l'entrée RECV) dans `prev_hash`. Cela prouve au nœud émetteur que son message a bien été reçu et loggé.

---

### Algorithm 4 : Vérification d'un acquittement

```rust
pub fn verify_recv_message(&mut self, ack_msg: &PeerReviewMessage, receiver_id: u32,
    receiver_public_key: &ed25519_dalek::PublicKey, _original_seq_num: usize,
    _original_message: &str) -> bool
```

**Étapes détaillées** :

1. **Extraire les données** :
   - `s_{l+1}` = `ack_msg.seq_num` (numéro de séquence de l'acquittement SEND)
   - `h_l` = `ack_msg.prev_hash` (hash de l'entrée RECV)
   - `α_{l+1}` = `ack_msg.signature` (signature de l'acquittement)

2. **Calculer ĥ_{l+1}** (hash attendu de l'acquittement) :
   - D'abord calculer `c_{l+1} = H(self.node_id || "")` (commitment de l'acquittement)
   ```rust
   let mut hasher = Sha256::new();
   hasher.update(self.node_id.to_be_bytes());
   hasher.update(b"");
   let c_l_plus_1 = hasher.finalize();
   ```
   
   - Puis calculer `ĥ_{l+1} = H(h_l || s_{l+1} || SEND || c_{l+1})`
   ```rust
   hasher = Sha256::new();
   hasher.update(recv_hash);
   hasher.update(ack_seq_num.to_be_bytes());
   hasher.update((LogType::Send as u8).to_be_bytes());
   hasher.update(c_l_plus_1);
   let h_l_plus_1_computed = hasher.finalize();
   ```

3. **Vérifier la signature Ed25519** :
   ```rust
   let mut signed_data = [0u8; 40];
   signed_data[..8].copy_from_slice(&ack_seq_num.to_be_bytes());
   signed_data[8..].copy_from_slice(&h_l_plus_1_computed);
   
   receiver_public_key.verify(&signed_data, &signature_obj).is_ok()
   ```

**Point clé** : Si la signature est valide, cela prouve que :
1. Le destinataire a bien reçu notre message (entrée RECV créée avec hash `h_l`)
2. Le destinataire a créé un acquittement légitime (entrée SEND avec hash `h_{l+1}`)

---

## Fonctions cryptographiques (dans Logger)

### Calcul du hash de commitment

Le Logger calcule automatiquement le hash du commitment selon le type d'entrée :

**Pour SEND** : `c_k = H(destinataire || message)`
```rust
// Dans logger.log_send()
let mut hasher = Sha256::new();
hasher.update(correspondent.to_be_bytes());
hasher.update(msg.as_bytes());
let c_k = hasher.finalize();
```

**Pour RECV** : `c_l = H(émetteur || s_k_émetteur || message)`
```rust
// Dans logger.log_recv()
let mut hasher = Sha256::new();
hasher.update(correspondent.to_be_bytes());
hasher.update(s_k_corr.to_be_bytes());
hasher.update(msg.as_bytes());
let c_k = hasher.finalize();
```

### Calcul du hash d'entrée

Le Logger calcule le hash de chaque entrée de journal : `h_k = H(h_{k-1} || s_k || log_type || c_k)`

```rust
hasher = Sha256::new();
hasher.update(self.hash);                        // h_{k-1} (hash précédent)
hasher.update(self.s_k.to_be_bytes());           // s_k (numéro de séquence)
hasher.update((log_type as u8).to_be_bytes());   // SEND ou RECV
hasher.update(c_k);                               // hash du commitment
let hash = hasher.finalize();
```

### Génération de signature Ed25519

Le Logger utilise une paire de clés Ed25519 pour signer chaque entrée :

**Données signées** : `(s_k || h_k)` (8 bytes + 32 bytes = 40 bytes)
**Signature** : 64 bytes (Ed25519)

```rust
let mut data_to_sign = [0u8; 40];
data_to_sign[..8].copy_from_slice(&self.s_k.to_be_bytes());
data_to_sign[8..].copy_from_slice(&hash);

let sig = self.keypair.sign(&data_to_sign);  // 64 bytes
```

---

## Gestion des challenges

### Challenge d'audit
Créé quand un nœud détecte un message avec une signature invalide :
```rust
fn create_audit_challenge(&mut self, target_node: u32, reason: &str)
```

Le challenge est loggé comme une entrée SEND normale avec un message spécial `"CHALLENGE_AUDIT: {raison}"`.

### Challenge d'envoi (timeout)
Créé quand un nœud ne reçoit pas d'acquittement dans le délai imparti :
```rust
pub fn send_with_acknowledgment(&mut self, receiver_id: u32, message: &str, 
    timeout: Duration, ack_received: Option<PeerReviewMessage>) -> bool
```

Le challenge est loggé comme une entrée SEND avec le message `"CHALLENGE_SEND: Timeout - pas d'acquittement"`.

---

## Structure PeerReviewMessage

Le message échangé sur le réseau contient :

```rust
pub struct PeerReviewMessage {
    pub msg_type: MessageType,      // Send ou Recv
    pub seq_num: usize,              // s_k (numéro de séquence)
    pub prev_hash: [u8; 32],         // h_{k-1} (hash précédent)
    pub signature: [u8; 64],         // α_k (signature Ed25519)
    pub dest: u32,                   // destinataire
    pub payload: String,             // message
}
```

---

## Utilisation

```rust
// Créer deux nœuds avec leur Logger respectif
let logger1 = Logger::new("node1.log", 5000, 300)?;
let logger2 = Logger::new("node2.log", 5000, 300)?;
let mut node1 = PeerReviewNode::new(1, logger1, [1; 32]);
let mut node2 = PeerReviewNode::new(2, logger2, [2; 32]);

// Enregistrer les clés publiques mutuelles (Ed25519)
let node1_public_key = node1.logger.get_public_key().clone();
let node2_public_key = node2.logger.get_public_key().clone();
node1.register_peer(2, node2_public_key);
node2.register_peer(1, node1_public_key);

// Nœud 1 envoie un message à Nœud 2 (Algorithm 1)
let send_msg = node1.send_message(2, "Hello")?;

// Nœud 2 reçoit et vérifie le message (Algorithm 2 et 3)
let ack_msg = node2.receive_message(&send_msg, 1)?;

// Nœud 1 vérifie l'acquittement (Algorithm 4)
if let Some(ack) = ack_msg {
    let receiver_public_key = node1.peer_public_keys.get(&2).unwrap();
    let is_valid = node1.verify_recv_message(&ack, 2, receiver_public_key, 
                                               send_msg.seq_num, "Hello");
    println!("Acquittement valide: {}", is_valid);
}
```

---

## Configuration du Logger

Le Logger nécessite une taille de ligne minimale suffisante pour stocker :
- `s_k` : ~10 caractères
- `log_type` : 1 caractère  
- `corr` : ~10 caractères
- `s_k_corr` : ~10 caractères
- `hash` : 64 caractères (32 bytes en hex)
- `sig` : 128 caractères (64 bytes en hex)
- `msg` : variable (encodé en base64)

**Minimum recommandé** : 300 caractères par ligne

```rust
Logger::new("node.log", 5000, 300)?;
//                             ^^^
//                         min_line_size
```

---

## Compilation et exécution

```bash
# Compiler
cargo build

# Exécuter la démonstration
cargo run

# Exécuter les tests
cargo test
```

---

## Dépendances

- `sha2`: Pour les fonctions de hachage SHA-256
- `hex`: Pour encoder/décoder les hash en hexadécimal
- `base64`: Pour encoder/décoder les messages
- `ed25519-dalek`: Pour les signatures Ed25519

---

## Points importants

1. **Le protocole ne calcule PAS les hash** - c'est le Logger qui les génère automatiquement
2. **Les signatures sont Ed25519 (64 bytes)** - pas des MAC simples
3. **Le message SEND contient h_{k-1}** (hash précédent) - pas le hash actuel
4. **L'acquittement contient h_l** (hash de l'entrée RECV) - cela prouve la réception
5. **La vérification de signature prouve l'égalité des hash** - `verify()` réussit ⟺ `h_k == ĥ_k`
