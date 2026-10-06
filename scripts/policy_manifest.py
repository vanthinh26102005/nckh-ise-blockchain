#!/usr/bin/env python3
"""Combine prepared public policy snapshots with identical trusted program keys."""
import argparse, json, re
from pathlib import Path
KEYS=('leafProgramVKey','aggregateProgramVKey','leafVKeyDigest','aggregateVKeyDigest')

def merge(records):
    if not records: raise ValueError('at least one prepared snapshot required')
    expected={key:records[0].get(key) for key in KEYS}
    if any(not isinstance(v,str) or not re.fullmatch('0x[0-9a-fA-F]{64}',v) for v in expected.values()): raise ValueError('prepared program keys required')
    pairs=[];workloads=[]
    for record in records:
        if record.get('protocolVersion')!=2 or any(record.get(k,'').lower()!=v.lower() for k,v in expected.items()): raise ValueError('protocol/program keys disagree')
        for workload in record.get('workloads',[]):
            if workload not in workloads:workloads.append(workload)
        for pair in record.get('acceptedPolicies',[record]):
            entry={key:pair.get(key) for key in ('certificateRoot','polygonCommitment')}
            if any(not isinstance(v,str) or not re.fullmatch('0x[0-9a-fA-F]{64}',v) for v in entry.values()): raise ValueError('canonical policy root pair required')
            entry={k:v.lower() for k,v in entry.items()}
            if entry not in pairs:pairs.append(entry)
    return {'protocolVersion':2,**expected,'acceptedPolicies':pairs,'workloads':workloads,'validationKind':'prebuilt-policy-configuration','syntheticFixture':True}

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,required=True);p.add_argument('snapshots',type=Path,nargs='+');a=p.parse_args()
    result=merge([json.loads(path.read_text()) for path in a.snapshots])
    with a.out.open('x') as stream: json.dump(result,stream,indent=2);stream.write('\n')
