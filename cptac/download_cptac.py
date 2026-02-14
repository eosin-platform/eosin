#!/usr/bin/env python3
"""
Download first N WSIs from a TCIA PathDB collection (e.g., CPTAC-*),
write <slide>.svs.json after each slide download, upload both files to S3,
then delete local copies (garbage collect).

Env (required):
  S3_BUCKET
  S3_PATH_PREFIX (optional, default "")
  S3_ENDPOINT (optional; e.g. https://nyc3.digitaloceanspaces.com)
  S3_REGION (optional; e.g. nyc3)
  AWS_ACCESS_KEY_ID
  AWS_SECRET_ACCESS_KEY

Notes:
- Uses boto3 S3 client; works with DigitalOcean Spaces.
- Upload is performed as soon as each .svs + .json pair exists.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple

import boto3
from botocore.config import Config
from botocore.exceptions import BotoCoreError, ClientError

from tcia_utils import pathdb  # pip install --upgrade tcia_utils


def env_required(name: str) -> str:
    v = os.getenv(name)
    if not v:
        raise SystemExit(f"Missing required env var: {name}")
    return v


def env_default(name: str, default: str) -> str:
    v = os.getenv(name)
    return v if v is not None else default


def safe_filename(name: str) -> str:
    keep = " ._-()[]{}"
    return "".join(c for c in name if c.isalnum() or c in keep).strip().replace(" ", "_")


def pick_metadata_fields(m: Dict[str, Any]) -> Dict[str, Any]:
    preferred_keys = [
        "collectionName",
        "collectionId",
        "subjectId",
        "caseId",
        "patientId",
        "imageId",
        "imageName",
        "imageUrl",
        "fileName",
        "fileSize",
        "stain",
        "stainType",
        "staining",
        "scanner",
        "scannerModel",
        "manufacturer",
        "tissue",
        "tissueType",
        "specimen",
        "specimenType",
        "anatomicSite",
        "organ",
        "diagnosis",
        "tumorType",
        "label",
        "grade",
    ]
    short: Dict[str, Any] = {}
    for k in preferred_keys:
        if k in m and m[k] not in (None, "", []):
            short[k] = m[k]

    # common variants
    for k in ("collection", "Collection", "COLLECTION"):
        if k in m and "collectionName" not in short:
            short["collectionName"] = m[k]
            break
    for k in ("subject", "SubjectID", "SubjectId"):
        if k in m and "subjectId" not in short:
            short["subjectId"] = m[k]
            break
    for k in ("id", "ImageID", "ImageId"):
        if k in m and "imageId" not in short:
            short["imageId"] = m[k]
            break

    return short


def iter_records(images_data: Any) -> Iterable[Dict[str, Any]]:
    if images_data is None:
        return []
    if isinstance(images_data, list):
        return (x for x in images_data if isinstance(x, dict))
    if hasattr(images_data, "to_dict"):
        rows = images_data.to_dict(orient="records")
        return (x for x in rows if isinstance(x, dict))
    raise TypeError(f"Unsupported images_data type: {type(images_data)!r}")


def infer_downloaded_filename(m: Dict[str, Any], outdir: Path, collection: str) -> str:
    """
    Try to infer the saved slide filename from PathDB metadata.
    Falls back to a stable synthetic name.
    """
    image_url = m.get("imageUrl") or m.get("url") or ""
    if image_url:
        base = os.path.basename(image_url)
        if base:
            return base

    image_id = str(m.get("imageId") or m.get("ImageID") or m.get("id") or "unknown")
    return safe_filename(f"{collection}__{image_id}.svs")


def wait_for_file(path: Path, timeout_s: float = 600.0, stable_window_s: float = 2.0) -> None:
    """
    Wait until file exists and size is stable for stable_window_s.
    Useful if downloader returns before flush completes (rare, but safe).
    """
    start = time.time()
    last_size = -1
    last_change = time.time()

    while True:
        if path.exists():
            size = path.stat().st_size
            if size != last_size:
                last_size = size
                last_change = time.time()
            else:
                if time.time() - last_change >= stable_window_s and size > 0:
                    return

        if time.time() - start > timeout_s:
            raise TimeoutError(f"Timed out waiting for file to stabilize: {path}")

        time.sleep(0.25)


def make_s3_client() -> Any:
    # DO Spaces wants signature v4; some clients also prefer path-style.
    endpoint = os.getenv("S3_ENDPOINT")
    region = os.getenv("S3_REGION") or "us-east-1"

    cfg = Config(
        signature_version="s3v4",
        s3={"addressing_style": "path"},
        retries={"max_attempts": 10, "mode": "standard"},
    )

    return boto3.client(
        "s3",
        region_name=region,
        endpoint_url=endpoint,
        aws_access_key_id=env_required("AWS_ACCESS_KEY_ID"),
        aws_secret_access_key=env_required("AWS_SECRET_ACCESS_KEY"),
        config=cfg,
    )


def s3_key_for(prefix: str, collection: str, filename: str) -> str:
    # Keep it deterministic and tidy:
    # <prefix>/<collection>/<filename>
    p = prefix.lstrip("/")
    if p and not p.endswith("/"):
        p += "/"
    return f"{p}{collection}/{filename}"


def upload_file_with_retries(
    s3: Any,
    bucket: str,
    key: str,
    local_path: Path,
    content_type: Optional[str],
    max_tries: int = 5,
) -> None:
    extra_args = {}
    if content_type:
        extra_args["ContentType"] = content_type

    last_err: Optional[BaseException] = None
    for attempt in range(1, max_tries + 1):
        try:
            s3.upload_file(str(local_path), bucket, key, ExtraArgs=extra_args or None)
            return
        except (BotoCoreError, ClientError) as e:
            last_err = e
            sleep_s = min(2 ** attempt, 20)
            print(f"[warn] upload failed (attempt {attempt}/{max_tries}) {local_path.name} -> s3://{bucket}/{key}: {e}", file=sys.stderr)
            time.sleep(sleep_s)

    raise RuntimeError(f"Upload failed after {max_tries} tries: {local_path} -> s3://{bucket}/{key}") from last_err


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--outdir", required=True, help="Working dir for temp downloads (files are deleted after upload)")
    ap.add_argument("--collection", required=True, help='PathDB collection name (e.g., "CPTAC-CCRCC")')
    ap.add_argument("--n", type=int, default=10, help="Number of slides to download (default: 10)")
    ap.add_argument("--max-workers", type=int, default=4, help="Concurrent download workers (per-file) (default: 4)")
    ap.add_argument("--write-full-metadata", action="store_true", help="Include full raw PathDB metadata in sidecar JSON")
    ap.add_argument("--keep-local", action="store_true", help="Do NOT delete local files after upload")
    args = ap.parse_args()

    bucket = env_required("S3_BUCKET")
    prefix = env_default("S3_PATH_PREFIX", "")
    s3 = make_s3_client()

    outdir = Path(args.outdir).expanduser().resolve()
    outdir.mkdir(parents=True, exist_ok=True)

    # Fetch all image metadata once; then process first N sequentially.
    images = pathdb.getImages(query=args.collection)  # list[dict] or df
    records = list(iter_records(images))
    if not records:
        raise SystemExit(f"No images returned for collection={args.collection!r}")

    n = max(0, min(args.n, len(records)))
    print(f"[info] collection={args.collection} total={len(records)} downloading n={n}")

    for idx in range(n):
        m = records[idx]
        filename = infer_downloaded_filename(m, outdir, args.collection)
        slide_path = outdir / filename

        # 1) Download exactly one slide
        print(f"[info] ({idx+1}/{n}) downloading {filename}")
        pathdb.downloadImages([m], path=str(outdir), max_workers=args.max_workers, number=1)

        # Ensure file exists and is stable
        wait_for_file(slide_path)

        # 2) Write sidecar JSON immediately
        sidecar = {
            "source": {"provider": "TCIA PathDB", "collection": args.collection},
            "short_metadata": pick_metadata_fields(m),
        }
        if args.write_full_metadata:
            sidecar["raw_pathdb_metadata"] = m

        sidecar_path = slide_path.with_suffix(slide_path.suffix + ".json")
        sidecar_path.write_text(json.dumps(sidecar, indent=2, sort_keys=True), encoding="utf-8")

        # 3) Upload both to S3
        slide_key = s3_key_for(prefix, args.collection, slide_path.name)
        json_key = s3_key_for(prefix, args.collection, sidecar_path.name)

        print(f"[info] uploading {slide_path.name} -> s3://{bucket}/{slide_key}")
        upload_file_with_retries(s3, bucket, slide_key, slide_path, content_type="application/octet-stream")

        print(f"[info] uploading {sidecar_path.name} -> s3://{bucket}/{json_key}")
        upload_file_with_retries(s3, bucket, json_key, sidecar_path, content_type="application/json")

        # 4) Garbage collect
        if not args.keep_local:
            try:
                slide_path.unlink(missing_ok=True)
            except Exception as e:
                print(f"[warn] failed to delete {slide_path}: {e}", file=sys.stderr)
            try:
                sidecar_path.unlink(missing_ok=True)
            except Exception as e:
                print(f"[warn] failed to delete {sidecar_path}: {e}", file=sys.stderr)

        print(f"[info] ({idx+1}/{n}) done")

    print("[info] all done")


if __name__ == "__main__":
    main()
