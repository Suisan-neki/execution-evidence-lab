# pynetdicomでの検証記録（#137）

確認日：2026年10月16日。ブランチ `feat/pinned-deps-multiprocess` の理解と試験の画面（`cargo run -- app`）で、公開GitHubのpynetdicomを試した。操作はCodexが `tools/dicom-smoke.mjs` で行った。実際の運用担当・開発者による評価ではない。

## 環境

macOS 26.3.1（Apple M4）、Rust 1.99.0、Node.js 24.10.0、Python 3.14.5、Playwright 1.62.1とChromium（headless shell 1243）。実行方式はlocal（OSによる隔離なし）。GitHubには認証なしで接続した。

## 対象

| 役割 | リポジトリ | コミット |
| --- | --- | --- |
| 対象 | [pydicom/pynetdicom](https://github.com/pydicom/pynetdicom) v3.0.4 | `2692cb93222c82c872ec8ecc8a426f1c9d86bc46` |
| 依存先 | [pydicom/pydicom](https://github.com/pydicom/pydicom) v3.0.2 | `20aa4b73aef76bffb6fbc46a8827588dfa9bb89d`（読み込み先 `src`） |

試験の画像は、pynetdicomのテスト用ファイル `pynetdicom/tests/dicom_files/CTImageStorage.dcm`（架空の患者ID `1CT1`）を使った。

## 試した内容

期待の例は2件。「送った画像が受信側に保存され、後から読み出せる」「保存できなかったときは、送った側に失敗と分かる」。どちらも受信側が保存する `pynetdicom/apps/common.py` の633行目（`ds.save_as`）に対応づけた。

| 役割 | 起動 |
| --- | --- |
| 待つプロセス（受信側） | `python -m pynetdicom storescp {{port}} -ba 127.0.0.1 -od received` |
| 操作するプロセス（送信側） | `python -m pynetdicom storescu -v 127.0.0.1 {{port}} {{source}}/pynetdicom/tests/dicom_files/CTImageStorage.dcm` |
| 後からの読み出し | pydicomのscripts `pydicom show <保存先>` |

保存先は `received/CT.1.3.6.1.4.1.5962.1.1.1.1.1.20040119072730.12322`。

## 結果

| 試行 | 観測 | 判定 |
| --- | --- | --- |
| 通常時 | 受信側は約0.4秒で準備できた。送信側は終了コード0、出力に `Status: 0x0000 - Success`。保存先のファイルがあり、`pydicom show` が `Patient ID ... '1CT1'` を表示した | 条件を満たす |
| 保存先のディレクトリを書き込み不可 | 受信側は `Permission denied` を出力し、0xA700を返した。送信側は出力に `0xA700 - Failure` を出したが、終了コードは0。保存先に患者IDを含むファイルはなかった | 条件を満たさない。4つの条件のうち「終了コードで失敗が分かる」だけが期待と異なった |
| 受信側を起動しない | 送信側は終了コード1。保存先に患者IDを含むファイルはなかった | 条件を満たす（条件は「終了コードが0以外」と「保存先に患者IDがない」の2つ） |

送信側の終了コード0は、保存の成功を意味しなかった。保存を確かめたのは、基盤による保存先の読み出しと、別のプロセスによる読み出し。

同じ試験は、GitHub Actions（Ubuntu）でもコミット `0b4712f` で同じ結果になった（[実行記録](https://github.com/Suisan-neki/execution-evidence-lab/actions/runs/38879861096)）。

生成した配布情報は `pynetdicom` と `pydicom` の2件で、記録に残った。試行の記録は試験用の一時ディレクトリに保存し、既存の `cases/` と `data/` は使っていない。

## そのほかの確認

| 確認 | 結果 |
| --- | --- |
| `cargo fmt --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings` | 成功 |
| `cargo test` | 成功。`tests/multi_process.rs` の架空の対象（`example/receiver`・`example/storekit`）で、通常時、書き込み不可で終了コード0のまま失敗すること、待つプロセスを起動しない故障、準備できない待つプロセスが判定できないになること、依存先の取得漏れ・存在しない起動先・`{{port}}` のない引数の拒否を確かめた |
| `npm run test:reader`、`npm run test:ui`、`npm run test:app` | 成功（既存の画面に影響がないこと） |

## 確かめていないこと

- bubblewrap方式。待つプロセスのある試行は受け付けない。
- C拡張・ビルドが必要な依存先、Node.jsの依存先、依存先の依存先の自動取得。
- 実際のPACSや医療機関の環境との通信。今回は同じ端末のlocalhostだけ。
- 実際の運用担当・開発者がこの結果を理解できるか。
