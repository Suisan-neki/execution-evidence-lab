# execution-evidence-lab

「技術者と非技術者で目線の高さを合わせる」「技術者が安心して世に出せる」を目指す研究用の検証基盤。初期の対象は医療機関の運用担当と開発者。期待する動作、確かめた範囲、まだ分からないことを同じ記録で確認し、公開・導入の判断理由を残す。条件の具体化から試行、改修、再検証までを支える手法を検討する。

11月に一度汎用版を完成させ、利用者の確認と医療特化版の開発を並行する方針。[完成条件と分割の案](docs/general-release-plan.md)を記載した。現在はローカルの試作で、汎用版の完成・配布前。

提供する機能は「理解」と「テスト」。理解では公開GitHubのコードを読み取り、構成・入口・関数・参照先をコードの根拠とともにたどる。関数の入力・分岐・呼び出し・戻りの記述を読み、別の版とファイルを比較できる。呼び出しの別名と読み込み宣言から候補を示し、候補の宣言と呼び出し元へ移動できる。テストでは対象の処理を実行して期待する動作・失敗・未確認事項を確かめる。理解した処理と試験結果を同じコードへ対応づける。

現在の理解画面は公開リポジトリの限定した読取と簡易解析の試作。AIによる意味の説明や完全な呼び出し解析は未実装。テスト基盤は架空データ1件のTCP送信・保存の4条件と、固定版VitalSensingの限定したローカル試験に対応する。PR #126では、Node.js/Pythonの固定版と明示した観測規則を使う外部試験を追加した。任意のリポジトリの自動起動・自動計測には未対応。研究の新規性、他者の理解や安心感への効果、医療機関での有効性は未検証。

## 理解から試験・再試験までを一つの画面で行う

```bash
cargo run -- app data/app --execution-mode local
```

表示された `http://127.0.0.1:8765` を開き、公開GitHubのURLと、必要ならブランチ・タグ・コミットを入力する。画面は4つの手順に分かれる。

1. ソフトウェアを理解する。目的・起動・構成・処理の流れを、コードの根拠とともに表示する。質問や関数名から、関係する行を調べられる。
2. 期待する動作を確認する。期待を書き、実装のコードと対応づけ、運用担当と開発者がそれぞれ確認を残す。
3. 実際に試す。正常な試行と、書き込み不可などの実際の故障を入れた試行を、利用者が押したときだけ実行する。
4. 結果を確認する。条件ごとの期待と観測、判定、確かめていないことを表示する。修正後のコミットで再試験し、前後を比べる。

URLを入力しただけでは対象のコードを実行しない。実行できるのは、Node.jsとPythonの対象。Pythonでは、固定コミットで指定した依存先と、受信側のように先に起動して待つプロセスを使える（[pynetdicomでの検証](docs/validation-dicom-2026-10-16.md)）。依存先のインストールやビルドは行わない。ダイジェスト付きのコンテナで OpenEMR と Orthanc を試験専用に起動できる（[コンテナでの検証](docs/validation-container-2026-10-16.md)）。Dockerが使えないときはほかの方式へ切り替えない。`--execution-mode local` はOSによる隔離ではないため、信頼できない対象には使わない。使い方・判定の規則・環境変数は[理解と試験の画面](docs/understand-and-test.md)、画面の部品は[デザインシステムの適用](docs/design-system.md)。

## GitHubのコードを理解する

```bash
# PR #110・#126はmainへマージ済み。
git switch main
git pull --ff-only
cargo run -- explore
```

表示されたURLを開き、公開GitHubのリポジトリURLを入力する。固定したコミットの説明・起動設定・コードを確認できる。詳細は[理解とテストの入口](docs/repository-understanding.md)。

## 理解とテストの記録を同じ画面から使う

```bash
cargo run -- case init cases/review
cargo run -- review cases/review
```

表示されたlocalhostのURLを開く。「理解する」と「テストする」を切り替えられる。理解画面で選んだコードから確認事項を作り、テスト結果から固定版の理解画面へ戻れる。別の版と比較して参照先を更新すると、古い結果は履歴に残り、現在の条件には再確認が必要になる。テスト画面では記入者と理由を入力して正常・保存前の人工失敗・観測欠落・遮断を試す。「保存処理の成功」と「担当者による後の閲覧」は別の条件として表示する。条件・依存先の変更後は再試行できる。

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

空の事例から条件・出所を追加する。初期条件や回答を自動で作らない。追加した条件の判定方法は未定義なので、外部プロセスの終了だけでは成立にしない。言語に依存しない報告観測の取り込みと、API・CLIによる比較・全履歴のバックアップと復元に対応する。[汎用バックエンドv1](docs/backend-api.md)に操作と限界を記載した。

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
npm run test:reader
npm ci --ignore-scripts
npx playwright install --with-deps chromium
npm run test:ui
# 理解と試験の画面の自動確認。GitHubの公開リポジトリ sjl/t を読み、試行を4回実行する。
npm run test:app
```

実行結果は[検証記録](docs/validation.md)、試作用の選択は[判断理由](docs/implementation-decisions.md)に残す。[3日間の理解計画](docs/understanding-plan.md)に沿って、送受信、記録と判定、実アプリへの適用の順で読める。

2026年10月6日から、2日間ほど実装を進め、3日間ほど理解・レビュー・修正に使う方針へ変更した。Codexは関連するIssueをまとめて実装・検証し、PRを提出する。マージは本人の指示で行う。[協働手順](AGENTS.md)と[研究・開発ロードマップ](docs/roadmap.md)にも反映した。

## 固定版の外部コードをテストする

```bash
git fetch origin
git switch main
git pull --ff-only
cargo build --locked
npm run test:external
```

合成のGitリポジトリでコードの改修と再試験を通し、正常・失敗・観測欠落・食い違いを同じ事例に残す。画面では、コード参照のある条件から観測名・対象と試験設定を保存できる。`cargo run -- review CASE_DIR --repository-root LOCAL_REPOSITORY --execution-mode local`で起動した場合は、指定したリポジトリの固定版を画面から実行し、結果へ進める。通常のreviewは外部実行を有効にしない。Node.js/Python向けの観測部品を追加した。結果から固定版・起動ファイル・観測の行へ戻れる。

実行方式はlocalかLinuxのbubblewrapを明示する。localはOSによる隔離ではない。bubblewrapを起動できなければローカルへ切り替えない。対象プログラムの報告と独立した状態の確認、実AWS・iOSとの照合は別に必要。[接続手順と限界](docs/pinned-execution.md)を参照。

## 汎用バックエンドのAPI

2026-10-14の方針によりバックエンドを優先する。`api CASE_DIR --token-file NEW_FILE`でlocalhostのv1 APIを起動できる。事例の更新、外部からの報告観測、固定版実行、コード読取と静的解析、試行比較、全履歴の取得を画面なしで扱う。画面の追加は根拠の表示に留める。

```bash
cargo build --locked
npm run test:backend
```

合成のPython接続例から、報告→判定→判断→再試験→比較→バックアップ→復元を確認する。Node.js/Pythonの固定版実行と、言語を問わない外部からの報告を区別する。APIの入力・出力・上限と残る条件は[バックエンドの手順](docs/backend-api.md)。
