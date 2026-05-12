## [unreleased]

## [0.9.3] - 2026-05-08

### 🔒 Security

- Verify package tarball integrity against npm registry `integrity` (SHA-512) and `shasum` (SHA-1) fields before extraction

### 🧪 Tests

- Add unit tests for integrity verification (valid hash, tampered bytes, empty input, unknown algorithm, malformed string)

### ⚙️ Miscellaneous Tasks

- Simplify release workflow, add workflow_dispatch
- Remove redundant comments from source files

## [0.3.0] - 2026-05-05

### 🚀 Features

- install
- uninstall
- login
