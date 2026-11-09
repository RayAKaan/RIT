"""Disposable, local, non-isolated microbenchmark; run after cargo build --release."""
import tempfile
import pathlib
import subprocess
import time
import statistics
import json
import platform
import os

binary = pathlib.Path(__file__).resolve().parents[1] / 'target/release/rit'
with tempfile.TemporaryDirectory() as d:
    p = pathlib.Path(d)
    r = p / 'repo'
    r.mkdir()
    env = os.environ.copy()
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
               GIT_AUTHOR_NAME='Bench', GIT_AUTHOR_EMAIL='bench@example.invalid',
               GIT_COMMITTER_NAME='Bench', GIT_COMMITTER_EMAIL='bench@example.invalid')

    def git(*args):
        subprocess.run(['git', *args], cwd=r, env=env, stdout=subprocess.DEVNULL,
                       stderr=subprocess.PIPE, check=True)

    git('init', '-q')
    for i in range(50):
        (r / f'module_{i}.py').write_text(
            (f'import module_{(i + 1) % 50}\n' if i else 'import external_missing\n')
            + f'VALUE = {i}\n', encoding='utf-8')
    git('add', '.')
    git('commit', '-q', '-m', 'benchmark fixture')
    trees = []
    for i in range(5):
        worktree = p / f'worktree{i}'
        git('-c', f'core.hooksPath={os.devnull}', 'worktree', 'add', '-q', '-b',
            f'test{i}', str(worktree), 'HEAD')
        trees.append(worktree)
    (trees[0] / 'module_0.py').write_text('changed 1\n', encoding='utf-8')
    (trees[1] / 'module_0.py').write_text('changed 2\n', encoding='utf-8')
    cases = {
        'repo inspect': ['repo', 'inspect', str(r), '--json'],
        'dependencies inspect': ['dependencies', 'inspect', '--repo', str(r), '--json'],
        'activity reconcile': ['activity', 'reconcile', '--repo', str(r), '--json'],
    }
    result = {}
    for name, args in cases.items():
        times = []
        for _ in range(5):
            start = time.perf_counter()
            subprocess.run([str(binary), '--data-dir', str(p / 'none'), *args],
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, check=True)
            times.append(round((time.perf_counter() - start) * 1000, 2))
        result[name] = {'samples_ms': times, 'median_ms': statistics.median(times),
                        'min_ms': min(times), 'max_ms': max(times)}
    print(json.dumps({'platform': platform.platform(),
                      'git': subprocess.check_output(['git', '--version'], text=True).strip(),
                      'build': 'release',
                      'fixture': '50 committed Python files; 5 Git-created worktrees plus primary; two dirty on same path',
                      'measure': 'wall time ms; 5 sequential samples, no warmup isolation',
                      'results': result}, indent=2))
