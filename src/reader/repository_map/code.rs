//! 明示された起動・接続の記述を役割にまとめる。依存パッケージの一覧だけではノードを作らない。
use super::super::analysis::{line_at, mask};
use super::*;
use regex::Regex;

struct Signal {
    role: &'static str,
    technology: &'static str,
    reference: Reference,
    value: String,
}
fn signals(s: &Snapshot) -> Vec<Signal> {
    let mut result = vec![];
    for (path, value) in &s.files {
        if snapshot::test_file(path)
            || !(snapshot::source(path) || path.ends_with(".html"))
            || path
                .split('/')
                .any(|p| matches!(p, "examples" | "fixtures" | "scripts" | "tools" | "cdk"))
        {
            continue;
        }
        let Some(text) = value.as_str() else {
            continue;
        };
        let masked = mask(text, path.ends_with(".rs"), path.ends_with(".py"));
        if path.ends_with(".py")
            && text.starts_with("#!")
            && text.lines().next().is_some_and(|l| l.contains("python"))
        {
            result.push(Signal {
                role: "application",
                technology: "Python",
                reference: Reference::new(path, 1),
                value: String::new(),
            });
        }
        let mut scan = |role, tech, re: &Regex| {
            for capture in re.captures_iter(text).take(60) {
                let Some(m) = capture.get(0) else {
                    continue;
                };
                if masked[m.start()..m.end()].trim().is_empty() {
                    continue;
                }
                result.push(Signal {
                    role,
                    technology: tech,
                    reference: Reference::new(path, line_at(text, m.start())),
                    value: capture
                        .get(1)
                        .map_or("", |v| v.as_str())
                        .chars()
                        .take(160)
                        .collect(),
                });
            }
        };
        scan(
            "ui",
            "Web UI",
            rx!(r"\b(?:createRoot|hydrateRoot|createApp)\s*\("),
        );
        scan("ui", "HTML", rx!(r"(?i)<(?:!doctype\s+html|html)(?:\s|>)"));
        scan(
            "ui",
            "デスクトップUI",
            rx!(r"\b(?:QApplication|Tk|CTk)\s*\("),
        );
        scan("api", "FastAPI", rx!(r"\bFastAPI\s*\("));
        scan("api", "Flask", rx!(r"\bFlask\s*\("));
        scan("api", "Express", rx!(r"\bexpress\s*\("));
        scan(
            "api",
            "HTTP server",
            rx!(r"\b(?:http\.createServer|HttpServer::new)\s*\("),
        );
        scan("api", "TCP server", rx!(r"\bTcpListener::bind\s*\("));
        scan("application", "Rust", rx!(r"\bfn\s+main\s*\("));
        scan(
            "application",
            "Python",
            rx!(r#"if\s+__name__\s*==\s*["']__main__["']"#),
        );
        scan(
            "route",
            "HTTP",
            rx!(
                r#"(?:@(?:app|router|api)\.(?:get|post|put|patch|delete)|\b(?:app|router)\.(?:get|post|put|patch|delete))\s*\(\s*["'`]([^"'`]+)"#
            ),
        );
        scan(
            "http",
            "HTTP",
            rx!(
                r#"\b(?:fetch|axios\.(?:get|post|put|patch|delete)|requests\.(?:get|post|put|patch|delete))\s*\(\s*["'`]([^"'`]+)"#
            ),
        );
        scan("http", "HTTP", rx!(r"\bfetch\s*\(\s*[A-Za-z_$]"));
        scan(
            "database",
            "DB接続",
            rx!(
                r"\b(?:create_engine|sqlite3\.connect|psycopg\.connect|psycopg2\.connect|mysql\.createConnection|mongoose\.connect)\s*\("
            ),
        );
        scan(
            "sqlite",
            "SQLite",
            rx!(r"\b(?:sqlite3\.connect|Database\.open|rusqlite::Connection::open)\s*\("),
        );
        scan(
            "dynamodb",
            "DynamoDB",
            rx!(
                r#"\b(?:boto3\.(?:resource|client)\s*\(\s*["']dynamodb["']|DynamoDBClient\s*\(|DynamoDBDocumentClient\.from\s*\()"#
            ),
        );
        scan(
            "s3",
            "S3",
            rx!(r#"\b(?:boto3\.(?:resource|client)\s*\(\s*["']s3["']|S3Client\s*\()"#),
        );
        scan(
            "auth",
            "Cognito",
            rx!(
                r#"\b(?:CognitoIdentityProviderClient\s*\(|boto3\.client\s*\(\s*["']cognito-idp["'])"#
            ),
        );
        scan(
            "auth_url",
            "Cognito",
            rx!(r#"\breturn\s+f?["']https://cognito-idp\.[^"'\n]+"#),
        );
        scan(
            "storage",
            "ファイル",
            rx!(
                r"\b(?:fs::write|fs::read_to_string|writeFileSync|readFileSync|writeFile|readFile)\s*\(|\.(?:write_bytes|write_text|read_bytes)\s*\("
            ),
        );
        scan(
            "storage",
            "ファイル",
            rx!(r#"\bopen\s*\(\s*(?:[rbuf]*["'][^"'\n]+["']|[^,\n]+),\s*["'][rwa][bt+]*["']"#),
        );
    }
    result
}
fn language(path: &str) -> &'static str {
    if path.ends_with(".py") {
        "Python"
    } else if path.ends_with(".rs") {
        "Rust"
    } else {
        "JavaScript / TypeScript"
    }
}
pub(super) fn build(b: &mut Builder<'_>) {
    let all = signals(b.snapshot);
    for signal in all
        .iter()
        .filter(|s| matches!(s.role, "ui" | "api" | "application"))
    {
        let root = root_for(b.snapshot, &signal.reference.path);
        let role = if signal.role == "application"
            && all
                .iter()
                .any(|s| s.role == "api" && root_for(b.snapshot, &s.reference.path) == root)
        {
            "api"
        } else {
            signal.role
        };
        if signal.role == "ui"
            && signal.technology == "HTML"
            && all.iter().any(|s| {
                s.role == "ui"
                    && s.technology != "HTML"
                    && root_for(b.snapshot, &s.reference.path) == root
            })
        {
            continue;
        }
        let candidates: Vec<_> = b
            .nodes
            .iter()
            .filter(|n| {
                (n.role == role || n.service.is_some() && n.role == "application" && role != "ui")
                    && (n.root == root
                        || n.service.is_some() && within(&signal.reference.path, &n.root))
            })
            .collect();
        let depth = candidates.iter().map(|n| n.root.len()).max();
        let deepest: Vec<_> = candidates
            .iter()
            .filter(|n| Some(n.root.len()) == depth)
            .collect();
        let exact: Vec<_> = deepest.iter().filter(|n| n.role == role).collect();
        let selected: Vec<_> = if exact.is_empty() {
            deepest.iter().map(|n| &n.element.id).collect()
        } else {
            exact.iter().map(|n| &n.element.id).collect()
        };
        if selected.len() > 1 {
            let notice = format!(
                "{} のコードを複数のサービスへ一意に対応づけられませんでした。",
                if root.is_empty() { "ルート" } else { &root }
            );
            if !b.notices.contains(&notice) {
                b.notices.push(notice);
            }
            continue;
        }
        let existing = selected.first().map(|id| (*id).clone());
        let node = existing.unwrap_or_else(|| {
            b.node(
                &format!("app:{root}:{role}"),
                &root,
                role,
                signal.technology,
                "",
                signal.reference.clone(),
                BasisKind::Inference,
            )
        });
        if let Some(n) = b.nodes.iter_mut().find(|n| n.element.id == node) {
            if n.service.is_some() && n.root.len() < root.len() {
                n.root = root.clone();
            }
            if n.role == "application" && role == "api" {
                n.role = role.into();
                n.element.title = format!(
                    "要求を処理するアプリ（{}）",
                    n.service.as_deref().unwrap_or("API")
                );
            }
            if !n.technology.iter().any(|t| t == signal.technology) {
                n.technology.push(signal.technology.into());
            }
            if n.technical_title.is_empty() {
                n.technical_title = signal.technology.into();
            }
        }
        b.detail(
            &node,
            "起動・表示",
            &format!(
                "{}の{}を確認できます。",
                language(&signal.reference.path),
                signal.technology
            ),
            signal.reference.clone(),
        );
    }
    if b.nodes
        .iter()
        .all(|n| !matches!(n.role.as_str(), "api" | "application" | "ui"))
    {
        for (path, text) in &b.snapshot.files {
            let Some(text) = text.as_str() else {
                continue;
            };
            if path.ends_with("package.json")
                && let Ok(p) = serde_json::from_str::<Value>(text)
                && let Some(scripts) = p["scripts"].as_object()
            {
                for key in ["start", "dev"] {
                    if let Some(cmd) = scripts.get(key).and_then(Value::as_str) {
                        let root = path.rsplit_once('/').map_or("", |(r, _)| r);
                        let role = if cmd
                            .split_whitespace()
                            .any(|s| matches!(s, "vite" | "next" | "react-scripts"))
                        {
                            "ui"
                        } else {
                            "application"
                        };
                        let tech = if role == "ui" { "Web UI" } else { "Node.js" };
                        let r = Reference::new(
                            path,
                            text.lines()
                                .position(|l| l.contains(&format!("\"{key}\"")))
                                .unwrap_or(0)
                                + 1,
                        );
                        let node = b.node(
                            &format!("app:{root}:{role}"),
                            root,
                            role,
                            tech,
                            "",
                            r.clone(),
                            BasisKind::Configuration,
                        );
                        b.detail(
                            &node,
                            "起動設定",
                            &format!("package.json の {key} スクリプトで起動します。"),
                            r,
                        );
                        break;
                    }
                }
            }
        }
    }
    for signal in &all {
        if let Some(owner) = b.owner(&signal.reference.path) {
            match signal.role {
                "route" => b.detail(
                    &owner,
                    "受け付ける要求",
                    &format!("HTTP {}", signal.value),
                    signal.reference.clone(),
                ),
                "database" | "sqlite" | "dynamodb" | "s3" | "storage" | "auth" | "auth_url" => {
                    storage(b, &owner, signal, &all)
                }
                "http" => http(b, &owner, signal, &all),
                _ => (),
            }
        }
    }
    let uis: Vec<_> = b
        .nodes
        .iter()
        .filter(|n| n.role == "ui")
        .map(|n| {
            (
                n.element.id.clone(),
                n.root.clone(),
                n.refs.first().cloned(),
            )
        })
        .collect();
    for (ui, root, reference) in uis {
        let apis: Vec<_> = b
            .nodes
            .iter()
            .filter(|n| {
                n.role == "api"
                    && n.root == root
                    && n.technology.iter().any(|t| {
                        matches!(
                            t.as_str(),
                            "FastAPI" | "Flask" | "Express" | "HTTP server" | "Python API"
                        )
                    })
            })
            .map(|n| (n.element.id.clone(), n.refs.first().cloned()))
            .collect();
        if apis.len() == 1
            && let (Some(r), Some(a)) = (reference, apis[0].1.clone())
        {
            b.edge(&ui,&apis[0].0,"画面を開く・操作する","HTML画面とAPIが同じアプリに記述されています。対応は静的な推定で、実際の配信は未確認です。","HTTP","request",vec![r,a],BasisKind::Inference);
        }
    }
}
fn storage(b: &mut Builder<'_>, owner: &str, s: &Signal, all: &[Signal]) {
    let role = match s.role {
        "storage" | "s3" => "storage",
        "auth" | "auth_url" => "auth",
        _ => "database",
    };
    if matches!(s.role, "database" | "sqlite")
        && b.edges.iter().any(|e| {
            e.relation.from == owner
                && e.relation_type == "data"
                && b.nodes.iter().any(|n| {
                    n.element.id == e.relation.to && n.role == "database" && n.service.is_some()
                })
        })
    {
        b.detail(
            owner,
            "保存の処理・代替設定",
            "DB接続の記述があります。図には選んだComposeの接続先を表示しています。",
            s.reference.clone(),
        );
        return;
    }
    let owner_root = b
        .nodes
        .iter()
        .find(|n| n.element.id == owner)
        .map(|n| n.root.clone())
        .unwrap_or_default();
    if s.role == "database"
        && all.iter().any(|a| {
            matches!(a.role, "sqlite" | "dynamodb")
                && root_for(b.snapshot, &a.reference.path) == owner_root
        })
    {
        b.detail(
            owner,
            "保存の処理",
            "データベース接続の呼び出しがあります。",
            s.reference.clone(),
        );
        return;
    }
    let technology = if s.role == "database" {
        "DB（種類未特定）"
    } else {
        s.technology
    };
    let key = format!(
        "resource:{owner_root}:{}",
        if s.role == "database" {
            "database"
        } else if s.role == "auth_url" {
            "auth"
        } else {
            s.role
        }
    );
    let target = b.node(
        &key,
        &owner_root,
        role,
        technology,
        "",
        s.reference.clone(),
        BasisKind::Inference,
    );
    let (label, protocol) = if s.role == "auth_url" {
        ("認証先の設定", "Cognito URL設定")
    } else if role == "auth" {
        ("本人確認", "認証方式は要確認")
    } else if role == "storage" {
        (
            "ファイルの保存・取得",
            if s.role == "s3" {
                "S3 SDK"
            } else {
                "ファイルI/O"
            },
        )
    } else {
        ("記録の保存・取得", technology)
    };
    let note = if s.role == "auth_url" {
        "認証サービスのURLを組み立てる設定がコードにあります。この線は実際の通信を表しません。"
    } else {
        "接続・読み書きの呼び出しがコードにあります。接続先の実体、実行時の有効性と成功は未確認です。"
    };
    b.edge(
        owner,
        &target,
        label,
        note,
        protocol,
        if s.role == "auth_url" {
            "configuration"
        } else if role == "auth" {
            "auth"
        } else {
            "data"
        },
        vec![s.reference.clone()],
        BasisKind::Inference,
    );
    b.detail(
        &target,
        "接続の記述",
        &format!("{}を使う呼び出しがあります。", s.technology),
        s.reference.clone(),
    );
}
fn http(b: &mut Builder<'_>, owner: &str, s: &Signal, all: &[Signal]) {
    if let Ok(url) = url::Url::parse(&s.value)
        && let Some(host) = url.host_str()
        && matches!(url.scheme(), "https" | "http")
    {
        let known = b
            .nodes
            .iter()
            .find(|n| n.service.as_deref() == Some(host))
            .map(|n| n.element.id.clone());
        let target = known.unwrap_or_else(|| {
            b.node(
                &format!("http:{host}"),
                "",
                "external",
                host,
                host,
                s.reference.clone(),
                BasisKind::Inference,
            )
        });
        b.edge(
            owner,
            &target,
            "外部へ要求を送る",
            "コードにHTTP要求の接続先が記述されています。送信する情報と応答は検証して確認します。",
            if url.scheme() == "https" {
                "HTTPS"
            } else {
                "HTTP"
            },
            "request",
            vec![s.reference.clone()],
            BasisKind::Inference,
        );
        return;
    }
    let servers: Vec<_> = b
        .nodes
        .iter()
        .filter(|n| n.role == "api" && n.element.id != owner)
        .map(|n| (n.element.id.clone(), n.refs.first().cloned()))
        .collect();
    let is_ui = b
        .nodes
        .iter()
        .any(|n| n.element.id == owner && n.role == "ui");
    let matching = all.iter().any(|r| {
        r.role == "route"
            && (s.value.is_empty()
                || s.value.starts_with(&r.value)
                || r.value.starts_with(&s.value))
    });
    if is_ui
        && servers.len() == 1
        && matching
        && let Some(reference) = servers[0].1.clone()
    {
        b.edge(owner,&servers[0].0,"処理を依頼し、結果を受け取る","画面のHTTP要求と、リポジトリ内のAPIを対応づけた推定です。環境変数や配信設定によって別の接続先になる場合があります。","HTTP / API","request",vec![s.reference.clone(),reference],BasisKind::Inference);
    } else {
        b.detail(
            owner,
            "接続先が未特定の要求",
            "HTTP要求の記述がありますが、接続先を構成要素に対応づけられていません。",
            s.reference.clone(),
        );
    }
}
