# Rustによる読取・解析の検証（#144）

対象：`feat/rust-repository-reader`。実装と実行確認の記録であり、本人によるPRの採用前。

## 確認したこと

Rust 1.99.0、Node.js 24.19.0、Linuxで確認した。

- `cargo fmt --check`、`cargo check --locked`、`cargo test --locked`、`cargo clippy --locked --all-targets -- -D warnings`。Rustの試験93件が成功。既存のDockerを使う2件は明示的なignoreのまま。
- 旧JSの解析を8種類の架空リポジトリへ適用し、期待結果を`tests/fixtures/reader/`へ保存した。Rustの試験はNode.jsや通信を使わず、モデル・概要・試験計画8組、関数56件の案内・呼び出し元・詳細、質問48件の結果を比較する。合計240組のJSONが一致した。
- 架空のGitHub応答で、版とtreeの固定、ブランチの`/`のエンコード、初回24ファイルと各128KiBの上限、symlinkを読まないこと、UTF-8不正・NUL・サイズ超過を未読に残すことを確認した。公開URL以外、非公開のメタデータ、API上限、HTTPエラー、期限超過を拒否した。
- 入力済みのsnapshotの解析ではHTTPクライアントを作らないことを、作成時に失敗する代替処理で確認した。未読の関数を追加で読み、質問に応じた追加読取が最大8件であることも確認した。
- `npm run test:reader-native -- --live`。APIサーバーのPATHを空のディレクトリに設定し、Node.jsを見つけられない状態でanalyze・overview・plan・function・inquireを実施した。実GitHubの`sjl/t`の固定コミット`70817fd5264caa926fd8ee88b52dfc7d25f4240c`について、一覧21件・本文15件を読み、追加本文の取得とopenによる概要生成まで成功した。
- `node tools/backend-demo.mjs evaluations/rust-reader-backend --live`。APIとCLIによる判定・履歴・比較・バックアップ・復元の既存の一往復が成功した。実GitHubの本リポジトリの固定版`63c8dac0789ed712f267a9956f5f012c645aea88`の一覧70件・本文24件を読めた。
- `npm run test:reader`。旧explore画面向けのJS解析の既存4スクリプトが成功した。

## この環境で完了していない確認

`npm run test:ui`と`npm run test:app`を試した。Playwright付属のChromiumは配布先がZIPを返さず取得できなかった。Google Chrome 155.0.8059.84でも試したが、この実行環境がブラウザ起動時のソケット作成を拒否したため、画面を操作する前に終了した。

直接のエラーは`process_singleton_posix.cc: socket() failed: Operation not permitted (1)`。ローカルの画面操作試験は成功と記録しない。既存の画面操作試験はCIに残し、Node.jsなしのAPI試験をCIにも追加した。今回のフロントエンドのHTML・CSS・操作コードは変更していない。

## 確認範囲

解析は既存の簡易解析の移植。実行経路や業務上の意味、型・動的呼び出しの完全な解析ではない。旧exploreはブラウザ内のJS版を維持し、AI接続には別のNode.js経路が残る。OSや特定の運用環境、実参加者の理解・判断、医療OSS3件への適用を評価した結果ではない。

候補の同順位はパスのバイト順へ変更した。大小文字・記号が混じるパスでは従来のlocaleCompareと初回の選択が異なることがある。HTTPのリダイレクトを拒否し、snapshot内の重複・空のパス要素・バックスラッシュ・制御文字も拒否する。解析には共通期限と入れ子の上限を追加した。これらの境界の変更を、解析精度の向上とは扱わない。
