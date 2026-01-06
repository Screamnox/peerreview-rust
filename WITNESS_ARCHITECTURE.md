# Architecture des Témoins (Witnesses)

## Vue d'ensemble

Dans le protocole PeerReview, chaque nœud est surveillé par un ensemble de **témoins** (witnesses). Cette architecture a été conçue pour que **tous les nœuds connaissent la configuration complète des témoins**, ce qui permet une vérification distribuée et une détection collaborative de comportements malveillants.

## Concepts clés

### Configuration Globale Statique

- La configuration des témoins est **définie à l'avance** et **ne change jamais** pendant l'exécution
- Tous les nœuds partagent la **même configuration globale** via `WitnessConfig`
- Cette configuration est partagée entre les nœuds via `Arc<WitnessConfig>` (pointeur avec comptage de références)

### Structure `WitnessConfig`

```rust
pub struct WitnessConfig {
    witnesses: HashMap<u32, Vec<u32>>
}
```

- **Clé** : `node_id` du nœud surveillé
- **Valeur** : Liste des `node_id` de ses témoins

### Avantages de cette architecture

1. **Transparence totale** : Chaque nœud connaît qui surveille qui
2. **Audit distribué** : Un nœud peut envoyer des challenges aux témoins d'un autre nœud
3. **Configuration simple** : Définie une seule fois au démarrage
4. **Partage efficace** : Un seul objet `WitnessConfig` partagé via `Arc`

## Utilisation

### 1. Créer la configuration globale

```rust
use std::sync::Arc;
use protocols::node::WitnessConfig;

// Créer la configuration
let mut witness_config = WitnessConfig::new();

// Définir les témoins de chaque nœud
witness_config.set_witnesses(1, vec![3, 4]);  // Nœud 1 → Témoins 3 et 4
witness_config.set_witnesses(2, vec![3, 5]);  // Nœud 2 → Témoins 3 et 5

// Partager la configuration entre tous les nœuds
let witness_config = Arc::new(witness_config);
```

### 2. Créer les nœuds avec la configuration

```rust
let node1 = PeerReviewNode::new(1, logger1, Arc::clone(&witness_config));
let node2 = PeerReviewNode::new(2, logger2, Arc::clone(&witness_config));
let witness3 = PeerReviewNode::new(3, logger3, Arc::clone(&witness_config));
```

### 3. Accéder aux informations de témoignage

#### Depuis n'importe quel nœud

```rust
// Obtenir les témoins de ce nœud
let my_witnesses = node1.get_witnesses();  // → [3, 4]

// Obtenir les nœuds que ce nœud surveille
let witnessing = node3.get_witnessing_for();  // → [1, 2]

// Obtenir les témoins d'un AUTRE nœud
let node2_witnesses = node1.get_witnesses_of(2);  // → [3, 5]
```

## Fonctionnement dans le Protocole

### Envoi de Hash aux Témoins

Lors de chaque opération qui crée une entrée dans le journal, le hash est automatiquement envoyé aux témoins :

```rust
// Dans send_message()
self.logger.log_send(receiver_id, message)?;
// ...
self.send_hash_to_witnesses(seq_num, hash)?;  // ← Envoi automatique
```

La méthode `send_hash_to_witnesses()` utilise `self.get_witnesses()` pour obtenir la liste des témoins :

```rust
fn send_hash_to_witnesses(&self, seq_num: usize, hash: [u8; 32]) -> std::io::Result<()> {
    let witnesses = self.get_witnesses();  // Utilise la config globale
    
    for witness_id in &witnesses {
        println!("[Nœud {}] → Témoin {} : hash {}", 
                 self.node_id, witness_id, hex::encode(hash));
        // TODO: Implémentation réseau réelle
    }
    Ok(())
}
```

### Challenges d'Audit

Lorsqu'un nœud détecte un comportement suspect, il peut créer un challenge et l'envoyer non seulement à ses propres témoins, mais aussi **aux témoins du nœud suspect** :

```rust
fn create_audit_challenge(&mut self, target_node: u32, reason: &str) -> std::io::Result<()> {
    // ... création du challenge ...
    
    // Envoyer aux témoins de ce nœud
    self.send_hash_to_witnesses(challenge_seq, challenge_hash)?;
    
    // Envoyer aux témoins du nœud ciblé
    let target_witnesses = self.get_witnesses_of(target_node);
    for witness_id in &target_witnesses {
        // Envoyer le challenge au témoin
    }
    
    Ok(())
}
```

## API Complète

### `WitnessConfig`

| Méthode | Description |
|---------|-------------|
| `new()` | Crée une configuration vide |
| `set_witnesses(node_id, witnesses)` | Définit les témoins d'un nœud |
| `add_witness(node_id, witness_id)` | Ajoute un témoin à un nœud |
| `get_witnesses(node_id)` | Retourne les témoins d'un nœud |
| `is_witness(node_id, witness_id)` | Vérifie si witness_id est témoin de node_id |
| `get_witnessing_for(witness_id)` | Retourne tous les nœuds surveillés par witness_id |

### `PeerReviewNode`

| Méthode | Description |
|---------|-------------|
| `new(node_id, logger, witness_config)` | Crée un nœud avec la config globale |
| `get_witnesses()` | Retourne les témoins de CE nœud |
| `get_witnessing_for()` | Retourne les nœuds que CE nœud surveille |
| `is_witness(node_id)` | Vérifie si node_id est témoin de ce nœud |
| `is_witnessing(node_id)` | Vérifie si ce nœud surveille node_id |
| `get_witnesses_of(node_id)` | Retourne les témoins d'UN AUTRE nœud |

## Exemple Complet

```rust
use std::sync::Arc;
use peerreview_rust::protocols::node::{WitnessConfig, PeerReviewNode};
use peerreview_rust::journal::Logger;

fn main() -> std::io::Result<()> {
    // 1. Configuration des témoins
    let mut witness_config = WitnessConfig::new();
    witness_config.set_witnesses(1, vec![3, 4]);
    witness_config.set_witnesses(2, vec![3, 5]);
    witness_config.set_witnesses(3, vec![4, 5]);
    let witness_config = Arc::new(witness_config);
    
    // 2. Créer les nœuds
    let logger1 = Logger::new("node1.log", 5000, 200)?;
    let logger2 = Logger::new("node2.log", 5000, 200)?;
    let logger3 = Logger::new("node3.log", 5000, 200)?;
    
    let mut node1 = PeerReviewNode::new(1, logger1, Arc::clone(&witness_config));
    let mut node2 = PeerReviewNode::new(2, logger2, Arc::clone(&witness_config));
    let mut node3 = PeerReviewNode::new(3, logger3, Arc::clone(&witness_config));
    
    // 3. Afficher la configuration
    println!("Nœud 1 surveillé par : {:?}", node1.get_witnesses());        // [3, 4]
    println!("Nœud 3 surveille : {:?}", node3.get_witnessing_for());       // [1, 2]
    println!("Témoins du nœud 2 : {:?}", node1.get_witnesses_of(2));       // [3, 5]
    
    // 4. Utilisation normale - les hash sont automatiquement envoyés aux témoins
    node1.send_message(2, "Hello")?;
    // → Hash envoyé automatiquement aux témoins 3 et 4
    
    Ok(())
}
```

## Notes d'Implémentation

### Actuel (Simulation)

- Les hash sont **affichés dans la console** avec `println!`
- Les témoins ne **reçoivent pas réellement** les hash (TODO réseau)
- La détection de fraude par les témoins n'est **pas encore implémentée**

### Futur (Réseau Réel)

Pour une implémentation réseau complète, il faudra :

1. **Stocker les hash reçus** : Chaque témoin doit maintenir un registre des hash reçus
2. **Comparer avec les logs** : Lors d'un audit, comparer les hash du témoin avec ceux du nœud
3. **Détecter les divergences** : Si les hash diffèrent, signaler une fraude
4. **Protocole de communication** : Implémenter l'envoi réseau réel des hash aux témoins

## Sécurité

### Avantages

- **Redondance** : Plusieurs témoins par nœud
- **Détection distribuée** : Les témoins peuvent se coordonner
- **Transparence** : Impossible de cacher qui surveille qui
- **Immuabilité** : Configuration fixe empêche la manipulation

### Considérations

- Le nombre de témoins impacte les **performances** (plus de messages réseau)
- Les témoins doivent être **fiables** (pas de collusion)
- Configuration optimale : **2-3 témoins par nœud** selon la littérature PeerReview
