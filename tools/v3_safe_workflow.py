"""Run 23 bounded real CLI/MCP operations in a disposable fixture, not the production scenario."""
import json
import os
import pathlib
import subprocess
import tempfile

rit = pathlib.Path(__file__).resolve().parents[1] / 'target/debug/rit'
with tempfile.TemporaryDirectory() as d:
    root = pathlib.Path(d)
    repo = root / 'repo'
    repo.mkdir()
    env = os.environ.copy()
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
               GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid',
               GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid')

    def git(*args):
        return subprocess.run(['git', *args], cwd=repo, env=env, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True, check=True).stdout.strip()

    git('init', '-q')
    (repo / 'a.py').write_text('import b\n', encoding='utf-8')
    (repo / 'b.py').write_text('VALUE = 1\n', encoding='utf-8')
    git('add', '.')
    git('commit', '-q', '-m', 'fixture')
    git('tag', 'fixture-tag')
    head = git('rev-parse', 'HEAD')
    trees = []
    for i in range(5):
        path = root / f'worktree{i}'
        git('-c', f'core.hooksPath={os.devnull}', 'worktree', 'add', '-q', '-b',
            f'fixture{i}', str(path), 'HEAD')
        trees.append(path)
    (trees[0] / 'a.py').write_text('import b\nVALUE = 2\n', encoding='utf-8')
    (trees[1] / 'a.py').write_text('import b\nVALUE = 3\n', encoding='utf-8')
    data = root / 'no-registry'
    n = [0]

    def call(label, *args, lines=False):
        result = subprocess.run([str(rit), '--data-dir', str(data), *map(str, args)],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                text=True, check=True)
        n[0] += 1
        print(f'{n[0]:02} {label}: ok')
        return [json.loads(x) for x in result.stdout.splitlines()] if lines else json.loads(result.stdout)

    path = str(repo)
    call('inspect', 'repo', 'inspect', path, '--json')
    call('status', 'repo', 'status', path, '--json')
    call('branches', 'repo', 'branches', path, '--json')
    call('tags', 'repo', 'tags', path, '--json')
    call('refs', 'repo', 'refs', path, '--json')
    call('log', 'repo', 'log', path, '--json')
    call('diff', 'repo', 'diff', str(trees[0]), '--json')
    inventory = call('worktree inventory', 'repo', 'worktrees', path, '--json')
    call('repo doctor', 'repo', 'doctor', path, '--json')
    call('global doctor', 'doctor', '--json')
    call('dependencies inspect', 'dependencies', 'inspect', '--repo', path, '--json')
    call('dependencies refresh', 'dependencies', 'refresh', '--repo', path, '--json')
    call('reverse impact', 'impact', '--repo', path, '--files', 'b.py', '--json')
    activity = call('activity reconcile', 'activity', 'reconcile', '--repo', path, '--json')
    assert len(activity['activity']['worktrees']) == 6
    assert activity['activity']['overlaps'][0]['path'] == 'a.py'
    call('activity list', 'activity', 'list', '--repo', path, '--json')
    overlap = call('activity overlaps', 'activity', 'overlaps', '--repo', path, '--json')
    assert len(overlap['overlaps'][0]['worktrees']) == 2
    watch = call('activity poll twice', 'activity', 'watch', '--repo', path,
                 '--count', '2', '--interval-ms', '250', '--json', lines=True)
    assert len(watch) == 2 and not watch[0]['activity']['complete_event_history']
    checkpoint = call('commit-only checkpoint', 'checkpoint', 'create', '--repo', path, '--commit-only', '--json')['checkpoint']
    assert checkpoint['commit']['hex'] == head
    assert 'untracked' in checkpoint['capture_scope']
    call('checkpoint list', 'checkpoint', 'list', '--repo', path, '--json')
    verified = call('checkpoint verify', 'checkpoint', 'verify', checkpoint['id'], '--repo', path, '--json')
    assert verified['git_fsck'] == 'passed' and not verified['uncommitted_work_captured']
    call('dirty status after checkpoint', 'repo', 'status', str(trees[0]), '--json')
    call('worktree inspect', 'repo', 'inspect', str(trees[1]), '--json')
    with subprocess.Popen([str(rit), '--data-dir', str(data), 'mcp', 'serve', '--repo', path],
                          stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) as child:
        child.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
                                      'params': {'name': 'repository.inspect', 'arguments': {}}}) + '\n')
        child.stdin.flush()
        response = json.loads(child.stdout.readline())
        assert response['result']['structuredContent']['inspection']['object_format'] == 'sha1'
        child.stdin.close()
        assert child.wait(timeout=5) == 0
    n[0] += 1
    print(f'{n[0]:02} scoped MCP repository read: ok')
    assert n[0] == 23 and not data.exists() and git('rev-parse', 'HEAD') == head
    print('PASS: disposable RIT subset only; Git fixture worktrees are not RIT-managed, no production workflow claim')
