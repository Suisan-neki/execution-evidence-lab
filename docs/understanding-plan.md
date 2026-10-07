# 実装した後の3日間

実装期間では動作がつながる範囲まで進める。理解期間では、実行した結果から必要なコードへ戻る。全行を最初から順番に読む必要はない。ここで挙げる試し方は説明用で、実装を止める通過試験ではない。

製品として提供する「理解する」と、本人が実装を読むこの3日間は別のもの。今回の製品の入口は[理解とテストの入口](repository-understanding.md)。公開GitHubを読む場合は`cargo run -- explore`で起動し、ui/explorer.html → ui/explorer.js → ui/scanner.js → src/review.rsの順で、構成・コード・出所の対応を確認する。以下は既存のテスト基盤を読む手順。

## 1日目：操作から保存・表示まで

今回追加したテスト画面は`cargo run -- case init cases/review`、`cargo run -- review cases/review`で起動し、表示されたURLの`/review`で開ける。事例作成済みなら初期化は省く。まず画面から正常と観測欠落を試し、確認事項と根拠の行を開く。画面のHTML・表示・操作は`ui/review.html`、`review.css`、`review.js`、HTTP入口は`src/review.rs`、保存処理の呼び出しは`workspace::run_demo`にある。Nodeは画面の実行には不要で、自動試験に使う。

まず`cargo run -- demo`を実行する。保存結果と試行ディレクトリを確認し、`src/main.rs`のdemoから`src/experiment.rs`のrun_trial、serveをたどる。データと識別子の形は`src/model.rs`、TCPの読み書きは`src/protocol.rs`にある。

```rust
// run_trialで発行した同じ識別子を要求に入れる。
let request = Request { correlation, data: config.data };
// TCPに書けたことは、サーバーが保存できたこととは別。
write_frame(&mut stream, &request)?;
// 保存処理を終えたサーバーの応答を読み、識別子を照合する。
let reply: Reply = read_frame(&mut stream)?;
// Savedの応答を受けた場合だけ、保存処理完了の表示を記録する。
```

`?`はこの処理をResultで返し、失敗を呼び出し元へ伝える。所有権が移るRequestと、スレッドへ渡すコピーを区別して読む。Recorderのcloneは同じ記録ファイルを共有し、データを別の試行へ複製する操作ではない。

名前や年齢を変えて再実行し、stored.jsonの内容と一致することを確認する。続けてsave-failureを実行し、受信した記録は残るが成功表示が出ない位置を見る。

## 2日目：根拠と答え合わせを分ける

共同レビューでは`src/workspace.rs`の`Check`、`Evidence`、`observed`、`view`の順に読む。条件の版、観測時の結果、現在の条件に使えるかを区別する。質問を追加した場合とコードの宣言を変更した場合を試し、理由と再確認の提案を見る。`tests/workspace.rs`は最新の不足を古い成功で埋める誤り、別キーの索引更新、条件変更後の回答の流用を防ぐ例になっている。

`cargo run -- suite`で4条件を動かす。missing-observationの試行を`show`で読み、続けて`verify`で保存物を読む。「判定できない」と「実際には入力と一致する保存物がある」が同時に成立する理由を、src/assessment.rsから確認する。

```rust
// 判定に使うのは、試行条件と観測した出来事。
let assessment = assess(&manifest, &events);
// 保存物を別経路で読む。assessmentへは渡さない。
let truth = verify_storage(&directory, &input);
```

次にsrc/recording.rsを見る。JSONLの各行が何を示すか、同じ識別子を伝える理由、Mutexで書き込みを直列化する理由、ファイルを上書きしない位置を確認する。イベント順序は記録した順であり、全ての因果関係の証明ではない。

tests/system.rsのmissing_save_observation、another_run_record_or_attempt、conflicting_observationsのテストを読む。どの誤判定を防いでいるかを、実行記録と対応させる。

## 3日目：再検証と実アプリへの接続

画面から人の判断を記録し、依存先の宣言を変えて再試行する。`workspace::change`と`save_revision`で、以前の版と判断当時の根拠が残る位置を見る。`src/evaluation.rs`と`case export`で3種類の比較資料を確認する。同じ入力情報であることと、他者評価を実施したことは別。操作を通す自動確認は`tools/review-smoke.mjs`、実ハンドラーへの接続は`tools/adapter-smoke.mjs`にある。

保存失敗の試行を`retest <run-dir> --fault normal`で再実行する。previous_runとcompareの変更一覧を読み、古い試行が残ることを確かめる。コードや条件が変わった場合、前の成功結果だけでは新しい条件を保証できない。

src/capture.rsと[ローカル適用手順](vitalsensing-local.md)を読む。元のVitalSensingハンドラーを呼ぶ部分と、認証・署名URL・索引を代替する部分を分けて見る。ローカルのuploadedと、医療機関の管理者が実データを閲覧できることには、未確認の処理が残っている。

修正候補は、困った操作、該当ファイル、期待する動作、追加する確認の4点でIssueへ残す。コードを手で変えてよい。変更に応じたテストを実行し、PRへ反映する。理解していない箇所を無理に採用済みへ変えない。

3日間を終えたら、#68・#69の利用条件、#81の試験環境、#86・#108の比較と理解評価のどこへ進めるかを決める。小さなアプリの結果を研究全体の達成結果へ読み替えない。

## 10月7日の継続を読む順番

1. ui/review.jsのrender：現在の件数、未確認範囲、古い根拠、再描画前の下書きと条件選択。
2. src/workspace.rsのobserved：発行キーと更新記録の順序、対象不明の失敗の扱いと根拠の行。
3. src/evaluation.rsのcommon_inputs：判断当時の自動説明を比較入力から外し、人の判断理由を残す理由。
4. tools/reproduce-methods.mjs：同じ元情報の別実装、実TCP試行と合成加工、未宣言の依存先の限界。
5. docs/general-release-plan.md：現在の入口、汎用版の未完了項目、医療特化と共通部分の分け方。
