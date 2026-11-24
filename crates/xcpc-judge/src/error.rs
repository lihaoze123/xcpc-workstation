//! Error types for `xcpc-judge`.

use thiserror::Error;

pub type Result<T, E = JudgeError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum JudgeError {
    #[error(transparent)]
    Storage(#[from] xcpc_storage::Error),

    #[error("no C++ compiler found (install g++ or clang++, or set XCPC_CXX)")]
    NoCompiler,

    #[error("compiler not found at {0:?}")]
    CompilerNotFound(String),

    #[error("problem {0:?} has no main solution (set `main = true` under `[solutions.<name>]`)")]
    NoMainSolution(String),

    #[error("problem {0:?} has multiple main solutions: {names}", names = .1.join(", "))]
    MultipleMainSolutions(String, Vec<String>),

    #[error("unknown checker {0:?}")]
    UnknownChecker(String),

    #[error("test case {0:?} not found")]
    TestNotFound(String),

    #[error("generator {0:?} not found")]
    GeneratorNotFound(String),

    #[error("solution {0:?} not found")]
    SolutionNotFound(String),

    #[error("invalid limit {kind} value {value:?}")]
    InvalidLimit { kind: &'static str, value: String },

    #[error("invalid package configuration: {0}")]
    InvalidConfig(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("internal judge error: {0}")]
    Internal(String),
}

impl JudgeError {
    /// Stable machine-readable error code for CLI `--json` and MCP.
    pub fn code(&self) -> &'static str {
        match self {
            JudgeError::Storage(e) => e.code(),
            JudgeError::NoCompiler => "no_compiler",
            JudgeError::CompilerNotFound(_) => "compiler_not_found",
            JudgeError::NoMainSolution(_) => "no_main_solution",
            JudgeError::MultipleMainSolutions(_, _) => "multiple_main_solutions",
            JudgeError::UnknownChecker(_) => "unknown_checker",
            JudgeError::TestNotFound(_) => "test_not_found",
            JudgeError::GeneratorNotFound(_) => "generator_not_found",
            JudgeError::SolutionNotFound(_) => "solution_not_found",
            JudgeError::InvalidLimit { .. } => "invalid_limit",
            JudgeError::InvalidConfig(_) => "invalid_config",
            JudgeError::Cancelled => "cancelled",
            JudgeError::Io { .. } => "io",
            JudgeError::Internal(_) => "internal",
        }
    }

    /// Concise human-readable Chinese message.
    pub fn human_message(&self) -> String {
        match self {
            JudgeError::Storage(e) => e.human_message(),
            JudgeError::NoCompiler => {
                "没有找到 C++ 编译器（g++ 或 clang++）。请安装后重试，或在「设置 → 工具链」中指定编译器路径。"
                    .to_owned()
            }
            JudgeError::CompilerNotFound(p) => {
                format!("指定的编译器不存在：「{p}」。请在「设置 → 工具链」中重新指定。")
            }
            JudgeError::NoMainSolution(slug) => {
                format!("题目「{slug}」还没有标程。请在解法中把一份解法设为标程。")
            }
            JudgeError::MultipleMainSolutions(slug, names) => {
                format!(
                    "题目「{slug}」有多份标程（{}），只能保留一份。",
                    names.join("、")
                )
            }
            JudgeError::UnknownChecker(name) => {
                format!("不认识的评测器「{name}」。可选内置评测器 wcmp、ncmp、fcmp、lcmp、rcmp4、rcmp6、rcmp9、yesno、nyesno，或自定义 checker.cpp。")
            }
            JudgeError::TestNotFound(sel) => {
                format!("找不到测试「{sel}」。请检查测试编号或名称。")
            }
            JudgeError::GeneratorNotFound(name) => {
                format!("找不到生成器「{name}」（generators/{name}.cpp）。")
            }
            JudgeError::SolutionNotFound(name) => {
                format!("找不到解法「{name}」（solutions/{name}.cpp）。")
            }
            JudgeError::InvalidLimit { kind, value } => {
                let kind = if *kind == "memory" { "内存" } else { "时间" };
                format!("题目的{kind}限制写法有误：「{value}」。示例：时间 1s，内存 256MB。")
            }
            JudgeError::InvalidConfig(msg) => format!("测试数据配置有误：{msg}"),
            JudgeError::Cancelled => "检查已取消。".to_owned(),
            JudgeError::Io { context, source } => format!("{context}：{source}"),
            JudgeError::Internal(msg) => format!("检查时出现内部错误：{msg}"),
        }
    }
}

pub(crate) fn io_err(context: impl Into<String>, source: std::io::Error) -> JudgeError {
    JudgeError::Io {
        context: context.into(),
        source,
    }
}
