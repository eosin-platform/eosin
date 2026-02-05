variable "REGISTRY" {
  default = ""
}

group "default" {
  targets = [
    "storage",
    "frusta"
  ]
}

target "base" {
  context    = "./"
  dockerfile = "Dockerfile.base"
  tags       = ["${REGISTRY}thavlik/histion-base:latest"]
  push       = false
}

target "browser" {
  context    = "./"
  dockerfile = "browser/Dockerfile"
  tags       = ["${REGISTRY}thavlik/histion-browser:latest"]
  push       = true
}

target "storage" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "storage/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-storage:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-storage"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-storage,mode=min"]
}

target "frusta" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "frusta/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-frusta:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-frusta"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-frusta,mode=min"]
}