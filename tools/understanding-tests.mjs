// 概要・質問・試験準備の解析を、架空のリポジトリで確かめる。ネットワークは使わない。
import assert from 'node:assert/strict';
import { analyze } from '../ui/scanner.js';
import { overview, lineCount } from '../ui/overview.js';
import { inquire, functionDetail } from '../ui/inquiry.js';
import { plan } from '../ui/trial-plan.js';

const COMMIT = 'a'.repeat(40);
function snapshot(files, extra = []) {
  const inventory = [...Object.keys(files), ...extra].map(path => ({ path, size: (files[path] || '').length || 10, mode: '100644', sha: '0'.repeat(40) }));
  return { schema: 1, fullName: 'example/notes', description: '架空の記録アプリ', reference: 'main', commit: COMMIT, truncated: false, inventory, files, omitted: extra.map(path => ({ path, reason: '初回の読取範囲外' })) };
}

const notes = `import os
import sys
from optparse import OptionParser

class BadFile(Exception):
    def __init__(self, path):
        super(BadFile, self).__init__()
        self.path = path

class Store(object):
    def __init__(self, directory):
        self.directory = directory

    def write(self, text):
        path = os.path.join(self.directory, 'notes')
        try:
            with open(path, 'w') as f:
                f.write(text)
        except IOError:
            raise BadFile(path)

def main():
    parser = OptionParser()
    parser.add_option("-d", "--dir", dest="directory", default="", metavar="DIR", help="保存するディレクトリ")
    parser.add_option("-n", "--name", dest="name", default="notes", metavar="NAME", help="保存するファイル名")
    (options, args) = parser.parse_args()
    store = Store(options.directory)
    try:
        store.write(' '.join(args))
    except BadFile as e:
        sys.stderr.write('保存できませんでした: %s\\n' % e.path)
        sys.exit(1)

if __name__ == '__main__':
    main()
`;
const readme = '# notes\n\nnotes は、短いメモをファイルに保存するための架空のコマンドです。\n\n    python notes.py 買い物\n';
const test = '  $ alias xn="python $TESTDIR/../notes.py --dir ."\n  $ xn 買い物\n  $ cat notes\n  買い物\n';
const files = { 'notes.py': notes, 'README.md': readme, 'tests/basic.t': test, 'requirements.txt': 'cram\n' };
const snap = snapshot(files, ['docs/large.md']);
const model = analyze(snap);

// 概要: 目的は作者の説明、入口と書き込みはコードの記述、処理の流れは推測として返す。
const o = overview(snap, model);
assert.equal(o.purpose[0].kind, 'author');
assert.ok(o.start.some(s => s.kind === 'code' && /__main__/.test(s.text)));
assert.ok(o.behaviors.some(b => b.function === 'Store.write' && b.kinds.includes('write')));
const flow = o.flows[0];
assert.equal(flow.kind, 'inference');
assert.ok(flow.steps.some(s => s.function === 'Store.write'), '入口から Store.write をたどれる');
assert.ok(flow.storage.some(e => e.function === 'Store.write' && /書き込/.test(e.label) && e.certainty === 'statement'));
assert.ok(flow.storage.some(e => e.certainty === 'name'), 'f.write は名前だけの候補として区別する');
assert.ok(o.unknown.some(u => /読み取っていないファイルが 1 件/.test(u.text)));
for (const c of o.components) for (const r of c.refs) assert.ok(r.end_line <= lineCount(files[c.path]), `${c.path} の参照が行数を超えない`);
for (const t of o.tests) for (const r of t.refs) assert.ok(r.end_line <= lineCount(files[t.path]));
assert.equal(lineCount('a\nb\n'), 2);
assert.equal(lineCount('a\nb'), 2);

// 質問: 保存先と、失敗したときの扱い。
const storage = inquire(snap, model, 'メモはどこに保存される？');
assert.ok(storage.intents.some(i => i.id === 'storage'));
assert.ok(storage.findings.some(f => /Store\.write/.test(f.text) && f.refs.some(r => r.path === 'notes.py' && r.line === 17)));
const failure = inquire(snap, model, '保存できなかったらどうなる？');
const write = failure.functions.find(f => f.name === 'Store.write');
assert.ok(write, '失敗の質問で Store.write を詳しく調べる');
const raised = write.raised.find(r => r.exception === 'BadFile');
assert.ok(raised.handlers.some(h => h.function === 'main' && h.line === 30));
assert.equal(raised.handled.kind, 'inference');
assert.ok(failure.limits.some(l => l.kind === 'inference'));

// 関数の詳細: 引数・呼び出し元の候補・関係する設定・テストでの言及。
const detail = functionDetail(model, 'notes.py', 14);
assert.equal(detail.found, true);
assert.deepEqual(detail.parameters, ['text']);
assert.ok(detail.name_callers.some(c => c.function === 'main' && /受け手の型は確かめていない/.test(c.reason)));
const init = functionDetail(model, 'notes.py', 11);
assert.ok(init.name_callers.some(c => c.function === 'main' && /__init__ が呼ばれる候補/.test(c.reason)));
assert.ok(!init.name_callers.some(c => /^\s*class\s/.test(c.code)));
assert.equal(functionDetail(model, 'docs/large.md', 1).found, false);

// 試験準備: 入口・オプション・テストの例。案は決定ではない。
const p = plan(snap, model);
assert.equal(p.executable, true);
assert.ok(p.entries.some(e => e.entrypoint === 'notes.py' && e.runtime === 'python3' && e.supported));
assert.ok(p.options.some(o => o.flags.includes('--dir') && o.metavar === 'DIR'));
assert.ok(p.examples.some(e => e.command === 'xn 買い物' && /notes\.py/.test(e.alias)));
assert.ok(p.test_commands.every(c => c.runnable === false && c.reason));
assert.match(p.origin, /決定ではない/);
assert.ok(p.faults.some(f => f.id === 'network_unavailable' && /bubblewrap/.test(f.applicable)));

// 実行が未対応の言語。
const go = snapshot({ 'main.go': 'package main\n\nfunc main() {}\n', 'README.md': '# hello\n\nGoで書いた架空のコマンドです。説明のための長めの文章です。\n' });
const goPlan = plan(go, analyze(go));
assert.equal(goPlan.executable, false);
assert.match(goPlan.unsupported_reason, /Go の実行は未対応/);
assert.ok(overview(go, analyze(go)).languages.some(l => l.name === 'Go' && !l.executable));

// 解析しない種類のファイルは、本文を読まないが、未読として示す。読んだファイル数だけを見せて未読を0件にしない。
const goUnread = snapshot({ 'README.md': '# hello\n\nGoで書いた架空のコマンドです。\n' });
goUnread.inventory.push({ path: 'cmd/run.go', size: 40, mode: '100644', sha: '0'.repeat(40) });
const goUnreadView = overview(goUnread, analyze(goUnread));
assert.deepEqual(goUnreadView.read.unread.map(u => u.path), ['cmd/run.go']);
assert.match(goUnreadView.read.unread[0].reason, /解析に対応していない種類/);
assert.ok(goUnreadView.unknown.some(u => /読み取っていないファイルが 1 件/.test(u.text)));

console.log('understanding-tests: ok');
