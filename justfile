# Conveniences for working on the demo. Running it needs only Docker:
#   ./setup.sh && docker compose up --build

set shell := ["bash", "-euo", "pipefail", "-c"]

# List the recipes
default:
    @just --list

# Generate .env with fresh secrets (once)
setup:
    ./setup.sh

# Build and start everything; the site is at http://localhost:3000
up: setup
    docker compose up --build -d
    @echo "Starting: http://localhost:3000 (the first build takes several minutes)"

# Stop everything (the data is kept)
down:
    docker compose down

# Start again from fresh demo data
reset:
    docker compose down -v
    just up

# Follow the services' logs
logs *services:
    docker compose logs -f {{services}}

# Backend checks as CI runs them, against a throwaway Postgres on port 55432
test-backend:
    docker run -d --rm --name acadex-demo-testdb -p 127.0.0.1:55432:5432 \
        -e POSTGRES_USER=acadex -e POSTGRES_PASSWORD=acadex -e POSTGRES_DB=acadex postgres:17 >/dev/null
    trap 'docker stop acadex-demo-testdb >/dev/null' EXIT; \
        until docker exec acadex-demo-testdb pg_isready -U acadex >/dev/null 2>&1; do sleep 1; done; \
        cd backend && cargo fmt --all --check \
        && cargo clippy --all-targets --locked -- -D warnings \
        && DATABASE_URL=postgres://acadex:acadex@127.0.0.1:55432/acadex cargo test --locked

# Frontend checks as CI runs them
test-frontend:
    cd frontend && pnpm install --frozen-lockfile && pnpm lint && pnpm typecheck
