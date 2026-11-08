#!/usr/bin/env python3
"""Reproducible V5 bounded development workflow; reports blocked steps, not acceptance."""
import json, os, pathlib, subprocess, sys, tempfile

binary = os.environ.get("RIT_BINARY", "target/debug/rit")
binary = str(pathlib.Path(binary).resolve())
results = []
def record(n, description, status, detail):
    results.append(dict(step=n, description=description, status=status, detail=detail))
    print(json.dumps(results[-1]), flush=True)
def cmd(args, *, cwd=None, ok=True, env=None):
    p = subprocess.run(args, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if ok and p.returncode != 0:
        raise RuntimeError(f"{args}: exit={p.returncode}: {p.stderr}")
    return p
def rit(*args, ok=True):
    p = cmd([binary, *map(str,args)], ok=ok)
    return json.loads(p.stdout) if p.returncode == 0 and p.stdout.strip() else p

def git(directory,*args):
    return cmd(['git',*args],cwd=directory,env=dict(os.environ,GIT_AUTHOR_NAME='Fixture',GIT_AUTHOR_EMAIL='fixture@example.invalid',GIT_COMMITTER_NAME='Fixture',GIT_COMMITTER_EMAIL='fixture@example.invalid',GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null')).stdout.strip()

with tempfile.TemporaryDirectory(prefix='rit-v5-workflow-') as td:
    root=pathlib.Path(td);os.chmod(root,0o700);repo=root/'repo';repo.mkdir()
    git(repo,'init','-q','-b','main');(repo/'shared.py').write_text('value = 0\n');(repo/'base.py').write_text('value = 0\n');git(repo,'add','.');git(repo,'commit','-qm','base')
    base=git(repo,'rev-parse','HEAD'); rp=str(repo)
    assert rit('repo','inspect',rp,'--json')['inspection']['worktree']==rp
    record(1,'inspect repository','PASS','actual Git inspection')
    paths=[];ids=[]
    for i in range(5):
        path=root/f'work{i}';created=rit('workspace','create','--repo',rp,'--path',path,'--name',f'work{i}','--json')['workspace'];assert created['state']=='ready';paths.append(path);ids.append(created['id'])
    assert len(set(ids))==5
    record(2,'create five RIT-managed workspaces','PASS','five distinct worktrees and workspace IDs')
    listed=rit('workspace','list','--repo',rp,'--json')['reconciliation']['workspaces'];assert {w['manifest']['id'] for w in listed}==set(ids)
    record(3,'verify identities and manifests','PASS','reconciled Git manifests; not authenticated')
    watch=subprocess.Popen([binary,'activity','watch','--repo',rp,'--interval-ms','1000','--duration-ms','5000','--json'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    first=json.loads(watch.stdout.readline());assert first['type']=='watch_started'
    for i,p in enumerate(paths):(p/f'module{i}.py').write_text(f'import base\nvalue = {i}\n')
    record(4,'modify different files in each workspace','PASS','five actual worktree edits')
    for i in [0,1]:(paths[i]/'shared.py').write_text(f'value = {i+1}\n')
    record(5,'make overlapping modifications','PASS','two worktrees changed shared.py')
    lines=watch.stdout.read().splitlines();err=watch.stderr.read();code=watch.wait();assert code==0,err
    events=[json.loads(line) for line in lines];notifications=[x for x in events if x.get('type')=='notifications'];assert notifications and any(e.get('path')=='shared.py' for n in notifications for e in n['batch']['events'])
    record(6,'observe events','PASS','native watcher observed shared.py; history is incomplete')
    activity=rit('activity','reconcile','--repo',rp,'--json')['activity'];assert any(o['path']=='shared.py' for o in activity['overlaps'])
    record(7,'reconcile Git status','PASS','exact-path overlap, not a conflict verdict')
    graph=rit('dependencies','inspect','--repo',rp,'--workspace',ids[0],'--json')['graph'];assert 'module0.py' in graph['files']
    record(8,'analyze dependencies','PASS_PARTIAL','live non-atomic graph with unresolved/partial semantics')
    impact=rit('impact','--repo',rp,'--workspace',ids[0],'--json');assert impact['complete'] is False
    record(9,'produce impact reports','PASS_PARTIAL','incomplete graph and non-atomic status')
    checkpoints=[]
    for wid in ids:
        c=rit('checkpoint','create','--repo',rp,'--workspace',wid,'--include-untracked','--json')['checkpoint'];assert c['manifest']['complete'] is False;checkpoints.append(c['manifest']['id'])
    record(10,'create a complete checkpoint in each workspace','PARTIAL','five real content checkpoints created; all explicitly incomplete/non-atomic')
    for cid in checkpoints:
        v=rit('checkpoint','verify',cid,'--repo',rp,'--json');assert v['manifest_tree_consistency']=='passed' and v['index_stage_consistency']=='passed'
    record(11,'verify captured content','PASS_PARTIAL','objects, trees and index stages verified; no provenance/completeness proof')
    assert len(rit('workspace','list','--repo',rp,'--json')['reconciliation']['workspaces'])==5
    record(12,'restart RIT','PASS','each CLI invocation is a new process')
    assert len(rit('workspace','operations','--repo',rp,'--json')['operations'])==5
    record(13,'rediscover workspaces','PASS','workspace manifests and provisioning intents persist')
    assert len(rit('checkpoint','list','--repo',rp,'--json')['content_checkpoints'])==5
    record(14,'rediscover checkpoints','PASS','five retained content checkpoints')
    for i in [0,1]:git(paths[i],'add','.');git(paths[i],'commit','-qm',f'work{i}')
    source=git(paths[0],'rev-parse','HEAD');other=git(paths[1],'rev-parse','HEAD')
    sub=rit('submit','--repo',rp,'--workspace',ids[0],'--target','refs/heads/main','--json')['receipt']['submission'];sid=sub['id']
    record(15,'create submission','PASS','RIT Git-ref-backed committed submission, unauthenticated')
    candidate=rit('submission','candidate',sid,'--repo',rp,'--path',root/'candidate','--json')['result'];assert candidate['status']=='created';candidate_id=candidate['candidate']['candidate']['id']
    record(16,'construct candidate','PASS','detached candidate; source and target not published')
    conflict=rit('conflicts','simulate','--repo',rp,'--left',source,'--right',other,'--json')['conflicts'];assert conflict['simulation']['has_conflicts']
    record(17,'simulate textual conflict','PASS','actual merge-tree conflict on RIT workspace commits')
    assert conflict['simulation']['stages']
    record(18,'produce conflict evidence','PASS_PARTIAL','Git stages and advisory structural evidence')
    validation=rit('integration','validate',candidate_id,'--repo',rp,'--json')['validation'];assert validation['object_check'].startswith('passed') and validation['execution'].startswith('NOT_RUN')
    record(19,'validate clean candidate','PASS_PARTIAL','object and partial static checks; builds/tests NOT_RUN')
    git(repo,'commit','--allow-empty','-qm','target move')
    record(20,'move target ref','PASS','fixture Git advances target')
    candidates=rit('integration','list','--repo',rp,'--json')['candidates'];assert next(c for c in candidates if c['candidate']['id']==candidate_id)['target_moved']
    record(21,'detect stale candidate','PASS','target movement reported')
    observed=rit('submission','reconcile','--repo',rp,'--json')['submissions'];assert observed
    record(22,'reconcile','PASS_PARTIAL','read-only source/target and candidate reconciliation')
    git(paths[2],'add','.');git(paths[2],'commit','-qm','second source');sub2=rit('submit','--repo',rp,'--workspace',ids[2],'--target','refs/heads/main','--json')['receipt']['submission'];wrapper=root/'git-kill';wrapper.write_text('#!/bin/sh\ncase "$*" in *"worktree add --no-checkout --detach"*) kill -KILL "$PPID"; exit 97;; esac\nexec /usr/bin/git "$@"\n');wrapper.chmod(0o700)
    killed=cmd([binary,'--git-executable',str(wrapper),'submission','candidate',sub2['id'],'--repo',rp,'--path',str(root/'interrupted'),'--json'],ok=False);assert killed.returncode!=0
    record(23,'interrupt candidate creation','PASS','RIT process killed before worktree addition')
    found=rit('integration','reconcile','--repo',rp,'--json')['candidates'];assert any(c['candidate']['submission']==sub2['id'] and c['observation']=='interrupted_missing_worktree' for c in found)
    record(24,'recover interrupted candidate','PARTIAL','intent/ref/directory rediscovered and preserved; automatic recovery remains unavailable')
    (paths[0]/'shared.py').write_text('newer original\n');restored=root/'recovered';out=rit('checkpoint','restore',checkpoints[0],'--repo',rp,'--to',restored,'--yes','--accept-incomplete','--json');assert out['restore']['observation']=='completed_inventory_only';assert (restored/'shared.py').read_text()=='value = 1\n'
    record(25,'restore checkpoint into new worktree','PASS_PARTIAL','captured subset only; checkpoint incomplete')
    assert (paths[0]/'shared.py').read_text()=='newer original\n'
    record(26,'verify original workspace unchanged','PASS','newer source content preserved')
    git(paths[3],'add','.');git(paths[3],'commit','-qm','clean source');removed=rit('workspace','remove',ids[3],'--repo',rp,ok=False);assert removed.returncode!=0 and paths[3].exists()
    record(27,'remove clean workspace','BLOCKED','removal disabled pending non-destructive proof; clean workspace preserved')
    dirty=rit('workspace','remove',ids[4],'--repo',rp,ok=False);assert dirty.returncode!=0 and (paths[4]/'module4.py').exists()
    record(28,'refuse dirty workspace removal','BLOCKED','all removal is disabled; dirty-specific logic not implemented')
    req=json.dumps({'jsonrpc':'2.0','id':1,'method':'tools/call','params':{'name':'workspace.list','arguments':{}}})+'\n'
    proc=subprocess.run([binary,'mcp','serve','--repo',rp],input=req,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    assert proc.returncode==0 and len(json.loads(proc.stdout)['result']['structuredContent']['reconciliation']['workspaces'])==5
    record(29,'inspect workspace state through MCP','PASS','local read-only stdio scope, not authentication')
    git(repo,'fsck','--full','--no-dangling');assert git(paths[0],'rev-parse','HEAD')==source
    record(30,'verify Git history and integrity','PASS_PARTIAL','Git fsck passed; provenance not authenticated')
    print(json.dumps({'workflow':'V5 bounded development preview','steps':len(results),'counts':{state:sum(x['status']==state for x in results) for state in sorted({x['status'] for x in results})},'full_acceptance':False}),flush=True)
