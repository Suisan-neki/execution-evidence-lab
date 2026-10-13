# 2026年10月7日の検証記録

PR #110、実装期間 #121。Linux x86_64、Rust 1.99.0、ローカルのNode.js v24.19.0で確認した。共同レビューのブラウザ操作はGitHub ActionsのNode.js v22.23.3とChrome Headless Shell 151.0.7922.34で確認した。PRは未マージ。

## 対象と実行条件

実装コミットは`f68b3e322c670d5a9234899a5d358af11ee75841`、画面の試験修正・説明資料は`066173de9f3404c7465ed0e2683258318fdd895d`。基盤・UI・アダプターのソース指紋は`fnv1a64:734ff63259fe5f38`。変更確認用で、署名や真正性を保証する値ではない。

最初のアダプター実行時は前のHEAD `b6c57f72caf9e04e2635760d2e5760cafe853e4a`に未コミットの変更がある状態だった。後のrelease実行は066173deのクリーンな状態からビルドした。同じソース指紋がmanifestに残った。NodeアダプターのSHA-256は`e4a63e1edf4ac16619fb4d0f32881a591f95ab27ab0f77c998b9ab38d581a6cf`。

VitalSensingの対象は旧版`218329b7a1409539c6c581990bbbb4ed9dd4175c`と、今回dev-masterで確認した`2bc818b2a8e7002154ab5c29524bb03315f00b42`。必要なファイルを取得し、アダプターが対象の4モジュールとpackage/lockのblob hashを照合した。完全なcheckout、稼働環境のコード版との照合ではない。

## 実行した確認

| 確認 | 結果 |
| --- | --- |
| cargo fmt --check / check --locked | 成功 |
| cargo test --locked | 31件成功。入力3、既存の結合17、共同レビュー・根拠更新・比較・HTTP11 |
| cargo clippy --locked --all-targets -- -D warnings | 成功、警告0件 |
| cargo build --release --locked | 成功 |
| releaseのsuite | 実TCPで正常・遮断・保存失敗・観測欠落。以前と同じ判定と読み出し結果 |
| 二つの固定版のadapter-smoke | 各3条件、計6試行。実ハンドラー→capture→事例への取り込み→比較資料出力が成功 |
| 旧版の選択した元テスト | handler、archiveUploadContract、enrollment、oversizedArchiveCleanupの4ファイル42件成功 |
| 今回の版の選択した元テスト | 上の4ファイルにarchiveIndex、presignLength、templateSafetyの3ファイルを加え、計7ファイル59件成功 |
| Chromeを使った画面操作 | GitHub Actionsで成功。下記の動作を実際のHTTPと記録まで確認 |
| 元情報をそろえた比較出力 | 3資料の共通入力一致、答え合わせ情報の不混入、回答欄は未記入、出力先の上書き拒否 |

元のバックエンド全テストを実行したとは扱わない。旧版のテストの最初の実行は取得していなかったtemplate.yamlを読む箇所で失敗し、同じ固定版のtemplateを取得した後に42件を再実行して成功した。

## 画面と記録の確認

4条件の試行、条件ごとの3値判定、同じ観測の行へのリンク、HTMLを文字として扱う認識記録、二つの合成の立場による異なる判断と理由、古い画面からの更新拒否、新しい質問と回答の保存、依存先の変更による失効、再試行、再読み込み後の記録、390px幅での横溢れがないことを確認した。

二つの立場は自動試験で入力した役割。実際の開発者と運用担当による評価、本人の理解や安心感を確認した結果ではない。

ローカルのブラウザ起動は実行環境のsocket制約で失敗した。最初のCIは再描画で閉じた入力欄が表示されるまで待つ試験の書き方で失敗した。保存済みの版を待つ形へ修正して再実行し、成功した。失敗したCIは[run 37566753876](https://github.com/Suisan-neki/execution-evidence-lab/actions/runs/37566753876)、成功した066173deのCIは[run 37567083704](https://github.com/Suisan-neki/execution-evidence-lab/actions/runs/37567083704)。スクリーンショットは同CIの合成事例の成果物で7日間保存する。

## 元ハンドラーの実行結果

| 固定版 | 条件 | 限定した索引更新の判定 | ローカルの読み出し | 管理者による閲覧 |
| --- | --- | --- | --- | --- |
| 218329b / 2bc818b | normal | Satisfied | uploaded | Unknown |
| 218329b / 2bc818b | index-failure | NotSatisfied | issued | Unknown |
| 218329b / 2bc818b | missing-observation | Unknown | uploaded | Unknown |

条件ごとに新しい試行を作った。取り込む情報はtarget/eventsと実行条件。adapter-truth・保存スナップショット・事前計算したassessmentは取り込みの判定に使わない。取り込み後に元記録を変えても保存済みの根拠が変わらないことは、別の合成契約試験で確認した。

別キー、キー不明、固定情報の不一致、別の試行、観測の食い違い、古い条件、最新の観測不足を成功にしないことも契約試験で確認した。これは改ざんされた記録の真正性を保証する確認ではない。

## 未実施

実AWS・Cognito・iOS・管理画面、稼働版との照合、実CASE-001でのヒアリング→改修→再検証、未知の見落とし、依存先の自動推定、計測・準備の負担、有力な既存手法の再現比較、構成要素を外すアルゴリズム比較、実参加者の理解・安心感、本人の実装力、新規性と有効性は未検証。次の条件と作業は[水木の実装記録](sprint-2026-10-07.md)。

## 同日の継続実装

開始時のPR #110のHEADはb5459bb。作業コピーは未コミット変更なしで、既存のコードとPR・Issue #121を照合してから継続した。

| 確認 | 結果 |
| --- | --- |
| Rust 1.99.0 / Linux x86_64 | fmt/check/test/clippy/build成功。Rust33件 |
| tools/reproduce-methods.mjs | 13事例成功。通常のチェックリストの参照実装と今回の判定が一致 |
| 二つの固定版の実ハンドラー | 218329b、2bc818bで各3条件成功。模擬索引のみで、業務条件はUnknown |
| 汎用の事例作成 | CLIで空の条件・発言・依存先を確認。条件追加後はUnknownの契約試験も成功 |
| ブラウザ | ローカルのChromium取得は失敗。実装f5b15ceの[CI](https://github.com/Suisan-neki/execution-evidence-lab/actions/runs/37578576163)でChromeの追加操作も成功 |

追加した契約試験は、対象発行前の成功、キー不明の失敗、比較の共通入力へのdecisions.basis混入、空の汎用事例を対象にする。既存の実TCP試行と区別して合成の反例を扱う。比較結果の要約は[確認手順の再現](method-reproduction.md)。

今回のUI試験へ、未保存の回答と選択した条件の保持、確認状況と観測の限界、古い根拠の表示を追加した。f5b15cea1a7e4b48fa23faf0460b60b52d80a4feのCIでは全項目が成功した。デスクトップ1440pxとスマートフォン幅390pxの画像を取得して、状況一覧・古い根拠の注意・未確認範囲の表示と折り返しを目視で確認した。

実参加者、実AWS・認証・iOS・管理画面、医療業務の利用要件、同じ作業予算での有力な既存手法との実測比較、研究上の優位性は保留。13事例の回帰試験から効果を推定しない。
