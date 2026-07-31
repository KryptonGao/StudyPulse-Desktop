# StudyPulse Desktop Architecture

StudyPulse Desktop is a native-UI application over a platform-neutral Rust
core. The MVP ships a macOS 15 SwiftUI client, while all Workspace, data,
Agent, Tool, permission, and backup behavior lives below the FFI boundary.
A future Windows client replaces the SwiftUI and Apple integration layers with
C# UI code and uses the same façade and wire protocol.

## Platform boundary

```text
macOS SwiftUI UI                         future Windows C# UI
        │                                      │
        └────────── simple UniFFI DTOs ─────────┘
                              │
                     studypulse-ffi
                              │
                     studypulse-agent
                    ┌─────────┴─────────┐
             studypulse-tools   studypulse-model-client
                    │
             studypulse-workspace
```

The dependency graph is acyclic:

- `studypulse-workspace` owns paths, JSONL, Workspace layout, data DTOs, and
  backup transactions.
- `studypulse-tools` depends only on Workspace and owns schemas, typed argument
  parsing, permissions, and Tool execution.
- `studypulse-model-client` owns the Cloud AI HTTP/auth protocol, structured
  Agent response envelope, error mapping, and deterministic test mock. It has
  no Workspace or UI dependency.
- `studypulse-agent` coordinates model calls and Tools. It depends on Tools,
  Workspace, and the model protocol.
- `studypulse-ffi` is the platform-neutral façade. It translates public DTOs
  and owns no business rules.
- `macOS/StudyPulseMac` owns SwiftUI, AppKit file panels, security-scoped
  bookmark lifetime, and main-actor state presentation.

No Core crate imports Foundation, AppKit, Swift, WinUI, Windows APIs, or C#
types. There is no embedded Python runtime. Cloud AI is contacted only by the
Rust model client; Swift never constructs chat, profile, refresh, or logout
requests.

## Workspace and path security

The capability root is the canonical path of the currently opened Workspace.
Public paths are validated Workspace-relative strings using `/` separators.
The Core rejects:

- absolute paths, `..`, empty/double components, and backslashes;
- drive and UNC forms;
- symbolic-link traversal;
- Windows reparse points and junction-like entries;
- any resolved path outside the canonical Workspace root.

Library traversal is limited to `Documents/` and `Notes/`. It does not follow
links, descends neither hidden directories nor hidden files, rejects binary
content, limits searchable files to 1 MiB, and caps results. The Tool Registry
contains no shell, process, delete, or arbitrary path capability.

Workspace metadata contains only a UUID, format identifier, and schema
version. It never stores an absolute path, Apple bookmark, or Windows drive
letter. Platform UI owns authorization handles.

## Data format

User records are JSONL. Tasks use the iOS-compatible envelope:

```json
{"dtoVersion":1,"id":"UUID","updatedAt":null,"value":{"id":"UUID","title":"…"}}
```

`TaskItem` retains the iOS fields `id`, `title`, `type`, `dueDate`,
`reminderDate`, `subject`, `importance`, `notes`, `isCompleted`,
`reminderEventId`, `reminderCalendarId`, `createdAt`, `phaseId`,
`coachExecutionData`, `coachGoalId`, and `coachProposalId`. Dates are validated
ISO-8601 strings, IDs are UUIDs, and `coachExecutionData` is validated Base64.
Unknown backup domains and fields are preserved as schema-aware JSON values.

## Agent lifecycle

One `AgentRuntime` permits one active run. A run selects one of six native
Capability modes: `chat`, `deep_solve`, `mastery`, `deep_research`,
`question_lab`, or `visualize`. Each mode publishes a manifest with its stages
and loop budget; the model prompt receives the selected mode and stage list.
The current loop remains bounded, while stage start/progress/completion events
make the multi-stage behavior visible without exposing hidden chain-of-thought.
Cancellation interrupts model, confirmation, and learner-input waiting and is
checked between Tool operations. Each event has a monotonically increasing
sequence and is appended to:

`Agent/runs/<run-id>.jsonl`

The event set is `Started`, `StatusChanged`, `TextDelta`, `ToolRequested`,
`ToolCompleted`, `ConfirmationRequired`, `Failed`, `Cancelled`, and
`Completed`.

Every Tool declares `Read`, `Write`, `Destructive`, or `Execute`. The current
registry includes Workspace/source tools, scoped memory, SearXNG/arXiv search,
bounded artifact writes, `ask_user`, and user-confirmed Python execution.
`create_task`, `write_memory`, `save_artifact`, and `code_execution` are
prepared and validated before any write or execution; all four require native
confirmation. The default `code_execution` backend launches Python locally
after the user allows it and applies temporary-directory, timeout, stdin, and
output bounds. It is not a security sandbox; Docker remains an optional
stronger backend.
`ask_user` pauses the run and resumes through a typed input submission. Denial
returns a structured `user_denied` Tool result so the model can finish without
a write. Modes change workflow guidance and loop budgets, not tool visibility.

The production `CloudModelClient` sends one bounded request to
`https://spapi.chenkai.space/v1/chat` per Agent iteration. The current gateway
does not expose native tool calling or multi-message history, so Rust
serializes the conversation, mode/stage context, and Tool schemas into a
strict JSON response protocol. Cloud AI may request a Tool by name, but Rust
remains the authority that validates, confirms, executes, and records it. The
adapter also normalizes common OpenAI-style and XML tool-call envelopes before
execution; invalid structured output is treated as final text and never
executed.

BYOK uses `OpenAICompatibleModelClient` and sends the same Agent context to the
configured `<base-url>/chat/completions` endpoint with OpenAI-style `messages`,
`tools`, `stream`, and Bearer authentication. It accepts both JSON and SSE
responses, including content envelopes and native OpenAI `tool_calls`. Cloud
login and BYOK are mutually exclusive active backends; the host-side Tool
validation, permission confirmation, execution, and event recording remain
unchanged.

The deterministic `MockModelClient`, used only by Rust tests, calls:

1. `list_workspace_files`
2. `get_tasks`
3. `create_task`
4. a final response in text deltas

Time-sensitive defaults are calculated by Tools from the Agent's injectable
clock, not by the mock model.

The optional `studypulse-runner` is a separate Rust HTTP process intended to
run inside a hardened container. It accepts only Python or JavaScript source,
requires a bearer token, and applies request, timeout, and output bounds. The
desktop Core owns a `RunnerManager` inside the ToolRegistry. Set
`STUDYPULSE_CODE_EXECUTION_BACKEND=docker` to use it; the Core then
health-checks an explicitly configured external Runner or starts a local Docker
image. The default local backend intentionally launches host Python only after
native user confirmation and does not claim container isolation.

## FFI contract

`StudyPulseCore` is an instantiated object, never a global singleton. Public
records and enums contain only strings, integers, booleans, arrays, and simple
records/enums. UUIDs, dates, and paths cross the boundary as validated strings.
Rust paths, Tokio handles, callbacks, traits, Foundation values, and platform
handles do not.

Agent events use cursor-based pull:

```text
waitForAgentEvents(runId, afterSequence, timeoutMs) -> [AgentEventDto]
waitForOperationEvents(operationId, afterSequence, timeoutMs) -> [OperationEventDto]
```

Re-sending the last consumed sequence resumes without duplicating later
events. Swift wraps the blocking read in `AsyncThrowingStream`; C# can wrap the
same operation in `IAsyncEnumerable` or an observable collection.

## Backup restore

The Core inspects iOS `.studypulsebackup` schema 3 and 4 ZIP archives before
mutation. Inspection validates the format, required files, SHA-256 checksums,
record counts, UUID uniqueness, task-to-phase relationships, dangerous paths,
link entries, per-file size, total expanded size, and JSON/JSONL structure.

Apply supports Replace and Merge. Merge accepts per-record, singleton, and
media conflict decisions. Before apply, the Core takes an exclusive Workspace
write lock and creates `.studypulse/recovery/BeforeRestore-<operation-id>`.
New data is assembled in a transaction directory and swapped only after it is
complete. Swap failures restore the previous Data and Media directories.
Successful restores keep the recovery point. The FFI rejects Agent start while
restore is active.

## macOS client

The app uses MVVM and protocol injection:

- `WorkspaceViewModel`
- `AgentViewModel`
- `TasksViewModel`
- `LibraryViewModel`
- `BackupImportViewModel`

The three columns are navigation, selected feature, and Agent Timeline. SwiftUI
Views contain presentation only. AppKit is limited to system file panels and
security-scoped access. AuthenticationServices presents the Cloud AI web login,
and Security stores the access/refresh pair as one Keychain item. Rust builds
and parses the login callback, validates the profile, refreshes sessions,
selects the plan-provided model, and owns all Agent network behavior. The UI
uses semantic colors, native controls and materials, scalable text, and honors
Reduced Transparency without introducing business policy into Swift.

## Windows port checklist

1. Select and pin the third-party C# UniFFI generator.
2. Build `studypulse-ffi` as a Windows `.dll` and generate bindings from the
   same UniFFI metadata.
3. Implement file picker and authorization lifetime in the Windows UI.
4. Wrap cursor polling as `IAsyncEnumerable` or equivalent.
5. Map the existing DTOs into Windows view models; do not duplicate Core rules.
6. Add Windows CI for Cargo tests, the C# binding build, and a façade smoke
   test.

The current compatibility claim is architectural. Windows compilation and a
.NET client are intentionally outside this MVP and have not been verified on a
Windows runner.

## Build and verification

```sh
cd core
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/build-macos-core.sh

cd ..
xcodebuild \
  -project "StudyPulse Desktop.xcodeproj" \
  -scheme "StudyPulse Desktop" \
  -configuration Debug \
  -derivedDataPath .derivedData \
  CODE_SIGNING_ALLOWED=NO \
  build
```

`core/scripts/build-macos-xcframework.sh` creates a universal arm64/x86_64
XCFramework under `core/target/`, plus generated Swift source, C header, and
module map for distribution. Keeping binary products outside the synchronized
Swift source group prevents Xcode from treating generated headers as app
sources.
