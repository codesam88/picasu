"""Pure measurement model for benchmark metrics.

No I/O. Families, backgrounds, accesses, statistics, units, column shapes,
keys, pooling, percentiles and bootstrap intervals.
"""

from __future__ import annotations

import math
import random

FAMILIES = ("latency", "wall", "stage", "upload", "gauge", "count")
BACKGROUNDS = ("idle", "scan")
ACCESSES = ("prefetch", "thumb", "edit", "scroll_bar", "scroll_rows", "scroll_data")
STATS = ("p50", "p95", "max")
COUNT_STAT = "sum"
UNITS = {
    "latency": "ms",
    "wall": "s",
    "stage": "us",
    "upload": "ms",
    "gauge": "MB",
    "count": "ops",
}
REPORT_COLUMNS = (
    "cell",
    "family",
    "background",
    "subject",
    "stat",
    "value",
    "unit",
    "n_samples",
    "n_clusters",
    "ci_low",
    "ci_high",
)
BASELINE_COLUMNS = REPORT_COLUMNS + ("recorded", "note")

_CI_LOW_PCT = 2.5
_CI_HIGH_PCT = 97.5
DEFAULT_RESAMPLES = 1000
LARGE_SAMPLE_N = 5000
LARGE_SAMPLE_RESAMPLES = 200


def metric_key(family, background, subject) -> str:
    return f"{family}|{background}|{subject}"


def split_key(key: str):
    family, background, subject = key.split("|", 2)
    return family, background, subject


def statistics_for(family):
    if family == "count":
        return (COUNT_STAT,)
    return STATS


def percentile(values, q):
    ordered = sorted(values)
    if not ordered:
        raise ValueError("percentile of an empty sample")
    if len(ordered) == 1:
        return ordered[0]
    rank = (len(ordered) - 1) * q / 100.0
    low = math.floor(rank)
    high = math.ceil(rank)
    if low == high:
        return ordered[low]
    return ordered[low] + (ordered[high] - ordered[low]) * (rank - low)


def resamples_for(size: int) -> int:
    return LARGE_SAMPLE_RESAMPLES if size > LARGE_SAMPLE_N else DEFAULT_RESAMPLES


def summarize(values, family):
    values = [float(v) for v in values]
    if family == "count":
        return {COUNT_STAT: float(sum(values)) if values else 0.0}
    if not values:
        return {"p50": 0.0, "p95": 0.0, "max": 0.0}
    return {
        "p50": float(percentile(values, 50)),
        "p95": float(percentile(values, 95)),
        "max": float(max(values)),
    }


def bootstrap_interval(values, seed=1234, resamples=None):
    values = list(values)
    if len(values) < 2:
        return (None, None)
    if resamples is None:
        resamples = resamples_for(len(values))
    if resamples < 1:
        raise ValueError("resamples must be at least 1")
    rng = random.Random(seed)
    size = len(values)
    bootstrapped = [
        percentile([values[rng.randrange(size)] for _ in range(size)], 50) for _ in range(resamples)
    ]
    low = percentile(bootstrapped, _CI_LOW_PCT)
    high = percentile(bootstrapped, _CI_HIGH_PCT)
    return (low, high)
