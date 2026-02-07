variable "REGISTRY" {
  default = ""
}

group "default" {
  targets = [
    "storage",
    "frusta",
    "meta",
    "browser",
    "compiler"
  ]
}

target "base" {
  context    = "./"
  dockerfile = "Dockerfile.base"
  tags       = ["${REGISTRY}thavlik/eosin-base:latest"]
  push       = false
}

target "browser" {
  context    = "./"
  dockerfile = "browser/Dockerfile"
  tags       = ["${REGISTRY}thavlik/eosin-browser:latest"]
  push       = true
}

target "storage" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "storage/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/eosin-storage:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/eosin-storage"]
  cache-to   = ["type=local,dest=.buildx-cache/eosin-storage,mode=min"]
}

target "frusta" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "frusta/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/eosin-frusta:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/eosin-frusta"]
  cache-to   = ["type=local,dest=.buildx-cache/eosin-frusta,mode=min"]
}

target "meta" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "meta/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/eosin-meta:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/eosin-meta"]
  cache-to   = ["type=local,dest=.buildx-cache/eosin-meta,mode=min"]
}

target "compiler" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "compiler/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/eosin-compiler:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/eosin-compiler"]
  cache-to   = ["type=local,dest=.buildx-cache/eosin-compiler,mode=min"]
}