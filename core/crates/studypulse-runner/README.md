# StudyPulse Runner

The Runner is an optional, strongly isolated companion for Agent code
execution. The desktop default is local Python after native user confirmation;
the confirmation card warns that local Python is not a security sandbox. Use
`STUDYPULSE_CODE_EXECUTION_BACKEND=docker` when this containerized
backend is preferred. The Runner refuses to execute code unless it detects that
it is inside a container. A locally launched binary is intentionally reported as
`isolation: unverified`.

Build the release binary and image from the `core/` directory:

```sh
cargo build --release -p studypulse-runner
docker build -f crates/studypulse-runner/Dockerfile -t studypulse-runner .
```

Start it with a random `STUDYPULSE_RUNNER_TOKEN` that is also supplied to the
desktop app, but never stored in source control:

```sh
docker run --rm \
  --publish 127.0.0.1:45891:45891 \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=64m \
  --network none --cap-drop ALL --pids-limit 64 \
  --memory 512m --cpus 1 --security-opt no-new-privileges \
  --env STUDYPULSE_RUNNER_TOKEN \
  studypulse-runner
```

When the Docker backend is enabled, the desktop client checks authenticated
`/health` before every execution and only proceeds when it reports
`ok: true` and `isolation: container`. With no external Runner token, the
desktop Core automatically starts and stops a local container from this image.
It generates the bearer token in memory and does not put it in command-line
arguments or logs.

Remote Runner URLs must use HTTPS. Plain HTTP is accepted only for loopback
development endpoints (`localhost`, `127.0.0.1`, or `::1`), and redirects are
not followed so the bearer token cannot be forwarded to another endpoint.

Recommended container flags are loopback-only port publishing, read-only root
filesystem, no network, no host mounts, a non-root user, and explicit CPU,
memory, process, and pids limits.
