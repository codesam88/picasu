#!/usr/bin/env python3
"""Tests for metrics.py."""

import sys
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent
if str(BENCH) not in sys.path:
    sys.path.insert(0, str(BENCH))

import extract_bench  # noqa: E402
import metrics  # noqa: E402


class PercentileTest(unittest.TestCase):
    def test_matches_extract_bench_linear_interpolation(self):
        for values, pct in (([1.0], 95), ([1.0, 3.0], 95), ([0.0, 10.0, 20.0], 50)):
            with self.subTest(values=values, pct=pct):
                self.assertEqual(
                    metrics.percentile(values, pct), extract_bench.percentile(values, pct)
                )

    def test_empty_sample_is_rejected(self):
        with self.assertRaises(ValueError):
            metrics.percentile([], 50)


class ResampleCapTest(unittest.TestCase):
    def test_small_and_large_samples_choose_their_counts(self):
        self.assertEqual(metrics.resamples_for(100), 1000)
        self.assertEqual(metrics.resamples_for(5000), 1000)
        self.assertEqual(metrics.resamples_for(5001), 200)

    def test_default_matches_the_capped_explicit_count(self):
        values = [(index % 31) / 100.0 for index in range(5001)]

        default = metrics.bootstrap_interval(values)
        capped = metrics.bootstrap_interval(values, resamples=200)

        # bootstrap_interval returns only (low, high); compare via rounded
        self.assertEqual(default, capped)


class BootstrapIntervalTest(unittest.TestCase):
    def test_fewer_than_two_values_has_no_interval(self):
        self.assertEqual(metrics.bootstrap_interval([]), (None, None))
        self.assertEqual(metrics.bootstrap_interval([5.0]), (None, None))

    def test_constant_samples_collapse_to_one_value(self):
        self.assertEqual(metrics.bootstrap_interval([5.0] * 4), (5.0, 5.0))

    def test_wide_samples_give_a_wider_deterministic_interval(self):
        values = [1.0, 2.0, 100.0, 200.0]

        first = metrics.bootstrap_interval(values)
        second = metrics.bootstrap_interval(values)
        narrow = metrics.bootstrap_interval([5.0] * 4)

        self.assertEqual(first, second)
        low, high = first
        median = 51.0
        self.assertLess(low, median)
        self.assertLess(median, high)
        self.assertGreaterEqual(low, min(values))
        self.assertLessEqual(high, max(values))
        self.assertGreater(high - low, narrow[1] - narrow[0])

    def test_custom_resample_count_is_deterministic_and_bounded(self):
        values = [1.0, 2.0, 100.0, 200.0]

        result = metrics.bootstrap_interval(values, resamples=50, seed=7)

        self.assertEqual(result, metrics.bootstrap_interval(values, resamples=50, seed=7))
        low, high = result
        self.assertLessEqual(low, high)


class SummarizeTest(unittest.TestCase):
    def test_summarize_count(self):
        self.assertEqual(metrics.summarize([1, 2, 3], "count"), {"sum": 6.0})

    def test_summarize_latency(self):
        out = metrics.summarize([1.0, 2.0, 3.0, 4.0], "latency")
        self.assertIn("p50", out)
        self.assertIn("p95", out)
        self.assertIn("max", out)


class MetricKeyTest(unittest.TestCase):
    def test_round_trip(self):
        key = metrics.metric_key("latency", "idle", "prefetch")
        self.assertEqual(key, "latency|idle|prefetch")
        self.assertEqual(metrics.split_key(key), ("latency", "idle", "prefetch"))

    def test_statistics_for(self):
        self.assertEqual(metrics.statistics_for("count"), ("sum",))
        self.assertEqual(metrics.statistics_for("latency"), ("p50", "p95", "max"))


if __name__ == "__main__":
    unittest.main()
