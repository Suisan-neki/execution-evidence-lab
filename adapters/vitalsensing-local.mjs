// 元のバックエンドを読み込み、注入可能な依存先をローカルに置き換える。
// S3、DynamoDB、Cognito、端末、管理画面を再現するものではない。
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

const targets = JSON.parse(await readFile(new URL('./vitalsensing-targets.json', import.meta.url), 'utf8'));
const [backendArgument, fault = 'normal', revision = '218329b7a1409539c6c581990bbbb4ed9dd4175c', ...extra] = process.argv.slice(2);
const expectedBlobs = targets[revision];
if (!backendArgument || !expectedBlobs || extra.length || !['normal', 'index-failure', 'missing-observation'].includes(fault)) {
  throw new Error('usage: node vitalsensing-local.mjs /absolute/path/to/backend [normal|index-failure|missing-observation] [pinned-revision]');
}
const backend = resolve(backendArgument);
for (const [path, expected] of Object.entries(expectedBlobs)) {
  const bytes = await readFile(join(backend, path));
  const actual = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
  assert.equal(actual, expected, `固定版と異なるファイル: ${path}`);
}
const writeJson = (path, data) => writeFile(path, `${JSON.stringify(data, null, 2)}\n`, { flag: 'wx' });
let manifest;
try { manifest = JSON.parse(await readFile('manifest.json', 'utf8')); }
catch (error) { if (error.code !== 'ENOENT') throw error; }
const runId = manifest?.correlation.run_id ?? `local-${randomUUID()}`;
const correlation = manifest?.correlation ?? { run_id: runId, record_id: `${runId}:record-1`, attempt_id: `${runId}:attempt-1` };
const adapterSha256 = createHash('sha256').update(await readFile(fileURLToPath(import.meta.url))).digest('hex');
await writeJson('target.json', { correlation, repository: 'Medliss-share/vitalsensing', revision, expectedBlobs, adapterSha256, node: process.version, fault, scope: 'original handlers with injected local dependencies; no AWS/iOS/admin portal' });
const { createArchiveUploadTargetHandler, createArchiveUploadedHandler } = await import(pathToFileURL(join(backend, 'src/handler.mjs')).href);
const env = {
  ARCHIVE_BUCKET: 'vs-lab-dev-000000000000-ap-northeast-1',
  DEPLOYMENT_ID: 'lab', ARCHIVE_ENVIRONMENT: 'dev',
  ARCHIVE_KEY_PREFIX: 'vitalsensing/session-archives/',
  ENROLLMENT_TABLE: 'local-enrollments', ARCHIVE_INDEX_TABLE: 'local-index',
  MAX_ARCHIVE_BYTES: '1000', PRESIGNED_URL_EXPIRES_SECONDS: '900',
};
const identity = { studyId: 'lab-study', registrationRef: 'reg_0000000000000000', startDate: '2026-10-01', endDate: '2026-10-31' };
const body = { studyId: identity.studyId, registrationRef: identity.registrationRef,
  sessionId: 'synthetic-session', sessionDate: '2026-10-06', fileName: 'synthetic-session.zip',
  contentType: 'application/zip', contentLength: 512, checksumSHA256: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=' };
const events = [];
const record = (kind, detail) => events.push({ sequence: events.length + 1, correlation, kind, detail });
const localState = new Map();
const putIndexItem = async item => {
  if (fault === 'index-failure' && item.uploadStatus === 'uploaded') {
    record('index_failed', { key: item.s3Key, reason: 'uploadedへの更新開始前に人工的に失敗させた' });
    throw new Error('injected local index failure');
  }
  // 模擬索引。実DynamoDBの永続性・整合性の保証ではない。
  localState.set(item.s3Key, structuredClone(item));
  if (!(fault === 'missing-observation' && item.uploadStatus === 'uploaded')) {
    record('index_updated', { key: item.s3Key, status: item.uploadStatus });
  }
};
const issueTarget = createArchiveUploadTargetHandler({ env, putIndexItem,
  getParticipantIdentity: async () => identity,
  getSignedUploadUrl: async () => 'https://example.invalid/synthetic-upload' });
record('operation_started', '合成入力でアップロード先発行を呼び出す');
const reply = await issueTarget({ requestContext: { authorizer: { jwt: { claims: { sub: 'synthetic-subject' } } } }, body: JSON.stringify(body) });
assert.equal(reply.statusCode, 200);
const target = JSON.parse(reply.body);
record('upload_target_issued', { key: target.key, statusCode: reply.statusCode });
// 署名URLへPUTしていない。S3通知の入力だけを合成して元のハンドラーを呼ぶ。
const indexUpload = createArchiveUploadedHandler({ env, putIndexItem,
  deleteOversizedArchive: async () => { throw new Error('unexpected deletion: no external resource may be touched'); } });
const syntheticEvent = { Records: [{ eventTime: '2026-10-06T00:00:00.000Z', s3: { bucket: { name: env.ARCHIVE_BUCKET }, object: { key: target.key, size: 512, eTag: 'synthetic-etag' } } }] };
let failure;
try { await indexUpload(syntheticEvent); record('handler_completed', '合成S3通知の処理完了'); }
catch (error) { failure = error.message; record('handler_failed', failure); }
// これは本ハーネスの終了時スナップショット。AWSからの読み出しではない。
await writeJson('local-index.json', [...localState.values()]);
await writeFile('adapter-events.jsonl', `${events.map(event => JSON.stringify(event)).join('\n')}\n`, { flag: 'wx' });
const positive = events.filter(event => event.kind === 'index_updated' && event.detail.status === 'uploaded');
const negative = events.filter(event => event.kind === 'index_failed');
const verdict = positive.length ? 'satisfied' : negative.length ? 'not_satisfied' : 'unknown';
await writeJson('adapter-assessment.json', {
  correlation,
  condition: 'このローカル試行で、合成S3通知による模擬索引のuploadedへの更新を観測した',
  verdict, evidence: [...positive, ...negative].map(event => `adapter-events.jsonl#${event.sequence}`),
  unconfirmed: ['S3上の実アーカイブ', 'DynamoDBの状態', 'iOSでの送信済み表示', '管理ポータルの一覧と閲覧', '医療機関が必要とする利用条件'],
  next: verdict === 'unknown' ? '索引更新の観測箇所を追加して再試行する' : '試験専用環境で実保存先と管理側の閲覧を別経路から照合する',
});
// 観測判定を計算した後に、別経路でハーネスの保存物を読み出す。
const readback = JSON.parse(await readFile('local-index.json', 'utf8'));
const item = readback.find(item => item.s3Key === target.key);
await writeJson('adapter-truth.json', { correlation, key: target.key, uploadStatus: item?.uploadStatus ?? null, scope: 'local harness snapshot only' });
assert.equal(item?.uploadStatus, fault === 'index-failure' ? 'issued' : 'uploaded');
assert.equal(verdict, { normal: 'satisfied', 'index-failure': 'not_satisfied', 'missing-observation': 'unknown' }[fault]);
assert.equal(Boolean(failure), fault === 'index-failure');
console.log(JSON.stringify({ fault, verdict, localReadbackStatus: item.uploadStatus, record: 'adapter-assessment.json', scope: 'local dependency injection only' }));
