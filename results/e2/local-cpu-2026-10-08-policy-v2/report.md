# E2 policy v2: local CPU smoke, 2026-10-08

## Scope and provenance

This is a **local end-to-end acceptance run** for issue #41, from source commit `773d9a02fad2cb9966c7bcaf8239a4af19283996` (the E2 logic fix). It is not the formal RQ2 campaign. The formal configuration remains 480 events/min for 60 minutes across 30 seeds on the designated GPU/HPC host. The analyzer reports `localComplete=true`, `formalComplete=false`, and `evidenceClass=local-smoke` in [analysis.json](analysis.json).

The host was an Apple M1 Pro MacBook Pro with 8 logical CPUs, 32 GiB RAM, macOS 26.5 arm64, and CPU SP1 v6.4.0. The observed stack used Fabric v2.5.16 with 2 organizations, 2 peers and 3 EtcdRaft orderers, plus Anvil chain 31337; see [preflight.json](preflight.json). GPU/CUDA is `N/A` on this host.

The deterministic local workload used `--local-smoke --accelerated --prover cpu --lambda-events-per-min 16 --duration-min 1 --seeds 1 --run-id 20261008 --queue-capacity 1 --queue-wait-timeout-sec 1`. The generated schedule contained **9 canonical EPCIS events in 1 lot**, with schedule SHA-256 `4203d1bed8c781fd4bf6242dc99468d7763ca24306863132795156e46dc2f6d6`. The manifest was prepared from the clean fix commit before network activity. The runtime consumed that exact manifest and the observed preflight record.

## Measured result

| Metric | Local result |
| --- | ---: |
| Scheduled / offered / Fabric committed / Fabric read back / proved / anchored events | 9 / 9 / 9 / 9 / 9 / 9 |
| Scheduled / anchored lots; failed or saturated lots | 1 / 1; 0 |
| Last Fabric acknowledgement to mined Anvil receipt | **1,614,431 ms** (26.91 min) |
| SP1 policy proof and local verification | 1,614,099 ms |
| Anvil anchor request and receipt | 296 ms |
| Proof size; ABI public values | 356 B; 352 B |
| Anvil gas used | 322,383 |
| Whole runner wall time; process-tree peak RSS | 1,679.32 s; 16.02 GiB |

The median, P95 and P99 in [analysis.json](analysis.json) all equal 1,614,431 ms **because there is only one successful lot**. They are a single observation, not estimates of a latency distribution. The 95% seed confidence intervals are null. Accelerated pacing and one seed make these numbers unsuitable for the formal paper RQ2 table. The clock starts at the last Fabric commit acknowledgement and includes readback, SP1 proving, local verification, Anvil submission and mining. Setup and stack bootstrap are outside that latency.

The runner recorded `fabricReadback=true` and used the fetched Fabric bytes as the proof witness. An independent later GET verified the exact 86-byte canonical value and SHA-256 digest for all event IDs `20261008001000000` through `20261008001000008`; see [ledger-check.json](ledger-check.json). The public [receipt.json](receipt.json) reports a real `sp1-groth16` receipt with status 1, transaction `0x0e724a313d12836d51bebcdaeeed059c656db214f071983087614a5142a90a14`, an `EpochAnchored` log, and nullifier root change from `0x1a8917f23b86dd549f86854cfb01e45ea09dab7a725e9ec64df653c16a3bf2be` to `0x6d3301b2bdb704c7700da162a8746a612746ef8ce7515b20983dbff842722ca2`. The repository's receipt validator passed. A separate RPC query confirmed the mined transaction, gas, log, block hash and current on-chain root; see [chain-check.json](chain-check.json).

## Run history and limits

The first attempt stopped before any event commit: the first Fabric endorsement exceeded the gateway's 30 s deadline while chaincode started cold. Its terminal record reported 0 committed events. A retry on the warmed stack committed events but the local watchdog stopped during proving when swap growth exceeded the initial 30 GiB limit. Neither attempt contributed latency to the result; their budgets are retained in [budgets/](budgets/). The successful run used a freshly rebuilt ledger. A single synthetic event with ID `1` was committed and read back as a **chaincode warmup outside the measured workload**; its first cold attempt timed out, its second attempt succeeded. The measured 9 events are distinct and accounted for separately. The successful watchdog allowance was 2 h wall, 28 GiB process-tree RSS and 64 GiB swap growth, with `RAYON_NUM_THREADS=2`; it exited 0 without reaching a limit.

The private raw JSONL, trusted policy manifest and full logs remain outside Git under the task's allocation output directory. [provenance.json](provenance.json) contains their SHA-256 digests and the release binary hash. No raw proof, witness or signing material is committed here. The public files are sufficient to audit the local accounting and mined receipt, while independent replay requires the private artifacts and a fresh local stack.
