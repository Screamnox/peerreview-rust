# PeerReview-Rust

Infrastructure distribuée multi-arbres pour l’intégration de PeerReview

---

Table of Contents

- [Introduction](#introduction)
- [Objectifs du projet](#objectifs-du-projet)
- [Architecture générale](#architecture-générale)
- [Topologie multi-arbres](#topologie-multi-arbres)
  - [Motivation](#motivation)
  - [Construction des arbres](#construction-des-arbres)
  - [Configuration](#configuration)
- [Communication interne (TCP)](#communication-interne-tcp)
- [API HTTP](#api-http)
- [Structure du dépôt](#structure-du-dépôt)
- [Fonctionnalités actuelles](#fonctionnalités-actuelles)
- [Démos possibles](#démos-possibles)
  - [Diffusion globale d'un message](#diffusion-globale-dun-message)
  - [Publication depuis n'importe quel nœud](#publication-depuis-nimporte-quel-nœud)
  - [Envoi intensif de messages](#envoi-intensif-de-messages)
- [Lien avec PeerReview](#lien-avec-peerreview)
- [Conclusion](#conclusion)

---

## 1. Introduction

Ce projet implémente une infrastructure distribuée en Rust destinée à servir de base au protocole PeerReview (Haeberlen et al., SOSP 2007).  
Il repose sur :

- une diffusion structurée en plusieurs arbres (multi-tree),
- un réseau TCP interne léger et performant,
- une API HTTP simplifiée pour les interactions extérieures,
- une configuration flexible via YAML,
- un cluster Docker reproductible à 10 nœuds.

L’objectif est de disposer d’un environnement distribué cohérent, sur lequel les mécanismes PeerReview pourront être implémentés : journaux sécurisés, audits, détection de comportements fautifs et preuves cryptographiques.

## 2. Objectifs du projet

Le projet vise à fournir :

- une topologie distribuée stable et configurable ;
- des canaux de diffusion multiples pour la résilience et la performance ;
- un protocole P2P clair et extensible ;
- une base technique pour intégrer PeerReview.

Le système actuel constitue la couche réseau et topologique du futur PeerReview.

## 3. Architecture générale

Chaque nœud exécute :

- un serveur TCP : communication P2P interne ;
- un serveur HTTP : interactions externes (publish, stats) ;
- un moteur de diffusion multi-arbres ;
- des tâches périodiques (heartbeats) ;
- un module de déduplication (évite les boucles et répétitions).

Le cluster est orchestré via Docker Compose, permettant un déploiement reproductible à 10 nœuds. Chaque nœud est autonome : pas de coordinateur central.

## 4. Topologie multi-arbres

### 4.1. Motivation

Le système repose sur plusieurs arbres de diffusion pour :

- améliorer la résilience (un arbre peut tomber) ;
- répartir la charge ;
- fournir des chemins logiques distincts, nécessaires aux opérations PeerReview ;
- limiter la congestion d’un seul arbre.

### 4.2. Construction des arbres

La topologie est dérivée de `cluster.yaml`, qui contient :

- la liste des nœuds,
- le nombre d'arbres (`num_trees`),
- le facteur de branchement (`fanout`).

Pour chaque arbre :

1. Un ordre aléatoire de tous les nœuds est généré.
2. Cet ordre est transformé en arbre k-aire selon le `fanout`.
3. Chaque nœud ne connaît que ses propres enfants pour cet arbre.

Chaque nœud possède donc une structure (exemple conceptuel) :

```text
children_by_tree = {
  0: [child1, child2, ...],
  1: [childA, childB, ...],
  ...
}
```

### 4.3. Configuration

Extrait de `configs/docker/cluster.yaml` (exemple) :

```yaml
fanout: 3
num_trees: 3

nodes:
  - { id: "node1", addr: "node1:7001" }
  - { id: "node2", addr: "node2:7001" }
  # ...
```

Modifier `fanout` ou `num_trees` régénère entièrement la topologie.

## 5. Communication interne (TCP)

Les nœuds communiquent en TCP avec un protocole binaire léger basé sur `bincode`. Chaque message est une structure partagée définie dans `crates/common_proto`.

Exemple (conceptuel) :

```rust
enum MsgKind {
    Heartbeat { counter: u64, tree_id: u8 },
    Publish,
    // ... autres types (Ihave, Request, Batch) ...
}

struct Message {
    msg_id: Uuid,
    sender_id: String,
    tree_id: u8,
    kind: MsgKind,
    payload: Option<Vec<u8>>,
    timestamp: u64,
}
```

La déduplication sur `(tree_id, msg_id)` empêche toute retransmission cyclique.

## 6. API HTTP

Une API REST minimale expose deux routes principales :

### POST /publish

Injecte un message dans le système. Pour chaque arbre, un message est généré et diffusé vers les enfants du nœud.

Exemple :

```bash
curl -X POST http://localhost:8083/publish \
     -H "Content-Type: application/json" \
     -d '{"payload":"Hello"}'
```

### GET /stats

Affiche l’état interne du nœud :

- nombre de messages distincts reçus,
- derniers messages reçus,
- nombre d’arbres, etc.

Exemple :

```bash
curl http://localhost:8081/stats
```

Note : les ports et hôtes dépendent de la configuration dans `configs/docker/cluster.yaml`. Ajustez les URL selon vos paramètres.

## 7. Structure du dépôt

peerreview-rust/
│
├─ crates/
│   ├─ common_proto/       # Format des messages TCP
│   └─ lib/                # Extension du protocole (Ihave/Request/Batch)
│
├─ gossip_node/            # Binaire du nœud distribué
│   └─ src/
│        ├─ main.rs        # Point d'entrée et logique principale
│        └─ ...
│
├─ configs/
│   ├─ docker/             # Configurations des 10 nœuds Docker
│   └─ local/              # Configurations pour tests locaux
│
├─ docker/
│   └─ Dockerfile          # Construction de l’image
│
├─ scripts/
│   ├─ gen_10nodes.sh      # Génération automatique des configs Docker
│   └─ up.sh               # Construction et lancement du cluster
│
└─ README.md

## 8. Fonctionnalités actuelles

- Diffusion distribuée via plusieurs arbres.
- Communication interne TCP performante, binaire et compacte.
- API HTTP simple pour tester, superviser et interagir avec un nœud.
- Déduplication robuste contre les boucles.
- Heartbeats périodiques pour chaque arbre.
- Déploiement automatisé d’un cluster 10 nœuds via Docker.
- Paramétrage complet via YAML.

## 9. Démos possibles

Ces démonstrations peuvent être effectuées en quelques commandes. Remplacez les ports par ceux définis dans `configs/docker/cluster.yaml` (ex. 8081, 8082, ...).

### 9.1. Diffusion globale d'un message

Publier depuis un nœud :

```bash
curl -X POST http://localhost:8083/publish \
  -H "Content-Type: application/json" \
  -d '{"payload":"Hello world"}'
```

Observer sur d’autres nœuds (exemples) :

```bash
curl http://localhost:8081/stats
curl http://localhost:8085/stats
curl http://localhost:8089/stats
```

### 9.2. Publication depuis n'importe quel nœud

```bash
curl -X POST http://localhost:8087/publish \
  -H "Content-Type: application/json" \
  -d '{"payload":"From node7"}'
```

Vérification (exemples) :

```bash
curl http://localhost:8082/stats
curl http://localhost:80810/stats  # Remplacez par le port configuré pour le nœud 10
```

### 9.3. Envoi intensif de messages

```bash
for i in $(seq 1 20); do
  curl -s -X POST http://localhost:8084/publish \
    -H "Content-Type: application/json" \
    -d "{\"payload\":\"msg_$i\"}" > /dev/null
done
```

Vérification :

```bash
curl http://localhost:8081/stats
```

Ces tests montrent : pas de duplication, propagation rapide et stabilité du système.

## 10. Lien avec PeerReview

L’objectif final est de transformer ce système en une implémentation complète du protocole PeerReview. Le système actuel fournit déjà les briques essentielles :

- overlay structuré multi-arbres,
- communication fiable et déterministe,
- propagation contrôlée,
- mécanisme de déduplication,
- transport léger,
- protocole extensible (Ihave, Request, Batch déjà définis).

Prochaines étapes techniques recommandées :

- journaux sécurisés (tamper-evident logs),
- hachage chaîné et signatures,
- audits pair-à-pair,
- vérification des écarts de comportement,
- preuves cryptographiques de faute.

## 11. Conclusion

Ce projet constitue une base solide pour la mise en œuvre du protocole PeerReview. Il propose une infrastructure distribuée réaliste, configurable, performante et extensible. Le cluster multi-arbres, la communication TCP, l’API HTTP et les mécanismes internes en font une plateforme prête à accueillir les étapes suivantes : logs sécurisés, audits et vérification comportementale.

---

Annexes rapides

- Lancement local (exemple) :

```bash
# Générer la configuration des 10 nœuds (script fourni)
./scripts/gen_10nodes.sh

# Lancer le cluster via docker-compose (depuis le dossier `configs/docker`)
./scripts/up.sh
```

- Emplacement des messages partagés : `crates/common_proto`
- Documentation technique et points d’extension : `crates/lib`
