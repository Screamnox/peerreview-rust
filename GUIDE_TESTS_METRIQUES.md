# Guide Complet: Tests et Metriques PeerReview

Ce guide vous accompagne du lancement de l'application Docker jusqu'a la generation des metriques pour valider le protocole PeerReview.

---

## Table des Matieres

1. [Prerequis](#1-prerequis)
2. [Architecture du Projet](#2-architecture-du-projet)
3. [Compilation du Projet](#3-compilation-du-projet)
4. [Lancement du Cluster Docker](#4-lancement-du-cluster-docker)
5. [Verification du Cluster](#5-verification-du-cluster)
6. [Execution des Tests Unitaires](#6-execution-des-tests-unitaires)
7. [Tests d'Integration sur le Cluster](#7-tests-dintegration-sur-le-cluster)
8. [Generation des Metriques](#8-generation-des-metriques)
9. [Injection de Fautes](#9-injection-de-fautes)
10. [Verification des Logs PeerReview](#10-verification-des-logs-peerreview)
11. [Interpretation des Resultats](#11-interpretation-des-resultats)
12. [Troubleshooting](#12-troubleshooting)

---

## 1. Prerequis

### Logiciels requis

```bash
# Verifier les versions
rustc --version    # >= 1.70
cargo --version    # >= 1.70
docker --version   # >= 20.0
docker compose version  # >= 2.0
```

### Installation (si necessaire)

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Docker (Ubuntu/Debian)
sudo apt update
sudo apt install docker.io docker-compose-v2
sudo usermod -aG docker $USER
```

---

## 2. Architecture du Projet

```
peerreview-rust/
├── apps/gossip_node/          # Application gossip (binaire principal)
├── peerreview_protocol/       # Protocole PeerReview
│   ├── src/
│   │   ├── journal/           # Journalisation cryptographique
│   │   ├── metrics/           # Module des 5 metriques
│   │   ├── audit/             # Verification des logs
│   │   └── protocols/         # Challenge/Response, Consistency
│   └── tests/                 # Tests d'integration
│       ├── common/            # Infrastructure de test
│       ├── test_1_baseline.rs # Test comportement correct
│       ├── test_2_tampering.rs# Test modification contenu
│       ├── test_3_silent.rs   # Test noeud silencieux
│       └── test_4_fork.rs     # Test fork de log
├── docker/
│   ├── Dockerfile             # Image Docker
│   └── docker-compose.yml     # Cluster 10 noeuds
├── configs/docker/
│   └── cluster.yaml           # Configuration du cluster
└── scripts/                   # Scripts utilitaires
```

### Topologie du Cluster

```
                    ┌─────────┐
                    │  node1  │ (source)
                    │ :8081   │
                    └────┬────┘
                         │
              ┌──────────┴──────────┐
              │                     │
         ┌────▼────┐          ┌────▼────┐
         │  node2  │          │  node3  │
         │ :8082   │          │ :8083   │
         └────┬────┘          └────┬────┘
              │                    │
        ┌─────┴─────┐        ┌─────┴─────┐
        │           │        │           │
   ┌────▼──┐  ┌────▼──┐ ┌────▼──┐  ┌────▼──┐
   │ node4 │  │ node5 │ │ node6 │  │ node7 │
   └───────┘  └───────┘ └───────┘  └───────┘
        ...         ...
```

- **10 noeuds** sur le reseau `172.30.0.0/24`
- **Ports APP (UDP)**: 7001 (gossip)
- **Ports PR (TCP)**: 7002 (PeerReview)
- **Ports HTTP**: 8081-8090 (API REST)

---

## 3. Compilation du Projet

### Etape 1: Cloner et compiler

```bash
cd /mnt/c/Users/oubar/Desktop/projet\ Sys\ dist/peerreview-rust

# Compilation en mode release (obligatoire pour Docker)
cargo build --release
```

### Etape 2: Verifier le binaire

```bash
ls -la target/release/gossip_node
# -rwxr-xr-x 1 user user 12M ... gossip_node
```

### Etape 3: Compiler les tests

```bash
cargo build --release -p peerreview_protocol --tests
```

---

## 4. Lancement du Cluster Docker

### Methode 1: Script automatique

```bash
./scripts/up.sh
```

### Methode 2: Commandes manuelles

```bash
# Aller dans le repertoire du projet
cd /mnt/c/Users/oubar/Desktop/projet\ Sys\ dist/peerreview-rust

# Construire les images Docker
docker compose -f docker/docker-compose.yml build

# Demarrer le cluster (mode detache)
docker compose -f docker/docker-compose.yml up -d

# Verifier que tous les conteneurs sont running
docker compose -f docker/docker-compose.yml ps
```

### Resultat attendu

```
NAME      IMAGE                    STATUS                   PORTS
node1     peerreview-rust-node1    Up (healthy)            0.0.0.0:8081->8081/tcp
node2     peerreview-rust-node2    Up (healthy)            0.0.0.0:8082->8082/tcp
node3     peerreview-rust-node3    Up (healthy)            0.0.0.0:8083->8083/tcp
...
node10    peerreview-rust-node10   Up (healthy)            0.0.0.0:8090->8090/tcp
```

---

## 5. Verification du Cluster

### Verifier la sante des noeuds

```bash
# Verifier le statut de tous les noeuds
for i in $(seq 1 10); do
  echo "=== Node $i ==="
  curl -s http://localhost:808$i/stats 2>/dev/null || \
  curl -s http://localhost:809${i#1?}/stats 2>/dev/null || \
  echo "Node $i not responding"
done
```

### API REST disponible

| Endpoint | Methode | Description |
|----------|---------|-------------|
| `/stats` | GET | Statistiques du noeud |
| `/publish_text` | POST | Publier un message texte |
| `/publish_binary_demo` | POST | Publier des chunks binaires |

### Test de publication

```bash
# Publier un message depuis node1
curl -X POST http://localhost:8081/publish_text \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello PeerReview!"}'

# Verifier la reception sur node10
curl http://localhost:8090/stats | jq '.last_events'
```

---

## 6. Execution des Tests Unitaires

### Lancer tous les tests

```bash
cd /mnt/c/Users/oubar/Desktop/projet\ Sys\ dist/peerreview-rust

# Tous les tests du module peerreview_protocol
cargo test -p peerreview_protocol --tests
```

### Tests par scenario

```bash
# Test 1: Baseline (comportement correct)
cargo test -p peerreview_protocol --test test_1_baseline -- --nocapture

# Test 2: Tampering (modification de contenu)
cargo test -p peerreview_protocol --test test_2_tampering -- --nocapture

# Test 3: Silent Node (noeud silencieux)
cargo test -p peerreview_protocol --test test_3_silent -- --nocapture

# Test 4: Fork Log (equivocation)
cargo test -p peerreview_protocol --test test_4_fork -- --nocapture
```

### Filtrer un test specifique

```bash
# Exemple: seulement le test de detection tampering
cargo test -p peerreview_protocol test_2_tampering_detection -- --nocapture
```

---

## 7. Tests d'Integration sur le Cluster

### Test de propagation gossip

```bash
# Terminal 1: Observer les logs de node10
docker logs -f node10

# Terminal 2: Publier un message depuis node1
curl -X POST http://localhost:8081/publish_text \
  -H "Content-Type: application/json" \
  -d '{"text": "Integration test message"}'
```

### Script de test automatise

```bash
#!/bin/bash
# test_cluster.sh

echo "=== Test de propagation ==="

# Publier 10 messages
for i in $(seq 1 10); do
  curl -s -X POST http://localhost:8081/publish_text \
    -H "Content-Type: application/json" \
    -d "{\"text\": \"Message $i\"}"
  sleep 0.5
done

# Attendre la propagation
sleep 3

# Verifier la reception
echo "=== Reception sur chaque noeud ==="
for port in 8081 8082 8083 8084 8085 8086 8087 8088 8089 8090; do
  count=$(curl -s http://localhost:$port/stats | jq '.msg_count // 0')
  echo "Port $port: $count messages"
done
```

---

## 8. Generation des Metriques

### Les 5 Metriques Obligatoires

| # | Metrique | Description | Unite |
|---|----------|-------------|-------|
| 1 | Content Delivery Rate | % de chunks recus par noeud | % |
| 2 | Network Traffic | Bytes/messages envoyes | KB, msg |
| 3 | Detection Time | Temps injection -> detection | ms |
| 4 | Propagation Latency | Temps source -> dernier noeud | ms |
| 5 | Fault Detection Accuracy | TP, FP, TN, FN, precision | % |

### Generer les metriques via les tests

```bash
# Executer les tests avec output des metriques
cargo test -p peerreview_protocol --test test_1_baseline \
  test_1_baseline_no_faults -- --nocapture 2>&1 | tee metrics_baseline.txt
```

### Exemple de sortie metriques

```
===================== test_1_baseline ======================
Duration: 749.85ms

Content Delivery:
  - Chunks sent: 1
  - Avg delivery rate: 100.00%

Network Traffic:
  - Total messages: 27
  - Total bytes: 1 KB
  - PR overhead: 15.30%

Detection:
  - No fault detected (baseline or no fault)

Propagation Latency:
  - Min: 14 ms
  - Avg: 18 ms
  - Max: 21 ms
  - P95: 21 ms

Accuracy:
  - Detection rate: 100.00%
  - Precision: 100.00%
  - False positives: 0
  - False negatives: 0
============================================================
```

### Export des metriques (CSV et HTML)

Les tests peuvent sauvegarder les metriques en plusieurs formats:

```rust
// Dans le code de test
let metrics = cluster.finalize_metrics();

// Export CSV
metrics.save_to_csv("metrics_test1.csv")?;

// Export HTML avec graphiques Chart.js interactifs
metrics.save_html_report("metrics_test1.html")?;

// Afficher le rapport ASCII dans la console
metrics.print_report();
```

### Rapport HTML avec visualisations

Le rapport HTML genere inclut:
- **Dashboard** avec metriques cles (delivery rate, detection rate, FP, latence)
- **Graphique barres** : Delivery rate par noeud
- **Matrice de confusion** : TP, FP, TN, FN
- **Distribution latence** : Min, P50, Avg, P95, Max

Les rapports sont generes dans `/tmp/` lors des tests:
- `baseline_load_report.html`
- `tampering_report.html`
- `silent_report.html`
- `fork_report.html`

### Script de collecte des metriques du cluster

```bash
#!/bin/bash
# collect_metrics.sh

OUTPUT_DIR="./metrics_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$OUTPUT_DIR"

echo "=== Collecte des metriques ==="

# Statistiques de chaque noeud
for i in $(seq 1 10); do
  port=$((8080 + i))
  curl -s "http://localhost:$port/stats" > "$OUTPUT_DIR/node${i}_stats.json"
done

# Logs PeerReview
for i in $(seq 1 10); do
  docker cp "node${i}:/app/peerreview_logs/node${i}_app.log" "$OUTPUT_DIR/" 2>/dev/null
done

# Resume
echo "Metriques collectees dans: $OUTPUT_DIR"
ls -la "$OUTPUT_DIR"
```

---

## 9. Injection de Fautes

### Types de fautes disponibles

| Faute | Description | Detection |
|-------|-------------|-----------|
| TamperContent | Modifie le contenu du message | Audit |
| DropAllOutgoing | Ne transmet aucun message | Challenge |
| DropRandomly | Drop aleatoire (probabilite) | Challenge |
| ForkLog | Envoie differents contenus | Consistency |
| DelayMessages | Retarde les messages | Timeout |
| InvalidAuth | Forge des signatures | Verification |

### Injection via les tests

```rust
// Dans test_2_tampering.rs
cluster.inject_fault(2, FaultType::TamperContent {
    pattern: "DATA:".to_string(),
    replacement: "FAKE:".to_string(),
});
```

### Injection manuelle sur le cluster Docker

#### Tampering de hash (simulation de Byzantine)

```bash
# Copier le log du noeud
docker cp node2:/app/peerreview_logs/node2_app.log /tmp/node2_app.log

# Modifier le hash (injection de faute)
./scripts/fault_tamper_hash.sh /tmp/node2_app.log

# Remettre le log modifie
docker cp /tmp/node2_app.log node2:/app/peerreview_logs/node2_app.log
```

#### Verification que la faute est detectee

```bash
# Verifier le log (doit echouer)
./scripts/verify_node_log.sh node2 <pubkey_hex>
```

### Simulation de noeud silencieux

```bash
# Arreter temporairement un noeud
docker pause node2

# Attendre et observer
sleep 10

# Reprendre
docker unpause node2
```

---

## 10. Verification des Logs PeerReview

### Structure d'une entree de log

```json
{
  "seq": 1,
  "peer": 2,
  "kind": "SEND",
  "hash": [72, 101, 108, ...],
  "prev_hash": [0, 0, 0, ...],
  "sig": [142, 56, 78, ...],
  "payload": "msg_id=MSG_001 peer=Some(2) ts_ms=123 hash=..."
}
```

### Verifier un log manuellement

```bash
# Extraire le log
docker cp node1:/app/peerreview_logs/node1_app.log /tmp/node1_app.log

# Verifier avec pr_verify
cargo run -p peerreview_protocol --bin pr_verify -- \
  --log /tmp/node1_app.log \
  --pubkey <pubkey_hex_32_bytes> \
  --strict-chain
```

### Verifier tous les logs du cluster

```bash
#!/bin/bash
# verify_all_logs.sh

echo "=== Verification de tous les logs ==="

# Generer les cles publiques (deterministes depuis le nom)
for i in $(seq 1 10); do
  echo "Verifying node$i..."
  docker cp "node${i}:/app/peerreview_logs/node${i}_app.log" "/tmp/node${i}_app.log"

  # La cle est derivee du nom du noeud
  # Dans le code: SigningKey::from_bytes(&sha256("node$i"))
  cargo run -p peerreview_protocol --bin pr_verify -- \
    --log "/tmp/node${i}_app.log" \
    --pubkey $(cargo run -p peerreview_protocol --bin pr_verify -- --derive-key "node${i}") \
    --strict-chain && echo "OK" || echo "FAILED"
done
```

### Comparer les logs (detection de fork)

```bash
cargo run -p peerreview_protocol --bin pr_verify_cluster -- \
  --log node1:/tmp/node1_app.log \
  --log node2:/tmp/node2_app.log \
  --node-key node1=<key1> \
  --node-key node2=<key2> \
  --strict-chain
```

---

## 11. Interpretation des Resultats

### Resultats attendus par scenario

#### Test 1: Baseline
```
Expected:
  - Delivery rate: 100%
  - Exposed nodes: 0
  - Suspected nodes: 0
  - False positives: 0
  - All nodes: TRUSTED
```

#### Test 2: Tampering
```
Expected:
  - Faulty node (2): EXPOSED or detected
  - Evidence type: InvalidOutput
  - Detection method: Audit
  - Other nodes: TRUSTED
  - False positives: 0
```

#### Test 3: Silent Node
```
Expected:
  - Faulty node (2): SUSPECTED
  - Detection method: Challenge/Response
  - Downstream nodes: may be isolated
  - After rehabilitation: recovers
```

#### Test 4: Fork Log
```
Expected:
  - Faulty node (2): EXPOSED
  - Evidence type: Equivocation
  - Detection method: Consistency
  - Conflicting authenticators detected
```

### Metriques cibles (selon le papier PeerReview)

| Metrique | Valeur cible |
|----------|--------------|
| False Positives | 0 (garantie) |
| False Negatives | 0 (garantie) |
| Detection Rate | 100% |
| Precision | 100% |
| PR Overhead | 15-30% |
| Detection Time | < 20 secondes |

---

## 12. Troubleshooting

### Le cluster ne demarre pas

```bash
# Verifier les logs Docker
docker compose -f docker/docker-compose.yml logs

# Reconstruire les images
docker compose -f docker/docker-compose.yml build --no-cache

# Nettoyer et redemarrer
docker compose -f docker/docker-compose.yml down -v
docker compose -f docker/docker-compose.yml up -d
```

### "Binary not found" dans Docker

```bash
# Recompiler en release
cargo build --release

# Verifier que le binaire existe
ls -la target/release/gossip_node
```

### Les tests echouent

```bash
# Nettoyer et recompiler
cargo clean
cargo build --release -p peerreview_protocol --tests

# Lancer avec plus de details
RUST_BACKTRACE=1 cargo test -p peerreview_protocol --tests -- --nocapture
```

### Port deja utilise

```bash
# Trouver le processus
lsof -i :8081

# Arreter les conteneurs existants
docker compose -f docker/docker-compose.yml down
```

### Logs PeerReview vides

```bash
# Verifier les permissions
docker exec node1 ls -la /app/peerreview_logs/

# Verifier le contenu
docker exec node1 cat /app/peerreview_logs/node1_app.log | head -5
```

---

## Commandes Rapides

```bash
# === DEMARRAGE ===
cargo build --release
./scripts/up.sh

# === TESTS RAPIDES (recommandes) ===
cargo test --lib                              # Tests metriques unitaires
cargo test --test logger_verify               # Test verification journal
cargo test test_baseline_propagation          # Test propagation rapide

# === TOUS LES TESTS ===
cargo test -p peerreview_protocol --tests -- --nocapture

# === TESTS PAR SCENARIO ===
cargo test --test test_1_baseline -- --nocapture   # Baseline
cargo test --test test_2_tampering -- --nocapture  # Tampering
cargo test --test test_3_silent -- --nocapture     # Silent node
cargo test --test test_4_fork -- --nocapture       # Fork/Equivocation

# === VERIFIER CLUSTER ===
for i in $(seq 1 10); do curl -s http://localhost:808$i/stats | jq '.msg_count'; done

# === PUBLIER MESSAGE ===
curl -X POST http://localhost:8081/publish_text -H "Content-Type: application/json" -d '{"text":"Test"}'

# === COLLECTER LOGS ===
for i in $(seq 1 10); do docker cp node$i:/app/peerreview_logs/node${i}_app.log ./logs/; done

# === ARRETER CLUSTER ===
docker compose -f docker/docker-compose.yml down
```

---

## Annexe: Schemas des Metriques

### Structure TestMetrics

```rust
pub struct TestMetrics {
    pub test_name: String,
    pub content_delivery: ContentDeliveryMetrics,
    pub network_traffic: NetworkTrafficMetrics,
    pub detection_time: DetectionTimeMetrics,
    pub propagation: PropagationMetrics,
    pub accuracy: AccuracyMetrics,
    pub test_duration: Duration,
}
```

### Calcul de l'Accuracy

```
Detection Rate (Recall) = TP / (TP + FN)
Precision = TP / (TP + FP)
Accuracy = (TP + TN) / Total

Ou:
  TP = Noeuds fautifs correctement detectes
  FP = Noeuds corrects incorrectement exposes
  TN = Noeuds corrects correctement trusted
  FN = Noeuds fautifs non detectes
```

---

## Changelog

### Version 1.1 (Janvier 2026)
- Renommage des fichiers log: `node{id}.log` → `node{id}_app.log`
- Ajout generation rapport HTML avec graphiques Chart.js
- API `finalize_metrics()` retourne maintenant une copie (plus de problemes de borrow)
- Ajout methode `print_report()` sur `TestMetrics`
- Optimisation des tests (simulation sans sleep)

### Version 1.0
- Creation initiale du guide

---

*Guide genere pour le projet PeerReview-Rust*
*Tests et Metriques - Version 1.1*
