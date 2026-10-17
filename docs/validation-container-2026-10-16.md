# 試験専用コンテナの検証記録（#139）

確認日：2026年10月16日。ブランチ `feat/container-trials`。操作はCodexが `cargo test` で行った。実際の運用担当・開発者による評価ではない。

## 環境

macOS 26.3.1（Apple M4）、Rust 1.99.0、Docker Desktop 4.49.0（Engine 28.5.1）。実行方式はlocal。試験用の一時ディレクトリに保存し、既存の `cases/` と `data/` は使っていない。

## 読んだ公式構成

OpenEMR の `docker/production/docker-compose.yml`（コミット `d7ecc612e166c6f9f3adc9f50bc9f8cc68cac701`）をGitHubから読んだ。対象のcomposeは実行していない。使ったイメージのダイジェストは、そこに書かれた値。

| 役割 | イメージ |
| --- | --- |
| OpenEMR | `openemr/openemr:latest@sha256:f0bfbc17fae13503ab64bbd271f74d80dd4a0786f5a8d0683841aeef4e159700` |
| MariaDB | `mariadb:12.3.3@sha256:2bdff1534a7e569fecaf4b10b66ad0d410806e382d9db29ba305448addda97c4` |
| Orthanc | `orthancteam/orthanc:26.9.1@sha256:d2705f2c56547e55ce9bc2250fd22d05d326493b3f3d9fa191ecb7db8ca6b5a2` |
| 接続確認 | `nginx:1.27.4-alpine@sha256:4ff102c5d78d254a6f0da062b3cf39eaf07f01eec0927fd21e219d0af8bc0591` |

## 結果

| 試行 | 観測 | 判定 |
| --- | --- | --- |
| nginx へ HTTP GET / | 本文に Welcome to nginx。ログに Configuration complete。試行後にコンテナは残らない | 条件を満たす（`cargo test --test containers`） |
| データベース役のコンテナを止める | 止まったことを確認した。アプリ側のnginxは応答した | 故障は効いた |
| データベースを試験用ネットワークから外す | 外れたことを確認した | 故障は効いた |
| Orthanc へ試験用DICOMを POST /instances | 応答に ParentStudy。画像は pynetdicom の試験用ファイル（患者ID `1CT1`） | 条件を満たす（`cargo test --test containers orthanc_stores -- --ignored`） |
| OpenEMR + MariaDB の準備 | 準備確認 `https://127.0.0.1/meta/health/readyz` が HTTP 200。基盤が MariaDB へ SELECT 1 を送り応答を得た。証明書は試験用のため検証していない | 条件を満たす（`cargo test --test containers openemr_becomes_ready -- --ignored`、約20秒） |

タグだけのイメージは起動を拒否した。

## そのほかの確認

| 確認 | 結果 |
| --- | --- |
| `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` | 成功（文書更新後に再実行する） |
| `cargo test`（ignored 以外） | 成功。nginxの3件を含む |

## 確かめていないこと

- 画面からの操作（患者登録・OHIFの表示）は#140。
- Podman、Windows、bubblewrap方式との併用。
- OpenEMRの管理画面でのログインと保存内容の照合。今回は準備完了とデータベースへの `SELECT 1` まで。
- GitHub Actionsでの Orthanc / OpenEMR イメージ取得。CIの通常の試験は nginx のみ。
