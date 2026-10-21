"""Real process-tree regression: socket EOF proves descendant termination."""
import concurrent.futures
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import unittest
from processes import run_command

DESCENDANT = "import os,socket,sys,threading; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.sendall((str(os.getpid())+'\\n').encode()); threading.Event().wait()"
PARENT = "import subprocess,sys,threading; subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); threading.Event().wait()"

class ProcessLifecycle(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix="tk3-process-")
        self.listener=socket.socket(socket.AF_UNIX)
        self.path=str(Path(self.temp.name)/"ready.sock")
        self.listener.bind(self.path);self.listener.listen(1);self.listener.settimeout(10)
        self.connection=None;self.descendant=None
    def tearDown(self):
        if self.descendant is not None:
            try: os.kill(self.descendant,signal.SIGKILL)
            except ProcessLookupError: pass
        if self.connection is not None: self.connection.close()
        self.listener.close();self.temp.cleanup()
    def ready(self):
        self.connection,_=self.listener.accept();self.connection.settimeout(3)
        received=b""
        while not received.endswith(b"\n"): received+=self.connection.recv(100)
        self.descendant=int(received)
    def assert_stopped(self):
        self.connection.settimeout(1)
        try: value=self.connection.recv(1)
        except socket.timeout: self.fail("runner left descendant alive")
        self.assertEqual(value,b"","descendant connection must close")
    def command(self): return [sys.executable,"-c",PARENT,DESCENDANT,self.path]
    def test_timeout_terminates_descendant(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            future=pool.submit(run_command,self.command(),cwd=Path.cwd(),env=os.environ.copy(),timeout=5)
            self.ready()
            with self.assertRaises(subprocess.TimeoutExpired): future.result(timeout=8)
            self.assert_stopped()
    def test_interrupt_terminates_descendant(self):
        controller="from processes import run_command; import os,sys; run_command(sys.argv[1:],cwd=os.getcwd(),env=os.environ.copy(),timeout=30)"
        p=subprocess.Popen([sys.executable,"-c",controller,*self.command()],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True)
        try:
            self.ready();p.send_signal(signal.SIGINT)
            self.assertNotEqual(p.wait(timeout=5),0)
            self.assert_stopped()
        finally:
            try:os.killpg(p.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            p.wait()

if __name__=="__main__": unittest.main()
