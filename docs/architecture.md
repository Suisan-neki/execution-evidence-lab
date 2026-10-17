# 構成と試験結果の対応づけ

最初の題材は、`examples/openemr-browser/recipe.json`で起動するOpenEMRとMariaDBの試験用構成。実運用の構成図ではない。`architecture.json`に構成、固定Git参照、イメージのダイジェスト、ブラウザの版、根拠と限界を記録する。Git参照のコードとイメージ内のコードが同一だとは証明していない。

## 保存と照合

事例の`architecture_models`に構成の版を順に追加する。同じ識別子は版1から始め、既存の版を変更・削除しない。親要素はsystemかboundaryに限定し、循環・不明な親・不明な接続先・重複IDを拒否する。設定で確認した構成、人による補足、推定を`basis.kind`で区別し、試験・実運用・模擬・不明を`environment`で区別する。

`architecture_links`は構成の識別子・版と要素または接続を指定し、次のいずれかを参照する。

- `check`: 確認事項の識別子と版。条件を改訂すると旧リンクは再確認が必要になる。
- `statement`: 発言・質問の識別子。理解や合意の成立を判定する情報ではない。
- `evidence`: 試行の根拠の識別子と、その試行内の観測条件の識別子。試行全体の判定で部分の観測欠落を埋めない。

試行のリンクには、レシピの固定コード参照、サービス名ごとのイメージ、ブラウザの版が構成と一致することが必要。対応づけは人の選択であり、その要素全体の動作を証明するものではない。DBの直接読取をアプリからDBへの通信観測として扱わない。観測の記録がない場合は未観測、観測した値が期待と違う場合は失敗として分ける。

構成が変わっても、過去の試行への参照を新版へ移さない。APIは`current_model`と、現在の条件への`applicable`を別々に返す。現在の条件への適用可否は既存のコード・観測規則・条件版・依存先の照合を共用する。歴史的な観測結果はそのまま残す。

## APIとCLI

認証付きの`GET /api/v1/architecture`は保存済み構成、リンクと部分観測の結果、OpenEMRの組み込み構成を返す。更新は既存の`POST /api/v1/change`か`case apply`を使う。記入者・理由・期待する事例版が必要。

```json
{"op":"add_architecture_model","model":{"schema_version":1,"id":"…","version":1}}
```

上のmodelは省略例。実際には`examples/openemr-browser/architecture.json`の全フィールドが必要。

```json
{"op":"add_architecture_link","link":{"id":"visit-row","model_id":"openemr-trial","model_version":1,"object_id":"db","target":{"kind":"evidence","evidence_id":"evidence-1","observation_id":"visit-row"}}}
```

全版バックアップは構成とリンクを含め、過去の構成・リンクを書き換えたバックアップを拒否する。PNGなど試行フォルダーのファイルは従来通り含めない。

## 検証の範囲

単体試験では、階層の循環、存在しない参照、版の再利用、条件改訂、イメージの不一致、HTTP失敗とDB未観測の混同、過去の構成の書換を確認する。単体試験の合成記録は実OSSを動かした証拠には使わない。

CIのOpenEMR実行試験では、正常時・DB停止時の実際の試行を事例へ取り込み、それぞれの表示・HTTP・DBの観測条件を構成へ対応づける。実行結果と事例の全版はCIの成果物に残す。参加者による理解・判断の評価は未実施。
