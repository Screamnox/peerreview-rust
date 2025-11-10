# 🕸️ PeerReview-RS — Application distribuée (Rust + Docker)

## 📘 Contexte
Ce dépôt constitue la base **distribuée** du projet *PeerReview-RS*.
Il s’agit d’une infrastructure de **communication pair-à-pair (P2P)** sur laquelle
les protocoles PeerReview seront intégrés ultérieurement.

L’objectif de ce module est :
- De fournir un **système distribué fonctionnel** (multi-nœuds),
- D’assurer la **communication fiable entre pairs**,
- Et de permettre le **déploiement reproductible via Docker**.

---

## 🚀 Fonctionnalités principales

| Fonctionnalité | Description |
|----------------|-------------|
| **Communication Gossip (Tokio)** | Échange automatique de messages (heartbeat, payloads) entre nœuds |
| **Infrastructure multi-nœuds** | Déploiement jusqu’à **10 nœuds** interconnectés sur un réseau Docker |
| **HTTP REST API** | Interface de publication et de monitoring (via `/publish` et `/stats`) |
| **Overlay multi-arbres** | Topologie hiérarchique (multi-tree) pour la diffusion efficace |
| **Docker Compose orchestration** | Conteneurisation complète, réseau privé virtuel `docker_gossip_net` |
| **Scalabilité** | Ajout ou suppression de nœuds simple, configuration paramétrable |
| **Préparation PeerReview** | Intégration future des sous-protocoles (SecureLog, Audit, Evidence, etc.) |

---

## 🧩 Architecture du projet

peerreview-rs/
├── apps/
│ └── gossip_node/ # Application binaire principale (un nœud)
├── crates/
│ └── common_proto/ # Types et messages partagés (Msg, MsgKind, etc.)
├── configs/
│ ├── local/ # Configs pour exécution locale (sans Docker)
│ └── docker/ # Configs pour exécution Dockerisée (10 nœuds)
├── docker/
│ └── docker-compose.yml # Déploiement complet du cluster
├── scripts/
│ ├── up.sh # Lancement des conteneurs
│ └── down.sh # Arrêt et nettoyage
├── Cargo.toml # Workspace Rust
└── README.md # Ce document


---

## ⚙️ Prérequis

| Logiciel | Version minimale |
|-----------|------------------|
| **Rust** | 1.91.0 |
| **Cargo** | 1.91.0 |
| **Docker Engine** | 28+ |
| **Docker Compose** | v2.40+ |

Pour vérifier :
```bash
rustc --version
cargo --version
docker --version
docker compose version

🧱 Lancement local (2 nœuds)

1️⃣ Compiler le workspace :

cargo build --workspace

2️⃣ Lancer un nœud :

cargo run -p gossip_node -- \
  --config configs/local/node1.yaml \
  --cluster configs/local/cluster.yaml

3️⃣ Lancer un deuxième terminal :

cargo run -p gossip_node -- \
  --config configs/local/node2.yaml \
  --cluster configs/local/cluster.yaml

4️⃣ Vérifier les échanges :

curl localhost:8081/stats
curl -X POST localhost:8081/publish -H 'content-type: application/json' \
     -d '{"payload":"hello local cluster"}'
curl localhost:8082/stats

🟢 Attendu :

    node1 et node2 s’échangent automatiquement des heartbeats.

    Les messages publiés par l’un sont reçus par l’autre.

🐳 Lancement Dockerisé (jusqu’à 10 nœuds)

1️⃣ Construire et lancer le cluster :

./scripts/up.sh

2️⃣ Vérifier les conteneurs :

docker compose -f docker/docker-compose.yml ps

🟢 Exemple :

NAME      IMAGE          STATUS   PORTS
node1     docker-node1   Up       8081->8081/tcp
node2     docker-node2   Up       8082->8082/tcp
...

3️⃣ Vérifier les logs :

docker compose -f docker/docker-compose.yml logs -f node1

4️⃣ Tester la diffusion :

curl -X POST localhost:8081/publish -H 'content-type: application/json' \
     -d '{"payload":"hello docker cluster"}'

5️⃣ Consulter l’état global :

curl localhost:8081/stats
curl localhost:8085/stats

🌳 Topologie multi-arbres (multi-tree overlay)

Chaque nœud maintient plusieurs arbres de diffusion indépendants (t0, t1, t2...),
ce qui :

    Évite la congestion d’un seul canal,

    Améliore la résilience aux pannes,

    Et prépare l’architecture pour la vérification PeerReview.

Exemple :

node1 children_by_tree = [
  "t0: [node2, node10, node6]",
  "t1: []",
  "t2: [node8, node7, node4]"
]

Chaque message Msg inclut désormais un champ tree_id, indiquant le canal de propagation.
🧠 Structure des messages

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub from: String,
    pub kind: MsgKind,
    pub tree_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MsgKind {
    Heartbeat { counter: u64, tree_id: String },
    Payload { data: String, tree_id: String },
}

🔄 Workflow de développement (équipe)
Étape	Responsable	Outil / Commande
Dev local	Tous	cargo run -p gossip_node
Tests intégration	Tous	docker compose up
CI / Lint	Automatique	via GitHub Actions
Ajout protocoles PeerReview	Autres binômes	Import via API commune
Synchronisation	Samy (app distrib.) + équipe	Revue de code + intégration
🧰 Commandes utiles
Commande	Description
./scripts/up.sh	Démarre les 10 nœuds Docker
./scripts/down.sh	Stoppe et nettoie le cluster
docker compose logs -f nodeX	Affiche les logs d’un nœud spécifique
curl localhost:808X/stats	Récupère l’état d’un nœud
cargo build --workspace	Compile l’ensemble du projet
cargo fmt	Formate le code
cargo clippy	Analyse statique du code
🧩 Étapes suivantes (Sprints 2 et 3)

    ✅ Sprint 1 (actuel) :
    Architecture distribuée + 10 nœuds + communication stable.

    🧠 Sprint 2 :
    Intégration du module PeerReview (SecureLog, Audit, Evidence).

    ⚡ Sprint 3 :
    Monitoring distribué, tolérance aux fautes, scénario d’évaluation.

