#!/usr/bin/env python3
"""Plan or execute E3 paired cells, with a fresh allocation-owned Fabric ledger per mode."""
import argparse, json, os, shlex, socket, subprocess
from pathlib import Path
from e3_plan import rows
REPO=Path(__file__).resolve().parents[1]

def commands(args,revision):
    result=[]
    for cell in rows():
        root=args.runtime_root.resolve()/f'seed-{cell["seed"]}-{cell["mode"]}'
        workspace=root/'workspace'
        command=['python3',str(workspace/'scripts/e3_run.py'),'--binary',str(args.binary.resolve()),'--gate',str(args.gate.resolve()),
                 '--policy-manifest',str(args.policy_manifest.resolve()),'--mode',cell['mode'],'--seed',str(cell['seed']),
                 '--prover',args.prover,'--wall-time-s',str(args.wall_time_s),'--out',str(args.out_dir.resolve()/f'{cell["mode"]}-{cell["seed"]}.json'),
                 '--git-commit',revision,'--isolation-manifest',str(root/'isolation.json')]
        result.append({'cell':cell,'root':str(root),'workspace':str(workspace),'run_argv':command,
                       'stack_up_argv':['bash',str(workspace/'scripts/e2_stack.sh'),'up'],
                       'stack_down_argv':['bash',str(workspace/'scripts/e2_stack.sh'),'down']})
    return result

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ('runtime-root','binary','gate','policy-manifest','out-dir'): p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--prover',choices=('cpu','cuda'),required=True);p.add_argument('--wall-time-s',type=int,default=7200)
    p.add_argument('--execute',action='store_true');a=p.parse_args()
    revision=os.environ.get('SOURCE_REVISION') or subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip()
    for path in (a.runtime_root.resolve(),a.out_dir.resolve()):
        if path.is_relative_to(REPO):p.error('private runtime/raw output must be outside Git')
    plan=commands(a,revision)
    if not a.execute:
        print(json.dumps({'operation':'dry-run-campaign-plan','cells':plan,'submitted':False},indent=2));return
    if not os.environ.get('SLURM_JOB_ID') or not os.environ.get('EUDR_RUN_ROOT'):p.error('execution requires private Slurm allocation workspace')
    if not a.runtime_root.resolve().is_relative_to(Path(os.environ['EUDR_RUN_ROOT']).resolve()):p.error('runtime must belong to this allocation')
    a.out_dir.mkdir(parents=True,exist_ok=True)
    for row in plan:
        root=Path(row['root']);workspace=Path(row['workspace'])
        root.mkdir(parents=True,exist_ok=False);workspace.mkdir()
        # Clone source and helpers only; private artifacts remain in this cell's runtime/output.
        if (REPO/'.git').exists():
            archive=subprocess.Popen(['git','archive',revision],cwd=REPO,stdout=subprocess.PIPE)
            subprocess.run(['tar','-x','-C',str(workspace)],stdin=archive.stdout,check=True);archive.stdout.close()
            if archive.wait():raise RuntimeError('source archive failed')
        else:
            import shutil
            shutil.copytree(REPO,workspace,dirs_exist_ok=True,ignore=shutil.ignore_patterns('target','runtime','node_modules','.superpowers','.git'))
        subprocess.run(['npm','ci','--ignore-scripts'],cwd=workspace/'contracts',check=True)
        evidence={'protocolVersion':2,'sourceRevision':revision,'computeHost':socket.gethostname(),'freshLedger':True,
                  'runtimePath':str(root),'seed':row['cell']['seed'],'mode':row['cell']['mode'],
                  'namespace':row['cell']['ledger_namespace'],'order':row['cell']['order']}
        (root/'isolation.json').write_text(json.dumps(evidence,indent=2)+'\n')
        env=dict(os.environ,EUDR_RUN_ROOT=str(root),POLICY_MANIFEST=str(a.policy_manifest.resolve()),SOURCE_REVISION=revision)
        subprocess.run(row['stack_up_argv'],env=env,check=True)
        collected=False
        try:
            subprocess.run(row['run_argv'],env=env,check=True)
            # The immutable raw cell is the collection boundary, before deleting ephemeral ledger state.
            record=json.loads(Path(row['run_argv'][row['run_argv'].index('--out')+1]).read_text())
            if record.get('seed')!=row['cell']['seed'] or record.get('mode')!=row['cell']['mode'] or record.get('git_commit')!=revision:raise RuntimeError('collected cell identity mismatch')
            (root/'collected.json').write_text(json.dumps({'rawCollected':True,'status':record.get('measurement',{}).get('status'),'receiptFilesRetained':True})+'\n')
            collected=True
        finally:
            if collected:
                subprocess.run(row['stack_down_argv'],env=env,check=True)
            else:
                print('Uncollected topology retained for recovery: '+str(root))

if __name__=='__main__':main()
