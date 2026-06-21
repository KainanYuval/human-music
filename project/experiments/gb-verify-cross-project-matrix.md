# gb-verify cross-project matrix (3×3)

## Context

Do the three example `.band` projects **reject each other's** released exports? Same-artist stem libraries might chroma-match unrelated songs → false PASS.

## Pairs tested

| Band | Audio export |
|------|----------------|
| band1 | example_1 — `כל מה .band` |
| band2 | example_2 — `השיר הלbm.band` |
| band3 | example_3 — `nobodys_listening_anyway.band` |
| audio1 | example_1 MP3 (19.3 s) |
| audio2 | example_2 WAV (50.5 s) |
| audio3 | example_3 WAV (206.5 s) |

CLI: `gb-verify --project <band> --audio <export> --out data/cross_check/<label>`

## Provenance matrix (%)

Row = `.band`, column = released audio. `*` = intended pair.

|  | audio1 | audio2 | audio3 |
|--|--------|--------|--------|
| **band1** | **98.7 PASS*** | 100.0 PASS | 99.7 PASS |
| **band2** | 98.7 PASS | **100.0 PASS*** | 99.7 PASS |
| **band3** | 98.7 PASS | 100.0 PASS | **99.7 PASS*** |

## Cross-validation result

**All 9 combinations PASS** (exit 0). No false negatives on correct pairs; **6/6 cross pairs also PASS**.

### Discriminability (correct vs best wrong band)

| Audio | Correct band % | Best wrong band % | Δ |
|-------|----------------|-------------------|---|
| audio1 | 98.73 | 98.73 | **0.00 pp** |
| audio2 | 99.98 | 99.98 | **0.00 pp** |
| audio3 | 99.66 | 99.66 | **0.00 pp** |

Provenance score is **identical** whether the matching `.band` is the true source or not (for each export). Asset file hashes are **disjoint** across all three projects (0 shared stems).

### Interpretation

- Verdict uses timeline coverage ≥70% + any strong/possible stem match inside **whichever** `.band` is supplied.
- Chroma is pitch-class based: short/generic stems (DI guitar, sub, loops) match many mixes from the same producer.
- **Provenance % measures “explained by *some* stems in this bundle”**, not “explained by *this* project uniquely.”
- Cross-project check **does not separate** these three examples — all look equally explained.

### Timing (cross runs)

| Label | Wall (s) |
|-------|----------|
| X band1 × audio2 | 2.65 |
| X band1 × audio3 | 6.64 |
| X band2 × audio1 | 1.26 |
| X band2 × audio3 | 5.33 |
| X band3 × audio1 | 12.43 |
| X band3 × audio2 | 19.15 |

## Artifacts

`data/cross_check/{P1_band1_audio1,P2_…,X_band1_audio2,…}/report.json`

## Notes

To discriminate projects: need uniqueness constraint (e.g. require majority of timeline explained by **one** project's **specific** stem set vs max score on unrelated bundles, or metadata/timeline alignment from `MetaData.plist`).
