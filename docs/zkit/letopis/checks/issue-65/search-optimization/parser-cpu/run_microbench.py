#!/usr/bin/env python3
"""Repeat the isolated baseline/candidate parser comparison on immutable input."""
import json
from pathlib import Path
import statistics
import subprocess

root = Path(__file__).resolve().parent
input_path = root.parents[1] / '2026-11-06-native-followup/ollama-100k/repository/knowledge/part-0000.md'
rows = {'baseline': [], 'line_cursor': []}
for run in range(9):
    pair = [('baseline', 'baseline-parser-probe'), ('line_cursor', 'line-cursor-parser-probe')]
    if run % 2:
        pair.reverse()
    for kind, binary in pair:
        result = subprocess.run([str(root / binary), str(input_path), '2000', str(root / (kind + '-projection.txt'))], capture_output=True, text=True, check=True)
        rows[kind].append(json.loads(result.stdout))
result = {kind: {'samples': samples, 'median_seconds': statistics.median(row['elapsed_seconds'] for row in samples)} for kind, samples in rows.items()}
result['projections_identical'] = (root / 'baseline-projection.txt').read_bytes() == (root / 'line_cursor-projection.txt').read_bytes()
print(json.dumps(result, indent=2))
