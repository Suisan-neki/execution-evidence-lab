//! Composeのサービスと明示的な接続設定。起動の依存をデータ通信と取り違えない。
use super::*;

fn value_text(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    })
}
fn service_role(image: &str, command: &str) -> (&'static str, &'static str) {
    let image = image.to_lowercase();
    let base = image
        .rsplit('/')
        .next()
        .unwrap_or(&image)
        .split([':', '@'])
        .next()
        .unwrap_or("");
    match base {
        "postgres" | "postgresql" => ("database", "PostgreSQL"),
        "mysql" | "mariadb" => ("database", "MySQL / MariaDB"),
        "mongo" | "mongodb" => ("database", "MongoDB"),
        "redis" | "valkey" => ("cache", "Redis / Valkey"),
        "rabbitmq" | "kafka" | "nats" => ("queue", "メッセージキュー"),
        "minio" => ("storage", "S3互換ストレージ"),
        "keycloak" => ("auth", "Keycloak"),
        _ if command.contains("uvicorn")
            || command.contains("gunicorn")
            || command.contains("flask") =>
        {
            ("api", "Python API")
        }
        _ if command
            .split_whitespace()
            .any(|p| matches!(p, "vite" | "next" | "react-scripts")) =>
        {
            ("ui", "Web UI")
        }
        _ => ("application", "コンテナ"),
    }
}
fn line_for(text: &str, name: &str) -> usize {
    text.lines()
        .position(|l| l.trim_start().starts_with(&format!("{name}:")))
        .unwrap_or(0)
        + 1
}
fn context(path: &str, v: &Value) -> Option<String> {
    let c = v.as_str().or_else(|| v["context"].as_str()).unwrap_or(".");
    if c.contains("://") || c.starts_with('/') || c.contains('$') {
        return None;
    }
    let base = path.rsplit_once('/').map_or("", |(p, _)| p);
    let mut parts: Vec<_> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in c.split('/') {
        match p {
            "" | "." => (),
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(p),
        }
    }
    Some(parts.join("/"))
}
fn env(v: &Value) -> Vec<(String, String)> {
    if let Some(map) = v.as_object() {
        map.iter()
            .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.into())))
            .collect()
    } else {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter_map(|v| v.split_once('='))
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect()
            })
            .unwrap_or_default()
    }
}
pub(super) fn build(b: &mut Builder<'_>) {
    let mut files: Vec<_> = b
        .snapshot
        .files
        .iter()
        .filter(|(p, _)| {
            snapshot::architecture_config(p)
                && (p.ends_with(".yml") || p.ends_with(".yaml"))
                && p.rsplit('/')
                    .next()
                    .is_some_and(|s| s.starts_with("compose") || s.starts_with("docker-compose"))
        })
        .collect();
    files.sort_by_key(|(p, _)| {
        let file = p.rsplit('/').next().unwrap_or(p);
        (
            !matches!(
                file,
                "compose.yaml" | "compose.yml" | "docker-compose.yml" | "docker-compose.yaml"
            ),
            p.matches('/').count(),
            p.as_str(),
        )
    });
    let Some((path, raw)) = files.first().copied() else {
        return;
    };
    let text = raw.as_str().unwrap_or_default();
    let Ok(config) = serde_yaml_ng::from_str::<Value>(text) else {
        b.notices
            .push(format!("{path} を構成定義として解析できませんでした。"));
        return;
    };
    let Some(services) = config["services"].as_object() else {
        b.notices
            .push(format!("{path} にサービス定義がありません。"));
        return;
    };
    if services.len() > 40 {
        b.notices.push(format!(
            "{path} のサービス数が40を超えています。構成定義を分けて確認してください。"
        ));
        return;
    }
    b.scope = format!("{path} のサービス定義と、関連するコード");
    if files.len() > 1 {
        b.notices.push(format!(
            "複数のCompose定義があります。図には {path} を使い、他の環境の定義は混ぜていません。"
        ));
    }
    let mut ids = BTreeMap::new();
    for (name, service) in services {
        let image = service["image"].as_str().unwrap_or("");
        let cmd = value_text(&service["command"]);
        let (role, technology) = service_role(image, &cmd);
        let root = context(path, &service["build"]).unwrap_or_default();
        let reference = Reference::new(path, line_for(text, name));
        let node = b.node(
            &format!("compose:{path}:{name}"),
            &root,
            role,
            technology,
            name,
            reference.clone(),
            BasisKind::Configuration,
        );
        if let Some(n) = b.nodes.iter_mut().find(|n| n.element.id == node) {
            n.service = Some(name.clone());
        }
        ids.insert(name.clone(), node.clone());
        if !image.is_empty() {
            b.detail(&node, "起動イメージ", image, reference.clone());
        }
        if let Some(ports) = service["ports"].as_array() {
            let ports: Vec<_> = ports
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if !ports.is_empty() {
                b.detail(
                    &node,
                    "公開ポート",
                    &ports.join("、"),
                    Reference::new(path, line_for(text, "ports")),
                );
            }
        }
        if service.get("profiles").is_some() {
            b.detail(
                &node,
                "起動する条件",
                "Composeのprofiles指定があります。すべてのサービスが同時に起動するとは限りません。",
                reference.clone(),
            );
        }
        if !service["build"].is_null() {
            b.detail(
                &node,
                "実装のまとまり",
                &format!(
                    "ビルド対象：{}",
                    if root.is_empty() {
                        "リポジトリ全体"
                    } else {
                        &root
                    }
                ),
                reference,
            );
        }
    }
    for (name, service) in services {
        let Some(from) = ids.get(name) else {
            continue;
        };
        for (key, value) in env(&service["environment"]) {
            let Ok(url) = url::Url::parse(&value) else {
                continue;
            };
            let Some(host) = url.host_str() else {
                continue;
            };
            if host.contains('$') {
                continue;
            }
            let reference = Reference::new(
                path,
                text.lines()
                    .position(|l| l.contains(&key) && l.contains(&value))
                    .unwrap_or_else(|| line_for(text, name) - 1)
                    + 1,
            );
            let target = if let Some(target) = ids.get(host) {
                target.clone()
            } else if matches!(url.scheme(), "http" | "https") {
                b.node(
                    &format!("http:{host}"),
                    "",
                    "external",
                    host,
                    host,
                    reference.clone(),
                    BasisKind::Configuration,
                )
            } else {
                continue;
            };
            let role = b
                .nodes
                .iter()
                .find(|n| n.element.id == target)
                .map(|n| n.role.as_str())
                .unwrap_or("");
            let (title, kind) = match role {
                "database" => ("記録の保存・取得", "data"),
                "cache" => ("一時データの送受信", "data"),
                "queue" => ("仕事・メッセージを渡す", "data"),
                "storage" => ("ファイルの保存・取得", "data"),
                _ => ("要求を送る", "request"),
            };
            b.edge(from,&target,title,&format!("環境変数 {key} に接続先が指定されています。実際の通信や保存の成功は未確認です。"),url.scheme(),kind,vec![reference],BasisKind::Configuration);
        }
        let dependencies: Vec<_> = if let Some(a) = service["depends_on"].as_array() {
            a.iter().filter_map(Value::as_str).collect()
        } else {
            service["depends_on"]
                .as_object()
                .map(|m| m.keys().map(String::as_str).collect())
                .unwrap_or_default()
        };
        for dependency in dependencies {
            let Some(to) = ids.get(dependency) else {
                continue;
            };
            let r = Reference::new(path, line_for(text, name));
            if b.edges
                .iter()
                .any(|e| e.relation.from == *from && e.relation.to == *to)
            {
                b.detail(
                    from,
                    "起動の依存",
                    &format!(
                        "{dependency}へのdepends_on指定があります。通信や保存の証明とは別です。"
                    ),
                    r,
                );
            } else {
                b.edge(
                    from,
                    to,
                    "起動を待つ",
                    "Composeのdepends_on指定です。データの送受信を表す線ではありません。",
                    "Compose depends_on",
                    "startup",
                    vec![r],
                    BasisKind::Configuration,
                );
            }
        }
    }
}
