DATABASE_URL ?= postgres://helterskelter:helterskelter@localhost:5433/helterskelter

.PHONY: db-up db-migrate db-reset

## Start PostgreSQL (docker-compose.yml) and wait for it to be healthy.
db-up:
	docker compose up -d db
	@echo "Waiting for db to be healthy..."
	@until [ "$$(docker inspect -f '{{.State.Health.Status}}' $$(docker compose ps -q db))" = "healthy" ]; do sleep 1; done
	@echo "db is healthy."

## Apply pending migrations from db/migrations/ to DATABASE_URL.
db-migrate:
	DATABASE_URL=$(DATABASE_URL) sqlx migrate run --source db/migrations

## Tear down the db (including its volume) and bring it back up fresh.
db-reset:
	docker compose down -v db
	$(MAKE) db-up
	$(MAKE) db-migrate
