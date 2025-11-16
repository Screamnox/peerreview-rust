PeerReview-Rust

Infrastructure distribuée multi-arbres pour l’intégration de PeerReview

Table of Contents

Introduction

Objectifs du projet

Architecture générale

Topologie multi-arbres

Communication interne (TCP)

API HTTP

Structure du dépôt

Fonctionnalités actuelles

Démos possibles

Lien avec PeerReview

Conclusion

1. Introduction

Ce projet implémente une infrastructure distribuée en Rust destinée à servir de base au protocole PeerReview (Haeberlen et al., SOSP 2007).
Il repose sur :

une diffusion structurée en plusieurs arbres (multi-tree),

un réseau TCP interne léger et performant,

une API HTTP simplifiée pour les interactions extérieures,

une configuration flexible via YAML,

un cluster Docker reproductible à 10 nœuds.

L’objectif est de disposer d’un environnement distribué cohérent, sur lequel les mécanismes PeerReview pourront être implémentés : journaux sécurisés, audits, détection de comportements fautifs, preuves cryptographiques.

2. Objectifs du projet

Le projet vise à fournir :

une topologie distribuée stable et configurable ;

des canaux de diffusion multiples pour la résilience et la performance ;

un protocole P2P clair et extensible ;

une base technique pour intégrer PeerReview.

Le système actuel constitue la couche réseau et topologique du futur PeerReview.

3. Architecture générale

Chaque nœud exécute :

un serveur TCP : communication P2P interne ;

un serveur HTTP : interactions externes (publish, stats) ;

un moteur de diffusion multi-arbres ;

des tâches périodiques (heartbeats) ;

un module de déduplication (évite les boucles et répétitions).

Le cluster entier est orchestré via Docker Compose, permettant un déploiement reproductible à 10 nœuds.

Chaque nœud est entièrement autonome : pas de coordinateur central.

4. Topologie multi-arbres
4.1. Motivation

Le système repose sur plusieurs arbres de diffusion pour :

améliorer la résilience (un arbre peut tomber) ;

répartir la charge ;

fournir des chemins logiques distincts, nécessaires aux futures opérations PeerReview ;

limiter la congestion d’un seul arbre.

4.2. Construction des arbres

La topologie est dérivée de cluster.yaml, qui contient :

la liste des nœuds,

le nombre d'arbres (num_trees),

le facteur de branchement (fanout).

Pour chaque arbre :

Un ordre aléatoire de tous les nœuds est généré.

Cet ordre est transformé en arbre k-aire selon le fanout.

Chaque nœud ne connaît que ses propres enfants pour cet arbre.

Un nœud possède donc une structure :

children_by_tree = {
  0: [child1, child2, ...],
  1: [childA, childB, ...],
  ...
}

4.3. Configuration

Dans configs/docker/cluster.yaml :

fanout: 3
num_trees: 3

nodes:
  - { id: "node1", addr: "node1:7001" }
  - { id: "node2", addr: "node2:7001" }
  ...


Modifier fanout ou num_trees régénère entièrement la topologie.

5. Communication interne (TCP)

Les nœuds communiquent en TCP avec un protocole binaire léger basé sur bincode.
Chaque message est une structure partagée définie dans crates/common_proto.

Message type :

enum MsgKind {
    Heartbeat { counter: u64, tree_id: u8 },
    Publish,
}


Chaque message inclut :

un identifiant unique (UUID),

l’émetteur,

un identifiant d’arbre (tree_id),

un payload optionnel,

un timestamp.

La déduplication (tree_id, msg_id) empêche toute retransmission cyclique.

6. API HTTP

Une API REST minimale expose deux routes :

6.1. POST /publish

Injecte un message dans le système.
Pour chaque arbre, un message est généré et diffusé vers les enfants du nœud.

Exemple :

curl -X POST http://localhost:8083/publish \
     -H "Content-Type: application/json" \
     -d '{"payload":"Hello"}'

6.2. GET /stats

Affiche l’état interne du nœud, notamment :

le nombre de messages distincts reçus,

les derniers messages,

le nombre d’arbres.

7. Structure du dépôt
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

8. Fonctionnalités actuelles

Diffusion distribuée via plusieurs arbres.

Communication interne TCP performante, binaire et compacte.

API HTTP simple pour tester, superviser et interagir avec un nœud.

Déduplication robuste contre les boucles.

Heartbeats périodiques pour chaque arbre.

Déploiement automatisé d’un cluster 10 nœuds via Docker.

Paramétrage complet via YAML.

9. Démos possibles

Ces démonstrations peuvent être effectuées en quelques commandes.

9.1. Diffusion globale d'un message

Publier depuis un nœud :

curl -X POST http://localhost:8083/publish \
  -H "Content-Type: application/json" \
  -d '{"payload":"Hello world"}'


Observer sur d’autres nœuds :

curl http://localhost:8081/stats
curl http://localhost:8085/stats
curl http://localhost:8089/stats

9.2. Publication depuis n'importe quel nœud
curl -X POST http://localhost:8087/publish \
  -H "Content-Type: application/json" \
  -d '{"payload":"From node7"}'


Vérification :

curl http://localhost:8082/stats
curl http://localhost:80810/stats

9.3. Envoi intensif de messages
for i in $(seq 1 20); do
  curl -s -X POST http://localhost:8084/publish \
    -H "Content-Type: application/json" \
    -d "{\"payload\":\"msg_$i\"}" > /dev/null
done


Vérification :

curl http://localhost:8081/stats


Ces tests montrent :

pas de duplication,

propagation rapide,

stabilité du système.

10. Lien avec PeerReview

L’objectif final est de transformer ce système en une implémentation complète du protocole PeerReview.

Le système actuel fournit déjà les briques essentielles :

overlay structuré multi-arbres,

communication fiable et déterministe,

propagation contrôlée,

mécanisme de déduplication,

transport léger,

protocole extensible (Ihave, Request, Batch déjà définis).

Les prochaines étapes incluent :

journaux sécurisés (tamper-evident logs),

hachage chaîné,

audits pair-à-pair,

vérification des écarts de comportement,

preuves cryptographiques de faute.

11. Conclusion

Ce projet constitue une base solide pour la mise en œuvre du protocole PeerReview.
Il propose une infrastructure distribuée réaliste, configurable, performante et extensible.
Le cluster multi-arbres, la communication TCP, l’API HTTP et les mécanismes internes en font une plateforme prête à accueillir les étapes suivantes : logs sécurisés, audits et vérification comportementale.
