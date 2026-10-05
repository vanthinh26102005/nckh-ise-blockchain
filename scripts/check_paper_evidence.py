#!/usr/bin/env python3
"""Check the current policy-v2 readiness ledger; never certify formal paper evidence."""
import argparse
import json
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
TASK_IDS = ('T-SEC-01', 'T-SEC-02', 'T-SEC-03', 'T-DM-01', 'T-SD-01', 'T-C1-01',
            'T-E2-01', 'T-E3-01', 'T-E1-01', 'T-E1-02', 'T-E1-03', 'T-E2-02',
            'T-E3-02', 'T-E4-01', 'T-E4-02', 'T-COST-01', 'T-SEP-01', 'T-ABS-01',
            'T-DOC-01', 'T-FIN-01')
STATUSES = {'locally-verified-awaiting-hpc', 'implemented-awaiting-hpc',
            'prototype-verified-measurements-pending', 'future-work',
            'optional-not-run', 'pending-measurements', 'pending-author-information',
            'pending-finalization'}


def validate(data, paper):
    if not isinstance(data, dict) or data.get('protocolVersion') != 2:
        raise ValueError('readiness checklist requires protocolVersion 2')
    if data.get('evidenceClass') != 'readiness-only' or data.get('formalComplete') is not False:
        raise ValueError('readiness checklist cannot claim formal completion')
    if data.get('finaltrue') is not False or data.get('formalMeasuredRuns') != 0:
        raise ValueError('finaltrue and formal measurements remain pending in this readiness snapshot')
    tasks = data.get('tasks')
    if not isinstance(tasks, list) or len(tasks) != 20:
        raise ValueError('exactly 20 approved task IDs are required')
    ids = [task.get('id') for task in tasks if isinstance(task, dict)]
    if len(ids) != 20 or set(ids) != set(TASK_IDS) or len(set(ids)) != 20:
        raise ValueError('task IDs must match the approved 20 tasks without duplicates')
    for task in tasks:
        if task.get('status') not in STATUSES or task.get('formalComplete') is not False:
            raise ValueError(f"{task['id']}: formal completion is unsupported by current evidence")
        if task.get('evidenceClass') not in {'local-code-validation', 'source-reviewed', 'pending', 'deferred'}:
            raise ValueError(f"{task['id']}: formal evidence is not available in this readiness snapshot")
        if not task.get('title') or not task.get('remaining'):
            raise ValueError(f"{task['id']}: title and remaining work are required")
    # Reject completion flags even when hidden inside an added metadata object.
    def inspect(value):
        if isinstance(value, dict):
            for key, child in value.items():
                if key in {'formal', 'formalComplete', 'formalMeasurementsComplete', 'complete', 'completed', 'finaltrue'} and child is not False:
                    raise ValueError(f'{key} cannot be enabled in readiness evidence')
                inspect(child)
        elif isinstance(value, list):
            for child in value:
                inspect(child)
    inspect(data)
    active = re.sub(r'(?<!\\)%[^\n]*', '', paper)
    if re.search(r'\\finaltrue\b', active):
        raise ValueError('paper finaltrue is forbidden while formal evidence or author information is pending')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--checklist', type=Path, default=REPO / 'results/policy-v2/checklist.json')
    parser.add_argument('--paper', type=Path, default=REPO / 'EPCIS_ACIIDS_2026_ThinhTriNhan/main.tex')
    args = parser.parse_args()
    try:
        validate(json.loads(args.checklist.read_text()), args.paper.read_text())
    except (OSError, ValueError, TypeError, KeyError) as error:
        parser.error(str(error))
    print(json.dumps({'status': 'passed', 'taskCount': 20, 'protocolVersion': 2,
                      'evidenceClass': 'readiness-only', 'formalComplete': False, 'finaltrue': False}))


if __name__ == '__main__':
    main()
