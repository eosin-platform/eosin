variable "REGISTRY" {
  default = ""
}

group "default" {
  targets = [
    "storage",
    "storage-operator",
    "frusta",
    "meta",
    "landing",
    "browser",
    "compiler",
    "iam"
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

target "landing" {
  context    = "./"
  dockerfile = "landing/Dockerfile"
  tags       = ["${REGISTRY}thavlik/eosin-landing:latest"]
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

target "storage-operator" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "storage-operator/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/eosin-storage-operator:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/eosin-storage-operator"]
  cache-to   = ["type=local,dest=.buildx-cache/eosin-storage-operator,mode=min"]
}

target "iam" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "iam/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/eosin-iam:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/eosin-iam"]
  cache-to   = ["type=local,dest=.buildx-cache/eosin-iam,mode=min"]
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