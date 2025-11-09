#!/bin/bash
set -e

# Aller à la racine du projet
cd "$(dirname "$0")/.."

echo "🛠️  Building Docker images..."
docker compose -f docker/docker-compose.yml build

echo "🚀  Starting containers..."
docker compose -f docker/docker-compose.yml up -d

echo "✅ Containers running:"
docker compose -f docker/docker-compose.yml ps
