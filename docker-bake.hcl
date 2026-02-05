variable "REGISTRY" {
  default = ""
}

group "default" {
  targets = [
    "storage"
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

target "analyzer" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "analyzer/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-analyzer:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-analyzer"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-analyzer,mode=min"]
}

target "archiver-worker" {
  context    = "./"
  dockerfile = "archiver/Dockerfile"
  tags       = ["${REGISTRY}thavlik/histion-archiver:latest"]
  push       = true
}

target "auth" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  args       = { BASE_IMAGE = "base_context" }
  dockerfile = "auth/Dockerfile"
  tags       = ["${REGISTRY}thavlik/histion-auth:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-auth"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-auth,mode=min"]
}

target "archiver-init" {
  context    = "./"
  dockerfile = "archiver/Dockerfile.init"
  tags       = ["${REGISTRY}thavlik/histion-archiver-init:latest"]
  push       = true
}

target "master" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  args       = { BASE_IMAGE = "base_context" }
  dockerfile = "master/Dockerfile"
  tags       = ["${REGISTRY}thavlik/histion-master:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-master"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-master,mode=min"]
}

target "operator" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "operator/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-operator:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-operator"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-operator,mode=min"]
}


target "iam" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "iam/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-iam:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-iam"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-iam,mode=min"]
}

target "party" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "party/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-party:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-party"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-party,mode=min"]
}

target "webrtc-auth" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "webrtc-auth/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-webrtc-auth:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-webrtc-auth"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-webrtc-auth,mode=min"]
}

target "sock" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "sock/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-sock:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-sock"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-sock,mode=min"]
}

target "server" {
  context    = "zandronum/"
  dockerfile = "Dockerfile.server"
  tags       = ["${REGISTRY}thavlik/zandronum-server:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-server"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-server,mode=min"]
}

target "spectator" {
  context    = "zandronum/"
  dockerfile = "Dockerfile.spectator"
  tags       = ["${REGISTRY}thavlik/zandronum-spectator:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-spectator"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-spectator,mode=min"]
}

target "downloader" {
  context    = "downloader/"
  tags       = ["${REGISTRY}thavlik/histion-downloader:latest"]
  push       = true
}

target "proxy" {
  contexts   = { base_context = "target:base" }
  context    = "./"
  dockerfile = "proxy/Dockerfile"
  args       = { BASE_IMAGE = "base_context" }
  tags       = ["${REGISTRY}thavlik/histion-proxy:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-proxy"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-proxy,mode=min"]
}

target "client" {
  context    = "zandronum/"
  dockerfile = "Dockerfile.client"
  tags       = ["${REGISTRY}thavlik/zandronum-client:latest"]
  push       = true
  cache-from = ["type=local,src=.buildx-cache/histion-client"]
  cache-to   = ["type=local,dest=.buildx-cache/histion-client,mode=min"]
}
