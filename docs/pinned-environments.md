# 依存の導入とビルドを固定して記録する

`environment` CLIは、依存の導入やビルドが必要な対象を試行専用のDockerコンテナで準備する。従来の`case execute`とは別の入口で、準備結果を業務条件の判定へ自動で取り込まない。

最初の対象はOHIF ViewerとMONAI。OHIFは固定コミットのロックファイルからビルドする。MONAIはCPU用の依存を導入し、固定コミットのソースから合成画像の推論を行う。

## 実行前に確認する

```bash
cargo build --locked
./target/debug/execution-evidence-lab environment inspect examples/ohif-build/recipe.json
./target/debug/execution-evidence-lab environment inspect examples/monai-cpu/recipe.json
```

設定にはソースのコミット、イメージのダイジェスト、固定ファイルのSHA-256、資源と待機時間の上限、実行コマンド、限界を記載する。`inspect`は補助スクリプトと基盤の実行用スクリプトも表示する。`run`でも同じ内容を表示してから実行する。依存のインストール用スクリプトがコンテナ内で動くことを実行前に確認できる。

ソースはローカルのGitリポジトリから、指定したコミットを`git archive`で読み出す。未コミットの作業ファイルは使わない。ホストのディレクトリ、Dockerソケット、認証情報は対象へ渡さない。コンテナはネットワークを使って配布ファイルを取得する。

## OHIF Viewerをビルドする

```bash
git clone https://github.com/OHIF/Viewers.git /tmp/ohif-source
git -C /tmp/ohif-source fetch origin 5a78dac37797efa74c10e63ec2028bca0b33e13b
./target/debug/execution-evidence-lab environment run examples/ohif-build/recipe.json --repository-root /tmp/ohif-source --output evaluations/ohif-build
```

Node 24.19.0のLinux amd64イメージとpnpm 12.8.1を固定する。pnpm本体とLinux実行バイナリの配布ファイルをSHA-512で確認した後、上流のロックファイルを使って`pnpm install --frozen-lockfile --verify-store-integrity`を実行する。ロックファイル、workspace設定、package.json、.npmrcのSHA-256を導入前後とビルド後に確認する。

上流のDockerfileはビルド対象からドキュメントを除き、`--no-frozen-lockfile`で依存を再解決する。そのままは使わず、固定コミット全体を渡してロックファイルに従う。Git submoduleのtestdataは取得しない。CypressとPlaywrightのブラウザー取得も省略する。`QUICK_BUILD=true`で、上流の標準ビルドを実行する。

生成した静的ファイルは`result/dist/`へ、各ファイルのSHA-256は`result/build-files.json`へ残す。ビルドの成功は、検査一覧や画像の取得・表示の成功を意味しない。

## MONAIで合成画像を推論する

```bash
git clone --branch 1.6.1 https://github.com/Project-MONAI/MONAI.git /tmp/monai-source
./target/debug/execution-evidence-lab environment run examples/monai-cpu/recipe.json --repository-root /tmp/monai-source --output evaluations/monai-cpu
```

MONAIは1.6.1のコミット`f2ec87ae9b536baaeca1d6bcc2b6349369e2af8f`、Pythonは3.12.12のLinux amd64イメージを使う。torch 2.8.0+cpu、numpy 2.3.3と必要な依存先を、配布URL・版・wheelのSHA-256で固定する。コンテナ内のvenvへ`--no-index --no-deps --require-hashes`で入れ、`pip check`と導入後の版の照合を行う。MONAIは固定ソースをPYTHONPATHから読み込む。任意の追加依存、C++/CUDA拡張、GPUは使わない。

MONAIの`create_test_image_2d`で64×64の合成画像を作り、seedを144に固定する。1×1の畳み込みの重みを2、バイアスを0.25に固定し、チェックポイントの保存・再読込後、MONAIの`sliding_window_inference`で推論する。出力と保存後の読み出しを、全画素について期待値`2 * input + 0.25`と照合する。float32、絶対許容差`1e-5`、相対許容差`1e-5`を実装前に選んだ。許容範囲は`atol + rtol * abs(reference)`。

入力、ラベル、出力、チェックポイントのハッシュと照合結果を`result/inference.json`へ残す。これは推論経路の接続確認であり、学習済みの医療モデルの性能評価ではない。学習経路、実データ、臨床上の妥当性は未確認。

## 記録と比較

毎回新しい試行ディレクトリを作り、既存の記録を上書きしない。

| 記録 | 内容 |
| --- | --- |
| plan.json | 実行前に表示した設定とスクリプト |
| source.tar | 固定コミットから作ったソースのアーカイブ |
| environment.json | ソースのtreeとアーカイブのSHA-256、イメージID、基盤スクリプトのSHA-256、準備・対象コマンドの状態、導入一覧、掃除の確認 |
| result/report.json | コンテナ内で記録した手順、終了コード、途中までの状態 |
| result/packages.json | 導入した依存の名前・版・ハッシュ |
| execution.stdout.log / stderr.log | コンテナ内で実行した処理の出力 |

Nodeの導入一覧は、導入先のpackage.jsonのSHA-256と、ロックファイルに記載された配布ファイルのintegrityを区別する。package.jsonのハッシュは、導入した全ファイルのハッシュではない。Pythonの一覧には、導入に使って照合したwheelのSHA-256を記録する。pipなどイメージに同梱された基盤ツールはイメージのダイジェストと`pip-list.json`で確認する。

```bash
./target/debug/execution-evidence-lab environment show RUN_DIR
./target/debug/execution-evidence-lab environment compare BEFORE_DIR AFTER_DIR
```

比較ではソース、イメージ、設定、基盤スクリプト、依存の版・ハッシュと結果の変化を示す。準備に失敗した試行の空の一覧を「同じ依存だった」と扱わない。業務条件やモデルの性能は判定しない。

OHIFはビルドまでを準備として扱い、`preparation=succeeded`でも`target=not_observed`のままにする。MONAIは依存の準備に成功してから推論コマンドを動かす。準備失敗なら`target=not_observed`、推論コマンドの失敗なら`preparation=succeeded, target=failed`になる。終了コードだけで業務条件を成立とするものではない。

待機時間または出力の上限に達した場合はコンテナを止め、残った記録を取得する。Dockerが使えない場合は準備失敗を記録し、別の実行方式に切り替えない。通常の終了時にはコンテナを削除し、削除を確認できなければCLIを失敗にする。ホスト自体が停止した場合の回収は保証しない。

## 確認のしかたと範囲

ローカルでは設定の検査、補助ファイルの改変の拒否、ハッシュの比較、Rustの検査を行う。この開発環境にはDockerがないため、実コンテナの確認はGitHub Actionsの`Pinned target environments`で行う。

CIは実際のOHIFビルド、MONAI推論、固定ファイルのハッシュを故意に変えた準備失敗を確認する。MONAIでは入力がないコマンドを実行した後、正常な設定で再試験し、導入一覧を比較する。CIの成否と実行記録はPRに残す。コンテナ内の記録は対象と同じ環境で得た報告であり、対象から独立した処理経路の証明ではない。

読む順番は`examples/*/recipe.json`、`src/environment.rs`、`tools/environment-runner.py`、`examples/*/*.py`、`tools/environment-smoke.mjs`。設定の検査と記録の保存はRust、依存の導入と対象コマンドの実行はコンテナ内のPythonが担当する。
