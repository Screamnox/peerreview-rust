# peerreview-rust – Infrastructure distribuée multi-arbres (Rust)

Ce projet implémente une infrastructure distribuée en Rust, pensée comme base pour l’intégration de **PeerReview** (Haeberlen et al.).  
La branche `feature/multitree-10nodes` fournit :

- un **cluster Docker** de 10 nœuds,
- un **overlay multi-arbres** pour la diffusion,
- un **protocole applicatif simple** (texte + binaire),
- une **API HTTP** pour piloter et observer les nœuds.

---

## 1. Ce que cette branche fait concrètement

Aujourd’hui, cette branche permet de :

- Lancer un **cluster de nœuds Rust** (10 nœuds) via Docker.
- Construire au démarrage un **ensemble de T arbres de diffusion** au-dessus du cluster.
- Diffuser des messages applicatifs :
  - texte (`PublishText`) ;
  - binaire en chunks (`PublishBinaryChunk`, par exemple pour simuler une vidéo ou un fichier).
- Assurer :
  - une **déduplication** des messages par `(tree_id, msg_id)` ;
  - la circulation de **heartbeats** périodiques par arbre.
- Exposer une API HTTP sur chaque nœud :
  - `GET /stats` : état local (messages reçus, taille du cache, nombre d’arbres) ;
  - `POST /publish` : injection d’un message texte dans tous les arbres ;
  - `POST /publish_binary_demo` : injection d’un flux binaire découpé en chunks.

En résumé :  
on a maintenant un **système distribué multi-arbres fonctionnel**, avec diffusion texte + binaire, prêt à accueillir la logique PeerReview (logs sécurisés, authenticators, IHAVE/REQUEST/BATCH, audit, etc.).

---

## 2. Architecture globale

### 2.1. Processus par nœud

Chaque nœud lance :

- une **socket TCP** pour les messages du protocole applicatif (texte, binaire, heartbeats) ;
- une **boucle d’envoi** qui se connecte aux pairs et écrit les messages (`length-prefix + payload` en bincode) ;
- une **boucle de réception** qui lit les messages, les désérialise, applique la déduplication puis les relaie ;
- un **serveur HTTP** (Axum) pour `/stats`, `/publish`, `/publish_binary_demo` ;
- une tâche périodique de **heartbeats** par arbre.

### 2.2. Fichiers / modules importants

- `apps/gossip_node/src/main.rs`  
  Point d’entrée d’un nœud :
  - parsing des configs (node + cluster),
  - construction des arbres,
  - lancement des tâches (TCP, HTTP, heartbeats),
  - logique de diffusion des messages texte et binaires.

- `crates/common_proto/src/lib.rs`  
  Définit les types partagés entre les nœuds :
  - `NodeId` ;
  - `MsgKind` (`Heartbeat`, `PublishText`, `PublishBinaryChunk`) ;
  - `Msg` (message applicatif sérialisé en bincode) ;
  - `BinaryChunk` (morceau d’un flux binaire).

- `configs/docker/nodeX.yaml`  
  Configuration d’un nœud `nodeX` :
  - `id` ;
  - `listen_addr` : adresse TCP pour le protocole applicatif ;
  - `heartbeat_ms`, `gossip_ms`, `anti_entropy_ms` (paramètres de périodicité) ;
  - `http_api` : port HTTP exposé dans le conteneur (et mappé sur `localhost:808X`).

- `configs/docker/cluster.yaml`  
  Description du cluster :
  - liste des nœuds (`id`, `addr`) ;
  - `fanout` : nombre d’enfants par nœud dans chaque arbre ;
  - `num_trees` : nombre d’arbres parallèles dans l’overlay.

- `docker/docker-compose.yml`  
  Lance 10 conteneurs `node1` à `node10`, avec les bons volumes et ports HTTP exposés.

- `scripts/up.sh` / `scripts/down.sh`  
  Scripts pour build + lancer / arrêter le cluster Docker.

---

## 3. Multi-tree : construction et fonctionnement

### 3.1. Construction des arbres

À partir de `cluster.yaml`, le nœud lit :

```yaml
nodes:
  - { id: "node1", addr: "node1:7001" }
  - { id: "node2", addr: "node2:7001" }
  - { id: "node3", addr: "node3:7001" }
  - { id: "node4", addr: "node4:7001" }

fanout: 2
num_trees: 3
Dans le code (main.rs) :

On construit, pour chaque arbre t :

un ordre permuté déterministe des nœuds (make_orders) à partir d’une seed fixe.

Pour chaque nœud i, on en déduit ses enfants k-aires via kary_children(...) :

children_by_tree[t] = Vec<Peer>.

Chaque nœud connaît donc, pour chaque arbre :

rust
Copy code
children_by_tree[tree_id] = [liste de peers enfants]
La structure des arbres est fixe pendant l’exécution (construite au démarrage).

3.2. Diffusion texte
Lorsqu’un client appelle POST /publish sur un nœud :

le nœud crée un Msg { kind: PublishText, tree_id = t } pour chaque arbre t ;

il marque le message comme connu (known_msgs.insert((t, msg.id))) ;

il le pousse dans inbox pour /stats ;

il l’envoie en TCP à tous ses enfants children_by_tree[t].

À la réception d’un Msg :

le nœud teste (tree_id, msg_id) dans known_msgs ;

s’il est déjà connu, il est ignoré (déduplication) ;

sinon :

s’il s’agit d’un PublishText, le nœud :

ajoute un aperçu dans inbox ;

le relaie à ses enfants dans cet arbre.

3.3. Diffusion binaire (chunks)
Pour les messages binaires, on procède en chunks :

Un appel à POST /publish_binary_demo sur un nœud :

génère un buffer binaire aléatoire (par exemple 200 kB) ;

le découpe en chunks (BinaryChunk) de taille configurable (par ex. 65 536 octets) ;

encapsule chaque chunk dans un Msg { kind: PublishBinaryChunk, payload = bincode(BinaryChunk) } ;

diffuse ces messages sur tous les arbres comme pour PublishText ;

loggue localement dans inbox des lignes de type :

(local BIN t0 video_demo chunk 1/4 size=65536).

À la réception d’un PublishBinaryChunk :

le nœud désérialise le BinaryChunk ;

met à jour un buffer local par file_id ;

trace dans inbox :

(BIN t2 from node3 file_id=video_demo chunk 3/4 size=65536) ;

lorsque tous les chunks sont reçus, il reconstitue le buffer complet et trace :

(BIN COMPLETE from node3 file_id=video_demo total_bytes=200000).

4. API HTTP
4.1. GET /stats
Retourne un JSON avec :

json
Copy code
{
  "node_id": "node5",
  "known_count": 143,
  "last_msgs": [
    "(BIN COMPLETE from node3 file_id=video_demo total_bytes=200000)",
    "(BIN t1 from node3 file_id=video_demo chunk 4/4 size=3392)",
    "(BIN t1 from node3 file_id=video_demo chunk 3/4 size=65536)",
    "(BIN t1 from node3 file_id=video_demo chunk 2/4 size=65536)",
    "(BIN t1 from node3 file_id=video_demo chunk 1/4 size=65536)",
    "(t1 from node3) hello from node3"
  ],
  "trees": 3
}
known_count : nombre de messages distincts vus (texte + binaire + heartbeats).

last_msgs : derniers messages (texte ou binaire) vus localement.

trees : nombre d’arbres configurés (num_trees).

4.2. POST /publish
Payload JSON :

json
Copy code
{
  "payload": "hello from node3"
}
Effet :

Le message est injecté sur tous les arbres à partir du nœud local.

Les autres nœuds reçoivent et relaient ce PublishText.

4.3. POST /publish_binary_demo
Payload JSON :

json
Copy code
{
  "file_id": "video_demo",
  "total_size": 200000,
  "chunk_size": 65536
}
Effet :

Le nœud simule l’envoi d’un flux binaire file_id découpé en chunks.

Les nœuds reçoivent, tracent les chunks et reconstituent le fichier.

5. Démos possibles
5.1. Démarrer le cluster Docker (10 nœuds)
Depuis la racine du repo :

bash
Copy code
./scripts/up.sh
Vérifier que les conteneurs tournent :

bash
Copy code
docker compose -f docker/docker-compose.yml ps
Ports HTTP exposés (exemple) :

node1 → http://localhost:8081

node2 → http://localhost:8082

node3 → http://localhost:8083

…

node10 → http://localhost:8090

5.2. Démo texte
Publier un message texte depuis node3 :

bash
Copy code
curl -X POST http://localhost:8083/publish \
  -H "Content-Type: application/json" \
  -d '{"payload":"hello from node3"}'
Observer sur d’autres nœuds :

bash
Copy code
curl http://localhost:8081/stats
curl http://localhost:8085/stats
On voit apparaître :

json
Copy code
"(t2 from node3) hello from node3"
ou similaire selon les arbres.

5.3. Démo binaire (chunks)
Lancer la démo binaire depuis node3 :

bash
Copy code
curl -X POST http://localhost:8083/publish_binary_demo \
  -H "Content-Type: application/json" \
  -d '{"file_id":"video_demo","total_size":200000,"chunk_size":65536}'
Observer sur d’autres nœuds :

bash
Copy code
curl http://localhost:8081/stats
curl http://localhost:8085/stats
Exemple de last_msgs sur node1 :

json
Copy code
"(BIN COMPLETE from node3 file_id=video_demo total_bytes=200000)"
"(BIN t2 from node3 file_id=video_demo chunk 4/4 size=3392)"
"(BIN t2 from node3 file_id=video_demo chunk 3/4 size=65536)"
"(BIN t2 from node3 file_id=video_demo chunk 2/4 size=65536)"
"(BIN t2 from node3 file_id=video_demo chunk 1/4 size=65536)"
"(t2 from node3) hello from node3"
On voit :

que le message texte est bien passé sur l’arbre t2 ;

que les 4 chunks ont été reçus ;

que la reconstitution a eu lieu (ligne BIN COMPLETE).

6. Lien avec PeerReview et sockets APP/PR
Cette branche fournit la machinerie réseau dont PeerReview a besoin :

diffusion fiable sur plusieurs arbres ;

messages sérialisés en bincode ;

HTTP pour piloter et introspecter ;

base pour l’ajout de logs sécurisés et d’authenticators.

À terme, l’idée est de séparer le transport en deux sockets :

socket APP :

pour les messages applicatifs (PublishText, PublishBinaryChunk, Heartbeat, etc.) ;

socket PR :

pour les messages PeerReview (IHAVE, REQUEST, BATCH, etc.).

Pour mettre en place cette séparation, il faudra :

Côté config :

dans les configs de nœud (nodeX.yaml), remplacer listen_addr par :

app_listen_addr ;

pr_listen_addr ;

dans cluster.yaml, remplacer addr par :

app_addr ;

pr_addr.

Côté code (main.rs) :

adapter NodeCfg et Peer :

rust
Copy code
struct NodeCfg {
    id: String,
    app_listen_addr: String,
    pr_listen_addr: String,
    ...
}

struct Peer {
    id: String,
    app_addr: String,
    pr_addr: String,
}
garder la logique actuelle sur la socket APP (texte + binaire + heartbeats) ;

ajouter un deuxième TcpListener + une boucle de réception pour la socket PR ;

ajouter un deuxième mpsc::Sender<(Peer, Vec<u8>)> pour envoyer les messages PeerReview ;

définir une enum PRMsg dans common_proto pour IHAVE, REQUEST, BATCH, etc., sérialisée en bincode.

Côté PeerReview :

brancher les appels peerreview.on_send(...) / peerreview.on_recv(...) autour de l’envoi/réception des Msg (APP) ;

utiliser la socket PR pour transporter les messages de métadonnées (authenticators, logs, audits) entre witnesses.

Pour l’instant, le choix a été de stabiliser l’implémentation mono-socket (APP) avec texte + binaire + multi-arbres, avant de séparer proprement APP / PR.
