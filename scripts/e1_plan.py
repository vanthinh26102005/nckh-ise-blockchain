#!/usr/bin/env python3
"""Print an E1 HPC campaign plan; never runs proofs or creates result files."""
import argparse
import json
from pathlib import Path


def campaign(output_root):
    root = Path(output_root).expanduser().resolve()
    repo = Path(__file__).resolve().parent.parent
    if root == repo or repo in root.parents:
        raise ValueError('raw campaign output must be outside the Git checkout')
    cells = []
    for depth in [6, 16]:
        for mode, seeds, circuits in [('base', 30, 'all'), ('wrapper', 10, 'wrapper')]:
            directory = root / mode / f'depth{depth}'
            binary = ['cargo', 'run', '--release', '-p', 'e1-bench']
            if mode == 'wrapper':
                binary += ['--features', 'recursion']
            sizes = [8, 64] if mode == 'wrapper' else [8, 16, 32, 64]
            sizes_arg = ','.join(map(str, sizes))
            binary += ['--', '--out', str(directory / 'raw.csv'), '--events', sizes_arg, '--seeds', str(seeds), '--circuits', circuits, '--profile', 'coffee-default', '--credential-depth', str(depth), '--jobs', '1', '--strict-output']
            identifiers = 'wrapper_recursive_plonky3' if mode == 'wrapper' else 'c1_polygon_outside,c2_poseidon2_merkle,c3_threshold_time,c4_poseidon2_actor_authorization,c5_poseidon2_sparse_nullifier_depth32'
            analyze = ['python3', 'scripts/e1_analyze.py', '--raw', str(directory / 'raw.csv'), '--out-dir', str(directory), '--strict', '--expected-circuits', identifiers, '--expected-events', sizes_arg, '--expected-depths', str(depth), '--expected-seeds', str(seeds), '--command', 'E1 HPC planned campaign']
            cells.append({'mode': mode, 'credential_depth': depth, 'events': sizes, 'seeds': seeds, 'run_argv': binary, 'analyze_argv': analyze})
    return {'status': 'planned; no measurements', 'scope': 'Path B circuit-cost research; SP1 Path A CPU/GPU campaign is separate', 'output_root': str(root), 'sp1_campaign': {'runner': 'scripts/e4_preflight.py', 'events_per_leaf': [8, 64], 'provers': ['cpu', 'cuda'], 'repetitions_per_config': 5, 'lots_per_aggregate': 2, 'samples': 20, 'raw_dir': str(root / 'sp1'), 'requires': ['trusted policy-v2 manifest', 'pinned clean source revision', 'private allocation topology'], 'argv_template': ['python3', 'scripts/e4_preflight.py', '--binary', '<built-e3_epoch_gate-with-cuda>', '--raw-dir', str(root / 'sp1'), '--out', str(root / 'sp1-preflight.json'), '--epoch-base', '<preissued-epoch-base>', '--git-commit', '<source-commit>', '--policy-manifest', '<trusted-manifest>']}, 'security': {'fri_log_blowup': 3, 'fri_num_queries': 33, 'fri_query_pow_bits': 16, 'checklist_conjectured_bits': 115, 'certification': False}, 'cells': cells}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-root', required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(campaign(args.output_root), indent=2))
    except ValueError as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
