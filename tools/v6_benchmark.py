#!/usr/bin/env python3
"""Small disposable Linux development benchmark; not a production performance claim."""
import argparse, json, os, pathlib, platform, sqlite3, statistics, subprocess, tempfile, time

parser=argparse.ArgumentParser();parser.add_argument('--samples',type=int,default=3);parser.add_argument('--files',type=int,default=12);args=parser.parse_args()
if not 1<=args.samples<=10 or not 2<=args.files<=200:parser.error('samples 1..10 and files 2..200')
binary=pathlib.Path(os.environ.get('RIT_BINARY','target/debug/rit')).resolve();measurements={}
def run(argv,cwd=None,env=None):
    p=subprocess.run(list(map(str,argv)),cwd=cwd,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    if p.returncode:raise RuntimeError(f'{argv}: exit {p.returncode}: {p.stderr}')
    return p.stdout
def measure(label,call):
    start=time.perf_counter_ns();value=call();measurements.setdefault(label,[]).append((time.perf_counter_ns()-start)/1e6);return value
with tempfile.TemporaryDirectory(prefix='rit-v6-bench-') as td:
    root=pathlib.Path(td);os.chmod(root,0o700);repo=root/'repo';repo.mkdir();rp=str(repo)
    env=dict(os.environ,GIT_AUTHOR_NAME='Benchmark',GIT_AUTHOR_EMAIL='benchmark@example.invalid',GIT_COMMITTER_NAME='Benchmark',GIT_COMMITTER_EMAIL='benchmark@example.invalid',GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null')
    def git(*a,cwd=repo):return run(['git',*a],cwd=cwd,env=env).strip()
    def rit(*a):return json.loads(run([binary,*a]))
    git('init','-q','-b','main');(repo/'base.py').write_text('value = 1\n')
    for i in range(args.files-1):(repo/f'module{i:03}.py').write_text(f'import base\nvalue = {i}\n')
    git('add','.');git('commit','-qm','fixture');paths=[];ids=[]
    for i in range(args.samples):
        path=root/f'work{i}';w=measure('workspace_create_ms',lambda:rit('workspace','create','--repo',rp,'--path',path,'--name',f'bench{i}','--json'))['workspace'];paths.append(path);ids.append(w['id'])
    work=paths[0];(work/'module000.py').write_text('import base\nvalue = 999\n')
    for i in range(args.samples):
        measure('repository_inspect_ms',lambda:rit('repo','inspect',rp,'--json'))
        measure('repository_status_ms',lambda:rit('repo','status',work,'--json'))
        measure('workspace_reconcile_ms',lambda:rit('workspace','reconcile','--repo',rp,'--json'))
        measure('activity_reconcile_ms',lambda:rit('activity','reconcile','--repo',rp,'--json'))
        measure('dependencies_worktree_ms',lambda:rit('dependencies','inspect','--repo',rp,'--workspace',ids[0],'--json'))
        measure('dependencies_index_ms',lambda:rit('dependencies','inspect','--repo',rp,'--workspace',ids[0],'--staged','--json'))
        c=measure('checkpoint_create_ms',lambda:rit('checkpoint','create','--repo',rp,'--workspace',ids[0],'--json'))['checkpoint']['manifest']['id']
        measure('checkpoint_verify_ms',lambda:rit('checkpoint','verify',c,'--repo',rp,'--json'))
        measure('checkpoint_extract_ms',lambda:rit('checkpoint','extract',c,'--repo',rp,'--to',root/f'extract{i}','--path','module000.py','--yes','--accept-incomplete','--json'))
        measure('checkpoint_new_worktree_restore_ms',lambda:rit('checkpoint','restore',c,'--repo',rp,'--to',root/f'restore{i}','--yes','--accept-incomplete','--json'))
    git('add','.',cwd=work);git('commit','-qm','source',cwd=work);source=git('rev-parse','HEAD',cwd=work);target=git('rev-parse','HEAD')
    sub=rit('submit','--repo',rp,'--workspace',ids[0],'--target','refs/heads/main','--json')['receipt']['submission']['id']
    for i in range(args.samples):
        measure('conflict_simulate_ms',lambda:rit('conflicts','simulate','--repo',rp,'--left',target,'--right',source,'--json'))
        measure('candidate_create_ms',lambda:rit('submission','candidate',sub,'--repo',rp,'--path',root/f'candidate{i}','--json'))
        req=json.dumps({'jsonrpc':'2.0','id':1,'method':'tools/call','params':{'name':'repository.inspect','arguments':{}}})+'\n'
        def mcp():
            p=subprocess.run([binary,'mcp','serve','--repo',rp],input=req,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            if p.returncode:raise RuntimeError(p.stderr)
            assert json.loads(p.stdout)['result']['structuredContent']['inspection'];return p.stdout
        measure('mcp_stdio_start_and_inspect_ms',mcp)
    summary={name:{'samples_ms':[round(x,3) for x in samples],'median_ms':round(statistics.median(samples),3),'min_ms':round(min(samples),3),'max_ms':round(max(samples),3)} for name,samples in sorted(measurements.items())}
    print(json.dumps({'environment':{'os':platform.platform(),'machine':platform.machine(),'cpu_count':os.cpu_count(),'git':git('--version'),'rustc':run(['rustc','--version']).strip(),'python_sqlite':sqlite3.sqlite_version,'rit_sqlite':'bundled SQLite runtime version not measured in this benchmark','cpu_model':next((line.split(':',1)[1].strip() for line in pathlib.Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')), 'unknown') if pathlib.Path('/proc/cpuinfo').exists() else 'unknown','fixture_bytes':sum(p.stat().st_size for p in repo.glob('*.py')),'workspaces':len(paths),'files_per_workspace':args.files,'samples':args.samples,'concurrency':1,'cache':'unspecified; repeated warm-ish local runs; no cache flush','method':'wall clock includes subprocess startup and JSON parsing; disposable local fixture'},'measurements':summary,'unavailable':['durable activity queries','authorized validation execution','permission evaluation','publication'],'claim':'small development smoke benchmark only; not comparable baseline or production performance'},indent=2))
