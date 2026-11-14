FROM rust:1.94.1-slim-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
RUN cargo build --release --locked

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /build/target/release/lottery-dlt /usr/local/bin/lottery-dlt
RUN mkdir -p /app/data && chown 1000:1000 /app/data
USER 1000:1000
ENV LOTTERY_BIND=0.0.0.0:6792 LOTTERY_DB=/app/data/lottery-dlt.sqlite3 TZ=Asia/Shanghai
EXPOSE 6792
VOLUME ["/app/data"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["lottery-dlt", "healthcheck"]
ENTRYPOINT ["lottery-dlt"]
CMD ["serve"]
