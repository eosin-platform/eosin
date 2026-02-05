#!/bin/bash
cd $(dirname "$0")
source .venv/bin/activate
python3 slide_viewer.py "/home/thavlik/Repositories/camelyon16/CAMELYON16/images/normal_004.tif"