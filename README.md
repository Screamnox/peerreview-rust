
# peerreview-rust – Infrastructure distribuée multi-arbres (Rust)

Branche : `feature/multitree-10nodes`  
Base réseau pour l'intégration de **PeerReview** (Haeberlen et al.)

---

## 🚀 Fonctionnalités de la branche

- Cluster Docker **10 nœuds**
- Overlay **multi-arbres** pour la diffusion
- Messages **texte** et **binaire en chunks**
- **Déduplication** `(tree_id, msg_id)`
- **Heartbeats** périodiques par arbre
- API HTTP par nœud :
  - `GET /stats`
  - `POST /publish`
  - `POST /publish_binary_demo`

---

## 1. Ce que cette branche fait concrètement

### Lancement du cluster distribué

- 10 nœuds Rust dans Docker (`docker-compose.yml`)
- Construction automatique de `num_trees` arbres parallèles au démarrage

### Diffusion

- **Texte** : `PublishText`
- **Binaire** : découpage en `BinaryChunk` (chunks de taille configurable)

### Réception & relai

- Déduplication locale par `(tree_id, msg_id)`
- Relai aux enfants de l’arbre correspondant
- Journalisation dans une inbox locale (visible via `GET /stats`)

En résumé : cette branche fournit un **overlay multi-arbres fonctionnel** (texte + binaire), prêt à accueillir la logique PeerReview.

---

## 2. Architecture globale

### 2.1 Processus dans chaque nœud

Chaque nœud lance :

- une **socket TCP** pour les messages du protocole applicatif (texte, binaire, heartbeats) ;
- une **boucle d’envoi** qui se connecte aux pairs et écrit les messages (`length-prefix + payload` en bincode) ;
- une **boucle de réception** qui lit les messages, les désérialise, applique la déduplication puis les relaie ;
- un **serveur HTTP** (Axum) exposant :
  - `GET /stats`
  - `POST /publish`
  - `POST /publish_binary_demo`
- une tâche périodique de **heartbeats** par arbre.

---

### 2.2 Fichiers / modules importants

| Fichier / dossier                     | Rôle principal                                                                 |
|--------------------------------------|-------------------------------------------------------------------------------|
| `apps/gossip_node/src/main.rs`       | Point d’entrée d’un nœud, parsing config, construction des arbres, tâches.   |
| `crates/common_proto/src/lib.rs`     | Types partagés : `NodeId`, `MsgKind`, `Msg`, `BinaryChunk`.                  |
| `configs/docker/nodeX.yaml`          | Config d’un nœud : id, écoute TCP, timers, port HTTP.                        |
| `configs/docker/cluster.yaml`        | Description du cluster : liste des nœuds, `fanout`, `num_trees`.            |
| `docker/docker-compose.yml`          | Définition du cluster Docker (10 conteneurs).                                |
| `scripts/up.sh` / `scripts/down.sh`  | Scripts pour build + lancer / arrêter le cluster.                            |

---

## 3. Multi-tree : construction et fonctionnement

### 3.1 Construction des arbres

À partir de `configs/docker/cluster.yaml`, le code :

- lit la liste des nœuds et les paramètres `fanout` et `num_trees` ;
- génère, pour chaque arbre `t`, un ordre permuté déterministe des nœuds ;
- construit un arbre k-aire (k = `fanout`) pour chaque `t` ;
- remplit une structure de type :

```rust
children_by_tree[tree_id] = Vec<Peer>;
```

Chaque nœud connaît ainsi, pour chaque arbre, la liste de ses enfants.  
La structure des arbres est **fixe** pendant l’exécution (construite au démarrage).

---

### 3.2 Diffusion texte

Lorsqu’un client appelle `POST /publish` sur un nœud :

1. le nœud crée un `Msg { kind: PublishText, tree_id = t }` pour chaque arbre `t` ;
2. il marque le message comme connu : insertion dans `known_msgs` ;
3. il pousse un aperçu du message dans `inbox` (visible dans `/stats`) ;
4. il envoie le message en TCP à tous ses enfants `children_by_tree[t]`.

À la réception d’un `Msg` :

- si `(tree_id, msg_id)` est déjà présent dans `known_msgs`, le message est ignoré ;
- sinon, s’il s’agit d’un `PublishText` :
  - le nœud ajoute un aperçu dans `inbox` ;
  - il relaie le message à ses enfants dans cet arbre.

---

### 3.3 Diffusion binaire (chunks)

Pour les messages binaires, la diffusion se fait en **chunks** :

1. `POST /publish_binary_demo` sur un nœud :
   - génère un buffer binaire (ex. `total_size = 200000` bytes) ;
   - découpe le buffer en chunks `BinaryChunk` de taille `chunk_size` (ex. 65 536 octets) ;
   - encapsule chaque chunk dans un `Msg { kind: PublishBinaryChunk, ... }` ;
   - diffuse ces messages sur tous les arbres, comme pour `PublishText` ;
   - loggue localement dans `inbox` des lignes du type :
     - `(local BIN t0 file_id=video_demo chunk 1/4 size=65536)`.

2. À la réception d’un `PublishBinaryChunk` :
   - le nœud désérialise le `BinaryChunk` ;
   - met à jour un buffer local indexé par `file_id` ;
   - trace dans `inbox`, par ex. :
     - `(BIN t2 from node3 file_id=video_demo chunk 3/4 size=65536)` ;
   - lorsqu’il a reçu tous les chunks, il reconstitue le buffer complet et trace :
     - `(BIN COMPLETE from node3 file_id=video_demo total_bytes=200000)`.

---

## 4. API HTTP

### 4.1 `GET /stats`

Retourne un JSON de type :

```json
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
```

- `known_count` : nombre de messages distincts vus (texte + binaire + heartbeats).  
- `last_msgs` : derniers messages vus localement.  
- `trees` : nombre d’arbres configurés (`num_trees`).

---

### 4.2 `POST /publish`

Payload JSON :

```json
{
  "payload": "hello from node3"
}
```

Effet :

- le message texte est injecté sur tous les arbres à partir du nœud local ;
- les autres nœuds reçoivent et relaient ce `PublishText`.

---

### 4.3 `POST /publish_binary_demo`

Payload JSON :

```json
{
  "file_id": "video_demo",
  "total_size": 200000,
  "chunk_size": 65536
}
```

Effet :

- le nœud simule l’envoi d’un flux binaire `file_id` découpé en chunks ;
- les nœuds reçoivent, tracent les chunks et reconstituent le fichier lorsque tous les chunks sont arrivés.

---

## 5. Démos possibles

### 5.1 Démarrer le cluster Docker (10 nœuds)

Depuis la racine du repo :

```bash
./scripts/up.sh
```

Vérifier que les conteneurs tournent :

```bash
docker compose -f docker/docker-compose.yml ps
```

Ports HTTP exposés (exemple) :

- node1 → http://localhost:8081  
- node2 → http://localhost:8082  
- node3 → http://localhost:8083  
- …  
- node10 → http://localhost:8090  

---

### 5.2 Démo texte

Publier un message texte depuis `node3` :

```bash
curl -X POST http://localhost:8083/publish   -H "Content-Type: application/json"   -d '{"payload":"hello from node3"}'
```

Observer sur d’autres nœuds :

```bash
curl http://localhost:8081/stats
curl http://localhost:8085/stats
```

On voit apparaître par exemple :

```json
"(t2 from node3) hello from node3"
```

(en fonction des arbres).

---

### 5.3 Démo binaire (chunks)

Lancer la démo binaire depuis `node3` :

```bash
curl -X POST http://localhost:8083/publish_binary_demo   -H "Content-Type: application/json"   -d '{"file_id":"video_demo","total_size":200000,"chunk_size":65536}'
```

Observer sur d’autres nœuds :

```bash
curl http://localhost:8081/stats
curl http://localhost:8085/stats
```

Exemple de `last_msgs` sur `node1` :

```json
"(BIN COMPLETE from node3 file_id=video_demo total_bytes=200000)"
"(BIN t2 from node3 file_id=video_demo chunk 4/4 size=3392)"
"(BIN t2 from node3 file_id=video_demo chunk 3/4 size=65536)"
"(BIN t2 from node3 file_id=video_demo chunk 2/4 size=65536)"
"(BIN t2 from node3 file_id=video_demo chunk 1/4 size=65536)"
"(t2 from node3) hello from node3"
```

---

## 6. Lien avec PeerReview et séparation APP / PR

Cette branche fournit la **machinerie réseau** dont PeerReview a besoin :

- diffusion fiable sur plusieurs arbres ;
- messages sérialisés en **bincode** ;
- HTTP pour piloter / introspecter ;
- base pour l’ajout de **logs sécurisés** et d’**authenticators**.

### 6.1 Séparation future APP / PR

À terme, l’idée est de séparer le transport en deux sockets :

- **socket APP** :
  - messages applicatifs (`PublishText`, `PublishBinaryChunk`, `Heartbeat`, etc.) ;

- **socket PR** :
  - messages PeerReview (`IHAVE`, `REQUEST`, `BATCH`, etc.).

#### Côté config

Dans `nodeX.yaml`, remplacer :

- `listen_addr` → `app_listen_addr` + `pr_listen_addr`.

Dans `cluster.yaml`, remplacer :

- `addr` → `app_addr` + `pr_addr`.

#### Côté code (`main.rs`)

Adapter les structures :

```rust
struct NodeCfg {
    id: String,
    app_listen_addr: String,
    pr_listen_addr: String,
    // ...
}

struct Peer {
    id: String,
    app_addr: String,
    pr_addr: String,
}
```

Ensuite :

- garder la logique actuelle sur la socket **APP** (texte + binaire + heartbeats) ;
- ajouter un deuxième `TcpListener` + boucle de réception pour la socket **PR** ;
- ajouter un deuxième `mpsc::Sender<(Peer, Vec<u8>)>` pour les messages PeerReview ;
- définir une enum `PRMsg` dans `common_proto` pour `IHAVE`, `REQUEST`, `BATCH`, etc., sérialisée en bincode.

#### Côté PeerReview

- brancher les appels `peerreview.on_send(...)` / `peerreview.on_recv(...)` autour de l’envoi/réception des `Msg` (APP) ;
- utiliser la socket PR pour transporter les métadonnées (authenticators, logs, audits) entre witnesses.

---

Ce README décrit ce que fait concrètement la branche `feature/multitree-10nodes` et comment l’utiliser pour des démos de diffusion texte + binaire, tout en expliquant comment elle prépare le terrain pour l’intégration complète de PeerReview.
