import { describe, expect, it } from "vitest";
import type { AgentEvent } from "../types";
import { pythonCodeForCompletedEvent, pythonCodeForConfirmation } from "./agentEvents";

function agentEvent(overrides: Partial<AgentEvent>): AgentEvent {
  return {
    run_id: "run-1",
    sequence: 1,
    timestamp: "2026-08-03T00:00:00.000Z",
    kind: "ToolRequested",
    status: null,
    text: null,
    tool_call_id: "call-1",
    tool_name: "code_execution",
    permission: "Execute",
    preview: null,
    confirmation_id: null,
    payload_json: null,
    mode: null,
    stage: null,
    progress: null,
    ...overrides,
  };
}

describe("Agent Python code events", () => {
  it("extracts the complete Python source from a confirmation request", () => {
    const code = "def answer():\n    return 42\n\nprint(answer())";
    const confirmation = agentEvent({
      kind: "ConfirmationRequired",
      payload_json: JSON.stringify({ language: "python", code }),
    });

    expect(pythonCodeForConfirmation(confirmation)).toBe(code);
  });

  it("matches completed executions to the correct request by tool call id", () => {
    const firstRequest = agentEvent({
      sequence: 2,
      tool_call_id: "call-1",
      payload_json: JSON.stringify({ language: "python", code: "print('first')" }),
    });
    const secondRequest = agentEvent({
      sequence: 5,
      tool_call_id: "call-2",
      payload_json: JSON.stringify({ language: "python", code: "print('second')" }),
    });
    const completed = agentEvent({
      sequence: 6,
      kind: "ToolCompleted",
      tool_call_id: "call-2",
      payload_json: JSON.stringify({ ok: true, stdout: "second\n" }),
    });

    expect(pythonCodeForCompletedEvent(completed, [firstRequest, secondRequest, completed])).toBe("print('second')");
  });

  it("ignores malformed payloads and non-Python tools", () => {
    const malformed = agentEvent({ kind: "ConfirmationRequired", payload_json: "{" });
    const otherTool = agentEvent({
      kind: "ConfirmationRequired",
      tool_name: "create_task",
      payload_json: JSON.stringify({ code: "print('hidden')" }),
    });

    expect(pythonCodeForConfirmation(malformed)).toBeNull();
    expect(pythonCodeForConfirmation(otherTool)).toBeNull();
  });

  it("does not show a completion code block when execution was denied", () => {
    const request = agentEvent({
      sequence: 2,
      payload_json: JSON.stringify({ language: "python", code: "print('denied')" }),
    });
    const denied = agentEvent({
      sequence: 3,
      kind: "ToolCompleted",
      payload_json: JSON.stringify({ ok: false, error: { code: "user_denied" } }),
    });

    expect(pythonCodeForCompletedEvent(denied, [request, denied])).toBeNull();
  });
});
