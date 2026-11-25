---
id: THW-001
epic: EPIC-01
title: Initialize Rust workspace, Leptos crate skeleton, and GitHub Actions PR CI
area: infra
status: done
priority: P0
depends_on: []
---

## Context
The monorepo needs a unified Rust Cargo workspace covering `backend/crates/*` and `frontend/`, strict lint/format defaults, and a GitHub Actions CI pipeline that runs tests and checks on every Pull Request.

## Spec & Design Notes
- Reference: [docs/architecture.md](../../docs/architecture.md)
- Create root `Cargo.toml` (`resolver = "2"`) declaring workspace members:
  - `backend/crates/thw-core`
  - `backend/crates/thw-sources`
  - `backend/crates/thw-store`
  - `backend/crates/thw-collector`
  - `backend/crates/thw-server`
  - `frontend`
- Add `rust-toolchain.toml` (stable + `wasm32-unknown-unknown` target) and `.gitignore`.
- Add `.github/workflows/ci.yml` triggered on `pull_request` and `push` to `main`:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `trunk build` (or `cargo check -p thw-frontend --target wasm32-unknown-unknown`)

## Acceptance Criteria
- [ ] Workspace compiles cleanly with `cargo check --workspace` and `cargo check -p thw-frontend --target wasm32-unknown-unknown`.
- [ ] `.github/workflows/ci.yml` validates formatting, clippy warnings-as-errors, workspace tests, and WASM frontend build on every PR.
- [ ] Minimal smoke unit test passes in `thw-core`.

## Test Plan
- Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.

## Execution Log
- *(pending)*
