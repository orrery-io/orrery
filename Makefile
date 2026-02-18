# Load .env if it exists, ignoring any shell-exported DATABASE_URL
ifneq (,$(wildcard .env))
  include .env
  export
endif

.PHONY: help db db-stop db-reset migrate sqlx-prepare test build run ui ui-build

help:
	@echo "Usage: make <target>"
	@echo "  db            Start PostgreSQL (docker compose up -d)"
	@echo "  db-stop       Stop PostgreSQL"
	@echo "  db-reset      Drop and recreate the database"
	@echo "  migrate       Run pending migrations"
	@echo "  sqlx-prepare  Regenerate sqlx offline query cache"
	@echo "  test          Run Rust test suite"
	@echo "  build         Build all Rust crates"
	@echo "  run           Regenerate sqlx cache then start orrery-server"
	@echo "  ui            Start Leptos UI dev server (trunk serve)"
	@echo "  ui-build      Build Leptos UI for production"

db:
	docker compose up -d

db-stop:
	docker compose down

db-reset:
	sqlx database drop -y
	sqlx database create
	sqlx migrate run --source crates/orrery-server/migrations

migrate:
	sqlx migrate run --source crates/orrery-server/migrations

sqlx-prepare:
	cargo sqlx prepare --workspace

test:
	cargo test --workspace

build:
	cargo build

run:
	cargo run -p orrery-server

ui:
	cd crates/orrery-ui && trunk serve

ui-build:
	cd crates/orrery-ui && trunk build --release
