import { describe, expect, it } from "vitest";
import type { AgentEvent } from "../types";
import {
  parseAgentInputRequest,
  pythonCodeForCompletedEvent,
  pythonCodeForConfirmation,
} from "./agentEvents";

// The fixture mirrors the complete wire shape so each test can override only
// the event fields relevant to its timeline scenario.
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
  // Confirmation payloads must expose the whole source, not a truncated
  // preview, because this is the code a user is being asked to allow.
  it("extracts the complete Python source from a confirmation request", () => {
    const code = "def answer():\n    return 42\n\nprint(answer())";
    const confirmation = agentEvent({
      kind: "ConfirmationRequired",
      payload_json: JSON.stringify({ language: "python", code }),
    });

    expect(pythonCodeForConfirmation(confirmation)).toBe(code);
  });

  it("matches completed executions to the correct request by tool call id", () => {
    // Sequence order alone is insufficient when multiple tool calls overlap;
    // the tool call id is the stable correlation key.
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
    // Timeline rendering is defensive: malformed JSON and unrelated tools
    // should produce no code block rather than a test/runtime failure.
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
    // `user_denied` is a structured Core result and must remain visibly
    // distinct from successful execution.
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

describe("Agent input events", () => {
  it("parses ask_user prompt and options instead of exposing raw JSON", () => {
    const event = agentEvent({
      kind: "InputRequired",
      tool_name: "ask_user",
      preview: JSON.stringify({
        options: ["数学与微积分", "概率与统计", "数学与微积分"],
        prompt: "请选择错题所属主题。",
      }),
      payload_json: null,
      confirmation_id: "input-1",
    });

    expect(parseAgentInputRequest(event)).toEqual({
      prompt: "请选择错题所属主题。",
      options: ["数学与微积分", "概率与统计"],
    });
  });

  it("keeps legacy plain-text previews readable and tolerates malformed payloads", () => {
    const event = agentEvent({
      kind: "InputRequired",
      preview: "请补充题目内容。",
      payload_json: "{",
    });

    expect(parseAgentInputRequest(event)).toEqual({
      prompt: "请补充题目内容。",
      options: [],
    });
  });
});
