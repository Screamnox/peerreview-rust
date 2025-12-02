# Guide de Tests NFS

Ce document contient toutes les commandes pour tester l'application NFS avec les résultats attendus.

---

## Prérequis

- Docker et Docker Compose installés
- Ports disponibles : `8091`, `8092`, `8093` (HTTP) et `9001`, `9002`, `9003` (TCP)

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

## 2. Tests des Opérations NFS

### 2.1 Vérifier les statistiques initiales

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

### 2.2 Test WRITE - Écrire un fichier

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

### 2.3 Test READ - Lire le fichier

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

### 2.4 Test LIST - Lister les fichiers

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

### 2.5 Test DELETE - Supprimer le fichier

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

### 2.6 Vérifier la suppression

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

### 2.7 Vérifier l'historique des opérations

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

## 3. Tests Multi-Serveurs

### 3.1 Tester le serveur 2

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

### 3.2 Écrire sur le serveur 2

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

### 3.3 Tester le serveur 3

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

## 4. Tests Avancés

### 4.1 Écriture partielle (avec offset)

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

### 4.2 Test d'erreur - Fichier inexistant

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

### 4.3 Test d'erreur - Suppression fichier inexistant

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

### 4.4 Créer plusieurs fichiers

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

## 5. Vérification des Logs

### 5.1 Voir les logs du serveur 1

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

### 5.2 Suivre les logs en temps réel

```bash
docker logs -f nfs_server1
```

Appuyez sur `Ctrl+C` pour arrêter.

---

## 6. Tests de Performance

### 6.1 Écriture de 100 fichiers

```bash
for i in {1..100}; do
  curl -s -X POST http://localhost:8091/write \
    -H "Content-Type: application/json" \
    -d "{\"path\":\"file_$i.txt\",\"offset\":0,\"data\":\"Content $i\"}" > /dev/null
  echo "Written file $i"
done
```

### 6.2 Vérifier le nombre de fichiers

```bash
curl -s http://localhost:8091/stats | grep file_count
```

**Résultat attendu :**
```
"file_count": 100
```

### 6.3 Lister tous les fichiers

```bash
curl -s -X POST http://localhost:8091/list \
  -H "Content-Type: application/json" \
  -d '{"path":"/"}' | grep -o "file_" | wc -l
```

**Résultat attendu :** `100`

---

## 7. Nettoyage

### 7.1 Arrêter le cluster

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

### 7.2 Supprimer les volumes (optionnel - ATTENTION : efface les données)

```bash
docker compose -f docker/nfs-compose.yml down -v
```

### 7.3 Supprimer l'image (optionnel)

```bash
docker rmi nfs-node
```

---

## 8. Compilation Locale (sans Docker)

### 8.1 Compiler le projet

```bash
cargo build --release --package nfs_node
```

### 8.2 Lancer un serveur local

```bash
cargo run --package nfs_node -- --config configs/docker/nfs_server1.yaml
```

**Note :** Modifier le fichier de config pour changer le `volume_root` si nécessaire.

---

## 9. Troubleshooting

### 9.1 Les ports sont déjà utilisés

```bash
# Vérifier quels processus utilisent les ports
lsof -i :8091
lsof -i :9001

# Ou sur Windows (WSL)
netstat -ano | grep 8091
```

### 9.2 Les conteneurs ne démarrent pas

```bash
# Voir les logs d'erreur
docker compose -f docker/nfs-compose.yml logs

# Redémarrer proprement
docker compose -f docker/nfs-compose.yml down
docker compose -f docker/nfs-compose.yml up -d
```

### 9.3 Erreur de build Docker

```bash
# Nettoyer le cache Docker
docker builder prune -a

# Rebuild sans cache
docker build --no-cache -f docker/Dockerfile.nfs -t nfs-node .
```

---

## 10. Format des Requêtes HTTP

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

## 11. Checklist de Test Complète

- [ ] Build Docker réussi
- [ ] Les 3 serveurs démarrent et sont healthy
- [ ] WRITE fonctionne (code 200, bytes_written correct)
- [ ] READ retourne le bon contenu
- [ ] LIST affiche les fichiers créés
- [ ] DELETE supprime les fichiers
- [ ] Stats affichent l'historique des opérations
- [ ] Les 3 serveurs sont indépendants (données séparées)
- [ ] Gestion d'erreurs (fichier inexistant)
- [ ] Écriture avec offset fonctionne
- [ ] Logs Docker sont corrects
- [ ] Arrêt propre du cluster


