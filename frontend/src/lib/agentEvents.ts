import type { AgentEvent } from "../types";

function parsePayload(payloadJson: string | null): unknown {
  if (!payloadJson) return null;
  try {
    return JSON.parse(payloadJson);
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function codeFromRequest(event: AgentEvent): string | null {
  if (event.tool_name !== "code_execution") return null;
  const payload = parsePayload(event.payload_json);
  return isRecord(payload) && typeof payload.code === "string" ? payload.code : null;
}

function wasDenied(event: AgentEvent): boolean {
  const payload = parsePayload(event.payload_json);
  if (!isRecord(payload) || !isRecord(payload.error)) return false;
  return payload.error.code === "user_denied";
}

export function pythonCodeForConfirmation(event: AgentEvent | undefined): string | null {
  if (!event || event.kind !== "ConfirmationRequired") return null;
  return codeFromRequest(event);
}

export function pythonCodeForCompletedEvent(event: AgentEvent, events: AgentEvent[]): string | null {
  if (
    event.kind !== "ToolCompleted"
    || event.tool_name !== "code_execution"
    || !event.tool_call_id
    || wasDenied(event)
  ) {
    return null;
  }

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
