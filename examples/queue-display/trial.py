"""番号保存・HTTP表示・同じ保存先での再起動を試す。ブラウザ描画の観測とは分ける。"""
import json
import os
import re
import sys
from pathlib import Path
from threading import Thread
from urllib.error import HTTPError
from urllib.request import Request, build_opener, ProxyHandler

from app import QueueServer

sys.path.insert(0, str(Path(__file__).parents[2] / "adapters/observation"))
from observe import Observer

opener = build_opener(ProxyHandler({}))


def start(storage):
    server = QueueServer(0, storage)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def stop(server, thread):
    server.shutdown()
    server.server_close()
    thread.join(timeout=2)
    if thread.is_alive():
        raise RuntimeError("試験用サーバーを停止できません")


def displayed(server):
    with opener.open(f"http://127.0.0.1:{server.server_port}/", timeout=3) as response:
        page = response.read().decode()
    match = re.search(r'id="current-number"[^>]*>(\d+)</span>', page)
    return int(match.group(1)) if match else None


if __name__ == "__main__":
    value = json.loads(Path(os.environ["EEL_INPUT"]).read_text())
    storage = Path(os.environ["EEL_WORK_DIR"]) / "stored.json"
    observer = Observer()
    server, thread = start(storage)
    try:
        request = Request(f"http://127.0.0.1:{server.server_port}/call", data=json.dumps(value).encode(), headers={"Content-Type": "application/json"})
        try:
            with opener.open(request, timeout=3) as response:
                print("ACCEPTED=" + str(json.loads(response.read())["number"]))
        except HTTPError as error:
            print("SAVE_REJECTED=" + str(error.code), file=sys.stderr)
            observer.emit("queue_matches_input", "display-1", "fail", "保存操作が失敗した")
            sys.exit(1)
        number = displayed(server)
        print("DISPLAYED=" + str(number))
    finally:
        stop(server, thread)
    # 新しいサーバーと状態オブジェクトを使い、同じ保存物から読み直す。
    server, thread = start(storage)
    try:
        restarted = displayed(server)
        print("RESTARTED=" + str(restarted))
        observer.emit("queue_matches_input", "display-1", "pass" if number == restarted == value["number"] else "fail", "HTTPで返した表示番号と再起動後の番号を入力と照合した。ブラウザ描画・本番環境は未確認")
    finally:
        stop(server, thread)
