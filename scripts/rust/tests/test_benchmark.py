import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("benchmark", Path(__file__).resolve().parents[1] / "benchmark.py")
benchmark = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(benchmark)


class BenchmarkTests(unittest.TestCase):
    def test_stat_handles_spaces_and_parentheses_in_process_name(self):
        # State is field 3, PPID field 4, start time field 22.
        stat = "123 (name (with) spaces) S 42 " + "0 " * 17 + "9876 0"
        self.assertEqual(benchmark.parse_stat(stat), (42, 9876))

    def test_descendants_count_shared_tree_once_and_exclude_unrelated(self):
        tree = {10: (1, 100), 11: (10, 101), 12: (11, 102), 13: (1, 103)}
        self.assertEqual(benchmark.descendants(tree, 10), [10, 11, 12])

    def test_pss_uses_pss_not_rss_and_sums_smaps_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "10").mkdir()
            (root / "10" / "smaps").write_text("Rss: 999 kB\nPss: 3 kB\nPss_Dirty: 2 kB\nPss: 4 kB\n")
            self.assertEqual(benchmark.pss_kib(10, root), 7)

    def test_empty_or_unmapped_process_is_not_reported_as_measured_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "10").mkdir()
            (root / "10" / "smaps_rollup").write_text("")
            with self.assertRaises(ValueError):
                benchmark.pss_kib(10, root)

    @unittest.skipUnless(Path("/proc/self/smaps_rollup").exists(), "Linux /proc required")
    def test_real_process_child_memory_is_included_and_root_identity_checked(self):
        code = "import subprocess, sys; p=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(15)']); print(p.pid, flush=True); p.wait()"
        process = subprocess.Popen([sys.executable, "-c", code], stdout=subprocess.PIPE, text=True)
        child = None
        try:
            child = int(process.stdout.readline())
            snapshot = benchmark.process_snapshot()
            self.assertIn(child, benchmark.descendants(snapshot, process.pid))
            start = snapshot[process.pid][1]
            total, count, measured = benchmark.sample_tree(process.pid, start)
            self.assertGreater(total, 0)
            self.assertGreaterEqual(count, 2)
            self.assertEqual(count, measured)
            self.assertIsNone(benchmark.sample_tree(process.pid, start + 1))
        finally:
            if child is not None:
                import os
                import signal
                try:
                    os.kill(child, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            process.terminate()
            process.wait(timeout=5)
            process.stdout.close()


if __name__ == "__main__":
    unittest.main()
