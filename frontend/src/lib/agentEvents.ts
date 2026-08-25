import type { AgentEvent } from "../types";

// Payloads are serialized by Rust so event records can be persisted as JSONL.
// UI helpers must tolerate absent or malformed payloads without breaking the
// rest of the timeline.
function parsePayload(payloadJson: string | null): unknown {
  if (!payloadJson) return null;
  try {
    return JSON.parse(payloadJson);
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  // This narrow guard lets callers read dynamic JSON fields without treating
  // arrays as request objects.
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function codeFromRequest(event: AgentEvent): string | null {
  // Only the code_execution tool exposes source code for the timeline; other
  // tool payloads must never be rendered as executable-looking content.
  if (event.tool_name !== "code_execution") return null;
  const payload = parsePayload(event.payload_json);
  return isRecord(payload) && typeof payload.code === "string" ? payload.code : null;
}

function wasDenied(event: AgentEvent): boolean {
  // A denied tool can still emit ToolCompleted, but its payload must not be
  // presented as successfully executed code.
  const payload = parsePayload(event.payload_json);
  if (!isRecord(payload) || !isRecord(payload.error)) return false;
  return payload.error.code === "user_denied";
}

export function pythonCodeForConfirmation(event: AgentEvent | undefined): string | null {
  // Confirmation events carry the original request payload, so they can show
  // the exact code before the user decides whether to allow execution.
  if (!event || event.kind !== "ConfirmationRequired") return null;
  return codeFromRequest(event);
}

export function pythonCodeForCompletedEvent(event: AgentEvent, events: AgentEvent[]): string | null {
  // Completion payloads contain results, not necessarily the original source;
  // match the earlier request by tool_call_id and monotonic sequence.
  if (
    event.kind !== "ToolCompleted"
    || event.tool_name !== "code_execution"
    || !event.tool_call_id
    || wasDenied(event)
  ) {
    return null;
  }

  // Reverse search prefers the nearest matching request when a run has made
  // several calls to the same code tool.
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const candidate = events[index];
    if (
      candidate.sequence < event.sequence
      && candidate.tool_call_id === event.tool_call_id
      && (candidate.kind === "ToolRequested" || candidate.kind === "ConfirmationRequired")
    ) {
      const code = codeFromRequest(candidate);
      if (code !== null) return code;
    }
  }
  return null;
}
