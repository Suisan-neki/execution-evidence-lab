# ブラウザ操作が未完了となった後の画面写真

## 変更の理由

main `c37e0a5` の [OpenEMR SOAP CI](https://github.com/Suisan-neki/execution-evidence-lab/actions/runs/39211505145) は、通常時の15項目とDB停止時の反例確認後、新しい試験環境の患者表示待ちで停止した。確認ボタン直後の写真では、15秒の待機後の表示を説明できない。これは調査用の観測を補う変更で、患者作成が停止する根本原因の修正ではない。

## ローカルで確認したこと

`npm run test:browser` は架空のWebアプリをlocalhostだけで動かし、実際のブラウザ実行基盤を別プロセスから呼ぶ。

- 要素が現れないまま操作の待機上限に達した後も、最大1秒でPNGを取得し、手順の記録へ対応づけた。操作は未完了のまま、後続の保存操作は実行しなかった。
- 準備中に要素が見つからない場合も停止後の写真を残し、試験操作へ進まなかった。
- 秘密入力が失敗した場合は写真を省き、JSONに秘密の値を残さなかった。
- 正常時の表示・HTTP対応づけ、DBに値が残らない反例、外部への要求の遮断、canvas観測、ブラウザ版違いの拒否という既存の確認も通過した。

`node tools/browser-evidence-smoke.mjs` はRustの結果画面へ合成の未完了記録を渡し、未完了の手順の写真を開閉しても結果が変わらないこと、写真欠落時の案内、1440pxと390pxでの表示を確認した。画面写真は `test-results/browser-evidence-*.png`。これは実参加者による操作性評価ではない。

Macでは導入済みPlaywright 1.62.1とChromium `153.0.8010.12`の実体を環境変数で指定した。デフォルトで探すChromiumのキャッシュは存在せず、初回起動は失敗した。新規インストールや依存先の変更はしていない。固定ブラウザ版を使う実対象のCIと、この合成試験を区別する。

Rustへの埋め込みを含め、`cargo build --locked --offline`、`cargo fmt --check`、`cargo check --locked --offline`、`cargo test --locked --offline`、`cargo clippy --locked --offline --all-targets -- -D warnings` が成功した。MacのRust試験は125件成功、Docker依存の4件は除外。変更したJavaScriptの構文検査と`git diff --check`も成功した。

## 未確認のこと

MacのDockerは接続できず、OpenEMRの全体再現はローカルで実行していない。固定したGit参照のダイアログ実装を読んだが、起動イメージ内の実装と一致する証明はない。失敗の原因をタイミング依存と断定しない。写真は撮影時点の表示であり、保存・DBの値・医療業務上の安全性の証明にしない。写真を取得できない場合も、操作の結果を成功へ変更しない。
