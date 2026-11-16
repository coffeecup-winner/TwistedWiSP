import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('runner', Path(__file__).resolve().parents[1] / 'run_tests.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_build_failure_does_not_run_stale_tests(self):
        with patch.object(runner.subprocess, 'run', return_value=subprocess.CompletedProcess([], 42)) as run:
            self.assertEqual(runner.main([]), 42)
            self.assertEqual(run.call_count, 1)

    def test_uses_metadata_target_and_propagates_pytest_status(self):
        results = [subprocess.CompletedProcess([], 0), subprocess.CompletedProcess([], 0, '{"target_directory":"/tmp/custom-target"}'), subprocess.CompletedProcess([], 7)]
        with patch.object(runner.subprocess, 'run', side_effect=results) as run:
            self.assertEqual(runner.main(['--release', '-q']), 7)
            self.assertEqual(run.call_args.kwargs['env']['WISP_LIBRARY_PATH'], '/tmp/custom-target/release/libtwisted_wisp.so')
            self.assertEqual(run.call_args.args[0][-1], '-q')
