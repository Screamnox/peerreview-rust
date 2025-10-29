# 🦀 PeerReview-Rust
Ce projet propose une implémentation en **Rust** de **PeerReview**, un système de responsabilisation pour systèmes distribués, dans le but de reproduire et d'évaluer expérimentalement les résultats présentés dans l'article original.

---

## 📘 Description général de PeerReview

PeerReview permet de **détecter et prouver les comportements fautifs** dans un système distribué. Chaque nœud maintient un **journal inaltérable de ses communications** (via hachage et signatures), et des **nœuds témoins vérifient périodiquement** la conformité des actions en rejouant les opérations enregistrées.

---

## 🧱 Structure du projet

```

peerreview-rust/
├── Cargo.toml                # Dépendances et configuration du projet
├── src/
│   ├── main.rs               # Point d’entrée principal
│   ├── lib.rs                # Exports communs
│   ├── journal/              # Gestion du journal et de la chaîne de hachage
│   ├── network/              # Communication client / serveur
|   ├── protocols/            # Ensemble des protocoles de PeerReview
│   └── types/                # Structures de données et sérialisation
├── tests/                    # Tests unitaires et d’intégration
├── docker/                   # Fichiers Docker et docker-compose
├── .github/workflows/ci.yml  # Intégration continue (tests et lint)
└── README.md                 # Ce fichier

```

---

## ⚙️ Installation et exécution

### Prérequis
- [Rust](https://www.rust-lang.org/tools/install) (>= 1.90)
- [Docker](https://www.docker.com/)
- [Git](https://git-scm.com/)

### Cloner le dépôt

```bash
git clone https://github.com/Screamnox/peerreview-rust.git
cd peerreview-rust
````

### Compilation

```bash
cargo build
```

### Exécution (localement)

```bash
cargo run
```

### Exécution via Docker

```bash
docker-compose up --build
```

*Cela lance un environnement avec plusieurs pairs simulés, communiquant entre eux.*

---

## 🧪 Tests et validation

### Tests unitaires et d’intégration


```bash
cargo test [option] [testname]
```

### Intégration continue

La pipeline CI sur GitHub exécute automatiquement à chaque push sur `main` ou pull request :
- `cargo test` — Exécute l'ensemble des tests
- `cargo clippy` — Analyse statiquement le code
- `cargo fmt` — Vérifie le formatage du code

---

## 🧩 Guide de contribution

### Exemple de workflow git
1. Créez une branche pour chaque tâche :
   - `feature/journal-hash`
   - `fix/network-timeout`
2. Avant de faire un commit :
   - `cargo fmt`
   - `cargo clippy`
   - `cargo test`
3. Ouvrez une Pull Request vers `main`
4. Une **review obligatoire** avant fusion

### Style de code

Le projet suit les conventions idiomatiques de Rust :

* Formatage : `cargo fmt`
* Analyse statique : `cargo clippy -- -D warnings`
* Documentation : `///` pour les fonctions et `//!` pour les modules

### Convention de nommage

| Élément             | Format     | Exemple                        |
| ------------------- | ---------- | ------------------------------ |
| Fichier / module    | snake_case | `hash_chain.rs`                |
| Structure / Enum    | PascalCase | `AuditEntry`, `PeerMessage`    |
| Fonction / variable | snake_case | `append_entry`, `peer_id`      |
| Constante           | SNAKE_CASE | `MAX_ENTRIES`, `DEFAULT_PORT`  |
| Test                | snake_case | `test_append_entry_validity`   |

### Documentation
Chaque fonction publique doit inclure :
```rust
/// Brève description de la fonction.
///
/// # Errors
/// Retourne une erreur si...
///
/// # Examples
/// ```
/// let result = ma_fonction(42)?;
/// ```
pub fn ma_fonction(param: i32) -> Result { ... }
```

### Commit messages

Utiliser une structure claire et explicite :

```
feat(journal): add chained hash verification
fix(network): resolve socket timeout issue
test(types): add serialization test for PeerMessage
```

---

## 💡 Ressources

* [*PeerReview: Practical Accountability for Distributed Systems*](https://dl.acm.org/doi/abs/10.1145/1323293.1294279) — A. Haeberlen, P. Kuznetsov, P. Druschel
* Documentation Rust : [https://doc.rust-lang.org](https://doc.rust-lang.org)
* Serde (sérialisation) : [https://serde.rs](https://serde.rs)
* Tokio (asynchronous runtime) : [https://tokio.rs](https://tokio.rs)
