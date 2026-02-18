(ns build
  (:require [clojure.tools.build.api :as b]))

(def lib 'io.orrery/orrery-sdk)
(def major 0)
(def minor 1)
(defn version
  ([] (format "%d.%d.%s" major minor (b/git-count-revs nil)))
  ([_] (version)))
(def class-dir "target/classes")
(defn jar-file [] (format "target/%s-%s.jar" (name lib) (version)))

(defn clean [_]
  (b/delete {:path "target"}))

(defn jar [_]
  (b/write-pom {:class-dir class-dir
                :lib lib
                :version (version)
                :basis (b/create-basis {:project "deps.edn"})
                :src-dirs ["src"]})
  (b/copy-dir {:src-dirs ["src"]
               :target-dir class-dir})
  (b/jar {:class-dir class-dir
          :jar-file (jar-file)}))

(defn install [_]
  (jar nil)
  (b/install {:basis (b/create-basis {:project "deps.edn"})
              :lib lib
              :version (version)
              :jar-file (jar-file)
              :class-dir class-dir}))
