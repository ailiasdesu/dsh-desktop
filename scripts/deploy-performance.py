"""Transactional local installation. No process killing or user-data rewrites.

Default is a read-only plan. --apply installs; --rollback restores an owned
manifest. All overwritten bytes are backed up, and concurrent edits refuse.
"""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

REPO=Path(__file__).resolve().parents[1]
def digest(data: bytes)->str:return hashlib.sha256(data).hexdigest()
def read_json(path: Path):return json.loads(path.read_text(encoding='utf-8-sig'))
def save_manifest(path:Path,value):
    staged=path.with_suffix('.json.new')
    staged.write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding='utf-8')
    os.replace(staged,path)
def atomic(path:Path,data:bytes,tag:str,record_rename=None)->Path|None:
    path.parent.mkdir(parents=True,exist_ok=True)
    staged=path.with_name(path.name+'.native-stage-'+tag)
    staged.write_bytes(data)
    moved=None
    try:
        try:os.replace(staged,path)
        except PermissionError:
            # Windows can permit a loaded image to be renamed while denying
            # overwrite. Keep that image intact; never stop its process.
            moved=path.with_name(path.name+'.before-native-'+tag)
            if moved.exists():raise RuntimeError('Backup sibling already exists')
            if record_rename:record_rename(moved)
            os.rename(path,moved)
            try:os.replace(staged,path)
            except BaseException:
                os.rename(moved,path);moved=None;raise
        return moved
    finally:
        if staged.exists():staged.unlink()

def allowed(path:Path,install:Path,plugin:Path)->Path:
    path=path.resolve()
    if not any(path.is_relative_to(root.resolve()) for root in [install,plugin]):
        raise RuntimeError(f'Target outside deployment roots: {path}')
    return path

def plan(install:Path,plugin:Path):
    if read_json(install/'kernel/package.json')['version']!='0.1.2-rc.1':
        raise RuntimeError('Installed kernel is not the verified 0.1.2-rc.1 baseline')
    payloads=[]
    for item in read_json(REPO/'native/manifest.json')['files']:
        source=REPO/'native'/item['name'];data=source.read_bytes()
        if digest(data)!=item['sha256']:raise RuntimeError(f'Native artifact changed: {source}')
        payloads.append((install/'native'/item['name'],data))
    payloads.append((install/'native/manifest.json',(REPO/'native/manifest.json').read_bytes()))
    for source in sorted((REPO/'desktop/native').glob('*')):
        if source.suffix in ['.js','.mjs']:payloads.append((install/'desktop/native'/source.name,source.read_bytes()))
    advisor=REPO/'plugins/memory-advisor-performance'
    changes=read_json(advisor/'deploy-manifest.json')
    if read_json(plugin/'package.json')['version']!=changes['packageVersion']:
        raise RuntimeError('Advisor package version changed; revalidate before deployment')
    replacements={}
    for item in reversed(changes['files']):
        target=allowed(plugin/item['path'],install,plugin)
        old=target.read_bytes() if target.exists() else None
        old_hash=digest(old) if old is not None else None
        if old_hash not in [item['expectedOriginalSha256'],item['sha256']]:
            raise RuntimeError(f'Advisor source changed: {target}')
        data=(advisor/item['path']).read_bytes()
        if digest(data)!=item['sha256']:raise RuntimeError(f'Advisor artifact changed: {item["path"]}')
        payloads.append((target,data));replacements[target]=item['sha256']
    compat=install/'repair-checks/compat-changes.json'
    if compat.exists():
        rows=read_json(compat)
        source_config=(REPO/'src-tauri/tauri.conf.json').resolve()
        for row in rows:
            target=Path(row['path']).resolve()
            if target in replacements:row['patchedSha256']=replacements[target]
            elif target==source_config:row['patchedSha256']=digest(source_config.read_bytes())
        payloads.append((compat,json.dumps(rows,ensure_ascii=False,indent=2).encode()))
    # Activation is last: the old running image may continue until normal exit.
    executable=REPO/'src-tauri/target/release/dsh-desktop.exe'
    if not executable.is_file():raise RuntimeError('Build the desktop executable before deployment')
    expected=read_json(REPO/'src-tauri/tauri.conf.json')['version']
    subprocess.run(['powershell','-NoProfile','-File',str(REPO/'scripts/verify-desktop-binary.ps1'),
                    '-Path',str(executable),'-ExpectedVersion',expected],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                    creationflags=subprocess.CREATE_NO_WINDOW)
    payloads.append((install/'dsh-desktop.exe',executable.read_bytes()))
    result=[]
    for target,data in payloads:
        target=allowed(target,install,plugin)
        before=target.read_bytes() if target.exists() else None
        result.append({'path':str(target),'before':before,'data':data,
            'beforeSha256':digest(before) if before is not None else None,'afterSha256':digest(data)})
    return result

def restore(manifest_path:Path,install:Path,plugin:Path):
    manifest=read_json(manifest_path);base=manifest_path.parent.resolve()
    if not base.is_relative_to((install/'repair-checks').resolve()):raise RuntimeError('Unowned rollback directory')
    for row in reversed(manifest['files']):
        if not (row.get('installed') or row.get('installing')):continue
        target=allowed(Path(row['path']),install,plugin)
        current=digest(target.read_bytes()) if target.exists() else None
        if current is None and row['beforeSha256'] is not None:
            # Recover only a journal-owned, verified intermediate image move.
            recovered=False
            for key,tag,expected in [('rollbackRenameIntent','rollback-'+manifest['tag'],row['afterSha256']),('renameIntent',manifest['tag'],row['beforeSha256'])]:
                intent=row.get(key)
                if not intent:continue
                sibling=Path(intent['path'])
                named=target.with_name(target.name+'.before-native-'+tag)
                if sibling!=named or sibling.is_symlink() or (hasattr(sibling,'is_junction') and sibling.is_junction()):raise RuntimeError('Unowned retained image')
                sibling=allowed(sibling,install,plugin)
                if not sibling.exists():continue
                if intent['sha256']!=expected or digest(sibling.read_bytes())!=expected:raise RuntimeError('Retained image hash mismatch')
                if key=='renameIntent':os.replace(sibling,target)
                else:
                    backup=(base/row['backup']).resolve()
                    if not backup.is_relative_to(base):raise RuntimeError('Unowned backup path')
                    data=backup.read_bytes()
                    if digest(data)!=row['beforeSha256']:raise RuntimeError('Backup hash mismatch')
                    atomic(target,data,'rollback-'+manifest['tag'])
                current=digest(target.read_bytes());recovered=True;break
            if not recovered:raise RuntimeError(f'Unowned missing target prevents rollback: {target}')
        if current==row['beforeSha256']:
            row['installed']=False;row['installing']=False;save_manifest(manifest_path,manifest);continue
        if current!=row['afterSha256']:raise RuntimeError(f'Concurrent edit prevents rollback: {target}')
        if row['beforeSha256'] is None:target.unlink()
        else:
            backup=(base/row['backup']).resolve()
            if not backup.is_relative_to(base):raise RuntimeError('Unowned backup path')
            data=backup.read_bytes()
            if digest(data)!=row['beforeSha256']:raise RuntimeError('Backup hash mismatch')
            def record_rollback(sibling):
                row['rollbackRenameIntent']={'path':str(sibling),'sha256':row['afterSha256']}
                save_manifest(manifest_path,manifest)
            atomic(target,data,'rollback-'+manifest['tag'],record_rollback)
        row['installed']=False
        row['installing']=False
        save_manifest(manifest_path,manifest)
    manifest['status']='rolled-back'
    save_manifest(manifest_path,manifest)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--install',type=Path,default=Path(os.environ.get('LOCALAPPDATA','C:/Users/34021/AppData/Local'))/'DSH Desktop')
    parser.add_argument('--plugin',type=Path,default=Path.home()/'.dsh/profiles/web/node_modules/dsh-memory-evolve')
    parser.add_argument('--apply',action='store_true')
    parser.add_argument('--rollback',type=Path)
    args=parser.parse_args();install=args.install.resolve();plugin=args.plugin.resolve()
    if args.rollback:
        restore(args.rollback.resolve(),install,plugin);print('Rollback verified');return
    rows=plan(install,plugin)
    if not args.apply:
        print(json.dumps({'files':len(rows),'changed':sum(r['beforeSha256']!=r['afterSha256'] for r in rows),
            'bytes':sum(len(r['data']) for r in rows),'targets':[r['path'] for r in rows]},ensure_ascii=True,indent=2));return
    tag=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    backup=install/'repair-checks'/('native-performance-backup-'+tag);backup.mkdir(parents=True)
    manifest={'tag':tag,'status':'installing','files':[]}
    manifest_path=backup/'manifest.json'
    for index,row in enumerate(rows):
        backup_name=f'{index:03d}.bin'
        if row['before'] is not None:(backup/backup_name).write_bytes(row['before'])
        manifest['files'].append({k:row[k] for k in ['path','beforeSha256','afterSha256']}|{'backup':backup_name,'installed':False})
    save_manifest(manifest_path,manifest)
    try:
        for row,record in zip(rows,manifest['files']):
            target=Path(row['path'])
            current=digest(target.read_bytes()) if target.exists() else None
            if current!=row['beforeSha256']:raise RuntimeError(f'Concurrent edit: {target}')
            if current!=row['afterSha256']:
                record['installing']=True;save_manifest(manifest_path,manifest)
                def record_move(sibling):
                    record['renameIntent']={'path':str(sibling),'sha256':row['beforeSha256']}
                    save_manifest(manifest_path,manifest)
                moved=atomic(target,row['data'],tag,record_move)
                if moved:record['retainedRunningImage']=str(moved)
                record['installed']=True
                record['installing']=False
                save_manifest(manifest_path,manifest)
            if digest(target.read_bytes())!=row['afterSha256']:raise RuntimeError(f'Installed hash mismatch: {target}')
        manifest['status']='applied'
        save_manifest(manifest_path,manifest)
        print(json.dumps({'status':'applied','manifest':str(manifest_path),'files':len(rows)},ensure_ascii=True))
    except BaseException as error:
        try:restore(manifest_path,install,plugin)
        except BaseException as rollback_error:
            raise RuntimeError(f'Install failed: {error}; rollback incomplete: {rollback_error}; manifest: {manifest_path}') from error
        raise

if __name__=='__main__':main()
