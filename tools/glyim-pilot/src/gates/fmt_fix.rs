use crate::error::PilotError;
use crate::gates::helpers::run_gate_command;
use crate::gates::types::{GateContext, GateResult, GateSideEffect};

/// `run_fmt_fix`.
pub async fn run_fmt_fix(ctx: &GateContext) -> Result<GateResult, PilotError> {
    let output = run_gate_command(
        "cargo",
        &["fmt"],
        &ctx.worktree_dir,
        ctx.timeout_secs,
        "fmt_fix",
    )
    .await?;
    if !output.status.success() {
        let stderr = crate::gates::helpers::strip_ansi(&String::from_utf8_lossy(&output.stderr));
        return Ok(GateResult::fail_with_details(
            "fmt_fix",
            "cargo fmt failed to apply formatting",
            stderr,
        ));
    }
    let changed_files_result =
        crate::git_ops::diff_name_only(&ctx.worktree_dir, &ctx.default_branch, ctx.timeout_secs)
            .await;
    let changed_files: Vec<String> = match changed_files_result {
        Ok(output) => output
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(e) => {
            tracing::warn!("fmt_fix: could not get changed files: {e}");
            Vec::new()
        }
    };
    Ok(GateResult::pass_with_side_effects(
        "fmt_fix",
        "auto-fixed: cargo fmt applied changes (not committed)",
        format!("Changed files:\n{}", changed_files.join("\n")),
        vec![GateSideEffect {
            description: "auto-fixed formatting via cargo fmt".into(),
            affected_files: changed_files,
        }],
    ))
}
