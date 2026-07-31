FROM python:3.13-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends nodejs ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY target/release/studypulse-runner /usr/local/bin/studypulse-runner
RUN useradd --create-home --uid 10001 runner
USER runner
WORKDIR /tmp
ENTRYPOINT ["/usr/local/bin/studypulse-runner"]
