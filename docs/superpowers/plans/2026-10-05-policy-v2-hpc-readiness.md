# Policy v2 implementation and HPC readiness

Approved scope: B2 credential/key binding and full 256-bit sparse nullifier tree; B3 salted polygon/event Merkle commitments; B1 shared accepted registry roots through typed epoch aggregation and EVM; ObjectEvent mapper and selective disclosure; E1–E4 strict runners/analyzers and configurable HPC dry-run jobs; cost conversion; truthful paper/checklist evidence.

Keep canonical events 86 bytes. Witness/epoch wire version 2, leaf/aggregate ABI 352 bytes; aggregate appends certificate root and polygon commitment. Explicit child kind. Preserve historical artifacts.

Commit/check order: test boundary; B2; B3; B1/ABI/adapters; mapper/disclosure; harness/HPC preparation; docs/local validation. Dependent consumers must all be synchronized before HPC-ready claims.

Local limits: build jobs 1, Rayon threads 2, guest execute only eight events with 50M-cycle/120s/4GiB ceiling and 512MiB swap-growth ceiling. No proof generation, full benchmarks, sbatch, Sepolia broadcast, submission tag or finaltrue. Anvil stubs are test-only. Missing measurements remain N/A/red placeholders.

Future work: C1 plot/multi-polygon and physical lot-graph continuity. Author ORCID/funding and formal measurements remain pending. Formal proof integration and all experiment results require HPC evidence.

Implementation tasks: (1) test boundary and v2 wire; (2) B2; (3) B3; (4) B1 contracts/typed aggregation; (5) shared fixture registry; (6) mapper; (7) disclosure; (8) E1; (9) E2; (10) E3; (11) E4/cost; (12) HPC jobs and evidence docs.
