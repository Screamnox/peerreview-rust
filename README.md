# PeerReview-Rust - Système NFS Distribué

Infrastructure distribuée pour l'intégration de PeerReview avec application NFS (Network File System) déterministe.

---

## Table of Contents

- [Introduction](#introduction)
- [Applications](#applications)
  - [Application Gossip (Multi-arbres)](#application-gossip-multi-arbres)
  - [Application NFS](#application-nfs)
- [Architecture NFS](#architecture-nfs)
- [Démarrage Rapide - NFS](#démarrage-rapide---nfs)
- [Utilisation de l'API NFS](#utilisation-de-lapi-nfs)
- [Configuration](#configuration)
- [Structure du Dépôt](#structure-du-dépôt)
- [Déterminisme et PeerReview](#déterminisme-et-peerreview)
- [Références](#références)

---

## Introduction

Ce projet implémente une infrastructure distribuée en Rust destinée à servir de base au protocole PeerReview (Haeberlen et al., SOSP 2007). Il contient deux applications démontrant la versatilité du framework :

1. **Gossip** : Diffusion structurée multi-arbres
2. **NFS** : Système de fichiers réseau déterministe

---

## Applications

### Application Gossip (Multi-arbres)

Infrastructure distribuée avec :
- Diffusion structurée en plusieurs arbres (multi-tree)
- Réseau TCP interne léger et performant
- API HTTP simplifiée pour interactions extérieures
- Cluster Docker à 10 nœuds

**Voir la documentation complète** : [Documentation Gossip détaillée](docs/gossip.md) *(fichier à créer si besoin)*

### Application NFS

Système de fichiers réseau avec :
- **Horloge logique de Lamport** pour déterminisme complet
- **Opérations** : READ, WRITE, DELETE, LIST
- **Protocoles duaux** : TCP binaire + API HTTP REST
- **Concurrence déterministe** : verrous par fichier avec DashMap
- **Traçabilité complète** : historique des opérations pour replay

---

## Architecture NFS

```
┌─────────────┐                  ┌─────────────┐
│   Client    │  ──── TCP ────>  │  NFS Server │
│             │  <─── RPC ─────  │             │
└─────────────┘                  └──────┬──────┘
                                        │
                                        ▼
                                  ┌──────────┐
                                  │  Volume  │
                                  │  /data/  │
                                  └──────────┘
```

### Composants NFS
- **Clients** : Émettent des requêtes NFS
- **Serveurs** : Traitent les requêtes et manipulent les fichiers
- **Volumes** : Stockage persistant isolé par serveur

### Stack Technologique
- **Langage** : Rust 1.82
- **Runtime Async** : Tokio
- **HTTP Framework** : Axum 0.7
- **Sérialisation** : Bincode (TCP) + Serde JSON (HTTP)
- **Concurrence** : DashMap + Parking Lot
- **Infrastructure** : Docker + Docker Compose

---

## Démarrage Rapide - NFS

### Prérequis
- Rust 1.82+
- Docker + Docker Compose
- Cargo (inclus avec Rust)

### Installation et Lancement

#### Option 1 : Compilation Locale
```bash
# Compiler uniquement le nœud NFS
cargo build --release --package nfs_node

# Lancer un serveur
cargo run --package nfs_node -- --config configs/docker/nfs_server1.yaml
```

#### Option 2 : Déploiement Docker (Recommandé)
```bash
# Construire l'image
docker build -f docker/Dockerfile.nfs -t nfs-node .

# Lancer le cluster (3 serveurs)
docker compose -f docker/nfs-compose.yml up
```

**Les serveurs démarrent sur :**
- Serveur 1 : TCP `9091`, HTTP `8091`
- Serveur 2 : TCP `9092`, HTTP `8092`
- Serveur 3 : TCP `9093`, HTTP `8093`

---

## Utilisation de l'API NFS

### Écrire un Fichier
```bash
curl -X POST http://localhost:8091/write \
  -H "Content-Type: application/json" \
  -d '{
    "file_path": "test.txt",
    "offset": 0,
    "data": [72, 101, 108, 108, 111]
  }'
```

### Lire un Fichier
```bash
curl -X POST http://localhost:8091/read \
  -H "Content-Type: application/json" \
  -d '{
    "file_path": "test.txt",
    "offset": 0,
    "length": 100
  }'
```

### Lister les Fichiers
```bash
curl -X POST http://localhost:8091/list \
  -H "Content-Type: application/json" \
  -d '{"dir_path": "/"}'
```

### Supprimer un Fichier
```bash
curl -X POST http://localhost:8091/delete \
  -H "Content-Type: application/json" \
  -d '{"file_path": "test.txt"}'
```

### Statistiques du Serveur
```bash
curl http://localhost:8091/stats
```

---

## Configuration

### Exemple de Configuration NFS (`nfs_server1.yaml`)

```yaml
node_id: "nfs_server_1"
tcp_port: 9091
http_port: 8091
volume_root: "/data/nfs_server_1"
peers:
  - node_id: "nfs_server_2"
    address: "nfs_server_2:9092"
  - node_id: "nfs_server_3"
    address: "nfs_server_3:9093"
```

### Exemple de Configuration Gossip (`cluster.yaml`)

```yaml
fanout: 3
num_trees: 3

nodes:
  - { id: "node1", addr: "node1:7001" }
  - { id: "node2", addr: "node2:7001" }
  # ...
```

---

## Structure du Dépôt

```
peerreview-rust/
│
├─ apps/
│   ├─ gossip_node/         # Application Gossip multi-arbres
│   │   └─ src/main.rs
│   └─ nfs_node/            # Application NFS
│       ├─ src/main.rs      # Serveur NFS (607 lignes)
│       └─ Cargo.toml
│
├─ crates/
│   └─ common_proto/        # Protocoles partagés
│       ├─ src/
│       │   ├─ lib.rs
│       │   └─ nfs.rs       # Définitions NFS (121 lignes)
│       └─ Cargo.toml
│
├─ configs/
│   └─ docker/              # Configurations des serveurs
│       ├─ cluster.yaml     # Config Gossip (10 nœuds)
│       ├─ nfs_cluster.yaml
│       ├─ nfs_server1.yaml
│       ├─ nfs_server2.yaml
│       └─ nfs_server3.yaml
│
├─ docker/
│   ├─ Dockerfile           # Image Gossip
│   ├─ Dockerfile.nfs       # Multi-stage build NFS
│   └─ nfs-compose.yml      # Orchestration cluster NFS
│
└─ README.md
```

---

## Déterminisme et PeerReview

### Garanties NFS pour PeerReview

L'application NFS garantit un déterminisme complet :

| Exigence PeerReview | Implémentation NFS |
|---------------------|---------------------|
| Horloge déterministe | `DeterministicClock` (Lamport) |
| Ordre des opérations | Verrous + Timestamps |
| Historique | `VecDeque<String>` (100 dernières ops) |
| RPC traçable | UUID v4 par requête |
| Pas de sources aléatoires | Pas de `SystemTime::now()` |
| Replay exact | Même timestamps → même ordre |

### Horloge Logique de Lamport

```rust
pub struct DeterministicClock {
    current_time: AtomicU64,
}

impl DeterministicClock {
    pub fn update_time(&self, time: u64) {
        self.current_time.fetch_max(time, Ordering::SeqCst);
    }
}
```

**Protocole** :
1. Client envoie requête avec `timestamp=T`
2. Serveur exécute `clock.update_time(T)` → `clock = max(clock, T)`
3. Serveur exécute opération
4. Serveur répond avec `timestamp=clock.now()`

### Exemple d'Historique
```
WRITE test.txt -> OK (42 bytes)
READ config.yaml -> ERROR: No such file
DELETE old.log -> OK
LIST /data -> OK (15 entries)
```

---

## Tests

```bash
# Tests unitaires NFS
cargo test --package nfs_node

# Tests d'intégration
cargo test --package nfs_node --test integration_test

# Benchmarks
cargo bench --package nfs_node

# Tests Gossip
cargo test --package gossip_node
```

---

## Limitations NFS

- Pas de réplication entre serveurs (prévu avec Raft)
- Pas d'authentification utilisateur
- Communication non chiffrée (TCP/HTTP en clair)
- DELETE fichiers uniquement (pas de répertoires récursifs)
- Pas de métadonnées avancées (permissions, timestamps)

---

## Roadmap

### Court Terme
- [ ] Logging PeerReview asynchrone
- [ ] Signatures cryptographiques Ed25519
- [ ] Checkpoints automatiques périodiques
- [ ] Tests automatisés CI/CD

### Moyen Terme
- [ ] Réplication Raft pour haute disponibilité
- [ ] Authentification TLS mutuelle (mTLS)
- [ ] Métadonnées avancées (permissions UNIX)
- [ ] Optimisations performance (cache LRU)

### Long Terme
- [ ] Intégration PeerReview complète (Witnesses + Challenge-Response)
- [ ] NFS v4 Partial Compliance
- [ ] Geo-Replication multi-datacenter
- [ ] Dashboard Web de monitoring

---

## Références

- **PeerReview Paper** : Haeberlen et al., "PeerReview: Practical Accountability for Distributed Systems", SOSP 2007
- **NFS v3 Specification** : RFC 1813
- **Lamport Clocks** : Lamport, "Time, Clocks, and the Ordering of Events in a Distributed System", CACM 1978
- **Rust Async Book** : https://tokio.rs/tokio/tutorial

---

## Contact

Pour toute question ou contribution, ouvrir une issue sur le dépôt Git.
