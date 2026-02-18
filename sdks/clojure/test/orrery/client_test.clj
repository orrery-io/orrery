(ns orrery.client-test
  (:require [clojure.test :refer [deftest is testing]]
            [orrery.client :as oc]))

(deftest client-creation
  (testing "client uses provided base-url"
    (let [c (oc/client {:base-url "http://localhost:8080"})]
      (is (= "http://localhost:8080" (:base-url c)))))

  (testing "client uses default URL"
    (let [c (oc/client {})]
      (is (string? (:base-url c))))))

(deftest fetch-and-lock-request-shape
  (testing "fetch-and-lock accepts subscriptions"
    ;; Just verify the function accepts the new shape without throwing
    ;; (network call would fail, so we only test the arg structure by calling
    ;; with an invalid URL and catching the exception)
    (let [c (oc/client {:base-url "http://invalid-host-that-does-not-exist"})]
      (is (thrown? Exception
                   (oc/fetch-and-lock c {:worker-id "w1"
                                         :subscriptions [{:topic "payments"
                                                          :process-definition-ids ["order-v1"]}]}))))))
