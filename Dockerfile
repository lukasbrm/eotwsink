FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY server /app/server

RUN mkdir -p /app/data

EXPOSE 3000

CMD ["/app/server"]
