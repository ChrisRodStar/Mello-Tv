import os
from pathlib import Path
import signal
import sys
import tempfile
import time
import unittest
from process_owner import run

class Ownership(unittest.TestCase):
    def test_timeout_stops_descendant(self):
        with tempfile.TemporaryDirectory() as root:
            pid_file = Path(root)/"pid"
            code = "import os,time; p=os.fork();\nif p==0:\n open('pid','w').write(str(os.getpid())); time.sleep(60)\nelse: time.sleep(60)"
            with self.assertRaises(TimeoutError):
                run([sys.executable,"-c",code],root,dict(os.environ),timeout=.5)
            pid=int(pid_file.read_text())
            # A terminated orphan may briefly remain a zombie, but must not run.
            result=__import__('subprocess').run(["ps","-o","stat=","-p",str(pid)],capture_output=True,text=True)
            self.assertTrue(not result.stdout.strip() or result.stdout.strip().startswith("Z"))

    def test_nonzero_is_failure(self):
        with self.assertRaises(__import__('subprocess').CalledProcessError):
            run([sys.executable,"-c","raise SystemExit(7)"],"/",dict(os.environ))

if __name__ == '__main__':
    unittest.main()
