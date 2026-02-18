(ns orrery.worker
  (:require [orrery.client :as oc])
  (:import (java.util.concurrent Executors TimeUnit)))

(defn worker
  "Create a worker config map.
  Options: :base-url, :worker-id, :lock-duration-ms, :request-timeout-ms, :concurrency"
  ([] (worker {}))
  ([opts]
   {:client (oc/client {:base-url (:base-url opts)})
    :worker-id (or (:worker-id opts) (str "worker-" (System/currentTimeMillis)))
    :lock-duration-ms (or (:lock-duration-ms opts) 30000)
    :request-timeout-ms (or (:request-timeout-ms opts) 20000)
    :concurrency (or (:concurrency opts) 4)
    :handlers (atom {})
    :running (atom false)}))

(defn subscribe
  "Register a handler function for a topic subscription.
  subscription is {:topic \"t\" :process-definition-ids [\"def-id\"]} — :process-definition-ids optional.
  handler is a 1-arity fn taking the task map, returning output variables map or throwing."
  [w subscription handler]
  (let [topic (:topic subscription)
        ids (or (:process-definition-ids subscription) [])]
    (swap! (:handlers w) assoc topic {:handler handler :process-definition-ids ids})
    w))

(defn- heartbeat-loop [c task-id worker-id lock-duration-ms stop-atom]
  (future
    (loop []
      (when-not @stop-atom
        (Thread/sleep (/ lock-duration-ms 2))
        (try
          (oc/extend-lock c task-id worker-id lock-duration-ms)
          (catch Exception _))
        (recur)))))

(defn- process-task [w task]
  (let [c (:client w)
        worker-id (:worker-id w)
        topic (:topic task)
        task-id (:id task)
        entry (get @(:handlers w) topic)
        handler (:handler entry)
        stop-hb (atom false)
        hb (heartbeat-loop c task-id worker-id (:lock-duration-ms w) stop-hb)]
    (try
      (let [result (handler task)]
        (reset! stop-hb true)
        (future-cancel hb)
        (oc/complete-task c task-id worker-id (or result {})))
      (catch Exception e
        (reset! stop-hb true)
        (future-cancel hb)
        (try
          (oc/fail-task c task-id worker-id (.getMessage e))
          (catch Exception _))))))

(defn run
  "Run the worker loop synchronously. Blocks until interrupted."
  [w]
  (reset! (:running w) true)
  (let [pool (Executors/newFixedThreadPool (:concurrency w))
        subscriptions (mapv (fn [[topic {:keys [process-definition-ids]}]]
                              {:topic topic
                               :process-definition-ids (or process-definition-ids [])})
                            @(:handlers w))]
    (.addShutdownHook (Runtime/getRuntime)
                      (Thread. #(reset! (:running w) false)))
    (try
      (while @(:running w)
        (try
          (let [tasks (oc/fetch-and-lock (:client w)
                                         {:worker-id (:worker-id w)
                                          :subscriptions subscriptions
                                          :max-tasks (:concurrency w)
                                          :lock-duration-ms (:lock-duration-ms w)
                                          :request-timeout-ms (:request-timeout-ms w)})]
            (doseq [task tasks]
              (.submit pool ^Callable #(process-task w task))))
          (catch Exception _e
            (when @(:running w)
              (Thread/sleep 2000)))))
      (finally
        (.shutdown pool)
        (.awaitTermination pool 30 TimeUnit/SECONDS)))))
