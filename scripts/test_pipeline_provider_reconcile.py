import subprocess
import sys
import os
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from pipeline.store import Store
from pipeline.isolation import ensure_github_provider


class ProviderReconcileTest(unittest.TestCase):
    def test_unchanged_credentials_do_not_change_provider_generation_but_rotation_does(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);self.addCleanup(store.close);store.set_setting('gateway','private-gateway')
            calls=[]
            def api(args,**kwargs):
                calls.append(args)
                if args[3:5]==['provider','create']:return subprocess.CompletedProcess(args,1,'','provider already exists')
                return subprocess.CompletedProcess(args,0,'existing','')
            with patch('pipeline.fullsend.forge_token',return_value='SECRET_ONE'),patch('pipeline.isolation.subprocess.run',side_effect=api):
                ensure_github_provider(store)
                ensure_github_provider(store)
                store.set_setting('dispatcher_provider_at',0)
                ensure_github_provider(store)
            self.assertEqual(sum(args[3:5]==['provider','update'] for args in calls),1)
            with patch('pipeline.fullsend.forge_token',return_value='SECRET_TWO'),patch('pipeline.isolation.subprocess.run',side_effect=api):
                ensure_github_provider(store)
                ensure_github_provider(store)
            self.assertEqual(sum(args[3:5]==['provider','update'] for args in calls),2)
            self.assertNotIn('SECRET_ONE',str(calls));self.assertNotIn('SECRET_TWO',str(calls))


    def test_concurrent_workers_create_the_shared_provider_once(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);store=Store(root/'state');self.addCleanup(store.close);store.set_setting('gateway','private-gateway')
            backend=root/'cli';calls=root/'calls';created=root/'created'
            backend.write_text('#!'+sys.executable+'\nimport sys,json\nfrom pathlib import Path\n'
                +f'p=Path({str(calls)!r});a=sys.argv[1:];p.open("a").write(json.dumps(a)+"\\n")\n'
                +f'created=Path({str(created)!r})\n'
                +'if a[2:4]==["provider","get"] and not created.exists(): print("not found");sys.exit(1)\n'
                +'if a[2:4]==["provider","create"]:\n'
                +' if created.exists(): print("provider already exists");sys.exit(1)\n'
                +' created.touch()\n')
            backend.chmod(0o700)
            code='from pipeline.store import Store;from pipeline.isolation import ensure_github_provider;import sys;s=Store(sys.argv[1]);ensure_github_provider(s)'
            env={**os.environ,'GH_TOKEN':'SECRET_FIXTURE','OPENSHELL':str(backend),'PYTHONPATH':str(Path(__file__).parent.resolve())}
            children=[subprocess.Popen([sys.executable,'-c',code,str(root/'state')],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for _ in range(4)]
            for child in children:
                out,err=child.communicate(timeout=15);self.assertEqual(child.returncode,0,err)
            rows=[json.loads(line) for line in calls.read_text().splitlines()]
            self.assertEqual(sum(a[2:4]==['provider','create'] for a in rows),1)
            self.assertFalse(any(a[2:4]==['provider','update'] for a in rows))
            self.assertNotIn('SECRET_FIXTURE',calls.read_text())
