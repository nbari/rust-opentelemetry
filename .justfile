up:
    docker-compose up

down:
    docker-compose down

load:
    # psql -h localhost -U postgres -XtAc "SELECT 1 FROM pg_database WHERE datname='demo'"
    psql -h localhost -U postgres demo < data/demo.sql
