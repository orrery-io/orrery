# @orrery-io/sdk

TypeScript SDK for [Orrery](https://github.com/orrery-io/orrery) — implement external task workers that integrate with your BPMN processes.

## Installation

```bash
npm install @orrery-io/sdk
```

## Usage

### Worker (recommended)

The worker handles polling, locking, heartbeats, retries, and graceful shutdown automatically.

```typescript
import { createWorker, subscribe, runWorker } from "@orrery-io/sdk";

const worker = subscribe(
  createWorker({ baseUrl: "http://localhost:3000" }),
  { topic: "send-email" },
  async (task) => {
    await sendEmail(task.variables.to, task.variables.subject);
    return {}; // returned variables are merged into the process instance
  }
);

await runWorker(worker);
```

`runWorker` blocks until `SIGINT` or `SIGTERM`, then drains in-flight tasks before exiting.

### Multiple topics

```typescript
let worker = createWorker({
  baseUrl: "http://localhost:3000",
  workerId: "my-worker",
  concurrency: 8,
  lockDurationMs: 60_000,
});

worker = subscribe(worker, { topic: "send-email" }, handleEmail);
worker = subscribe(worker, { topic: "generate-pdf" }, handlePdf);

await runWorker(worker);
```

### Low-level client

Use `createClient` directly if you need manual control over polling.

```typescript
import { createClient } from "@orrery-io/sdk";

const client = createClient({ baseUrl: "http://localhost:3000" });

const tasks = await client.fetchAndLock({
  workerId: "my-worker",
  subscriptions: [{ topic: "send-email" }],
  maxTasks: 5,
  lockDurationMs: 30_000,
});

for (const task of tasks) {
  try {
    await doWork(task);
    await client.completeTask(task.id, "my-worker", { result: "ok" });
  } catch (err) {
    await client.failTask(task.id, "my-worker", err.message);
  }
}
```

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `baseUrl` | `$ORRERY_URL` or `http://localhost:3000` | Orrery server URL |
| `workerId` | `worker-<timestamp>` | Unique worker identifier |
| `concurrency` | `4` | Max tasks processed in parallel |
| `lockDurationMs` | `30000` | How long to hold a task lock |
| `requestTimeoutMs` | `20000` | Long-poll timeout for fetch-and-lock |
