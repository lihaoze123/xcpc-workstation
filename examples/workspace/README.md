# 示例工作区

用来试用 XCPC Workstation 的工作区。

```sh
# 桌面端（在开发环境里）
cd apps/desktop && XCPC_WORKSPACE=../../examples/workspace nsh dev

# 命令行
xcpc -w examples/workspace list
xcpc -w examples/workspace build contest spring-training
xcpc -w examples/workspace check problem draft-problem
```

- `spring-training`：中文比赛，带封面、留白页，C 题覆写了题名、时限并隐藏限制表。
- `open-cup`：英文比赛，一道题，自定义输入输出文件名。
- `archived-2025`：已冻结的比赛，从 `snapshot/` 渲染，编辑器只读。
- `draft-problem`：故意留了一个公式错误，看诊断怎么显示。

导出的 PDF 在 `build/`（已在 `.gitignore` 中）。
