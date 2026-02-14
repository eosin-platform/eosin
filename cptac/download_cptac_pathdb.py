#!/usr/bin/env python3
"""
Download first N pathology WSIs from a CPTAC collection hosted on TCIA PathDB,
and write a JSON sidecar per slide with basic metadata.

Uses tcia_utils.pathdb:
- getCollections(query=..., format=...)
- getImages(query=..., format=...)  (paginates automatically)
- downloadImages(images_data, path=..., number=...)  (downloads concurrently)

Docs/examples: TCIA_PathDB_Queries.ipynb (tcia_utils PathDB module) :contentReference[oaicite:1]{index=1}
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional

from tcia_utils import pathdb  # pip install --upgrade tcia_utils


def _safe_filename(name: str) -> str:
    keep = " ._-()[]{}"
    return "".join(c for c in name if c.isalnum() or c in keep).strip().replace(" ", "_")


def _pick_metadata_fields(m: Dict[str, Any]) -> Dict[str, Any]:
    """
    Return a "short" metadata dict. We don't know the exact field set ahead of time
    (PathDB differs per collection), so we opportunistically extract common keys.
    """
    preferred_keys = [
        # identifiers
        "collectionName", "collectionId",
        "subjectId", "caseId", "patientId",
        "imageId", "imageName",
        # file/url info
        "imageUrl", "fileName", "fileSize",
        # slide/scanner/stain/specimen-ish fields (names vary)
        "stain", "stainType", "staining",
        "scanner", "scannerModel", "manufacturer",
        "tissue", "tissueType", "specimen", "specimenType",
        "anatomicSite", "organ",
        # anything diagnosis-ish if present
        "diagnosis", "tumorType", "label", "grade",
    ]

    short: Dict[str, Any] = {}
    for k in preferred_keys:
        if k in m and m[k] not in (None, "", []):
            short[k] = m[k]

    # Always include these if present under any common variant
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


def _iter_records(images_data: Any) -> Iterable[Dict[str, Any]]:
    """
    pathdb.getImages() can return a list[dict] by default, or a DataFrame if format="df".
    We support both without requiring pandas.
    """
    if images_data is None:
        return []
    if isinstance(images_data, list):
        return (x for x in images_data if isinstance(x, dict))
    # DataFrame-like (duck typing)
    if hasattr(images_data, "to_dict"):
        rows = images_data.to_dict(orient="records")
        return (x for x in rows if isinstance(x, dict))
    raise TypeError(f"Unsupported images_data type: {type(images_data)!r}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--outdir", required=True, help="Output directory for slides + sidecar JSON")
    ap.add_argument(
        "--collection",
        default=None,
        help='PathDB collection name (e.g., "CPTAC-CCRCC", "CPTAC-LUAD"). If omitted, we list matches for --query and exit.',
    )
    ap.add_argument(
        "--query",
        default="CPTAC",
        help='Collection search query used when listing collections (default: "CPTAC")',
    )
    ap.add_argument("--n", type=int, default=10, help="Number of slides to download (default: 10)")
    ap.add_argument("--max-workers", type=int, default=8, help="Concurrent downloads (default: 8)")
    ap.add_argument(
        "--write-full-metadata",
        action="store_true",
        help="Also embed the full raw PathDB metadata dict in the sidecar JSON",
    )
    args = ap.parse_args()

    outdir = Path(args.outdir).expanduser().resolve()
    outdir.mkdir(parents=True, exist_ok=True)

    if not args.collection:
        cols = pathdb.getCollections(query=args.query, format="df")
        # Print a helpful minimal view without requiring pandas formatting assumptions
        print("Matching collections (showing first ~50 rows):")
        try:
            # DataFrame case
            print(cols.head(50).to_string(index=False))
        except Exception:
            # List-of-dicts case
            for i, row in enumerate(cols[:50] if isinstance(cols, list) else []):
                print(row)
        print("\nRe-run with --collection <CollectionName> to download slides.")
        return

    # 1) query slide metadata (PathDB paginates under the hood) :contentReference[oaicite:2]{index=2}
    images = pathdb.getImages(query=args.collection)  # default is JSON/list
    records = list(_iter_records(images))
    if not records:
        raise SystemExit(f"No images returned for collection={args.collection!r}")

    # 2) download first N images
    n = max(0, min(args.n, len(records)))
    to_download = records[:n]

    # downloadImages() expects the metadata output from getImages() (list[dict] or df). :contentReference[oaicite:3]{index=3}
    # We pass a list[dict] slice for clarity.
    pathdb.downloadImages(to_download, path=str(outdir), max_workers=args.max_workers, number=n)

    # 3) write sidecar JSON for each downloaded slide
    # We infer the downloaded filename from the imageUrl when possible.
    for m in to_download:
        image_url = m.get("imageUrl") or m.get("url") or ""
        filename_guess = os.path.basename(image_url) if image_url else None

        # Fall back to a stable synthetic name if we can't infer the file name
        if not filename_guess:
            image_id = str(m.get("imageId") or m.get("ImageID") or m.get("id") or "unknown")
            filename_guess = _safe_filename(f"{args.collection}__{image_id}.svs")

        slide_path = outdir / filename_guess

        sidecar = {
            "source": {
                "provider": "TCIA PathDB",
                "collection": args.collection,
            },
            "short_metadata": _pick_metadata_fields(m),
        }
        if args.write_full_metadata:
            sidecar["raw_pathdb_metadata"] = m

        sidecar_path = slide_path.with_suffix(slide_path.suffix + ".json")
        with open(sidecar_path, "w", encoding="utf-8") as f:
            json.dump(sidecar, f, indent=2, sort_keys=True)

    print(f"Done. Downloaded {n} slides to: {outdir}")


if __name__ == "__main__":
    main()
