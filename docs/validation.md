# 2026年10月6日の検証記録

10月7日に追加した共同レビューと実アプリ接続の結果は[10月7日の検証記録](validation-2026-10-07.md)。以下は10月6日に確認した対象版と結果として残す。

Linux x86_64、rustc 1.99.0（b940084d7、2026-09-28）、Node.js v24.19.0で実行した。今回のコードはPR #110でレビューする。まだマージ済みではない。

## 対象の識別

Rustの実装コミットは`f343fb1d135939edbfdaeda686516f90160c94b3`。実行時は元のHEAD `ddb237260238d7345d80ffcb64a3d64f7db1754f`に未コミットの変更がある状態だった。ビルド対象の内容を識別する指紋は`fnv1a64:eca9cab99f634245`。指紋は署名や改ざん検知ではない。

アダプターの実装コミットは`f928d03aa723ae93884b25d34b4425c2bae85b88`。ファイルのSHA-256は`8ed245aa178019ca3d555f412a798520c89d1dc20565338e312e0d8a900c42c2`。対象バックエンドは固定コミット`218329b7a1409539c6c581990bbbb4ed9dd4175c`の必要なファイルだけを取得し、Git blob hashを照合した。完全なcheckoutや稼働環境との照合ではない。

## 実行した確認

| 確認 | 結果 |
| --- | --- |
| cargo fmt --check | 成功 |
| cargo check --locked | 成功 |
| cargo test --locked | 20件成功、失敗0件。入力検証3件と結合テスト17件 |
| cargo clippy --locked --all-targets -- -D warnings | 成功、警告0件 |
| cargo build --release --locked | 成功 |
| releaseバイナリのsuite | TCPを使った4条件を実行。下表と一致 |
| show / verify / retest / compare | 観測不足、独立した読み出し、前の試行へのリンクと条件変更を確認 |
| 外部プロセスの起動失敗・待機上限・終了 | 記録とUnknownを結合テストで確認 |
| node --test test/handler.test.mjs test/archiveUploadContract.test.mjs | 固定版の既存テスト23件成功、失敗0件。元リポジトリ全テストを実行したわけではない |
| VitalSensingローカルアダプター | 3条件をreleaseバイナリのcaptureから実行。試行IDが基盤とアダプターで一致 |
| 対象package.jsonに差分を入れた確認 | hash不一致で、元のハンドラーを読み込む前に拒否 |
| yomiyasuの静的点検 | 説明資料に指摘なし。条件や意味の対応も目視で確認 |

GitHub Actionsのワークフローを追加した。上表はこの実行環境での結果であり、GitHub Actionsの実行成功を示す表ではない。

## 4条件の実測

| 条件 | 観測からの判定 | 別経路の保存状態 | 確認した出来事 |
| --- | --- | --- | --- |
| normal | Satisfied | 入力と一致する1件 | 受信・保存成功・応答・成功表示 |
| disconnect | NotSatisfied | 保存物なし | TCPゲートで遮断。サーバーの受信記録なし |
| save-failure | NotSatisfied | 保存物なし | 受信後の人工的な保存失敗。成功表示なし |
| missing-observation | Unknown | 入力と一致する1件 | 応答・成功表示はあるが保存の観測記録なし |

別の試行・データ・送信試行の記録、クライアント側の成功だけを根拠にした判定、成功と失敗の食い違い、条件の変更、保存物の破損・別内容もテストした。Truthのファイルを書き換えても観測側の再判定が変わらないことを確認した。

保存失敗から正常条件へ再実行すると、前はNotSatisfied、後はSatisfiedになる。旧試行は上書きされず、previous_runが残る。compareは故障条件の変更を示し、旧証拠と現在の設定の一致をfalseにする。この確認はコード改修の効果を評価した結果ではない。

## 元のハンドラーへの適用

| ローカル条件 | 模擬索引更新の限定した判定 | 終了時のローカル索引 | 「管理者が保存データを閲覧できる」の判定 |
| --- | --- | --- | --- |
| normal | Satisfied | uploaded | Unknown |
| index-failure | NotSatisfied | issued | Unknown |
| missing-observation | Unknown | uploaded | Unknown |

模擬索引の更新と読み出しは実行した。S3へのPUT、実DynamoDBの更新・読み出し、iOS、管理画面、Cognitoは実行していない。外部プロセスの終了コード0を、指定した業務条件の成立へ読み替えない。

## 元の入力検証の確認

変更前の`ddb237260238d7345d80ffcb64a3d64f7db1754f`を別ディレクトリへ取り出した。cargo checkとcargo testは成功し、3件のテストが通った。cargo fmt --checkはmainの既存の整形差分で失敗した。今回のCLIへの変更後は整形確認も通った。旧版の失敗を成功として記録しない。

## 今回は確認していないこと

医療機関の利用条件、実環境への適用、端末と管理側の表示、電源断後の復旧、分散した複数試行・再送、計測の負担、既存手法との比較、構成要素を外す比較、他者による理解、新規性と有効性は未検証。関連する作業は全体計画#5に残す。
