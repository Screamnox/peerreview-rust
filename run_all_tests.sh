#!/bin/bash
# =============================================================================
# Script d'execution complete des tests et generation des metriques PeerReview
# =============================================================================

set -e

# Couleurs pour l'affichage
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Repertoire du projet
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_DIR"

# Repertoire de sortie des metriques
METRICS_DIR="$PROJECT_DIR/metrics_output"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
OUTPUT_DIR="$METRICS_DIR/$TIMESTAMP"

echo -e "${BLUE}=============================================${NC}"
echo -e "${BLUE}   PeerReview - Tests & Metriques Runner     ${NC}"
echo -e "${BLUE}=============================================${NC}"
echo ""

# Fonction d'aide
show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --build        Compiler le projet avant les tests"
    echo "  --docker       Demarrer le cluster Docker"
    echo "  --tests        Executer les tests unitaires"
    echo "  --integration  Executer les tests d'integration sur le cluster"
    echo "  --metrics      Collecter les metriques du cluster"
    echo "  --all          Executer toutes les etapes"
    echo "  --clean        Nettoyer et arreter le cluster"
    echo "  --help         Afficher cette aide"
    echo ""
    echo "Exemples:"
    echo "  $0 --all              # Execution complete"
    echo "  $0 --build --tests    # Compiler et tester"
    echo "  $0 --docker --metrics # Demarrer cluster et collecter metriques"
}

# Fonction: Compilation
do_build() {
    echo -e "\n${YELLOW}[1/5] Compilation du projet...${NC}"
    cargo build --release
    cargo build --release -p peerreview_protocol --tests
    echo -e "${GREEN}Compilation terminee.${NC}"
}

# Fonction: Demarrer Docker
do_docker() {
    echo -e "\n${YELLOW}[2/5] Demarrage du cluster Docker (10 noeuds)...${NC}"

    # Arreter si deja en cours
    docker compose -f docker/docker-compose.yml down 2>/dev/null || true

    # Construire et demarrer
    docker compose -f docker/docker-compose.yml build
    docker compose -f docker/docker-compose.yml up -d

    # Attendre que tous les noeuds soient healthy
    echo "Attente du demarrage des noeuds..."
    for i in $(seq 1 30); do
        healthy=$(docker compose -f docker/docker-compose.yml ps --format json 2>/dev/null | grep -c '"healthy"' || echo "0")
        if [ "$healthy" -eq 10 ]; then
            echo -e "${GREEN}Tous les 10 noeuds sont healthy.${NC}"
            break
        fi
        echo "  En attente... ($healthy/10 noeuds ready)"
        sleep 2
    done

    # Afficher le statut
    docker compose -f docker/docker-compose.yml ps
}

# Fonction: Tests unitaires
do_tests() {
    echo -e "\n${YELLOW}[3/5] Execution des tests unitaires...${NC}"

    mkdir -p "$OUTPUT_DIR"

    echo -e "\n${BLUE}--- Test 1: Baseline (comportement correct) ---${NC}"
    cargo test -p peerreview_protocol --test test_1_baseline -- --nocapture 2>&1 | tee "$OUTPUT_DIR/test_1_baseline.log"

    echo -e "\n${BLUE}--- Test 2: Tampering (modification contenu) ---${NC}"
    cargo test -p peerreview_protocol --test test_2_tampering -- --nocapture 2>&1 | tee "$OUTPUT_DIR/test_2_tampering.log"

    echo -e "\n${BLUE}--- Test 3: Silent Node (noeud silencieux) ---${NC}"
    cargo test -p peerreview_protocol --test test_3_silent -- --nocapture 2>&1 | tee "$OUTPUT_DIR/test_3_silent.log"

    echo -e "\n${BLUE}--- Test 4: Fork Log (equivocation) ---${NC}"
    cargo test -p peerreview_protocol --test test_4_fork -- --nocapture 2>&1 | tee "$OUTPUT_DIR/test_4_fork.log"

    echo -e "${GREEN}Tests unitaires termines. Logs dans: $OUTPUT_DIR${NC}"
}

# Fonction: Tests d'integration
do_integration() {
    echo -e "\n${YELLOW}[4/5] Tests d'integration sur le cluster...${NC}"

    mkdir -p "$OUTPUT_DIR"

    # Verifier que le cluster est up
    if ! docker compose -f docker/docker-compose.yml ps | grep -q "Up"; then
        echo -e "${RED}Erreur: Le cluster Docker n'est pas demarre. Utilisez --docker d'abord.${NC}"
        return 1
    fi

    echo "Test de propagation gossip..."

    # Publier des messages de test
    for i in $(seq 1 5); do
        curl -s -X POST http://localhost:8081/publish_text \
            -H "Content-Type: application/json" \
            -d "{\"text\": \"Integration test message $i - $(date +%H:%M:%S)\"}" > /dev/null
        sleep 0.5
    done

    echo "Attente de la propagation (3s)..."
    sleep 3

    # Collecter les statistiques
    echo -e "\n${BLUE}Statistiques de reception:${NC}"
    echo "Node,Messages,Bytes" > "$OUTPUT_DIR/integration_stats.csv"

    for i in $(seq 1 10); do
        port=$((8080 + i))
        stats=$(curl -s "http://localhost:$port/stats" 2>/dev/null || echo '{"msg_count":0}')
        msg_count=$(echo "$stats" | grep -o '"msg_count":[0-9]*' | cut -d: -f2 || echo "0")
        echo "  Node$i: $msg_count messages"
        echo "node$i,$msg_count,0" >> "$OUTPUT_DIR/integration_stats.csv"
    done

    echo -e "${GREEN}Tests d'integration termines.${NC}"
}

# Fonction: Collecter les metriques
do_metrics() {
    echo -e "\n${YELLOW}[5/5] Collecte des metriques...${NC}"

    mkdir -p "$OUTPUT_DIR/logs"
    mkdir -p "$OUTPUT_DIR/stats"

    # Verifier que le cluster est up
    if ! docker compose -f docker/docker-compose.yml ps 2>/dev/null | grep -q "Up"; then
        echo -e "${YELLOW}Cluster non demarre - collecte des metriques des tests seulement${NC}"
    else
        # Collecter les logs PeerReview
        echo "Collecte des logs PeerReview..."
        for i in $(seq 1 10); do
            docker cp "node${i}:/app/peerreview_logs/node${i}.log" "$OUTPUT_DIR/logs/" 2>/dev/null || true
        done

        # Collecter les statistiques JSON
        echo "Collecte des statistiques des noeuds..."
        for i in $(seq 1 10); do
            port=$((8080 + i))
            curl -s "http://localhost:$port/stats" > "$OUTPUT_DIR/stats/node${i}_stats.json" 2>/dev/null || true
        done
    fi

    # Generer le rapport de synthese
    echo "Generation du rapport de synthese..."

    cat > "$OUTPUT_DIR/RAPPORT_METRIQUES.md" << EOF
# Rapport des Metriques PeerReview

**Date:** $(date "+%Y-%m-%d %H:%M:%S")
**Projet:** peerreview-rust

## Resume des Tests

### Tests Unitaires

| Test | Scenario | Statut |
|------|----------|--------|
| Test 1 | Baseline (comportement correct) | $(grep -q "ok" "$OUTPUT_DIR/test_1_baseline.log" 2>/dev/null && echo "PASS" || echo "N/A") |
| Test 2 | Tampering (modification contenu) | $(grep -q "ok" "$OUTPUT_DIR/test_2_tampering.log" 2>/dev/null && echo "PASS" || echo "N/A") |
| Test 3 | Silent Node (noeud silencieux) | $(grep -q "ok" "$OUTPUT_DIR/test_3_silent.log" 2>/dev/null && echo "PASS" || echo "N/A") |
| Test 4 | Fork Log (equivocation) | $(grep -q "ok" "$OUTPUT_DIR/test_4_fork.log" 2>/dev/null && echo "PASS" || echo "N/A") |

## Fichiers Generes

- \`logs/\` - Journaux PeerReview de chaque noeud
- \`stats/\` - Statistiques JSON de chaque noeud
- \`test_*.log\` - Sortie des tests unitaires
- \`integration_stats.csv\` - Statistiques d'integration

## Metriques Cles (Objectifs)

| Metrique | Objectif |
|----------|----------|
| False Positives | 0 |
| False Negatives | 0 |
| Detection Rate | 100% |
| PR Overhead | < 30% |

EOF

    echo -e "${GREEN}Metriques collectees dans: $OUTPUT_DIR${NC}"
    echo ""
    echo "Fichiers generes:"
    ls -la "$OUTPUT_DIR/"
}

# Fonction: Nettoyage
do_clean() {
    echo -e "\n${YELLOW}Nettoyage...${NC}"

    # Arreter Docker
    docker compose -f docker/docker-compose.yml down -v 2>/dev/null || true

    # Nettoyer les fichiers temporaires
    rm -rf /tmp/node*.log 2>/dev/null || true

    echo -e "${GREEN}Nettoyage termine.${NC}"
}

# Parsing des arguments
if [ $# -eq 0 ]; then
    show_help
    exit 0
fi

DO_BUILD=false
DO_DOCKER=false
DO_TESTS=false
DO_INTEGRATION=false
DO_METRICS=false
DO_CLEAN=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --build)
            DO_BUILD=true
            shift
            ;;
        --docker)
            DO_DOCKER=true
            shift
            ;;
        --tests)
            DO_TESTS=true
            shift
            ;;
        --integration)
            DO_INTEGRATION=true
            shift
            ;;
        --metrics)
            DO_METRICS=true
            shift
            ;;
        --all)
            DO_BUILD=true
            DO_DOCKER=true
            DO_TESTS=true
            DO_INTEGRATION=true
            DO_METRICS=true
            shift
            ;;
        --clean)
            DO_CLEAN=true
            shift
            ;;
        --help)
            show_help
            exit 0
            ;;
        *)
            echo -e "${RED}Option inconnue: $1${NC}"
            show_help
            exit 1
            ;;
    esac
done

# Execution des etapes selectionnees
if $DO_CLEAN; then
    do_clean
fi

if $DO_BUILD; then
    do_build
fi

if $DO_DOCKER; then
    do_docker
fi

if $DO_TESTS; then
    do_tests
fi

if $DO_INTEGRATION; then
    do_integration
fi

if $DO_METRICS; then
    do_metrics
fi

echo ""
echo -e "${GREEN}=============================================${NC}"
echo -e "${GREEN}   Execution terminee avec succes!           ${NC}"
echo -e "${GREEN}=============================================${NC}"

if [ -d "$OUTPUT_DIR" ]; then
    echo ""
    echo -e "Resultats disponibles dans: ${BLUE}$OUTPUT_DIR${NC}"
fi
