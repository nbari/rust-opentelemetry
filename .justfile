build:
    docker build -t my-postgres-db ./

run:
    docker run -d --name my-postgresdb-container -p 5432:5432 my-postgres-db

load:
    psql -h localhost -U postgres demo < demo.sql
