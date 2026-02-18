import { describe, it, expect, vi, beforeEach } from "vitest";
import { createClient } from "./client.js";

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("createClient", () => {
  it("uses ORRERY_URL env var as fallback", () => {
    process.env["ORRERY_URL"] = "http://custom:9000";
    const client = createClient();
    expect(client).toBeDefined();
    expect(typeof client.fetchAndLock).toBe("function");
    delete process.env["ORRERY_URL"];
  });

  it("returns a client with all expected functions", () => {
    const client = createClient({ baseUrl: "http://localhost:8080" });
    expect(typeof client.fetchAndLock).toBe("function");
    expect(typeof client.completeTask).toBe("function");
    expect(typeof client.failTask).toBe("function");
    expect(typeof client.extendLock).toBe("function");
  });

  it("fetchAndLock sends correct JSON body", async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => [],
    } as Response);
    vi.stubGlobal("fetch", mockFetch);

    const client = createClient({ baseUrl: "http://localhost:8080" });
    const result = await client.fetchAndLock({
      workerId: "w1",
      subscriptions: [{ topic: "payments", processDefinitionIds: ["order-v1"] }],
      maxTasks: 2,
      lockDurationMs: 10_000,
      requestTimeoutMs: 5_000,
    });

    expect(result).toEqual([]);
    expect(mockFetch).toHaveBeenCalledOnce();
    const [calledUrl, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(calledUrl).toBe("http://localhost:8080/v1/external-tasks/fetch-and-lock");
    const body = JSON.parse(init.body as string);
    expect(body.worker_id).toBe("w1");
    expect(body.subscriptions).toEqual([
      { topic: "payments", process_definition_ids: ["order-v1"] },
    ]);
    expect(body.max_tasks).toBe(2);
  });

  it("completeTask sends correct endpoint and body", async () => {
    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ id: "task-1" }),
    } as Response);
    vi.stubGlobal("fetch", mockFetch);

    const client = createClient({ baseUrl: "http://localhost:8080" });
    await client.completeTask("task-1", "w1", { receipt: "abc" });

    const [calledUrl, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(calledUrl).toBe("http://localhost:8080/v1/external-tasks/task-1/complete");
    const body = JSON.parse(init.body as string);
    expect(body.worker_id).toBe("w1");
    expect(body.variables).toEqual({ receipt: "abc" });
  });

  it("throws on non-ok response", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: false,
      status: 409,
      text: async () => "lock expired",
    } as Response));

    const client = createClient({ baseUrl: "http://localhost:8080" });
    await expect(client.completeTask("x", "w1")).rejects.toThrow("HTTP 409: lock expired");
  });
});
