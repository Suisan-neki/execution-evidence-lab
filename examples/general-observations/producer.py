"""合成の通知をファイルへ保存し、読み出した結果をJSONで報告する接続例。"""
import argparse
import hashlib
import json
import pathlib
import platform
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('--run-id', required=True)
parser.add_argument('--mode', choices=['pass', 'fail', 'missing', 'conflict', 'incomplete'], required=True)
args = parser.parse_args()
correlation = {'run_id': args.run_id, 'record_id': 'notification-1', 'attempt_id': args.run_id + '-attempt'}
input_data = {'message': '合成の通知'}
with tempfile.TemporaryDirectory(prefix='eel-notification-') as directory:
    storage = pathlib.Path(directory) / 'notification.json'
    stored = {'message': '人工的に内容を変えた'} if args.mode == 'fail' else input_data
    storage.write_text(json.dumps(stored, ensure_ascii=False), encoding='utf8')
    matches = json.loads(storage.read_text(encoding='utf8')) == input_data
    outcomes = [] if args.mode == 'missing' else ['pass' if matches else 'fail']
    if args.mode == 'conflict':
        outcomes.append('fail')  # 比較規則の反例として成功と矛盾する報告を意図的に追加する。
    observations = [{'sequence': i+1, 'correlation': correlation, 'assertion': 'readback_matches',
                     'subject': 'notification-1', 'outcome': outcome,
                     'detail': '合成の通知ファイルを読み出して入力と照合した。矛盾の報告は反例用の注入。',
                     'source': None} for i, outcome in enumerate(outcomes)]
    trace = {'schema_version': 1, 'correlation': correlation, 'producer': '合成の通知ファイル接続例・Python',
             'origin': 'examples/general-observations/producer.pyによる報告。固定版の実行証明ではない',
             'completed': args.mode != 'incomplete', 'source': None,
             'context': {'input': input_data, 'mode': args.mode, 'python': platform.python_version(),
                         'producer_code_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()},
             'observations': observations}
    print(json.dumps(trace, ensure_ascii=False))
