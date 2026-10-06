"""Offline checks: no Slurm submission, Docker stack or proof generation."""
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import hpc_jobs


class Handoff(unittest.TestCase):
    def config(self):
        return dict(partition="verified", account=None, gres="gpu:1",
                    time="01:00:00", container_runtime="docker", compute_host="gpu-node",
                    cpus=4, memory_gib=64, prover="cuda", concurrency=1,
                    anvil_image="ghcr.io/foundry-rs/foundry:stable", epoch_base=700001,
                    e1_mode="base", credential_depth=16, e3_seed=0, e3_mode="fabric-only",
                    e4_phase="screening", e4_plan_index=0, wall_time_s=7200)

    def test_generate_builtin_gate_without_already_having_its_own_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            script = hpc_jobs.render(self.config(), "gate", tmp, "a" * 40)
            result = subprocess.run(["bash", "-n"], input=script, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_scheduler_budget_cannot_truncate_formal_workload_by_design(self):
        c = self.config()
        for phase in ('e2', 'e3', 'e4'):
            with self.assertRaisesRegex(ValueError, 'time budget'):
                hpc_jobs.render(c, phase, '/tmp/short-hpc-job', 'a'*40)
        c['time'] = '00:99:00'
        with self.assertRaises(ValueError):
            hpc_jobs.render(c, 'gate', '/tmp/short-hpc-job', 'a'*40)

    def test_missing_anvil_image_rejected_before_scheduling_gpu_setup(self):
        c = self.config()
        c['anvil_image'] = None
        with self.assertRaisesRegex(ValueError, 'anvil_image'):
            hpc_jobs.render(c, 'gate', '/tmp/missing-image-job', 'a'*40)

    def test_phase_plan_uses_formal_axes_and_separate_policy_preparation(self):
        from hpc_run import plan
        c = self.config()
        c.update(gate="/external/gate.json", policy_manifest="/external/catalog.json",
                 preflight="/external/preflight.json", pilot_report="/external/pilot.json")
        for phase in ("e2-pilot", "e2", "e3", "e4"):
            value = plan(c, phase, Path("/external/output"), "a" * 40)
            # Every proving pipeline exposes its real runner args without executing it.
            self.assertTrue(value["measure"])
        formal = plan(c, "e2", Path("/external/output"), "a" * 40)
        run = formal["measure"][1]["argv"]
        self.assertEqual(run[run.index("--seeds") + 1], "30")
        self.assertNotIn("--accelerated", run)
        self.assertTrue(formal["prepare"])
        e3 = plan(c, "e3", Path("/external/output"), "a" * 40)["measure"][0]["argv"]
        self.assertEqual(e3[e3.index("--mode") + 1], "fabric-only")
        e4 = plan(c, "e4", Path("/external/output"), "a" * 40)["measure"][0]["argv"]
        self.assertEqual(e4[e4.index("--plan-index") + 1], "0")
        self.assertEqual(e4[e4.index("--executor") + 1], "real-e3")

    def test_preparation_catalog_covers_both_campaigns_and_ab_workloads(self):
        from hpc_run import plan
        c = self.config()
        c["policy_manifest"] = "/external/gate-policy.json"
        value = plan(c, "prepare", Path("/external/output"), "a" * 40)
        prepares = [s["argv"] for s in value["prepare"] if "--prepare-policy-manifest" in s["argv"]]
        self.assertEqual(len(prepares), 380)  # E3 30 + E4 80 + 270.
        ab = [s["argv"] for s in value["prepare"] if "--prepare-only" in s["argv"]]
        self.assertEqual([a[a.index("--events-per-leaf") + 1] for a in ab], ["8", "64"])

    def test_invalid_selection_and_unreviewed_formal_input_rejected(self):
        from hpc_run import plan
        c = self.config()
        c["e3_seed"] = 30
        with self.assertRaises(ValueError):
            plan(c, "e3", Path("/external/output"), "a" * 40)
        with self.assertRaises(ValueError):
            hpc_jobs.render(self.config(), "e4", "/tmp/handoff", "a" * 40)

    def test_cpu_or_unreviewed_pilot_cannot_unlock_formal_cuda_e2(self):
        from hpc_run import validate_inputs
        with tempfile.TemporaryDirectory() as tmp:
            c = self.config()
            gate = Path(tmp) / 'gate.json'
            gate.write_text(json.dumps(dict(protocolVersion=2, gitCommit='a'*40, status='passed',
                verificationKind='sp1-groth16', anvilReceiptStatus=1, registry='0x'+'11'*20,
                tamperedChildRejected=True, tamperedAggregateProofRejected=True,
                tamperedAggregateValuesRejected=True)))
            pilot = Path(tmp) / 'pilot.json'
            record = dict(protocolVersion=2, run=dict(sourceRevision='a'*40, computeHost='gpu-node',
                prover='cpu', mode='accelerated-pilot', seeds=1, durationMin=1, lambdaEventsPerMin=480,
                sourceDirty=False), aggregate=dict(anchoredLots=1))
            pilot.write_text(json.dumps(record))
            c.update(gate=str(gate), pilot_report=str(pilot), pilot_reviewed=True)
            with self.assertRaises(ValueError):
                validate_inputs(c, 'e2', 'a'*40)
            record['run']['prover'] = 'cuda'
            pilot.write_text(json.dumps(record))
            validate_inputs(c, 'e2', 'a'*40)
            c['pilot_reviewed'] = False
            with self.assertRaises(ValueError):
                validate_inputs(c, 'e2', 'a'*40)

    def test_generated_job_keeps_git_provenance_and_runs_in_fresh_workspace(self):
        # Real temporary Git source. External commands only stand in for unavailable HPC services.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            repo = root / "source"
            repo.mkdir()
            (repo / "payload").write_text("committed source")
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "add", "payload"], check=True)
            subprocess.run(["git", "-C", str(repo), "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                            "commit", "-qm", "fixture"], check=True)
            revision = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            c = self.config()
            c.update(account="verified", gres="gpu:1", prover="cpu", compute_host="gpu-node",
                     commands={"e1": [["python3", "-c",
                        "import pathlib,subprocess; pathlib.Path('../output/head').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True)); assert pathlib.Path('payload').read_text()=='committed source'"]]})
            with patch.object(hpc_jobs, "REPO", repo):
                script = hpc_jobs.render(c, "e1", root / "runs", revision)
            fake = root / "bin"
            fake.mkdir()
            for name, content in {"hostname": "echo gpu-node", "docker": "exit 0", "flock": "exit 0"}.items():
                p = fake / name
                p.write_text("#!/bin/sh\n" + content + "\n")
                p.chmod(0o700)
            import os
            env = dict(os.environ, PATH=str(fake) + os.pathsep + os.environ['PATH'], SLURM_JOB_ID="123")
            result = subprocess.run(["bash"], input=script, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / 'runs/123-e1/output/head').read_text().strip(), revision)
            second = subprocess.run(["bash"], input=script, env=env, text=True, capture_output=True)
            self.assertNotEqual(second.returncode, 0)  # No overwrite/retry into old evidence.


if __name__ == "__main__":
    unittest.main()
