"""架空の番号だけを扱う試験用アプリ。患者情報を入れず、localhostで使う。"""
import argparse
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Lock

DISPLAY_OFFSET = 0


class QueueServer(ThreadingHTTPServer):
    def __init__(self, port, storage):
        self.storage = Path(storage)
        self.lock = Lock()
        self.value = json.loads(self.storage.read_text()) if self.storage.is_file() else None
        super().__init__(("127.0.0.1", port), Handler)

    def call(self, value):
        number = value.get("number")
        test_id = value.get("test_id", "manual-trial")
        if type(number) is not int or not 1 <= number <= 9999:
            raise ValueError("番号は1〜9999の整数です")
        if not isinstance(test_id, str) or len(test_id) > 200:
            raise ValueError("試験識別子は200文字以内です")
        data = {"number": number, "test_id": test_id}
        with self.lock:
            temporary = self.storage.with_suffix(".pending")
            try:
                with temporary.open("x", encoding="utf-8") as stream:
                    json.dump(data, stream)
                    stream.flush()
                    os.fsync(stream.fileno())
                os.replace(temporary, self.storage)
            finally:
                temporary.unlink(missing_ok=True)
            # 保存できた後に表示用の状態を更新する。
            self.value = data
        return data


PAGE = """<!doctype html><html lang="ja"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>試験用呼び出し番号</title>
<style>body{font-family:sans-serif;max-width:640px;margin:32px auto;padding:16px}#current-number{font-size:64px}input,button{font:inherit;padding:12px}label{display:block}#error{color:#b00020}</style>
<main><h1>試験用呼び出し番号</h1><p>架空の番号だけを使う参照アプリです。患者情報は入力しないでください。</p>
<p>現在の番号 <span id="current-number" aria-live="polite">__NUMBER__</span></p>
<form><label>呼び出す番号<input name="number" type="number" min="1" max="9999" required></label><button>呼び出す</button></form>
<p id="error" role="alert"></p><script>
document.querySelector('form').addEventListener('submit',async e=>{e.preventDefault();
const button=e.target.querySelector('button');button.disabled=true;
try{const response=await fetch('/call',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({number:Number(e.target.number.value)})});
const data=await response.json();if(!response.ok)throw Error(data.error);
location.reload();}catch(error){document.getElementById('error').textContent=error.message;}finally{button.disabled=false;}});
</script></main></html>"""


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, status, body, content_type="application/json; charset=utf-8"):
        data = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/":
            with self.server.lock:
                number = str(self.server.value["number"] + DISPLAY_OFFSET) if self.server.value else "未選択"
            self.reply(200, PAGE.replace("__NUMBER__", number), "text/html; charset=utf-8")
        elif self.path == "/state":
            with self.server.lock:
                self.reply(200, json.dumps(self.server.value))
        else:
            self.reply(404, '{"error":"not found"}')

    def do_POST(self):
        if self.path != "/call":
            self.reply(404, '{"error":"not found"}')
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 2048:
                raise ValueError("入力は1〜2048バイトです")
            value = json.loads(self.rfile.read(length))
            if not isinstance(value, dict):
                raise ValueError("オブジェクト形式の入力が必要です")
            self.reply(200, json.dumps(self.server.call(value)))
        except (ValueError, UnicodeError) as error:
            self.reply(400, json.dumps({"error": str(error)}, ensure_ascii=False))
        except OSError:
            self.reply(503, json.dumps({"error": "保存できませんでした。表示する番号は更新していません。"}, ensure_ascii=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8766)
    parser.add_argument("--storage", default="queue-state.json")
    args = parser.parse_args()
    with QueueServer(args.port, args.storage) as server:
        print(f"http://127.0.0.1:{server.server_port}", flush=True)
        server.serve_forever()
