import { createClient, ExternalTask, Client, TopicSubscription } from "./client.js";

export type TaskHandler = (task: ExternalTask) => Promise<Record<string, unknown>>;

export interface Subscription {
  topic: string;
  processDefinitionIds?: string[];
}

export interface WorkerConfig {
  baseUrl?: string;
  workerId?: string;
  lockDurationMs?: number;
  requestTimeoutMs?: number;
  concurrency?: number;
}

export type Worker = {
  readonly config: Required<WorkerConfig>;
  readonly handlers: ReadonlyMap<string, { handler: TaskHandler; processDefinitionIds: string[] }>;
};

export function createWorker(options: WorkerConfig = {}): Worker {
  return {
    config: {
      baseUrl: options.baseUrl ?? process.env["ORRERY_URL"] ?? "http://localhost:8080",
      workerId: options.workerId ?? `worker-${Date.now()}`,
      lockDurationMs: options.lockDurationMs ?? 30_000,
      requestTimeoutMs: options.requestTimeoutMs ?? 20_000,
      concurrency: options.concurrency ?? 4,
    },
    handlers: new Map(),
  };
}

export function subscribe(worker: Worker, subscription: Subscription, handler: TaskHandler): Worker {
  const handlers = new Map(worker.handlers);
  handlers.set(subscription.topic, {
    handler,
    processDefinitionIds: subscription.processDefinitionIds ?? [],
  });
  return { ...worker, handlers };
}

export async function runWorker(worker: Worker): Promise<void> {
  const { config, handlers } = worker;
  const client = createClient({ baseUrl: config.baseUrl });
  const active = new Set<Promise<void>>();
  let running = true;

  const stop = new Promise<void>((resolve) => {
    process.once("SIGINT", resolve);
    process.once("SIGTERM", resolve);
  });
  stop.then(() => { running = false; });

  const subscriptions: TopicSubscription[] = Array.from(handlers.entries()).map(
    ([topic, { processDefinitionIds }]) => ({ topic, processDefinitionIds })
  );

  while (running) {
    if (active.size >= config.concurrency) {
      await Promise.race(active);
      continue;
    }

    const tasks = await Promise.race([
      stop.then(() => [] as ExternalTask[]),
      client.fetchAndLock({
        workerId: config.workerId,
        subscriptions,
        maxTasks: config.concurrency - active.size,
        lockDurationMs: config.lockDurationMs,
        requestTimeoutMs: config.requestTimeoutMs,
      }).catch(() => [] as ExternalTask[]),
    ]);

    for (const task of tasks) {
      const entry = handlers.get(task.topic);
      if (!entry) continue;
      const p: Promise<void> = processTask(client, config, task, entry.handler).finally(() => {
        active.delete(p);
      });
      active.add(p);
    }
  }

  await Promise.allSettled(active);
}

async function processTask(
  client: Client,
  config: Required<WorkerConfig>,
  task: ExternalTask,
  handler: TaskHandler
): Promise<void> {
  const heartbeat = setInterval(
    () => client.extendLock(task.id, config.workerId, config.lockDurationMs).catch(() => {}),
    config.lockDurationMs / 2
  );
  try {
    const variables = await handler(task);
    clearInterval(heartbeat);
    await client.completeTask(task.id, config.workerId, variables);
  } catch (err) {
    clearInterval(heartbeat);
    await client
      .failTask(task.id, config.workerId, err instanceof Error ? err.message : String(err))
      .catch(() => {});
  }
}
