PeerReview-Rust — Infrastructure distribuée multi-arbres (10 nœuds)

Ce projet implémente la base d’un système distribué en Rust servant de socle pour l’intégration du protocole PeerReview.
Il fournit :

✔️ une topologie multi-arbres configurable,

✔️ un réseau TCP efficace pour la communication P2P,

✔️ une API HTTP simple pour publier des messages et superviser les nœuds,

✔️ un cluster de 10 nœuds Docker entièrement automatisé,

✔️ la propagation distribuée des messages avec déduplication,

✔️ une architecture prête pour PeerReview : journaux sécurisés, audits, preuves.

Ce README explique ce que nous avons, ce que nous pouvons faire, et pourquoi.

🧠 1. Motivation & Objectif

Le protocole PeerReview (Haeblerlen et al., SOSP’07) impose une infrastructure distribuée :

capable de diffuser des messages à tous les nœuds,

détecter les incohérences,

empêcher la falsification des logs,

fournir des preuves d’intégrité.

Notre but :

Construire une base distribuée fiable qui servira de support direct pour PeerReview.

Le présent système représente la première brique, sur laquelle la mécanique d’audit et de vérification sera rajoutée.

🏗️ 2. Architecture globale

Chaque nœud Rust exécute :

un serveur TCP (communication interne entre nœuds),

un serveur HTTP (API externe : publish + stats),

un moteur de diffusion multi-arbres,

des tasks périodiques (heartbeats),

des structures de déduplication pour éviter les boucles.

Autour de ça :

un cluster Docker 10 nœuds,

des configurations YAML flexibles,

un protocole de message commun (via common_proto).

🌳 3. La Topologie Multi-Arbres
3.1 Pourquoi des arbres multiples ?

Les multi-arbres permettent :

meilleure résilience (si un arbre “meurt”, d’autres continuent),

répartition de charge lors de la diffusion,

des canaux logiques indépendants (essentiel pour PeerReview),

des chemins distincts pour relayer des preuves.

3.2 Construction des arbres

À partir du cluster.yaml :

mélange aléatoire des nœuds → ordre aléatoire par arbre,

construction d’un arbre k-aire selon fanout,

chaque nœud ne connaît que ses enfants dans chaque arbre.

Chaque nœud stocke :

children_by_tree = {
  0: [childA, childB, ...],
  1: [childC, childD, ...],
  2: [...]
}

3.3 Configuration simple (YAML)
fanout: 3
num_trees: 3

nodes:
  - { id: "node1", addr: "node1:7001" }
  - { id: "node2", addr: "node2:7001" }
  ...


En modifiant fanout et num_trees, vous changez toute la topologie.

🔗 4. Communication interne (TCP)

Chaque nœud :

écoute en TCP,

se connecte aux autres nœuds si besoin,

échange des messages sérialisés avec bincode,

applique une déduplication (tree_id, msg_id).

Les messages (common_proto) :

enum MsgKind {
    Heartbeat { counter: u64, tree_id: u8 },
    Publish,
}


Chaque message porte :

un UUID,

l’auteur,

le tree_id,

le payload.

🌐 5. API HTTP externe

Exposée par Axum :

POST /publish

Injecte un message dans le système (un par arbre).

curl -X POST http://localhost:8083/publish \
     -H "Content-Type: application/json" \
     -d '{"payload":"Hello"}'

GET /stats

Retourne l’état du nœud :

{
  "node_id": "node5",
  "known_count": 42,
  "last_msgs": [
    "(t1 from node3) Hello"
  ],
  "trees": 3
}

📦 6. Structure du Répertoire
peerreview-rust/
│
├─ crates/
│   ├─ common_proto/      # Format des messages TCP
│   └─ lib/               # Proto riche (Ihave/Request/Batch) pour PeerReview
│
├─ gossip_node/           # Binaire principal
│   └─ src/
│        ├─ main.rs       # Point d'entrée, topologie, services
│        └─ ...
│
├─ configs/
│   ├─ docker/            # Configs 10 nœuds Docker
│   └─ local/             # Configs 2 nœuds local
│
├─ docker/
│   └─ Dockerfile         # Build de l’image
│
├─ scripts/
│   ├─ gen_10nodes.sh     # Génère les configs Docker
│   └─ up.sh              # Build + run du cluster
│
└─ README.md

🚀 7. Ce que le système sait faire maintenant
✔️ Diffusion distribuée multi-arbres

Chaque Publish est diffusé sur tous les arbres.

✔️ Communication P2P performante (TCP)

Bincode = faible overhead.

✔️ API HTTP simple

Pour injecter des messages & observer l’état.

✔️ Déduplication robuste

Empêche toute boucle potentielle.

✔️ Heartbeats réguliers

Chaque nœud signale sa présence dans chaque arbre.

✔️ Cluster complet en 1 commande

./scripts/up.sh

🎮 8. Demos possibles

Ces démonstrations montrent l’efficacité et la stabilité de votre architecture.

🎯 DEMO 1 – Diffusion globale d’un message

Publier sur un nœud et voir la réception sur tous les autres.

Publier depuis node3
curl -X POST http://localhost:8083/publish \
  -H "Content-Type: application/json" \
  -d '{"payload":"Hello world"}'

Vérifier sur d'autres nœuds
curl http://localhost:8081/stats
curl http://localhost:8085/stats
curl http://localhost:8089/stats


Résultat :
Le message apparaît sur les 10 nœuds, dans les différents arbres (t0, t1, t2).

🎯 DEMO 2 – Publier depuis n’importe quel nœud

Montre que la diffusion fonctionne quelle que soit la racine d’injection.

Publier depuis node7
curl -X POST http://localhost:8087/publish \
  -H "Content-Type: application/json" \
  -d '{"payload":"From node7"}'

Vérifier sur d'autres nœuds
curl http://localhost:8082/stats
curl http://localhost:80810/stats


Résultat :
Tous les nœuds reçoivent le message sans duplication.

🎯 DEMO 3 – Charge & scalabilité

Envoi de 20 messages rapides → démontre la stabilité et la déduplication.

for i in $(seq 1 20); do
  curl -s -X POST http://localhost:8084/publish \
    -H "Content-Type: application/json" \
    -d "{\"payload\":\"msg_$i\"}" > /dev/null
done


Puis :

curl http://localhost:8081/stats


Résultat :

aucune duplication,

les derniers messages sont visibles,

la propagation reste fluide.

🔮 9. Lien avec PeerReview

Votre infrastructure fournit tout le nécessaire pour intégrer PeerReview :

overlay multi-arbres → plusieurs canaux de propagation,

communication fiable → messages binaires + déduplication,

heartbeats → utiles pour les audits,

structuration claire → nœuds indépendants mais synchronisés,

proto extensible (lib.rs) → ajouté pour supporter Ihave / Request / Batch.

Le prochain travail consistera à :

enregistrer tous les messages dans un log inviolable,

produire des preuves cryptographiques,

échanger des IHave / Request / Batch pour vérifier les pairs,

détecter les nœuds malhonnêtes.

Votre système est déjà une version simplifiée d’un vrai état de PeerReview, avec le cœur réseau fonctionnel.

🏁 10. Conclusion

Ce projet fournit une infrastructure distribuée complète, en Rust, multi-nœuds, multi-arbres et prête pour PeerReview.
C’est une base solide pour expérimenter :

la diffusion distribuée,

les protocoles de vérification,

les mécanismes d’audit et de preuve.

Vous disposez maintenant d’un vrai cluster distribué, contrôlable, configurable, efficace et extensible.
