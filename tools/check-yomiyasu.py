"""上流の検査を実際に動かし、否定と数値の変更を見落とさないことを確認する。"""
from pathlib import Path
import os
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
subprocess.run([sys.executable, str(root / "tools/yomiyasu-policy.py"), "--check"], check=True)
checker = root / "third_party/yomiyasu/skill/scripts/yomiyasu_check.py"
with tempfile.TemporaryDirectory(prefix="eel-yomiyasu-") as temporary:
    directory = Path(temporary)
    before, after = directory / "before.md", directory / "after.md"
    before.write_text("保存を3回試しました。まだ保存の成功は確認していません。\n")
    after.write_text("保存を30回試しました。保存の成功を確認しました。\n")
    env = {**os.environ, "YOMIYASU_STATE_DIR": str(directory / "state"), "PYTHONDONTWRITEBYTECODE": "1"}
    result = subprocess.run([sys.executable, str(checker), "run", "--source", str(before), "--final", str(after), "--reader", "general", "--doc-type", "article", "--fail-on", "meaning-risk"], env=env, capture_output=True, text=True)
    if result.returncode != 1 or "意味が変わる" not in result.stdout:
        raise SystemExit("上流の検査で、数値と否定を変えた反例を検出できませんでした\n" + result.stdout + result.stderr)
    verified = subprocess.run([sys.executable, str(checker), "verify", "--final", str(after)], env=env, capture_output=True, text=True)
    if verified.returncode != 0:
        raise SystemExit("上流の最終稿照合を実行できませんでした\n" + verified.stdout + verified.stderr)
print("yomiyasu: 上流のlint・diff・最終稿照合を実行。意味の保持や読みやすさの合格を示す検査ではありません。")
