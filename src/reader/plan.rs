//! 実行せずに既存設定・テストから試験計画の材料を集める。
use super::{
    analysis::{Model, line_at},
    comprehension::{array, number, string},
    inquiry,
    overview::{self, effects, line_ref, reference},
    snapshot::Snapshot,
};
use serde_json::{Value, json};
fn dependencies(m: &Model<'_>) -> Vec<Value> {
    let mut items = vec![];
    for f in &m.files {
        if rx!(r"(^|/)requirements[^/]*\.txt$").is_match(&f.path) {
            for (i, l) in f.text.split('\n').enumerate() {
                let name = l.split('#').next().unwrap_or("").trim();
                if !name.is_empty() {
                    items.push(json!({"name":name,"file":f.path,"refs":[line_ref(&f.path,i+1)]}));
                }
            }
        }
        if rx!(r"(^|/)package\.json$").is_match(&f.path) {
            match serde_json::from_str::<Value>(&f.text) {
                Ok(p) => {
                    for group in ["dependencies", "devDependencies", "peerDependencies"] {
                        if let Some(obj) = p[group].as_object() {
                            for name in obj.keys() {
                                let line = f
                                    .text
                                    .split('\n')
                                    .position(|l| l.contains(&format!("\"{name}\"")))
                                    .map_or(1, |i| i + 1);
                                items.push(json!({"name":format!("{name}（{group}）"),"file":f.path,"refs":[line_ref(&f.path,line)]}));
                            }
                        }
                    }
                }
                Err(_) => items.push(
                    json!({"name":"package.json を読み取れなかった","file":f.path,"refs":[]}),
                ),
            }
        }
        if rx!(r"(^|/)pyproject\.toml$").is_match(&f.path)
            && let Some(c) = rx!(r"dependencies\s*=\s*\[([\s\S]*?)\]").captures(&f.text)
        {
            for d in rx!(r#"["']([^"']+)["']"#).captures_iter(&c[1]) {
                items.push(json!({"name":&d[1],"file":f.path,"refs":[line_ref(&f.path,line_at(&f.text,c.get(0).expect("全体の一致").start()))]}));
            }
        }
    }
    items
}
fn examples(m: &Model<'_>) -> Vec<Value> {
    let mut examples = vec![];
    for f in m.files.iter().filter(|f| f.path.ends_with(".t")) {
        let lines = f.text.split('\n').collect::<Vec<_>>();
        let mut alias: Option<(&str, &str)> = None;
        for (i, l) in lines.iter().enumerate() {
            if examples.len() >= 40 {
                break;
            }
            let Some(cmd) = l.strip_prefix("  $ ") else {
                continue;
            };
            if let Some(c) = rx!(r#"^alias\s+(\w+)="(.*)"$"#).captures(cmd) {
                alias = Some((
                    c.get(1).expect("別名").as_str(),
                    c.get(2).expect("コマンド").as_str(),
                ));
                continue;
            }
            let mut expected = vec![];
            let mut j = i + 1;
            while j < lines.len()
                && lines[j].starts_with("  ")
                && !lines[j].starts_with("  $ ")
                && lines[j]
                    .as_bytes()
                    .get(2)
                    .is_some_and(|b| !b.is_ascii_whitespace())
            {
                expected.push(&lines[j][2..]);
                j += 1;
            }
            examples.push(json!({"path":f.path,"command":cmd,"expected":expected,"alias":alias.filter(|(name,_)|cmd.split(' ').next()==Some(*name)).map(|(_,cmd)|cmd),"refs":[reference(&f.path,i+1,(i+1).max(j))]}));
        }
    }
    examples
}
fn test_commands(s: &Snapshot, m: &Model<'_>, deps: &[Value]) -> Vec<Value> {
    let mut out = vec![];
    for script in &m.scripts {
        if string(&script["name"]).contains("test") {
            out.push(json!({"text":format!("npm run {}（package.json: {}）",string(&script["name"]),string(&script["command"])),"refs":[line_ref(string(&script["path"]),number(&script["line"]).max(1))],"runnable":false,"reason":"npm scripts は依存パッケージのインストールを伴う場合がある。この基盤はインストールを行わないため、画面からは実行しない"}));
        }
    }
    if let Some(f) = m.files.iter().find(|f| f.path == "Makefile") {
        for (i, l) in f.text.split('\n').enumerate() {
            if rx!(r"^test\w*:").is_match(l) {
                out.push(json!({"text":format!("make {}",l.split(':').next().unwrap_or("")),"refs":[line_ref("Makefile",i+1)],"runnable":false,"reason":"Makefile の処理は任意のコマンドを含むため、画面からは実行しない"}));
            }
        }
    }
    for f in m
        .files
        .iter()
        .filter(|f| f.path.starts_with(".github/workflows/"))
    {
        for (i, l) in f.text.split('\n').enumerate() {
            if let Some(c) = rx!(r"^\s*(?:-\s*)?run:\s*(.+)$").captures(l)
                && rx!(r"test|pytest|cram|npm|node").is_match(&c[1])
            {
                out.push(json!({"text":format!("GitHub Actions: {}",c[1].trim()),"refs":[line_ref(&f.path,i+1)],"runnable":false,"reason":"自動テストの設定にあるコマンド。依存関係の準備を含むため、画面からは実行しない"}));
            }
        }
    }
    if s.inventory.iter().any(|e| e.path.ends_with(".t")) {
        let cram = deps
            .iter()
            .find(|d| rx!(r"(?i)^cram\b").is_match(string(&d["name"])));
        out.push(json!({"text":"cram tests/*.t（cram形式のテスト）","refs":cram.map_or(json!([]),|c|c["refs"].clone()),"runnable":false,"reason":if let Some(c)=cram{format!("cram は {} に依存パッケージとして書かれている。この基盤は依存パッケージをインストールしないため、既存テストは実行しない。テストの例を判定条件の案に使う",string(&c["file"]))}else{"cram が必要。この基盤は依存パッケージをインストールしない".to_owned()}}));
    }
    out
}
fn entries(s: &Snapshot, m: &Model<'_>) -> Vec<Value> {
    let mut items = vec![];
    let has =
        |p: &str| s.inventory.iter().any(|e| e.path == p) || m.files.iter().any(|f| f.path == p);
    for e in &m.entries {
        let p = string(&e["path"]);
        let runtime = if p.ends_with(".py") {
            Some("python3")
        } else if rx!(r"\.(m?js|cjs)$").is_match(p) {
            Some("node")
        } else {
            None
        };
        items.push(json!({"entrypoint":p,"launch":"file","runtime":runtime,"reason":e["reason"],"kind":if string(&e["reason"]).contains("__main__"){"code"}else{"inference"},"refs":[line_ref(p,number(&e["line"]))],"supported":runtime.is_some(),"unsupported_reason":if runtime.is_some(){None}else{Some("この言語の実行は未対応")}}));
    }
    for f in s
        .inventory
        .iter()
        .filter(|f| rx!(r"(^|/)__main__\.py$").is_match(&f.path))
    {
        let module = f
            .path
            .strip_suffix("/__main__.py")
            .unwrap_or(&f.path)
            .replace('/', ".");
        if rx!(r"^[A-Za-z_][\w.]*$").is_match(&module)
            && has(&f.path.replace("__main__.py", "__init__.py"))
        {
            items.push(json!({"entrypoint":module,"launch":"module","runtime":"python3","reason":format!("{} があり、python -m {module} で起動できるパッケージ",f.path),"kind":"code","refs":[line_ref(&f.path,1)],"supported":true}));
        }
    }
    if let Some(f) = m.files.iter().find(|f| f.path == "pyproject.toml") {
        let mut section = "";
        for (i, l) in f.text.split('\n').enumerate() {
            if let Some(c) = rx!(r"^\s*\[([^\]]+)\]\s*$").captures(l) {
                section = c.get(1).expect("セクション名").as_str().trim();
                continue;
            }
            if section != "project.scripts" {
                continue;
            }
            if let Some(c) = rx!(r#"^\s*([\w.-]+)\s*=\s*["']([\w.]+):(\w+)["']"#).captures(l) {
                let base = c[2].replace('.', "/");
                items.push(json!({"entrypoint":&c[1],"launch":"script","runtime":"python3","reason":format!("pyproject.toml の [project.scripts] でコマンド {} の入口が {} の関数 {} とされている",&c[1],&c[2],&c[3]),"kind":"code","refs":[line_ref("pyproject.toml",i+1)],"supported":has(&format!("{base}.py"))||has(&format!("{base}/__init__.py"))}));
            }
        }
    }
    if let Some(f) = m.files.iter().find(|f| f.path == "setup.py") {
        for (i, l) in f.text.split('\n').enumerate() {
            if let Some(c) = rx!(r#"['"]\s*([\w.-]+)\s*=\s*([\w.]+):([\w.]+)\s*['"]"#).captures(l) {
                let path = format!("{}.py", c[2].replace('.', "/"));
                if !items.iter().any(|e| e["entrypoint"] == path) {
                    items.push(json!({"entrypoint":path,"runtime":"python3","reason":format!("setup.py でコマンド {} の入口とされる関数 {} がある",&c[1],&c[3]),"kind":"code","refs":[line_ref("setup.py",i+1)],"supported":has(&path)}));
                }
            }
        }
    }
    if let Some(f) = m.files.iter().find(|f| f.path == "package.json")
        && let Ok(data) = serde_json::from_str::<Value>(&f.text)
    {
        let mut add = |p: &str, why: String| {
            let p = p.strip_prefix("./").unwrap_or(p);
            if !items.iter().any(|e| e["entrypoint"] == p) {
                let supported = rx!(r"\.(m?js|cjs)$").is_match(p);
                items.push(json!({"entrypoint":p,"runtime":if supported{Some("node")}else{None},"reason":why,"kind":"code","refs":[line_ref("package.json",1)],"supported":supported}));
            }
        };
        if let Some(p) = data["main"].as_str() {
            add(p, "package.json の main".to_owned());
        }
        if let Some(p) = data["bin"].as_str() {
            add(p, "package.json の bin".to_owned());
        } else if let Some(bin) = data["bin"].as_object() {
            for (n, p) in bin {
                if let Some(p) = p.as_str() {
                    add(p, format!("package.json の bin（{n}）"));
                }
            }
        }
    }
    items
}
pub fn plan(s: &Snapshot, m: &Model<'_>) -> Value {
    let langs = overview::languages(s);
    let options = inquiry::options(m);
    let deps = dependencies(m);
    let entries = entries(s, m);
    let mut storage = vec![];
    let sends = m
        .files
        .iter()
        .flat_map(effects)
        .any(|e| e["kind"] == "send");
    for f in &m.files {
        for e in effects(f)
            .into_iter()
            .filter(|e| e["kind"] == "write" && e["certainty"] != "name")
        {
            storage.push(json!({"text":format!("{} の{}行目に書き込みの記述がある: {}",e["function"].as_str().unwrap_or(&f.path),number(&e["line"]),string(&e["code"])),"kind":"code","refs":e["refs"]}));
        }
    }
    for o in &options {
        let mut words = vec![string(&o["dest"]), string(&o["metavar"])];
        words.extend(array(&o["flags"]).iter().map(string));
        if rx!(r"(?i)dir|file|path|list|name|db|out|store").is_match(&words.join(" ")) {
            let help = o["help"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map_or(String::new(), |s| format!("（説明: {s}）"));
            let default = o["default"]
                .as_str()
                .map_or(String::new(), |s| format!("、既定値 {s}"));
            storage.push(json!({"text":format!("オプション {}{help}{default}。保存先の指定に関係する可能性がある",array(&o["flags"]).iter().map(string).collect::<Vec<_>>().join(" / ")),"kind":"inference","refs":o["refs"],"option":o}));
        }
    }
    let executable = entries.iter().any(|e| e["supported"] == true);
    let runtime_deps = deps
        .iter()
        .filter(|d| {
            !rx!(
                r"(?i)^cram\b|pytest|mocha|jest|vitest|playwright|eslint|typescript|devDependencies"
            )
            .is_match(string(&d["name"]))
        })
        .collect::<Vec<_>>();
    let mut missing = vec![];
    if !executable {
        missing.push(json!({"text":"実行できる入口が見つからない。この基盤は Python 3 と Node.js（JavaScript）だけを実行できる","kind":"code"}));
    }
    for text in [
        "試行で使う引数。試験用の値を入れる場所には {{test_value}} と書く",
        "保存先のパス（試行用の作業ディレクトリからの相対パス）。保存先を指定しないと、基盤による保存先の読み出しと保存先の故障は使えない",
        "後から読み出すコマンド（任意）。書き込みとは別のプロセスで読み出せるかを確かめる",
    ] {
        missing.push(json!({"text":text,"kind":"inference"}));
    }
    if !runtime_deps.is_empty() {
        missing.push(json!({"text":format!("依存パッケージ（{}）は、この基盤ではインストールしない。Pythonだけで書かれた依存先は、公開GitHubのリポジトリと固定コミットを「依存先」に指定すれば読み込める。どのリポジトリ・コミットが対応するかは、コードからは決めない。C拡張やビルドが必要な依存先は扱えない",runtime_deps.iter().take(6).map(|d|string(&d["name"])).collect::<Vec<_>>().join("、")),"kind":"code","refs":runtime_deps.iter().take(6).flat_map(|d|array(&d["refs"]).to_vec()).collect::<Vec<_>>()}));
    }
    missing.push(json!({"text":"先に起動して待つプロセス（任意）。受信サーバーのように待ち受けるプロセスは、引数のポートを {{port}} と書くと、基盤が 127.0.0.1 の空いているポートを割り当てる","kind":"inference"}));
    let mut faults: Vec<Value> =
        serde_json::from_str(include_str!("faults.json")).expect("組み込んだ故障一覧はJSON");
    for f in &mut faults {
        let applicable = if f["id"] == "network_unavailable" {
            if sends {
                "bubblewrap方式（Linux）でだけ使える。送信の記述がある"
            } else {
                "送信の記述が見つからないため、効果を確かめにくい。bubblewrap方式（Linux）でだけ使える"
            }
        } else if f["needs_server"] == true {
            "待つプロセスを指定したときに使える"
        } else if f["needs_containers"] == true {
            "試験専用のコンテナを指定したときに使える"
        } else if f["id"] == "storage_directory_read_only" {
            "ディレクトリを含む保存先のパス（例: received/file）を指定したときに使える"
        } else if f["needs_storage"] == true {
            "保存先のパスを指定したときに使える"
        } else {
            "使える"
        };
        f["applicable"] = json!(applicable);
    }
    let names = langs
        .iter()
        .map(|l| string(&l["name"]))
        .collect::<Vec<_>>()
        .join("・");
    json!({"schema_version":1,"repository":s.full_name,"commit":s.commit,"languages":langs,"executable":executable,"unsupported_reason":if executable{None}else{Some(format!("{} の実行は未対応。理解と試験計画だけを表示する",if names.is_empty(){"この言語"}else{&names}))},"entries":entries,"options":options,"storage":storage,"tests":overview::tests(s,m),"test_commands":test_commands(s,m,&deps),"examples":examples(m),"dependencies":deps,"faults":faults,"observation":[{"method":"exit_code","label":"終了コード","source":"対象プログラムの報告"},{"method":"stdout","label":"標準出力","source":"対象プログラムの報告"},{"method":"stderr","label":"標準エラー（エラー表示）","source":"対象プログラムの報告"},{"method":"stored_file","label":"保存先の内容","source":"基盤が保存先を直接読み出した結果"},{"method":"readback_output","label":"後からの読み出し","source":"別のプロセスで対象の読み出し機能を動かした結果"},{"method":"server_output","label":"待つプロセスの出力","source":"待つプロセス（受信側など）の出力"}],"missing":missing,"isolation":"対象は試行ごとに新しい作業ディレクトリで動く。固定コミットのファイルは読み取り専用で複製し、環境変数は消し、HOMEも作業ディレクトリにする。local方式はOSによる隔離を行わないため、対象が絶対パスで書き込む処理は止められない","origin":"コード・テストからの提案。人が確認して保存するまで、期待や判定条件の決定ではない"})
}
