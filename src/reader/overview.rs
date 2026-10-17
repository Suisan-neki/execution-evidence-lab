//! 根拠の種類・行と未確認範囲を持つ概要。対象のコードは実行しない。
use super::{
    analysis::{File, Model, clip, mask},
    comprehension::{array, explain, number, range, string},
    snapshot::{self, Snapshot},
};
use serde_json::{Map, Value, json};
use std::collections::{HashSet, VecDeque};
pub fn reference(path: &str, line: usize, end: usize) -> Value {
    json!({"path":path,"line":line,"end_line":end})
}
pub fn line_ref(path: &str, line: usize) -> Value {
    reference(path, line, line)
}
pub fn lines(text: &str) -> usize {
    text.split('\n').count() - usize::from(text.ends_with('\n'))
}
pub fn language(path: &str) -> Option<&'static str> {
    if path.ends_with(".py") {
        Some("python")
    } else if path.ends_with(".rs") {
        Some("rust")
    } else if rx!(r"\.[cm]?[jt]sx?$").is_match(path) {
        Some("javascript")
    } else {
        None
    }
}
pub fn effect_label(kind: &str) -> &str {
    match kind {
        "input" => "外部からの入力（引数・標準入力・リクエスト）を受け取る記述",
        "env" => "環境変数を読む記述",
        "read" => "ファイル等から読み出す記述",
        "write" => "ファイル等へ書き込む記述",
        "delete" => "ファイル等を削除する記述",
        "send" => "外部へ送信・通信する記述",
        "output" => "画面（標準出力）へ表示する記述",
        "error_output" => "エラー表示（標準エラー）へ出力する記述",
        "raise" => "例外・エラーを発生させる記述",
        "catch" => "例外・エラーを捕捉する記述",
        "exit" => "プロセスを終了させる記述",
        _ => kind,
    }
}
fn kinds(lang: &str, line: &str) -> Vec<&'static str> {
    let mut result = vec![];
    match lang {
        "python" => {
            if rx!(r#"\bsys\.argv\b|\.parse_args\s*\(|\binput\s*\(|\bsys\.stdin\b"#).is_match(line)
            {
                result.push("input");
            }
            if rx!(r#"\bos\.environ\b|\bos\.getenv\s*\("#).is_match(line) {
                result.push("env");
            }
            if rx!(r#"\bsys\.stderr\.write\s*\(|print\s*\([^)]*file\s*=\s*sys\.stderr"#)
                .is_match(line)
            {
                result.push("error_output");
            }
            if rx!(r#"\bprint\s*\(|\bsys\.stdout\.write\s*\("#).is_match(line) {
                result.push("output");
            }
            if rx!(r#"\bjson\.dump\s*\(|\bpickle\.dump\s*\(|\bshutil\.(copy\w*|move)\s*\(|\bos\.(rename|replace|makedirs|mkdir)\s*\(|\.commit\s*\(|\.executemany?\s*\("#).is_match(line){result.push("write");}
            if rx!(r#"\bos\.(remove|unlink|rmdir)\s*\(|\bshutil\.rmtree\s*\("#).is_match(line) {
                result.push("delete");
            }
            if rx!(r#"\brequests\.(get|post|put|patch|delete|request)\s*\(|\burllib\.request\b|\bhttp\.client\b|\bsocket\.\w+\s*\(|\bsmtplib\b"#).is_match(line){result.push("send");}
            if rx!(r#"\braise\b"#).is_match(line) {
                result.push("raise");
            }
            if rx!(r#"^\s*except\b"#).is_match(line) {
                result.push("catch");
            }
            if rx!(r#"\bsys\.exit\s*\(|\bos\._exit\s*\("#).is_match(line) {
                result.push("exit");
            }
        }
        "javascript" => {
            if rx!(r#"\bprocess\.argv\b|\bprocess\.stdin\b|\breadline\b|\b(req|request|ctx\.request)\.(body|query|params)\b"#).is_match(line){result.push("input");}
            if rx!(r#"\bprocess\.env\b"#).is_match(line) {
                result.push("env");
            }
            if rx!(r#"\bconsole\.(error|warn)\s*\(|\bprocess\.stderr\.write\s*\("#).is_match(line) {
                result.push("error_output");
            }
            if rx!(r#"\bconsole\.(log|info)\s*\(|\bprocess\.stdout\.write\s*\(|\bres\.(send|json|end|write)\s*\("#).is_match(line){result.push("output");}
            if rx!(r#"\breadFile(Sync)?\s*\(|\bcreateReadStream\s*\(|\blocalStorage\.getItem\s*\(|\breaddir(Sync)?\s*\("#).is_match(line){result.push("read");}
            if rx!(r#"\b(writeFile|appendFile|rename|mkdir|copyFile)(Sync)?\s*\(|\bcreateWriteStream\s*\(|\blocalStorage\.setItem\s*\(|\.(insert|insertOne|save|put)\s*\("#).is_match(line){result.push("write");}
            if rx!(r#"\b(unlink|rm|rmdir)(Sync)?\s*\(|\blocalStorage\.removeItem\s*\("#)
                .is_match(line)
            {
                result.push("delete");
            }
            if rx!(r#"\bfetch\s*\(|\baxios\b|\bhttps?\.request\s*\(|\bXMLHttpRequest\b|\bWebSocket\s*\(|\.sendMail\s*\("#).is_match(line){result.push("send");}
            if rx!(r#"\bthrow\b"#).is_match(line) {
                result.push("raise");
            }
            if rx!(r#"\bcatch\s*[({]|\.catch\s*\("#).is_match(line) {
                result.push("catch");
            }
            if rx!(r#"\bprocess\.exit\s*\("#).is_match(line) {
                result.push("exit");
            }
        }
        "rust" => {
            if rx!(r#"\bstd::env::args\b|\benv::args\s*\(|\bstdin\s*\("#).is_match(line) {
                result.push("input");
            }
            if rx!(r#"\benv::var(_os)?\s*\("#).is_match(line) {
                result.push("env");
            }
            if rx!(r#"\beprintln!\s*\("#).is_match(line) {
                result.push("error_output");
            }
            if rx!(r#"\bprintln!\s*\(|\bprint!\s*\("#).is_match(line) {
                result.push("output");
            }
            if rx!(r#"\bfs::read(_to_string|_dir)?\s*\(|\bFile::open\s*\("#).is_match(line) {
                result.push("read");
            }
            if rx!(r#"\bfs::(write|rename|create_dir\w*|copy|hard_link)\s*\(|\bFile::create\s*\(|\bOpenOptions\b"#).is_match(line){result.push("write");}
            if rx!(r#"\bfs::remove_\w+\s*\("#).is_match(line) {
                result.push("delete");
            }
            if rx!(r#"\bTcpStream::connect\w*\s*\(|\breqwest\b|\bUdpSocket\b"#).is_match(line) {
                result.push("send");
            }
            if rx!(r#"\bpanic!\s*\(|\bbail!\s*\(|\breturn\s+Err\s*\("#).is_match(line) {
                result.push("raise");
            }
            if rx!(r#"\bprocess::exit\s*\("#).is_match(line) {
                result.push("exit");
            }
        }
        _ => {}
    }
    result
}
pub fn functions(file: &File) -> Vec<Value> {
    if !file.budget.check() {
        return vec![];
    }
    let clean = mask(
        &file.text,
        file.path.ends_with(".rs"),
        file.path.ends_with(".py"),
    );
    let text = clean.split('\n').collect::<Vec<_>>();
    let mut classes = vec![];
    if file.path.ends_with(".py") {
        for (i, l) in text.iter().enumerate() {
            if let Some(c) = rx!(r"^(\s*)class\s+([A-Za-z_]\w*)").captures(l) {
                classes.push((c[2].to_owned(), i + 1, c[1].len()));
            }
        }
    }
    file.symbols.iter().take_while(|_|file.budget.check()).map(|s|{let r=range(file,s,&clean);let l=text[s.line-1];let indent=l.len()-l.trim_start().len();let owner=classes.iter().rev().find(|(_,line,n)|*line<s.line&&*n<indent).map(|(name,_,_)|name.as_str());
        json!({"name":if let Some(o)=owner{format!("{o}.{}",s.name)}else{s.name.to_owned()},"short":s.name,"line":s.line,"end":r.end.max(s.line),"owner":owner})}).collect()
}
pub fn enclosing(functions: &[Value], line: usize) -> Option<&Value> {
    functions
        .iter()
        .filter(|f| number(&f["line"]) <= line && line <= number(&f["end"]))
        .max_by_key(|f| number(&f["line"]))
}
pub fn effects(file: &File) -> Vec<Value> {
    let Some(lang) = language(&file.path) else {
        return vec![];
    };
    let clean = mask(&file.text, lang == "rust", lang == "python");
    let fns = functions(file);
    let mut found = vec![];
    for (i, (line, original)) in clean
        .split('\n')
        .zip(file.text.split('\n'))
        .enumerate()
        .take_while(|_| file.budget.check())
    {
        let mut kinds = kinds(lang, line);
        if lang == "python" && rx!(r"\bopen\s*\(").is_match(line) {
            let mode = rx!(r#"open\s*\([^)]*?,\s*(?:mode\s*=\s*)?['"]([rwaxbt+]+)['"]"#)
                .captures(original)
                .map(|c| c[1].to_owned())
                .unwrap_or_else(|| "r".to_owned());
            let k = if rx!(r"[wax+]").is_match(&mode) {
                "write"
            } else {
                "read"
            };
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
        let by_name = lang == "python"
            && rx!(r"\.write\s*\(").is_match(line)
            && !rx!(r"\bsys\.std(out|err)\.write").is_match(line)
            && !kinds.contains(&"write");
        if kinds.contains(&"error_output") {
            kinds.retain(|k| *k != "output");
        }
        let owner = enclosing(&fns, i + 1);
        let mut push = |kind: &str, label: &str, certainty: &str| {
            found.push(json!({"kind":kind,"label":label,"certainty":certainty,"path":file.path,"line":i+1,"code":clip(original.trim(),200),"function":owner.map(|o|&o["name"]),"function_line":owner.map(|o|&o["line"]),"refs":[line_ref(&file.path,i+1)]}));
        };
        for k in kinds {
            push(k, effect_label(k), "statement");
        }
        if by_name {
            push(
                "write",
                "名前が write の呼び出し。ファイル等への書き込みの候補",
                "name",
            );
        }
    }
    found
}
fn declared_commands(model: &Model<'_>) -> Vec<String> {
    let mut names = vec![];
    for f in &model.files {
        if f.path == "setup.py" || f.path == "pyproject.toml" {
            for c in rx!(r#"['"]?\s*([\w.-]+)\s*=\s*['"]?[\w.]+:[\w.]+"#).captures_iter(&f.text) {
                names.push(c[1].to_owned());
            }
        }
        if f.path == "package.json"
            && let Ok(p) = serde_json::from_str::<Value>(&f.text)
        {
            if p["bin"].is_string() && p["name"].is_string() {
                names.push(string(&p["name"]).to_owned());
            } else if let Some(bin) = p["bin"].as_object() {
                names.extend(bin.keys().cloned());
            }
        }
    }
    names
}
fn readme_parts(path: &str, text: &str, names: &[String]) -> (Vec<Value>, Vec<Value>) {
    let mut purpose = vec![];
    let mut commands = vec![];
    let mut paragraph = vec![];
    let mut start = 0;
    let mut fence = false;
    let flush = |paragraph: &mut Vec<&str>, purpose: &mut Vec<Value>, start: usize, end: usize| {
        let joined = paragraph.join(" ");
        let joined = joined.trim();
        if !joined.is_empty()
            && purpose.is_empty()
            && !joined.starts_with(['[', '!', '<'])
            && joined.chars().count() > 20
        {
            purpose.push(json!({"text":clip(joined,500),"refs":[reference(path,start,end)]}));
        }
        paragraph.clear();
    };
    for (i, line) in text.split('\n').enumerate() {
        if rx!(r"^\s*```").is_match(line) {
            fence = !fence;
            flush(&mut paragraph, &mut purpose, start, i);
            continue;
        }
        if fence || rx!(r"^( {4}|\t)").is_match(line) {
            let cmd = line.trim().strip_prefix("$ ").unwrap_or(line.trim());
            let first = cmd.split_whitespace().next().unwrap_or("");
            if !cmd.is_empty()
                && ([
                    "pip", "pip3", "python", "python3", "node", "npm", "npx", "yarn", "pnpm",
                    "cargo", "make", "go", "docker",
                ]
                .contains(&first)
                    || names.iter().any(|n| n == first)
                    || first.starts_with("./"))
                && commands.len() < 6
                && !commands.iter().any(|c: &Value| c["text"] == cmd)
            {
                commands.push(json!({"text":clip(cmd,200),"refs":[line_ref(path,i+1)]}));
            }
        }
        if fence {
            continue;
        }
        if line.trim().is_empty() || rx!(r"^#+\s|^[=-]{3,}\s*$").is_match(line) {
            flush(&mut paragraph, &mut purpose, start, i);
            continue;
        }
        if paragraph.is_empty() {
            start = i + 1;
        }
        paragraph.push(line.trim());
    }
    flush(
        &mut paragraph,
        &mut purpose,
        start,
        text.split('\n').count(),
    );
    (purpose, commands)
}
fn leading(file: &File) -> Value {
    let lines = file.text.split('\n').collect::<Vec<_>>();
    if file.path.ends_with(".py") {
        let mut i = 0;
        while i < lines.len() && rx!(r"^\s*(#!.*|#.*coding[:=].*)?$").is_match(lines[i]) {
            i += 1;
        }
        if let Some(c) = rx!(r#"^\s*[rRuU]?("""|''')(.*)$"#).captures(lines.get(i).unwrap_or(&"")) {
            let mut body = vec![c[2].to_owned()];
            let mut j = i;
            if !c[2].contains(&c[1]) {
                j = i + 1;
                while j < lines.len() && !lines[j].contains(&c[1]) {
                    body.push(lines[j].to_owned());
                    j += 1;
                }
                body.push(
                    lines
                        .get(j)
                        .unwrap_or(&"")
                        .split(&c[1])
                        .next()
                        .unwrap_or("")
                        .to_owned(),
                );
            }
            let text = body.join(" ").replacen(&c[1], "", 1).trim().to_owned();
            if !text.is_empty() {
                return json!({"text":clip(&text,300),"refs":[reference(&file.path,i+1,j+1)]});
            }
        }
        let comments = lines
            .iter()
            .skip(i)
            .take_while(|l| rx!(r"^\s*#").is_match(l))
            .map(|l| rx!(r"^\s*#\s?").replace(l, "").into_owned())
            .collect::<Vec<_>>();
        if !comments.join("").trim().is_empty() {
            return json!({"text":clip(comments.join(" ").trim(),300),"refs":[reference(&file.path,i+1,i+comments.len())]});
        }
    } else if rx!(r"\.([cm]?[jt]sx?|rs)$").is_match(&file.path) {
        let comments = lines
            .iter()
            .take_while(|l| rx!(r"^\s*(//|/\*|\*)").is_match(l))
            .map(|l| {
                rx!(r"^\s*(//[!/]?|/\*+|\*/?)\s?")
                    .replace(l, "")
                    .into_owned()
            })
            .collect::<Vec<_>>();
        if !comments.join("").trim().is_empty() {
            return json!({"text":clip(rx!(r"\*/\s*$").replace(&comments.join(" "),"").trim(),300),"refs":[reference(&file.path,1,comments.len())]});
        }
    }
    Value::Null
}
fn start_methods(model: &Model<'_>) -> Vec<Value> {
    let mut out = vec![];
    for s in &model.scripts {
        out.push(json!({"text":format!("package.json のコマンド「{}」: {}",string(&s["name"]),string(&s["command"])),"kind":"code","refs":[line_ref(string(&s["path"]),number(&s["line"]).max(1))]}));
    }
    if let Some(f) = model.files.iter().find(|f| f.path == "package.json")
        && let Ok(data) = serde_json::from_str::<Value>(&f.text)
    {
        let at = |key: &str| {
            f.text
                .split('\n')
                .position(|l| l.contains(&format!("\"{key}\"")))
                .map_or(1, |i| i + 1)
        };
        if let Some(main) = data["main"].as_str() {
            out.push(json!({"text":format!("package.json の main に {main} が指定されている"),"kind":"code","refs":[line_ref("package.json",at("main"))]}));
        }
        if !data["bin"].is_null() {
            out.push(json!({"text":format!("package.json の bin にコマンド {} が指定されている",data["bin"]),"kind":"code","refs":[line_ref("package.json",at("bin"))]}));
        }
    }
    for f in model
        .files
        .iter()
        .filter(|f| f.path == "setup.py")
        .chain(model.files.iter().filter(|f| f.path == "pyproject.toml"))
    {
        if f.path == "setup.py" {
            for (i, l) in f.text.split('\n').enumerate() {
                if let Some(c) =
                    rx!(r#"['"]\s*([\w.-]+)\s*=\s*([\w.]+):([\w.]+)\s*['"]"#).captures(l)
                {
                    out.push(json!({"text":format!("setup.py の console_scripts で、コマンド {} がモジュール {} の関数 {} に対応づけられている",&c[1],&c[2],&c[3]),"kind":"code","refs":[line_ref(&f.path,i+1)]}));
                }
            }
        }
        if f.path == "pyproject.toml" {
            let mut section = "";
            for (i, l) in f.text.split('\n').enumerate() {
                if let Some(c) = rx!(r"^\s*\[([^\]]+)\]").captures(l) {
                    section = c.get(1).expect("セクション名").as_str();
                    continue;
                }
                if section == "project.scripts"
                    && let Some(c) = rx!(r#"^\s*([\w.-]+)\s*=\s*"([\w.]+):([\w.]+)""#).captures(l)
                {
                    out.push(json!({"text":format!("pyproject.toml でコマンド {} が {} の {} に対応づけられている",&c[1],&c[2],&c[3]),"kind":"code","refs":[line_ref(&f.path,i+1)]}));
                }
            }
        }
    }
    for e in &model.entries {
        out.push(json!({"text":format!("{}: {}",string(&e["path"]),string(&e["reason"])),"kind":if rx!(r"__main__|Rust").is_match(string(&e["reason"])){"code"}else{"inference"},"refs":[line_ref(string(&e["path"]),number(&e["line"]))]}));
    }
    if let Some(f) = model.files.iter().find(|f| f.path == "Makefile") {
        for (i, l) in f.text.split('\n').enumerate() {
            if let Some(c) = rx!(r"^([A-Za-z][\w-]*):").captures(l) {
                out.push(json!({"text":format!("Makefile の目標「{}」",&c[1]),"kind":"code","refs":[line_ref(&f.path,i+1)]}));
            }
        }
    }
    let names = declared_commands(model);
    for f in model
        .files
        .iter()
        .filter(|f| snapshot::readme(&f.path) && !f.path.contains('/'))
    {
        for c in readme_parts(&f.path, &f.text, &names).1 {
            out.push(json!({"text":format!("README に書かれたコマンド: {}",string(&c["text"])),"kind":"author","refs":c["refs"]}));
        }
    }
    out
}
pub fn languages(s: &Snapshot) -> Vec<Value> {
    let mut counts: Vec<(&str, usize)> = vec![];
    for f in &s.inventory {
        let ext = f
            .path
            .rsplit_once('.')
            .map(|(_, s)| s.to_ascii_lowercase())
            .unwrap_or_default();
        let name = match ext.as_str() {
            "py" => "Python",
            "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
            "ts" | "tsx" => "TypeScript",
            "rs" => "Rust",
            "go" => "Go",
            "rb" => "Ruby",
            "java" => "Java",
            "kt" => "Kotlin",
            "php" => "PHP",
            "c" => "C",
            "cpp" => "C++",
            "cs" => "C#",
            "swift" => "Swift",
            _ => continue,
        };
        if let Some((_, n)) = counts.iter_mut().find(|(s, _)| *s == name) {
            *n += 1;
        } else {
            counts.push((name, 1));
        }
    }
    counts.sort_by_key(|a| std::cmp::Reverse(a.1));
    counts.into_iter().map(|(name,files)|json!({"name":name,"files":files,"execution":match name{"Python"=>"Python 3 で実行できる（標準ライブラリで動く範囲）".to_owned(),"JavaScript"=>"Node.js で実行できる（JavaScript。TypeScriptは変換が必要なため未対応）".to_owned(),_=>format!("{name} の実行は未対応。理解と試験計画だけを表示する")},"executable":name=="Python"||name=="JavaScript"})).collect()
}
pub fn tests(s: &Snapshot, m: &Model<'_>) -> Vec<Value> {
    s.inventory.iter().filter(|e|snapshot::test_file(&e.path)).map(|e|{let f=m.files.iter().find(|f|f.path==e.path);let kind=if e.path.ends_with(".t"){"cram（シェルのコマンドと期待する出力を並べた形式）"}else if e.path.ends_with(".py"){"Pythonのテスト"}else if rx!(r"\.[cm]?[jt]sx?$").is_match(&e.path){"JavaScriptのテスト"}else{"テスト"};json!({"path":e.path,"kind":kind,"read":f.is_some(),"refs":f.map_or_else(Vec::new,|f|vec![reference(&e.path,1,lines(&f.text).max(1))])})}).collect()
}
pub fn flows(model: &Model<'_>) -> Vec<Value> {
    let mut result = vec![];
    for entry in model.entries.iter().take(4) {
        let Some(file) = model
            .files
            .iter()
            .find(|f| f.path == string(&entry["path"]))
        else {
            continue;
        };
        let fns = functions(file);
        let line = number(&entry["line"]);
        let entry_line = file
            .text
            .split('\n')
            .nth(line.saturating_sub(1))
            .unwrap_or("");
        let mut queue = VecDeque::new();
        if let Some(f) = enclosing(&fns, line) {
            queue.push_back((file.path.to_owned(), number(&f["line"]), 0, Value::Null));
        } else {
            for (i, l) in file.text.split('\n').enumerate().skip(line).take(6) {
                for c in rx!(r"\b([A-Za-z_]\w*)\s*\(").captures_iter(l) {
                    if let Some(f) = fns
                        .iter()
                        .find(|f| f["short"] == c[1] && f["owner"].is_null())
                    {
                        queue.push_back((
                            file.path.to_owned(),
                            number(&f["line"]),
                            0,
                            line_ref(&file.path, i + 1),
                        ));
                    }
                }
            }
        }
        let mut steps = vec![];
        let mut seen = HashSet::new();
        let mut unresolved = 0;
        while !queue.is_empty() && steps.len() < 24 {
            let (path, line, depth, from) = queue.pop_front().expect("空でないキュー");
            if !seen.insert((path.clone(), line)) {
                continue;
            }
            let guide = explain(model, &path, line);
            if guide.is_null() {
                continue;
            }
            let Some(f) = model.files.iter().find(|f| f.path == path) else {
                continue;
            };
            let start = number(&guide["range"]["start"]);
            let end = number(&guide["range"]["end"]);
            let fns = functions(f);
            let name = fns
                .iter()
                .find(|f| f["line"] == start)
                .map_or(&guide["name"], |f| &f["name"]);
            let found = effects(f)
                .into_iter()
                .filter(|e| number(&e["line"]) >= start && number(&e["line"]) <= end)
                .map(|mut e| {
                    let obj = e.as_object_mut().expect("effectの構造");
                    for k in ["path", "function", "function_line"] {
                        obj.shift_remove(k);
                    }
                    e
                })
                .collect::<Vec<_>>();
            steps.push(json!({"function":name,"path":path,"line":start,"end_line":end,"depth":depth,"called_from":from,"effects":found,"refs":[reference(&path,start,end)]}));
            if depth >= 3 {
                continue;
            }
            for call in array(&guide["calls"]) {
                let candidates = array(&call["candidates"]);
                if candidates.is_empty() {
                    if !array(&call["unread"]).is_empty() {
                        unresolved += 1;
                    }
                    continue;
                }
                for c in candidates.iter().take(2) {
                    queue.push_back((
                        string(&c["path"]).to_owned(),
                        number(&c["line"]),
                        depth + 1,
                        line_ref(&path, number(&call["line"])),
                    ));
                }
            }
            for call in array(&guide["calls"])
                .iter()
                .filter(|c| array(&c["candidates"]).is_empty())
            {
                let name = string(&call["name"]);
                let method = name.rsplit_once('.').map(|(_, m)| m);
                for f in &model.files {
                    for fun in functions(f)
                        .into_iter()
                        .filter(|fun| {
                            !fun["owner"].is_null()
                                && if let Some(method) = method {
                                    fun["short"] == method
                                } else {
                                    fun["owner"] == name && fun["short"] == "__init__"
                                }
                        })
                        .take(2)
                    {
                        queue.push_back((
                            f.path.clone(),
                            number(&fun["line"]),
                            depth + 1,
                            line_ref(&path, number(&call["line"])),
                        ));
                    }
                }
            }
        }
        let pick = |kinds: &[&str]| {
            steps
                .iter()
                .flat_map(|s| {
                    array(&s["effects"])
                        .iter()
                        .filter(|e| kinds.contains(&string(&e["kind"])))
                        .map(|e| {
                            let mut e = e.clone();
                            e["function"] = s["function"].clone();
                            e
                        })
                })
                .collect::<Vec<_>>()
        };
        result.push(json!({"entry":{"path":entry["path"],"line":line,"reason":entry["reason"],"code":entry_line.trim()},"kind":"inference","basis":"入口から、名前と参照宣言で対応づけた呼び出し候補をたどった。分岐の条件・実際に実行される順序・到達は確かめていない","inputs":pick(&["input","env","read"]),"storage":pick(&["write","delete"]),"sending":pick(&["send"]),"outputs":pick(&["output","error_output"]),"errors":pick(&["raise","catch","exit"]),"steps":steps,"truncated":!queue.is_empty(),"unresolved_calls":unresolved}));
    }
    result
}
pub fn overview(s: &Snapshot, model: &Model<'_>) -> Value {
    let mut purpose = vec![];
    if let Some(d) = s.description.as_ref().filter(|s| !s.is_empty()) {
        purpose.push(json!({"text":d,"kind":"author","origin":"GitHubのリポジトリ説明（作者の記入）","refs":[]}));
    }
    for f in model
        .files
        .iter()
        .filter(|f| snapshot::readme(&f.path) && !f.path.contains('/'))
    {
        for mut p in readme_parts(&f.path, &f.text, &[]).0 {
            p["kind"] = json!("author");
            p["origin"] = json!(format!("{}（作者の説明）", f.path));
            purpose.push(p);
        }
    }
    let mut components = vec![];
    let mut behaviors = vec![];
    for f in model
        .files
        .iter()
        .filter(|f| language(&f.path).is_some() && !snapshot::test_file(&f.path))
    {
        let found = effects(f);
        let fns = functions(f);
        let mut desc = leading(f);
        if !desc.is_null() {
            desc["kind"] = json!("author");
            desc["origin"] = json!("コード中のコメント・説明文");
        }
        let mut counts = Map::new();
        for e in &found {
            let k = string(&e["kind"]);
            let count = counts.get(k).map_or(0, number) + 1;
            counts.insert(k.to_owned(), json!(count));
        }
        components.push(json!({"path":f.path,"role":f.role,"lines":lines(&f.text),"description":desc,"functions":fns.iter().take(40).map(|fun|json!({"name":fun["name"],"line":fun["line"],"end_line":fun["end"],"refs":[reference(&f.path,number(&fun["line"]),number(&fun["end"]))]})).collect::<Vec<_>>(),"effect_counts":counts,"refs":[reference(&f.path,1,lines(&f.text).max(1))]}));
        let mut by: Vec<(&str, Vec<&Value>)> = vec![];
        for e in &found {
            if let Some(name) = e["function"].as_str() {
                if let Some((_, list)) = by.iter_mut().find(|(n, _)| *n == name) {
                    list.push(e);
                } else {
                    by.push((name, vec![e]));
                }
            }
        }
        for (name, list) in by {
            let mut kinds = vec![];
            let mut labels = vec![];
            for e in &list {
                let k = string(&e["kind"]);
                if !kinds.contains(&k) {
                    kinds.push(k);
                }
                let l = string(&e["label"]);
                if !labels.contains(&l) {
                    labels.push(l);
                }
            }
            let fun = fns.iter().find(|x| x["name"] == name);
            let phrases = labels
                .iter()
                .map(|label| {
                    format!(
                        "{label}（{}行目）",
                        list.iter()
                            .filter(|e| e["label"] == *label)
                            .map(|e| e["line"].to_string())
                            .collect::<Vec<_>>()
                            .join("・")
                    )
                })
                .collect::<Vec<_>>();
            behaviors.push(json!({"function":name,"path":f.path,"line":fun.map_or(&list[0]["line"],|f|&f["line"]),"text":format!("{name} には、{}がある",phrases.join("、")),"kind":"code","kinds":kinds,"refs":list.iter().take(8).map(|e|e["refs"][0].clone()).collect::<Vec<_>>()}));
        }
    }
    let mut unread = s
        .omitted
        .iter()
        .map(|o| json!({"path":o.path,"reason":o.reason}))
        .collect::<Vec<_>>();
    for e in &s.inventory {
        if !s.files.contains_key(&e.path) && !s.omitted.iter().any(|o| o.path == e.path) {
            unread.push(json!({"path":e.path,"reason":"解析に対応していない種類（未対応の言語、画像、設定など）"}));
        }
    }
    let mut unknown = vec![];
    if s.truncated {
        unknown.push(json!({"text":"GitHubのファイル一覧が途中で切れている。一覧にないファイルがある可能性がある","kind":"code","refs":[]}));
    }
    if !unread.is_empty() {
        unknown.push(json!({"text":format!("読み取っていないファイルが {} 件ある。必要に応じて読み込む",unread.len()),"kind":"code","refs":[]}));
    }
    for text in [
        "README やコメントの説明が、現在のコードの動作と一致するかは確かめていない",
        "処理の関係は、名前と参照宣言から作った呼び出し候補。実際にどの経路が実行されるかは、試験で確かめる必要がある",
        "外部サービス、実行環境の設定、利用者の権限、保存先の実際の状態は、コードを読むだけでは確かめられない",
    ] {
        unknown.push(json!({"text":text,"kind":"inference","refs":[]}));
    }
    json!({"schema_version":1,"repository":s.full_name,"commit":s.commit,"reference":s.reference,"read":{"inventory":s.inventory.len(),"files_read":model.files.len(),"unread":unread,"truncated":s.truncated},"languages":languages(s),"purpose":purpose,"start":start_methods(model),"components":components,"behaviors":behaviors,"flows":flows(model),"tests":tests(s,model),"unknown":unknown,"kinds":{"author":"作者の説明（README・コメント等）。コードで確かめていない","code":"コードの記述から確認した事実","inference":"解析による推測。実行して確かめていない"},"limits":"コードの記述から作った説明です。実行していないため、実際の動作・到達・成否は試験で確かめる必要があります。"})
}
