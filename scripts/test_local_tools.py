import unittest
from e3_plan import rows
from local_watchdog import run
class LocalTools(unittest.TestCase):
    def test_counterbalanced_full_schedule(self):
        data=list(rows());self.assertEqual(len(data),120)
        self.assertEqual(len({(r['seed'],r['mode']) for r in data}),120)
        from e4_preflight import mode_order
        self.assertEqual([r['mode'] for r in data[:4]], list(mode_order(0)))
        self.assertEqual(data[4]['mode'],mode_order(1)[0])
    def test_watchdog_success_and_wall_limit(self):
        import sys
        self.assertEqual(run([sys.executable,'-c','pass'])['status'],'passed')
        result=run([sys.executable,'-c','import time;time.sleep(2)'],seconds=.05)
        self.assertEqual(result['reason'],'wall_time_limit')
if __name__=='__main__':unittest.main()
