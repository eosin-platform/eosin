#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import boto3
import requests
from tqdm import tqdm

GDC_FILES_ENDPOINT = "https://api.gdc.cancer.gov/files"
GDC_DATA_ENDPOINT = "https://api.gdc.cancer.gov/data"

# TIFF tag IDs
TAG_ImageWidth = 256
TAG_ImageLength = 257
TAG_ImageDescription = 270

# TIFF type sizes (bytes per component)
TIFF_TYPE_SIZES = {
    1: 1,   # BYTE
    2: 1,   # ASCII
    3: 2,   # SHORT
    4: 4,   # LONG
    5: 8,   # RATIONAL
    16: 8,  # LONG8 (BigTIFF)
    17: 8,  # SLONG8 (BigTIFF)
}


@dataclass
class S3UploadConfig:
    bucket: str
    path_prefix: str
    endpoint: Optional[str]
    region: Optional[str]


def _normalize_s3_prefix(prefix: str) -> str:
    cleaned = prefix.strip().strip("/")
    return f"{cleaned}/" if cleaned else ""


def load_s3_upload_config_from_env() -> Optional[S3UploadConfig]:
    bucket = (os.getenv("S3_BUCKET") or "").strip()
    if not bucket:
        return None

    path_prefix = _normalize_s3_prefix(os.getenv("S3_PATH_PREFIX") or "")
    endpoint = (os.getenv("S3_ENDPOINT") or "").strip() or None
    region = (os.getenv("S3_REGION") or "").strip() or None

    return S3UploadConfig(
        bucket=bucket,
        path_prefix=path_prefix,
        endpoint=endpoint,
        region=region,
    )


def build_s3_client(config: S3UploadConfig) -> Any:
    return boto3.client(
        "s3",
        endpoint_url=config.endpoint,
        region_name=config.region,
    )


def s3_object_key(config: S3UploadConfig, rel_path: Path) -> str:
    rel = rel_path.as_posix().lstrip("/")
    return f"{config.path_prefix}{rel}" if config.path_prefix else rel


def upload_file_to_s3(client: Any, config: S3UploadConfig, local_file: Path, rel_path: Path) -> str:
    key = s3_object_key(config, rel_path)
    client.upload_file(
        Filename=str(local_file),
        Bucket=config.bucket,
        Key=key,
    )
    return key


def safe_filename(name: str) -> str:
    name = name.replace("/", "_")
    name = re.sub(r"[^A-Za-z0-9._-]+", "_", name)
    return name


def http_range_get(url: str, start: int, end_inclusive: int, timeout: int = 60) -> bytes:
    # end_inclusive because Range uses inclusive end
    headers = {"Range": f"bytes={start}-{end_inclusive}"}
    r = requests.get(url, headers=headers, timeout=timeout)
    # 206 Partial Content expected; some servers may return 200 with full content (bad but possible)
    if r.status_code not in (200, 206):
        raise RuntimeError(f"Range request failed: HTTP {r.status_code}")
    return r.content


def parse_tiff_first_ifd_dimensions(url: str, timeout: int = 60) -> Tuple[Optional[int], Optional[int], Optional[str], bool]:
    """
    Return (width, height, image_description, used_range_ok)
    - Only reads small byte ranges from the remote file.
    - Supports classic TIFF and BigTIFF.
    """
    # Read first 64 KiB — enough for header + first IFD in most cases.
    head = http_range_get(url, 0, 65535, timeout=timeout)

    if len(head) < 16:
        return None, None, None, False

    endian = head[0:2]
    if endian == b"II":
        bo = "<"
    elif endian == b"MM":
        bo = ">"
    else:
        return None, None, None, False

    # Classic TIFF: magic 42 at bytes 2-4; BigTIFF: magic 43 at bytes 2-4
    magic = struct.unpack(bo + "H", head[2:4])[0]

    is_bigtiff = (magic == 43)
    if not (magic == 42 or magic == 43):
        return None, None, None, False

    if not is_bigtiff:
        # offset to first IFD is uint32 at bytes 4-8
        ifd_offset = struct.unpack(bo + "I", head[4:8])[0]
        entry_count_off = ifd_offset
        if entry_count_off + 2 > len(head):
            # fetch a tiny range around ifd_offset
            chunk = http_range_get(url, ifd_offset, ifd_offset + 4095, timeout=timeout)
            head = head + b""  # keep original; we’ll index into chunk
            base = ifd_offset
            # entry count is at chunk[0:2]
            n = struct.unpack(bo + "H", chunk[0:2])[0]
            entries_bytes = 12 * n
            need = 2 + entries_bytes
            if need > len(chunk):
                chunk = http_range_get(url, ifd_offset, ifd_offset + need + 16, timeout=timeout)
            entries = chunk[2:2 + entries_bytes]
            width, height, desc = _parse_classic_ifd_entries(url, bo, entries, ifd_offset, timeout)
            return width, height, desc, True

        n = struct.unpack(bo + "H", head[entry_count_off:entry_count_off + 2])[0]
        entries_off = entry_count_off + 2
        entries_bytes = 12 * n
        if entries_off + entries_bytes > len(head):
            # fetch just enough
            chunk = http_range_get(url, ifd_offset, ifd_offset + 2 + entries_bytes + 16, timeout=timeout)
            n = struct.unpack(bo + "H", chunk[0:2])[0]
            entries = chunk[2:2 + 12 * n]
            width, height, desc = _parse_classic_ifd_entries(url, bo, entries, ifd_offset, timeout)
            return width, height, desc, True

        entries = head[entries_off:entries_off + entries_bytes]
        width, height, desc = _parse_classic_ifd_entries(url, bo, entries, 0, timeout)
        return width, height, desc, True

    else:
        # BigTIFF header layout:
        # bytes 4-6: offset size (should be 8)
        # bytes 6-8: always 0
        # bytes 8-16: uint64 first IFD offset
        if len(head) < 16:
            return None, None, None, False
        offsize = struct.unpack(bo + "H", head[4:6])[0]
        if offsize != 8:
            return None, None, None, False
        ifd_offset = struct.unpack(bo + "Q", head[8:16])[0]

        # BigTIFF IFD: uint64 entry count, then entries of 20 bytes
        # entry: tag(2), type(2), count(8), value_or_offset(8)
        # Try to ensure we have it
        if ifd_offset + 8 > len(head):
            chunk = http_range_get(url, ifd_offset, ifd_offset + 8191, timeout=timeout)
            n = struct.unpack(bo + "Q", chunk[0:8])[0]
            entries_bytes = 20 * n
            need = 8 + entries_bytes
            if need > len(chunk):
                chunk = http_range_get(url, ifd_offset, ifd_offset + need + 16, timeout=timeout)
            entries = chunk[8:8 + entries_bytes]
            width, height, desc = _parse_bigtiff_ifd_entries(url, bo, entries, ifd_offset, timeout)
            return width, height, desc, True

        n = struct.unpack(bo + "Q", head[ifd_offset:ifd_offset + 8])[0]
        entries_off = ifd_offset + 8
        entries_bytes = 20 * n
        if entries_off + entries_bytes > len(head):
            chunk = http_range_get(url, ifd_offset, ifd_offset + 8 + entries_bytes + 16, timeout=timeout)
            n = struct.unpack(bo + "Q", chunk[0:8])[0]
            entries = chunk[8:8 + 20 * n]
            width, height, desc = _parse_bigtiff_ifd_entries(url, bo, entries, ifd_offset, timeout)
            return width, height, desc, True

        entries = head[entries_off:entries_off + entries_bytes]
        width, height, desc = _parse_bigtiff_ifd_entries(url, bo, entries, 0, timeout)
        return width, height, desc, True


def _read_value_bytes(url: str, offset: int, nbytes: int, timeout: int) -> bytes:
    # Avoid huge range fetches; cap per request
    cap = min(nbytes, 256 * 1024)
    return http_range_get(url, offset, offset + cap - 1, timeout=timeout)


def _parse_classic_ifd_entries(url: str, bo: str, entries: bytes, base_offset: int, timeout: int) -> Tuple[Optional[int], Optional[int], Optional[str]]:
    width = None
    height = None
    desc = None

    for i in range(0, len(entries), 12):
        tag, typ, count, value_or_offset = struct.unpack(bo + "HHII", entries[i:i+12])
        if tag not in (TAG_ImageWidth, TAG_ImageLength, TAG_ImageDescription):
            continue

        tsize = TIFF_TYPE_SIZES.get(typ)
        if not tsize:
            continue
        total = tsize * count

        # If value fits in 4 bytes, it's stored inline.
        if total <= 4:
            raw = struct.pack(bo + "I", value_or_offset)[:total]
        else:
            raw = _read_value_bytes(url, value_or_offset, total, timeout)

        if tag == TAG_ImageWidth:
            width = _decode_scalar(raw, bo, typ)
        elif tag == TAG_ImageLength:
            height = _decode_scalar(raw, bo, typ)
        elif tag == TAG_ImageDescription:
            # ASCII; stop at NUL
            try:
                desc = raw.split(b"\x00", 1)[0].decode("utf-8", errors="replace")
            except Exception:
                desc = None

    return width, height, desc


def _parse_bigtiff_ifd_entries(url: str, bo: str, entries: bytes, base_offset: int, timeout: int) -> Tuple[Optional[int], Optional[int], Optional[str]]:
    width = None
    height = None
    desc = None

    for i in range(0, len(entries), 20):
        tag, typ = struct.unpack(bo + "HH", entries[i:i+4])
        count = struct.unpack(bo + "Q", entries[i+4:i+12])[0]
        value_or_offset = struct.unpack(bo + "Q", entries[i+12:i+20])[0]

        if tag not in (TAG_ImageWidth, TAG_ImageLength, TAG_ImageDescription):
            continue

        tsize = TIFF_TYPE_SIZES.get(typ)
        if not tsize:
            continue
        total = tsize * count

        # BigTIFF inline threshold is 8 bytes
        if total <= 8:
            raw = struct.pack(bo + "Q", value_or_offset)[:total]
        else:
            raw = _read_value_bytes(url, int(value_or_offset), int(total), timeout)

        if tag == TAG_ImageWidth:
            width = _decode_scalar(raw, bo, typ)
        elif tag == TAG_ImageLength:
            height = _decode_scalar(raw, bo, typ)
        elif tag == TAG_ImageDescription:
            try:
                desc = raw.split(b"\x00", 1)[0].decode("utf-8", errors="replace")
            except Exception:
                desc = None

    return width, height, desc


def _decode_scalar(raw: bytes, bo: str, typ: int) -> Optional[int]:
    try:
        if typ == 3:  # SHORT
            return struct.unpack(bo + "H", raw[:2])[0]
        if typ == 4:  # LONG
            return struct.unpack(bo + "I", raw[:4])[0]
        if typ == 16:  # LONG8
            return struct.unpack(bo + "Q", raw[:8])[0]
        # BYTE / ASCII not valid for width/height
    except Exception:
        return None
    return None


def gdc_query_slides(n_pool: int, seed: int, timeout: int = 60) -> List[Dict[str, Any]]:
    rng = random.Random(seed)
    fields = [
        "file_id","file_name","file_size","md5sum","data_type","data_format","access",
        "cases.submitter_id","cases.project.project_id",
        "cases.samples.sample_type","cases.samples.tissue_or_organ_of_origin",
        "cases.diagnoses.primary_diagnosis","cases.diagnoses.ajcc_pathologic_stage",
        "cases.diagnoses.ajcc_pathologic_t","cases.diagnoses.ajcc_pathologic_n","cases.diagnoses.ajcc_pathologic_m",
        "cases.diagnoses.age_at_diagnosis",
    ]

    filters = {
        "op": "and",
        "content": [
            {"op": "in", "content": {"field": "cases.project.project_id", "value": ["TCGA-BRCA"]}},
            {"op": "in", "content": {"field": "data_type", "value": ["Slide Image"]}},
            {"op": "in", "content": {"field": "access", "value": ["open"]}},
            {"op": "in", "content": {"field": "data_format", "value": ["SVS"]}},
        ],
    }

    params = {"size": str(n_pool), "fields": ",".join(fields), "format": "JSON"}
    resp = requests.post(GDC_FILES_ENDPOINT, params=params, headers={"Content-Type": "application/json"},
                         json={"filters": filters}, timeout=timeout)
    resp.raise_for_status()
    hits = resp.json().get("data", {}).get("hits", [])
    rng.shuffle(hits)
    return hits


def stream_download(url: str, dest_path: Path, expected_size: Optional[int], expected_md5: Optional[str],
                    chunk_size: int = 8 * 1024 * 1024, timeout: int = 60) -> Tuple[int, Optional[str]]:
    dest_path.parent.mkdir(parents=True, exist_ok=True)

    if dest_path.exists() and expected_size is not None:
        on_disk = dest_path.stat().st_size
        if on_disk == expected_size and expected_size > 0:
            return on_disk, None

    md5 = hashlib.md5() if expected_md5 else None

    with requests.get(url, stream=True, timeout=timeout) as r:
        r.raise_for_status()
        total = expected_size if expected_size is not None else int(r.headers.get("Content-Length", "0") or "0")

        tmp_path = dest_path.with_suffix(dest_path.suffix + ".part")
        bytes_written = 0

        with open(tmp_path, "wb") as f, tqdm(
            total=total if total > 0 else None,
            unit="B",
            unit_scale=True,
            unit_divisor=1024,
            desc=dest_path.name,
        ) as pbar:
            for chunk in r.iter_content(chunk_size=chunk_size):
                if not chunk:
                    continue
                f.write(chunk)
                bytes_written += len(chunk)
                if md5:
                    md5.update(chunk)
                pbar.update(len(chunk))

        os.replace(tmp_path, dest_path)

    computed = md5.hexdigest() if md5 else None
    if expected_md5 and computed and computed.lower() != expected_md5.lower():
        raise RuntimeError(f"MD5 mismatch for {dest_path.name}: expected {expected_md5}, got {computed}")

    return bytes_written, computed


def extract_low_hanging_labels(hit: Dict[str, Any]) -> Dict[str, Any]:
    out: Dict[str, Any] = {
        "file_id": hit.get("file_id"),
        "file_name": hit.get("file_name"),
        "file_size": hit.get("file_size"),
        "md5sum": hit.get("md5sum"),
        "data_type": hit.get("data_type"),
        "data_format": hit.get("data_format"),
        "access": hit.get("access"),
        "project_id": None,
        "case_submitter_id": None,
        "sample_type": None,
        "tissue_or_organ_of_origin": None,
        "primary_diagnosis": None,
        "ajcc_pathologic_stage": None,
        "ajcc_pathologic_t": None,
        "ajcc_pathologic_n": None,
        "ajcc_pathologic_m": None,
        "age_at_diagnosis_days": None,
        "tiff_probe": None,
    }

    cases = hit.get("cases") or []
    if cases:
        c0 = cases[0]
        out["project_id"] = ((c0.get("project") or {}).get("project_id")) if isinstance(c0.get("project"), dict) else None
        out["case_submitter_id"] = c0.get("submitter_id")

        samples = c0.get("samples") or []
        if samples:
            s0 = samples[0]
            out["sample_type"] = s0.get("sample_type")
            out["tissue_or_organ_of_origin"] = s0.get("tissue_or_organ_of_origin")

        diags = c0.get("diagnoses") or []
        if diags:
            d0 = diags[0]
            out["primary_diagnosis"] = d0.get("primary_diagnosis")
            out["ajcc_pathologic_stage"] = d0.get("ajcc_pathologic_stage")
            out["ajcc_pathologic_t"] = d0.get("ajcc_pathologic_t")
            out["ajcc_pathologic_n"] = d0.get("ajcc_pathologic_n")
            out["ajcc_pathologic_m"] = d0.get("ajcc_pathologic_m")
            out["age_at_diagnosis_days"] = d0.get("age_at_diagnosis")

    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--outdir", default="./tcga_brca_fullres_slides", help="Output directory")
    ap.add_argument("--n", type=int, default=10, help="How many slides to download")
    ap.add_argument("--seed", type=int, default=1337, help="RNG seed")
    ap.add_argument("--pool", type=int, default=500, help="How many candidates to probe before downloading")
    ap.add_argument("--min-width", type=int, default=50000, help="Min level-0 width to accept")
    ap.add_argument("--min-height", type=int, default=50000, help="Min level-0 height to accept")
    ap.add_argument("--dx-only", action="store_true", help="Only consider filenames containing 'DX'")
    ap.add_argument("--timeout", type=int, default=60, help="HTTP timeout seconds")
    args = ap.parse_args()

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)

    s3_config = load_s3_upload_config_from_env()
    s3_client = build_s3_client(s3_config) if s3_config else None
    if s3_config:
        print(
            f"S3 upload enabled: bucket={s3_config.bucket}, "
            f"prefix={s3_config.path_prefix or '/'}"
        )
    else:
        print("S3 upload disabled: set S3_BUCKET to enable uploads.")

    hits = gdc_query_slides(n_pool=args.pool, seed=args.seed, timeout=args.timeout)
    with open(outdir / "candidate_hits.json", "w") as f:
        json.dump(hits, f, indent=2)

    accepted: List[Dict[str, Any]] = []
    rejected: List[Dict[str, Any]] = []

    for hit in hits:
        if len(accepted) >= args.n:
            break

        file_id = hit["file_id"]
        file_name = safe_filename(hit.get("file_name") or f"{file_id}.svs")

        if args.dx_only and "DX" not in file_name:
            continue

        data_url = f"{GDC_DATA_ENDPOINT}/{file_id}"

        # Probe TIFF header/IFD to get full-res dimensions without full download
        try:
            w, h, desc, ranged = parse_tiff_first_ifd_dimensions(data_url, timeout=args.timeout)
        except Exception as e:
            # If range probing fails, fallback to a simple heuristic by size
            w, h, desc, ranged = None, None, None, False

        meta = extract_low_hanging_labels(hit)
        meta["tiff_probe"] = {"width": w, "height": h, "ranged": ranged}

        # Accept only if probe succeeded and dimensions are large
        if w is None or h is None:
            # fallback: reject small files aggressively (optional heuristic)
            fs = int(hit.get("file_size") or 0)
            if fs < 600 * 1024 * 1024:  # 600MB
                rejected.append(meta)
                continue
        else:
            if w < args.min_width or h < args.min_height:
                rejected.append(meta)
                continue

        # Passed prefilter: download
        dest = outdir / file_name
        print(f"\nDownloading (accepted by probe): {file_name}  ({w}x{h} if known)")
        stream_download(
            url=data_url,
            dest_path=dest,
            expected_size=int(hit["file_size"]) if hit.get("file_size") else None,
            expected_md5=hit.get("md5sum"),
            timeout=args.timeout,
        )

        if s3_client and s3_config:
            slide_key = upload_file_to_s3(
                client=s3_client,
                config=s3_config,
                local_file=dest,
                rel_path=dest.relative_to(outdir),
            )
            print(f"Uploaded slide to s3://{s3_config.bucket}/{slide_key}")

        meta_path = outdir / f"{file_name}.json"
        with open(meta_path, "w") as f:
            json.dump(meta, f, indent=2)

        if s3_client and s3_config:
            meta_key = upload_file_to_s3(
                client=s3_client,
                config=s3_config,
                local_file=meta_path,
                rel_path=meta_path.relative_to(outdir),
            )
            print(f"Uploaded metadata to s3://{s3_config.bucket}/{meta_key}")

        accepted.append(meta)

    accepted_path = outdir / "accepted.json"
    rejected_path = outdir / "rejected.json"
    candidate_path = outdir / "candidate_hits.json"

    with open(accepted_path, "w") as f:
        json.dump(accepted, f, indent=2)
    with open(rejected_path, "w") as f:
        json.dump(rejected, f, indent=2)

    if s3_client and s3_config:
        for summary_path in (accepted_path, rejected_path, candidate_path):
            summary_key = upload_file_to_s3(
                client=s3_client,
                config=s3_config,
                local_file=summary_path,
                rel_path=summary_path.relative_to(outdir),
            )
            print(f"Uploaded summary to s3://{s3_config.bucket}/{summary_key}")

    print(f"\nDone. Accepted {len(accepted)} / requested {args.n}. Output: {outdir.resolve()}")
    if len(accepted) < args.n:
        print("Tip: increase --pool or lower --min-width/--min-height.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
