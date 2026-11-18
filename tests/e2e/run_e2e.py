#!/usr/bin/env python3
"""Unified CLI runner for neoveg opaque-box E2E tests (Tiers 1-4, Features F1-F10)."""

from __future__ import annotations

import argparse
import os
import sys
import unittest
from collections import defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from tests.e2e import (  # noqa: E402
    test_tier1_build_fonts,
    test_tier1_encoder,
    test_tier2_boundaries,
    test_tier3_pairwise,
    test_tier4_scenarios,
)

TIER_MODULES = {
    1: [test_tier1_build_fonts, test_tier1_encoder],
    2: [test_tier2_boundaries],
    3: [test_tier3_pairwise],
    4: [test_tier4_scenarios],
}

VALID_FEATURES = [f"F{i}" for i in range(1, 11)]


def _test_tier(test: unittest.TestCase) -> int:
    mod = sys.modules.get(test.__class__.__module__)
    return int(getattr(mod, "TIER", 0))


def _test_features(test: unittest.TestCase) -> tuple[str, ...]:
    method = getattr(test, test._testMethodName, None)
    m_tags = tuple(getattr(method, "_feature_tags", ()))
    c_tags = tuple(getattr(test.__class__, "_feature_tags", ()))
    merged = tuple(dict.fromkeys((*c_tags, *m_tags)))
    return merged


def collect_suite(tier_arg: str, feature_arg: str | None) -> unittest.TestSuite:
    loader = unittest.TestLoader()
    tiers = [1, 2, 3, 4] if tier_arg == "all" else [int(tier_arg)]
    selected = unittest.TestSuite()
    for t in tiers:
        for mod in TIER_MODULES[t]:
            mod_suite = loader.loadTestsFromModule(mod)
            for suite_or_case in mod_suite:
                cases = (
                    list(suite_or_case)
                    if isinstance(suite_or_case, unittest.TestSuite)
                    else [suite_or_case]
                )
                for case in cases:
                    if feature_arg is not None:
                        if feature_arg not in _test_features(case):
                            continue
                    selected.addTest(case)
    return selected


def iter_cases(suite: unittest.TestSuite) -> list[unittest.TestCase]:
    out: list[unittest.TestCase] = []
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            out.extend(iter_cases(item))
        else:
            out.append(item)
    return out


class TrackingResult(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.outcomes: dict[str, str] = {}
        self.cases_by_id: dict[str, unittest.TestCase] = {}

    def startTest(self, test: unittest.TestCase) -> None:
        super().startTest(test)
        self.cases_by_id[test.id()] = test

    def addSuccess(self, test: unittest.TestCase) -> None:
        super().addSuccess(test)
        self.outcomes[test.id()] = "PASS"

    def addFailure(self, test: unittest.TestCase, err) -> None:
        super().addFailure(test, err)
        self.outcomes[test.id()] = "FAIL"

    def addError(self, test: unittest.TestCase, err) -> None:
        super().addError(test, err)
        self.outcomes[test.id()] = "ERROR"

    def addSkip(self, test: unittest.TestCase, reason: str) -> None:
        super().addSkip(test, reason)
        self.outcomes[test.id()] = "SKIP"


class TrackingRunner(unittest.TextTestRunner):
    resultclass = TrackingResult


def print_breakdown(cases: list[unittest.TestCase], outcomes: dict[str, str] | None = None) -> None:
    tier_counts: dict[int, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    feat_counts: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))

    for c in cases:
        t = _test_tier(c)
        feats = _test_features(c)
        status = outcomes.get(c.id(), "LISTED") if outcomes is not None else "LISTED"
        tier_counts[t]["total"] += 1
        tier_counts[t][status] += 1
        for f in feats:
            feat_counts[f]["total"] += 1
            feat_counts[f][status] += 1

    print("\n=== E2E Breakdown by Tier ===")
    for t in sorted(tier_counts):
        d = tier_counts[t]
        if outcomes is None:
            print(f"  Tier {t}: {d['total']} tests")
        else:
            print(
                f"  Tier {t}: total={d['total']} | pass={d['PASS']} | fail={d['FAIL']} | error={d['ERROR']} | skip={d['SKIP']}"
            )

    print("\n=== E2E Breakdown by Feature ===")
    for f in VALID_FEATURES:
        if f not in feat_counts:
            continue
        d = feat_counts[f]
        if outcomes is None:
            print(f"  {f}: {d['total']} tests")
        else:
            print(
                f"  {f}: total={d['total']} | pass={d['PASS']} | fail={d['FAIL']} | error={d['ERROR']} | skip={d['SKIP']}"
            )
    print(f"\nTotal selected tests: {len(cases)}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="neoveg Opaque-Box E2E Test Runner")
    parser.add_argument(
        "--tier",
        choices=["1", "2", "3", "4", "all"],
        default="all",
        help="Run only tests from the selected tier (default: all)",
    )
    parser.add_argument(
        "--feature",
        choices=VALID_FEATURES,
        default=None,
        help="Run only tests tagged with the given feature (F1..F10)",
    )
    parser.add_argument(
        "--allow-destructive-target-clean",
        action="store_true",
        help="Allow Tier 4 Scenario 1 to invoke build_portable.py directly on repo root (deletes <repo_root>/target)",
    )
    parser.add_argument(
        "--skip-cargo-gates",
        action="store_true",
        help="Skip slow cargo fmt/clippy/test subprocess gates inside E2E tests",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="List discovered test counts by Tier and Feature without running them",
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        help="Verbose unittest output",
    )
    args = parser.parse_args(argv)

    if args.allow_destructive_target_clean:
        os.environ["NEOVEG_E2E_ALLOW_TARGET_CLEAN"] = "1"
    if args.skip_cargo_gates:
        os.environ["NEOVEG_E2E_SKIP_CARGO_GATES"] = "1"

    suite = collect_suite(args.tier, args.feature)
    cases = iter_cases(suite)

    if args.list:
        print_breakdown(cases, outcomes=None)
        return 0

    verbosity = 2 if args.verbose else 1
    runner = TrackingRunner(verbosity=verbosity)
    result: TrackingResult = runner.run(suite)  # type: ignore[assignment]
    print_breakdown(cases, outcomes=result.outcomes)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
