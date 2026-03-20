# orrery-sdk

Clojure SDK for [Orrery](https://github.com/orrery-io/orrery) — implement external task workers that integrate with your BPMN processes.

## Dependency

```clojure
;; deps.edn
net.clojars.myxomatozis/orrery-sdk {:mvn/version "0.1.0"}
```

## Usage

### Worker (recommended)

The worker handles polling, locking, heartbeats, retries, and graceful shutdown automatically.

```clojure
(require '[orrery.worker :as ow])

(-> (ow/worker {:base-url "http://localhost:3000"})
    (ow/subscribe {:topic "send-email"}
                  (fn [task]
                    (send-email (get-in task [:variables :to])
                                (get-in task [:variables :subject]))
                    {})) ; returned map is merged into process instance variables
    (ow/run))
```

`run` blocks until the JVM receives `SIGINT` or `SIGTERM`, then drains in-flight tasks before exiting.

### Multiple topics

```clojure
(-> (ow/worker {:base-url   "http://localhost:3000"
                :worker-id  "my-worker"
                :concurrency 8
                :lock-duration-ms 60000})
    (ow/subscribe {:topic "send-email"} handle-email)
    (ow/subscribe {:topic "generate-pdf"} handle-pdf)
    (ow/run))
```

### Scoped to specific process definitions

```clojure
(ow/subscribe worker
              {:topic "send-email"
               :process-definition-ids ["order-process" "signup-flow"]}
              handle-email)
```

### Low-level client

Use `orrery.client` directly if you need manual control over polling.

```clojure
(require '[orrery.client :as oc])

(let [c     (oc/client {:base-url "http://localhost:3000"})
      tasks (oc/fetch-and-lock c {:worker-id     "my-worker"
                                  :subscriptions [{:topic "send-email"}]
                                  :max-tasks      5
                                  :lock-duration-ms 30000})]
  (doseq [task tasks]
    (try
      (do-work task)
      (oc/complete-task c (:id task) "my-worker" {:result "ok"})
      (catch Exception e
        (oc/fail-task c (:id task) "my-worker" (.getMessage e))))))
```

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `:base-url` | `$ORRERY_URL` or `http://localhost:8080` | Orrery server URL |
| `:worker-id` | `worker-<timestamp>` | Unique worker identifier |
| `:concurrency` | `4` | Max tasks processed in parallel |
| `:lock-duration-ms` | `30000` | How long to hold a task lock |
| `:request-timeout-ms` | `20000` | Long-poll timeout for fetch-and-lock |

## Running tests

```bash
clojure -M:test
```
