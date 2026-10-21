# GitDiagramから参照・翻案した部分

参照元：[ahmedkhaleel2004/gitdiagram](https://github.com/ahmedkhaleel2004/gitdiagram)。Copyright (c) 2024 Ahmed Khaleel。MIT Licenseを同じディレクトリのLICENSEに保持する。

2026年10月21日に取得した `src/server/generate/prompts.ts`（Git blob SHA `098c954f81872e70c26e526a0d555110b572b5a8`）と `docs/architecture.md` を基に、構成把握の指示を `src/system_diagram/prompt.txt` へ日本語で翻案した。目的と主要な業務の流れ、実際のサブシステム、責務ごとの要素、根拠のある方向付きの関係、推定と未読範囲を分ける方針を参照した。

GitDiagramのWebサービスやMCPを呼び出しているわけではない。読取、行番号の照合、構造化モデルの検査、保存、AI接続、ELKによる描画はこのリポジトリの実装を使う。立場別の説明と疑問・検証への接続は、このプロジェクトの要件に合わせて追加した。GitDiagramと同じ品質を達成したと判断した意味にはしない。
