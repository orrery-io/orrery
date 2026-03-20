(ns build
  (:require [clojure.tools.build.api :as b]
            [deps-deploy.deps-deploy :as dd]))

(def lib 'net.clojars.myxomatozis/orrery-sdk)
(def class-dir "target/classes")

(defn- ver [opts]
  (or (:version opts)
      (format "0.1.%s" (b/git-count-revs nil))))

(defn- jar-file [opts]
  (format "target/%s-%s.jar" (name lib) (ver opts)))

(defn clean [_]
  (b/delete {:path "target"}))

(defn jar [opts]
  (b/write-pom {:class-dir class-dir
                :lib lib
                :version (ver opts)
                :basis (b/create-basis {:project "deps.edn"})
                :src-dirs ["src"]
                :pom-data [[:licenses
                            [:license
                             [:name "MIT License"]
                             [:url "https://opensource.org/licenses/MIT"]]]]})
  (b/copy-dir {:src-dirs ["src"]
               :target-dir class-dir})
  (b/jar {:class-dir class-dir
          :jar-file (jar-file opts)}))

(defn install [opts]
  (jar opts)
  (b/install {:basis (b/create-basis {:project "deps.edn"})
              :lib lib
              :version (ver opts)
              :jar-file (jar-file opts)
              :class-dir class-dir}))

(defn deploy [opts]
  (jar opts)
  (dd/deploy {:installer :remote
              :artifact  (jar-file opts)
              :pom-file  (b/pom-path {:lib lib :class-dir class-dir})}))
