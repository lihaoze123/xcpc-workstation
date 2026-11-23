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
                "未检测到 C++ 编译器（g++ 或 clang++）。请在「设置 → 工具链」指定编译器路径，或安装后重试。"
                    .to_owned()
            }
            JudgeError::CompilerNotFound(p) => {
                format!("指定的编译器路径不存在：「{p}」。请检查「设置 → 工具链」或 XCPC_CXX 配置。")
            }
            JudgeError::NoMainSolution(slug) => {
                format!("题目「{slug}」未声明标程。请在 problem.toml 中为标程添加 `main = true`。")
            }
            JudgeError::MultipleMainSolutions(slug, names) => {
                format!(
                    "题目「{slug}」声明了多个标程（{}），只能保留一个 `main = true`。",
                    names.join("、")
                )
            }
            JudgeError::UnknownChecker(name) => {
                format!("未知的内置校验器「{name}」。可选：wcmp、ncmp、fcmp、lcmp、rcmp4、rcmp6、rcmp9、yesno、nyesno 或 checker.cpp。")
            }
            JudgeError::TestNotFound(sel) => {
                format!("找不到测试点「{sel}」。请检查测试点编号或名称。")
            }
            JudgeError::GeneratorNotFound(name) => {
                format!("找不到生成器「{name}」（generators/{name}.cpp）。")
            }
            JudgeError::SolutionNotFound(name) => {
                format!("找不到解法「{name}」（solutions/{name}.cpp）。")
            }
            JudgeError::InvalidLimit { kind, value } => {
                format!("题目 {kind} 限制格式无效：「{value}」。示例：time = \"1s\"，memory = \"256MB\"。")
            }
            JudgeError::InvalidConfig(msg) => format!("题目包配置有误：{msg}"),
            JudgeError::Cancelled => "构建已取消。".to_owned(),
            JudgeError::Io { context, source } => format!("{context}：{source}"),
            JudgeError::Internal(msg) => format!("评测引擎内部错误：{msg}"),
        }
    }
}

pub(crate) fn io_err(context: impl Into<String>, source: std::io::Error) -> JudgeError {
    JudgeError::Io {
        context: context.into(),
        source,
    }
}
