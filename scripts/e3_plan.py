#!/usr/bin/env python3
"""Fixed 120-cell E3 schedule; mode order rotates by seed, each cell needs fresh ledger state."""
import argparse, csv
from e4_preflight import mode_order
MODES=('fabric-only','hash-on-chain','vecro-adapted','proposed')
def rows():
    for seed in range(30):
        order=mode_order(seed)
        for position,mode in enumerate(order):
            yield dict(seed=seed,mode=mode,order=position,np=125,shipment=64,epoch_s=60,epochs=60,
                       ledger_namespace=f'e3-{seed}-{mode}',reset_required=True)
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',required=True);args=parser.parse_args()
    data=list(rows())
    with open(args.out,'x',newline='') as stream:
        writer=csv.DictWriter(stream,fieldnames=list(data[0]));writer.writeheader();writer.writerows(data)
