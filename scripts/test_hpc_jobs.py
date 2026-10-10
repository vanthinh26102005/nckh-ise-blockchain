import unittest
from hpc_jobs import render
class HPCJobs(unittest.TestCase):
    def test_configuration_dry_run_and_isolation(self):
        c=dict(partition='actual-partition',account='actual-account',qos='verified-qos',gres='gpu:1',time='01:00:00',container_runtime='docker',compute_host='allocated-host',memory_gib=64,cpus=4,prover='cpu',commands={'e1':[['echo','test']]})
        script=render(c,'e1','/tmp/eudr-hpc-test','a'*40)
        self.assertIn('flock -n',script);self.assertNotIn('sbatch ',script);self.assertIn('SLURM_JOB_ID',script)
        c['concurrency']=2
        with self.assertRaises(ValueError): render(c,'e1','/tmp/eudr-hpc-test','a'*40)
        c['concurrency']=1;c['partition']='bad\n#SBATCH --extra'
        with self.assertRaises(ValueError): render(c,'e1','/tmp/eudr-hpc-test','a'*40)
    def test_formal_gate_required(self):
        with self.assertRaises(ValueError): render({},'e3','/tmp/x','a'*40)
if __name__=='__main__': unittest.main()
