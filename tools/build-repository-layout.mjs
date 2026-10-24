// 実行時のNode.jsや外部CDNを使わず、動的な構成図をローカルで配置する。
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { gzipSync } from 'node:zlib';
import { resolve, join } from 'node:path';
const root = resolve(import.meta.dirname, '..'), pkg = join(root, 'node_modules/elkjs');
const { version } = JSON.parse(await readFile(join(pkg, 'package.json'), 'utf8'));
if (version !== '0.12.0') throw Error('ELKの固定バージョンを確認してください');
const out = join(root, 'ui/repository-generated');
await mkdir(out, { recursive: true });
const code = await readFile(join(pkg, 'lib/elk.bundled.js'));
await writeFile(join(out, 'elk.js.gz'), gzipSync(code, { level: 9, mtime: 0 }));
await writeFile(join(out, 'LICENSE-ELK.md'), await readFile(join(pkg, 'LICENSE.md')));
console.log(JSON.stringify({ elk: version, bytes: code.length }));
