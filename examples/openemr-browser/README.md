# OpenEMRの患者と受診理由を試験する

`recipe.json`は、架空の患者を登録し、受診理由を保存して再表示するブラウザ試験。OpenEMRの固定コミット `d7ecc612e166c6f9f3adc9f50bc9f8cc68cac701` のproduction composeを参照し、記載されたOpenEMRとMariaDBのイメージをダイジェストで固定する。通常時と、患者登録と受診理由の入力後、保存する直前にDBを止める条件を試す。

```sh
npm ci --ignore-scripts
npx playwright install --with-deps chromium
# 管理者だけが読める試験用ファイルを作る。値はこのRecipe専用の架空のアカウント。
umask 077
printf '%s' '{"username":"eel-operator","password":"eel-pass-trial"}' > /tmp/eel-openemr-secrets.json
EEL_BROWSER_SECRETS_FILE=/tmp/eel-openemr-secrets.json cargo test --locked --test containers openemr_browser -- --ignored --nocapture
```

Dockerのエンジンが必要。試行ごとに新しいDBとコンテナを作り、終了後に消す。既存のOpenEMRや実患者のデータには接続しない。認証情報は公開した架空の値なので、実環境には流用しない。

画面の選択は、固定コミットのログインテンプレート、メニュー、患者登録画面、受診フォームを読んで定義した。初回の製品登録案内では「Ask again later」を選び、登録しない。同じ名前の非表示メニューを避け、開いている患者メニューを選ぶ。再表示する受診記録が折り畳まれていれば`expand`で開く。受診区分は試験用にOffice Visitを選ぶ。架空の識別値は名前の自動大文字化で変わらない大文字の接頭辞を使う。ラベルがない箇所はCSSを使う。`read_text.contains`は指定文字列が表示されるまで待つ条件であり、保存先を検査するものではない。OpenEMRはiframeの中へ画面を読み込むため、親から順にframeを指定する。

| 条件 | 調べる根拠 |
| --- | --- |
| 患者の表示 | 患者パネルに試行ごとの架空の値が表示されたか |
| 患者登録の応答 | 登録操作のPOST応答がHTTP 200か |
| 患者の保存 | `patient_data.lname`が試験値に一致する行をDBから読めるか |
| 受診理由の応答 | 保存操作のPOST応答がHTTP 200か |
| 受診理由の保存 | `form_encounter.reason`が試験値に一致する行をDBから読めるか |
| 受診理由の再表示 | Currentから受診記録を開き直し、試験値を表示できるか |

DBを止めた場合は、DBから行を読めないことを「行がない」に置き換えない。HTTP 200のエラーページが返る可能性もあるため、HTTPだけで保存を成功と判定しない。観測記録と写真は`evaluations/ci-openemr/`に残す。

この試験で扱う診療記録は受診理由だけ。SOAPの各項目、署名、各職種の閲覧権限、長期保存、実環境の構成との一致、OHIFの画像表示は別の確認が必要。#140全体の完了とは扱わない。
