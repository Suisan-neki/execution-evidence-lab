# VitalSensingのバックエンドをローカルで試す

対象はMedliss-share/vitalsensingの固定コミット`218329b7a1409539c6c581990bbbb4ed9dd4175c`。今回の対象経路は、アップロード先の発行と、合成S3通知による索引更新。`adapters/vitalsensing-local.mjs`は元のハンドラーを読み込み、注入可能な依存先を置き換える。元リポジトリのソースをこのリポジトリへコピーしていない。

## 準備と実行

対象リポジトリを読める環境で固定版を取得し、backendで`npm ci --ignore-scripts`を実行する。スクリプトは対象の4モジュール、package.json、package-lock.jsonのGit blob hashを照合し、固定版と違えば起動を止める。Node.js v24.19.0で確認した。

```bash
git clone https://github.com/Medliss-share/vitalsensing.git
git -C vitalsensing checkout 218329b7a1409539c6c581990bbbb4ed9dd4175c
# vitalsensing/backendで実行する。
npm ci --ignore-scripts
```

実行記録の作業ディレクトリからも対象へ到達できるよう、次のパスを自分の絶対パスに置き換える。

```bash
cargo build
./target/debug/execution-evidence-lab capture \
  --condition "管理者が保存データを閲覧できる" \
  -- /absolute/path/to/node \
  /absolute/path/to/execution-evidence-lab/adapters/vitalsensing-local.mjs \
  /absolute/path/to/vitalsensing/backend normal
```

最後の引数を`index-failure`、`missing-observation`へ変えて新しい試行を作る。アダプターを直接起動するときは、出力先として空のディレクトリを作り、そこで起動する。既存ファイルへの上書きは拒否する。

## この実行で確かめること

認証済み利用者の識別情報、署名URL発行、索引の更新先を模擬の関数へ置き換える。S3へのPUTは行わない。元の`createArchiveUploadTargetHandler`でissuedの模擬索引を作り、元の`createArchiveUploadedHandler`へ合成S3通知を渡す。索引更新前の人工失敗と、更新の観測だけを省く条件を比較する。

| 条件 | 模擬索引更新の観測判定 | ローカルの終了時スナップショット |
| --- | --- | --- |
| normal | satisfied | uploaded |
| index-failure | not_satisfied | issued |
| missing-observation | unknown | uploaded |

target.jsonに対象版とファイルのhash、adapter-events.jsonlに出来事、adapter-assessment.jsonに限定した判定と根拠を残す。local-index.jsonはハーネスの終了時スナップショット。adapter-truth.jsonはそのファイルを別経路で読み出した答え合わせであり、DynamoDBから読み出した記録ではない。

Rust基盤のassessment.jsonは、指定した「管理者が保存データを閲覧できる」という問いに対して3条件ともUnknownになる。プロセス終了や模擬索引の更新だけで、その業務条件を確認したとは言えない。

## 残る適用範囲

実S3のアーカイブ、DynamoDBの永続性・整合性、Cognitoの認証、iOSの操作・送信済み表示、管理ポータルの一覧と閲覧は未再現。モックを使っていることは実行ごとのtarget.jsonにも記載する。稼働AWSと固定版の対応は今回独立に照合していない。

対象版と経路は#80、試験環境は#59・#81、独立した保存・表示の照合は#82、運用担当の利用条件は#68・#69・#94で追う。次は試験専用の保存先・認証・管理側の経路を定める。既知の保存への不安を試した結果を、未知の見落としを発見した件数に含めない。
