"""固定した上流資料から、実行時に渡す規則を再現する。外部通信はしない。"""
from pathlib import Path
import argparse
import hashlib
import json

root = Path(__file__).resolve().parents[1] / "third_party/yomiyasu"
parser = argparse.ArgumentParser()
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
metadata = json.loads((root / "UPSTREAM.json").read_text())
for path, expected in metadata["source_files"].items():
    if hashlib.sha256((root / path).read_bytes()).hexdigest() != expected:
        raise SystemExit(f"上流資料の固定値と一致しません: {path}")
readme = (root / "README.md").read_text()
audience = (root / "skill/references/audience.md").read_text()
rules = readme.split("### 7つの変換原則\n", 1)[1].split("### 2.0.0", 1)[0].strip()
readers = audience.split("## 7. 読者の3段階\n", 1)[1].split("## 10.", 1)[0].strip()
text = f"# yomiyasu {metadata['version']}\n\n{rules}\n\n## 読者に応じた説明\n\n{readers}\n"
output = root / "runtime-rules.md"
if args.check:
    if not output.exists() or output.read_text() != text:
        raise SystemExit("実行時の規則が上流資料から再現できません")
else:
    output.write_text(text)
print(f"yomiyasu {metadata['version']} / {metadata['commit']}: 規則の固定値と内容を確認")
