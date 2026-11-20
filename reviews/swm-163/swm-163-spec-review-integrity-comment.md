<!-- reviewer-dispositions repo=agent-ix/ix-board visibility=public quoin=0.36.7 module=spec-artifacts-process@v0.29.2 id=SR-6050 round=1 model=unavailable run=C2172778-CBEE-4CEA-AA04-3AD2D7E45137 -->

| FND | Outcome | Reason |
|---|---|---|
| FND-001 | fixed | Actual Inputs now requires selected statistic/component unit, expected native key set and worst_severity reduction; missing keys remain unknown with completeness=false, unselected keys are diagnostic only, and statistic/key/reducer changes affect comparison identity. AC-11 supplies the exact p90-vs-median/missing-C/unselected-D oracle and invalid configuration cases. |

Verified against frozen fix commit 09931ee2fc9f07a6f4e451fdd26eb607efe64221. No new findings. Exact model ID unavailable; provenance limitation.

+++ [reviewer data]

```yaml
{
  "dispositions": [
    {
      "fnd": "FND-001",
      "outcome": "fixed",
      "after_excerpt": "A scalar policy selects `value`; a duration policy explicitly selects `count`,\n`median` or `p90`. The policy unit matches the selected component, rather than\ntreating a population count as a duration. A keyed policy declares a nonempty\nset of expected native keys and `key_reduction: worst_severity`. The engine\nSHALL assess the selected statistic separately for every expected key and retain\neach key's value, severity and unavailable/unassessed reason.",
      "reason": "Actual Inputs now requires selected statistic/component unit, expected native key set and worst_severity reduction; missing keys remain unknown with completeness=false, unselected keys are diagnostic only, and statistic/key/reducer changes affect comparison identity. AC-11 supplies the exact p90-vs-median/missing-C/unselected-D oracle and invalid configuration cases."
    }
  ]
}
```

+++
