# letopis resource model: consolidated partial arithmetic; capacity remains unverified.
# D-56: A-1 is the year-end target; history references are not a growth curve.
# Fixed target and ranges: D-31, D-36, D-38, D-41 through D-47; file spread: D-51; packing interval: D-54.
import json
from math import ceil

ASSUMPTIONS = {
    "A-1": {"label": "knowledge nodes at the year-end target scale", "unit": "nodes",
            "best": 100000, "expected": 100000, "worst": 100000},
    "A-2": {"label": "mean marked Markdown fragment", "unit": "bytes/node",
            "best": 500, "expected": 2000, "worst": 8000},
    "A-3": {"label": "instantaneous reference input: new or changed knowledge nodes", "unit": "nodes/minute",
            "best": 10, "expected": 100, "worst": 1000},
    "A-4": {"label": "source nodes at the target scale", "unit": "nodes",
            "best": 100000, "expected": 100000, "worst": 100000},
    "A-5": {"label": "mean knowledge nodes per marked file", "unit": "nodes/file",
            "best": 10, "expected": 30, "worst": 50},  # Midpoint 30 is assumed, D-43.
    "A-6": {"label": "reference active hours per day, not a measured duty cycle", "unit": "hours/day",
            "best": 24, "expected": 24, "worst": 24},
    "A-7": {"label": "forecast horizon", "unit": "days",
            "best": 365, "expected": 365, "worst": 365},
    "A-8": {"label": "mean created or edited nodes per successful apply", "unit": "nodes/apply",
            "best": 5, "expected": 5, "worst": 5},
    "A-9": {"label": "reference interval between Git packing", "unit": "hours",
            "best": 24, "expected": 24, "worst": 24},
    "A-10": {"label": "saved diagnostic reports per day; not individual retries or requests", "unit": "reports/day",
             "best": 10, "expected": 100, "worst": 1000},
    "A-11": {"label": "mean diagnostic report payload; excludes SQLite overhead", "unit": "bytes/report",
             "best": 4000, "expected": 4000, "worst": 4000},
}

VECTOR_DIMENSIONS = 768          # D-41, default output of the exact Ollama tag.
MODEL_BLOB_BYTES = 238530688     # R-3, official registry model layer only.
FLOAT32_BYTES = 4               # Conditional payload estimate, not a format decision.

UNITS = {
    "create_edit_apply_per_second": "apply/s",
    "reference_daily_create_edit_apply_reference": "apply/day",
    "reference_horizon_create_edit_apply_reference": "apply",
    "reference_daily_changes_reference": "changes/day",
    "reference_horizon_changes_reference": "changes",
    "knowledge_file_count_estimate": "files",
    "mean_marked_file_kb": "KB/file",
    "mean_touched_files_per_apply": "files/apply",
    "reference_horizon_file_versions_reference": "file versions",
    "reference_horizon_file_content_reference_tb": "TB",
    "reference_between_pack_apply_reference": "apply",
    "reference_between_pack_file_versions_reference": "file versions",
    "reference_between_pack_file_content_reference_gb": "GB",
    "graph_node_rows": "rows",
    "canonical_markdown_mb": "MB",
    "vectors_payload_mb_if_float32": "MB",
    "model_blob_mb": "MB",
    "known_storage_payload_mb": "MB",
    "changed_elements_per_second": "nodes/s",
    "changed_markdown_kb_per_second": "KB/s",
    "embedding_job_rows_upper": "rows",
    "bulk_input_minutes_reference": "minutes",
    "diagnostics_daily_payload_mb": "MB/day",
    "diagnostics_horizon_reports": "reports",
    "diagnostics_horizon_payload_mb": "MB",
}


def compute(a):
    # D-59: conditional constant-rate history reference, not ordinary traffic or a growth forecast.
    reference_daily_changes_reference = ASSUMPTIONS["A-3"]["expected"] * a["A-6"] * 60
    reference_horizon_changes_reference = reference_daily_changes_reference * a["A-7"]
    create_edit_apply_per_second = a["A-3"] / a["A-8"] / 60
    reference_daily_apply_reference = reference_daily_changes_reference / a["A-8"]
    reference_horizon_apply_reference = reference_horizon_changes_reference / a["A-8"]
    knowledge_file_count_estimate = ceil(a["A-1"] / a["A-5"])  # Corpus shape estimate.
    mean_marked_file_kb = a["A-2"] * a["A-5"] / 1000          # Marked text, no extra file text.
    mean_touched_files_per_apply = a["A-8"]  # D-51: one changed node per touched file.
    reference_horizon_file_versions = reference_horizon_apply_reference * mean_touched_files_per_apply
    reference_horizon_file_content_tb = reference_horizon_file_versions * mean_marked_file_kb / 1000000000
    reference_between_pack_apply = reference_daily_apply_reference * a["A-9"] / 24
    reference_between_pack_file_versions = reference_between_pack_apply * mean_touched_files_per_apply
    reference_between_pack_file_content_gb = reference_between_pack_file_versions * mean_marked_file_kb / 1000000
    graph_node_rows = a["A-1"] + a["A-4"]                  # Graph: knowledge plus sources.
    canonical_markdown_mb = a["A-1"] * a["A-2"] / 1000000   # Storage: marked fragments.
    vectors_payload_mb = a["A-1"] * VECTOR_DIMENSIONS * FLOAT32_BYTES / 1000000
    model_blob_mb = MODEL_BLOB_BYTES / 1000000              # Model file, not runtime RAM.
    known_storage_payload_mb = canonical_markdown_mb + vectors_payload_mb + model_blob_mb
    changed_elements_per_second = a["A-3"] / 60            # Input flow, not completed jobs.
    changed_markdown_kb_per_second = a["A-3"] * a["A-2"] / 60 / 1000
    embedding_job_rows_upper = a["A-1"]                    # All job rows, queued + failed; no sources.
    bulk_input_minutes = a["A-1"] / ASSUMPTIONS["A-3"]["worst"]  # D-59: one full input at heavy rate; not vector readiness.
    return {
        "create_edit_apply_per_second": create_edit_apply_per_second,
        "reference_daily_create_edit_apply_reference": reference_daily_apply_reference,
        "reference_horizon_create_edit_apply_reference": reference_horizon_apply_reference,
        "reference_daily_changes_reference": reference_daily_changes_reference,
        "reference_horizon_changes_reference": reference_horizon_changes_reference,
        "knowledge_file_count_estimate": knowledge_file_count_estimate,
        "mean_marked_file_kb": mean_marked_file_kb,
        "mean_touched_files_per_apply": mean_touched_files_per_apply,
        "reference_horizon_file_versions_reference": reference_horizon_file_versions,
        "reference_horizon_file_content_reference_tb": reference_horizon_file_content_tb,
        "reference_between_pack_apply_reference": reference_between_pack_apply,
        "reference_between_pack_file_versions_reference": reference_between_pack_file_versions,
        "reference_between_pack_file_content_reference_gb": reference_between_pack_file_content_gb,
        "graph_node_rows": graph_node_rows,
        "canonical_markdown_mb": canonical_markdown_mb,
        "vectors_payload_mb_if_float32": vectors_payload_mb,
        "model_blob_mb": model_blob_mb,
        "known_storage_payload_mb": known_storage_payload_mb,
        "changed_elements_per_second": changed_elements_per_second,
        "changed_markdown_kb_per_second": changed_markdown_kb_per_second,
        "embedding_job_rows_upper": embedding_job_rows_upper,
        "bulk_input_minutes_reference": bulk_input_minutes,
        # No manual clearing over A-7. Scenario rates and mean payload are not hard bounds.
        "diagnostics_daily_payload_mb": a["A-10"] * a["A-11"] / 1000000,
        "diagnostics_horizon_reports": a["A-10"] * a["A-7"],
        "diagnostics_horizon_payload_mb": a["A-10"] * a["A-11"] * a["A-7"] / 1000000,
    }


# Draft: no capacity limit can be evaluated until RAM/CPU/storage overheads are known.
# Do not read an empty list as a successful check of NFR-2/NFR-3/NFR-4/NFR-6.
LIMITS = []


if __name__ == "__main__":
    results = {
        scenario: compute({key: value[scenario] for key, value in ASSUMPTIONS.items()})
        for scenario in ("best", "expected", "worst")
    }
    print(json.dumps(results, indent=2))
