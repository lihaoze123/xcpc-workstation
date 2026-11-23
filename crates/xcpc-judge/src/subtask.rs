//! OI subtask validation (DAG / cycle / assignment checks) and scoring (`min` / `sum` + `quitp`).

use std::collections::{BTreeMap, HashMap, HashSet};

use xcpc_domain::{SubtaskScoring, SubtaskSpec};

use crate::package::{DiscoveredSolution, ExpandedTest};
use crate::report::{MatrixCell, SolutionSubtaskScore, SubtaskSummary};
use crate::runner::Verdict;

/// Result of statically validating `[[subtask]]` definitions against expanded tests.
#[derive(Debug, Clone, Default)]
pub struct SubtaskValidation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub summaries: Vec<SubtaskSummary>,
    pub total_score: u32,
    pub transitive_deps: BTreeMap<String, Vec<String>>,
}

/// Validates `[[subtask]]` configuration and test assignments.
pub fn validate_subtask_config(
    subtasks: &[SubtaskSpec],
    tests: &[ExpandedTest],
) -> SubtaskValidation {
    let mut out = SubtaskValidation::default();

    if subtasks.is_empty() {
        for t in tests {
            if let Some(ref sid) = t.subtask {
                out.errors.push(format!(
                    "测试点 #{} ({}) 指定了未知子任务「{sid}」（未声明 [[subtask]]）",
                    t.index, t.name
                ));
            }
        }
        return out;
    }

    let mut id_set: HashSet<&str> = HashSet::new();
    let mut by_id: HashMap<&str, &SubtaskSpec> = HashMap::new();
    for st in subtasks {
        let id = st.id.trim();
        if id.is_empty() {
            out.errors.push("存在空的 [[subtask]].id".to_owned());
            continue;
        }
        if !id_set.insert(id) {
            out.errors.push(format!("存在重复的子任务 ID「{id}」"));
            continue;
        }
        by_id.insert(id, st);
        out.total_score = out.total_score.saturating_add(st.score);
    }

    // Check unknown dependencies and self-dependencies.
    for st in subtasks {
        let id = st.id.trim();
        for dep in &st.depends {
            let dep = dep.trim();
            if dep == id {
                out.errors.push(format!("子任务「{id}」不能依赖自身"));
            } else if !id_set.contains(dep) {
                out.errors
                    .push(format!("子任务「{id}」依赖了未知子任务「{dep}」"));
            }
        }
    }

    // Cycle detection via 3-color DFS (0 = unvisited, 1 = visiting, 2 = visited).
    let mut color: HashMap<&str, u8> = HashMap::new();
    let mut has_cycle = false;
    for st in subtasks {
        let id = st.id.trim();
        if id.is_empty() || color.get(id).copied().unwrap_or(0) != 0 {
            continue;
        }
        if dfs_has_cycle(id, &by_id, &mut color) {
            has_cycle = true;
            break;
        }
    }
    if has_cycle {
        out.errors
            .push("子任务 depends 存在循环依赖（必须构成有向无环图 DAG）".to_owned());
    } else {
        // Compute transitive dependencies for each subtask.
        for st in subtasks {
            let id = st.id.trim();
            if id.is_empty() {
                continue;
            }
            let mut visited: HashSet<String> = HashSet::new();
            let mut stack: Vec<&str> = st.depends.iter().map(String::as_str).collect();
            while let Some(d) = stack.pop() {
                let d = d.trim();
                if d.is_empty() || d == id || !by_id.contains_key(d) {
                    continue;
                }
                if visited.insert(d.to_owned())
                    && let Some(dep_spec) = by_id.get(d)
                {
                    for next in &dep_spec.depends {
                        stack.push(next.as_str());
                    }
                }
            }
            let mut deps_vec: Vec<String> = visited.into_iter().collect();
            deps_vec.sort();
            out.transitive_deps.insert(id.to_owned(), deps_vec);
        }
    }

    // Assign tests to subtasks.
    let mut tests_by_subtask: HashMap<&str, Vec<usize>> = HashMap::new();
    for t in tests {
        match t
            .subtask
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(sid) => {
                if id_set.contains(sid) {
                    tests_by_subtask.entry(sid).or_default().push(t.index);
                } else {
                    out.errors.push(format!(
                        "测试点 #{} ({}) 引用了未知子任务「{sid}」",
                        t.index, t.name
                    ));
                }
            }
            None if !t.is_sample => {
                out.errors.push(format!(
                    "非样例测试点 #{} ({}) 未归属任何子任务（请设置 subtask）",
                    t.index, t.name
                ));
            }
            None => {}
        }
    }

    // Build summaries and check for empty subtasks.
    for st in subtasks {
        let id = st.id.trim();
        let indices = tests_by_subtask.remove(id).unwrap_or_default();
        if indices.is_empty() {
            out.errors
                .push(format!("子任务「{id}」为空（包含 0 个测试点）"));
        }
        out.summaries.push(SubtaskSummary {
            id: id.to_owned(),
            score: st.score,
            scoring: st.scoring,
            depends: st.depends.clone(),
            test_count: indices.len(),
            test_indices: indices,
        });
    }

    if out.total_score != 100 {
        out.warnings.push(format!(
            "子任务分值之和为 {}（通常为 100）",
            out.total_score
        ));
    }

    out
}

fn dfs_has_cycle<'a>(
    node: &'a str,
    by_id: &HashMap<&'a str, &'a SubtaskSpec>,
    color: &mut HashMap<&'a str, u8>,
) -> bool {
    color.insert(node, 1);
    if let Some(spec) = by_id.get(node) {
        for dep in &spec.depends {
            let dep = dep.trim();
            if !by_id.contains_key(dep) {
                continue;
            }
            match color.get(dep).copied().unwrap_or(0) {
                1 => return true,
                0 if dfs_has_cycle(dep, by_id, color) => return true,
                _ => {}
            }
        }
    }
    color.insert(node, 2);
    false
}

/// Evaluates one solution's subtask scores, total score, and whether its `score` / `subtasks`
/// assertions pass.
pub struct SolutionSubtaskEval {
    pub total_score: f64,
    pub max_score: u32,
    pub subtask_scores: Vec<SolutionSubtaskScore>,
    pub assertions_matched: bool,
    pub failure_reasons: Vec<String>,
}

pub fn evaluate_solution_subtasks(
    validation: &SubtaskValidation,
    sol: &DiscoveredSolution,
    cells_by_test: &HashMap<usize, &MatrixCell>,
    invalid_test_indices: &HashSet<usize>,
) -> SolutionSubtaskEval {
    const EPS: f64 = 1e-9;

    let summary_by_id: HashMap<&str, &SubtaskSummary> = validation
        .summaries
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();

    let cell_ratio = |test_idx: usize| -> (Verdict, f64) {
        if !sol.is_main && invalid_test_indices.contains(&test_idx) {
            return (Verdict::JE, 0.0);
        }
        match cells_by_test.get(&test_idx) {
            Some(c) => {
                if c.verdict == Verdict::AC {
                    let r = c.score_ratio.unwrap_or(1.0).clamp(0.0, 1.0);
                    (Verdict::AC, r)
                } else {
                    (c.verdict, 0.0)
                }
            }
            None => (Verdict::JE, 0.0),
        }
    };

    let mut subtask_scores = Vec::with_capacity(validation.summaries.len());
    let mut total_score = 0.0f64;
    let mut assertions_matched = true;
    let mut failure_reasons = Vec::new();

    for summary in &validation.summaries {
        let deps = validation
            .transitive_deps
            .get(&summary.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        // Check transitive dependencies first.
        let mut deps_all_full = true;
        let mut deps_min_ratio = 1.0f64;
        let mut first_dep_fail_verdict: Option<Verdict> = None;

        for dep_id in deps {
            if let Some(dep_sum) = summary_by_id.get(dep_id.as_str()) {
                for &ti in &dep_sum.test_indices {
                    let (v, r) = cell_ratio(ti);
                    if v != Verdict::AC || r < 1.0 - EPS {
                        deps_all_full = false;
                    }
                    if v != Verdict::AC && first_dep_fail_verdict.is_none() {
                        first_dep_fail_verdict = Some(v);
                    }
                    deps_min_ratio = deps_min_ratio.min(r);
                }
            }
        }

        let mut owned_first_fail: Option<Verdict> = None;
        let mut owned_min_ratio = 1.0f64;
        let mut owned_ratio_sum = 0.0f64;
        let mut owned_all_full = !summary.test_indices.is_empty();

        for &ti in &summary.test_indices {
            let (v, r) = cell_ratio(ti);
            if v != Verdict::AC {
                owned_all_full = false;
                if owned_first_fail.is_none() {
                    owned_first_fail = Some(v);
                }
            } else if r < 1.0 - EPS {
                owned_all_full = false;
            }
            owned_min_ratio = owned_min_ratio.min(r);
            owned_ratio_sum += r;
        }

        let (score, full_score) = if summary.test_indices.is_empty() {
            (0.0, false)
        } else {
            match summary.scoring {
                SubtaskScoring::Min => {
                    let combined_min = owned_min_ratio.min(deps_min_ratio);
                    if combined_min <= EPS
                        || owned_first_fail.is_some()
                        || first_dep_fail_verdict.is_some()
                    {
                        (0.0, false)
                    } else {
                        let s = f64::from(summary.score) * combined_min;
                        (round_score(s), combined_min >= 1.0 - EPS)
                    }
                }
                SubtaskScoring::Sum => {
                    if !deps_all_full {
                        (0.0, false)
                    } else {
                        let per_test =
                            f64::from(summary.score) / (summary.test_indices.len() as f64);
                        let s = per_test * owned_ratio_sum;
                        (round_score(s), owned_all_full)
                    }
                }
            }
        };

        let verdict = owned_first_fail
            .or(first_dep_fail_verdict)
            .unwrap_or(Verdict::AC);

        let matched = sol.subtasks.get(&summary.id).map(|exp| {
            let ok = exp.matches(verdict.as_str());
            if !ok {
                assertions_matched = false;
                failure_reasons.push(format!(
                    "subtask `{}` expected {:?}, got {}",
                    summary.id,
                    exp.normalized(),
                    verdict
                ));
            }
            ok
        });

        total_score += score;
        subtask_scores.push(SolutionSubtaskScore {
            subtask_id: summary.id.clone(),
            max_score: summary.score,
            score,
            verdict,
            full_score,
            matched,
        });
    }

    total_score = round_score(total_score);

    // Check for any subtask assertions referencing unknown subtask IDs.
    for req_id in sol.subtasks.keys() {
        if !summary_by_id.contains_key(req_id.as_str()) {
            assertions_matched = false;
            failure_reasons.push(format!(
                "subtasks assertion references unknown subtask `{req_id}`"
            ));
        }
    }

    // Main solution must obtain full score across all subtasks.
    if sol.is_main && (total_score - f64::from(validation.total_score)).abs() > 1e-6 {
        assertions_matched = false;
        failure_reasons.push(format!(
            "main solution scored {total_score}/{} (must be full score)",
            validation.total_score
        ));
    }

    // Optional `score` assertion (exact or `[min, max]`).
    if let Some(score_exp) = sol.score
        && !score_exp.matches(total_score)
    {
        assertions_matched = false;
        failure_reasons.push(format!("expected score {score_exp:?}, got {total_score}"));
    }

    SolutionSubtaskEval {
        total_score,
        max_score: validation.total_score,
        subtask_scores,
        assertions_matched,
        failure_reasons,
    }
}

fn round_score(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}
