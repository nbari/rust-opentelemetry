FROM postgres
ENV POSTGRES_PASSWORD docker
ENV POSTGRES_DB demo
# COPY demo.sql /docker-entrypoint-initdb.d/

# psql -h localhost -U postgres
