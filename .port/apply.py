"""One-time application of the reviewed, checksummed native-menu patch."""
import hashlib
import lzma
import os
from pathlib import Path
import shutil
import subprocess

assert os.environ['GITHUB_REPOSITORY'] == 'Xon333/ytfast-macos'
assert os.environ['GITHUB_REF'] == 'refs/heads/perf/menubar'
patch = lzma.decompress(b''.join(Path(f'.port/part-{i}').read_bytes() for i in range(4)))
assert hashlib.sha256(patch).hexdigest() == '03cb72a6edc87b6f8a355b9fd4a5ffa77abcc2eab5ccc06d0ee2ebbec97d8e52'
subprocess.run(['git', 'apply', '--index', '-'], input=patch, check=True)
shutil.rmtree('.port')
Path('.github/workflows/menubar.yml').unlink()
subprocess.run(['cargo', 'fmt', '--all'], check=True)
subprocess.run(['git', 'add', '-A'], check=True)
subprocess.run(['git', 'config', 'user.name', 'github-actions[bot]'], check=True)
subprocess.run(['git', 'config', 'user.email', '41898282+github-actions[bot]@users.noreply.github.com'], check=True)
subprocess.run(['git', 'commit', '-m', 'Replace Mac renderer with native menu and shared playback core'], check=True)
subprocess.run(['git', 'push', 'origin', 'HEAD:refs/heads/perf/menubar'], check=True)
sha = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
    output.write(f'sha={sha}\n')
