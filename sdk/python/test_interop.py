"""Run with RIT_PROTOCOL_BINARY pointing to a built rit-protocol executable."""
import json
import os
import subprocess
import unittest
import tempfile
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
    def test_scoped_reads_real_repository_and_denies_escape(self):
        with tempfile.TemporaryDirectory() as d:
            subprocess.run(['git','init','-q',d],check=True)
            with subprocess.Popen([os.environ['RIT_PROTOCOL_BINARY'],'--repo',d],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True) as p:
                def transport(req):
                    p.stdin.write(json.dumps(req)+'\n'); p.stdin.flush()
                    return json.loads(p.stdout.readline())
                c=Client(transport)
                self.assertEqual(len(c.request('tools/list')['tools']),13)
                for name, field in [('workspace.list','reconciliation'), ('checkpoint.list','content_checkpoints'), ('submission.list','submissions'), ('candidate.list','candidates'), ('workspace.operations','operations'), ('checkpoint.restores','restores'), ('submission.reconcile','submissions')]:
                    item = c.request('tools/call', {'name':name, 'arguments':{}})['structuredContent']
                    self.assertIn(field, item)
                    self.assertFalse(item['authenticated'])
                result=c.request('tools/call',{'name':'repository.inspect','arguments':{}})
                self.assertEqual(result['structuredContent']['inspection']['object_format'],'sha1')
                with self.assertRaises(RuntimeError): c.request('tools/call',{'name':'repository.inspect','arguments':{'path':'..'}})
                p.stdin.close(); self.assertEqual(p.wait(timeout=5),0)
if __name__ == '__main__': unittest.main()
