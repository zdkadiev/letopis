#!/usr/bin/env python3
"""Literal RRF counterexamples; no product import, build, database or timing."""

from fractions import Fraction
import json


def rank_channels(channels, eligible, limit, filter_before_rank=True):
    scores = {}
    for channel in channels:
        seen = set()
        ordered = []
        for element in channel:
            if element not in seen and (not filter_before_rank or element in eligible):
                ordered.append(element)
                seen.add(element)
        for rank, element in enumerate(ordered, 1):
            if element in eligible:
                scores[element] = scores.get(element, Fraction()) + Fraction(1, rank)
    return sorted(scores, key=lambda element: (-scores[element], element))[:limit]


def main():
    # Given: channel order is already metric-ascending, with literal eligible IDs.
    cases = [
        ("filter after raw fusion", [["excluded", "a", "b"], ["b", "a"]],
         {"a", "b"}, [["excluded", "a", "b"], ["b", "a"]], False, ["a"], ["b"]),
        ("native K=1 loses overlapping winner", [["z", "a"], ["y", "a"]],
         {"a", "y", "z"}, [["z", "a"], ["y"]], True, ["a"], ["y"]),
        ("native K=1 consumed by excluded head", [[], ["excluded", "a"]],
         {"a"}, [[], ["excluded"]], True, ["a"], []),
    ]
    for name, complete, eligible, tempting, filter_first, expected, wrong in cases:
        # When: complete ranking and the tempting shortcut consume the same literal case.
        actual = rank_channels(complete, eligible, 1)
        shortcut = rank_channels(tempting, eligible, 1, filter_first)
        # Then: independent literal winners differ; the shortcut violates exact search.
        assert actual == expected, (name, actual, expected)
        assert shortcut == wrong, (name, shortcut, wrong)
        assert actual != shortcut
        print(json.dumps({"case": name, "required": actual, "shortcut": shortcut}))


if __name__ == "__main__":
    main()
