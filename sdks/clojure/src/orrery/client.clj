(ns orrery.client
  (:require [babashka.http-client :as http]
            [cheshire.core :as json]))

(defn- base-url []
  (or (System/getenv "ORRERY_URL") "http://localhost:8080"))

(defn client
  "Create an Orrery HTTP client config map.
  Options: :base-url (default: $ORRERY_URL or http://localhost:8080)"
  ([] (client {}))
  ([opts]
   {:base-url (or (:base-url opts) (base-url))
    :http     (http/client {:connect-timeout 5000})}))

(defn- url [c path]
  (str (:base-url c) "/v1" path))

(defn- post! [context path body timeout-ms]
  (let [resp (http/post (url context path)
                        {:client  (:http context)
                         :headers {"Content-Type" "application/json"
                                   "X-Client-ID"  "orrery-clojure-sdk"}
                         :body    (json/generate-string body)
                         :timeout timeout-ms
                         :throw   false})]
    (if (< (:status resp) 300)
      (json/parse-string (:body resp) true)
      (throw (ex-info (str "HTTP " (:status resp)) {:body (:body resp)})))))

(defn fetch-and-lock
  "Long-poll fetch-and-lock.
  :subscriptions is a vector of {:topic \"t\" :process-definition-ids [\"def-id\"]}
  :process-definition-ids is optional per subscription; empty means any definition."
  [c {:keys [worker-id subscriptions max-tasks lock-duration-ms request-timeout-ms]
      :or   {max-tasks 1 lock-duration-ms 30000 request-timeout-ms 20000}}]
  (post! c "/external-tasks/fetch-and-lock"
         {:worker_id          worker-id
          :subscriptions      (mapv (fn [{:keys [topic process-definition-ids]}]
                                      {:topic                  topic
                                       :process_definition_ids (or process-definition-ids [])})
                                    subscriptions)
          :max_tasks          max-tasks
          :lock_duration_ms   lock-duration-ms
          :request_timeout_ms request-timeout-ms}
         (+ request-timeout-ms 5000)))

(defn complete-task
  "Complete an external task with output variables."
  [c task-id worker-id variables]
  (post! c (str "/external-tasks/" task-id "/complete")
         {:worker_id worker-id :variables (or variables {})}
         10000))

(defn fail-task
  "Report failure on an external task."
  [c task-id worker-id error-message & {:keys [retries retry-timeout-ms]
                                        :or   {retries 0 retry-timeout-ms 0}}]
  (post! c (str "/external-tasks/" task-id "/failure")
         {:worker_id        worker-id
          :error_message    error-message
          :retries          retries
          :retry_timeout_ms retry-timeout-ms}
         10000))

(defn extend-lock
  "Extend the lock on an external task (heartbeat)."
  [c task-id worker-id new-duration-ms]
  (post! c (str "/external-tasks/" task-id "/extend-lock")
         {:worker_id worker-id :new_duration_ms new-duration-ms}
         10000))
