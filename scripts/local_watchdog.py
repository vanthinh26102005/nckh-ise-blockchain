#!/usr/bin/env python3
"""Run one local smoke with process-tree RSS, wall time and swap-growth limits."""
import argparse, json, os, signal, subprocess, sys, time


def swap_bytes():
    if sys.platform != 'darwin':
        return 0
    import re
    text = subprocess.check_output(['sysctl', 'vm.swapusage'], text=True, timeout=2)
    match = re.search(r'used = ([\d.]+)([MG])', text)
    return int(float(match[1]) * (1024**2 if match[2] == 'M' else 1024**3)) if match else 0


def run(command, seconds=120, rss_mib=4096, swap_mib=512):
    env = dict(os.environ, CARGO_BUILD_JOBS='1', RAYON_NUM_THREADS='2')
    initial_swap = swap_bytes()
    start = time.monotonic()
    process = subprocess.Popen(command, env=env, start_new_session=True)
    peak = 0
    reason = None
    try:
        while process.poll() is None:
            rows = subprocess.check_output(['ps', '-axo', 'pid=,ppid=,rss='], text=True, timeout=2)
            rows = [tuple(map(int, row.split())) for row in rows.splitlines() if row.strip()]
            pids = {process.pid}
            for _ in range(len(rows)):
                descendants = {pid for pid, ppid, _ in rows if ppid in pids}
                if descendants <= pids: break
                pids |= descendants
            rss = sum(value for pid, _, value in rows if pid in pids) * 1024
            peak = max(peak, rss)
            if time.monotonic() - start > seconds: reason = 'wall_time_limit'
            elif rss > rss_mib * 1024**2: reason = 'rss_limit'
            elif swap_bytes() - initial_swap > swap_mib * 1024**2: reason = 'swap_growth_limit'
            if reason:
                os.killpg(process.pid, signal.SIGTERM)
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired: os.killpg(process.pid, signal.SIGKILL)
                break
            time.sleep(.2)
    except (OSError, subprocess.SubprocessError) as error:
        reason = 'watchdog_monitor_failure: ' + type(error).__name__
    finally:
        # The group is unique to this child; descendants must not escape the smoke budget.
        try: os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError: pass
    code = process.wait()
    return {'validationKind': 'local-resource-budget', 'status': 'passed' if code == 0 and not reason else 'local validation incomplete',
            'reason': reason, 'exitCode': code, 'peakRssBytes': peak, 'elapsedSeconds': time.monotonic()-start}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', required=True)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command: parser.error('command required')
    from pathlib import Path
    report = Path(args.report)
    if report.exists(): parser.error('report already exists')
    result = run(command)
    report.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result))
    raise SystemExit(0 if result['status'] == 'passed' else 2)
