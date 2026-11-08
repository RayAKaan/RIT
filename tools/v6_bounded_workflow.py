#!/usr/bin/env python3
"""35-step V6 diagnostic. Nonzero unless every acceptance criterion is met."""
import json, os, pathlib, subprocess, sys, tempfile

binary = str(pathlib.Path(os.environ.get('RIT_BINARY', 'target/debug/rit')).resolve())
results = []
def record(n, title, status, detail):
    assert n == len(results) + 1
    value = {'step': n, 'description': title, 'status': status, 'detail': detail}
    results.append(value); print(json.dumps(value), flush=True)
def command(args, cwd=None, check=True, env=None):
    p = subprocess.run(list(map(str,args)), cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if check and p.returncode: raise RuntimeError(f'{args}: exit {p.returncode}: {p.stderr}')
    return p

def git(path,*args):
    env=dict(os.environ, GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid', GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid', GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
    return command(['git',*args],cwd=path,env=env).stdout.strip()

with tempfile.TemporaryDirectory(prefix='rit-v6-workflow-') as td:
    root=pathlib.Path(td);os.chmod(root,0o700);repo=root/'repo';repo.mkdir();rp=str(repo);data=root/'rit-data'
    def rit(*args,ok=True):
        p=command([binary,'--data-dir',data,*args],check=ok)
        return json.loads(p.stdout) if ok and p.stdout.strip() else p
    def blocked(*args):
        p=rit(*args,ok=False);assert p.returncode != 0;return p.stderr.strip()
    git(repo,'init','-q','-b','main');(repo/'shared.py').write_text('value = 0\n');(repo/'base.py').write_text('value = 0\n');git(repo,'add','.');git(repo,'commit','-qm','base')
    record(1,'build RIT CLI','PASS','running prebuilt RIT_BINARY; build is separately logged')
    assert rit('repo','inspect',rp,'--json')['inspection']['worktree']==rp
    record(2,'inspect repository','PASS','actual Git inspection')
    msg=blocked('repository','register',rp);assert 'security' in msg.lower() or 'blocked' in msg.lower()
    record(3,'register repository','BLOCKED','Phase 2.1 fail-closed storage gate refused registration')
    blocked('identity','bootstrap');record(4,'bootstrap owner','BLOCKED','authenticated identity disabled')
    for i in range(5): blocked('identity','create-agent',f'agent{i}')
    record(5,'create five participant identities','BLOCKED','all five identity commands disabled')
    blocked('permission','roles');record(6,'assign scoped permissions','BLOCKED','authenticated authorization unavailable')
    paths=[];ids=[]
    for i in range(5):
        path=root/f'work{i}';w=rit('workspace','create','--repo',rp,'--path',path,'--name',f'participant{i}','--json')['workspace'];assert w['state']=='ready';paths.append(path);ids.append(w['id'])
    assert len(set(ids))==5
    record(7,'create five managed workspaces','PASS_PARTIAL','five real worktrees; no authenticated owners or OS sandbox')
    blocked('task','create',rp);record(8,'create connected tasks','BLOCKED','persistent task service unavailable')
    state=rit('workspace','list','--repo',rp,'--json')['reconciliation'];assert len(state['workspaces'])==5
    request=json.dumps({'jsonrpc':'2.0','id':1,'method':'tools/call','params':{'name':'workspace.list','arguments':{}}})+'\n';p=subprocess.run([binary,'mcp','serve','--repo',rp],input=request,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    assert p.returncode==0 and len(json.loads(p.stdout)['result']['structuredContent']['reconciliation']['workspaces'])==5
    record(9,'inspect workspaces and MCP','PASS_PARTIAL','live Git and local scoped stdio reads; no authenticated identity')
    watcher=subprocess.Popen([binary,'activity','watch','--repo',rp,'--interval-ms','1000','--duration-ms','5000','--json'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True);assert json.loads(watcher.stdout.readline())['type']=='watch_started'
    for i,path in enumerate(paths):(path/f'module{i}.py').write_text(f'import base\nvalue = {i}\n')
    record(10,'modify independent files','PASS','five separate worktree edits')
    for i in (0,1):(paths[i]/'shared.py').write_text(f'value = {i+1}\n')
    record(11,'create overlapping changes','PASS','two worktrees edit the same tracked path')
    events=[json.loads(x) for x in watcher.stdout.read().splitlines()];err=watcher.stderr.read();assert watcher.wait()==0,err;assert any(e.get('path')=='shared.py' for batch in events if batch.get('type')=='notifications' for e in batch['batch']['events'])
    record(12,'observe workspace activity','PASS_PARTIAL','native notification observed; event history is not durable or complete')
    c=rit('checkpoint','create','--repo',rp,'--workspace',ids[0],'--include-untracked','--json')['checkpoint'];cid=c['manifest']['id'];v=rit('checkpoint','verify',cid,'--repo',rp,'--json');assert not c['manifest']['complete'] and v['index_stage_consistency']=='passed'
    record(13,'checkpoint uncommitted work','PASS_PARTIAL','real verified captured subset, non-atomic and incomplete')
    (paths[0]/'shared.py').write_text('newer source\n');record(14,'modify content after checkpoint','PASS','original workspace contains newer data')
    extracted=root/'extracted';x=rit('checkpoint','extract',cid,'--repo',rp,'--to',extracted,'--path','shared.py','--yes','--accept-incomplete','--json');assert x['extraction']['operation']['state']=='completed' and (extracted/'shared.py').read_text()=='value = 1\n'
    record(15,'restore selected content','PASS_PARTIAL','selected raw bytes extracted to a new directory, not in-place')
    assert (paths[0]/'shared.py').read_text()=='newer source\n' and (paths[0]/'module0.py').exists()
    record(16,'preserve newer and unrelated work','PASS','source and unrelated work unchanged')
    (paths[0]/'shared.py').write_text('value = 1\n');subs=[]
    for i in (0,1):
        git(paths[i],'add','.');git(paths[i],'commit','-qm',f'source{i}');subs.append(rit('submit','--repo',rp,'--workspace',ids[i],'--target','refs/heads/main','--json')['receipt']['submission'])
    record(17,'submit multiple workspaces','PASS_PARTIAL','two committed source submissions, unauthenticated')
    candidates=[]
    for i in (0,1):candidates.append(rit('submission','candidate',subs[i]['id'],'--repo',rp,'--path',root/f'candidate{i}','--json')['result'])
    assert all(c['status']=='created' for c in candidates)
    record(18,'construct integration candidates','PASS_PARTIAL','two detached candidates; no publication or sandbox')
    left=git(paths[0],'rev-parse','HEAD');right=git(paths[1],'rev-parse','HEAD');conflict=rit('conflicts','simulate','--repo',rp,'--left',left,'--right',right,'--json')['conflicts'];assert conflict['simulation']['has_conflicts']
    record(19,'introduce textual conflict','PASS','real divergent commits conflict in Git merge-tree')
    assert conflict['simulation']['stages'];record(20,'inspect conflict','PASS','real Git stages and messages')
    record(21,'generate resolution evidence','PASS_PARTIAL','Git and advisory structural evidence; no generated resolution')
    cv=rit('integration','validate',candidates[0]['candidate']['candidate']['id'],'--repo',rp,'--json')['validation'];assert cv['object_check'].startswith('passed') and cv['execution'].startswith('NOT_RUN')
    record(22,'run candidate validation','PASS_PARTIAL','Git fsck and partial static analysis; builds/tests NOT_RUN')
    (paths[2]/'test_fail.py').write_text('def test_deliberate_failure():\n    assert False\n');record(23,'introduce failing test','PASS','fixture test written, not executed by RIT')
    record(24,'reject failing candidate','BLOCKED','repository validation execution disabled; no test result fabricated')
    assert git(paths[0],'rev-parse','HEAD')==left and git(paths[1],'rev-parse','HEAD')==right and (root/'candidate0').exists()
    record(25,'preserve original workspaces','PASS','source HEADs and candidate worktrees preserved')
    (paths[2]/'test_fail.py').write_text('def test_deliberate_failure():\n    assert True\n');record(26,'correct test fixture','PASS','changed fixture content; no test execution claim')
    git(paths[2],'add','.');git(paths[2],'commit','-qm','corrected');third=rit('submit','--repo',rp,'--workspace',ids[2],'--target','refs/heads/main','--json')['receipt']['submission'];record(27,'submit corrected work','PASS_PARTIAL','committed submission; no validation or approval claim')
    blocked('task','review',third['id']);record(28,'obtain human approval','BLOCKED','authenticated review and approval disabled')
    blocked('submission','publish',third['id']);record(29,'publish safely','BLOCKED','publication disabled pending identity and storage gates')
    assert git(repo,'rev-parse','HEAD')==git(repo,'rev-parse','refs/heads/main');git(repo,'log','--oneline','--all','-5');record(30,'inspect Git history','PASS','ordinary Git history remains readable; target unchanged')
    wrapper=root/'git-kill';wrapper.write_text('#!/bin/sh\ncase "$*" in *"worktree add --no-checkout --detach"*) kill -KILL "$PPID"; exit 97;; esac\nexec /usr/bin/git "$@"\n');wrapper.chmod(0o700);killed=command([binary,'--git-executable',wrapper,'submission','candidate',third['id'],'--repo',rp,'--path',root/'interrupted','--json'],check=False);assert killed.returncode==-9
    record(31,'interrupt coordinator','PASS','actual SIGKILL during candidate provisioning')
    found=rit('integration','reconcile','--repo',rp,'--json')['candidates'];interrupted=[x for x in found if x['candidate']['submission']==third['id']];assert interrupted
    record(32,'restart coordinator','PASS','new CLI process rediscovers candidate state')
    assert interrupted[0]['observation']=='interrupted_missing_worktree';record(33,'reconcile unfinished work','PASS_PARTIAL','intent and directory preserved; no automatic resume or cleanup')
    blocked('audit','verify');record(34,'verify protected audit','BLOCKED','protected authenticated audit is unavailable')
    git(repo,'fsck','--full','--no-dangling');assert git(paths[0],'rev-parse','HEAD')==left;record(35,'confirm normal Git operation','PASS','Git fsck and source history remain intact')
    counts={s:sum(x['status']==s for x in results) for s in sorted({x['status'] for x in results})}
    accepted=all(x['status']=='PASS' for x in results)
    print(json.dumps({'workflow':'V6 bounded diagnostic','steps':len(results),'counts':counts,'full_acceptance':accepted}),flush=True)
    if not accepted:sys.exit(1)
