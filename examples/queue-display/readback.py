"""基盤が別プロセスで呼び出す保存物の読み出し。"""
import json
import sys
from pathlib import Path

value = json.loads(Path(sys.argv[1]).read_text())
print("STORED=" + str(value["number"]))
print("TEST_ID=" + value["test_id"])
