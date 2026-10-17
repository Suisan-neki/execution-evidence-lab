"""Build the pinned viewer and retain static assets for a later browser trial."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

subprocess.run(["pnpm", "run", "build"], check=True)
dist = Path("platform/app/dist")
if not (dist / "index.html").is_file():
    raise RuntimeError("OHIF build did not produce index.html")
shutil.copytree(dist, "/result/dist")
manifest = [dict(path=str(p.relative_to(dist)), bytes=p.stat().st_size,
                 sha256=hashlib.sha256(p.read_bytes()).hexdigest())
            for p in sorted(dist.rglob("*")) if p.is_file()]
Path("/result/build-files.json").write_text(json.dumps(manifest, indent=2))
