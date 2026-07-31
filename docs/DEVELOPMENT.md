# Development

## Prerequisites

- Xcode 26.6 or newer
- macOS 15 or newer deployment target
- Rust 1.97.1 (pinned by `rust-toolchain.toml`)

The repository does not embed Python. Cloud AI access and refresh tokens are
created by the unified web login and stored only in the macOS Keychain; never
put them in source files, UserDefaults, fixtures, or logs.

The macOS Agent also supports BYOK for OpenAI-compatible providers. Configure
the API key, base URL (for example, `https://api.openai.com/v1`), and model in
Settings → Cloud AI. The complete BYOK configuration is stored in a separate
macOS Keychain item; the key is never stored in UserDefaults, Workspace data,
fixtures, or logs. Requests are sent directly from Rust to the configured
`/chat/completions` endpoint with OpenAI-style tools and streaming.

## First build

Generate the local Swift binding, header, module map, and debug static library:

```sh
core/scripts/build-macos-core.sh
```

Then open `StudyPulse Desktop.xcodeproj`, or build from the command line using
the command in `ARCHITECTURE.md`.

The Xcode project deliberately links the local debug/release Rust library. The
low-level header and module map are generated under ignored `core/target/`
rather than the synchronized Swift source group. Run
the Core generation script after changing a UniFFI-exposed record, enum, or
method. Generated low-level headers and build products are ignored; the
generated high-level `StudyPulseCore.swift` binding is versioned so interface
changes are reviewable.

## Cloud AI walkthrough

1. Create a Workspace.
2. On Agent, choose **Sign In** and complete the unified Cloud AI web login.
3. Enter a learning goal.
4. Observe file and task read calls in the Timeline.
5. When a write Tool such as `create_task` requests `Write`, choose Allow once
   or Deny.
6. On allow, inspect Tasks and `Data/tasks.jsonl`.
7. Canceling while the Agent waits for Cloud AI or confirmation produces a
   terminal Cancelled event.

No permanent Tool authorization is available; every write still needs a
one-time decision.

## Optional Agent services

Deep Research and code-enabled modes can use the following local environment
configuration:

- `STUDYPULSE_SEARXNG_URL`: base URL for a SearXNG instance (for example,
  `http://127.0.0.1:8080`).
- `STUDYPULSE_RUNNER_URL`: optional Docker Runner base URL; defaults to
  `http://127.0.0.1:45891`.
- `STUDYPULSE_RUNNER_TOKEN`: bearer token for the optional Docker Runner.
- `STUDYPULSE_CODE_EXECUTION_BACKEND`: defaults to `local`; set to `docker`
  only when a containerized Runner is intentionally configured.
- `STUDYPULSE_PYTHON`: optional absolute path to the Python 3 executable.

The `code_execution` Tool is visible in the model capabilities and always
requires native Execute confirmation. The default backend starts Python 3 on
the Mac only after the user selects Allow once. It runs with isolated Python
imports, a temporary working directory, a 30-second timeout, 64 KiB stdin and
output bounds, and no shell invocation. This is a consent and resource-limit
boundary, not a security sandbox: approved Python can still access the host
according to the user's permissions.

Set `STUDYPULSE_CODE_EXECUTION_BACKEND=docker` to use the optional
container-verified Runner instead. In that mode, configure the Runner URL and
token; Docker setup is described in
`core/crates/studypulse-runner/README.md`.

The production Worker currently accepts one `message` per `/v1/chat` request
and does not expose server-side tools. The Rust model client therefore owns the
structured tool-call envelope and sends the full bounded Agent context on each
iteration. Do not move this protocol into Swift.

The Agent presents one coherent tool catalog to the model in every mode. Modes
change the workflow guidance and loop budget; they do not hide tools from the
model. The host remains the authority for JSON-schema validation, permission
confirmation, execution, and structured tool results. The model adapter accepts
the StudyPulse envelope plus common OpenAI-style and `<tool_call>` responses so
provider formatting differences do not silently turn an action request into a
plain chat answer.

`code_execution` is intentionally visible even when Docker is not installed.
If local Python is missing, the Tool returns a structured error and suggests
installing Python with Homebrew. The model receives the execution result and
must not invent one.
