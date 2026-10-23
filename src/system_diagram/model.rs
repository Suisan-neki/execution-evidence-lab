//! モデル出力を境界で検査し、既存の構成モデルと表示形式へ変換する。
use super::*;
use crate::architecture::{Basis, BasisKind, Element, ElementKind, Environment, Model, Relation};
use crate::workspace::CodeReference;
use std::collections::BTreeSet;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reference {
    path: String,
    line: u32,
    end_line: u32,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    title: String,
    purpose: String,
    workflow: String,
    operator_purpose: String,
    operator_workflow: String,
    refs: Vec<Reference>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Group {
    id: String,
    label: String,
    description: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Node {
    id: String,
    label: String,
    role: String,
    group_id: Option<String>,
    path: Option<String>,
    operator_description: String,
    technical_description: String,
    technology: Vec<String>,
    evidence: String,
    refs: Vec<Reference>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Edge {
    from: String,
    to: String,
    label: String,
    operator_description: String,
    technical_description: String,
    protocol: String,
    relation_type: String,
    evidence: String,
    refs: Vec<Reference>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    summary: Summary,
    groups: Vec<Group>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    unknowns: Vec<String>,
}
fn check_text(s: &str, max: usize) -> io::Result<()> {
    if s.trim().is_empty()
        || s.chars().count() > max
        || s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(invalid("説明が空、長すぎる、または制御文字を含んでいます"));
    }
    Ok(())
}
fn identifier(s: &str) -> io::Result<()> {
    if s.is_empty()
        || s.len() > 64
        || !s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        || !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(invalid("構成要素のIDが指定形式と一致しません"));
    }
    Ok(())
}
fn refs(refs: &[Reference], context: &context::Context, evidence: &str) -> io::Result<()> {
    if !matches!(evidence, "code" | "documentation" | "inference")
        || refs.len() > 8
        || evidence != "inference" && refs.is_empty()
    {
        return Err(invalid("根拠の種類と参照行が不足しています"));
    }
    for r in refs {
        let lines = context.provided.get(&r.path).ok_or_else(|| {
            invalid(format!(
                "AIに渡していないファイルを参照しています: {}",
                r.path
            ))
        })?;
        if r.line == 0
            || r.end_line < r.line
            || r.end_line.saturating_sub(r.line) > 79
            || !(r.line..=r.end_line).all(|l| lines.contains(&l))
        {
            return Err(invalid(format!(
                "AIに渡していない行範囲を参照しています: {}:{}-{}",
                r.path, r.line, r.end_line
            )));
        }
    }
    Ok(())
}
fn source(snapshot: &Value, reference: &Reference) -> CodeReference {
    CodeReference {
        repository: snapshot["fullName"].as_str().unwrap_or_default().into(),
        commit: snapshot["commit"].as_str().unwrap_or_default().into(),
        path: reference.path.clone(),
        line: reference.line,
        end_line: reference.end_line,
    }
}
fn basis(snapshot: &Value, references: &[Reference], evidence: &str) -> Basis {
    Basis {
        kind: BasisKind::Inference,
        note: match evidence {
            "code" => {
                "AIがコードの記述から整理。参照行の存在のみ照合済み。説明内容と動作は未検証。"
            }
            "documentation" => "AIが作者の説明資料から整理。実装との一致と動作は未検証。",
            _ => "AIによる推定。関係を示す直接の記述は未確認。",
        }
        .into(),
        source: references.first().map(|r| source(snapshot, r)),
    }
}
pub(super) fn compile(
    content: &str,
    snapshot: &Value,
    context: &context::Context,
) -> io::Result<Value> {
    let graph: Graph = serde_json::from_str(content)
        .map_err(|e| invalid(format!("構成データの形式が不正です: {e}")))?;
    if graph.nodes.is_empty()
        || graph.nodes.len() > 34
        || graph.edges.len() > 48
        || graph.groups.len() > 10
        || graph.unknowns.len() > 16
    {
        return Err(invalid("構成の数が上限と一致しません"));
    }
    check_text(&graph.summary.title, 120)?;
    check_text(&graph.summary.purpose, 1500)?;
    check_text(&graph.summary.workflow, 1500)?;
    check_text(&graph.summary.operator_purpose, 1500)?;
    check_text(&graph.summary.operator_workflow, 1500)?;
    refs(&graph.summary.refs, context, "documentation")?;
    for unknown in &graph.unknowns {
        check_text(unknown, 1000)?;
    }
    let mut ids = BTreeSet::new();
    let mut groups = BTreeMap::new();
    let mut elements = vec![];
    let mut nodes = vec![];
    let mut edges = vec![];
    let mut relations = vec![];
    for g in &graph.groups {
        identifier(&g.id)?;
        check_text(&g.label, 40)?;
        check_text(&g.description, 1000)?;
        if !ids.insert(g.id.clone()) {
            return Err(invalid("構成のIDが重複しています"));
        }
        let id = format!("g-{}", &digest(g.id.as_bytes())[..24]);
        groups.insert(g.id.clone(), id.clone());
        elements.push(Element {
            id,
            parent: None,
            kind: ElementKind::Boundary,
            title: g.label.clone(),
            description: g.description.clone(),
            basis: basis(snapshot, &[], "inference"),
        });
    }
    let mut mapped = BTreeMap::new();
    let mut labels = BTreeSet::new();
    for n in &graph.nodes {
        identifier(&n.id)?;
        check_text(&n.label, 40)?;
        if !labels.insert(n.label.trim().to_lowercase()) {
            return Err(invalid(
                "構成要素の役割名が重複しています。それぞれの機能を区別する名前にしてください",
            ));
        }
        check_text(&n.operator_description, 1000)?;
        check_text(&n.technical_description, 1000)?;
        refs(&n.refs, context, &n.evidence)?;
        if !ids.insert(n.id.clone()) {
            return Err(invalid("構成のIDが重複しています"));
        }
        if n.technology.len() > 8 {
            return Err(invalid("技術名が上限を超えています"));
        }
        for t in &n.technology {
            check_text(t, 100)?;
        }
        let kind = match n.role.as_str() {
            "actor" => ElementKind::Person,
            "ui" | "api" | "application" => ElementKind::App,
            "database" | "cache" => ElementKind::Database,
            "storage" => ElementKind::Storage,
            "auth" | "external" => ElementKind::System,
            "component" | "queue" => ElementKind::Component,
            _ => return Err(invalid("構成要素のroleが指定形式と一致しません")),
        };
        let parent = n
            .group_id
            .as_ref()
            .map(|g| {
                groups
                    .get(g)
                    .cloned()
                    .ok_or_else(|| invalid("存在しないgroupを参照しています"))
            })
            .transpose()?;
        if let Some(path) = &n.path {
            if !snapshot["inventory"].as_array().is_some_and(|entries| {
                entries.iter().any(|e| {
                    e["path"].as_str().is_some_and(|p| {
                        p == path || p.strip_prefix(path).is_some_and(|s| s.starts_with('/'))
                    })
                })
            }) {
                return Err(invalid(format!("存在しない実装のパスです: {path}")));
            }
            if n.evidence == "code"
                && !n.refs.iter().any(|r| {
                    r.path == *path
                        || r.path
                            .strip_prefix(path)
                            .is_some_and(|s| s.starts_with('/'))
                })
            {
                return Err(invalid("実装のパスに対応するコードの参照がありません"));
            }
        } else if !matches!(n.role.as_str(), "actor" | "external" | "auth") {
            return Err(invalid("内部の構成要素に実装のパスがありません"));
        }
        let id = format!(
            "n-{}",
            &digest(format!("{}:{}", n.path.as_deref().unwrap_or("external"), n.id).as_bytes())
                [..24]
        );
        mapped.insert(n.id.clone(), id.clone());
        let element = Element {
            id: id.clone(),
            parent,
            kind,
            title: n.label.clone(),
            description: n.operator_description.clone(),
            basis: basis(snapshot, &n.refs, &n.evidence),
        };
        let mut value = serde_json::to_value(&element).map_err(io::Error::other)?;
        value["role"] = json!(n.role);
        value["technology"] = json!(n.technology);
        value["technical_title"] = json!(n.technology.join(" / "));
        value["technical_description"] = json!(n.technical_description);
        value["path"] = json!(n.path);
        value["refs"] = serde_json::to_value(&n.refs).map_err(io::Error::other)?;
        value["evidence_kind"] = json!(n.evidence);
        nodes.push(value);
        elements.push(element);
    }
    let mut pairs = BTreeSet::new();
    for e in &graph.edges {
        let from = mapped
            .get(&e.from)
            .ok_or_else(|| invalid("接続元の構成要素がありません"))?;
        let to = mapped
            .get(&e.to)
            .ok_or_else(|| invalid("接続先の構成要素がありません"))?;
        if from == to || !pairs.insert((from.clone(), to.clone())) {
            return Err(invalid("自己接続または同じ方向の接続が重複しています"));
        }
        check_text(&e.label, 40)?;
        check_text(&e.operator_description, 1000)?;
        check_text(&e.technical_description, 1000)?;
        check_text(&e.protocol, 120)?;
        refs(&e.refs, context, &e.evidence)?;
        if !matches!(
            e.relation_type.as_str(),
            "request" | "data" | "call" | "event" | "startup" | "configuration"
        ) {
            return Err(invalid("接続の種類が指定形式と一致しません"));
        }
        let relation = Relation {
            id: format!("e-{}", &digest(format!("{from}:{to}").as_bytes())[..24]),
            from: from.clone(),
            to: to.clone(),
            title: e.label.clone(),
            description: e.operator_description.clone(),
            basis: basis(snapshot, &e.refs, &e.evidence),
        };
        let mut value = serde_json::to_value(&relation).map_err(io::Error::other)?;
        value["technical_description"] = json!(e.technical_description);
        value["protocol"] = json!(e.protocol);
        value["relation_type"] = json!(e.relation_type);
        value["evidence_kind"] = json!(e.evidence);
        value["refs"] = serde_json::to_value(&e.refs).map_err(io::Error::other)?;
        edges.push(value);
        relations.push(relation);
    }
    let model_id = format!(
        "ai-{}",
        &digest(snapshot["fullName"].as_str().unwrap_or_default().as_bytes())[..24]
    );
    Model {
        schema_version: 1,
        id: model_id.clone(),
        version: 1,
        title: graph.summary.title.clone(),
        source: source(snapshot, &graph.summary.refs[0]),
        environment: Environment::Unknown,
        images: BTreeMap::new(),
        browser_version: None,
        elements,
        relations,
        limits: graph.unknowns.clone(),
    }
    .validate()?;
    let revision = digest(
        serde_json::to_vec(&(
            PROMPT_VERSION,
            crate::writing::metadata(),
            &context.hash,
            &graph,
        ))
        .map_err(io::Error::other)?
        .as_slice(),
    );
    let boundaries: Vec<_> = graph
        .groups
        .iter()
        .map(|g| json!({"id":groups[&g.id],"title":g.label,"description":g.description}))
        .collect();
    Ok(
        json!({"schema_version":3,"generator":"gitdiagram-inspired-ai","model_id":model_id,"model_revision":revision,"repository":snapshot["fullName"],"commit":snapshot["commit"],"nodes":nodes,"edges":edges,"boundaries":boundaries,"overview":graph.summary,"scope":"READMEとコードの抜粋からAIが整理した構成","summary":format!("{}個の構成要素と{}本の接続",graph.nodes.len(),graph.edges.len()),"basis":"参照したファイルと行は照合済みです。説明と接続の内容はAIの解釈であり、コードに記述された動作が実際に成立するかは未検証です。","unknown":"READMEの説明、コードの記述、根拠が不足する推定を区別しています。実際の配備・通信・保存の成立は検証して確認します。","notices":graph.unknowns,"read_files":context.read_files,"excerpted_files":context.excerpted_files,"inventory_files":context.inventory_files,"unread_files":context.inventory_files.saturating_sub(context.read_files),"inventory_truncated":context.truncated,"context_sha256":context.hash,"prompt_version":PROMPT_VERSION,"writing_policy":crate::writing::metadata()}),
    )
}
