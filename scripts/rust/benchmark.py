#!/usr/bin/env python3
"""Sample aggregate Linux process-tree PSS without launching or killing an app."""

import argparse
import csv
from pathlib import Path
import sys
import time


def parse_stat(text):
    # comm can contain spaces and parentheses; fields after its final ')' are fixed.
    fields = text[text.rfind(")") + 2:].split()
    return int(fields[1]), int(fields[19])  # ppid (field 4), starttime (field 22)


def process_snapshot(proc_root=Path("/proc")):
    result = {}
    for entry in proc_root.iterdir():
        if entry.name.isdecimal():
            try:
                result[int(entry.name)] = parse_stat((entry / "stat").read_text())
            except (OSError, ValueError, IndexError):
                pass  # Processes can exit while /proc is read.
    return result


def descendants(snapshot, root_pid):
    children = {}
    for pid, (ppid, _) in snapshot.items():
        children.setdefault(ppid, []).append(pid)
    found = {root_pid}
    pending = [root_pid]
    while pending:
        for pid in children.get(pending.pop(), []):
            if pid not in found:
                found.add(pid)
                pending.append(pid)
    return sorted(found)


def pss_kib(pid, proc_root=Path("/proc")):
    try:
        text = (proc_root / str(pid) / "smaps_rollup").read_text()
    except FileNotFoundError:
        text = (proc_root / str(pid) / "smaps").read_text()
    values = [int(line.split()[1]) for line in text.splitlines() if line.startswith("Pss:")]
    if not values:
        raise ValueError("PSS counters unavailable; process may be exiting")
    return sum(values)


def sample_tree(root_pid, root_start, proc_root=Path("/proc")):
    snapshot = process_snapshot(proc_root)
    if snapshot.get(root_pid, (None, None))[1] != root_start:
        return None  # Root exited or PID was recycled; never sample a different app.
    pids = descendants(snapshot, root_pid)
    total, measured = 0, 0
    for pid in pids:
        try:
            identity = parse_stat((proc_root / str(pid) / "stat").read_text())
            if identity != snapshot[pid]:
                continue
            value = pss_kib(pid, proc_root)
            # Reject a PID recycled while reading memory counters, too.
            if parse_stat((proc_root / str(pid) / "stat").read_text()) == identity:
                total += value
                measured += 1
        except (OSError, ValueError, IndexError):
            pass
    return total, len(pids), measured


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True, help="application root PID, not its shell or cargo launcher")
    parser.add_argument("--duration", type=float, default=60)
    parser.add_argument("--interval", type=float, default=1)
    args = parser.parse_args()
    if args.pid <= 0 or args.duration <= 0 or args.interval <= 0:
        parser.error("pid, duration, and interval must be positive")
    try:
        root_start = parse_stat(Path(f"/proc/{args.pid}/stat").read_text())[1]
    except (OSError, ValueError, IndexError) as exc:
        parser.error(f"cannot inspect root process: {exc}")
    writer = csv.writer(sys.stdout)
    writer.writerow(["elapsed_s", "pss_kib", "processes", "measured", "complete"])
    start = time.monotonic()
    while True:
        result = sample_tree(args.pid, root_start)
        if result is None:
            print("Root process exited; sampling stopped.", file=sys.stderr)
            break
        pss, count, measured = result
        writer.writerow([f"{time.monotonic() - start:.3f}", pss, count, measured, int(measured == count)])
        sys.stdout.flush()
        remaining = args.duration - (time.monotonic() - start)
        if remaining <= 0:
            break
        time.sleep(min(args.interval, remaining))


if __name__ == "__main__":
    main()
