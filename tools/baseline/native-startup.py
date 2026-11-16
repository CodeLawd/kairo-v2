#!/usr/bin/env python3
"""Prompt 107: native counterpart of tools/baseline/startup.mjs.

Launches the given Kairo executable once per trial with a new, empty HOME
(so a fresh profile, no keychain items, the account screen), times spawn to
the app's `kairo-first-paint-ms` line on a monotonic clock, waits 15 s, then
samples `ps` once per second for five seconds over the main process and all
descendants (RSS summed, CPU summed, relative to one logical core), exactly as
baseline.md's idle row. Prints one JSON object.

Usage: native-startup.py KAIRO_BINARY [TRIALS] [ENGINE_BINARY] [BIBLE_DB]
"""
import json, os, shutil, signal, statistics, subprocess, sys, tempfile, threading, time


def tree(root):
    out = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,rss=,%cpu="], capture_output=True, text=True).stdout
    rows = {}
    for line in out.splitlines():
        pid, ppid, rss, cpu = line.split()[:4]
        rows[int(pid)] = (int(ppid), int(rss), float(cpu))
    keep = {root}
    changed = True
    while changed:
        changed = False
        for pid, (ppid, _, _) in rows.items():
            if ppid in keep and pid not in keep:
                keep.add(pid)
                changed = True
    return [(pid, rows[pid][1], rows[pid][2]) for pid in keep if pid in rows]


def trial(binary, engine, bible):
    home = tempfile.mkdtemp(prefix="kairo-107-")
    env = dict(os.environ, HOME=home, KAIRO_REPORT_STARTUP="1", CFFIXED_USER_HOME=home)
    if engine:
        env["KAIRO_ENGINE_BINARY"] = engine
    if bible:
        env["KAIRO_BIBLE_DB"] = bible
    start = time.monotonic()
    p = subprocess.Popen([binary], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    first = {}
    def read():
        for line in p.stderr:
            if line.startswith("kairo-first-paint-ms") and "spawn" not in first:
                first["spawn"] = (time.monotonic() - start) * 1000
                first["internal"] = float(line.split()[1])
    threading.Thread(target=read, daemon=True).start()
    deadline = time.monotonic() + 60
    try:
        while "spawn" not in first:
            if p.poll() is not None:
                return {"failed": f"exited {p.returncode} before first paint"}
            if time.monotonic() > deadline:
                return {"failed": "no first paint within 60 s"}
            time.sleep(0.01)
        time.sleep(15)
        samples = []
        for _ in range(5):
            if p.poll() is not None:
                return {"failed": "main process exited during sampling"}
            rows = tree(p.pid)
            samples.append((sum(r[1] for r in rows), sum(r[2] for r in rows), len(rows)))
            time.sleep(1)
        return {
            "spawn_to_first_paint_ms": round(first["spawn"], 1),
            "main_to_first_paint_ms": first["internal"],
            "idle_rss_kib": statistics.median(s[0] for s in samples),
            "idle_cpu_percent": statistics.median(s[1] for s in samples),
            "processes": max(s[2] for s in samples),
        }
    finally:
        for pid, _, _ in tree(p.pid):
            if pid != p.pid:
                try:
                    os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
        p.terminate()
        try:
            p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()
        shutil.rmtree(home, ignore_errors=True)


def main():
    binary = sys.argv[1]
    trials = int(sys.argv[2]) if len(sys.argv) > 2 else 5
    engine = sys.argv[3] if len(sys.argv) > 3 else None
    bible = sys.argv[4] if len(sys.argv) > 4 else None
    results = [trial(binary, engine, bible) for _ in range(trials)]
    ok = [r for r in results if "failed" not in r]
    summary = {"trials": results}
    for key in ("spawn_to_first_paint_ms", "main_to_first_paint_ms", "idle_rss_kib", "idle_cpu_percent"):
        values = [r[key] for r in ok]
        if values:
            summary[key] = {"median": statistics.median(values), "min": min(values), "max": max(values)}
    print(json.dumps(summary, indent=1))
    if len(ok) != len(results):
        sys.exit(1)


if __name__ == "__main__":
    main()
