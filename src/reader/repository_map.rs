//! サービス・責務と接続を固定版から組み立てる。ファイルやimportを構成要素・通信にしない。
mod code;
mod compose;
#[cfg(test)]
mod tests;

use super::snapshot::{self, Snapshot};
use crate::architecture::{self, Basis, BasisKind, Element, ElementKind, Environment, Relation};
use crate::workspace::CodeReference;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io};

#[derive(Clone, Serialize)]
pub(super) struct Reference {
    path: String,
    line: usize,
    end_line: usize,
}
impl Reference {
    fn new(path: &str, line: usize) -> Self {
        Self {
            path: path.into(),
            line,
            end_line: line,
        }
    }
}
#[derive(Clone, Serialize)]
pub(super) struct Detail {
    title: String,
    text: String,
    refs: Vec<Reference>,
}
#[derive(Clone, Serialize)]
pub(super) struct Node {
    #[serde(flatten)]
    element: Element,
    technical_title: String,
    technology: Vec<String>,
    role: String,
    refs: Vec<Reference>,
    details: Vec<Detail>,
    scope_note: Option<String>,
    #[serde(skip)]
    root: String,
    #[serde(skip)]
    service: Option<String>,
}
#[derive(Clone, Serialize)]
pub(super) struct Edge {
    #[serde(flatten)]
    relation: Relation,
    protocol: String,
    relation_type: String,
    refs: Vec<Reference>,
}
struct Builder<'a> {
    snapshot: &'a Snapshot,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    notices: Vec<String>,
    scope: String,
}

fn id(key: &str) -> String {
    format!("r-{:x}", Sha256::digest(key.as_bytes()))[..26].to_owned()
}
fn within(path: &str, root: &str) -> bool {
    root.is_empty() || path == root || path.strip_prefix(root).is_some_and(|s| s.starts_with('/'))
}
fn root_for(s: &Snapshot, path: &str) -> String {
    let conventional = path.split('/').next().filter(|p| {
        matches!(
            *p,
            "client"
                | "frontend"
                | "web"
                | "ui"
                | "server"
                | "server_python"
                | "backend"
                | "api"
                | "service"
        )
    });
    let manifest = s
        .inventory
        .iter()
        .filter(|e| snapshot::manifest(&e.path))
        .filter_map(|e| e.path.rsplit_once('/').map(|(p, _)| p))
        .filter(|p| within(path, p))
        .max_by_key(|p| p.len());
    let entry = s
        .inventory
        .iter()
        .filter(|e| !snapshot::test_file(&e.path))
        .filter(|e| {
            e.path.rsplit('/').next().is_some_and(|p| {
                matches!(
                    p,
                    "main.py" | "main.rs" | "main.tsx" | "main.jsx" | "server.js" | "server.ts"
                )
            })
        })
        .filter_map(|e| {
            e.path
                .rsplit_once('/')
                .map(|(r, _)| r.strip_suffix("/src").unwrap_or(r))
        })
        .filter(|r| within(path, r))
        .max_by_key(|r| r.len());
    [manifest, entry, conventional]
        .into_iter()
        .flatten()
        .max_by_key(|r| r.len())
        .unwrap_or("")
        .into()
}
fn boundary(role: &str) -> &'static str {
    match role {
        "ui" => "client",
        "database" | "storage" | "cache" | "queue" => "data",
        "external" | "auth" => "external",
        _ => "application",
    }
}
fn role_info(role: &str) -> (&'static str, &'static str, ElementKind) {
    match role {
        "ui" => (
            "操作画面",
            "利用者が入力し、処理の結果を見る画面です。",
            ElementKind::App,
        ),
        "api" => (
            "要求を処理するアプリ",
            "画面や他のプログラムからの要求を受け取り、処理して結果を返します。",
            ElementKind::App,
        ),
        "database" => (
            "記録の保存先",
            "アプリが扱う記録を保存し、必要なときに取り出す場所です。",
            ElementKind::Database,
        ),
        "storage" => (
            "ファイルの保存先",
            "アプリがファイルを保存し、読み出す場所です。",
            ElementKind::Storage,
        ),
        "cache" => (
            "一時データの保存先",
            "一時的なデータを扱うサービスです。用途と保持期間は設定や処理の確認が必要です。",
            ElementKind::Database,
        ),
        "queue" => (
            "処理を受け渡す仕組み",
            "別の処理へ仕事やメッセージを渡すサービスです。",
            ElementKind::Component,
        ),
        "auth" => (
            "ログイン・本人確認",
            "利用者のログインや本人確認に関わるサービスです。",
            ElementKind::System,
        ),
        "external" => (
            "外部のサービス",
            "アプリから要求を送る接続先です。渡す情報と応答は個別に確認します。",
            ElementKind::System,
        ),
        _ => (
            "アプリケーション",
            "起動する処理をまとめた部分です。詳しい役割は説明とコードから確認します。",
            ElementKind::App,
        ),
    }
}
impl<'a> Builder<'a> {
    fn basis(&self, kind: BasisKind, note: &str, reference: &Reference) -> Basis {
        Basis {
            kind,
            note: note.into(),
            source: Some(CodeReference {
                repository: self.snapshot.full_name.clone(),
                commit: self.snapshot.commit.clone(),
                path: reference.path.clone(),
                line: reference.line as u32,
                end_line: reference.end_line as u32,
            }),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        key: &str,
        root: &str,
        role: &str,
        tech: &str,
        name: &str,
        reference: Reference,
        kind: BasisKind,
    ) -> String {
        let object = id(key);
        if let Some(existing) = self.nodes.iter_mut().find(|n| n.element.id == object) {
            if !tech.is_empty() && !existing.technology.iter().any(|t| t == tech) {
                existing.technology.push(tech.into());
            }
            add_ref(&mut existing.refs, reference);
            return object;
        }
        let (title, description, element_kind) = role_info(role);
        let basis = self.basis(
            kind,
            "構成定義または呼び出しの記述。実際の配備・実行は未確認。",
            &reference,
        );
        self.nodes.push(Node {
            element: Element {
                id: object.clone(),
                parent: Some(boundary(role).into()),
                kind: element_kind,
                title: if name.is_empty() {
                    title.into()
                } else {
                    format!("{title}（{name}）")
                },
                description: description.into(),
                basis,
            },
            technical_title: if name.is_empty() {
                tech.into()
            } else {
                name.into()
            },
            technology: if tech.is_empty() {
                vec![]
            } else {
                vec![tech.into()]
            },
            role: role.into(),
            refs: vec![reference],
            details: vec![],
            scope_note: None,
            root: root.into(),
            service: None,
        });
        object
    }
    fn detail(&mut self, node: &str, title: &str, text: &str, reference: Reference) {
        if let Some(n) = self.nodes.iter_mut().find(|n| n.element.id == node) {
            add_ref(&mut n.refs, reference.clone());
            if let Some(d) = n
                .details
                .iter_mut()
                .find(|d| d.title == title && d.text == text)
            {
                add_ref(&mut d.refs, reference);
            } else if n.details.len() < 60 {
                n.details.push(Detail {
                    title: title.into(),
                    text: text.into(),
                    refs: vec![reference],
                });
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn edge(
        &mut self,
        from: &str,
        to: &str,
        title: &str,
        description: &str,
        protocol: &str,
        relation_type: &str,
        refs: Vec<Reference>,
        kind: BasisKind,
    ) {
        if from == to || refs.is_empty() {
            return;
        }
        let object = id(&format!("edge:{from}:{to}:{relation_type}"));
        if let Some(e) = self.edges.iter_mut().find(|e| e.relation.id == object) {
            for r in refs {
                add_ref(&mut e.refs, r);
            }
            return;
        }
        self.edges.push(Edge {
            relation: Relation {
                id: object,
                from: from.into(),
                to: to.into(),
                title: title.into(),
                description: description.into(),
                basis: self.basis(kind, description, &refs[0]),
            },
            protocol: protocol.into(),
            relation_type: relation_type.into(),
            refs,
        });
    }
    fn owner(&self, path: &str) -> Option<String> {
        let root = root_for(self.snapshot, path);
        let web = path.ends_with(".html")
            || path.ends_with(".tsx")
            || path.ends_with(".jsx")
            || path.starts_with("client/")
            || path.starts_with("ui/")
            || path.starts_with("frontend/");
        let candidates: Vec<_> = self
            .nodes
            .iter()
            .filter(|n| {
                matches!(n.role.as_str(), "api" | "application" | "ui") && within(path, &n.root)
            })
            .filter(|n| n.role != "ui" || web)
            .collect();
        let preferred: Vec<_> = candidates
            .iter()
            .filter(|n| n.root == root && (!web || n.role == "ui"))
            .collect();
        let best = if preferred.is_empty() {
            candidates.iter().collect::<Vec<_>>()
        } else {
            preferred
        };
        let depth = best.iter().map(|n| n.root.len()).max()?;
        let most: Vec<_> = best.into_iter().filter(|n| n.root.len() == depth).collect();
        if most.len() == 1 {
            Some(most[0].element.id.clone())
        } else {
            None
        }
    }
}
fn add_ref(refs: &mut Vec<Reference>, r: Reference) {
    if refs.len() < 60 && !refs.iter().any(|x| x.path == r.path && x.line == r.line) {
        refs.push(r);
    }
}

pub fn build(snapshot: &Snapshot) -> io::Result<Value> {
    let mut b = Builder {
        snapshot,
        nodes: vec![],
        edges: vec![],
        notices: vec![],
        scope: "読んだ設定とコードに記述された構成".into(),
    };
    compose::build(&mut b);
    code::build(&mut b);
    for node in &mut b.nodes {
        if node.root.is_empty() {
            continue;
        }
        for (path, text) in &snapshot.files {
            if !(snapshot::readme(path) || path.to_lowercase().contains("architecture")) {
                continue;
            }
            let Some(text) = text.as_str() else {
                continue;
            };
            if let Some((i, _)) = text.lines().enumerate().find(|(_, line)| {
                line.trim_start()
                    .trim_start_matches("- ")
                    .strip_prefix(&format!("`{}/`", node.root))
                    .is_some_and(|rest| {
                        let rest = rest.trim_start();
                        rest.starts_with(':')
                            || rest.starts_with('：')
                            || rest.starts_with("は")
                            || rest.starts_with("is ")
                    })
                    && ["プロトタイプ", "未使用", "旧構成", "legacy", "prototype"]
                        .iter()
                        .any(|word| line.contains(word))
            }) {
                node.element.parent = Some("reference".into());
                node.scope_note =
                    Some("説明資料では、別の試作・旧構成として記載されています。".into());
                add_ref(&mut node.refs, Reference::new(path, i + 1));
                break;
            }
        }
    }
    let role_counts = b
        .nodes
        .iter()
        .fold(BTreeMap::<String, usize>::new(), |mut counts, n| {
            *counts.entry(n.role.clone()).or_default() += 1;
            counts
        });
    for node in &mut b.nodes {
        if role_counts.get(&node.role) == Some(&1) && node.service.is_some() {
            node.element.title = role_info(&node.role).0.into();
        }
    }
    if b.nodes.len() > 48 || b.edges.len() > 160 {
        return Err(super::invalid(
            "構成が表示上限を超えています。対象を分けて確認してください。",
        ));
    }
    let mut elements: Vec<_> = b.nodes.iter().map(|n| n.element.clone()).collect();
    let groups = [
        ("client", "画面・端末"),
        ("application", "アプリの処理"),
        ("data", "データを扱う場所"),
        ("external", "接続するサービス"),
        ("reference", "別の構成・試作"),
    ];
    let boundaries: Vec<Value> = groups
        .iter()
        .filter(|(key, _)| {
            b.nodes
                .iter()
                .any(|n| n.element.parent.as_deref() == Some(key))
        })
        .map(|(key, title)| json!({"id":key,"title":title}))
        .collect();
    for g in &boundaries {
        elements.push(Element {
            id: g["id"].as_str().unwrap_or_default().into(),
            parent: None,
            kind: ElementKind::Boundary,
            title: g["title"].as_str().unwrap_or_default().into(),
            description: "役割ごとにまとめた分類。実際のサーバー配置を表すものではありません。"
                .into(),
            basis: Basis {
                kind: BasisKind::Inference,
                note: "表示上の分類".into(),
                source: None,
            },
        });
    }
    if let Some(source) = b.nodes.first().and_then(|n| n.element.basis.source.clone()) {
        architecture::Model {
            schema_version: 1,
            id: id(&snapshot.full_name),
            version: 1,
            title: snapshot.full_name.clone(),
            source,
            environment: Environment::Unknown,
            images: BTreeMap::new(),
            browser_version: None,
            elements,
            relations: b.edges.iter().map(|e| e.relation.clone()).collect(),
            limits: vec!["記述された構成であり、実行や配備は未確認".into()],
        }
        .validate()?;
    }
    let unread: Vec<_> = snapshot
        .inventory
        .iter()
        .filter(|e| {
            !snapshot.files.contains_key(&e.path)
                && snapshot::architecture_priority(&e.path).is_some()
        })
        .map(|e| &e.path)
        .collect();
    let explained: Vec<_> = b
        .nodes
        .iter()
        .flat_map(|n| &n.refs)
        .map(|r| &r.path)
        .collect();
    let unclassified: Vec<_> = snapshot
        .files
        .keys()
        .filter(|p| snapshot::source(p) && !snapshot::test_file(p) && !explained.contains(p))
        .collect();
    if !unread.is_empty() {
        b.notices.push(format!(
            "構成に関係する候補のうち、{}ファイルの本文は未読です。",
            unread.len()
        ));
    }
    if !unclassified.is_empty() {
        b.notices.push(format!(
            "読んだコードのうち、{}ファイルは役割や接続を特定できていません。",
            unclassified.len()
        ));
    }
    if b.nodes.is_empty() {
        b.notices.push("サービスや起動するアプリを特定できませんでした。説明・設定・対応外のコードを確認してください。".into());
    }
    let summary = format!(
        "{}個の構成要素と{}本の接続を、設定とコードの記述から整理しました。",
        b.nodes.len(),
        b.edges.len()
    );
    let model_revision = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(2, &snapshot.commit, &b.nodes, &b.edges))
                .map_err(io::Error::other)?
        )
    );
    Ok(
        json!({"schema_version":2,"model_id":id(&snapshot.full_name),"model_revision":model_revision,"repository":snapshot.full_name,"commit":snapshot.commit,"scope":b.scope,"summary":summary,"nodes":b.nodes,"edges":b.edges,"boundaries":boundaries,"notices":b.notices,"read_files":snapshot.files.len(),"unread_files":unread.len(),"unread_paths":unread,"unclassified_paths":unclassified,"inventory_truncated":snapshot.truncated,
        "basis":"設定のサービス定義と、API・保存・通信などの呼び出しを対応づけています。点線は設定・起動の依存、実線は要求やデータのやり取りの記述です。",
        "unknown":"役割ごとのまとまりを示しています。実際の配置、通信の成功、保存や認証の成立は、検証して確認します。"}),
    )
}
