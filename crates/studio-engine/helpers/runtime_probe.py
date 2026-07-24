import subprocess
import sys
import time
from pathlib import Path

root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()
main = root / "main.py"
if not main.exists():
    print(f"STUDIO_CI_FAIL: {main}: main.py is missing")
    sys.exit(1)

proc = subprocess.Popen(
    [sys.executable, str(main)],
    cwd=str(root),
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)

deadline = time.monotonic() + 6.0
while time.monotonic() < deadline:
    code = proc.poll()
    if code is not None:
        break
    time.sleep(0.2)
else:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
    print("STUDIO_CI_DONE runtime probe clean: the game loop survived 6s")
    sys.exit(0)

_, err = proc.communicate()
if code == 0:
    print("STUDIO_CI_DONE runtime probe clean: the program exited 0")
    sys.exit(0)

tail = err.decode(errors="replace").strip().splitlines()[-10:]
for line in tail:
    print(f"STUDIO_CI_FAIL: runtime: {line.strip()}")
print(f"STUDIO_CI_DONE runtime probe failed with exit code {code}")
sys.exit(1)
