import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

repo=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('deployment',repo/'scripts/deploy-performance.py')
deployment=importlib.util.module_from_spec(spec);spec.loader.exec_module(deployment)

class DeploymentTests(unittest.TestCase):
    def setUp(self):
        base=repo/'target/deployment-tests';base.mkdir(parents=True,exist_ok=True)
        self.temp=tempfile.TemporaryDirectory(dir=base)
        self.root=Path(self.temp.name).resolve();assert self.root.is_relative_to(base.resolve())
        self.install=self.root/'install';self.install.mkdir()
        self.plugin=self.root/'plugin';self.plugin.mkdir()
    def tearDown(self):self.temp.cleanup()
    def journal(self,current,installing=True):
        target=self.install/'code.js';target.write_bytes(current)
        backup=self.install/'repair-checks/backup';backup.mkdir(parents=True)
        (backup/'before.bin').write_bytes(b'before')
        path=backup/'manifest.json'
        deployment.save_manifest(path,{'tag':'test','status':'installing','files':[{'path':str(target),
          'beforeSha256':deployment.digest(b'before'),'afterSha256':deployment.digest(b'after'),
          'backup':'before.bin','installed':not installing,'installing':installing}]})
        return path,target
    def test_journal_recovers_interruption_before_mutation(self):
        manifest,target=self.journal(b'before');deployment.restore(manifest,self.install,self.plugin)
        self.assertEqual(target.read_bytes(),b'before');self.assertEqual(deployment.read_json(manifest)['status'],'rolled-back')
    def test_journal_recovers_interruption_after_mutation(self):
        manifest,target=self.journal(b'after');deployment.restore(manifest,self.install,self.plugin)
        self.assertEqual(target.read_bytes(),b'before')
    def test_rollback_refuses_foreign_edits(self):
        manifest,target=self.journal(b'foreign')
        with self.assertRaisesRegex(RuntimeError,'Concurrent edit'):deployment.restore(manifest,self.install,self.plugin)
        self.assertEqual(target.read_bytes(),b'foreign')
    def test_unknown_roots_and_tampered_backups_refuse(self):
        with self.assertRaisesRegex(RuntimeError,'outside'):deployment.allowed(self.root/'outside',self.install,self.plugin)
        manifest,target=self.journal(b'after');(manifest.parent/'before.bin').write_bytes(b'tampered')
        with self.assertRaisesRegex(RuntimeError,'hash mismatch'):deployment.restore(manifest,self.install,self.plugin)
        self.assertEqual(target.read_bytes(),b'after')
    def test_failed_image_replacement_restores_original_name(self):
        target=self.install/'app.exe';target.write_bytes(b'old')
        with patch.object(deployment.os,'replace',side_effect=PermissionError('locked')):
            with self.assertRaises(PermissionError):deployment.atomic(target,b'new','fault')
        self.assertEqual(target.read_bytes(),b'old')
    def test_hard_stop_after_install_rename_is_recoverable(self):
        manifest,target=self.journal(b'before');state=deployment.read_json(manifest)
        sibling=target.with_name(target.name+'.before-native-test')
        state['files'][0]['renameIntent']={'path':str(sibling),'sha256':deployment.digest(b'before')}
        deployment.save_manifest(manifest,state);target.rename(sibling)
        deployment.restore(manifest,self.install,self.plugin)
        self.assertEqual(target.read_bytes(),b'before')
    def test_hard_stop_during_rollback_rename_is_recoverable(self):
        manifest,target=self.journal(b'after',installing=False);state=deployment.read_json(manifest)
        sibling=target.with_name(target.name+'.before-native-rollback-test')
        state['files'][0]['rollbackRenameIntent']={'path':str(sibling),'sha256':deployment.digest(b'after')}
        deployment.save_manifest(manifest,state);target.rename(sibling)
        deployment.restore(manifest,self.install,self.plugin)
        self.assertEqual(target.read_bytes(),b'before')
    def test_unowned_missing_target_or_tampered_sibling_refuses(self):
        manifest,target=self.journal(b'after');target.unlink()
        with self.assertRaisesRegex(RuntimeError,'Unowned missing'):deployment.restore(manifest,self.install,self.plugin)
        state=deployment.read_json(manifest);sibling=target.with_name(target.name+'.before-native-test');sibling.write_bytes(b'wrong')
        state['files'][0]['renameIntent']={'path':str(sibling),'sha256':deployment.digest(b'before')};deployment.save_manifest(manifest,state)
        with self.assertRaisesRegex(RuntimeError,'hash mismatch'):deployment.restore(manifest,self.install,self.plugin)
    @unittest.skipUnless(os.name=='nt','Windows loaded-image semantics')
    def test_running_image_can_be_replaced_without_stopping_it(self):
        target=self.install/'helper.exe';original=(repo/'native/dsh-native-helper.exe').read_bytes();target.write_bytes(original)
        child=subprocess.Popen([str(target),'--cache',str(self.root/'cache')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            def hello(number):
                child.stdin.write((json.dumps({'id':number,'version':1,'op':'hello'})+'\n').encode());child.stdin.flush()
                return json.loads(child.stdout.readline())
            self.assertTrue(hello(1)['ok']);self.assertIsNone(child.poll())
            deployment.atomic(target,original+b'\0','running')
            self.assertEqual(target.read_bytes(),original+b'\0');self.assertIsNone(child.poll());self.assertTrue(hello(2)['ok'])
            child.stdin.close();self.assertEqual(child.wait(timeout=5),0)
        finally:
            if child.poll() is None:child.kill();child.wait()
            child.stdout.close();child.stderr.close()

if __name__=='__main__':unittest.main()
