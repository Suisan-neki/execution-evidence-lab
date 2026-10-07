# execution-evidence-lab

「技術者と非技術者で目線の高さを合わせる」「技術者が安心して世に出せる」を目指す研究用の検証基盤。初期の対象は医療機関の運用担当と開発者。期待する動作、確かめた範囲、まだ分からないことを同じ記録で確認し、公開・導入の判断理由を残す。条件の具体化から試行、改修、再検証までを支える手法を検討する。

11月に一度汎用版を完成させ、利用者の確認と医療特化版の開発を並行する方針。[完成条件と分割の案](docs/general-release-plan.md)を記載した。現在はローカルの試作で、汎用版の完成・配布前。

現在の試作は、確認事項・出所・未回答、試行と根拠、条件変更による再確認、人による判断をローカルのレビュー画面でつなぐ。画面から架空データ1件のTCP送信・保存を4条件で試せる。固定版のVitalSensingバックエンドをローカルの代替依存先で動かすアダプターもある。研究の新規性、他者の理解や安心感への効果、医療機関での有効性は未検証。

## 共同レビューを動かす

```bash
cargo run -- case init cases/review
cargo run -- review cases/review
```

表示されたlocalhostのURLを開く。記入者と理由を入力し、正常・保存前の人工失敗・観測の欠落・遮断を試す。「保存処理の成功」と「必要な担当者による後の閲覧」は別の条件として表示する。認識・出所、未回答への回答、人が決めた行動を記録し、条件・依存先の変更後に再試行できる。

```bash
cargo run -- case show cases/review
cargo run -- case export cases/review --output evaluations/review-1
```

練習用の条件・質問はCodexの未採用の案。本人や医療機関の発言ではない。画面は同じ端末でのレビュー用で、外部共有や実際の公開は行わない。[共同レビューの仕様と限界](docs/shared-review.md)、[水木の実装記録](docs/sprint-2026-10-07.md)に手順と詰まった箇所を記載した。

## 自分の条件で事例を始める

```bash
cargo run -- case init cases/general --template general --id general-001 --title "通知の確認" --actor 作成者 --reason 新しい事例を始める
cargo run -- review cases/general
```

空の事例から条件・出所を追加する。初期条件や回答を自動で作らない。追加した条件の判定方法は未定義なので、外部プロセスの終了だけでは成立にしない。一般の観測入力と判定規則の拡張は残る作業。

[確認手順と反例の再現](docs/method-reproduction.md)は`cargo build --locked`後に`npm run test:methods`で実行できる。13事例で通常のチェックリストの参照実装と照合する。利用者評価や研究手法の優位性は未検証。

## 実行する

Rust 1.99.0、cargo、rustfmt、clippyを使用して確認した。外部アダプターにはNode.jsと対象バックエンドが別途必要。

```bash
cargo run -- suite
cargo run -- demo --fault missing-observation
cargo run -- show runs/<run-id>
cargo run -- verify runs/<run-id>
cargo run -- retest runs/<run-id> --fault normal
cargo run -- compare runs/<before-id> runs/<after-id>
```

`<run-id>`は実行時に表示されたディレクトリ名へ置き換える。`demo`では`--name`、`--age`、`--output`も指定できる。名前が空、年齢が150を超える入力は拒否する。既存コードの入力条件を保ったもので、医療上の妥当性を表す基準ではない。

| 条件 | 観測からの判定 | 別経路で読み出した保存状態 |
| --- | --- | --- |
| normal | 条件を満たす | 入力と一致する1件がある |
| disconnect | 条件を満たさない | 保存物がない |
| save-failure | 条件を満たさない | 保存物がない |
| missing-observation | 判定できない | 入力と一致する1件がある |

ここでの条件は「この試行のデータについて、サーバーのファイル書き込みと同期処理が成功した」。後の閲覧、クラッシュ復旧、医療業務での利用可否は含まない。

## 試行の記録

毎回新しい`runs/run-.../`を作り、既存の試行を上書きしない。

| ファイル | 内容 |
| --- | --- |
| manifest.json | 入力、故障条件、初期状態、観測範囲、基盤のコード識別情報、前の試行 |
| events.jsonl | 試行・データ・送信試行の識別子を伝えた出来事と順序 |
| assessment.json / report.md | 観測から言える判定、根拠の行番号、未確認事項、次の確認 |
| stored.json | 再現用アプリの保存物。保存失敗・遮断時には作られない |
| truth.json | 評価側が別経路で読み出した結果。判定器へ渡さない |
| complete.json | 記録出力の完了。存在しない試行は出力途中の可能性がある |

`show`は保存ログから再判定し、`truth.json`を使わない。`verify`は現在の保存物を再度読み出す。`suite`と`demo`のCLIは開発者向けの答え合わせとして両方を表示する。他者評価では`show`と`report.md`を用い、答え合わせの情報を別に管理する。

## 外部アプリへつなぐ

```bash
cargo build
# コマンドの出力・終了を記録する。シェルは自動では挟まない。
./target/debug/execution-evidence-lab capture --condition "管理者が保存データを閲覧できる" -- /absolute/path/to/program arg1
```

`capture`は作業ディレクトリを新しくし、stdout・stderr・終了・待機上限を記録する。外部ファイルやDB、環境変数を初期化する隔離機能はない。終了コード0でも業務条件は「判定できない」とする。待機上限を超えると直接の子を停止・回収するが、子孫プロセス全体の停止は保証しない。

VitalSensingの実コードへの接続方法、固定版、再現できる範囲は[適用手順](docs/vitalsensing-local.md)に記載した。

## 確認と読む順番

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
# 画面の自動確認。Node.js 22以上とブラウザが必要。
cargo build --locked
npm ci --ignore-scripts
npx playwright install --with-deps chromium
npm run test:ui
```

実行結果は[検証記録](docs/validation.md)、試作用の選択は[判断理由](docs/implementation-decisions.md)に残す。[3日間の理解計画](docs/understanding-plan.md)に沿って、送受信、記録と判定、実アプリへの適用の順で読める。

2026年10月6日から、2日間ほど実装を進め、3日間ほど理解・レビュー・修正に使う方針へ変更した。Codexは関連するIssueをまとめて実装・検証し、PRを提出する。マージは本人の指示で行う。[協働手順](AGENTS.md)と[研究・開発ロードマップ](docs/roadmap.md)にも反映した。
