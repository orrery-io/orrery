export interface ClientConfig {
  baseUrl?: string;
}

export interface TopicSubscription {
  topic: string;
  processDefinitionIds?: string[];
}

export interface FetchAndLockRequest {
  workerId: string;
  subscriptions: TopicSubscription[];
  maxTasks?: number;
  lockDurationMs?: number;
  requestTimeoutMs?: number;
}

export interface ExternalTask {
  id: string;
  topic: string;
  processInstanceId: string;
  processDefinitionId: string;
  elementId: string;
  variables: Record<string, unknown>;
  workerId: string;
  lockedUntil: string;
  retryCount: number;
  maxRetries: number;
  createdAt: string;
}

export type Client = {
  fetchAndLock: (req: FetchAndLockRequest) => Promise<ExternalTask[]>;
  completeTask: (
    id: string,
    workerId: string,
    variables?: Record<string, unknown>,
  ) => Promise<unknown>;
  failTask: (
    id: string,
    workerId: string,
    errorMessage: string,
    retries?: number,
    retryTimeoutMs?: number,
  ) => Promise<unknown>;
  extendLock: (
    id: string,
    workerId: string,
    newDurationMs: number,
  ) => Promise<unknown>;
};

export function createClient(config: ClientConfig = {}): Client {
  const baseUrl = (
    config.baseUrl ??
    process.env["ORRERY_URL"] ??
    "http://localhost:3000"
  ).replace(/\/$/, "");

  const url = (path: string) => `${baseUrl}/v1${path}`;

  const checkResponse = async <T>(resp: Response): Promise<T> => {
    if (!resp.ok) {
      const body = await resp.text().catch(() => "");
      throw new Error(`HTTP ${resp.status}: ${body}`);
    }
    return resp.json() as Promise<T>;
  };

  const fetchAndLock = async (
    req: FetchAndLockRequest,
  ): Promise<ExternalTask[]> => {
    const body = {
      worker_id: req.workerId,
      subscriptions: req.subscriptions.map((s) => ({
        topic: s.topic,
        process_definition_ids: s.processDefinitionIds ?? [],
      })),
      max_tasks: req.maxTasks ?? 1,
      lock_duration_ms: req.lockDurationMs ?? 30_000,
      request_timeout_ms: req.requestTimeoutMs ?? 20_000,
    };
    const timeout = (req.requestTimeoutMs ?? 20_000) + 5_000;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeout);
    try {
      const resp = await fetch(url("/external-tasks/fetch-and-lock"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: controller.signal,
      });
      return checkResponse<ExternalTask[]>(resp);
    } finally {
      clearTimeout(timer);
    }
  };

  const completeTask = async (
    id: string,
    workerId: string,
    variables: Record<string, unknown> = {},
  ): Promise<unknown> => {
    const resp = await fetch(url(`/external-tasks/${id}/complete`), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ worker_id: workerId, variables }),
    });
    return checkResponse(resp);
  };

  const failTask = async (
    id: string,
    workerId: string,
    errorMessage: string,
    retries = 0,
    retryTimeoutMs = 0,
  ): Promise<unknown> => {
    const resp = await fetch(url(`/external-tasks/${id}/failure`), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        worker_id: workerId,
        error_message: errorMessage,
        retries,
        retry_timeout_ms: retryTimeoutMs,
      }),
    });
    return checkResponse(resp);
  };

  const extendLock = async (
    id: string,
    workerId: string,
    newDurationMs: number,
  ): Promise<unknown> => {
    const resp = await fetch(url(`/external-tasks/${id}/extend-lock`), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        worker_id: workerId,
        new_duration_ms: newDurationMs,
      }),
    });
    return checkResponse(resp);
  };

  return { fetchAndLock, completeTask, failTask, extendLock };
}
