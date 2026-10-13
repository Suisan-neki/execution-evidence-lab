# JavaScript版と同じ観測契約を使う合成データの保存試験。
import json
import os
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parents[2] / 'adapters' / 'observation'))
from observe import Observer
observer = Observer()
scenario = sys.argv[1] if len(sys.argv) > 1 else 'normal'
input_data = json.loads(Path(os.environ['EEL_INPUT']).read_text())
def observe(outcome, detail):
    observer.emit('stored_matches_input','record-1',outcome,detail)
if scenario == 'failure':
    observe('fail', '実験条件で保存前に中止した')
else:
    destination = Path(os.environ['EEL_WORK_DIR']) / 'stored.json'
    destination.write_text(json.dumps(input_data))
    matches = json.loads(destination.read_text()) == input_data
    if scenario != 'missing':
        observe('pass' if matches else 'fail', '試行用の保存物を読み出して入力と照合した')
    if scenario == 'conflict':
        observe('fail', '同じ対象について食い違う人工的な観測')
