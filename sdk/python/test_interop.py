"""Run with RIT_PROTOCOL_BINARY pointing to a built rit-protocol executable."""
import json
import os
import subprocess
import unittest
from rit_client import Client
class Interop(unittest.TestCase):
    def test_discovery_and_denial(self):
        with subprocess.Popen([os.environ['RIT_PROTOCOL_BINARY']], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) as p:
            def transport(req):
                p.stdin.write(json.dumps(req) + '\n')
                p.stdin.flush()
                return json.loads(p.stdout.readline())
            c=Client(transport)
            self.assertEqual(c.initialize()['protocolVersion'], '2025-03-26')
            self.assertEqual(c.request('tools/list')['tools'], [])
            with self.assertRaises(RuntimeError):
                c.request('tools/call', {'name':'publish'})
            p.stdin.close()
            self.assertEqual(p.wait(timeout=5), 0)
if __name__ == '__main__': unittest.main()
