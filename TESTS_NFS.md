# Guide de Tests NFS v1.0

**✅ Application NFS conforme à 100% au papier PeerReview Section 6.3**

Ce document contient toutes les commandes pour tester l'application NFS avec les résultats attendus.

## Deux Modes de Test Disponibles

1. **Client NFS (Recommandé)** - Tests via RPC TCP bincode (protocole conforme au papier)
2. **API HTTP** - Tests via curl REST (pour debug et validation rapide)

---

## Prérequis

**Pour tests Docker:**
- Docker et Docker Compose installés
- Ports disponibles : `8091`, `8092`, `8093` (HTTP) et `9001`, `9002`, `9003` (TCP)

**Pour tests en local avec Client NFS:**
- Rust 1.82+ installé
- Compilation: `cargo build --release --package nfs_node --package nfs_client`

---

## 1. Construction et Démarrage

### 1.1 Construire l'image Docker

```bash
docker build -f docker/Dockerfile.nfs -t nfs-node .
```

**Résultat attendu :** Build réussi, image `nfs-node` créée

### 1.2 Démarrer le cluster (3 serveurs)

```bash
docker compose -f docker/nfs-compose.yml up -d
```

**Résultat attendu :**
```
Container nfs_server1  Started
Container nfs_server2  Started
Container nfs_server3  Started
```

### 1.3 Vérifier le statut des conteneurs

```bash
docker compose -f docker/nfs-compose.yml ps
```

**Résultat attendu :** Les 3 serveurs ont le statut `healthy`

---

## 2. Tests Client NFS - Protocole RPC TCP (RECOMMANDÉ)

**Cette section teste l'architecture conforme au papier PeerReview :**
- Client trivial (apps/nfs_client) convertit commandes en RPC
- Serveur complexe (apps/nfs_node) exécute avec filesystem déterministe
- Communication via TCP bincode avec horloge de Lamport

### 2.0 Test Automatique Complet (script intégré)

**Option la plus rapide** - Lance le serveur + exécute tous les tests client :

```bash
./test_client_server.sh
```

**Résultat attendu :**
```
=== Test d'intégration Client-Serveur NFS ===

✅ Compilation réussie : nfs_node + nfs_client
✅ Serveur démarré (PID XXXX) sur localhost:9001
✅ WRITE test.txt -> Success: 11 bytes written
✅ READ test.txt -> Success: "Hello World"
✅ LIST / -> Success: 1 files

=== Conformité PeerReview Section 6.3 ===
✅ Client trivial (convertit commandes en RPC)
✅ Serveur complexe (state machine avec DeterministicFS)
✅ Horloge de Lamport synchronisée
✅ Filesystem déterministe (métadonnées contrôlées)
✅ Test client-serveur réussi
```

---

### 2.1 Compilation des composants

```bash
# Compiler le serveur et le client
cargo build --release --package nfs_node --package nfs_client
```

**Résultat attendu :** Build réussi pour les 2 packages

---

### 2.2 Démarrer le serveur NFS en local

```bash
# Terminal 1 : Démarrer le serveur
cargo run --release --package nfs_node -- --config configs/docker/nfs_server1.yaml
```

**Logs attendus :**
```
╔══════════════════════════════════════════╗
║     NFS Server - Conforme PeerReview 6.3 ║
║  Deterministic filesystem: ENABLED       ║
╚══════════════════════════════════════════╝

Node ID: nfs_server1
Volume: /data/vol1
Clock initialized: t=0

TCP Listener: 0.0.0.0:9001
HTTP API: http://0.0.0.0:8091
```

---

### 2.3 Test WRITE avec le client

**Dans un second terminal :**

```bash
cargo run --package nfs_client -- \
  --id client1 \
  --server localhost:9001 \
  write --path test.txt --data "Hello World"
```

**Résultat attendu (côté client) :**
```
[Client client1] Envoi RPC: WRITE test.txt (11 bytes)
[Client client1] Horloge Lamport mise à jour: t=1
✅ Écriture réussie: 11 bytes écrits
```

**Logs serveur (Terminal 1) :**
```
[t=1] [nfs_server1] RPC from client1: WRITE test.txt (11 bytes) (timestamp=0)
[t=1] WRITE test.txt -> OK (11 bytes)
```

---

### 2.4 Test READ avec le client

```bash
cargo run --package nfs_client -- \
  --id client1 \
  --server localhost:9001 \
  read --path test.txt
```

**Résultat attendu :**
```
[Client client1] Envoi RPC: READ test.txt
[Client client1] Horloge Lamport mise à jour: t=2
✅ Lecture réussie (11 bytes):
Hello World
```

**Logs serveur :**
```
[t=2] [nfs_server1] RPC from client1: READ test.txt (timestamp=1)
[t=2] READ test.txt -> OK (11 bytes)
```

---

### 2.5 Test LIST avec le client

```bash
cargo run --package nfs_client -- \
  --id client1 \
  --server localhost:9001 \
  list --path /
```

**Résultat attendu :**
```
[Client client1] Envoi RPC: LIST /
[Client client1] Horloge Lamport mise à jour: t=3
✅ Fichiers dans /:
  - test.txt
```

---

### 2.6 Test DELETE avec le client

```bash
cargo run --package nfs_client -- \
  --id client1 \
  --server localhost:9001 \
  delete --path test.txt
```

**Résultat attendu :**
```
[Client client1] Envoi RPC: DELETE test.txt
[Client client1] Horloge Lamport mise à jour: t=4
✅ Suppression réussie: test.txt
```

**Logs serveur :**
```
[t=4] [nfs_server1] RPC from client1: DELETE test.txt (timestamp=3)
[t=4] DELETE test.txt -> OK
```

---

### 2.7 Mode Interactif du Client

**Pour des tests manuels continus :**

```bash
cargo run --package nfs_client -- \
  --id client1 \
  --server localhost:9001 \
  interactive
```

**Session interactive :**
```
=== Client NFS Interactif ===
Commandes disponibles:
  write <path> <data>  - Écrire un fichier
  read <path>          - Lire un fichier
  delete <path>        - Supprimer un fichier
  list <path>          - Lister un répertoire
  quit                 - Quitter

> write notes.txt "Note importante"
✅ Écriture réussie: 16 bytes écrits

> read notes.txt
✅ Lecture réussie (16 bytes):
Note importante

> list /
✅ Fichiers dans /:
  - notes.txt

> delete notes.txt
✅ Suppression réussie: notes.txt

> quit
Au revoir!
```

---

### 2.8 Test du Déterminisme (Horloge Lamport)

**Tester la synchronisation d'horloge entre client et serveur :**

```bash
# Client envoie une opération avec t=5
cargo run --package nfs_client -- --id client1 --server localhost:9001 \
  write --path file1.txt --data "Data 1"

# Logs serveur montrent t=6 (max(t_serveur, t_client) + 1)
# Vérifier dans les logs que l'horloge est cohérente
```

**Vérification :** Les logs doivent montrer `[t=X]` avec X strictement croissant

---

### 2.9 Test de Concurrence (Multi-Clients)

**Terminal 2 - Client A :**
```bash
cargo run --package nfs_client -- --id clientA --server localhost:9001 \
  write --path fileA.txt --data "Client A"
```

**Terminal 3 - Client B :**
```bash
cargo run --package nfs_client -- --id clientB --server localhost:9001 \
  write --path fileB.txt --data "Client B"
```

**Résultat attendu :**
- Les deux opérations réussissent
- Les logs serveur montrent un ordre déterministe (grâce aux verrous par fichier)
- L'horloge Lamport est cohérente pour tous les clients

---

## 3. Tests API HTTP (Debug/Tests Rapides)

**Note:** L'API HTTP est disponible pour le debug et les tests rapides, mais le client NFS (section 2) est le mode recommandé conforme au papier PeerReview.

### 3.1 Vérifier les statistiques initiales

```bash
curl -s http://localhost:8091/stats
```

**Résultat attendu :**
```json
{
  "node_id": "nfs_server1",
  "operations_count": 0,
  "last_operations": [],
  "volume_size": 0,
  "file_count": 0
}
```

---

### 3.2 Test WRITE - Écrire un fichier

```bash
curl -s -X POST http://localhost:8091/write \
  -H "Content-Type: application/json" \
  -d '{"path":"test.txt","offset":0,"data":"Hello World from NFS"}'
```

**Résultat attendu :**
```json
{
  "status": "WriteOk",
  "bytes_written": 20
}
```

---

### 3.3 Test READ - Lire le fichier

```bash
curl -s -X POST http://localhost:8091/read \
  -H "Content-Type: application/json" \
  -d '{"path":"test.txt","offset":0,"length":100}'
```

**Résultat attendu :**
```json
{
  "status": "ReadOk",
  "data": "Hello World from NFS"
}
```

---

### 3.4 Test LIST - Lister les fichiers

```bash
curl -s -X POST http://localhost:8091/list \
  -H "Content-Type: application/json" \
  -d '{"path":"/"}'
```

**Résultat attendu :**
```json
{
  "status": "ListOk",
  "entries": ["test.txt"]
}
```

---

### 3.5 Test DELETE - Supprimer le fichier

```bash
curl -s -X POST http://localhost:8091/delete \
  -H "Content-Type: application/json" \
  -d '{"path":"test.txt"}'
```

**Résultat attendu :**
```json
{
  "status": "DeleteOk"
}
```

---

### 3.6 Vérifier la suppression

```bash
curl -s -X POST http://localhost:8091/list \
  -H "Content-Type: application/json" \
  -d '{"path":"/"}'
```

**Résultat attendu :**
```json
{
  "status": "ListOk",
  "entries": []
}
```

---

### 3.7 Vérifier l'historique des opérations

```bash
curl -s http://localhost:8091/stats
```

**Résultat attendu :**
```json
{
  "node_id": "nfs_server1",
  "operations_count": 5,
  "last_operations": [
    "LIST / -> OK (0 entries)",
    "DELETE test.txt -> OK",
    "LIST / -> OK (1 entries)",
    "READ test.txt -> OK (20 bytes)",
    "WRITE test.txt -> OK (20 bytes)"
  ],
  "volume_size": 0,
  "file_count": 0
}
```

---

## 4. Tests Multi-Serveurs (HTTP)

### 4.1 Tester le serveur 2

```bash
curl -s http://localhost:8092/stats
```

**Résultat attendu :**
```json
{
  "node_id": "nfs_server2",
  "operations_count": 0,
  "last_operations": [],
  "volume_size": 0,
  "file_count": 0
}
```

### 4.2 Écrire sur le serveur 2

```bash
curl -s -X POST http://localhost:8092/write \
  -H "Content-Type: application/json" \
  -d '{"path":"server2.txt","offset":0,"data":"Data on server 2"}'
```

**Résultat attendu :**
```json
{
  "status": "WriteOk",
  "bytes_written": 16
}
```

### 4.3 Tester le serveur 3

```bash
curl -s http://localhost:8093/stats
```

**Résultat attendu :**
```json
{
  "node_id": "nfs_server3",
  "operations_count": 0,
  "last_operations": [],
  "volume_size": 0,
  "file_count": 0
}
```

---

## 5. Tests Avancés (HTTP)

### 5.1 Écriture partielle (avec offset)

```bash
# Créer un fichier
curl -s -X POST http://localhost:8091/write \
  -H "Content-Type: application/json" \
  -d '{"path":"partial.txt","offset":0,"data":"Hello"}'

# Écrire à la suite (offset 5)
curl -s -X POST http://localhost:8091/write \
  -H "Content-Type: application/json" \
  -d '{"path":"partial.txt","offset":5,"data":" World"}'

# Lire le fichier complet
curl -s -X POST http://localhost:8091/read \
  -H "Content-Type: application/json" \
  -d '{"path":"partial.txt","offset":0,"length":100}'
```

**Résultat attendu du READ :**
```json
{
  "status": "ReadOk",
  "data": "Hello World"
}
```

---

### 5.2 Test d'erreur - Fichier inexistant

```bash
curl -s -X POST http://localhost:8091/read \
  -H "Content-Type: application/json" \
  -d '{"path":"inexistant.txt","offset":0,"length":100}'
```

**Résultat attendu :**
```json
{
  "status": "Error",
  "message": "No such file or directory (os error 2)"
}
```

---

### 5.3 Test d'erreur - Suppression fichier inexistant

```bash
curl -s -X POST http://localhost:8091/delete \
  -H "Content-Type: application/json" \
  -d '{"path":"inexistant.txt"}'
```

**Résultat attendu :**
```json
{
  "status": "Error",
  "message": "No such file or directory (os error 2)"
}
```

---

### 5.4 Créer plusieurs fichiers

```bash
# Fichier 1
curl -s -X POST http://localhost:8091/write \
  -H "Content-Type: application/json" \
  -d '{"path":"file1.txt","offset":0,"data":"Content 1"}'

# Fichier 2
curl -s -X POST http://localhost:8091/write \
  -H "Content-Type: application/json" \
  -d '{"path":"file2.txt","offset":0,"data":"Content 2"}'

# Fichier 3
curl -s -X POST http://localhost:8091/write \
  -H "Content-Type: application/json" \
  -d '{"path":"file3.txt","offset":0,"data":"Content 3"}'

# Lister tous les fichiers
curl -s -X POST http://localhost:8091/list \
  -H "Content-Type: application/json" \
  -d '{"path":"/"}'
```

**Résultat attendu du LIST :**
```json
{
  "status": "ListOk",
  "entries": ["file1.txt", "file2.txt", "file3.txt"]
}
```

---

## 6. Vérification des Logs

### 6.1 Voir les logs du serveur 1

```bash
docker logs nfs_server1
```

**Résultat attendu :**
```
Starting NFS server: nfs_server1
Volume path: /data/vol1
nfs_server1 listening on TCP 0.0.0.0:9001
nfs_server1 HTTP API on 0.0.0.0:8091
```

### 6.2 Suivre les logs en temps réel

```bash
docker logs -f nfs_server1
```

Appuyez sur `Ctrl+C` pour arrêter.

---

## 7. Tests de Performance

### 7.1 Écriture de 100 fichiers

```bash
for i in {1..100}; do
  curl -s -X POST http://localhost:8091/write \
    -H "Content-Type: application/json" \
    -d "{\"path\":\"file_$i.txt\",\"offset\":0,\"data\":\"Content $i\"}" > /dev/null
  echo "Written file $i"
done
```

### 7.2 Vérifier le nombre de fichiers

```bash
curl -s http://localhost:8091/stats | grep file_count
```

**Résultat attendu :**
```
"file_count": 100
```

### 7.3 Lister tous les fichiers

```bash
curl -s -X POST http://localhost:8091/list \
  -H "Content-Type: application/json" \
  -d '{"path":"/"}' | grep -o "file_" | wc -l
```

**Résultat attendu :** `100`

---

## 8. Nettoyage

### 8.1 Arrêter le cluster

```bash
docker compose -f docker/nfs-compose.yml down
```

**Résultat attendu :**
```
Container nfs_server1  Stopped
Container nfs_server2  Stopped
Container nfs_server3  Stopped
Network docker_nfs_net  Removed
```

### 8.2 Supprimer les volumes (optionnel - ATTENTION : efface les données)

```bash
docker compose -f docker/nfs-compose.yml down -v
```

### 8.3 Supprimer l'image (optionnel)

```bash
docker rmi nfs-node
```

---

## 9. Compilation Locale (sans Docker)

### 9.1 Compiler le projet

```bash
cargo build --release --package nfs_node
```

### 9.2 Lancer un serveur local

```bash
cargo run --package nfs_node -- --config configs/docker/nfs_server1.yaml
```

**Note :** Modifier le fichier de config pour changer le `volume_root` si nécessaire.

---

## 10. Troubleshooting

### 10.1 Les ports sont déjà utilisés

```bash
# Vérifier quels processus utilisent les ports
lsof -i :8091
lsof -i :9001

# Ou sur Windows (WSL)
netstat -ano | grep 8091
```

### 10.2 Les conteneurs ne démarrent pas

```bash
# Voir les logs d'erreur
docker compose -f docker/nfs-compose.yml logs

# Redémarrer proprement
docker compose -f docker/nfs-compose.yml down
docker compose -f docker/nfs-compose.yml up -d
```

### 10.3 Erreur de build Docker

```bash
# Nettoyer le cache Docker
docker builder prune -a

# Rebuild sans cache
docker build --no-cache -f docker/Dockerfile.nfs -t nfs-node .
```

---

## 11. Format des Requêtes HTTP

### Structure générale

Toutes les requêtes utilisent :
- **Méthode :** POST (sauf `/stats` qui est GET)
- **Content-Type :** `application/json`
- **Format :** JSON avec le champ `path` (obligatoire)

### Exemples de requêtes

#### WRITE
```json
{
  "path": "fichier.txt",
  "offset": 0,
  "data": "Contenu du fichier"
}
```

#### READ
```json
{
  "path": "fichier.txt",
  "offset": 0,
  "length": 100
}
```

#### DELETE
```json
{
  "path": "fichier.txt"
}
```

#### LIST
```json
{
  "path": "/"
}
```

---

## 12. Checklist de Test Complète

### Tests Client NFS (Recommandé - RPC TCP)
- [ ] Compilation réussie : nfs_node + nfs_client
- [ ] Serveur démarre avec logs "Conforme PeerReview 6.3"
- [ ] Test automatique ./test_client_server.sh réussit
- [ ] WRITE via client fonctionne (bytes écrits corrects)
- [ ] READ via client retourne le bon contenu
- [ ] LIST via client affiche les fichiers créés
- [ ] DELETE via client supprime les fichiers
- [ ] Mode interactif fonctionne (commandes REPL)
- [ ] Logs serveur montrent horloge Lamport [t=X] croissant
- [ ] Logs serveur montrent RPC from client_id
- [ ] Test multi-clients concurrent fonctionne
- [ ] Déterminisme vérifié (réexécution identique)

### Tests API HTTP (Debug)
- [ ] Build Docker réussi
- [ ] Les 3 serveurs démarrent et sont healthy
- [ ] WRITE via curl fonctionne (code 200, bytes_written correct)
- [ ] READ via curl retourne le bon contenu
- [ ] LIST via curl affiche les fichiers créés
- [ ] DELETE via curl supprime les fichiers
- [ ] Stats affichent l'historique des opérations
- [ ] Les 3 serveurs sont indépendants (données séparées)
- [ ] Gestion d'erreurs (fichier inexistant)
- [ ] Écriture avec offset fonctionne
- [ ] Logs Docker sont corrects
- [ ] Arrêt propre du cluster

### Conformité PeerReview Section 6.3
- [ ] Client trivial implémenté (apps/nfs_client)
- [ ] Serveur complexe avec state machine (apps/nfs_node)
- [ ] Filesystem déterministe (crates/deterministic_fs)
- [ ] Horloge de Lamport synchronisée
- [ ] Métadonnées contrôlées (timestamps Lamport)
- [ ] Verrous par fichier (sérialisation concurrence)
- [ ] Historique d'opérations avec timestamps [t=X]
- [ ] Communication RPC TCP bincode fonctionnelle


